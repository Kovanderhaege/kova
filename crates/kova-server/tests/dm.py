"""Private-message HTTP, persistence and Rust-client integration checks."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "client"))
from identity import Ed25519PrivateKey, authorize, digest, public
from compose_instance import instance


def main():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    compose, query, ready = instance(str(port))
    origin = f"http://127.0.0.1:{port}"

    def call(path, expected, token=None, data=None, method="GET", source=origin):
        headers = {"Origin": source}
        if token:
            headers["Authorization"] = "Bearer " + token
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = Request(
            origin + "/api/v1" + path,
            method=method,
            headers=headers,
            data=None if data is None else json.dumps(data).encode(),
        )
        try:
            response = urlopen(request, timeout=15)
        except HTTPError as error:
            response = error
        with response:
            raw = response.read()
            assert response.status == expected, (path, response.status, raw)
            assert response.headers["Cache-Control"] == "no-store"
            return json.loads(raw) if raw else None

    def login(person, invite=None, handle="guest"):
        root, device, certificate = person
        identity = "kova1-" + digest(certificate["root_public_key"])
        challenge = call("/auth/challenges", 201, data={
            "global_identity_id": identity,
            "device_id": digest(certificate["device_public_key"]),
            "operation": "join" if invite else "login",
            "invite": invite,
            "session_mode": "bearer",
        }, method="POST")
        proof = dict(
            certificate,
            challenge_id=challenge["challenge_id"],
            invite=invite,
            signature=device.sign(challenge["challenge"].encode()).hex(),
        )
        if invite:
            proof["profile"] = {"handle": handle, "display_name": handle}
        status = 201 if invite else 200
        return call("/auth/verify", status, data=proof, method="POST")["access_token"]

    def signed_bundle(person, key_package):
        root, device, certificate = person
        bundle = {
            "global_identity_id": "kova1-" + digest(certificate["root_public_key"]),
            "device_id": digest(certificate["device_public_key"]),
            "root_public_key": certificate["root_public_key"],
            "device_public_key": certificate["device_public_key"],
            "issued_at": certificate["device_authorization"]["issued_at"],
            "expires_at": certificate["device_authorization"]["expires_at"],
            "authorization_signature": certificate["device_authorization"]["signature"],
            "key_package": key_package,
            "signature": "",
        }
        payload = (
            "kova-dm-device-v1", bundle["global_identity_id"], bundle["device_id"],
            bundle["root_public_key"], bundle["device_public_key"], bundle["issued_at"],
            bundle["expires_at"], bundle["authorization_signature"], key_package,
        )
        bundle["signature"] = device.sign(json.dumps(payload, separators=(",", ":")).encode()).hex()
        return bundle

    def signed_conversation(alice, creator, recipient):
        manifest = {
            "conversation_id": str(uuid4()), "origin": origin,
            "creator": creator, "recipient": recipient, "welcome": "a1", "signature": "",
        }
        payload = (
            "kova-dm-conversation-v1", manifest["conversation_id"], origin,
            creator, recipient, manifest["welcome"],
        )
        manifest["signature"] = alice[1].sign(json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode()).hex()
        return manifest

    def signed_event(person, manifest, event_id, parents, ciphertext):
        event = {
            "conversation_id": manifest["conversation_id"], "event_id": event_id,
            "author_identity": manifest["creator"]["global_identity_id"],
            "author_device": manifest["creator"]["device_id"], "parents": sorted(parents),
            "ciphertext": ciphertext, "signature": "",
        }
        payload = (
            "kova-dm-event-v1", event["conversation_id"], event["event_id"],
            event["author_identity"], event["author_device"], event["parents"], ciphertext,
        )
        event["signature"] = person[1].sign(json.dumps(payload, separators=(",", ":")).encode()).hex()
        return event

    try:
        compose("up", "-d", "--build", "--wait")
        ready(200)
        alice = (Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate(), None)
        alice = (alice[0], alice[1], authorize(alice[0], alice[1]))
        compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "dm_owner", "DM owner", public(alice[0]))
        token = login(alice)
        community = call("/communities", 201, token, {"name": "DM test"}, "POST")["id"]

        def invite():
            result = call(f"/communities/{community}/invites", 201, token, {"expires_in": 3600}, "POST")
            return result["invite"]

        alice_invite, bob_invite, carol_invite = invite(), invite(), invite()
        bob = (Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate(), None)
        bob = (bob[0], bob[1], authorize(bob[0], bob[1]))
        carol = (Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate(), None)
        carol = (carol[0], carol[1], authorize(carol[0], carol[1]))
        bob_token, carol_token = login(bob, bob_invite, "dm_bob"), login(carol, carol_invite, "dm_carol")

        bob_bundle = signed_bundle(bob, (b"bob-keypackage" * 100).hex())
        alice_bundle = signed_bundle(alice, (b"alice-keypackage" * 100).hex())
        carol_bundle = signed_bundle(carol, (b"carol-keypackage" * 100).hex())
        call("/dm/key-packages", 401, data=bob_bundle, method="POST")
        call(f"/dm/key-packages/{bob_bundle['global_identity_id']}/{bob_bundle['device_id']}", 401)
        call("/dm/key-packages", 403, bob_token, bob_bundle, "POST", "https://attacker.example")
        call("/dm/key-packages", 400, bob_token, {**alice_bundle, "signature": bob_bundle["signature"]}, "POST")
        call("/dm/key-packages", 400, bob_token, {**bob_bundle, "key_package": "00" * 100}, "POST")
        stale = dict(bob_bundle)
        stale["issued_at"] = int(time.time()) - 90_000
        stale["expires_at"] = stale["issued_at"] + 86_400
        auth_payload = f"kova-device-v2\n{stale['global_identity_id']}\n{stale['device_id']}\n{stale['device_public_key']}\n{stale['issued_at']}\n{stale['expires_at']}"
        stale["authorization_signature"] = bob[0].sign(auth_payload.encode()).hex()
        stale_payload = (
            "kova-dm-device-v1", stale["global_identity_id"], stale["device_id"],
            stale["root_public_key"], stale["device_public_key"], stale["issued_at"],
            stale["expires_at"], stale["authorization_signature"], stale["key_package"],
        )
        stale["signature"] = bob[1].sign(json.dumps(stale_payload, separators=(",", ":")).encode()).hex()
        call("/dm/key-packages", 400, bob_token, stale, "POST")
        call("/dm/key-packages", 200, bob_token, bob_bundle, "POST")
        call("/dm/key-packages", 200, token, alice_bundle, "POST")
        assert call(f"/dm/key-packages/{bob_bundle['global_identity_id']}/{bob_bundle['device_id']}", 200, token) == bob_bundle

        # Creation requires a valid signed manifest from this session's exact device.
        carol_manifest = signed_conversation(carol, carol_bundle, bob_bundle)
        call("/dm/conversations", 400, token, carol_manifest, "POST")
        manifest = signed_conversation(alice, alice_bundle, bob_bundle)
        tampered = {**manifest, "welcome": "b2"}
        call("/dm/conversations", 400, token, tampered, "POST")
        created = call("/dm/conversations", 200, token, manifest, "POST")
        assert created == manifest
        assert call("/dm/conversations", 200, token, manifest, "POST") == manifest
        call("/dm/key-packages", 409, bob_token, bob_bundle, "POST")
        call(f"/dm/conversations/{manifest['conversation_id']}", 404, carol_token)
        assert call(f"/dm/conversations/{manifest['conversation_id']}", 200, bob_token) == manifest

        # Retries racing on a fresh one-use package converge on one conversation.
        next_bob_bundle = signed_bundle(bob, (b"bob-next-keypackage" * 100).hex())
        call("/dm/key-packages", 200, bob_token, next_bob_bundle, "POST")
        second_manifest = signed_conversation(alice, alice_bundle, next_bob_bundle)
        with ThreadPoolExecutor(2) as workers:
            results = list(workers.map(lambda _: call("/dm/conversations", 200, token, second_manifest, "POST"), range(2)))
        assert results == [second_manifest, second_manifest]

        missing = signed_event(alice, manifest, str(uuid4()), [str(uuid4())], "aa")
        call(f"/dm/conversations/{manifest['conversation_id']}/events", 409, token, missing, "POST")
        first = signed_event(alice, manifest, str(uuid4()), [], "bb")
        event_path = f"/dm/conversations/{manifest['conversation_id']}/events"
        with ThreadPoolExecutor(2) as workers:
            results = list(workers.map(lambda _: call(event_path, 200, token, first, "POST"), range(2)))
        assert [item["cursor"] for item in results] == [1, 1]
        call(event_path, 400, token, {**first, "ciphertext": "cc"}, "POST")
        conflicting = {**first, "ciphertext": "cc"}
        conflicting["signature"] = alice[1].sign(json.dumps((
            "kova-dm-event-v1", conflicting["conversation_id"], conflicting["event_id"],
            conflicting["author_identity"], conflicting["author_device"], conflicting["parents"], conflicting["ciphertext"],
        ), separators=(",", ":")).encode()).hex()
        call(event_path, 409, token, conflicting, "POST")
        second = signed_event(alice, manifest, str(uuid4()), [first["event_id"]], "cc")
        assert call(event_path, 200, token, second, "POST")["cursor"] == 2
        assert len(call(event_path + "?after=0&limit=1", 200, token)["items"]) == 1
        call(event_path + "?after=0&limit=101", 400, token)
        call(event_path, 400, carol_token, first, "POST")

        # The other participant can revoke its own device; that blocks new sends,
        # while the surviving participant can still read ciphertext history.
        revoke_payload = f"kova-revoke-v1\n{bob_bundle['global_identity_id']}\n{bob_bundle['device_id']}"
        call("/identity/devices/revoke", 200, bob_token, {
            "device_public_key": bob_bundle["device_public_key"],
            "signature": bob[0].sign(revoke_payload.encode()).hex(),
        }, "POST")
        third = signed_event(alice, manifest, str(uuid4()), [second["event_id"]], "dd")
        call(event_path, 403, token, third, "POST")
        assert len(call(event_path, 200, token)["items"]) == 2

        rust_alice_invite, rust_bob_invite = invite(), invite()
        if os.environ.get("KOVA_TEST_RUST_CLIENT") == "1":
            env = dict(
                os.environ,
                KOVA_TEST_ORIGIN=origin,
                KOVA_DM_INVITE_ALICE=rust_alice_invite,
                KOVA_DM_INVITE_BOB=rust_bob_invite,
            )
            subprocess.run(
                ["cargo", "test", "-p", "kova-client-core", "--locked", "--test", "dm_live", "--", "--ignored", "--nocapture"],
                env=env,
                cwd=Path(__file__).resolve().parents[3],
                check=True,
                timeout=600,
            )
        assert int(query("SELECT count(*) FROM dm_conversations")) >= 1
        assert int(query("SELECT count(*) FROM dm_events")) >= 2
        assert query(f"SELECT count(*) FROM dm_key_packages WHERE identity_id='{bob_bundle['global_identity_id']}' AND device_id='{bob_bundle['device_id']}'") == "0"
        assert int(query("SELECT count(*) FROM dm_used_key_packages")) >= 1
        for plaintext in ("secret-alice", "secret-bob"):
            assert query(f"SELECT count(*) FROM dm_events WHERE event::text LIKE '%{plaintext}%' ") == "0"
        compose("restart", "api")
        ready(200)
        assert int(query("SELECT count(*) FROM dm_events")) >= 2
        print("MP OK : enveloppes MLS opaque, accès appareil, messages hors ligne, reprises et persistance")
    finally:
        compose("down", "-v", "--remove-orphans")


if __name__ == "__main__":
    main()

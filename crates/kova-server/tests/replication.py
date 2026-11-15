"""Bilateral DM replica policy, authenticated synchronization and durable convergence."""
import json
from hashlib import sha256
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from uuid import uuid4

from compose_instance import COMPOSE

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "client"))
from identity import Ed25519PrivateKey, authorize, digest, public

ROOT = Path(__file__).resolve().parents[3]


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def call(origin, path, expected, token=None, data=None, method="GET", source=None):
    headers = {"Origin": source or origin}
    if token:
        headers["Authorization"] = "Bearer " + token
    if data is not None:
        headers["Content-Type"] = "application/json"
    request = Request(origin + "/api/v1" + path, method=method, headers=headers,
                      data=None if data is None else json.dumps(data).encode())
    try:
        response = urlopen(request, timeout=10)
    except HTTPError as error:
        response = error
    with response:
        raw = response.read()
        accepted = expected if isinstance(expected, tuple) else (expected,)
        assert response.status in accepted, (origin, path, response.status, raw)
        assert response.headers["Cache-Control"] == "no-store"
        return json.loads(raw) if raw else None


def login(origin, person, invite=None, handle="guest"):
    root, device, certificate = person
    identity = "kova1-" + digest(certificate["root_public_key"])
    challenge = call(origin, "/auth/challenges", 201, data={
        "global_identity_id": identity,
        "device_id": digest(certificate["device_public_key"]),
        "operation": "join" if invite else "login",
        "invite": invite,
        "session_mode": "bearer",
    }, method="POST")
    proof = dict(certificate, challenge_id=challenge["challenge_id"], invite=invite,
                 signature=device.sign(challenge["challenge"].encode()).hex())
    if invite:
        proof["profile"] = {"handle": handle, "display_name": handle}
    return call(origin, "/auth/verify", 201 if invite else 200,
                data=proof, method="POST")["access_token"]


def identity():
    root, device = Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate()
    return root, device, authorize(root, device)


def signed_bundle(person, package):
    root, device, cert = person
    bundle = {
        "global_identity_id": "kova1-" + digest(cert["root_public_key"]),
        "device_id": digest(cert["device_public_key"]),
        "root_public_key": cert["root_public_key"],
        "device_public_key": cert["device_public_key"],
        "issued_at": cert["device_authorization"]["issued_at"],
        "expires_at": cert["device_authorization"]["expires_at"],
        "authorization_signature": cert["device_authorization"]["signature"],
        "key_package": package.hex(),
        "signature": "",
    }
    payload = (
        "kova-dm-device-v1", bundle["global_identity_id"], bundle["device_id"],
        bundle["root_public_key"], bundle["device_public_key"], bundle["issued_at"],
        bundle["expires_at"], bundle["authorization_signature"], bundle["key_package"],
    )
    bundle["signature"] = device.sign(json.dumps(payload, separators=(",", ":")).encode()).hex()
    return bundle


def signed_conversation(alice, creator, recipient, origin):
    manifest = {"conversation_id": str(uuid4()), "origin": origin,
                "creator": creator, "recipient": recipient,
                "welcome": "a1", "signature": ""}
    payload = ("kova-dm-conversation-v1", manifest["conversation_id"], origin,
               creator, recipient, manifest["welcome"])
    manifest["signature"] = alice[1].sign(
        json.dumps(payload, separators=(",", ":")).encode()).hex()
    return manifest


def signed_event(person, manifest, event_id, parents, ciphertext):
    event = {
        "conversation_id": manifest["conversation_id"], "event_id": event_id,
        "author_identity": "kova1-" + digest(person[2]["root_public_key"]),
        "author_device": digest(person[2]["device_public_key"]),
        "parents": sorted(parents), "ciphertext": ciphertext, "signature": "",
    }
    payload = ("kova-dm-event-v1", event["conversation_id"], event["event_id"],
               event["author_identity"], event["author_device"], event["parents"], ciphertext)
    event["signature"] = person[1].sign(
        json.dumps(payload, separators=(",", ":")).encode()).hex()
    return event


def policy_payload(conversation_id, replicas, version=1, previous_hash=None):
    # This tuple is the cross-language kova-dm-replica-policy-v1 canonical encoding.
    return json.dumps(("kova-dm-replica-policy-v1", conversation_id, version,
                       previous_hash, replicas), separators=(",", ":")).encode()


def make_policy(manifest, descriptors):
    descriptors = sorted(descriptors, key=lambda item: item["origin"])
    return {"conversation_id": manifest["conversation_id"], "version": 1,
            "previous_hash": None, "replicas": descriptors, "approvals": []}


def approval(person, policy):
    return {"identity": "kova1-" + digest(person[2]["root_public_key"]),
            "device": digest(person[2]["device_public_key"]),
            "signature": person[1].sign(policy_payload(
                policy["conversation_id"], policy["replicas"], policy["version"],
                policy["previous_hash"])).hex()}


def federation_call(source, destination, signer_path, public_key, body, timestamp, nonce,
                    signature_override=None, expected=200):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey as SigningKey
    path = "/api/v1/federation/v1/dm/sync"
    encoded = json.dumps(body, separators=(",", ":")).encode()
    canonical = (f"kova-federation-dm-v1\n{source}\n{destination}\nPOST\n{path}\n"
                 f"{sha256(encoded).hexdigest()}\n{timestamp}\n{nonce}").encode()
    key_bytes = Path(signer_path).read_bytes()
    # ring's PKCS#8 v2 stores the Ed25519 seed in this fixed RFC 8410 structure.
    assert key_bytes[:16] == bytes.fromhex("3051020101300506032b657004220420")
    signer = SigningKey.from_private_bytes(key_bytes[16:48])
    assert public(signer) == public_key
    signature = signature_override or signer.sign(canonical).hex()
    headers = {
        "Origin": destination, "Content-Type": "application/json",
        "x-kova-signer": public_key, "x-kova-timestamp": str(timestamp),
        "x-kova-nonce": nonce, "x-kova-signature": signature,
    }
    request = Request(destination + path, method="POST", headers=headers, data=encoded)
    try:
        response = urlopen(request, timeout=10)
    except HTTPError as error:
        response = error
    with response:
        raw = response.read()
        accepted = expected if isinstance(expected, tuple) else (expected,)
        assert response.status in accepted, (destination, response.status, raw)
        assert response.headers["Cache-Control"] == "no-store"
        return json.loads(raw) if raw else None


def wait_health(origin, process):
    deadline = time.monotonic() + 45
    last = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"API exited early with code {process.returncode}")
        try:
            with urlopen(origin + "/health/ready", timeout=2) as response:
                if response.status == 200:
                    return
                last = response.status
        except (URLError, TimeoutError) as error:
            last = error
        time.sleep(0.25)
    raise RuntimeError(f"API did not become ready: {last}")


def main():
    binary = Path(os.environ.get("KOVA_TEST_SERVER_BINARY", ""))
    if not binary.is_file():
        target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        suffix = ".exe" if os.name == "nt" else ""
        binary = target / "debug" / f"kova-server{suffix}"
        if not binary.is_file():
            cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / f"cargo{suffix}")
            subprocess.run([cargo, "build", "-p", "kova-server", "--locked"],
                           cwd=ROOT, check=True, timeout=600)

    processes, db_names = [], []
    temp = tempfile.TemporaryDirectory(prefix="kova-replication-")
    key_files = []
    password = secrets.token_urlsafe(32)
    project = "kova-replica-" + uuid4().hex[:12]
    compose_env = dict(os.environ, POSTGRES_PASSWORD=password,
                       KOVA_API_PORT="0", KOVA_DB_PORT="0",
                       KOVA_ORIGIN="http://127.0.0.1:0")

    def compose(*args, capture=False):
        return subprocess.run(
            ["docker", "compose", "-p", project, "-f", str(COMPOSE), *args],
            env=compose_env, check=True, capture_output=capture, text=True,
        ).stdout

    def query_db(database, sql):
        return compose("exec", "-T", "db", "psql", "-XAtq", "-v",
                       "ON_ERROR_STOP=1", "-U", "kova", "-d", database,
                       "-c", sql, capture=True).strip()

    def query(sql):
        return query_db(db_names[0], sql)

    try:
        compose("up", "db", "-d", "--wait")
        address = compose("port", "db", "5432", capture=True).strip()
        db_host, db_port = address.rsplit(":", 1)
        db_port = str(int(db_port))
        for name in ("replica_a", "replica_b", "replica_c"):
            query_db("kova", f"CREATE DATABASE {name}")
            db_names.append(name)

        people = {name: identity() for name in ("alice", "bob", "carol")}
        origins = [f"http://127.0.0.1:{free_port()}" for _ in range(3)]
        descriptors = []
        for index, name in enumerate(("a", "b", "c")):
            path = Path(temp.name) / f"{name}.pk8"
            generated = subprocess.run([str(binary), "generate-federation-key", str(path)],
                                       cwd=ROOT, check=True, capture_output=True, text=True)
            key_bytes = path.read_bytes()
            duplicate = subprocess.run([str(binary), "generate-federation-key", str(path)],
                                       cwd=ROOT, capture_output=True, text=True)
            assert duplicate.returncode != 0
            assert path.read_bytes() == key_bytes
            key_files.append(path)
            descriptors.append({"origin": origins[index], "public_key": generated.stdout.strip()})

        # Every instance pins only explicitly known peers. C is configured so the test can
        # prove that a live but unapproved peer cannot read or write this conversation.
        for index, name in enumerate(("a", "b", "c")):
            port = origins[index].rsplit(":", 1)[1]
            peers = [descriptor for peer_index, descriptor in enumerate(descriptors)
                     if peer_index != index]
            env = dict(os.environ, PGHOST=db_host, PGPORT=db_port,
                       PGDATABASE=db_names[index], PGUSER="kova",
                       PGPASSWORD=password,
                       KOVA_ORIGIN=origins[index], KOVA_LISTEN=f"127.0.0.1:{port}",
                       KOVA_FEDERATION_KEY_FILE=str(key_files[index]),
                       KOVA_FEDERATION_PEERS=json.dumps(peers))
            log_path = Path(temp.name) / f"{name}.log"
            log = log_path.open("w", encoding="utf-8")
            process = subprocess.Popen([str(binary)], cwd=ROOT, env=env,
                                       stdout=log, stderr=subprocess.STDOUT)
            processes.append((process, log, log_path))
        for index in range(3):
            wait_health(origins[index], processes[index][0])

        tokens = {}
        communities = {}
        for index in range(3):
            origin = origins[index]
            alice = people["alice"]
            bootstrap = dict(os.environ, PGHOST=db_host, PGPORT=db_port,
                             PGDATABASE=db_names[index], PGUSER="kova",
                             PGPASSWORD=password)
            subprocess.run([str(binary), "bootstrap-admin", "alice", "Alice", public(alice[0])],
                           cwd=ROOT, env=bootstrap, check=True, capture_output=True)
            tokens[(index, "alice")] = login(origin, alice)
            community = call(origin, "/communities", 201, tokens[(index, "alice")],
                             {"name": "Replication test"}, "POST")["id"]
            communities[index] = community
            invite = call(origin, f"/communities/{community}/invites", 201,
                          tokens[(index, "alice")], {"expires_in": 3600}, "POST")["invite"]
            tokens[(index, "bob")] = login(origin, people["bob"], invite, "bob")
            invite_c = call(origin, f"/communities/{community}/invites", 201,
                            tokens[(index, "alice")], {"expires_in": 3600}, "POST")["invite"]
            tokens[(index, "carol")] = login(origin, people["carol"], invite_c, "carol")

        a, b, c = origins
        alice, bob, carol = people["alice"], people["bob"], people["carol"]
        bundle_a = signed_bundle(alice, b"alice-key-package")
        bundle_b = signed_bundle(bob, b"bob-key-package")
        call(a, "/dm/key-packages", 200, tokens[(0, "alice")], bundle_a, "POST")
        call(a, "/dm/key-packages", 200, tokens[(0, "bob")], bundle_b, "POST")
        manifest = signed_conversation(alice, bundle_a, bundle_b, a)
        assert call(a, "/dm/conversations", 200, tokens[(0, "alice")], manifest, "POST") == manifest

        policy = make_policy(manifest, descriptors[:2])
        # One participant's approval remains pending; a forged identity or changed policy is rejected.
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas", 404,
             tokens[(0, "alice")])
        first_approval = approval(alice, policy)
        pending = call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve",
                       200, tokens[(0, "alice")],
                       {"policy": policy, "approval": first_approval}, "POST")
        assert len(pending["approvals"]) == 1
        assert call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/proposal",
                    200, tokens[(0, "bob")]) == pending
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve",
             (403, 404), tokens[(0, "carol")],
             {"policy": policy, "approval": approval(carol, policy)}, "POST")
        second_approval = approval(bob, policy)
        active = call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve",
                      200, tokens[(0, "bob")],
                      {"policy": policy, "approval": second_approval}, "POST")
        assert len(active["approvals"]) == 2
        assert call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas",
                    200, tokens[(0, "alice")]) == active

        # Sync is explicit. A signed Alice event reaches approved B only after durable ack.
        event_a = signed_event(alice, manifest, str(uuid4()), [], "aa")
        event_path = f"/dm/conversations/{manifest['conversation_id']}/events"
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 200,
             tokens[(0, "alice")], {"origin": b}, "POST")
        assert call(b, event_path, 200, tokens[(1, "bob")])["items"] == []
        call(a, event_path, 200, tokens[(0, "alice")], event_a, "POST")
        sync_result = call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync",
                           200, tokens[(0, "alice")], {"origin": b}, "POST")
        assert sync_result["sent"] >= 1
        page_b = call(b, event_path, 200, tokens[(1, "bob")])
        assert [item["event"] for item in page_b["items"]] == [event_a]

        # Federation signatures bind both origins and exact body bytes; nonce replay is rejected.
        timestamp = int(time.time())
        nonce = secrets.token_hex(16)
        federation_body = {"manifest": manifest, "policy": active,
                           "policies": [active],
                           "source_origin": a, "source_after": 1,
                           "remote_after": 0, "events": []}
        federation_call(a, b, key_files[0], descriptors[0]["public_key"],
                        federation_body, timestamp, nonce)  # Simulate a lost acknowledgment.
        tampered_body = dict(federation_body, source_after=2)
        federation_call(a, b, key_files[0], descriptors[0]["public_key"],
                        tampered_body, timestamp, nonce, expected=(400, 401, 403))
        federation_call(a, b, key_files[0], descriptors[0]["public_key"],
                        federation_body, timestamp, nonce, expected=(400, 401, 403))
        federation_call(a, b, key_files[0], descriptors[0]["public_key"],
                        federation_body, timestamp, secrets.token_hex(16))

        # B accepts an event signed by the other conversation participant and syncs it back.
        event_b = dict(event_a, event_id=str(uuid4()), author_identity=bundle_b["global_identity_id"],
                       author_device=bundle_b["device_id"], ciphertext="bb", signature="")
        payload = ("kova-dm-event-v1", event_b["conversation_id"], event_b["event_id"],
                   event_b["author_identity"], event_b["author_device"], event_b["parents"],
                   event_b["ciphertext"])
        event_b["signature"] = bob[1].sign(json.dumps(payload, separators=(",", ":")).encode()).hex()
        call(b, event_path, 200, tokens[(1, "bob")], event_b, "POST")
        call(b, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 200,
             tokens[(1, "bob")], {"origin": a}, "POST")
        page_a = call(a, event_path, 200, tokens[(0, "alice")])
        assert {item["event"]["event_id"] for item in page_a["items"]} == {
            event_a["event_id"], event_b["event_id"]}

        # A forged remote cursor must be rejected without moving the durable ACK
        # beyond B's last local cursor. A subsequent ordinary sync still works.
        cursor_row = query_db(db_names[1],
            "SELECT acknowledged_cursor FROM dm_replica_cursors "
            f"WHERE conversation_id='{manifest['conversation_id']}'::uuid AND origin='{a}'")
        invalid_cursor = dict(federation_body, remote_after=999999,
                              source_after=1, policies=[active], events=[])
        federation_call(a, b, key_files[0], descriptors[0]["public_key"],
                        invalid_cursor, int(time.time()), secrets.token_hex(16),
                        expected=(400, 403, 409))
        assert query_db(db_names[1],
            "SELECT acknowledged_cursor FROM dm_replica_cursors "
            f"WHERE conversation_id='{manifest['conversation_id']}'::uuid AND origin='{a}'") == cursor_row
        call(b, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 200,
             tokens[(1, "bob")], {"origin": a}, "POST")
        # Repeating sync and replayed event is idempotent; conflicting bytes are rejected.
        call(b, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 200,
             tokens[(1, "bob")], {"origin": a}, "POST")
        call(b, event_path, 200, tokens[(1, "bob")], event_b, "POST")
        conflicting = dict(event_b, ciphertext="cc", signature="")
        conflicting_payload = ("kova-dm-event-v1", conflicting["conversation_id"],
                               conflicting["event_id"], conflicting["author_identity"],
                               conflicting["author_device"], conflicting["parents"],
                               conflicting["ciphertext"])
        conflicting["signature"] = bob[1].sign(
            json.dumps(conflicting_payload, separators=(",", ":")).encode()).hex()
        call(b, event_path, 409, tokens[(1, "bob")], conflicting, "POST")
        missing = signed_event(alice, manifest, str(uuid4()), [str(uuid4())], "cc")
        call(a, event_path, 409, tokens[(0, "alice")], missing, "POST")

        # More than one federation page is durable across a remote outage and restart.
        bulk_ids = []
        parent = event_b["event_id"]
        for number in range(27):
            event = signed_event(alice, manifest, str(uuid4()), [parent], f"{number:02x}")
            call(a, event_path, 200, tokens[(0, "alice")], event, "POST")
            bulk_ids.append(event["event_id"])
            parent = event["event_id"]
        b_process, b_log, _ = processes[1]
        b_process.terminate()
        b_process.wait(timeout=15)
        b_log.close()
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync",
             (500, 502, 503), tokens[(0, "alice")], {"origin": b}, "POST")
        port = origins[1].rsplit(":", 1)[1]
        peers_b = [descriptor for peer_index, descriptor in enumerate(descriptors) if peer_index != 1]
        env_b = dict(os.environ, PGHOST=db_host, PGPORT=db_port, PGDATABASE=db_names[1],
                     PGUSER="kova", PGPASSWORD=password, KOVA_ORIGIN=b,
                     KOVA_LISTEN=f"127.0.0.1:{port}",
                     KOVA_FEDERATION_KEY_FILE=str(key_files[1]),
                     KOVA_FEDERATION_PEERS=json.dumps(peers_b))
        log_path = Path(temp.name) / "b-resumed.log"
        log = log_path.open("w", encoding="utf-8")
        process = subprocess.Popen([str(binary)], cwd=ROOT, env=env_b,
                                   stdout=log, stderr=subprocess.STDOUT)
        processes[1] = (process, log, log_path)
        wait_health(b, process)
        sent_total = 0
        for _ in range(64):
            result = call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync",
                          200, tokens[(0, "alice")], {"origin": b}, "POST")
            sent_total += result["sent"]
            if result["sent"] == 0 and result["received"] == 0:
                break
        else:
            raise AssertionError("Replica synchronization did not reach a stable cursor")
        assert sent_total >= 27
        page_b = call(b, event_path, 200, tokens[(1, "bob")])
        assert len(page_b["items"]) == 29
        received_ids = {item["event"]["event_id"] for item in page_b["items"]}
        assert set(bulk_ids).issubset(received_ids)
        assert int(query(f"SELECT count(*) FROM dm_events WHERE conversation_id='{manifest['conversation_id']}'")) == 29
        c_count = compose("exec", "-T", "db", "psql", "-XAtq", "-v", "ON_ERROR_STOP=1",
                          "-U", "kova", "-d", db_names[2], "-c",
                          f"SELECT count(*) FROM dm_conversations WHERE id='{manifest['conversation_id']}'::uuid",
                          capture=True).strip()
        assert c_count == "0"

        # A far-future, correctly signed candidate cannot advance policy state or
        # leave the conversation blocked by a fabricated policy conflict.
        future = dict(policy, version=65, previous_hash="a1" * 32)
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve", 400,
             tokens[(0, "alice")],
             {"policy": future, "approval": approval(alice, future)}, "POST")
        assert call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas", 200,
                    tokens[(0, "alice")]) == active
        assert query(
            "SELECT count(*) FROM dm_replica_conflicts "
            f"WHERE conversation_id='{manifest['conversation_id']}'::uuid") == "0"

        # Exercise the Rust MLS client across the same two configured replicas when enabled.
        if os.environ.get("KOVA_TEST_RUST_CLIENT") == "1":
            invites = {}
            for label, index in (("ALICE_A", 0), ("BOB_A", 0),
                                 ("ALICE_B", 1), ("BOB_B", 1)):
                invites[label] = call(
                    origins[index],
                    f"/communities/{communities[index]}/invites", 201,
                    tokens[(index, "alice")], {"expires_in": 3600}, "POST")["invite"]
            client_env = dict(os.environ,
                              KOVA_REPLICATION_ORIGIN_A=a,
                              KOVA_REPLICATION_ORIGIN_B=b,
                              KOVA_REPLICATION_INVITE_ALICE_A=invites["ALICE_A"],
                              KOVA_REPLICATION_INVITE_ALICE_B=invites["ALICE_B"],
                              KOVA_REPLICATION_INVITE_BOB_A=invites["BOB_A"],
                              KOVA_REPLICATION_INVITE_BOB_B=invites["BOB_B"])
            handshake = Path(temp.name) / "failover-handshake"
            handshake.mkdir()
            client_env["KOVA_REPLICATION_FAILOVER_HANDSHAKE"] = str(handshake)
            cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" /
                                                  ("cargo.exe" if os.name == "nt" else "cargo"))
            client_log = tempfile.TemporaryFile(mode="w+", encoding="utf-8")

            def client_output():
                client_log.seek(0)
                return client_log.read()

            client = subprocess.Popen(
                [cargo, "test", "-p", "kova-client-core", "--locked", "--test",
                 "replication_live", "--", "--ignored", "--nocapture"],
                cwd=ROOT, env=client_env, stdout=client_log,
                stderr=subprocess.STDOUT, text=True)
            try:
                deadline = time.monotonic() + 600
                while not (handshake / "ready").exists():
                    if client.poll() is not None:
                        raise RuntimeError(f"Rust client exited before failover: {client_output()}")
                    if time.monotonic() > deadline:
                        raise TimeoutError("Rust client did not request primary outage")
                    time.sleep(0.1)
                primary, primary_log, _ = processes[0]
                primary.terminate()
                primary.wait(timeout=15)
                primary_log.close()
                (handshake / "stopped").write_text("ok", encoding="utf-8")
                while not (handshake / "restart").exists():
                    if client.poll() is not None:
                        raise RuntimeError(f"Rust failover test failed: {client_output()}")
                    if time.monotonic() > deadline:
                        raise TimeoutError("Rust client did not request primary restart")
                    time.sleep(0.1)
                port = origins[0].rsplit(":", 1)[1]
                env_a = dict(os.environ, PGHOST=db_host, PGPORT=db_port,
                             PGDATABASE=db_names[0], PGUSER="kova", PGPASSWORD=password,
                             KOVA_ORIGIN=a, KOVA_LISTEN=f"127.0.0.1:{port}",
                             KOVA_FEDERATION_KEY_FILE=str(key_files[0]),
                             KOVA_FEDERATION_PEERS=json.dumps(descriptors[1:]))
                log_path = Path(temp.name) / "a-failover-restart.log"
                log = log_path.open("w", encoding="utf-8")
                primary = subprocess.Popen([str(binary)], cwd=ROOT, env=env_a,
                                           stdout=log, stderr=subprocess.STDOUT)
                processes[0] = (primary, log, log_path)
                wait_health(a, primary)
                (handshake / "resumed").write_text("ok", encoding="utf-8")
                client.wait(timeout=120)
                output = client_output()
                print(output)
                if client.returncode:
                    raise RuntimeError(f"Rust failover test failed ({client.returncode}): {output}")
            finally:
                if client.poll() is None:
                    client.terminate()
                    client.wait(timeout=10)
                client_log.close()

        # C is configured and reachable but absent from the bilateral policy.
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 403,
             tokens[(0, "alice")], {"origin": c}, "POST")
        call(c, f"/dm/conversations/{manifest['conversation_id']}/events", 404,
             tokens[(2, "carol")])
        # Editing an approved descriptor requires a new version and both signatures.
        changed = dict(policy, version=2, previous_hash="00" * 32,
                       replicas=sorted(descriptors[:2] + [descriptors[2]], key=lambda x: x["origin"]))
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve", 409,
             tokens[(0, "alice")], {"policy": changed, "approval": approval(alice, changed)}, "POST")

        # Bilateral removal blocks subsequent transfers and local event access.
        reduced = make_policy(manifest, [descriptors[0]])
        reduced["version"] = 2
        reduced["previous_hash"] = sha256(policy_payload(
            policy["conversation_id"], policy["replicas"], policy["version"],
            policy["previous_hash"])).hexdigest()
        call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve", 200,
             tokens[(0, "alice")], {"policy": reduced, "approval": approval(alice, reduced)}, "POST")
        reduced = call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/approve", 200,
             tokens[(0, "bob")], {"policy": reduced, "approval": approval(bob, reduced)}, "POST")
        removed_sync = call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 200,
                            tokens[(0, "alice")], {"origin": b}, "POST")
        assert removed_sync["sent"] == 0
        assert call(b, f"/dm/conversations/{manifest['conversation_id']}/replicas", 200,
                    tokens[(1, "bob")]) == reduced
        # Reapplying a removal notice with a fresh nonce is idempotent.
        assert call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas/sync", 200,
                    tokens[(0, "alice")], {"origin": b}, "POST")["sent"] == 0
        call(b, event_path, 403, tokens[(1, "bob")])
        call(b, event_path, 403, tokens[(1, "bob")],
             signed_event(bob, manifest, str(uuid4()), [], "dd"), "POST")

        # B's older signed v1 is stale after it has applied the removal v2.
        # It must be rejected without recording a false same-version fork.
        stale_body = {"manifest": manifest, "policy": active,
                      "policies": [active], "source_origin": a,
                      "source_after": 0, "remote_after": 0, "events": []}
        federation_call(a, b, key_files[0], descriptors[0]["public_key"],
                        stale_body, int(time.time()), secrets.token_hex(16),
                        expected=(400, 403, 409))
        assert query(
            "SELECT count(*) FROM dm_replica_conflicts "
            f"WHERE conversation_id='{manifest['conversation_id']}'::uuid") == "0"

        # A valid Alice session cannot federate a conversation to a replica where Bob's
        # enrolled device has been revoked, even though the source instance still accepts it.
        bundle_a2 = signed_bundle(alice, b"alice-key-package-2")
        bundle_b2 = signed_bundle(bob, b"bob-key-package-2")
        call(a, "/dm/key-packages", 200, tokens[(0, "alice")], bundle_a2, "POST")
        call(a, "/dm/key-packages", 200, tokens[(0, "bob")], bundle_b2, "POST")
        manifest2 = signed_conversation(alice, bundle_a2, bundle_b2, a)
        call(a, "/dm/conversations", 200, tokens[(0, "alice")], manifest2, "POST")
        policy2 = make_policy(manifest2, descriptors[:2])
        call(a, f"/dm/conversations/{manifest2['conversation_id']}/replicas/approve", 200,
             tokens[(0, "alice")], {"policy": policy2, "approval": approval(alice, policy2)}, "POST")
        call(a, f"/dm/conversations/{manifest2['conversation_id']}/replicas/approve", 200,
             tokens[(0, "bob")], {"policy": policy2, "approval": approval(bob, policy2)}, "POST")
        revoke_body = (f"kova-revoke-v1\n{bundle_b2['global_identity_id']}\n"
                       f"{bundle_b2['device_id']}")
        call(b, "/identity/devices/revoke", 200, tokens[(1, "bob")], {
            "device_public_key": bundle_b2["device_public_key"],
            "signature": bob[0].sign(revoke_body.encode()).hex(),
        }, "POST")
        call(a, f"/dm/conversations/{manifest2['conversation_id']}/replicas/sync", 403,
             tokens[(0, "alice")], {"origin": b}, "POST")
        missing_on_b = compose("exec", "-T", "db", "psql", "-XAtq", "-v", "ON_ERROR_STOP=1",
                               "-U", "kova", "-d", db_names[1], "-c",
                               f"SELECT count(*) FROM dm_conversations WHERE id='{manifest2['conversation_id']}'::uuid",
                               capture=True).strip()
        assert missing_on_b == "0"

        # Restart both stores and prove policy and replicated event history survive.
        processes[0][0].terminate(); processes[1][0].terminate()
        for process, log, _ in processes[:2]:
            process.wait(timeout=15)
            log.close()
        for index in (0, 1):
            name = ("a", "b")[index]
            port = origins[index].rsplit(":", 1)[1]
            peers = [descriptor for peer_index, descriptor in enumerate(descriptors)
                     if peer_index != index]
            env = dict(os.environ, PGHOST=db_host, PGPORT=db_port,
                       PGDATABASE=db_names[index], PGUSER="kova",
                       PGPASSWORD=password,
                       KOVA_ORIGIN=origins[index], KOVA_LISTEN=f"127.0.0.1:{port}",
                       KOVA_FEDERATION_KEY_FILE=str(key_files[index]),
                       KOVA_FEDERATION_PEERS=json.dumps(peers))
            log_path = Path(temp.name) / f"{name}-restart.log"
            log = log_path.open("w", encoding="utf-8")
            process = subprocess.Popen([str(binary)], cwd=ROOT, env=env,
                                       stdout=log, stderr=subprocess.STDOUT)
            processes[index] = (process, log, log_path)
            wait_health(origins[index], process)
        assert call(a, f"/dm/conversations/{manifest['conversation_id']}/replicas",
                    200, tokens[(0, "alice")]) == reduced
        call(b, event_path, (401, 403), tokens[(1, "bob")])
        assert compose("exec", "-T", "db", "psql", "-XAtq", "-v", "ON_ERROR_STOP=1",
                       "-U", "kova", "-d", db_names[1], "-c",
                       f"SELECT count(*) FROM dm_conversations WHERE id='{manifest2['conversation_id']}'::uuid",
                       capture=True).strip() == "0"
        print("Réplication MP OK : consentement bilatéral, synchronisation A/B, exclusion de C, reprise persistante")
    finally:
        for process, log, _ in processes:
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            log.close()
        if sys.exc_info()[0] is not None:
            for _, _, log_path in processes:
                if log_path.exists():
                    print(f"--- {log_path.name} ---", file=sys.stderr)
                    print(log_path.read_text(encoding="utf-8", errors="replace"), file=sys.stderr)
        compose("down", "-v", "--remove-orphans")
        temp.cleanup()


if __name__ == "__main__":
    main()

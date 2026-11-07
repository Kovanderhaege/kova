"""Two replicas: signed revocation union, delayed delivery and fail-closed expiry."""
from hashlib import sha256
import json
from pathlib import Path
import socket
import sys
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "client"))
from identity import Ed25519PrivateKey, authorize, public, digest
from compose_instance import instance


def main():
    replicas = []
    for _ in range(2):
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        replicas.append((f"http://127.0.0.1:{port}", *instance(str(port))))
    root, device, second, unseen = (Ed25519PrivateKey.generate() for _ in range(4))
    identity = "kova1-" + digest(public(root))

    def call(index, path, expected, data=None, token=None):
        origin = replicas[index][0]
        headers = {"Origin": origin, "Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        req = Request(origin + "/api/v1" + path, headers=headers,
                      data=json.dumps(data).encode() if data is not None else None)
        try:
            response = urlopen(req, timeout=10)
        except HTTPError as error:
            response = error
        with response:
            assert response.status == expected, (path, response.status, expected)
            raw = response.read()
            return json.loads(raw) if raw else None

    def login(index, key, expected=200, expiry=None, issued=None, legacy=False):
        cert = authorize(root, key)
        if expiry is not None:
            issued = int(time.time()) if issued is None else issued
            payload = (f"kova-device-v1\n{identity}\n{digest(public(key))}\n{public(key)}\n{expiry}" if legacy else
                       f"kova-device-v2\n{identity}\n{digest(public(key))}\n{public(key)}\n{issued}\n{expiry}")
            cert["device_authorization"] = dict(issued_at=issued, expires_at=expiry, signature=root.sign(payload.encode()).hex())
            if legacy:
                del cert["device_authorization"]["issued_at"]
        challenge = call(index, "/auth/challenges", 201, dict(global_identity_id=identity,
            device_id=digest(public(key)), operation="login", session_mode="bearer"))
        proof = dict(cert, challenge_id=challenge["challenge_id"],
                     signature=key.sign(challenge["challenge"].encode()).hex())
        return call(index, "/auth/verify", expected, proof)

    def revoke(key):
        return dict(global_identity_id=identity, device_public_key=public(key), signature=root.sign(
            f"kova-revoke-v1\n{identity}\n{digest(public(key))}".encode()).hex())

    try:
        for _, compose, _, ready in replicas:
            compose("up", "-d", "--build", "--wait")
            ready(200)
            compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "alice", "Alice", public(root))
        a = login(0, device)["access_token"]
        b = login(1, device)["access_token"]
        observer = login(0, second)["access_token"]
        # A valid signature cannot extend the accepted authorization horizon.
        login(0, second, 401, int(time.time()) + 90000)
        login(0, second, 401, int(time.time()) - 1)
        login(1, second, 401, int(time.time()) + 60, int(time.time()) - 90*86400)
        login(1, second, 401, int(time.time()) + 60, legacy=True)
        proof = revoke(device)
        call(0, "/identity/revocations", 401, dict(proof, signature="00" * 64))
        call(0, "/identity/revocations", 200, proof)
        call(0, "/identity/revocations", 200, proof)  # replay is idempotent
        call(0, "/me", 401, token=a)
        call(1, "/me", 200, token=b)  # not yet delivered: explicitly documented delay
        journal = call(0, "/identity/revocations", 200, token=observer)["items"]
        assert len(journal) == 1
        event = journal[0]
        root.public_key().verify(bytes.fromhex(event["signature"]), event["payload"].encode())
        assert event["event_id"] == sha256(event["payload"].encode()).hexdigest()
        # Deliver the same proof to B without any session on that replica.
        call(1, "/identity/revocations", 200, proof)
        call(1, "/me", 401, token=b)
        login(1, device, 401)  # a fresh certificate never clears a tombstone
        # A device need not have visited B before it can be revoked there.
        call(1, "/identity/revocations", 200, revoke(unseen))
        login(1, unseen, 401)
        assert replicas[1][2]("SELECT count(*) FROM identity_revocations") == "2"
        print("OK : journal signé, idempotence, deux instances, révocation avant connexion, autorisations expirées/trop longues.")
    finally:
        for _, compose, _, _ in replicas:
            compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()

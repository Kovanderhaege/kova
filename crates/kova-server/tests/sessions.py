"""Test HTTP du jalon comptes et sessions contre PostgreSQL."""
from hashlib import sha256
from http.cookiejar import CookieJar
import json
import secrets
import sys
from pathlib import Path
from uuid import uuid4
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "client"))
from identity import Ed25519PrivateKey, public, authorize, digest, prove, save, main as client_main
from tempfile import TemporaryDirectory
from unittest.mock import patch
from contextlib import redirect_stdout
from io import StringIO
import socket
from subprocess import CalledProcessError
from urllib.error import HTTPError
from urllib.request import build_opener, HTTPCookieProcessor, Request

from compose_instance import instance


def main():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    compose, query, wait_ready = instance(str(port))
    origin = f"http://127.0.0.1:{port}"
    def client():
        return build_opener(HTTPCookieProcessor(CookieJar()))

    def call(browser, path, status, *, method="GET", data=None, source=origin, cookie=None, bearer=None):
        headers = {}
        if source is not None:
            headers["Origin"] = source
        if bearer:
            headers["Authorization"] = "Bearer " + bearer
        if cookie:
            headers["Cookie"] = cookie
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = Request(origin + "/api/v1" + path, method=method, headers=headers,
                          data=json.dumps(data).encode() if data is not None else None)
        try:
            response = browser.open(request, timeout=5)
        except HTTPError as error:
            response = error
        with response:
            assert response.status in (status if isinstance(status, tuple) else (status,)), (path, response.status, status)
            raw = response.read()
            return response.headers.get("Set-Cookie"), json.loads(raw) if raw else None

    def challenge(browser, cert, invite=None):
        request = dict(global_identity_id="kova1-" + digest(cert["root_public_key"]),
                       device_id=digest(cert["device_public_key"]),
                       operation="join" if invite else "login", invite=invite)
        return call(browser, "/auth/challenges", 201, method="POST", data=request)[1]

    def verify(browser, proof, status):
        return call(browser, "/auth/verify", status, method="POST", data=proof)

    try:
        compose("up", "-d", "--build", "--wait")
        wait_ready(200)
        root, device, other = (Ed25519PrivateKey.generate() for _ in range(3))
        cert = authorize(root, device)
        compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "alice", "Alice", public(root))
        try:
            compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "bob", "Bob", public(other), capture=True)
        except CalledProcessError:
            pass
        else:
            raise AssertionError("Second administrateur accepté")
        a, b = client(), client()
        call(a, "/me", 401)
        for source in (None, origin + ".evil"):
            call(a, "/auth/challenges", 403, method="POST", data={}, source=source)
        ch = challenge(a, cert)
        proof = prove(device, cert, ch, origin)
        verify(a, dict(proof, signature="00" * 64), 401)
        verify(a, dict(proof, root_public_key=public(other)), 401)
        cookie_a, user = verify(a, proof, 200)
        assert user["global_identity_id"] == "kova1-" + digest(public(root))
        verify(a, proof, 401)
        desktop = client()
        desktop_ch = call(desktop, "/auth/challenges", 201, method="POST", data=dict(
            global_identity_id=user["global_identity_id"], device_id=digest(public(device)),
            operation="login", session_mode="bearer"))[1]
        assert desktop_ch["challenge"].startswith("kova-auth-desktop-v1\n")
        desktop_proof = dict(cert, challenge_id=desktop_ch["challenge_id"],
                             signature=device.sign(desktop_ch["challenge"].encode()).hex())
        # Signing the legacy context cannot downgrade a desktop challenge.
        wrong = device.sign(desktop_ch["challenge"].replace("kova-auth-desktop-v1", "kova-auth-v1", 1).encode()).hex()
        verify(desktop, dict(desktop_proof, signature=wrong), 401)
        no_cookie, session = verify(desktop, desktop_proof, 200)
        token = session["access_token"]
        assert no_cookie is None and 0 < session["expires_in"] <= 3600
        assert session["profile"] == user and session["token_type"] == "Bearer"
        verify(desktop, desktop_proof, 401)
        call(desktop, "/me", 200, bearer=token)
        call(desktop, "/me", 401, cookie="kova_session=" + token)
        call(desktop, "/me", 401, bearer=cookie_a.split(";", 1)[0].split("=", 1)[1])
        call(desktop, "/me", 401, bearer=token, cookie=cookie_a.split(";", 1)[0])
        call(desktop, "/me", 401, cookie=cookie_a.split(";", 1)[0] + "; " + cookie_a.split(";", 1)[0])
        stored = call(desktop, "/identity/devices", 200, bearer=token)[1]["items"][0]
        assert stored["authorization_signature"] == cert["device_authorization"]["signature"]
        assert stored["authorization_expires_at"] == cert["device_authorization"]["expires_at"]
        call(desktop, "/auth/logout", 204, method="POST", bearer=token)
        call(desktop, "/me", 401, bearer=token)
        desktop_ch = call(desktop, "/auth/challenges", 201, method="POST", data=dict(
            global_identity_id=user["global_identity_id"], device_id=digest(public(device)),
            operation="login", session_mode="bearer"))[1]
        active_token = verify(desktop, dict(cert, challenge_id=desktop_ch["challenge_id"],
            signature=device.sign(desktop_ch["challenge"].encode()).hex()), 200)[1]["access_token"]
        second = authorize(root, other)
        cookie_b, same_user = verify(b, prove(other, second, challenge(b, second), origin), 200)
        assert user == same_user == call(a, "/me", 200)[1] and cookie_a != cookie_b
        assert {"HttpOnly", "Path=/", "SameSite=Lax", "Max-Age=604800"} <= set(cookie_a.split("; "))
        for header in (cookie_a, cookie_b):
            token = bytes.fromhex(header.split(";", 1)[0].split("=", 1)[1])
            assert len(token) == 32
            assert query(f"SELECT count(*) FROM sessions WHERE token_hash=decode('{sha256(token).hexdigest()}','hex')") == "1"
        call(a, "/auth/logout", 403, method="POST", source="https://third.example")
        call(a, "/auth/logout", 204, method="POST")
        call(a, "/me", 401, cookie=cookie_a.split(";")[0])
        call(b, "/me", 200)
        assert len(call(b, "/identity/devices", 200)[1]["items"]) == 2
        device_id = digest(public(device))
        revoke = dict(device_public_key=public(device), signature=root.sign(
            f"kova-revoke-v1\n{user['global_identity_id']}\n{device_id}".encode()).hex())
        verify(a, prove(device, cert, challenge(a, cert), origin), 200)
        call(b, "/identity/devices/revoke", 200, method="POST", data=revoke)
        call(a, "/me", 401)
        call(desktop, "/me", 401, bearer=active_token)
        revoked = next(d for d in call(b, "/identity/devices", 200)[1]["items"] if d["device_id"] == device_id)
        assert revoked["revoked"] and revoked["revocation_signature"] == revoke["signature"]
        call(b, "/me", 200)
        query("UPDATE sessions SET expires_at=now()")
        call(b, "/me", 401)
        expired = prove(device, cert, challenge(a, cert), origin)
        query("UPDATE auth_challenges SET expires_at=now()")
        verify(a, expired, 401)
        verify(a, prove(device, cert, challenge(a, cert), origin), 401)

        community, invite = uuid4(), secrets.token_hex(32)
        query(f"""BEGIN;
            INSERT INTO communities VALUES ('{community}','Atelier','{user["id"]}');
            INSERT INTO memberships VALUES ('{community}','{user["id"]}','member');
            INSERT INTO invites VALUES (decode('{digest(invite)}','hex'),'{community}',now()+interval '1 day');
            COMMIT;""")
        guest_root, guest = Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate()
        guest_cert = authorize(guest_root, guest)
        proofs = [dict(prove(guest, guest_cert, challenge(a, guest_cert, invite), origin, invite),
                       profile=dict(handle="bob", display_name="Bob")) for _ in range(2)]
        def join(proof):
            return call(client(), "/auth/verify", (201, 400), method="POST", data=proof)[1]
        with ThreadPoolExecutor(2) as workers:
            results = list(workers.map(join, proofs))
        assert sum(r.get("handle") == "bob" for r in results) == 1
        assert sum(r.get("error", {}).get("code") == "invalid_invite" for r in results) == 1
        assert query("SELECT count(*) FROM memberships") == "2"
        # Exercise the encrypted device vault on a second, independent instance.
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            other_port = str(sock.getsockname()[1])
        compose2, _, ready2 = instance(other_port)
        try:
            compose2("up", "-d", "--build", "--wait")
            ready2(200)
            compose2("exec", "-T", "api", "kova-server", "bootstrap-admin", "robert", "Robert", public(guest_root))
            with TemporaryDirectory() as directory:
                vault = Path(directory) / "device.kova-vault"
                password = secrets.token_hex(16)
                save(vault, guest, password, certificate=guest_cert)
                argv = ["identity.py", "login", str(vault), f"http://127.0.0.1:{other_port}"]
                with patch.object(sys, "argv", argv), patch("identity.getpass", return_value=password), redirect_stdout(StringIO()) as output:
                    client_main()
                remote = json.loads(output.getvalue())
            local = next(r for r in results if "id" in r)
            assert local["global_identity_id"] == remote["global_identity_id"]
            assert remote["id"] != local["id"] and remote["handle"] == "robert"
        finally:
            compose2("down", "--volumes", "--rmi", "local")
        print("OK : identité, deux appareils, challenges, invitation concurrente et sessions.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()


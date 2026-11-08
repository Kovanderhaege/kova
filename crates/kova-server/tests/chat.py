"""Public chat: concurrency, rollback, replay, WS access and restart on real PostgreSQL."""
import json
import os
import subprocess
from pathlib import Path
import socket
import time
from uuid import uuid4
from websockets.sync.client import connect
from websockets.exceptions import InvalidStatus, ConnectionClosed
from concurrent.futures import ThreadPoolExecutor
from hashlib import sha256
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from compose_instance import instance
from sessions import Ed25519PrivateKey, authorize, public, digest


def main():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    compose, query, ready = instance(str(port))
    origin = f"http://127.0.0.1:{port}"

    def call(path, status=200, token=None, data=None, method="GET", source=origin):
        headers = {"Origin": source}
        if token:
            headers["Authorization"] = "Bearer " + token
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = Request(origin + "/api/v1" + path, method=method, headers=headers,
                          data=None if data is None else json.dumps(data, ensure_ascii=False).encode("utf-8"))
        try:
            response = urlopen(request, timeout=15)
        except HTTPError as error:
            response = error
        with response:
            raw = response.read()
            assert response.status in (status if isinstance(status, tuple) else (status,)), (path, response.status, raw)
            assert response.headers["Cache-Control"] == "no-store"
            return json.loads(raw) if raw else None

    def identity():
        root, device = Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate()
        return root, device, authorize(root, device)

    def login(person, invite=None, handle="guest", status=201):
        _, device, cert = person
        challenge = call("/auth/challenges", 201, data=dict(
            global_identity_id="kova1-" + digest(cert["root_public_key"]),
            device_id=digest(cert["device_public_key"]), operation="join" if invite else "login",
            invite=invite, session_mode="bearer"), method="POST")
        proof = dict(cert, challenge_id=challenge["challenge_id"],
                     signature=device.sign(challenge["challenge"].encode()).hex(),
                     invite=invite, profile=dict(handle=handle, display_name=handle))
        return call("/auth/verify", status, data=proof, method="POST")

    def create(token, name):
        return call("/communities", 201, token, {"name": name}, "POST")["id"]

    def invite(token, community):
        result = call(f"/communities/{community}/invites", 201, token, {"expires_in": 3600}, "POST")
        raw = bytes.fromhex(result["invite"])
        assert len(raw) == 32
        assert query(f"SELECT count(*) FROM invites WHERE token_hash=decode('{sha256(raw).hexdigest()}','hex')") == "1"
        return result["invite"]

    def ws(token, channel, after=0, source=origin):
        return connect(origin.replace("http", "ws", 1) + f"/api/v1/channels/{channel}/events?after={after}",
                       origin=source, additional_headers={"Authorization": "Bearer " + token},
                       proxy=None, open_timeout=5, close_timeout=2, max_size=3_000_000)

    def denied_ws(token, channel, expected, after=0, source=origin):
        try:
            with ws(token, channel, after, source):
                raise AssertionError("Unexpected WebSocket access")
        except InvalidStatus as error:
            assert error.response.status_code == expected, error

    def read_until(stream, seq):
        items = []
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            page = json.loads(stream.recv(timeout=5))
            items.extend(page["items"])
            if page["next_seq"] >= seq:
                return items
        raise AssertionError("Missing WebSocket messages")

    def closed(stream, forbidden_body=None):
        deadline = time.monotonic() + 6
        try:
            while time.monotonic() < deadline:
                page = json.loads(stream.recv(timeout=5))
                assert all(m["body"] != forbidden_body for m in page["items"])
        except ConnectionClosed as error:
            assert error.rcvd.code == 1008, error
            return
        raise AssertionError("WebSocket stayed open after access removal")

    try:
        compose("up", "-d", "--build", "--wait")
        ready(200)
        alice = identity()
        compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "alice", "Alice", public(alice[0]))
        owner = login(alice, status=200)
        a = owner["access_token"]
        community = create(a, "Chat")
        channel = call(f"/communities/{community}/channels", token=a)["items"][0]["id"]
        other = create(a, "Secret")
        secret_channel = call(f"/communities/{other}/channels", token=a)["items"][0]["id"]
        bob = identity()
        member = login(bob, invite(a, community), "bob")
        b, bob_id = member["access_token"], member["profile"]["id"]
        path = f"/channels/{channel}/messages"

        def send(body, token=a, client_id=None, status=201, target=path):
            return call(target, status, token, {"client_id": client_id or str(uuid4()), "body": body}, "POST")

        call(path, 401)
        call(f"/channels/{secret_channel}/messages", 404, b)
        send("secret", b, status=404, target=f"/channels/{secret_channel}/messages")
        denied_ws(b, secret_channel, 404)
        denied_ws(a, channel, 403, source="https://evil.example")
        denied_ws(a, channel, 403, source=None)
        denied_ws("00" * 32, channel, 401)
        denied_ws(a, channel, 400, after=1)
        for body in ("", " \n\t", "x" * 4001, "nul\0"):
            send(body, status=400)
        send("bad id", client_id="not-uuid", status=400)
        call(path + "?after=-1", 400, a)
        call(path, 403, a, {"client_id": str(uuid4()), "body": "origin"}, "POST", "https://evil.example")
        assert call(path, token=a) == {"items": [], "next_seq": 0, "last_seq": 0}
        # Failed INSERT must roll back the increment as well as the message.
        query("CREATE FUNCTION reject_message() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test failure'; END $$; CREATE TRIGGER reject_message BEFORE INSERT ON messages FOR EACH ROW EXECUTE FUNCTION reject_message()")
        send("rollback", status=500)
        assert query(f"SELECT last_seq FROM channels WHERE id='{channel}'") == "0"
        query("DROP TRIGGER reject_message ON messages; DROP FUNCTION reject_message()")
        first_id = str(uuid4())
        first = send("Bonjour 🦀", client_id=first_id)
        assert first["seq"] == 1
        assert send("Bonjour 🦀", client_id=first_id, status=200) == first
        send("different", client_id=first_id, status=409)
        assert send("same client id, other author", b, first_id)["seq"] == 2
        assert send("🦀" * 4000)["seq"] == 3
        replay_id = str(uuid4())
        with ThreadPoolExecutor(max_workers=8) as workers:
            replies = list(workers.map(lambda _: send("concurrent retry", client_id=replay_id, status=(200,201)), range(12)))
        assert {reply["seq"] for reply in replies} == {4}
        with ws(a, channel, 4) as one, ws(b, channel, 4) as two:
            assert json.loads(one.recv(timeout=5))["items"] == []
            with ThreadPoolExecutor(max_workers=8) as workers:
                replies = list(workers.map(lambda n: send(f"parallel {n}"), range(110)))
            assert sorted(reply["seq"] for reply in replies) == list(range(5,115))
            assert [m["seq"] for m in read_until(one,114)] == list(range(5,115))
            assert [m["seq"] for m in read_until(two,114)] == list(range(5,115))
        page = call(path, token=b)
        assert len(page["items"]) == 100 and page["next_seq"] == 100 and page["last_seq"] == 114
        page2 = call(path + "?after=100", token=b)
        assert [m["seq"] for m in page2["items"]] == list(range(101,115))
        send("sent while disconnected")
        with ws(b, channel,114) as stream:
            assert [m["seq"] for m in read_until(stream,115)] == [115]
            call(f"/communities/{community}/members/{bob_id}",204,a,method="DELETE")
            send("after exclusion")
            closed(stream,"after exclusion")
        call(path,404,b)
        send("forbidden",b,status=404)
        denied_ws(b,channel,404)
        # Independent session expiry and logout close existing subscriptions, even when idle.
        second = login(alice,status=200)["access_token"]
        with ws(second,channel,116) as stream:
            read_until(stream,116)
            query(f"UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=decode('{sha256(bytes.fromhex(second)).hexdigest()}','hex')")
            closed(stream)
        third = login(alice,status=200)["access_token"]
        with ws(third,channel,116) as stream:
            read_until(stream,116)
            call("/auth/logout",204,third,method="POST")
            closed(stream)
        # Persistent log survives API restart and resumes without duplicates.
        compose("restart", "api")
        ready(200)
        assert call(path + "?after=115",token=a)["items"][0]["body"] == "after exclusion"
        assert send("after restart")["seq"] == 117
        with ws(a,channel,116) as stream:
            assert [m["seq"] for m in read_until(stream,117)] == [117]
        assert query(f"SELECT count(*)=max(seq) FROM messages WHERE channel_id='{channel}'") == "t"
        # Escaped JSON controls produce >2.4 MB for a full valid page.
        for _ in range(100):
            send("x" + "\x01" * 3999)
        with ws(a, channel, 117) as stream:
            assert len(read_until(stream, 217)) == 100
        if os.environ.get("KOVA_TEST_RUST_CLIENT") == "1":
            env = dict(os.environ, KOVA_TEST_ORIGIN=origin, KOVA_TEST_CHANNEL=channel,
                       KOVA_TEST_SESSION=json.dumps(dict(origin=origin, token=a, expires_at=int(time.time())+600, profile=owner["profile"])))
            subprocess.run(["cargo", "test", "-p", "kova-client-core", "--locked", "--test", "chat_live", "--", "--ignored"],
                           env=env, cwd=Path(__file__).resolve().parents[3], check=True)
        last = int(query(f"SELECT last_seq FROM channels WHERE id='{channel}'"))
        with ws(a, channel, last) as stream:
            read_until(stream, last)
            payload = "kova-revoke-v1\n" + owner["profile"]["global_identity_id"] + "\n" + digest(public(alice[1]))
            call("/identity/devices/revoke", 200, a, {"device_public_key": public(alice[1]), "signature": alice[0].sign(payload.encode()).hex()}, "POST")
            closed(stream)
        print("Chat OK: isolation, Unicode, atomic seq, rollback, retries, pagination, two WS clients, resume, exclusion, expiry, logout, restart")
    finally:
        compose("down", "-v", "--remove-orphans")


if __name__ == "__main__":
    main()


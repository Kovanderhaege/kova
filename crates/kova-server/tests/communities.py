"""Community HTTP contracts, real PostgreSQL transactions and concurrent authorization."""
import json
import socket
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
                          data=None if data is None else json.dumps(data).encode())
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

    try:
        compose("up", "-d", "--build", "--wait")
        ready(200)
        alice = identity()
        compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "alice", "Alice", public(alice[0]))
        owner = login(alice, status=200)
        a = owner["access_token"]
        owner_id = owner["profile"]["id"]
        call("/communities", 401)
        call("/communities", 403, a, {"name": "Denied"}, "POST", "https://evil.example")
        for name in (" ", "x" * 81, "bad\nname", "bad\u0000name"):
            call("/communities", 400, a, {"name": name}, "POST")
        # A failure on the final write must leave neither community nor owner membership.
        query("CREATE FUNCTION reject_channel() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test failure'; END $$; CREATE TRIGGER reject_channel BEFORE INSERT ON channels FOR EACH ROW EXECUTE FUNCTION reject_channel()")
        call("/communities", 500, a, {"name": "Rollback"}, "POST")
        assert query("SELECT count(*) FROM communities") == "0"
        assert query("SELECT count(*) FROM memberships") == "0"
        query("DROP TRIGGER reject_channel ON channels; DROP FUNCTION reject_channel()")
        first, second = create(a, " Atelier "), create(a, "Privé")
        path = f"/communities/{first}"
        assert call(path, token=a)["name"] == "Atelier"
        assert call(path, token=a)["role"] == "owner"
        assert [c["name"] for c in call(path + "/channels", token=a)["items"]] == ["général"]
        assert call(path + "/members", token=a)["items"][0]["id"] == owner_id
        bob = identity()
        b_session = login(bob, invite(a, first), "bob")
        b, bob_id = b_session["access_token"], b_session["profile"]["id"]
        assert [c["id"] for c in call("/communities", token=b)["items"]] == [first]
        for suffix in ("", "/channels", "/members"):
            call(f"/communities/{second}" + suffix, 404, b)
        for suffix, data in (("/channels", {"name": "secret"}), ("/invites", {"expires_in": 60})):
            call(f"/communities/{second}" + suffix, 404, b, data, "POST")
            call(path + suffix, 403, b, data, "POST")
        call("/communities/not-a-uuid", 404, a)
        member_path = path + f"/members/{bob_id}"
        call(member_path, 403, a, {"role": "moderator"}, "PATCH", "https://evil.example")
        call(member_path, 403, a, method="DELETE", source="https://evil.example")
        call("/invites/redeem", 401, data={"invite": invite(a, second)}, method="POST")
        second_invite = invite(a, second)
        call("/invites/redeem", 201, b, {"invite": second_invite}, "POST")
        call("/invites/redeem", 400, b, {"invite": second_invite}, "POST")
        call(f"/communities/{second}", token=b)
        call(member_path, 403, b, {"role": "moderator"}, "PATCH")
        call(member_path, 400, a, {"role": "owner"}, "PATCH")
        call(path + f"/members/{owner_id}", 403, a, {"role": "member"}, "PATCH")
        call(member_path, 204, a, {"role": "moderator"}, "PATCH")
        call(path + "/channels", 201, b, {"name": " entraide "}, "POST")
        call(path + "/channels", 409, a, {"name": "entraide"}, "POST")
        for seconds in (0, -1, 604801):
            call(path + "/invites", 400, a, {"expires_in": seconds}, "POST")
        guest = identity()
        expired = invite(b, first)
        query(f"UPDATE invites SET expires_at=now() WHERE token_hash=decode('{digest(expired)}','hex')")
        login(guest, expired, "guest", 400)
        one_use = invite(b, first)
        with ThreadPoolExecutor(2) as workers:
            results = list(workers.map(lambda _: login(guest, one_use, "guest", (201, 400)), range(2)))
        assert sum("access_token" in result for result in results) == 1
        guest_session = next(result for result in results if "access_token" in result)
        g, guest_id = guest_session["access_token"], guest_session["profile"]["id"]
        guest_path = path + f"/members/{guest_id}"
        call(guest_path, 204, a, {"role": "moderator"}, "PATCH")
        call(guest_path, 403, b, method="DELETE")
        call(path + f"/members/{owner_id}", 403, b, method="DELETE")
        call(member_path, 403, b, method="DELETE")
        call(guest_path, 204, a, {"role": "member"}, "PATCH")
        other = create(g, "Communauté du membre")
        # Even the instance administrator cannot inspect a community they have not joined.
        call(f"/communities/{other}", 404, a)
        rejoin = invite(a, first)
        with ThreadPoolExecutor(2) as workers:
            pending = workers.submit(login, guest, rejoin, "guest", (200, 403))
            call(guest_path, 204, b, method="DELETE")
            pending.result()
        for suffix in ("", "/channels", "/members"):
            call(path + suffix, 404, g)
        assert [c["id"] for c in call("/communities", token=g)["items"]] == [other]
        call("/me", token=g)
        blocked_invite = invite(a, first)
        call("/invites/redeem", 403, g, {"invite": blocked_invite}, "POST")
        login(guest, blocked_invite, "guest", 403)
        # Failed redemption rolls back consumption, so another identity can use it.
        newcomer = login(identity(), blocked_invite, "newcomer")
        assert newcomer["profile"]["handle"] == "newcomer"
        call(member_path, 204, a, {"role": "member"}, "PATCH")
        call(path + "/invites", 403, b, {"expires_in": 60}, "POST")
        call(path + "/channels", 403, b, {"name": "refusé"}, "POST")
        call(member_path, 204, a, method="DELETE")
        call(path, 404, b)
        compose("restart", "api")
        ready(200)
        call(path, 404, g)
        login(guest, invite(a, first), "guest", 403)
        call(f"/communities/{other}", token=g)
        assert query("SELECT count(*) FROM community_exclusions") == "2"
        print("OK : création atomique, invitations, rôles, isolation, concurrence et exclusions persistantes.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()

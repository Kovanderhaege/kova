"""Test HTTP du jalon comptes et sessions contre PostgreSQL."""
from hashlib import sha256
from http.cookiejar import CookieJar
import json
import secrets
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
    password = secrets.token_urlsafe(24)
    login = {"handle": "alice", "password": password}

    def client():
        return build_opener(HTTPCookieProcessor(CookieJar()))

    def call(browser, path, status, *, method="GET", data=None, source=origin, cookie=None):
        headers = {}
        if source is not None:
            headers["Origin"] = source
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
            assert response.status == status, (path, response.status, status)
            raw = response.read()
            return response.headers.get("Set-Cookie"), json.loads(raw) if raw else None

    def bootstrap(handle):
        compose("exec", "-T", "api", "kova-server", "create-first-user",
                sql=f"{handle}\nAlice\n{password}\n", capture=True)

    try:
        compose("up", "-d", "--build", "--wait")
        wait_ready(200)
        bootstrap("alice")
        try:
            bootstrap("second")
        except CalledProcessError:
            pass
        else:
            raise AssertionError("Création d'un deuxième compte par amorçage")
        assert query("SELECT count(*) FROM users") == "1"
        assert query("SELECT password_hash FROM users").startswith("$argon2id$v=19$m=19456,t=2,p=1$")

        a, b = client(), client()
        call(a, "/me", 401)
        for source in (None, origin + ".evil"):
            call(a, "/auth/login", 403, method="POST", data=login, source=source)
        invalid = []
        for handle in ("alice", "unknown"):
            _, error = call(a, "/auth/login", 401, method="POST",
                            data={"handle": handle, "password": "incorrect"})
            invalid.append(error)
        assert invalid[0] == invalid[1]

        cookie_a, user = call(a, "/auth/login", 200, method="POST", data=login)
        cookie_b, same_user = call(b, "/auth/login", 200, method="POST", data=login)
        assert user == same_user and set(user) == {"id", "handle", "display_name"}
        assert user["handle"] == "alice" and user["display_name"] == "Alice"
        assert cookie_a != cookie_b
        assert {"HttpOnly", "Path=/", "SameSite=Lax", "Max-Age=604800"} <= set(cookie_a.split("; "))
        assert call(a, "/me", 200)[1] == call(b, "/me", 200)[1] == user
        for header in (cookie_a, cookie_b):
            token = bytes.fromhex(header.split(";", 1)[0].split("=", 1)[1])
            assert len(token) == 32
            digest = sha256(token).hexdigest()
            assert query(f"""SELECT count(*) FROM sessions WHERE token_hash=decode('{digest}','hex')
                AND expires_at BETWEEN now() + interval '167 hours' AND now() + interval '168 hours'""") == "1"

        call(a, "/auth/logout", 403, method="POST", source="https://third.example")
        call(a, "/me", 200)
        cleared, _ = call(a, "/auth/logout", 204, method="POST")
        assert "Max-Age=0" in cleared
        call(a, "/me", 401)
        call(client(), "/me", 401, cookie=cookie_a.split(";", 1)[0])
        assert query("SELECT count(*) FROM sessions") == "1"
        call(b, "/me", 200)
        query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        call(b, "/me", 401)
        call(b, "/auth/logout", 401, method="POST")
        for _ in range(10):
            call(a, "/auth/login", 401, method="POST", data={"handle": "limited", "password": "x"})
        call(a, "/auth/login", 429, method="POST", data={"handle": "limited", "password": "x"})
        print("OK : amorçage, deux sessions, refus d’origine, révocation et expiration.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()


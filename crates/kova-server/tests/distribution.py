"""Production Compose smoke test for TLS, secrets, and database recovery."""

import http.client
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import sys
import tempfile
import time
from unittest.mock import patch
from urllib.error import URLError
from uuid import uuid4


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "deployment"))
from backup import create_backup, restore_backup

COMPOSE = ROOT / "deployment" / "compose.production.yaml"


def compose_prefix(project, environment):
    files = [COMPOSE]
    overlay = environment.get("KOVA_TEST_COMPOSE_OVERLAY")
    if overlay:
        files.append(Path(overlay))
    prefix = ["docker", "compose", "--project-name", project]
    for compose_file in files:
        prefix.extend(("--file", str(compose_file)))
    return prefix


def command(project, environment, *arguments, capture=False, binary=False):
    return subprocess.run(
        [*compose_prefix(project, environment), *arguments],
        check=True,
        env=environment,
        capture_output=capture,
        text=capture and not binary,
    ).stdout


def mapped_port(project, environment, container_port):
    mapping = command(
        project, environment, "port", "proxy", str(container_port), capture=True
    ).strip()
    return int(mapping.rsplit(":", 1)[1])


def free_ports(count):
    listeners = []
    try:
        for _ in range(count):
            listener = socket.socket()
            listener.bind(("127.0.0.1", 0))
            listeners.append(listener)
        return tuple(listener.getsockname()[1] for listener in listeners)
    finally:
        for listener in listeners:
            listener.close()


def sql(project, environment, statement, database="kova"):
    return command(
        project,
        environment,
        "exec",
        "--no-TTY",
        "db",
        "psql",
        "--no-psqlrc",
        "--tuples-only",
        "--no-align",
        "--set=ON_ERROR_STOP=1",
        "--username=kova",
        f"--dbname={database}",
        f"--command={statement}",
        capture=True,
    ).strip()


def container_environment(project, environment):
    container_id = command(project, environment, "ps", "--quiet", "api", capture=True).strip()
    result = subprocess.run(
        ["docker", "inspect", "--format", "{{json .Config.Env}}", container_id],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def validate_production_caddy(project, environment):
    subprocess.run(
        [
            "docker",
            "compose",
            "--project-name",
            project,
            "--file",
            str(COMPOSE),
            "run",
            "--rm",
            "--no-deps",
            "proxy",
            "caddy",
            "validate",
            "--config",
            "/etc/caddy/Caddyfile",
            "--adapter",
            "caddyfile",
        ],
        check=True,
        env=environment,
    )


def verify_api_and_database_are_not_published(project, environment):
    for service in ("api", "db"):
        container_id = command(
            project, environment, "ps", "--quiet", service, capture=True
        ).strip()
        result = subprocess.run(
            ["docker", "inspect", "--format", "{{json .NetworkSettings.Ports}}", container_id],
            check=True,
            capture_output=True,
            text=True,
        )
        published_ports = json.loads(result.stdout)
        assert not any(bindings for bindings in published_ports.values()), (
            service,
            published_ports,
        )


def https_request(port, context, path, *, method="GET", body=None, origin=None):
    connection = http.client.HTTPSConnection("localhost", port, context=context, timeout=5)
    headers = {"Host": f"localhost:{port}"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    if origin is not None:
        headers["Origin"] = origin
    try:
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        return response.status, dict(response.getheaders()), response.read()
    finally:
        connection.close()


def wait_for_proxy(project, environment, http_port, tls_port, secret):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", http_port), timeout=1):
                pass
            root = command(
                project,
                environment,
                "exec",
                "--no-TTY",
                "proxy",
                "cat",
                "/data/caddy/pki/authorities/local/root.crt",
                capture=True,
                binary=True,
            )
            context = ssl.create_default_context(cadata=root.decode("ascii"))
            status, headers, _ = https_request(tls_port, context, "/health/ready")
            if status == 200:
                if secret.encode() in root:
                    raise AssertionError("Le secret PostgreSQL apparaît dans le certificat TLS.")
                return context
        except (OSError, subprocess.CalledProcessError, ssl.SSLError, URLError):
            time.sleep(0.5)
    raise RuntimeError("Le proxy HTTPS local ne devient pas disponible.")


def verify_http_redirect(http_port, tls_port):
    connection = http.client.HTTPConnection("127.0.0.1", http_port, timeout=5)
    try:
        connection.request("GET", "/health/ready", headers={"Host": "localhost"})
        response = connection.getresponse()
        location = response.getheader("Location")
        assert response.status in (301, 308), (response.status, location)
        assert location == f"https://localhost:{tls_port}/health/ready", location
    finally:
        connection.close()


def verify_https_api(port, context):
    status, headers, _ = https_request(port, context, "/health/ready")
    assert status == 200, status

    status, headers, _ = https_request(port, context, "/api/v1/me")
    assert status == 401, status
    assert headers.get("Cache-Control") == "no-store", headers

    status, headers, _ = https_request(
        port,
        context,
        "/api/v1/auth/challenges",
        method="POST",
        body=b"{}",
        origin="https://attacker.invalid",
    )
    assert status == 403, status
    assert headers.get("Cache-Control") == "no-store", headers


def verify_secret_is_file_only(project, environment, secret):
    values = container_environment(project, environment)
    assert not any(value.startswith("PGPASSWORD=") for value in values), values
    assert "KOVA_DATABASE_PASSWORD_FILE=/run/secrets/database_password" in values, values
    assert secret not in "\n".join(values), "Le secret PostgreSQL apparaît dans l’environnement API."


def verify_backup_restore(project, environment, temporary_directory, tls_port, context):
    profile_id = str(uuid4())
    handle = "distribution_" + uuid4().hex[:12]
    sql(
        project,
        environment,
        "INSERT INTO users(id, handle, display_name) "
        f"VALUES ('{profile_id}', '{handle}', 'Backup smoke test')",
    )
    command(project, environment, "restart", "api")
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            status, _, _ = https_request(
                tls_port,
                context,
                "/health/ready",
            )
            if status == 200:
                break
        except (OSError, ssl.SSLError):
            pass
        time.sleep(0.5)
    else:
        raise RuntimeError("L’API ne redevient pas disponible après redémarrage.")
    assert sql(
        project,
        environment,
        f"SELECT id FROM users WHERE handle='{handle}'",
    ) == profile_id
    migration_checksums = sql(
        project,
        environment,
        "SELECT version || ':' || encode(checksum, 'hex') "
        "FROM _sqlx_migrations ORDER BY version",
    )

    archive = Path(temporary_directory) / "production.dump"
    compose_file = COMPOSE
    with patch.dict(os.environ, environment):
        create_backup(project, compose_file, archive)
        assert archive.stat().st_size > 0
        try:
            create_backup(project, compose_file, archive)
        except FileExistsError:
            pass
        else:
            raise AssertionError("Une sauvegarde a écrasé un fichier existant.")

        sql(project, environment, f"DELETE FROM users WHERE id='{profile_id}'")
        restored = "restore_" + uuid4().hex[:16]
        restore_backup(project, compose_file, archive, restored)
        assert sql(
            project,
            environment,
            f"SELECT id FROM users WHERE handle='{handle}'",
            restored,
        ) == profile_id
        assert sql(
            project,
            environment,
            "SELECT version || ':' || encode(checksum, 'hex') "
            "FROM _sqlx_migrations ORDER BY version",
            restored,
        ) == migration_checksums

        try:
            restore_backup(project, compose_file, archive, restored)
        except subprocess.CalledProcessError:
            pass
        else:
            raise AssertionError("La restauration a remplacé une base déjà existante.")
        assert sql(
            project,
            environment,
            f"SELECT id FROM users WHERE handle='{handle}'",
            restored,
        ) == profile_id


def run():
    project = "kova-distribution-test-" + uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix="kova-distribution-") as temporary_directory:
        temporary = Path(temporary_directory)
        secret = secrets.token_urlsafe(32)
        secret_file = temporary / "database-password"
        secret_file.write_text(secret, encoding="ascii")
        if os.name != "nt":
            secret_file.chmod(0o644)

        caddyfile = temporary / "Caddyfile"
        http_port, tls_port = free_ports(2)
        expected_http_port, expected_tls_port = http_port, tls_port
        caddyfile.write_text(
            f"""http://localhost {{
    redir https://localhost:{tls_port}{{uri}} 308
}}

https://localhost {{
    tls internal
    reverse_proxy api:8080
}}
""",
            encoding="utf-8",
        )
        overlay = temporary / "compose.test.yaml"
        image = os.environ.get("KOVA_TEST_DISTRIBUTION_IMAGE")
        image_override = f"    image: {json.dumps(image)}\n" if image else ""
        overlay.write_text(
            "services:\n"
            "  proxy:\n"
            "    ports: !override\n"
            f"      - \"127.0.0.1:{http_port}:80\"\n"
            f"      - \"127.0.0.1:{tls_port}:443\"\n"
            "    volumes:\n"
            f"      - {json.dumps(str(caddyfile) + ':/etc/caddy/Caddyfile:ro')}\n"
            "  api:\n"
            "    environment:\n"
            f"      KOVA_ORIGIN: https://localhost:{tls_port}\n"
            f"{image_override}",
            encoding="utf-8",
        )
        environment = dict(
            os.environ,
            KOVA_DOMAIN="localhost",
            ACME_EMAIL="distribution-test@example.invalid",
            KOVA_DATABASE_PASSWORD_HOST_FILE=str(secret_file),
            KOVA_TEST_COMPOSE_OVERLAY=str(overlay),
        )

        def test_compose(*arguments, capture=False, binary=False):
            return subprocess.run(
                [*compose_prefix(project, environment), *arguments],
                check=True,
                env=environment,
                capture_output=capture,
                text=capture and not binary,
            ).stdout

        try:
            validate_production_caddy(project, environment)
            if image:
                test_compose("up", "--detach", "--no-build", "--wait", "proxy")
            else:
                test_compose("up", "--detach", "--build", "--wait", "proxy")
            http_port = mapped_port(project, environment, 80)
            tls_port = mapped_port(project, environment, 443)
            assert (http_port, tls_port) == (expected_http_port, expected_tls_port)
            context = wait_for_proxy(project, environment, http_port, tls_port, secret)
            verify_api_and_database_are_not_published(project, environment)
            verify_http_redirect(http_port, tls_port)
            verify_https_api(tls_port, context)
            verify_secret_is_file_only(project, environment, secret)
            verify_backup_restore(project, environment, temporary, tls_port, context)
            print("OK: production Compose TLS, Origin policy, file secret and backup restore.")
        finally:
            test_compose("down", "--volumes", "--remove-orphans")


if __name__ == "__main__":
    run()

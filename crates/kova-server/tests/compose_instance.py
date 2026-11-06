"""Instance de test Compose isolée avec volume et ports dédiés."""
import os
from pathlib import Path
import secrets
import subprocess
import time
from urllib.error import HTTPError, URLError
from urllib.request import urlopen
from uuid import uuid4


COMPOSE = Path(__file__).resolve().parents[3] / "deployment" / "compose.yaml"


def instance(api_port="0"):
    project = "kova-test-" + uuid4().hex
    env = dict(os.environ, POSTGRES_PASSWORD=secrets.token_urlsafe(32),
               KOVA_API_PORT=api_port, KOVA_DB_PORT="0", KOVA_ORIGIN=f"http://127.0.0.1:{api_port}")

    def compose(*args, sql=None, capture=False):
        return subprocess.run(
            ["docker", "compose", "-p", project, "-f", str(COMPOSE), *args],
            input=sql, env=env, check=True, encoding="utf-8", capture_output=capture,
        ).stdout

    def query(sql):
        return compose("exec", "-T", "db", "psql", "-XAtq", "-v", "ON_ERROR_STOP=1",
                       "-U", "kova", "-d", "kova", sql=sql, capture=True).strip()

    def wait_ready(expected):
        address = compose("port", "api", "8080", capture=True).strip()
        url = f"http://{address}/health/ready"
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            try:
                with urlopen(url, timeout=4) as response:
                    status = response.status
            except HTTPError as error:
                status = error.code
            except (URLError, TimeoutError):
                status = None
            if status == expected:
                return
            time.sleep(0.5)
        raise RuntimeError(f"{url} : attendu {expected}, obtenu {status}")

    return compose, query, wait_ready

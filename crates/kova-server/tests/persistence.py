"""Test de persistance sur un projet Compose jetable. Exécuter avec Python 3."""
from hashlib import sha384
from pathlib import Path
from uuid import uuid4

from compose_instance import instance


def main():
    compose, query, wait_ready = instance()
    user, community, channel, client = (uuid4() for _ in range(4))
    try:
        compose("up", "-d", "--wait", "db")
        initial = (Path(__file__).resolve().parents[1] / "migrations/0001_initial.sql").read_bytes()
        query(initial.decode())
        query(f"""
            BEGIN;
            CREATE TABLE _sqlx_migrations (
                version bigint PRIMARY KEY, description text NOT NULL,
                installed_on timestamptz NOT NULL DEFAULT now(), success boolean NOT NULL,
                checksum bytea NOT NULL, execution_time bigint NOT NULL
            );
            INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time)
                VALUES (1,'initial',true,decode('{sha384(initial).hexdigest()}','hex'),0);
            INSERT INTO users (id, handle, display_name, password_hash)
                VALUES ('{user}', 'alice', 'Alice','legacy');
            INSERT INTO sessions VALUES (decode('abcd','hex'),'{user}',now()+interval '1 day');
            INSERT INTO communities VALUES ('{community}', 'Atelier', '{user}');
            INSERT INTO memberships VALUES ('{community}', '{user}', 'member');
            INSERT INTO channels VALUES ('{channel}', '{community}', 'general', 1);
            INSERT INTO messages (channel_id, seq, author_id, client_id, body)
                VALUES ('{channel}', 1, '{user}', '{client}', 'Bonjour Kova');
            COMMIT;
        """)
        compose("up", "--build", "-d", "--wait")
        wait_ready(200)
        assert query("SELECT count(*) FROM sessions") == "0"
        assert query("SELECT count(*) FROM users WHERE global_identity_id IS NULL") == "1"
        compose("exec", "-T", "api", "kova-server", "bootstrap-admin", "alice", "Alice",
                "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        assert query("SELECT id FROM users WHERE is_admin AND global_identity_id IS NOT NULL") == str(user)
        compose("stop", "db")
        wait_ready(503)
        compose("down")  # conserver le volume, recréer les deux conteneurs
        compose("up", "-d", "--wait")
        wait_ready(200)
        saved = query(f"SELECT body FROM messages WHERE channel_id='{channel}' AND seq=1")
        if saved != "Bonjour Kova":
            raise RuntimeError(f"Message relu inattendu : {saved!r}")
        if query("SELECT version, success FROM _sqlx_migrations ORDER BY version") != "1|t\n2|t\n3|t\n4|t":
            raise RuntimeError("Historique de migration inattendu")
        print("OK : disponibilité, migrations et message conservé après recréation.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()

"""Test de persistance sur un projet Compose jetable. Exécuter avec Python 3."""
from uuid import uuid4

from compose_instance import instance


def main():
    compose, query, wait_ready = instance()
    user, community, channel, client = (uuid4() for _ in range(4))
    try:
        compose("up", "--build", "-d", "--wait")
        wait_ready(200)
        query(f"""
            BEGIN;
            INSERT INTO users (id, handle, display_name, password_hash)
                VALUES ('{user}', 'alice', 'Alice', 'test');
            INSERT INTO communities VALUES ('{community}', 'Atelier', '{user}');
            INSERT INTO memberships VALUES ('{community}', '{user}', 'member');
            INSERT INTO channels VALUES ('{channel}', '{community}', 'general', 1);
            INSERT INTO messages (channel_id, seq, author_id, client_id, body)
                VALUES ('{channel}', 1, '{user}', '{client}', 'Bonjour Kova');
            COMMIT;
        """)
        compose("stop", "db")
        wait_ready(503)
        compose("down")  # conserver le volume, recréer les deux conteneurs
        compose("up", "-d", "--wait")
        wait_ready(200)
        saved = query(f"SELECT body FROM messages WHERE channel_id='{channel}' AND seq=1")
        if saved != "Bonjour Kova":
            raise RuntimeError(f"Message relu inattendu : {saved!r}")
        if query("SELECT version, success FROM _sqlx_migrations") != "1|t":
            raise RuntimeError("Historique de migration inattendu")
        print("OK : disponibilité, migration unique et message conservé après recréation.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()

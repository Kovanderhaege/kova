"""Restore a full PostgreSQL dump into another database on an isolated instance."""
from uuid import uuid4
from compose_instance import instance


def main():
    compose, query, ready = instance()
    profile = str(uuid4())
    try:
        compose("up", "-d", "--build", "--wait")
        ready(200)
        query(f"INSERT INTO users(id,handle,display_name) VALUES ('{profile}','restore_test','Restoration test')")
        before = query("SELECT version,encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version")
        dump = compose("exec", "-T", "db", "pg_dump", "-U", "kova", "-d", "kova",
                       "--no-owner", "--no-acl", capture=True)
        compose("exec", "-T", "db", "createdb", "-U", "kova", "kova_restored")
        compose("exec", "-T", "db", "psql", "-Xq", "-v", "ON_ERROR_STOP=1", "-U", "kova",
                "-d", "kova_restored", sql=dump, capture=True)

        def restored(sql):
            return compose("exec", "-T", "db", "psql", "-XAtq", "-v", "ON_ERROR_STOP=1",
                           "-U", "kova", "-d", "kova_restored", sql=sql, capture=True).strip()

        assert restored("SELECT id FROM users WHERE handle='restore_test'") == profile
        assert restored("SELECT version,encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version") == before
        assert restored("SELECT count(*) FROM information_schema.columns WHERE table_name='devices' AND column_name='authorization_signature'") == "1"
        query("DELETE FROM users WHERE handle='restore_test'")
        assert restored("SELECT count(*) FROM users WHERE handle='restore_test'") == "1"
        print("OK: full SQL backup restored independently, profile and migration checksums preserved.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()

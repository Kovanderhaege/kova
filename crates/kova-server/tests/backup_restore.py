"""Restore a full PostgreSQL dump into another database on an isolated instance."""
from uuid import uuid4
from compose_instance import instance


def main():
    compose, query, ready = instance()
    profile = str(uuid4())
    excluded, community, channel = (str(uuid4()) for _ in range(3))
    try:
        compose("up", "-d", "--build", "--wait")
        ready(200)
        query(f"INSERT INTO users(id,handle,display_name) VALUES ('{profile}','restore_test','Restoration test')")
        query(f"""BEGIN;
            INSERT INTO users(id,handle,display_name) VALUES ('{excluded}','excluded_test','Excluded');
            INSERT INTO communities(id,name,owner_id) VALUES ('{community}','Restored community','{profile}');
            INSERT INTO memberships(community_id,user_id) VALUES ('{community}','{profile}');
            INSERT INTO channels(id,community_id,name) VALUES ('{channel}','{community}','general');
            INSERT INTO community_exclusions(community_id,user_id,excluded_by) VALUES ('{community}','{excluded}','{profile}');
            COMMIT;""")
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
        assert restored(f"SELECT excluded_by FROM community_exclusions WHERE community_id='{community}' AND user_id='{excluded}'") == profile
        assert restored(f"SELECT count(*) FROM memberships WHERE community_id='{community}' AND user_id='{excluded}'") == "0"
        assert restored(f"SELECT community_id FROM channels WHERE id='{channel}'") == community
        query(f"DELETE FROM communities WHERE id='{community}'")
        query("DELETE FROM users WHERE handle='restore_test'")
        assert restored("SELECT count(*) FROM users WHERE handle='restore_test'") == "1"
        assert restored("SELECT count(*) FROM community_exclusions") == "1"
        print("OK: full SQL backup restored independently, community exclusions, profile and migration checksums preserved.")
    finally:
        compose("down", "--volumes", "--rmi", "local")


if __name__ == "__main__":
    main()

-- Proposition de migration initiale. UUID générés par le serveur sauf client_id.
BEGIN;
CREATE TABLE users (
    id uuid PRIMARY KEY,
    handle text UNIQUE NOT NULL CHECK (handle ~ '^[a-z0-9_]{3,32}$'),
    display_name text NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 64),
    password_hash text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX sessions_expiry ON sessions(expires_at);
CREATE TABLE communities (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    owner_id uuid NOT NULL REFERENCES users(id)
);
CREATE TABLE memberships (
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id),
    role text NOT NULL DEFAULT 'member' CHECK (role IN ('member', 'moderator')),
    PRIMARY KEY (community_id, user_id)
);
ALTER TABLE communities ADD FOREIGN KEY (id, owner_id)
    REFERENCES memberships(community_id, user_id) DEFERRABLE INITIALLY DEFERRED;
CREATE INDEX memberships_by_user ON memberships(user_id, community_id);
CREATE TABLE invites (
    token_hash bytea PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE TABLE channels (
    id uuid PRIMARY KEY,
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    last_seq bigint NOT NULL DEFAULT 0,
    UNIQUE (community_id, name)
);
CREATE TABLE messages (
    channel_id uuid NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    seq bigint NOT NULL CHECK (seq > 0),
    author_id uuid NOT NULL REFERENCES users(id),
    client_id uuid NOT NULL,
    body text NOT NULL CHECK (length(body) BETWEEN 1 AND 4000 AND length(trim(body)) > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, seq),
    UNIQUE (channel_id, author_id, client_id)
);
COMMIT;

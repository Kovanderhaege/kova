-- Documentation du schéma réellement appliqué : migrations 0001 à 0003.
-- Source de vérité : crates/kova-server/migrations.

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

-- Existing profile UUIDs continue to own their memberships and messages.
CREATE TABLE identities (
    id text PRIMARY KEY,
    root_public_key bytea UNIQUE NOT NULL CHECK (octet_length(root_public_key) = 32)
);
ALTER TABLE users DROP COLUMN password_hash,
    ADD COLUMN global_identity_id text UNIQUE REFERENCES identities(id),
    ADD COLUMN is_admin boolean NOT NULL DEFAULT false;
CREATE UNIQUE INDEX first_admin ON users (is_admin) WHERE is_admin;

CREATE TABLE devices (
    identity_id text NOT NULL REFERENCES identities(id),
    device_id text NOT NULL,
    public_key bytea NOT NULL CHECK (octet_length(public_key) = 32),
    revoked_at timestamptz,
    PRIMARY KEY (identity_id, device_id)
);

DELETE FROM sessions;
ALTER TABLE users ADD UNIQUE (id, global_identity_id);
ALTER TABLE sessions ADD COLUMN identity_id text NOT NULL,
    ADD COLUMN device_id text NOT NULL,
    ADD FOREIGN KEY (user_id, identity_id) REFERENCES users(id, global_identity_id),
    ADD FOREIGN KEY (identity_id, device_id) REFERENCES devices(identity_id, device_id);

CREATE TABLE auth_challenges (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    identity_id text NOT NULL,
    device_id text NOT NULL,
    operation text NOT NULL CHECK (operation IN ('login','join')),
    origin text NOT NULL,
    invite_hash bytea,
    nonce text NOT NULL,
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '2 minutes',
    CHECK ((operation = 'join') = (invite_hash IS NOT NULL))
);
CREATE INDEX auth_challenges_expiry ON auth_challenges(expires_at);

-- Preserve legacy sessions and tombstones; unknown historic proofs stay NULL.
ALTER TABLE auth_challenges ADD COLUMN session_mode text NOT NULL DEFAULT 'cookie'
    CHECK (session_mode IN ('cookie', 'bearer'));
ALTER TABLE sessions ADD COLUMN transport text NOT NULL DEFAULT 'cookie'
    CHECK (transport IN ('cookie', 'bearer'));
ALTER TABLE devices
    ADD COLUMN authorization_payload text,
    ADD COLUMN authorization_signature bytea,
    ADD COLUMN authorization_expires_at bigint,
    ADD COLUMN revocation_signature bytea,
    ADD CONSTRAINT authorization_complete CHECK (
      (authorization_payload IS NULL AND authorization_signature IS NULL AND authorization_expires_at IS NULL)
      OR (authorization_payload IS NOT NULL AND authorization_signature IS NOT NULL
          AND authorization_expires_at IS NOT NULL AND octet_length(authorization_signature)=64 AND authorization_expires_at BETWEEN 1 AND 253402300799)),
    ADD CONSTRAINT revocation_proof_size CHECK (revocation_signature IS NULL OR octet_length(revocation_signature)=64);

-- Bound freshness: legacy sessions may outlive a new 24-hour authorization.
DELETE FROM sessions;
CREATE TABLE identity_revocations (
    identity_id text NOT NULL REFERENCES identities(id),
    device_id text NOT NULL,
    event_id text UNIQUE NOT NULL,
    payload text NOT NULL,
    signature bytea NOT NULL CHECK (octet_length(signature)=64),
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(identity_id,device_id)
);
-- Preserve verified historical revocations, without fabricating missing signatures.
INSERT INTO identity_revocations(identity_id,device_id,event_id,payload,signature,received_at)
SELECT identity_id,device_id,encode(sha256(convert_to(payload,'UTF8')),'hex'),payload,revocation_signature,revoked_at
FROM (SELECT *, 'kova-revoke-v1' || chr(10) || identity_id || chr(10) || device_id AS payload
      FROM devices WHERE revoked_at IS NOT NULL AND revocation_signature IS NOT NULL) AS historical;

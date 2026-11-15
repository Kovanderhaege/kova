-- Documentation du schéma réellement appliqué : migrations 0001 à 0007.
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

-- Migration 0005 : exclusions communautaires persistantes.
CREATE TABLE community_exclusions (
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id),
    excluded_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, user_id)
);

-- Migration 0006 : compteurs du chat public.
-- Historical fixtures/imports could leave last_seq at zero. Preserve message numbers.
-- Refuse a non-contiguous historical log rather than silently renumbering messages.
DO $$ BEGIN
    IF EXISTS (SELECT channel_id FROM messages GROUP BY channel_id HAVING count(*) <> max(seq)) THEN
        RAISE EXCEPTION 'Non-contiguous channel history: repair the import before upgrading';
    END IF;
END $$;
UPDATE channels c SET last_seq = COALESCE((SELECT max(seq) FROM messages WHERE channel_id=c.id),0);
ALTER TABLE channels ADD CONSTRAINT channels_nonnegative_seq CHECK (last_seq >= 0);


-- Migration 0007: direct message envelopes and one-use device packages.
-- The first DM milestone stores opaque, signed MLS envelopes on one instance.
-- Cursors below are local to one conversation and never imply a global order.
CREATE TABLE dm_key_packages (
    identity_id text NOT NULL REFERENCES identities(id),
    device_id text NOT NULL,
    bundle jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (identity_id, device_id),
    FOREIGN KEY (identity_id, device_id) REFERENCES devices(identity_id, device_id)
);

CREATE TABLE dm_conversations (
    id uuid PRIMARY KEY,
    origin text NOT NULL,
    creator_identity text NOT NULL REFERENCES identities(id),
    creator_device text NOT NULL,
    recipient_identity text NOT NULL REFERENCES identities(id),
    recipient_device text NOT NULL,
    manifest jsonb NOT NULL,
    last_cursor bigint NOT NULL DEFAULT 0 CHECK (last_cursor >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (creator_identity <> recipient_identity),
    FOREIGN KEY (creator_identity, creator_device) REFERENCES devices(identity_id, device_id),
    FOREIGN KEY (recipient_identity, recipient_device) REFERENCES devices(identity_id, device_id)
);

CREATE INDEX dm_conversations_creator ON dm_conversations(creator_identity, creator_device);
CREATE INDEX dm_conversations_recipient ON dm_conversations(recipient_identity, recipient_device);

CREATE TABLE dm_used_key_packages (
    package_hash text PRIMARY KEY CHECK (package_hash ~ '^[0-9a-f]{64}$'),
    identity_id text NOT NULL,
    device_id text NOT NULL,
    conversation_id uuid NOT NULL REFERENCES dm_conversations(id) ON DELETE CASCADE,
    consumed_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (identity_id, device_id) REFERENCES devices(identity_id, device_id)
);

CREATE TABLE dm_events (
    conversation_id uuid NOT NULL REFERENCES dm_conversations(id) ON DELETE CASCADE,
    cursor bigint NOT NULL CHECK (cursor > 0),
    event_id uuid NOT NULL,
    author_identity text NOT NULL,
    author_device text NOT NULL,
    parents uuid[] NOT NULL DEFAULT '{}',
    ciphertext bytea NOT NULL CHECK (octet_length(ciphertext) BETWEEN 1 AND 16384),
    signature bytea NOT NULL CHECK (octet_length(signature) = 64),
    event jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (conversation_id, cursor),
    UNIQUE (conversation_id, event_id)
);

CREATE FUNCTION check_dm_event_participant() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT FROM dm_conversations c
        WHERE c.id = NEW.conversation_id
          AND ((c.creator_identity, c.creator_device) = (NEW.author_identity, NEW.author_device)
            OR (c.recipient_identity, c.recipient_device) = (NEW.author_identity, NEW.author_device))
    ) THEN
        RAISE EXCEPTION 'DM event author is not a conversation participant';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER dm_event_participant BEFORE INSERT OR UPDATE ON dm_events
FOR EACH ROW EXECUTE FUNCTION check_dm_event_participant();

-- Migration 0008: replica policies and synchronization
-- Pending proposals and activated bilateral replica policies.
CREATE TABLE dm_replica_policies (
    conversation_id uuid NOT NULL REFERENCES dm_conversations(id) ON DELETE CASCADE,
    version bigint NOT NULL CHECK (version > 0),
    policy_hash text NOT NULL CHECK (policy_hash ~ '^[0-9a-f]{64}$'),
    policy jsonb NOT NULL,
    active boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (conversation_id, version)
);
CREATE UNIQUE INDEX dm_replica_single_active_policy
    ON dm_replica_policies(conversation_id) WHERE active;

CREATE TABLE dm_replica_cursors (
    conversation_id uuid NOT NULL REFERENCES dm_conversations(id) ON DELETE CASCADE,
    origin text NOT NULL,
    received_cursor bigint NOT NULL DEFAULT 0 CHECK (received_cursor >= 0),
    acknowledged_cursor bigint NOT NULL DEFAULT 0 CHECK (acknowledged_cursor >= 0),
    PRIMARY KEY (conversation_id, origin)
);

CREATE TABLE dm_federation_nonces (
    peer_key text NOT NULL CHECK (peer_key ~ '^[0-9a-f]{64}$'),
    nonce text NOT NULL CHECK (nonce ~ '^[0-9a-f]{32}$'),
    used_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (peer_key, nonce)
);
CREATE INDEX dm_federation_nonces_used_at ON dm_federation_nonces(used_at);

CREATE TABLE dm_replica_conflicts (
    conversation_id uuid PRIMARY KEY REFERENCES dm_conversations(id) ON DELETE CASCADE,
    conflicting_hash text NOT NULL CHECK (conflicting_hash ~ '^[0-9a-f]{64}$'),
    detected_at timestamptz NOT NULL DEFAULT now()
);

-- 0009_dm_authorizations.sql
ALTER TABLE dm_conversations
    ADD COLUMN authorization_state jsonb NOT NULL
        DEFAULT '{"authorizations":[],"revocations":[]}'::jsonb;

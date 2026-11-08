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

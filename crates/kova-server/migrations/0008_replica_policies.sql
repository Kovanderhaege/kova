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

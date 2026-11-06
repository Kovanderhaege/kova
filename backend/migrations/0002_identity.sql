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

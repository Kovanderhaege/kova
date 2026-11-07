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

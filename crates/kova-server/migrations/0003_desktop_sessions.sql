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

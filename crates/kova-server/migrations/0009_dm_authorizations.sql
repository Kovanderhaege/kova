ALTER TABLE dm_conversations
    ADD COLUMN authorization_state jsonb NOT NULL
        DEFAULT '{"authorizations":[],"revocations":[]}'::jsonb;

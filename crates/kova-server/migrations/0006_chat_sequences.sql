-- Historical fixtures/imports could leave last_seq at zero. Preserve message numbers.
-- Refuse a non-contiguous historical log rather than silently renumbering messages.
DO $$ BEGIN
    IF EXISTS (SELECT channel_id FROM messages GROUP BY channel_id HAVING count(*) <> max(seq)) THEN
        RAISE EXCEPTION 'Non-contiguous channel history: repair the import before upgrading';
    END IF;
END $$;
UPDATE channels c SET last_seq = COALESCE((SELECT max(seq) FROM messages WHERE channel_id=c.id),0);
ALTER TABLE channels ADD CONSTRAINT channels_nonnegative_seq CHECK (last_seq >= 0);

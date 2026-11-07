CREATE TABLE community_exclusions (
    community_id uuid NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id),
    excluded_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, user_id)
);

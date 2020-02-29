CREATE TABLE active_subscriptions (
    id INTEGER NOT NULL PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);

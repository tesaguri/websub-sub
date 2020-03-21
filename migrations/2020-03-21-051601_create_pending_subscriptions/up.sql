CREATE TABLE pending_subscriptions (
    id INTEGER NOT NULL PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL DEFAULT (strftime('%s', 'now'))
);

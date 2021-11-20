CREATE TABLE renewing_subscriptions (
    old INTEGER NOT NULL UNIQUE REFERENCES active_subscriptions(id) ON DELETE CASCADE,
    new INTEGER NOT NULL PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE
);

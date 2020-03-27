CREATE TABLE renewing_subscriptions (
    old BIGINT NOT NULL REFERENCES active_subscriptions(id) ON DELETE CASCADE,
    new BIGINT NOT NULL REFERENCES pending_subscriptions(id) ON DELETE CASCADE,
    PRIMARY KEY (old, new)
);

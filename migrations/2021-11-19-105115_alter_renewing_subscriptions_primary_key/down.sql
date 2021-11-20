CREATE TABLE renewing_subscriptions_new (
    old BIGINT NOT NULL REFERENCES active_subscriptions(id) ON DELETE CASCADE,
    new BIGINT NOT NULL REFERENCES pending_subscriptions(id) ON DELETE CASCADE,
    PRIMARY KEY (old, new)
);

INSERT INTO renewing_subscriptions_new (old, new)
    SELECT old, new FROM renewing_subscriptions;
DROP TABLE renewing_subscriptions;
ALTER TABLE renewing_subscriptions_new RENAME TO renewing_subscriptions;

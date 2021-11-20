CREATE TABLE renewing_subscriptions_new (
    old INTEGER NOT NULL UNIQUE REFERENCES active_subscriptions(id) ON DELETE CASCADE,
    new INTEGER NOT NULL PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE
);

INSERT INTO renewing_subscriptions_new (old, new)
    SELECT old, new FROM renewing_subscriptions;
DROP TABLE renewing_subscriptions;
ALTER TABLE renewing_subscriptions_new RENAME TO renewing_subscriptions;

DROP TABLE pending_subscriptions;

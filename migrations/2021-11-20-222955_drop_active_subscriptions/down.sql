CREATE TABLE active_subscriptions (
    id INTEGER NOT NULL PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);

CREATE TABLE subscriptions_new (
    id INTEGER NOT NULL PRIMARY KEY,
    hub TEXT NOT NULL,
    topic TEXT NOT NULL,
    secret TEXT NOT NULL
);

INSERT INTO subscriptions_new (id, hub, topic, secret)
    SELECT id, hub, topic, secret FROM subscriptions;
INSERT INTO active_subscriptions (id, expires_at)
    SELECT id, expires_at
    FROM subscriptions
    WHERE expires_at IS NOT NULL;
DROP TABLE subscriptions;
ALTER TABLE subscriptions_new RENAME TO subscriptions;

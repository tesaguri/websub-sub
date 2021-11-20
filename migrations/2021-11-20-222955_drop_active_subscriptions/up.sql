CREATE TABLE subscriptions_new (
    id INTEGER NOT NULL PRIMARY KEY,
    hub TEXT NOT NULL,
    topic TEXT NOT NULL,
    secret TEXT NOT NULL,
    expires_at BIGINT
);

INSERT INTO subscriptions_new (id, hub, topic, secret, expires_at)
    SELECT subscriptions.id, hub, topic, secret, expires_at
    FROM subscriptions
    LEFT OUTER JOIN active_subscriptions
        ON subscriptions.id = active_subscriptions.id;
DROP TABLE subscriptions;
ALTER TABLE subscriptions_new RENAME TO subscriptions;

DROP TABLE active_subscriptions;

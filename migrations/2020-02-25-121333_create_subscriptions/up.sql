CREATE TABLE subscriptions (
    id INTEGER NOT NULL PRIMARY KEY,
    hub TEXT NOT NULL,
    topic TEXT NOT NULL,
    secret TEXT NOT NULL,
    UNIQUE (hub, topic)
);

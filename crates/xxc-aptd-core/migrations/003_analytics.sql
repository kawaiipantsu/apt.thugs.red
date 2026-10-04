CREATE TABLE analytics_settings (
    id INTEGER PRIMARY KEY CHECK(id=1),
    secret BLOB NOT NULL CHECK(length(secret)=32),
    started INTEGER NOT NULL
);
CREATE TABLE analytics_daily (
    day INTEGER NOT NULL,
    kind TEXT NOT NULL,
    requests INTEGER NOT NULL DEFAULT 0,
    downloads INTEGER NOT NULL DEFAULT 0,
    ranges INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    not_modified INTEGER NOT NULL DEFAULT 0,
    interrupted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(day,kind)
) WITHOUT ROWID;
CREATE TABLE analytics_clients (
    day INTEGER NOT NULL,
    client BLOB NOT NULL CHECK(length(client)=32),
    PRIMARY KEY(day,client)
) WITHOUT ROWID;
CREATE TABLE analytics_assets (
    day INTEGER NOT NULL,
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    package TEXT NOT NULL,
    requests INTEGER NOT NULL DEFAULT 0,
    downloads INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(day,path)
) WITHOUT ROWID;
PRAGMA user_version = 3;

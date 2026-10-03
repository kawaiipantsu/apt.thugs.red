CREATE TABLE packages (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    architecture TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('uploaded','staged','published')),
    active INTEGER NOT NULL DEFAULT 0,
    metadata TEXT NOT NULL,
    UNIQUE(name, version, architecture)
);
CREATE INDEX packages_active ON packages(active,name);
CREATE VIRTUAL TABLE package_search USING fts5(id UNINDEXED, name, description);
CREATE TABLE generations (
    id TEXT PRIMARY KEY,
    created TEXT NOT NULL,
    manifest TEXT NOT NULL
);
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK(state IN ('running','succeeded','failed')),
    created TEXT NOT NULL,
    finished TEXT,
    message TEXT NOT NULL,
    actor TEXT NOT NULL
);
CREATE TABLE audit (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    request_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    interface TEXT NOT NULL,
    action TEXT NOT NULL,
    object TEXT NOT NULL,
    result TEXT NOT NULL
);
PRAGMA user_version = 1;

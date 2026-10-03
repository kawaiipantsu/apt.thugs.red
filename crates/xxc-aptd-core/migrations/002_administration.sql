CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('viewer','operator','administrator')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    security_version INTEGER NOT NULL DEFAULT 1,
    created TEXT NOT NULL
);
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id,created);
CREATE INDEX sessions_expiry ON sessions(expires);
CREATE TABLE login_challenges (
    token_hash TEXT PRIMARY KEY,
    expires INTEGER NOT NULL
);
CREATE TABLE login_limits (
    bucket TEXT PRIMARY KEY,
    window_start INTEGER NOT NULL,
    attempts INTEGER NOT NULL
);
CREATE INDEX login_limits_window ON login_limits(window_start);
ALTER TABLE audit ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';
PRAGMA user_version = 2;

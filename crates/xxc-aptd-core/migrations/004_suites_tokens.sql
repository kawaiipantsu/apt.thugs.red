CREATE TABLE package_suites (
    suite TEXT NOT NULL,
    package_id TEXT NOT NULL REFERENCES packages(id),
    state TEXT NOT NULL CHECK(state IN ('uploaded','staged','published')),
    active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0,1)),
    PRIMARY KEY(suite,package_id)
) WITHOUT ROWID;
CREATE INDEX package_suites_package ON package_suites(package_id);
CREATE TABLE api_tokens (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    digest BLOB NOT NULL UNIQUE CHECK(length(digest)=32),
    scopes TEXT NOT NULL,
    suites TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    last_used INTEGER,
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))
);
PRAGMA user_version=4;

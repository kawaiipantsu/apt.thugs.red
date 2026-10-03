use crate::{config::Config, package::Package};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
    audit: PathBuf,
}

impl Database {
    pub fn open(c: &Config) -> Result<Self> {
        let db = Self {
            path: c.paths.database.clone(),
            audit: c.logging.audit_file.clone(),
        };
        let mut conn = db.connect()?;
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= 2,
            "Database schema is newer than this binary; refusing downgrade"
        );
        conn.pragma_update(None, "journal_mode", "WAL")?;
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            tx.commit()?;
        }
        if version < 2 {
            if version > 0 {
                let backup = db
                    .path
                    .with_extension(format!("pre-v2-{}.db", uuid::Uuid::new_v4()));
                let file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&backup)?;
                conn.backup("main", &backup, None)?;
                file.sync_all()?;
                fs::File::open(backup.parent().expect("database parent"))?.sync_all()?;
            }
            let tx = conn.transaction()?;
            tx.execute_batch(include_str!("../migrations/002_administration.sql"))?;
            tx.commit()?;
        }
        conn.execute("UPDATE jobs SET state='failed',finished=?1,message='Interrupted by service restart' WHERE state='running'",[crate::repository::now()])?;
        Ok(db)
    }
    pub fn connect(&self) -> Result<Connection> {
        let c = Connection::open(&self.path)?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.pragma_update(None, "foreign_keys", true)?;
        c.pragma_update(None, "synchronous", "FULL")?;
        Ok(c)
    }
    pub fn get(&self, id: &str) -> Result<Option<Package>> {
        let value: Option<String> = self
            .connect()?
            .query_row("SELECT metadata FROM packages WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .transpose()
    }
    pub fn list(&self, public: bool, query: &str, page: u32) -> Result<Vec<Package>> {
        self.list_filtered(public, query, page, "")
    }
    pub fn list_filtered(
        &self,
        public: bool,
        query: &str,
        page: u32,
        state: &str,
    ) -> Result<Vec<Package>> {
        let conn = self.connect()?;
        let term = query
            .split_whitespace()
            .take(10)
            .map(|x| format!("\"{}\"", x.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = if query.trim().is_empty() {
            "SELECT metadata FROM packages WHERE (?1=0 OR active=1) AND (?4='' OR state=?4) AND ?2=?2 ORDER BY name,version,architecture LIMIT 50 OFFSET ?3"
        } else {
            "SELECT metadata FROM packages WHERE (?1=0 OR active=1) AND (?4='' OR state=?4) AND id IN (SELECT id FROM package_search WHERE package_search MATCH ?2) ORDER BY name,version,architecture LIMIT 50 OFFSET ?3"
        };
        let mut s = conn.prepare(sql)?;
        let rows = s.query_map(
            params![public, term, i64::from(page.min(1_000_000)) * 50, state],
            |r| r.get::<_, String>(0),
        )?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn selected(&self) -> Result<Vec<Package>> {
        let c = self.connect()?;
        let mut s=c.prepare("SELECT metadata FROM packages WHERE active=1 OR state='staged' ORDER BY name,version,architecture")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn insert(&self, p: &Package) -> Result<String> {
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT id,sha256 FROM packages WHERE name=?1 AND version=?2 AND architecture=?3",
                params![p.name, p.version, p.architecture],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, hash)) = existing {
            ensure!(
                hash == p.sha256,
                "Conflicting package identity: identical name/version/architecture has different bytes"
            );
            return Ok(id);
        }
        tx.execute("INSERT INTO packages(id,name,version,architecture,sha256,state,metadata) VALUES(?1,?2,?3,?4,?5,'uploaded',?6)",params![p.id,p.name,p.version,p.architecture,p.sha256,serde_json::to_string(p)?])?;
        tx.execute(
            "INSERT INTO package_search(id,name,description) VALUES(?1,?2,?3)",
            params![p.id, p.name, p.description],
        )?;
        tx.commit()?;
        Ok(p.id.clone())
    }
    pub fn stage(&self, id: &str) -> Result<()> {
        ensure!(
            self.connect()?.execute(
                "UPDATE packages SET state='staged' WHERE id=?1 AND active=0",
                [id]
            )? == 1,
            "Package missing or already published"
        );
        Ok(())
    }
    pub fn audit(
        &self,
        request: &str,
        actor: &str,
        action: &str,
        object: &str,
        result: &str,
    ) -> Result<()> {
        self.audit_event(
            &crate::auth::Actor {
                id: actor.into(),
                interface: "unix".into(),
                request_id: request.into(),
            },
            action,
            object,
            result,
            &json!({}),
        )
    }
    pub fn audit_event(
        &self,
        actor: &crate::auth::Actor,
        action: &str,
        object: &str,
        result: &str,
        metadata: &Value,
    ) -> Result<()> {
        let event = Self::record_audit(&self.connect()?, actor, action, object, result, metadata)?;
        self.append_audit(&event);
        Ok(())
    }
    pub(crate) fn record_audit(
        conn: &Connection,
        actor: &crate::auth::Actor,
        action: &str,
        object: &str,
        result: &str,
        metadata: &Value,
    ) -> Result<Value> {
        let timestamp = crate::repository::now();
        conn.execute("INSERT INTO audit(timestamp,request_id,actor,interface,action,object,result,metadata) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![timestamp, actor.request_id, actor.id, actor.interface, action, object, result, metadata.to_string()])?;
        Ok(
            json!({"timestamp":timestamp,"request_id":actor.request_id,"actor":actor.id,"interface":actor.interface,"action":action,"object":object,"result":result,"metadata":metadata}),
        )
    }
    pub(crate) fn append_audit(&self, event: &Value) {
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(&self.audit)?;
            writeln!(file, "{event}")
        })();
        if result.is_err() {
            tracing::warn!("audit file unavailable; durable audit retained in SQLite");
        }
    }
    pub fn status(&self) -> Result<Value> {
        let c = self.connect()?;
        let count: i64 = c.query_row("SELECT count(*) FROM packages WHERE active=1", [], |r| {
            r.get(0)
        })?;
        let staged: i64 = c.query_row(
            "SELECT count(*) FROM packages WHERE state='staged'",
            [],
            |r| r.get(0),
        )?;
        Ok(
            json!({"version":crate::VERSION,"packages":count,"staged":staged,"administrative_http":"authentication_required"}),
        )
    }
    pub fn start_job(&self, actor: &crate::auth::Actor, action: &str, id: &str) -> Result<()> {
        let mut c = self.connect()?;
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO jobs(id,state,created,message,actor) VALUES(?1,'running',?2,?3,?4)",
            params![id, crate::repository::now(), action, actor.id],
        )?;
        let event = Self::record_audit(
            &tx,
            actor,
            action,
            "repository",
            "requested",
            &json!({"job_id":id}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(())
    }
    pub fn finish_job(
        &self,
        actor: &crate::auth::Actor,
        action: &str,
        id: &str,
        state: &str,
        message: &str,
    ) -> Result<()> {
        let mut c = self.connect()?;
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE jobs SET state=?1,finished=?2,message=?3 WHERE id=?4",
            params![state, crate::repository::now(), message, id],
        )?;
        let event = Self::record_audit(
            &tx,
            actor,
            action,
            "repository",
            state,
            &json!({"job_id":id}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(())
    }
    pub fn jobs(&self) -> Result<Vec<Value>> {
        let c = self.connect()?;
        let mut s = c.prepare(
            "SELECT id,state,created,finished,message FROM jobs ORDER BY created DESC LIMIT 100",
        )?;
        Ok(s.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"created":r.get::<_,String>(2)?,"finished":r.get::<_,Option<String>>(3)?,"message":r.get::<_,String>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn audits(&self) -> Result<Vec<Value>> {
        let c = self.connect()?;
        let mut s=c.prepare("SELECT timestamp,request_id,actor,action,object,result,interface,metadata FROM audit ORDER BY id DESC LIMIT 100")?;
        Ok(s.query_map([],|r|Ok(json!({"timestamp":r.get::<_,String>(0)?,"request_id":r.get::<_,String>(1)?,"actor":r.get::<_,String>(2)?,"action":r.get::<_,String>(3)?,"object":r.get::<_,String>(4)?,"result":r.get::<_,String>(5)?,"interface":r.get::<_,String>(6)?,"metadata":serde_json::from_str::<Value>(&r.get::<_,String>(7)?).unwrap_or(Value::Null)})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_data_and_leaves_private_recovery_backup() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let config = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let conn = Connection::open(&config.paths.database).unwrap();
        conn.execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        conn.execute("INSERT INTO audit(timestamp,request_id,actor,interface,action,object,result) VALUES('fixture','fixture','fixture','unix','test','test','ok')",[]).unwrap();
        drop(conn);
        let db = Database::open(&config).unwrap();
        assert_eq!(db.audits().unwrap().len(), 1);
        let backup = std::fs::read_dir(config.paths.database.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.to_string_lossy().contains("pre-v2-"))
            .unwrap();
        assert_eq!(
            std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let saved = Connection::open(backup).unwrap();
        assert_eq!(
            saved
                .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
        // An incompatible existing table causes the whole migration to roll back.
        let other = tempfile::tempdir().unwrap();
        let config = Config::initialize(&other.path().join("config"), Some(other.path())).unwrap();
        let conn = Connection::open(&config.paths.database).unwrap();
        conn.execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        conn.execute("CREATE TABLE sessions(incompatible TEXT)", [])
            .unwrap();
        assert!(Database::open(&config).is_err());
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='users'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn search_paginates_and_excludes_unpublished_packages() {
        let root = tempfile::tempdir().unwrap();
        let c = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let db = Database::open(&c).unwrap();
        for index in 0..51 {
            let name = format!("fixture-{index:03}");
            let p = Package {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.clone(),
                version: "1.0".into(),
                architecture: "all".into(),
                source: name,
                component: "main".into(),
                description: "searchable fixture".into(),
                fields: Default::default(),
                filename: "pool/main/f/fixture/fixture.deb".into(),
                size: 0,
                sha256: "fixture".into(),
                sha512: "fixture".into(),
                uploaded: crate::repository::now(),
            };
            db.insert(&p).unwrap();
        }
        assert!(db.list(true, "searchable", 0).unwrap().is_empty());
        db.connect()
            .unwrap()
            .execute("UPDATE packages SET active=1", [])
            .unwrap();
        assert_eq!(db.list(true, "searchable", 0).unwrap().len(), 50);
        assert_eq!(db.list(true, "searchable", 1).unwrap().len(), 1);
        assert!(db.list(true, "searchable", 2).unwrap().is_empty());
        assert!(db.list(true, "\" OR *", 0).is_ok());
    }
    #[test]
    fn refuses_corruption_and_future_schema_without_modifying_them() {
        let root = tempfile::tempdir().unwrap();
        let c = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let db = Database::open(&c).unwrap();
        db.connect()
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        db.connect()
            .unwrap()
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        let original = std::fs::read(&c.paths.database).unwrap();
        assert!(Database::open(&c).is_err());
        assert_eq!(
            std::fs::read(&c.paths.database).unwrap(),
            original,
            "future schemas must be rejected before changing the journal mode"
        );
        let version: u32 = db
            .connect()
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 99);
        drop(db);
        std::fs::remove_file(&c.paths.database).unwrap();
        std::fs::write(&c.paths.database, b"corrupt database fixture").unwrap();
        assert!(Database::open(&c).is_err());
        assert_eq!(
            std::fs::read(&c.paths.database).unwrap(),
            b"corrupt database fixture"
        );
    }
}

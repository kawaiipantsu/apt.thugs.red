//! Local identity, password verification and revocable opaque sessions.
use crate::{config::Admin, db::Database};
use anyhow::{Context, Result, ensure};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use subtle::ConstantTimeEq;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Administrator,
}
impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Administrator => "administrator",
        }
    }
    pub fn can_operate(self) -> bool {
        self != Self::Viewer
    }
}
#[derive(Clone)]
pub struct Actor {
    pub id: String,
    pub interface: String,
    pub request_id: String,
}
impl Actor {
    pub fn local() -> Self {
        Self {
            id: "local-socket".into(),
            interface: "unix".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub role: Role,
    pub enabled: bool,
    pub created: String,
}
/// Deliberately does not implement Debug or Serialize: credentials stay out of diagnostics.
#[derive(Clone)]
pub struct Session {
    pub user: User,
    pub csrf: String,
    pub token_hash: String,
    pub expires: i64,
}
pub struct Login {
    pub token: String,
    pub session: Session,
}
pub enum UserChange {
    Password(String),
    Role(Role),
    Enabled(bool),
    Delete,
}
impl Drop for UserChange {
    fn drop(&mut self) {
        if let Self::Password(value) = self {
            zeroize::Zeroize::zeroize(value);
        }
    }
}
pub fn timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}
pub fn token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Operating system randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn equal_token(a: &str, b: &str) -> bool {
    a.len() == 64 && b.len() == 64 && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
fn argon() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19456, 2, 1, None).expect("fixed Argon2 parameters"),
    )
}
pub fn hash_password(password: &str) -> Result<String> {
    ensure!(
        (12..=1024).contains(&password.len()) && !password.contains('\0'),
        "Password must contain 12..1024 bytes without NUL"
    );
    argon()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|_| anyhow::anyhow!("Password hashing failed"))
}
fn verify_password(password: &str, hash: &str) -> bool {
    password.len() <= 1024
        && PasswordHash::new(hash)
            .is_ok_and(|h| argon().verify_password(password.as_bytes(), &h).is_ok())
}
fn user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let role: String = row.get(2)?;
    let role = match role.as_str() {
        "viewer" => Role::Viewer,
        "operator" => Role::Operator,
        "administrator" => Role::Administrator,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        role,
        enabled: row.get(3)?,
        created: row.get(4)?,
    })
}
const USER_COLUMNS: &str = "id,username,role,enabled,created";
impl Database {
    pub fn users(&self) -> Result<Vec<User>> {
        let c = self.connect()?;
        let mut s = c.prepare(&format!(
            "SELECT {USER_COLUMNS} FROM users ORDER BY username LIMIT 1000"
        ))?;
        Ok(s.query_map([], user_row)?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn add_user(
        &self,
        actor: &Actor,
        role: Role,
        username: &str,
        password: &str,
        permission: Role,
    ) -> Result<User> {
        ensure!(
            permission == Role::Administrator,
            "Administrator role required"
        );
        ensure!(
            (1..=64).contains(&username.len())
                && username
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b)),
            "Username must use 1..64 lowercase ASCII letters, digits, dot, underscore or hyphen"
        );
        let hash = hash_password(password)?;
        let id = uuid::Uuid::new_v4().to_string();
        let created = crate::repository::now();
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?;
        ensure!(count < 1000, "Local user limit reached");
        ensure!(
            count > 0 || role == Role::Administrator,
            "First user must be an administrator"
        );
        tx.execute(
            "INSERT INTO users(id,username,password_hash,role,created) VALUES(?1,?2,?3,?4,?5)",
            params![id, username, hash, role.as_str(), created],
        )
        .context("Cannot create user (name may already exist)")?;
        let event = Self::record_audit(
            &tx,
            actor,
            "user.add",
            &id,
            "succeeded",
            &json!({"role":role}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(User {
            id,
            username: username.into(),
            role,
            enabled: true,
            created,
        })
    }
    pub fn change_user(
        &self,
        actor: &Actor,
        id: &str,
        change: UserChange,
        permission: Role,
    ) -> Result<()> {
        ensure!(
            permission == Role::Administrator,
            "Administrator role required"
        );
        let hash = if let UserChange::Password(p) = &change {
            Some(hash_password(p)?)
        } else {
            None
        };
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = tx
            .query_row(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id=?1"),
                [id],
                user_row,
            )
            .optional()?
            .context("User not found")?;
        let removes_admin = matches!(
            change,
            UserChange::Delete
                | UserChange::Enabled(false)
                | UserChange::Role(Role::Viewer | Role::Operator)
        );
        if before.enabled && before.role == Role::Administrator && removes_admin {
            let admins: i64 = tx.query_row(
                "SELECT count(*) FROM users WHERE enabled=1 AND role='administrator'",
                [],
                |r| r.get(0),
            )?;
            ensure!(admins > 1, "Cannot remove the last enabled administrator");
        }
        let (action, after) = match &change {
            UserChange::Password(_) => {
                tx.execute("UPDATE users SET password_hash=?1,security_version=security_version+1 WHERE id=?2",params![hash,id])?;
                ("user.password", json!({"sessions_revoked":true}))
            }
            UserChange::Role(role) => {
                tx.execute(
                    "UPDATE users SET role=?1,security_version=security_version+1 WHERE id=?2",
                    params![role.as_str(), id],
                )?;
                ("user.role", json!({"before":before.role,"after":role}))
            }
            UserChange::Enabled(enabled) => {
                tx.execute(
                    "UPDATE users SET enabled=?1,security_version=security_version+1 WHERE id=?2",
                    params![enabled, id],
                )?;
                (
                    "user.enabled",
                    json!({"before":before.enabled,"after":enabled}),
                )
            }
            UserChange::Delete => {
                tx.execute("DELETE FROM users WHERE id=?1", [id])?;
                ("user.delete", json!({"previous_role":before.role}))
            }
        };
        tx.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
        let event = Self::record_audit(&tx, actor, action, id, "succeeded", &after)?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(())
    }
    /// Counts attempts before password verification, including unknown accounts.
    /// Immediate peer addresses are used by the HTTP layer; forwarded headers are ignored.
    pub fn login_allowed(&self, settings: &Admin, username: &str, peer: &str) -> Result<bool> {
        let now = timestamp();
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM login_limits WHERE window_start<=?1",
            [now - settings.login_window_seconds as i64],
        )?;
        let count: i64 = tx.query_row("SELECT count(*) FROM login_limits", [], |r| r.get(0))?;
        if count >= 10000 {
            return Ok(false);
        }
        let mut allowed = true;
        for (key, limit) in [
            (
                format!("account:{}", digest(username)),
                settings.login_max_attempts,
            ),
            (
                format!("peer:{}", digest(peer)),
                settings.login_max_attempts.saturating_mul(5),
            ),
        ] {
            tx.execute("INSERT INTO login_limits(bucket,window_start,attempts) VALUES(?1,?2,1) ON CONFLICT(bucket) DO UPDATE SET attempts=MIN(attempts+1,1000000)",params![key,now])?;
            let attempts: u32 = tx.query_row(
                "SELECT attempts FROM login_limits WHERE bucket=?1",
                [key],
                |r| r.get(0),
            )?;
            allowed &= attempts <= limit;
        }
        tx.commit()?;
        Ok(allowed)
    }
    pub fn challenge(&self) -> Result<String> {
        let value = token()?;
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM login_challenges WHERE expires<=?1",
            [timestamp()],
        )?;
        let count: i64 = tx.query_row("SELECT count(*) FROM login_challenges", [], |r| r.get(0))?;
        ensure!(count < 4096, "Login challenge capacity reached");
        tx.execute(
            "INSERT INTO login_challenges(token_hash,expires) VALUES(?1,?2)",
            params![digest(&value), timestamp() + 600],
        )?;
        tx.commit()?;
        Ok(value)
    }
    pub fn consume_challenge(&self, cookie: &str, csrf: &str) -> Result<bool> {
        if !equal_token(cookie, csrf) {
            return Ok(false);
        }
        Ok(self.connect()?.execute(
            "DELETE FROM login_challenges WHERE token_hash=?1 AND expires>?2",
            params![digest(cookie), timestamp()],
        )? == 1)
    }
    pub fn login(
        &self,
        settings: &Admin,
        actor: &Actor,
        username: &str,
        password: &str,
    ) -> Result<Option<Login>> {
        static DUMMY: OnceLock<String> = OnceLock::new();
        let dummy = DUMMY.get_or_init(|| {
            hash_password("randomized-dummy-verification-password").expect("Argon2 initialization")
        });
        let c = self.connect()?;
        let record: Option<(User, String, i64)> = c.query_row(&format!("SELECT {USER_COLUMNS},password_hash,security_version FROM users WHERE username=?1"),[username],|r| Ok((user_row(r)?,r.get(5)?,r.get(6)?))).optional()?;
        let hash = record
            .as_ref()
            .map_or(dummy.as_str(), |(_, h, _)| h.as_str());
        let verified = verify_password(password, hash);
        let Some((user, _, revision)) = record.filter(|(u, _, _)| verified && u.enabled) else {
            self.audit_event(actor, "auth.login", "session", "denied", &json!({}))?;
            return Ok(None);
        };
        drop(c);
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1 AND enabled=1 AND security_version=?2)",
            params![user.id, revision],
            |r| r.get(0),
        )?;
        if !valid {
            return Ok(None);
        }
        let now = timestamp();
        tx.execute("DELETE FROM sessions WHERE expires<=?1", [now])?;
        tx.execute("DELETE FROM sessions WHERE user_id=?1 AND token_hash NOT IN (SELECT token_hash FROM sessions WHERE user_id=?1 ORDER BY created DESC,rowid DESC LIMIT ?2)",params![user.id,settings.max_sessions_per_user-1])?;
        let value = token()?;
        let csrf = token()?;
        let expires = now + settings.session_lifetime_seconds as i64;
        tx.execute("INSERT INTO sessions(token_hash,user_id,csrf_token,created,expires) VALUES(?1,?2,?3,?4,?5)",params![digest(&value),user.id,csrf,now,expires])?;
        let actor = Actor {
            id: user.id.clone(),
            ..actor.clone()
        };
        let event = Self::record_audit(
            &tx,
            &actor,
            "auth.login",
            "session",
            "succeeded",
            &json!({}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(Some(Login {
            session: Session {
                user,
                csrf,
                token_hash: digest(&value),
                expires,
            },
            token: value,
        }))
    }
    pub fn session(&self, token: &str) -> Result<Option<Session>> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let c = self.connect()?;
        Ok(c.query_row("SELECT u.id,u.username,u.role,u.enabled,u.created,s.csrf_token,s.token_hash,s.expires FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=?1 AND s.expires>?2 AND u.enabled=1",params![digest(token),timestamp()],|r| Ok(Session { user:user_row(r)?, csrf:r.get(5)?, token_hash:r.get(6)?, expires:r.get(7)? })).optional()?)
    }
    pub fn logout(&self, actor: &Actor, hash: &str) -> Result<()> {
        let mut c = self.connect()?;
        let tx = c.transaction()?;
        tx.execute("DELETE FROM sessions WHERE token_hash=?1", [hash])?;
        let event = Self::record_audit(
            &tx,
            actor,
            "auth.logout",
            "session",
            "succeeded",
            &json!({}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    const PASSWORD: &str = "disposable-test-password";
    #[test]
    fn passwords_sessions_revocation_and_last_administrator() {
        let root = tempfile::tempdir().unwrap();
        let c = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let db = Database::open(&c).unwrap();
        let actor = Actor::local();
        assert!(
            db.add_user(&actor, Role::Viewer, "first", PASSWORD, Role::Administrator)
                .is_err()
        );
        let admin = db
            .add_user(
                &actor,
                Role::Administrator,
                "admin",
                PASSWORD,
                Role::Administrator,
            )
            .unwrap();
        let viewer = db
            .add_user(
                &actor,
                Role::Viewer,
                "viewer",
                PASSWORD,
                Role::Administrator,
            )
            .unwrap();
        let hashes: Vec<String> = db
            .connect()
            .unwrap()
            .prepare("SELECT password_hash FROM users")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_ne!(hashes[0], hashes[1]);
        assert!(
            hashes
                .iter()
                .all(|h| h.starts_with("$argon2id$v=19$m=19456,t=2,p=1$") && !h.contains(PASSWORD))
        );
        assert!(
            db.login(&c.admin, &actor, "unknown", PASSWORD)
                .unwrap()
                .is_none()
        );
        assert!(
            db.login(&c.admin, &actor, "viewer", "incorrect")
                .unwrap()
                .is_none()
        );
        assert!(
            db.change_user(
                &actor,
                &admin.id,
                UserChange::Enabled(false),
                Role::Administrator
            )
            .is_err()
        );
        assert!(
            db.change_user(
                &actor,
                &admin.id,
                UserChange::Role(Role::Viewer),
                Role::Administrator
            )
            .is_err()
        );
        assert!(
            db.change_user(&actor, &admin.id, UserChange::Delete, Role::Administrator)
                .is_err()
        );
        assert!(
            db.change_user(&actor, &admin.id, UserChange::Delete, Role::Operator)
                .is_err()
        );
        let login = db
            .login(&c.admin, &actor, "viewer", PASSWORD)
            .unwrap()
            .unwrap();
        assert_eq!(
            db.session(&login.token).unwrap().unwrap().user.role,
            Role::Viewer
        );
        let stored: String = db
            .connect()
            .unwrap()
            .query_row("SELECT token_hash FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, login.token);
        assert_eq!(stored, digest(&login.token));
        db.logout(&actor, &stored).unwrap();
        assert!(db.session(&login.token).unwrap().is_none());
        for change in [
            UserChange::Role(Role::Operator),
            UserChange::Password(PASSWORD.into()),
            UserChange::Enabled(false),
        ] {
            let login = db
                .login(&c.admin, &actor, "viewer", PASSWORD)
                .unwrap()
                .unwrap();
            db.change_user(&actor, &viewer.id, change, Role::Administrator)
                .unwrap();
            assert!(db.session(&login.token).unwrap().is_none());
        }
        assert!(
            db.login(&c.admin, &actor, "viewer", PASSWORD)
                .unwrap()
                .is_none()
        );
        db.change_user(
            &actor,
            &viewer.id,
            UserChange::Enabled(true),
            Role::Administrator,
        )
        .unwrap();
        let login = db
            .login(&c.admin, &actor, "viewer", PASSWORD)
            .unwrap()
            .unwrap();
        db.connect()
            .unwrap()
            .execute("UPDATE sessions SET expires=0", [])
            .unwrap();
        assert!(db.session(&login.token).unwrap().is_none());
        db.change_user(&actor, &viewer.id, UserChange::Delete, Role::Administrator)
            .unwrap();
        let audit = std::fs::read_to_string(&c.logging.audit_file).unwrap();
        for secret in [
            PASSWORD,
            login.token.as_str(),
            login.session.csrf.as_str(),
            hashes[0].as_str(),
        ] {
            assert!(!audit.contains(secret));
        }
        assert!(
            !serde_json::to_string(&db.users().unwrap())
                .unwrap()
                .contains("password")
        );
        assert!(hash_password("short").is_err());
        assert!(hash_password(&"x".repeat(1025)).is_err());
    }
    #[test]
    fn challenges_rate_limits_and_session_limit() {
        let root = tempfile::tempdir().unwrap();
        let c = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let db = Database::open(&c).unwrap();
        let challenge = db.challenge().unwrap();
        assert!(!db.consume_challenge(&challenge, &"a".repeat(64)).unwrap());
        assert!(db.consume_challenge(&challenge, &challenge).unwrap());
        assert!(!db.consume_challenge(&challenge, &challenge).unwrap());
        let expired = db.challenge().unwrap();
        db.connect()
            .unwrap()
            .execute("UPDATE login_challenges SET expires=0", [])
            .unwrap();
        assert!(!db.consume_challenge(&expired, &expired).unwrap());
        let mut settings = c.admin.clone();
        settings.login_max_attempts = 2;
        settings.max_sessions_per_user = 1;
        for _ in 0..2 {
            assert!(db.login_allowed(&settings, "test", "127.0.0.1").unwrap());
        }
        assert!(!db.login_allowed(&settings, "test", "127.0.0.2").unwrap());
        for i in 0..8 {
            assert!(
                db.login_allowed(&settings, &format!("other{i}"), "127.0.0.1")
                    .unwrap()
            );
        }
        assert!(!db.login_allowed(&settings, "new", "127.0.0.1").unwrap());
        db.connect()
            .unwrap()
            .execute("UPDATE login_limits SET window_start=0", [])
            .unwrap();
        assert!(db.login_allowed(&settings, "test", "127.0.0.1").unwrap());
        let actor = Actor::local();
        db.add_user(
            &actor,
            Role::Administrator,
            "admin",
            PASSWORD,
            Role::Administrator,
        )
        .unwrap();
        let a = db
            .login(&settings, &actor, "admin", PASSWORD)
            .unwrap()
            .unwrap();
        let b = db
            .login(&settings, &actor, "admin", PASSWORD)
            .unwrap()
            .unwrap();
        assert!(db.session(&a.token).unwrap().is_none());
        assert!(db.session(&b.token).unwrap().is_some());
    }
}

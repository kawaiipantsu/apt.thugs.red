//! Scoped automation credentials. Only a digest is durable; the secret is returned once.
use crate::{auth::Actor, config::Config, db::Database};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
pub struct Token {
    pub id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub suites: Vec<String>,
    pub created: i64,
    pub expires: i64,
    pub last_used: Option<i64>,
    pub revoked: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateToken {
    pub name: String,
    pub scopes: Vec<String>,
    pub suites: Vec<String>,
    pub days: u32,
}
impl Token {
    pub fn expires_label(&self) -> String {
        date(self.expires)
    }
    pub fn last_used_label(&self) -> String {
        self.last_used.map(date).unwrap_or_else(|| "never".into())
    }
    pub fn expired(&self) -> bool {
        self.expires <= chrono::Utc::now().timestamp()
    }
    pub fn allows(&self, scope: &str, suite: &str) -> bool {
        self.scopes.iter().any(|s| s == scope) && self.suites.iter().any(|s| s == suite)
    }
}
fn date(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "invalid date".into())
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Token> {
    let parse = |i| -> rusqlite::Result<Vec<String>> {
        serde_json::from_str(&r.get::<_, String>(i)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e))
        })
    };
    Ok(Token {
        id: r.get(0)?,
        name: r.get(1)?,
        scopes: parse(2)?,
        suites: parse(3)?,
        created: r.get(4)?,
        expires: r.get(5)?,
        last_used: r.get(6)?,
        revoked: r.get(7)?,
    })
}
const COLUMNS: &str = "id,name,scopes,suites,created,expires,last_used,revoked";
impl Database {
    pub fn create_token(
        &self,
        c: &Config,
        actor: &Actor,
        input: CreateToken,
    ) -> Result<(Token, zeroize::Zeroizing<String>)> {
        ensure!(
            !input.name.trim().is_empty()
                && input.name.len() <= 80
                && !input.name.chars().any(char::is_control),
            "Token name must be 1–80 bytes without control characters"
        );
        ensure!(
            (1..=365).contains(&input.days),
            "Token lifetime must be 1–365 days"
        );
        ensure!(
            !input.scopes.is_empty()
                && input.scopes.len() <= 4
                && input
                    .scopes
                    .iter()
                    .all(|s| matches!(s.as_str(), "read" | "upload" | "stage" | "publish")),
            "Unknown or empty token scopes"
        );
        ensure!(
            !input.suites.is_empty()
                && input.suites.len() <= 32
                && input.suites.iter().all(|s| c.repository.has_suite(s)),
            "Token suites must be explicitly configured"
        );
        let secret = zeroize::Zeroizing::new(format!("xxc_aptd_{}", crate::auth::token()?));
        let now = chrono::Utc::now().timestamp();
        let token = Token {
            id: uuid::Uuid::new_v4().to_string(),
            name: input.name,
            scopes: input.scopes,
            suites: input.suites,
            created: now,
            expires: now + i64::from(input.days) * 86400,
            last_used: None,
            revoked: false,
        };
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM api_tokens WHERE revoked=0 AND expires>?1",
            [now],
            |r| r.get(0),
        )?;
        ensure!(
            count < 1000,
            "Revoke unused automation tokens before creating more"
        );
        tx.execute("INSERT INTO api_tokens(id,name,digest,scopes,suites,created,expires) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![token.id,token.name,Sha256::digest(secret.as_bytes()).as_slice(),serde_json::to_string(&token.scopes)?,serde_json::to_string(&token.suites)?,now,token.expires])?;
        let event = Self::record_audit(
            &tx,
            actor,
            "token.create",
            &token.id,
            "succeeded",
            &serde_json::json!({"scopes":token.scopes,"suites":token.suites,"expires":token.expires}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok((token, secret))
    }
    pub fn tokens(&self) -> Result<Vec<Token>> {
        let conn = self.connect()?;
        let mut query = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM api_tokens ORDER BY created DESC,id LIMIT 1000"
        ))?;
        Ok(query.query_map([], row)?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn revoke_token(&self, actor: &Actor, id: &str) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        ensure!(
            tx.execute("UPDATE api_tokens SET revoked=1 WHERE id=?1", [id])? == 1,
            "Token not found"
        );
        let event = Self::record_audit(
            &tx,
            actor,
            "token.revoke",
            id,
            "succeeded",
            &serde_json::json!({}),
        )?;
        tx.commit()?;
        self.append_audit(&event);
        Ok(())
    }
    pub fn authenticate_token(&self, secret: &str) -> Result<Option<Token>> {
        if secret.len() != 73
            || !secret.starts_with("xxc_aptd_")
            || !secret[9..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Ok(None);
        }
        let conn = self.connect()?;
        let now = chrono::Utc::now().timestamp();
        let token = conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM api_tokens WHERE digest=?1 AND revoked=0 AND expires>?2"
                ),
                params![Sha256::digest(secret.as_bytes()).as_slice(), now],
                row,
            )
            .optional()?;
        if let Some(t) = &token {
            conn.execute("UPDATE api_tokens SET last_used=?1 WHERE id=?2 AND (last_used IS NULL OR last_used<?1-60)",params![now,t.id])?;
        }
        Ok(token)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digest_only_expiry_revocation_and_scope() {
        let root = tempfile::tempdir().unwrap();
        let c = Config::initialize(&root.path().join("config"), Some(root.path())).unwrap();
        let db = Database::open(&c).unwrap();
        let (token, secret) = db
            .create_token(
                &c,
                &Actor::local(),
                CreateToken {
                    name: "fixture".into(),
                    scopes: vec!["upload".into()],
                    suites: vec![c.repository.suite.clone()],
                    days: 1,
                },
            )
            .unwrap();
        assert!(token.allows("upload", &c.repository.suite));
        assert!(!token.allows("publish", &c.repository.suite));
        assert!(!token.allows("upload", "other"));
        assert!(db.authenticate_token(&secret).unwrap().is_some());
        assert!(
            !serde_json::to_string(&db.tokens().unwrap())
                .unwrap()
                .contains(secret.as_str())
        );
        db.connect()
            .unwrap()
            .execute("UPDATE api_tokens SET expires=0", [])
            .unwrap();
        assert!(db.authenticate_token(&secret).unwrap().is_none());
        db.connect()
            .unwrap()
            .execute("UPDATE api_tokens SET expires=9999999999", [])
            .unwrap();
        db.revoke_token(&Actor::local(), &token.id).unwrap();
        assert!(db.authenticate_token(&secret).unwrap().is_none());
        for path in std::fs::read_dir(c.paths.database.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
        {
            let bytes = std::fs::read(path).unwrap();
            assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
    }
}

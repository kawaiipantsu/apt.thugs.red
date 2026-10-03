//! XXC Trust API 1.1 OpenPGP operations. No private-key export route.
use super::{RemoteResult, TrustClient, TrustError};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenPgpKey {
    pub id: String,
    pub fingerprint: String,
    pub label: String,
    pub status: String,
    pub algorithm: String,
    pub bits: u32,
    pub capabilities: String,
    pub not_after: Option<String>,
    pub has_private_key: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenPgpKeys {
    pub items: Vec<OpenPgpKey>,
    pub page: u32,
    pub pages: u32,
    pub total: u64,
}
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeyQuery {
    pub page: u32,
    pub q: String,
}
impl KeyQuery {
    pub fn validate(&self) -> RemoteResult<()> {
        if self.page > 1_000_000 || self.q.len() > 254 || self.q.chars().any(char::is_control) {
            return Err(TrustError("trust_invalid_query"));
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateKey {
    pub name: String,
    pub email: String,
    pub algorithm: String,
    pub days: u32,
}
impl GenerateKey {
    pub fn validate(&self) -> RemoteResult<()> {
        if self.name.trim().is_empty()
            || self.name.len() > 180
            || self.name.chars().any(char::is_control)
            || !(1..=3650).contains(&self.days)
            || !["Ed25519", "RSA-3072", "RSA-4096"].contains(&self.algorithm.as_str())
            || self.email.len() > 254
            || !self.email.is_ascii()
            || self.email.chars().any(char::is_whitespace)
            || self.email.chars().any(char::is_control)
            || self.email.matches('@').count() != 1
            || self.email.split('@').any(str::is_empty)
        {
            return Err(TrustError("trust_invalid_query"));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Artifact {
    filename: String,
    content_type: String,
    data_base64: String,
}
#[derive(Deserialize)]
struct SignedRelease {
    fingerprint: String,
    artifacts: Vec<Artifact>,
}
pub struct ReleaseSignatures {
    pub inline: Vec<u8>,
    pub detached: Vec<u8>,
}
pub fn key_id(id: &str) -> RemoteResult<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(TrustError("trust_invalid_query"));
    }
    Ok(())
}
impl TrustClient {
    pub async fn openpgp_keys(&self, q: &KeyQuery) -> RemoteResult<OpenPgpKeys> {
        q.validate()?;
        let keys: OpenPgpKeys = self
            .get(
                "openpgp/keys",
                &[("page", q.page.max(1).to_string()), ("q", q.q.clone())],
            )
            .await?;
        if keys.page != q.page.max(1) {
            return Err(TrustError("trust_invalid_response"));
        }
        Ok(keys)
    }
    pub async fn openpgp_key(&self, id: &str) -> RemoteResult<OpenPgpKey> {
        key_id(id)?;
        let key: OpenPgpKey = self.get(&format!("openpgp/keys/{id}"), &[]).await?;
        if key.id != id {
            return Err(TrustError("trust_invalid_response"));
        }
        Ok(key)
    }
    pub async fn generate_openpgp(&self, input: &GenerateKey) -> RemoteResult<OpenPgpKey> {
        input.validate()?;
        // No automatic retry: an ambiguous response may already have created a key.
        let bytes = self
            .request(
                reqwest::Method::POST,
                "openpgp/keys",
                &[],
                Some(&json!(input)),
                201,
            )
            .await?;
        serde_json::from_slice(&bytes).map_err(|_| TrustError("trust_invalid_response"))
    }
    pub async fn openpgp_public_key(&self, id: &str) -> RemoteResult<zeroize::Zeroizing<Vec<u8>>> {
        key_id(id)?;
        self.request(
            reqwest::Method::GET,
            &format!("openpgp/keys/{id}/download"),
            &[("format", "binary".into())],
            None,
            200,
        )
        .await
    }
    pub async fn sign_release(
        &self,
        id: &str,
        fingerprint: &str,
        release: &[u8],
    ) -> RemoteResult<ReleaseSignatures> {
        key_id(id)?;
        if release.len() > 8 * 1024 * 1024 {
            return Err(TrustError("trust_request_too_large"));
        }
        let bytes = self
            .request(
                reqwest::Method::POST,
                "debian/sign",
                &[],
                Some(&json!({"key_id":id,"kind":"release","data_base64":STANDARD.encode(release)})),
                200,
            )
            .await?;
        let response: SignedRelease =
            serde_json::from_slice(&bytes).map_err(|_| TrustError("trust_invalid_response"))?;
        if !response.fingerprint.eq_ignore_ascii_case(fingerprint) || response.artifacts.len() != 2
        {
            return Err(TrustError("trust_signer_mismatch"));
        }
        let mut inline = None;
        let mut detached = None;
        for artifact in response.artifacts {
            let bytes = STANDARD
                .decode(&artifact.data_base64)
                .map_err(|_| TrustError("trust_invalid_response"))?;
            if bytes.is_empty()
                || bytes.len() > 8 * 1024 * 1024 + 65536
                || artifact.content_type.is_empty()
            {
                return Err(TrustError("trust_invalid_response"));
            }
            match artifact.filename.as_str() {
                "InRelease" if inline.is_none() => inline = Some(bytes),
                "Release.gpg" if detached.is_none() => detached = Some(bytes),
                _ => return Err(TrustError("trust_invalid_response")),
            }
        }
        Ok(ReleaseSignatures {
            inline: inline.ok_or(TrustError("trust_invalid_response"))?,
            detached: detached.ok_or(TrustError("trust_invalid_response"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_inputs_are_bounded_and_cannot_be_paths() {
        for id in ["../keys", "A123", "1111111111111111111111111111111/"] {
            assert!(key_id(id).is_err());
        }
        let mut input = GenerateKey {
            name: "Fixture archive".into(),
            email: "archive@example.invalid".into(),
            algorithm: "Ed25519".into(),
            days: 365,
        };
        assert!(input.validate().is_ok());
        input.email = "bad\nheader@example.invalid".into();
        assert!(input.validate().is_err());
        input.email = "archive@example.invalid".into();
        input.days = 0;
        assert!(input.validate().is_err());
        input.days = 365;
        input.algorithm = "arbitrary-command".into();
        assert!(input.validate().is_err());
        assert!(
            KeyQuery {
                page: 1,
                q: "x".repeat(255)
            }
            .validate()
            .is_err()
        );
    }
}

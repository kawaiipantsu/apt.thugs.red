//! Public metadata and remote generation; private material never has an API route.
use crate::{
    State,
    api::{Error, Result},
    auth::Principal,
    blocking, trust,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State as App},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use xxc_aptd_core::trust::{
    TrustClient, TrustError,
    openpgp::{GenerateKey, KeyQuery},
};

fn client<'a>(s: &'a State, p: &Principal) -> Result<&'a TrustClient> {
    p.administrator()?;
    s.trust
        .as_ref()
        .ok_or_else(|| trust::error(TrustError("trust_disabled"), p))
}
pub(crate) async fn list_keys(s: &State, p: &Principal, q: KeyQuery) -> Result<Value> {
    Ok(json!(
        client(s, p)?
            .openpgp_keys(&q)
            .await
            .map_err(|e| trust::error(e, p))?
    ))
}
pub(crate) async fn create_key(s: &State, p: &Principal, input: GenerateKey) -> Result<Value> {
    let client = client(s, p)?;
    input.validate().map_err(|e| trust::error(e, p))?;
    let db = s.db.clone();
    let actor = p.actor.clone();
    blocking(move || db.audit_event(&actor, "key.generate", "xxc_trust", "requested", &json!({})))
        .await?;
    let result = client.generate_openpgp(&input).await;
    let outcome = if result.is_ok() {
        "succeeded"
    } else {
        "failed"
    };
    let db = s.db.clone();
    let actor = p.actor.clone();
    blocking(move || db.audit_event(&actor, "key.generate", "xxc_trust", outcome, &json!({})))
        .await?;
    Ok(json!(result.map_err(|e| trust::error(e, p))?))
}
pub(crate) async fn list(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<KeyQuery>,
) -> Result<Json<Value>> {
    Ok(Json(list_keys(&s, &p, q).await?))
}
pub(crate) async fn show(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(json!(
        client(&s, &p)?
            .openpgp_key(&id)
            .await
            .map_err(|e| trust::error(e, &p))?
    )))
}
pub(crate) async fn generate(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Json(input): Json<GenerateKey>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((StatusCode::CREATED, Json(create_key(&s, &p, input).await?)))
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Export {
    format: String,
}
impl Default for Export {
    fn default() -> Self {
        Self {
            format: "binary".into(),
        }
    }
}
async fn public_bytes(
    s: State,
    p: Principal,
    id: String,
    armor: bool,
) -> Result<(Vec<u8>, String)> {
    let client = client(&s, &p)?;
    let key = client
        .openpgp_key(&id)
        .await
        .map_err(|e| trust::error(e, &p))?;
    if id == s.config.signing.remote_key_id
        && !key
            .fingerprint
            .eq_ignore_ascii_case(&s.config.signing.fingerprint)
    {
        return Err(trust::error(TrustError("trust_signer_mismatch"), &p));
    }
    let bytes = client
        .openpgp_public_key(&id)
        .await
        .map_err(|e| trust::error(e, &p))?;
    let fingerprint = key.fingerprint;
    blocking(move || {
        Ok((
            xxc_aptd_core::signing::PublicKey::new(&s.config, &fingerprint, bytes)?
                .export(armor)?,
            fingerprint,
        ))
    })
    .await
    .map_err(Error::from)
}
pub(crate) async fn export(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Query(q): Query<Export>,
) -> Result<Response> {
    if !["armor", "binary"].contains(&q.format.as_str()) {
        return Err(trust::error(TrustError("trust_invalid_query"), &p));
    }
    let (bytes, _) = public_bytes(s, p, id, q.format == "armor").await?;
    Ok((
        [
            ("content-type", "application/pgp-keys"),
            ("cache-control", "no-store"),
        ],
        bytes,
    )
        .into_response())
}
pub(crate) async fn verify(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (_, fingerprint) = public_bytes(s, p, id, false).await?;
    Ok(Json(
        json!({"public_key_valid":true,"fingerprint":fingerprint}),
    ))
}

//! Administrator-only projections of the external X.509 inventory.
use crate::{
    State,
    api::{Error, Result},
    auth::Principal,
    blocking,
};
use axum::{
    Extension, Json,
    extract::{Query, State as App},
    http::StatusCode,
};
use serde_json::{Value, json};
use xxc_aptd_core::trust::{CertificateQuery, TrustError};

pub(crate) fn error(e: TrustError, p: &Principal) -> Error {
    let status = match e.0 {
        "trust_invalid_query" => StatusCode::BAD_REQUEST,
        "trust_busy" | "trust_disabled" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    };
    Error(status, e.0, p.actor.request_id.clone())
}
/// Shared by HTML and JSON. Only action names and results enter the audit stream.
pub(crate) async fn read(
    s: &State,
    p: &Principal,
    operation: &str,
    query: CertificateQuery,
) -> Result<Value> {
    p.administrator()?;
    query.validate().map_err(|e| error(e, p))?;
    let result = if let Some(client) = &s.trust {
        match operation {
            "status" => client.status().await.map(|v| json!(v)),
            "authorities" => client.authorities().await.map(|v| json!(v)),
            "templates" => client.templates().await.map(|v| json!(v)),
            "certificates" => client.certificates(&query).await.map(|v| json!(v)),
            _ => Err(TrustError("trust_invalid_query")),
        }
    } else if operation == "status" {
        Ok(json!({"enabled":false,"connected":false}))
    } else {
        Err(TrustError("trust_disabled"))
    };
    let db = s.db.clone();
    let actor = p.actor.clone();
    let action = match operation {
        "status" => "trust.status",
        "authorities" => "trust.authorities",
        "templates" => "trust.templates",
        _ => "trust.certificates",
    };
    let outcome = if result.is_ok() {
        "succeeded"
    } else {
        "failed"
    };
    blocking(move || db.audit_event(&actor, action, "xxc_trust", outcome, &json!({}))).await?;
    result.map_err(|e| error(e, p))
}
pub(crate) async fn status(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    Ok(Json(
        read(&s, &p, "status", CertificateQuery::default()).await?,
    ))
}
pub(crate) async fn authorities(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    Ok(Json(
        read(&s, &p, "authorities", CertificateQuery::default()).await?,
    ))
}
pub(crate) async fn templates(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    Ok(Json(
        read(&s, &p, "templates", CertificateQuery::default()).await?,
    ))
}
pub(crate) async fn certificates(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<CertificateQuery>,
) -> Result<Json<Value>> {
    Ok(Json(read(&s, &p, "certificates", q).await?))
}

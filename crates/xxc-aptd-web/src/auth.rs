//! HTTP trust boundary. Unix authentication is installed only on the socket router.
use crate::{
    State,
    api::{Error, Result},
    blocking,
};
use axum::{
    Extension, Json,
    body::{Body, to_bytes},
    extract::{ConnectInfo, State as App},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use xxc_aptd_core::auth::{self, Actor, Role, Session};

#[derive(Clone)]
pub struct Principal {
    pub actor: Actor,
    pub role: Role,
    pub session: Option<Session>,
    pub token: Option<xxc_aptd_core::tokens::Token>,
}
impl Principal {
    pub fn operator(&self) -> Result<()> {
        if self.role.can_operate() {
            Ok(())
        } else {
            Err(error(StatusCode::FORBIDDEN, "role_required"))
        }
    }
    pub fn administrator(&self) -> Result<()> {
        if self.role == Role::Administrator {
            Ok(())
        } else {
            Err(error(StatusCode::FORBIDDEN, "administrator_required"))
        }
    }
    pub fn csrf(&self) -> &str {
        self.session.as_ref().map_or("", |s| s.csrf.as_str())
    }
}
pub fn error(status: StatusCode, code: &'static str) -> Error {
    Error(status, code, uuid::Uuid::new_v4().to_string())
}
pub async fn local(mut request: Request<Body>, next: Next) -> Response {
    request.extensions_mut().insert(Principal {
        actor: Actor::local(),
        role: Role::Administrator,
        session: None,
        token: None,
    });
    next.run(request).await
}
pub fn secure_cookie(s: &State) -> bool {
    // Only the admin origin receives session cookies. The independently proxied
    // public origin must not change the admin transport or cookie namespace.
    s.config.admin.external_url.starts_with("https://")
}
pub fn cookie_name(s: &State, challenge: bool) -> &'static str {
    match (secure_cookie(s), challenge) {
        (true, true) => "__Host-xxc-login",
        (true, false) => "__Host-xxc-session",
        (false, true) => "xxc-login",
        (false, false) => "xxc-session",
    }
}
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut found = None;
    for value in headers.get_all("cookie") {
        for pair in value.to_str().ok()?.split(';') {
            let Some((key, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if found.is_some() {
                    return None;
                }
                found = Some(value.to_owned());
            }
        }
    }
    found.filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn set_cookie(response: &mut Response, s: &State, challenge: bool, value: &str, age: u64) {
    let secure = if secure_cookie(s) { "; Secure" } else { "" };
    let text = format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={age}{secure}",
        cookie_name(s, challenge)
    );
    if let Ok(value) = HeaderValue::from_str(&text) {
        response.headers_mut().append("set-cookie", value);
    }
}
pub fn origin(s: &State, headers: &HeaderMap) -> bool {
    headers.get_all("origin").iter().count() == 1
        && headers.get("origin").and_then(|v| v.to_str().ok())
            == Some(s.config.admin.external_url.as_str())
}
/// Validate Host on every admin request and never interpret forwarded identity headers.
pub async fn boundary(App(s): App<State>, request: Request<Body>, next: Next) -> Response {
    let expected = s
        .config
        .admin
        .external_url
        .split_once("://")
        .map(|(_, h)| h)
        .unwrap_or("");
    let host = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let valid = request.headers().get_all("host").iter().count() == 1
        && (host == expected || host == s.config.server.admin_listen.to_string());
    let api = request.uri().path().starts_with("/api/")
        || request.uri().path().starts_with("/admin/api/");
    let mut response = if !s.config.admin.enabled {
        error(StatusCode::SERVICE_UNAVAILABLE, "administration_disabled").into_response()
    } else if !valid {
        error(StatusCode::BAD_REQUEST, "invalid_host").into_response()
    } else {
        next.run(request).await
    };
    if api
        && (response.status().is_client_error() || response.status().is_server_error())
        && response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
    {
        response = error(response.status(), "invalid_request").into_response();
    }
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    if !response.headers().contains_key("x-request-id") {
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&uuid::Uuid::new_v4().to_string()).expect("UUID header"),
        );
    }
    response
}
pub async fn gate(App(s): App<State>, mut request: Request<Body>, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let api = path.starts_with("/api/") || matches!(path.as_str(), "/healthz" | "/readyz");
    if request.headers().contains_key("authorization") {
        return bearer(s, request, next).await;
    }
    let Some(token) = cookie(request.headers(), cookie_name(&s, false)) else {
        return unauthenticated(api);
    };
    let db = s.db.clone();
    let session = match blocking(move || db.session(&token)).await {
        Ok(Some(session)) => session,
        Ok(None) => return unauthenticated(api),
        Err(e) => return Error::internal(e).into_response(),
    };
    let principal = Principal {
        actor: Actor {
            id: session.user.id.clone(),
            interface: "http".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
        },
        role: session.user.role,
        session: Some(session),
        token: None,
    };
    let admin_only = path.starts_with("/api/v1/tokens")
        || path.starts_with("/admin/tokens")
        || path == "/api/v1/config"
        || path.starts_with("/api/v1/users")
        || path.starts_with("/api/v1/keys")
        || path.starts_with("/admin/keys")
        || path.starts_with("/admin/users")
        || path == "/admin/settings"
        || path.starts_with("/admin/trust")
        || path.starts_with("/api/v1/trust/");
    if admin_only && let Err(e) = principal.administrator() {
        return e.into_response();
    }
    let mutation = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if mutation {
        if !origin(&s, request.headers()) {
            return error(StatusCode::FORBIDDEN, "origin_rejected").into_response();
        }
        if path != "/admin/logout"
            && path != "/api/v1/auth/logout"
            && let Err(e) = principal.operator()
        {
            return e.into_response();
        }
        if api {
            let csrf = request
                .headers()
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !auth::equal_token(csrf, principal.csrf()) {
                return error(StatusCode::FORBIDDEN, "csrf_rejected").into_response();
            }
        } else if path == "/admin/uploads" {
            // Multipart CSRF is checked as the first field before any package bytes are accepted.
        } else {
            if request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(';').next().unwrap_or(""))
                != Some("application/x-www-form-urlencoded")
            {
                return error(StatusCode::BAD_REQUEST, "form_required").into_response();
            }
            let (parts, body) = request.into_parts();
            let bytes = match to_bytes(body, 65536).await {
                Ok(b) => b,
                Err(_) => {
                    return error(StatusCode::PAYLOAD_TOO_LARGE, "form_too_large").into_response();
                }
            };
            let input = serde_urlencoded::from_bytes::<Csrf>(&bytes);
            if !input.is_ok_and(|v| auth::equal_token(&v.csrf, principal.csrf())) {
                return error(StatusCode::FORBIDDEN, "csrf_rejected").into_response();
            }
            request = Request::from_parts(parts, Body::from(bytes));
        }
    }
    let request_id = principal.actor.request_id.clone();
    request.extensions_mut().insert(principal);
    let mut response = next.run(request).await;
    if !response.headers().contains_key("x-request-id") {
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("UUID header"),
        );
    }
    response
}
fn unauthenticated(api: bool) -> Response {
    if api {
        error(StatusCode::UNAUTHORIZED, "authentication_required").into_response()
    } else {
        Redirect::to("/admin/login").into_response()
    }
}
#[derive(Deserialize)]
struct Csrf {
    csrf: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Credentials {
    pub username: String,
    pub password: String,
    pub csrf: String,
}
pub(crate) async fn authenticate(
    s: State,
    headers: HeaderMap,
    peer: SocketAddr,
    input: Credentials,
) -> Result<auth::Login> {
    let password = zeroize::Zeroizing::new(input.password);
    if !origin(&s, &headers) {
        return Err(error(StatusCode::FORBIDDEN, "origin_rejected"));
    }
    if input.username.len() > 64 || password.len() > 1024 {
        return Err(error(StatusCode::UNAUTHORIZED, "login_failed"));
    }
    let cookie = cookie(&headers, cookie_name(&s, true)).unwrap_or_default();
    let permit = s
        .passwords
        .clone()
        .try_acquire_owned()
        .map_err(|_| error(StatusCode::TOO_MANY_REQUESTS, "login_busy"))?;
    blocking(move || {
        let _permit = permit;
        if !s.db.consume_challenge(&cookie, &input.csrf)? {
            return Ok(Err(error(StatusCode::FORBIDDEN, "csrf_rejected")));
        }
        let actor = Actor {
            id: "unauthenticated".into(),
            interface: "http".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
        };
        if !s
            .db
            .login_allowed(&s.config.admin, &input.username, &peer.ip().to_string())?
        {
            s.db.audit_event(&actor, "auth.login", "session", "rate_limited", &json!({}))?;
            return Ok(Err(error(
                StatusCode::TOO_MANY_REQUESTS,
                "login_rate_limited",
            )));
        }
        Ok(s.db
            .login(&s.config.admin, &actor, &input.username, &password)?
            .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "login_failed")))
    })
    .await?
}
pub async fn challenge(App(s): App<State>) -> Result<Response> {
    let db = s.db.clone();
    let value = blocking(move || db.challenge()).await?;
    let mut response = Json(json!({"csrf":value})).into_response();
    set_cookie(&mut response, &s, true, &value, 600);
    Ok(response)
}
pub(crate) async fn login(
    App(s): App<State>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Credentials>,
) -> Result<Response> {
    let login = authenticate(s.clone(), headers, peer, input).await?;
    let mut response=Json(json!({"user":login.session.user,"csrf":login.session.csrf,"expires":login.session.expires})).into_response();
    set_cookie(
        &mut response,
        &s,
        false,
        &login.token,
        s.config.admin.session_lifetime_seconds,
    );
    set_cookie(&mut response, &s, true, "", 0);
    Ok(response)
}
pub async fn session(Extension(p): Extension<Principal>) -> Json<serde_json::Value> {
    Json(
        json!({"user":p.session.as_ref().map(|s|&s.user),"csrf":p.csrf(),"expires":p.session.as_ref().map(|s|s.expires)}),
    )
}
pub async fn logout(App(s): App<State>, Extension(p): Extension<Principal>) -> Result<Response> {
    end_session(&s, p).await?;
    let mut response = Json(json!({"logged_out":true})).into_response();
    set_cookie(&mut response, &s, false, "", 0);
    Ok(response)
}
pub(crate) async fn end_session(s: &State, p: Principal) -> Result<()> {
    let db = s.db.clone();
    if let Some(session) = p.session {
        blocking(move || db.logout(&p.actor, &session.token_hash)).await?;
    }
    Ok(())
}

/// Bearer authority is limited to an explicit route/method allowlist. It never
/// authenticates HTML, account management, signer operations or configuration.
async fn bearer(s: State, mut request: Request<Body>, next: Next) -> Response {
    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    if request.headers().get_all("authorization").iter().count() != 1
        || request.headers().contains_key("cookie")
    {
        return error(StatusCode::UNAUTHORIZED, "ambiguous_credentials").into_response();
    }
    let Some(secret) = header
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|s| s.len() == 73)
    else {
        return error(StatusCode::UNAUTHORIZED, "invalid_token").into_response();
    };
    let secret = zeroize::Zeroizing::new(secret.to_owned());
    let db = s.db.clone();
    let token = match blocking(move || db.authenticate_token(&secret)).await {
        Ok(Some(t)) => t,
        Ok(None) => return error(StatusCode::UNAUTHORIZED, "invalid_token").into_response(),
        Err(e) => return Error::internal(e).into_response(),
    };
    let path = request.uri().path();
    let scope = match (request.method(), path) {
        (
            &Method::GET,
            "/api/v1/status"
            | "/api/v1/suites"
            | "/api/v1/packages"
            | "/api/v1/uploads"
            | "/api/v1/jobs"
            | "/api/v1/repository/diff",
        ) => "read",
        (&Method::GET, p)
            if p.starts_with("/api/v1/packages/") || p.starts_with("/api/v1/jobs/") =>
        {
            "read"
        }
        (&Method::POST, "/api/v1/uploads") => "upload",
        (&Method::POST, p) if p.starts_with("/api/v1/uploads/") && p.ends_with("/stage") => "stage",
        (&Method::POST, "/api/v1/repository/publish") => "publish",
        _ => return error(StatusCode::FORBIDDEN, "token_scope_required").into_response(),
    };
    let query =
        match serde_urlencoded::from_str::<crate::api::Search>(request.uri().query().unwrap_or(""))
        {
            Ok(q) => q,
            Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_query").into_response(),
        };
    let suite = query.suite.as_deref().unwrap_or(&s.config.repository.suite);
    if !s.config.repository.has_suite(suite) || !token.allows(scope, suite) {
        return error(StatusCode::FORBIDDEN, "token_scope_required").into_response();
    }
    request.extensions_mut().insert(Principal {
        actor: Actor {
            id: format!("token:{}", token.id),
            interface: "token".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
        },
        role: Role::Operator,
        session: None,
        token: Some(token),
    });
    next.run(request).await
}

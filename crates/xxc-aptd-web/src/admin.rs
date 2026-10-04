//! Server-rendered administration. Mutations call the same services as the JSON API.
use crate::{
    State, api,
    auth::{self, Principal},
    blocking, secure,
};
use askama::Template;
use axum::{
    Extension, Form, Router,
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Multipart, Path, Query, State as App},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use xxc_aptd_core::{auth::Role, package::Package, preview, repository};

#[derive(Template)]
#[template(path = "admin.html")]
struct Page {
    title: String,
    section: String,
    message: String,
    csrf: String,
    role: String,
    signed_in: bool,
    version: &'static str,
    stats: Vec<(String, String)>,
    records: Vec<Record>,
    users: Vec<xxc_aptd_core::auth::User>,
    details: Vec<(String, String)>,
    text: String,
    token: String,
    next: String,
    query: String,
    dashboard: Option<crate::dashboard::View>,
    suite: String,
    suites: Vec<String>,
    tokens: Vec<xxc_aptd_core::tokens::Token>,
}
struct Record {
    id: String,
    title: String,
    subtitle: String,
    state: String,
    action: String,
}
impl Page {
    fn new(section: &str, title: &str, p: Option<&Principal>) -> Self {
        Self {
            title: title.into(),
            section: section.into(),
            message: String::new(),
            csrf: p.map_or("", |p| p.csrf()).into(),
            role: p.map_or("", |p| p.role.as_str()).into(),
            signed_in: p.is_some(),
            version: xxc_aptd_core::VERSION,
            stats: vec![],
            records: vec![],
            users: vec![],
            details: vec![],
            text: String::new(),
            token: String::new(),
            next: String::new(),
            query: String::new(),
            dashboard: None,
            suite: String::new(),
            suites: vec![],
            tokens: vec![],
        }
    }
    fn suite(&mut self, s: &State) {
        self.suite = s.db.suite().into();
        self.suites = s.config.repository.suite_names();
    }
    fn response(&self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Page rendering failed").into_response(),
        }
    }
}
pub fn router(s: State) -> Router {
    let private = Router::new()
        .route("/admin/", get(dashboard))
        .route("/admin", get(|| async { Redirect::to("/admin/") }))
        .route("/admin/packages", get(packages))
        .route("/admin/packages/{id}", get(package))
        .route(
            "/admin/uploads",
            get(uploads).post(upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/admin/uploads/{id}/stage", post(stage))
        .route("/admin/staging", get(staging))
        .route("/admin/publish", get(review).post(publish))
        .route("/admin/releases", get(releases))
        .route("/admin/releases/rollback", post(rollback))
        .route("/admin/jobs", get(jobs))
        .route("/admin/jobs/{id}", get(job))
        .route("/admin/audit", get(audit))
        .route("/admin/users", get(users).post(add_user))
        .route("/admin/tokens", get(tokens).post(create_token))
        .route("/admin/tokens/{id}/revoke", post(revoke_token))
        .route("/admin/users/{id}", post(change_user))
        .route("/admin/settings", get(settings))
        .route("/admin/trust", get(trust_status))
        .route("/admin/trust/{kind}", get(trust_inventory))
        .route("/admin/system", get(system))
        .route("/admin/signing", get(signing))
        .route("/admin/keys", get(keys).post(generate_key))
        .route("/admin/logout", post(logout))
        .route("/healthz", get(api::health))
        .route("/readyz", get(api::health))
        .with_state(s.clone())
        .layer(middleware::from_fn_with_state(s.clone(), auth::gate));
    let authenticated_api = Router::new()
        .route("/api/v1/auth/session", get(auth::session))
        .route("/api/v1/auth/logout", post(auth::logout))
        .with_state(s.clone())
        .merge(api::routes(s.clone()))
        .layer(middleware::from_fn_with_state(s.clone(), auth::gate));
    let apis = Router::new()
        .route("/api/v1/auth/challenge", get(auth::challenge))
        .route("/api/v1/auth/login", post(auth::login))
        .with_state(s.clone())
        .merge(authenticated_api);
    let assets = Router::new()
        .route(
            "/static/site.css",
            get(|| async {
                (
                    [("content-type", "text/css; charset=utf-8")],
                    include_str!("../../../web/static/css/site.css"),
                )
            }),
        )
        .route(
            "/static/admin.js",
            get(|| async {
                (
                    [("content-type", "text/javascript; charset=utf-8")],
                    include_str!("../../../web/static/js/admin.js"),
                )
            }),
        )
        .route(
            "/favicon.svg",
            get(|| async {
                (
                    [("content-type", "image/svg+xml")],
                    include_str!("../../../web/static/favicon.svg"),
                )
            }),
        );
    let public = Router::new()
        .route("/admin/login", get(login_page).post(login))
        .with_state(s.clone());
    secure(
        public
            .merge(private)
            .merge(assets.clone())
            .merge(apis.clone())
            // Everything used by the browser stays under /admin for a single
            // proxy location. Existing direct-port /api/v1 routes stay valid.
            .nest("/admin", assets.merge(apis))
            .layer(DefaultBodyLimit::max(65536))
            .layer(middleware::from_fn(theme_errors))
            .layer(middleware::from_fn_with_state(s.clone(), auth::boundary)),
    )
}
async fn theme_errors(request: Request<Body>, next: Next) -> Response {
    let html = request.uri().path().starts_with("/admin")
        && !request.uri().path().starts_with("/admin/api/");
    let response = next.run(request).await;
    if html && (response.status().is_client_error() || response.status().is_server_error()) {
        let status = response.status();
        let mut p = Page::new(
            "error",
            &format!("{} / request failed", status.as_u16()),
            None,
        );
        p.message=match status { StatusCode::FORBIDDEN=>"Your role or request verification does not allow this action. Return to the page and try again.",StatusCode::CONFLICT=>"The repository changed or another operation is running. Review the current state before retrying.",StatusCode::TOO_MANY_REQUESTS=>"Too many attempts. Wait before trying again.",_=>"The operation could not be completed. Check your input or ask an administrator to inspect the operational log." }.into();
        let mut result = p.response();
        *result.status_mut() = status;
        if let Some(id) = response.headers().get("x-request-id") {
            result.headers_mut().insert("x-request-id", id.clone());
        }
        result
    } else {
        response
    }
}
async fn login_page(App(s): App<State>) -> api::Result<Response> {
    let db = s.db.clone();
    let csrf = blocking(move || db.challenge()).await?;
    let mut page = Page::new("login", "administrator login", None);
    page.csrf = csrf.clone();
    let mut response = page.response();
    auth::set_cookie(&mut response, &s, true, &csrf, 600);
    Ok(response)
}
async fn login(
    App(s): App<State>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(input): Form<auth::Credentials>,
) -> api::Result<Response> {
    let login = auth::authenticate(s.clone(), headers, peer, input).await?;
    let mut response = Redirect::to("/admin/").into_response();
    auth::set_cookie(
        &mut response,
        &s,
        false,
        &login.token,
        s.config.admin.session_lifetime_seconds,
    );
    auth::set_cookie(&mut response, &s, true, "", 0);
    Ok(response)
}
async fn logout(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    auth::end_session(&s, p).await?;
    let mut response = Redirect::to("/admin/login").into_response();
    auth::set_cookie(&mut response, &s, false, "", 0);
    Ok(response)
}
async fn dashboard(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<crate::dashboard::Query>,
) -> api::Result<Response> {
    if !q.valid() {
        return Err(auth::error(
            StatusCode::BAD_REQUEST,
            "invalid_analytics_window",
        ));
    }
    let mut page = Page::new("dashboard", "repository control", Some(&p));
    let options = (
        s.config.analytics.enabled,
        s.config.analytics.retention_days,
        s.analytics.dropped(),
        s.config.server.external_url.clone(),
    );
    let days = q.days;
    let info = blocking(move || {
        let status = s.db.status()?;
        let current = repository::current(&s.config)?;
        let traffic =
            s.db.analytics_snapshot(days, xxc_aptd_core::analytics::today())?;
        Ok((status, current, traffic))
    })
    .await?;
    page.dashboard = Some(crate::dashboard::View::new(
        info.2, q, options.0, options.1, options.2, options.3,
    ));
    page.stats = vec![
        ("version".into(), page.version.into()),
        ("active packages".into(), info.0["packages"].to_string()),
        ("staged".into(), info.0["staged"].to_string()),
        ("your role".into(), p.role.as_str().into()),
    ];
    if let Some(current) = info.1 {
        page.details = vec![
            ("generation".into(), current.id),
            ("published".into(), current.created),
            ("signing fingerprint".into(), current.fingerprint),
        ];
    } else {
        page.message="No generation published. Upload, inspect and stage packages, then review the publication.".into();
    }
    Ok(page.response())
}
async fn package_records(
    s: State,
    p: Principal,
    q: api::Search,
    section: &str,
) -> api::Result<Response> {
    let s = api::scoped(s, q.suite.as_deref())?;
    if q.q.len() > 256 {
        return Err(auth::error(StatusCode::BAD_REQUEST, "query_too_long"));
    }
    let mut page = Page::new(section, section, Some(&p));
    page.suite(&s);
    page.query = q.q.clone();
    let section = section.to_owned();
    let page_number = q.page;
    let filter = if section == "staging" { "staged" } else { "" };
    let records = blocking(move || {
        let packages = s.db.list_filtered(false, &q.q, q.page, filter)?;
        packages
            .into_iter()
            .map(|pkg| {
                let state = s.db.package_state(&pkg.id)?;
                Ok((pkg, state))
            })
            .collect::<anyhow::Result<Vec<_>>>()
    })
    .await?;
    if records.len() == 50 {
        page.next = format!(
            "/admin/{section}?page={}&q={}&suite={}",
            page_number.saturating_add(1),
            percent_encoding::utf8_percent_encode(&page.query, percent_encoding::NON_ALPHANUMERIC),
            page.suite
        );
    }
    page.records = records
        .into_iter()
        .filter(|(_, state)| section != "staging" || state == "staged")
        .map(|(pkg, state)| Record {
            id: pkg.id,
            title: pkg.name,
            subtitle: format!(
                "{} / {} / {} bytes",
                pkg.version, pkg.architecture, pkg.size
            ),
            action: if state == "uploaded" {
                "stage".into()
            } else {
                String::new()
            },
            state,
        })
        .collect();
    Ok(page.response())
}
async fn packages(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<api::Search>,
) -> api::Result<Response> {
    package_records(s, p, q, "packages").await
}
async fn uploads(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<api::Search>,
) -> api::Result<Response> {
    package_records(s, p, q, "uploads").await
}
async fn staging(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<api::Search>,
) -> api::Result<Response> {
    package_records(s, p, q, "staging").await
}
async fn package(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> api::Result<Response> {
    let package = blocking(move || s.db.get(&id))
        .await?
        .ok_or_else(|| auth::error(StatusCode::NOT_FOUND, "package_not_found"))?;
    let mut page = Page::new("package", &package.name, Some(&p));
    page.message = package.description.clone();
    page.details = vec![
        ("version".into(), package.version.clone()),
        ("architecture".into(), package.architecture.clone()),
        ("source".into(), package.source.clone()),
        ("component".into(), package.component.clone()),
        ("size".into(), package.size.to_string()),
        ("SHA-256".into(), package.sha256.clone()),
        ("pool path".into(), package.filename.clone()),
    ];
    page.details.extend(package.fields);
    Ok(page.response())
}
async fn stage(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Query(q): Query<api::Search>,
) -> api::Result<Redirect> {
    let s = api::scoped(s, q.suite.as_deref())?;
    let suite = s.db.suite().to_owned();
    api::stage_package(s, p, id).await?;
    Ok(Redirect::to(&format!("/admin/staging?suite={suite}")))
}
async fn upload(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<api::Search>,
    mut input: Multipart,
) -> api::Result<Redirect> {
    let s = api::scoped(s, q.suite.as_deref())?;
    let suite = s.db.suite().to_owned();
    p.operator()?;
    let permit = s
        .uploads
        .clone()
        .try_acquire_owned()
        .map_err(|_| api::busy())?;
    let temp = tempfile::NamedTempFile::new_in(&s.config.paths.uploads)
        .map_err(|e| api::Error::internal(e.into()))?;
    let receive = async {
        let mut csrf = input
            .next_field()
            .await
            .map_err(|_| auth::error(StatusCode::BAD_REQUEST, "invalid_multipart"))?
            .ok_or_else(|| auth::error(StatusCode::FORBIDDEN, "csrf_rejected"))?;
        if csrf.name() != Some("csrf") {
            return Err(auth::error(StatusCode::FORBIDDEN, "csrf_rejected"));
        }
        let mut token = Vec::new();
        while let Some(chunk) = csrf
            .chunk()
            .await
            .map_err(|_| auth::error(StatusCode::BAD_REQUEST, "invalid_multipart"))?
        {
            if token.len() + chunk.len() > 64 {
                return Err(auth::error(StatusCode::FORBIDDEN, "csrf_rejected"));
            }
            token.extend_from_slice(&chunk);
        }
        if !xxc_aptd_core::auth::equal_token(std::str::from_utf8(&token).unwrap_or(""), p.csrf()) {
            return Err(auth::error(StatusCode::FORBIDDEN, "csrf_rejected"));
        }
        drop(csrf);
        let mut field = input
            .next_field()
            .await
            .map_err(|_| auth::error(StatusCode::BAD_REQUEST, "invalid_multipart"))?
            .ok_or_else(|| auth::error(StatusCode::BAD_REQUEST, "package_required"))?;
        if field.name() != Some("package") {
            return Err(auth::error(StatusCode::BAD_REQUEST, "package_required"));
        }
        // Client filenames are never used as a filesystem path.
        let mut file =
            tokio::fs::File::from_std(temp.reopen().map_err(|e| api::Error::internal(e.into()))?);
        let mut size = 0u64;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| auth::error(StatusCode::BAD_REQUEST, "invalid_multipart"))?
        {
            size = size.saturating_add(chunk.len() as u64);
            if size > s.config.server.max_upload_bytes {
                return Err(auth::error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "upload_too_large",
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| api::Error::internal(e.into()))?;
        }
        drop(field);
        if input
            .next_field()
            .await
            .map_err(|_| auth::error(StatusCode::BAD_REQUEST, "invalid_multipart"))?
            .is_some()
        {
            return Err(auth::error(StatusCode::BAD_REQUEST, "one_package_required"));
        }
        file.sync_all()
            .await
            .map_err(|e| api::Error::internal(e.into()))?;
        Ok(())
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(s.config.server.upload_timeout_seconds),
        receive,
    )
    .await
    .map_err(|_| auth::error(StatusCode::REQUEST_TIMEOUT, "upload_timeout"))??;
    api::accept_upload(s, p, temp, permit).await?;
    Ok(Redirect::to(&format!("/admin/uploads?suite={suite}")))
}
async fn review(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<api::Search>,
) -> api::Result<Response> {
    let s = api::scoped(s, q.suite.as_deref())?;
    let mut page = Page::new("publish", "review publication", Some(&p));
    page.suite(&s);
    let permit = s
        .publisher
        .clone()
        .try_acquire_owned()
        .map_err(|_| api::busy())?;
    let diff = blocking(move || {
        let _permit = permit;
        preview::preview(&s.config, &s.db)
    })
    .await?;
    page.token = diff.token;
    page.stats = vec![
        ("packages added".into(), diff.added.len().to_string()),
        ("packages removed".into(), diff.removed.len().to_string()),
        ("upgrades".into(), diff.upgrades.len().to_string()),
        ("downgrades".into(), diff.downgrades.len().to_string()),
        ("size delta / bytes".into(), diff.size_delta.to_string()),
    ];
    page.details = vec![
        (
            "current generation".into(),
            diff.current_generation.unwrap_or_else(|| "none".into()),
        ),
        (
            "architectures added".into(),
            diff.architectures_added.join(", "),
        ),
        (
            "architectures removed".into(),
            diff.architectures_removed.join(", "),
        ),
    ];
    for change in diff.upgrades {
        page.details.push((
            format!("upgrade / {} / {}", change.name, change.architecture),
            format!("{} → {}", change.before, change.after),
        ));
    }
    for change in diff.downgrades {
        page.details.push((
            format!(
                "older version added / {} / {}",
                change.name, change.architecture
            ),
            format!("{} → {}", change.before, change.after),
        ));
    }
    page.records = diff
        .added
        .into_iter()
        .map(|p: Package| Record {
            id: p.id,
            title: p.name,
            subtitle: format!("{} / {} / {} bytes", p.version, p.architecture, p.size),
            state: "add".into(),
            action: String::new(),
        })
        .collect();
    page.message="Existing versions remain published. Review every change before creating a signed generation. A changed selection invalidates this review.".into();
    Ok(page.response())
}
#[derive(Deserialize)]
struct Publish {
    review_token: String,
}
async fn publish(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<api::Search>,
    Form(input): Form<Publish>,
) -> api::Result<Redirect> {
    queued(
        api::scoped(s, q.suite.as_deref())?,
        p,
        "publish",
        Some(input.review_token),
    )
    .await
}
#[derive(Deserialize)]
struct Rollback {
    generation: String,
}
async fn rollback(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Form(input): Form<Rollback>,
) -> api::Result<Redirect> {
    queued(s, p, "rollback", Some(input.generation)).await
}
async fn queued(
    s: State,
    p: Principal,
    action: &'static str,
    value: Option<String>,
) -> api::Result<Redirect> {
    let (_, axum::Json(job)) = api::enqueue(s, p, action, value).await?;
    Ok(Redirect::to(&format!(
        "/admin/jobs/{}",
        job["job_id"].as_str().unwrap_or("")
    )))
}
async fn releases(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    let generations = blocking(move || repository::generations(&s.config)).await?;
    let mut page = Page::new("releases", "signed generations", Some(&p));
    page.records = generations
        .into_iter()
        .map(|m| Record {
            id: m.id.clone(),
            title: m.id,
            subtitle: m.created,
            state: format!("{} packages", m.packages.len()),
            action: "rollback".into(),
        })
        .collect();
    Ok(page.response())
}
async fn jobs(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    let jobs = blocking(move || s.db.jobs()).await?;
    let mut page = Page::new("jobs", "background jobs", Some(&p));
    page.records = jobs
        .iter()
        .map(|j| Record {
            id: j["id"].as_str().unwrap_or("").into(),
            title: j["id"].as_str().unwrap_or("").into(),
            subtitle: j["created"].as_str().unwrap_or("").into(),
            state: j["state"].as_str().unwrap_or("").into(),
            action: String::new(),
        })
        .collect();
    Ok(page.response())
}
async fn job(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> api::Result<Response> {
    let copy = id.clone();
    let value = blocking(move || Ok(s.db.jobs()?.into_iter().find(|v| v["id"] == copy)))
        .await?
        .ok_or_else(|| auth::error(StatusCode::NOT_FOUND, "job_not_found"))?;
    let mut page = Page::new("job", "job result", Some(&p));
    page.token = id;
    page.message = value["message"].as_str().unwrap_or("").into();
    for key in ["id", "state", "created", "finished"] {
        page.details
            .push((key.into(), value[key].as_str().unwrap_or("pending").into()));
    }
    Ok(page.response())
}
async fn audit(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    let events = blocking(move || s.db.audits()).await?;
    let mut page = Page::new("audit", "audit records", Some(&p));
    page.message="Most recent 100 actions. Actors are opaque local user IDs; session tokens and passwords are never recorded.".into();
    page.text =
        serde_json::to_string_pretty(&events).map_err(|e| api::Error::internal(e.into()))?;
    Ok(page.response())
}
async fn users(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    p.administrator()?;
    let mut page = Page::new("users", "local users", Some(&p));
    page.users = blocking(move || s.db.users()).await?;
    Ok(page.response())
}
#[derive(Deserialize)]
struct UserForm {
    username: String,
    password: String,
    role: Role,
}
async fn add_user(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Form(input): Form<UserForm>,
) -> api::Result<Redirect> {
    api::add_user(
        s,
        p,
        api::AddUser {
            username: input.username,
            password: input.password,
            role: input.role,
        },
    )
    .await?;
    Ok(Redirect::to("/admin/users"))
}
#[derive(Deserialize)]
struct ChangeForm {
    action: String,
    role: Option<Role>,
    password: Option<String>,
}
async fn change_user(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Form(input): Form<ChangeForm>,
) -> api::Result<Redirect> {
    let change = match input.action.as_str() {
        "enable" => api::ChangeUser::Enable,
        "disable" => api::ChangeUser::Disable,
        "delete" => api::ChangeUser::Delete,
        "role" => api::ChangeUser::Role {
            role: input
                .role
                .ok_or_else(|| auth::error(StatusCode::BAD_REQUEST, "role_required"))?,
        },
        "password" => api::ChangeUser::Password {
            password: input.password.unwrap_or_default(),
        },
        _ => return Err(auth::error(StatusCode::BAD_REQUEST, "invalid_action")),
    };
    api::change_user(s, p, id, change).await?;
    Ok(Redirect::to("/admin/users"))
}
async fn settings(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    p.administrator()?;
    let mut page = Page::new("settings", "effective configuration", Some(&p));
    page.message="Configuration is read from the server file. Validate changes with xxc-aptd config check, then restart the service.".into();
    page.text =
        serde_json::to_string_pretty(&*s.config).map_err(|e| api::Error::internal(e.into()))?;
    Ok(page.response())
}
async fn system(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    let axum::Json(info) = api::health(App(s)).await?;
    let mut page = Page::new("system", "operational health", Some(&p));
    page.text = serde_json::to_string_pretty(&info).map_err(|e| api::Error::internal(e.into()))?;
    Ok(page.response())
}
async fn signing(App(s): App<State>, Extension(p): Extension<Principal>) -> Response {
    let mut page = Page::new("signing", "repository signing", Some(&p));
    page.message="OpenPGP signing is configured on the server. Private key material is never available through this application.".into();
    page.details = vec![
        ("backend".into(), s.config.signing.backend.clone()),
        (
            "remote key ID".into(),
            s.config.signing.remote_key_id.clone(),
        ),
        (
            "configured fingerprint".into(),
            s.config.signing.fingerprint.clone(),
        ),
        (
            "public key".into(),
            format!(
                "{}/repo/thugsred-archive-keyring.asc",
                s.config.server.external_url
            ),
        ),
    ];
    page.response()
}

async fn trust_status(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
) -> api::Result<Response> {
    let status = crate::trust::read(&s, &p, "status", Default::default()).await?;
    let mut page = Page::new("trust", "XXC Trust / infrastructure identity", Some(&p));
    page.message =
        "Read-only X.509 inventory. APT metadata uses the separate OpenPGP signer.".into();
    for key in [
        "enabled",
        "connected",
        "authorities",
        "templates",
        "authority_available",
        "template_available",
    ] {
        if let Some(value) = status.get(key) {
            page.details
                .push((key.replace('_', " "), value.to_string()));
        }
    }
    Ok(page.response())
}
async fn trust_inventory(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(kind): Path<String>,
    Query(q): Query<xxc_aptd_core::trust::CertificateQuery>,
) -> api::Result<Response> {
    p.administrator()?;
    if !["authorities", "templates", "certificates"].contains(&kind.as_str()) {
        return Err(api::Error(
            StatusCode::NOT_FOUND,
            "not_found",
            p.actor.request_id.clone(),
        ));
    }
    let value = crate::trust::read(
        &s,
        &p,
        &kind,
        xxc_aptd_core::trust::CertificateQuery {
            page: q.page,
            q: q.q.clone(),
            status: q.status.clone(),
        },
    )
    .await?;
    let mut page = Page::new("trust", &format!("XXC Trust / {kind}"), Some(&p));
    page.query = q.q.clone();
    page.token = kind.clone();
    if let Some(items) = value["items"].as_array() {
        for item in items {
            let field = |key: &str| item[key].as_str().unwrap_or("").to_owned();
            let (title, state, subtitle) = match kind.as_str() {
                "authorities" => (
                    field("name"),
                    if item["active"] == 1 {
                        "active"
                    } else {
                        "inactive"
                    }
                    .into(),
                    format!("expires {}", field("not_after")),
                ),
                "templates" => (
                    field("name"),
                    field("algorithm"),
                    format!(
                        "{} days · {} · {}",
                        item["days"],
                        field("eku"),
                        field("domain_suffix")
                    ),
                ),
                _ => (
                    field("label"),
                    field("status"),
                    format!(
                        "expires {} · {} · SHA-256 {}",
                        field("not_after"),
                        item["sans"]
                            .as_array()
                            .map(|a| a
                                .iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", "))
                            .unwrap_or_default(),
                        field("fingerprint")
                    ),
                ),
            };
            page.records.push(Record {
                id: field("id"),
                title,
                state,
                subtitle,
                action: String::new(),
            });
        }
    }
    if let (Some(current), Some(pages)) = (value["page"].as_u64(), value["pages"].as_u64()) {
        page.stats
            .push(("certificates".into(), value["total"].to_string()));
        page.stats
            .push(("page".into(), format!("{current} / {pages}")));
        if current < pages {
            let query = serde_urlencoded::to_string([
                ("page", (current + 1).to_string()),
                ("q", q.q),
                ("status", q.status),
            ])
            .map_err(|e| api::Error::internal(e.into()))?;
            page.next = format!("/admin/trust/certificates?{query}");
        }
    }
    Ok(page.response())
}

async fn keys(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Query(q): Query<xxc_aptd_core::trust::openpgp::KeyQuery>,
) -> api::Result<Response> {
    let value = crate::keys::list_keys(
        &s,
        &p,
        xxc_aptd_core::trust::openpgp::KeyQuery {
            page: q.page,
            q: q.q.clone(),
        },
    )
    .await?;
    let mut page = Page::new("keys", "XXC Trust / OpenPGP keys", Some(&p));
    page.message="Private keys stay in XXC Trust. Generating a key does not activate it. Pin its ID and fingerprint in the server configuration after distributing the public key.".into();
    page.query = q.q.clone();
    if let Some(items) = value["items"].as_array() {
        for key in items {
            let field = |name: &str| key[name].as_str().unwrap_or("").to_owned();
            page.records.push(Record {
                id: field("id"),
                title: field("label"),
                subtitle: format!(
                    "{} · {} · {}",
                    field("fingerprint"),
                    field("algorithm"),
                    field("not_after")
                ),
                state: field("status"),
                action: "public-key".into(),
            });
        }
    }
    if let (Some(current), Some(pages)) = (value["page"].as_u64(), value["pages"].as_u64())
        && current < pages
    {
        let query = serde_urlencoded::to_string([("page", (current + 1).to_string()), ("q", q.q)])
            .map_err(|e| api::Error::internal(e.into()))?;
        page.next = format!("/admin/keys?{query}");
    }
    Ok(page.response())
}
#[derive(Deserialize)]
struct NewKey {
    name: String,
    email: String,
    algorithm: String,
    days: u32,
}
async fn generate_key(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Form(input): Form<NewKey>,
) -> api::Result<Redirect> {
    crate::keys::create_key(
        &s,
        &p,
        xxc_aptd_core::trust::openpgp::GenerateKey {
            name: input.name,
            email: input.email,
            algorithm: input.algorithm,
            days: input.days,
        },
    )
    .await?;
    Ok(Redirect::to("/admin/keys"))
}

async fn tokens(App(s): App<State>, Extension(p): Extension<Principal>) -> api::Result<Response> {
    p.administrator()?;
    let mut page = Page::new("tokens", "project API tokens", Some(&p));
    page.suite(&s);
    page.tokens = blocking(move || s.db.tokens()).await?;
    Ok(page.response())
}
#[derive(Deserialize)]
struct TokenForm {
    name: String,
    scopes: String,
    suites: String,
    days: u32,
}
async fn create_token(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Form(input): Form<TokenForm>,
) -> api::Result<Response> {
    p.administrator()?;
    let input = xxc_aptd_core::tokens::CreateToken {
        name: input.name,
        scopes: input.scopes.split_whitespace().map(str::to_owned).collect(),
        suites: input.suites.split_whitespace().map(str::to_owned).collect(),
        days: input.days,
    };
    let actor = p.actor.clone();
    let (token, secret) = blocking(move || s.db.create_token(&s.config, &actor, input)).await?;
    let mut page = Page::new("token-created", "save your project token", Some(&p));
    page.message="This credential is shown once. Save it in your project's protected CI secret store. It cannot be recovered; revoke it and create another if lost.".into();
    page.token = secret.to_string();
    page.details = vec![
        ("token ID".into(), token.id),
        ("scopes".into(), token.scopes.join(" ")),
        ("suites".into(), token.suites.join(" ")),
    ];
    Ok(page.response())
}
async fn revoke_token(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> api::Result<Redirect> {
    p.administrator()?;
    blocking(move || s.db.revoke_token(&p.actor, &id)).await?;
    Ok(Redirect::to("/admin/tokens"))
}

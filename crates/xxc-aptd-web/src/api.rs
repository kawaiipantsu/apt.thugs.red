use crate::{State, auth::Principal, blocking, secure};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, Query, State as App},
    http::{HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use xxc_aptd_core::{
    auth::{Role, UserChange},
    package, repository,
};

pub struct Error(pub StatusCode, pub &'static str, pub String);
impl Error {
    pub(crate) fn internal(e: anyhow::Error) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        tracing::error!(request_id=%id,error=%e,"management operation failed");
        Self(StatusCode::BAD_REQUEST, "operation_failed", id)
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let id = self.2.clone();
        let mut response = (self.0, Json(json!({"error":{
            "code":self.1,
            "message":"Operation could not be completed; inspect the privileged operational log",
            "request_id":self.2
        }}))).into_response();
        if let Ok(value) = HeaderValue::from_str(&id) {
            response.headers_mut().insert("x-request-id", value);
        }
        response
    }
}
pub(crate) type Result<T> = std::result::Result<T, Error>;
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self::internal(e)
    }
}
pub(crate) fn busy() -> Error {
    Error(
        StatusCode::CONFLICT,
        "operation_already_running",
        uuid::Uuid::new_v4().to_string(),
    )
}
pub fn router(state: State) -> Router {
    routes(state).layer(middleware::from_fn(crate::auth::local))
}
pub(crate) fn routes(state: State) -> Router {
    secure(
        Router::new()
            .route("/api/v1/status", get(status))
            .route(
                "/api/v1/keys",
                get(crate::keys::list).post(crate::keys::generate),
            )
            .route("/api/v1/keys/{id}", get(crate::keys::show))
            .route("/api/v1/keys/{id}/public", get(crate::keys::export))
            .route("/api/v1/keys/{id}/verify", get(crate::keys::verify))
            .route("/api/v1/trust/status", get(crate::trust::status))
            .route("/api/v1/trust/authorities", get(crate::trust::authorities))
            .route("/api/v1/trust/templates", get(crate::trust::templates))
            .route(
                "/api/v1/trust/certificates",
                get(crate::trust::certificates),
            )
            .route("/api/v1/health", get(health))
            .route("/api/v1/packages", get(packages))
            .route("/api/v1/packages/{id}", get(show))
            .route("/api/v1/uploads", get(packages).post(upload))
            .route("/api/v1/uploads/{id}/stage", post(stage))
            .route("/api/v1/repository/generations", get(generations))
            .route("/api/v1/repository/publish", post(publish))
            .route("/api/v1/repository/rollback", post(rollback))
            .route("/api/v1/repository/verify", post(verify))
            .route("/api/v1/repository/reindex", post(reindex))
            .route("/api/v1/jobs", get(jobs))
            .route("/api/v1/jobs/{id}", get(job))
            .route("/api/v1/audit", get(audit))
            .route("/api/v1/config", get(config))
            .route("/api/v1/repository/diff", get(diff))
            .route("/api/v1/users", get(users).post(user_add))
            .route("/api/v1/users/{id}", post(user_change))
            .fallback(|| async {
                Error(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    uuid::Uuid::new_v4().to_string(),
                )
            })
            .layer(middleware::from_fn(api_response))
            .with_state(state),
    )
}
async fn api_response(request: Request<Body>, next: Next) -> Response {
    let id = request
        .extensions()
        .get::<Principal>()
        .map(|p| p.actor.request_id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut response = next.run(request).await;
    if (response.status().is_client_error() || response.status().is_server_error())
        && response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
    {
        response = Error(response.status(), "invalid_request", id.clone()).into_response();
    }
    if !response.headers().contains_key("x-request-id")
        && let Ok(value) = HeaderValue::from_str(&id)
    {
        response.headers_mut().insert("x-request-id", value);
    }
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
async fn status(App(s): App<State>) -> Result<Json<Value>> {
    Ok(Json(
        blocking(move || {
            let mut v = s.db.status()?;
            v["administrative_http"] = json!(if s.config.admin.enabled {
                "authentication_required"
            } else {
                "disabled"
            });
            v["generation"] = json!(repository::current(&s.config)?.map(|m| m.id));
            Ok(v)
        })
        .await?,
    ))
}
pub(crate) async fn health(App(s): App<State>) -> Result<Json<Value>> {
    Ok(Json(blocking(move||{s.config.check_directories()?;let check:String=s.db.connect()?.query_row("PRAGMA quick_check",[],|r|r.get(0))?;anyhow::ensure!(check=="ok","SQLite health check failed");let m=repository::current(&s.config)?;Ok(json!({"database":"ok","directories":"ok","published":m.is_some(),"signer_configured":!s.config.signing.fingerprint.is_empty()}))}).await?))
}
#[derive(Default, Deserialize)]
pub struct Search {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub page: u32,
}
async fn packages(App(s): App<State>, Query(q): Query<Search>) -> Result<Json<Value>> {
    if q.q.len() > 256 {
        return Err(Error(
            StatusCode::BAD_REQUEST,
            "query_too_long",
            uuid::Uuid::new_v4().to_string(),
        ));
    }
    Ok(Json(
        json!({"packages":blocking(move||s.db.list(false,&q.q,q.page)).await?}),
    ))
}
async fn show(App(s): App<State>, Path(id): Path<String>) -> Result<Json<Value>> {
    let package = blocking(move || s.db.get(&id)).await?;
    match package {
        Some(p) => Ok(Json(json!(p))),
        None => Err(Error(
            StatusCode::NOT_FOUND,
            "package_not_found",
            uuid::Uuid::new_v4().to_string(),
        )),
    }
}
async fn stage(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    stage_package(s, p, id).await?;
    Ok(Json(json!({"staged":true})))
}
pub(crate) async fn stage_package(s: State, p: Principal, id: String) -> Result<()> {
    p.operator()?;
    let permit = s
        .publisher
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    blocking(move || {
        let _permit = permit;
        s.db.audit_event(&p.actor, "package.stage", &id, "requested", &json!({}))?;
        s.db.stage(&id)?;
        s.db.audit_event(&p.actor, "package.stage", &id, "succeeded", &json!({}))
    })
    .await?;
    Ok(())
}
async fn upload(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    body: Body,
) -> Result<Json<Value>> {
    p.operator()?;
    let permit = s.uploads.clone().try_acquire_owned().map_err(|_| busy())?;
    let temp = tempfile::NamedTempFile::new_in(&s.config.paths.uploads)
        .map_err(|e| Error::internal(e.into()))?;
    let file = temp.reopen().map_err(|e| Error::internal(e.into()))?;
    let mut file = tokio::fs::File::from_std(file);
    let limit = s.config.server.max_upload_bytes;
    let receive = async {
        let mut stream = body.into_data_stream();
        let mut total = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::internal(e.into()))?;
            total = total.saturating_add(chunk.len() as u64);
            if total > limit {
                return Err(Error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "upload_too_large",
                    uuid::Uuid::new_v4().to_string(),
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| Error::internal(e.into()))?;
        }
        file.sync_all()
            .await
            .map_err(|e| Error::internal(e.into()))?;
        Ok(())
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(s.config.server.upload_timeout_seconds),
        receive,
    )
    .await
    .map_err(|_| {
        Error(
            StatusCode::REQUEST_TIMEOUT,
            "upload_timeout",
            uuid::Uuid::new_v4().to_string(),
        )
    })??;
    drop(file);
    let package = accept_upload(s, p, temp, permit).await?;
    Ok(Json(json!(package)))
}
pub(crate) async fn accept_upload(
    s: State,
    p: Principal,
    temp: tempfile::NamedTempFile,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<package::Package> {
    p.operator()?;
    Ok(blocking(move || {
        let _permit = permit;
        let package = package::ingest(&s.config, &s.db, temp.path())?;
        s.db.audit_event(
            &p.actor,
            "upload.inspect",
            &package.id,
            "succeeded",
            &json!({}),
        )?;
        Ok(package)
    })
    .await?)
}
async fn generations(App(s): App<State>) -> Result<Json<Value>> {
    Ok(Json(
        json!({"generations":blocking(move||repository::generations(&s.config)).await?}),
    ))
}
async fn jobs(App(s): App<State>) -> Result<Json<Value>> {
    Ok(Json(json!({"jobs":blocking(move||s.db.jobs()).await?})))
}
async fn job(App(s): App<State>, Path(id): Path<String>) -> Result<Json<Value>> {
    let found = blocking(move || Ok(s.db.jobs()?.into_iter().find(|j| j["id"] == id))).await?;
    found.map(Json).ok_or(Error(
        StatusCode::NOT_FOUND,
        "job_not_found",
        uuid::Uuid::new_v4().to_string(),
    ))
}
async fn audit(App(s): App<State>) -> Result<Json<Value>> {
    Ok(Json(json!({"audit":blocking(move||s.db.audits()).await?})))
}
async fn config(App(s): App<State>, Extension(p): Extension<Principal>) -> Result<Json<Value>> {
    p.administrator()?;
    Ok(Json(json!(&*s.config)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publish {
    review_token: String,
}
async fn publish(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Json(input): Json<Publish>,
) -> Result<(StatusCode, Json<Value>)> {
    enqueue(s, p, "publish", Some(input.review_token)).await
}
async fn diff(App(s): App<State>) -> Result<Json<Value>> {
    let permit = s
        .publisher
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    Ok(Json(
        blocking(move || {
            let _permit = permit;
            Ok(json!(xxc_aptd_core::preview::preview(&s.config, &s.db)?))
        })
        .await?,
    ))
}
#[derive(Deserialize)]
struct Rollback {
    generation: String,
}
async fn rollback(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Json(input): Json<Rollback>,
) -> Result<(StatusCode, Json<Value>)> {
    enqueue(s, p, "rollback", Some(input.generation)).await
}
async fn verify(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
) -> Result<(StatusCode, Json<Value>)> {
    enqueue(s, p, "verify", None).await
}
async fn reindex(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
) -> Result<(StatusCode, Json<Value>)> {
    enqueue(s, p, "reindex", None).await
}
pub(crate) async fn enqueue(
    s: State,
    p: Principal,
    action: &'static str,
    generation: Option<String>,
) -> Result<(StatusCode, Json<Value>)> {
    p.operator()?;
    let permit = s
        .publisher
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    let id = uuid::Uuid::new_v4().to_string();
    let job_id = id.clone();
    let (accepted, receipt) = tokio::sync::oneshot::channel();
    // The bounded task owns the permit before persisting a job. A disconnected
    // HTTP caller cannot leave a recorded job without a worker.
    tokio::spawn(async move {
        let outcome = blocking(move || {
            let _permit = permit;
            let setup = (|| -> Result<()> {
                if action == "publish" {
                    let review = xxc_aptd_core::preview::preview(&s.config, &s.db)?;
                    if generation.as_deref() != Some(review.token.as_str()) {
                        return Err(Error(
                            StatusCode::CONFLICT,
                            "publication_review_changed",
                            p.actor.request_id.clone(),
                        ));
                    }
                }
                s.db.start_job(&p.actor, action, &job_id)?;
                Ok(())
            })();
            if let Err(error) = setup {
                let _ = accepted.send(Err(error));
                return Ok(());
            }
            let _ = accepted.send(Ok(()));
            let result = match action {
                "publish" => (|| {
                    let signer = xxc_aptd_core::signing::configured(
                        &s.config,
                        s.trust.as_ref(),
                        tokio::runtime::Handle::current(),
                    )?;
                    repository::publish(&s.config, &s.db, signer.as_ref()).map(|_| ())
                })(),
                "rollback" => {
                    repository::rollback(&s.config, &s.db, generation.as_deref().unwrap_or(""))
                        .map(|_| ())
                }
                "verify" => repository::current(&s.config).and_then(|m| {
                    repository::verify(
                        &s.config,
                        &m.ok_or_else(|| anyhow::anyhow!("No published generation"))?,
                    )
                }),
                "reindex" => repository::reconcile(&s.config, &s.db),
                _ => unreachable!(),
            };
            let (state, message) = match result {
                Ok(()) => ("succeeded", "Operation completed"),
                Err(e) => {
                    tracing::error!(job_id=%job_id,error=%e,"repository job failed");
                    (
                        "failed",
                        "Operation failed; inspect privileged logs using the job ID",
                    )
                }
            };
            s.db.finish_job(&p.actor, action, &job_id, state, message)?;
            Ok(())
        })
        .await;
        if let Err(e) = outcome {
            tracing::error!(error=%e,"could not persist job result");
        }
    });
    receipt
        .await
        .map_err(|_| Error::internal(anyhow::anyhow!("Job acceptance interrupted")))??;
    Ok((StatusCode::ACCEPTED, Json(json!({"job_id":id}))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddUser {
    pub username: String,
    pub password: String,
    pub role: Role,
}
async fn users(App(s): App<State>, Extension(p): Extension<Principal>) -> Result<Json<Value>> {
    p.administrator()?;
    Ok(Json(json!({"users":blocking(move || s.db.users()).await?})))
}
async fn user_add(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Json(input): Json<AddUser>,
) -> Result<Json<Value>> {
    Ok(Json(json!(add_user(s, p, input).await?)))
}
pub(crate) async fn add_user(
    s: State,
    p: Principal,
    input: AddUser,
) -> Result<xxc_aptd_core::auth::User> {
    p.administrator()?;
    let password = zeroize::Zeroizing::new(input.password);
    let permit = s
        .passwords
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    Ok(blocking(move || {
        let _permit = permit;
        s.db.add_user(&p.actor, input.role, &input.username, &password, p.role)
    })
    .await?)
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ChangeUser {
    Password { password: String },
    Role { role: Role },
    Enable,
    Disable,
    Delete,
}
async fn user_change(
    App(s): App<State>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(input): Json<ChangeUser>,
) -> Result<Json<Value>> {
    change_user(s, p, id, input).await?;
    Ok(Json(json!({"changed":true})))
}
pub(crate) async fn change_user(
    s: State,
    p: Principal,
    id: String,
    input: ChangeUser,
) -> Result<()> {
    p.administrator()?;
    let permit = s
        .passwords
        .clone()
        .try_acquire_owned()
        .map_err(|_| busy())?;
    let change = match input {
        ChangeUser::Password { password } => UserChange::Password(password),
        ChangeUser::Role { role } => UserChange::Role(role),
        ChangeUser::Enable => UserChange::Enabled(true),
        ChangeUser::Disable => UserChange::Enabled(false),
        ChangeUser::Delete => UserChange::Delete,
    };
    blocking(move || {
        let _permit = permit;
        s.db.change_user(&p.actor, &id, change, p.role)
    })
    .await?;
    Ok(())
}

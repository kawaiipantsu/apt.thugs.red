pub mod admin;
pub mod analytics;
pub mod api;
mod api_docs;
pub mod auth;
mod dashboard;
pub mod keys;
pub mod public;
pub mod trust;

use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Request},
    middleware::{self, Next},
    response::Response,
};
use std::sync::Arc;
use tokio::sync::Semaphore;
use xxc_aptd_core::{config::Config, db::Database};

#[derive(Clone)]
pub struct State {
    pub config: Arc<Config>,
    pub db: Database,
    pub publisher: Arc<Semaphore>,
    pub uploads: Arc<Semaphore>,
    pub passwords: Arc<Semaphore>,
    pub trust: Option<xxc_aptd_core::trust::TrustClient>,
    pub analytics: analytics::Collector,
}
impl State {
    pub fn new(config: Config, db: Database) -> anyhow::Result<Self> {
        let credential_directory =
            std::env::var_os("CREDENTIALS_DIRECTORY").map(std::path::PathBuf::from);
        let trust = xxc_aptd_core::trust::TrustClient::new(
            &config.xxc_trust,
            credential_directory.as_deref(),
        )?;
        let upload_limit = config.server.max_concurrent_uploads;
        let analytics = analytics::Collector::start(&config, db.clone())?;
        Ok(Self {
            analytics,
            trust,
            config: Arc::new(config),
            db,
            publisher: Arc::new(Semaphore::new(1)),
            uploads: Arc::new(Semaphore::new(upload_limit)),
            passwords: Arc::new(Semaphore::new(2)),
        })
    }
}
pub async fn blocking<T, F>(f: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await?
}
async fn headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    h.insert("content-security-policy",HeaderValue::from_static("default-src 'none'; style-src 'self'; script-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"));
    response
}
pub fn secure(router: Router) -> Router {
    router.layer(middleware::from_fn(headers))
}

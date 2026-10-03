use crate::{State, blocking, secure};
use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::{OriginalUri, Path, Query, State as App},
    http::{HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use tower::ServiceExt;
use xxc_aptd_core::{package::Package, repository};

#[derive(Template)]
#[template(path = "page.html")]
struct Page {
    site: String,
    main_site: String,
    heading: String,
    intro: String,
    prompt: String,
    version: &'static str,
    stats: Vec<(String, String)>,
    search: bool,
    query: String,
    packages: Vec<Package>,
    entries: Vec<Entry>,
    browser: bool,
    parent: String,
    directory: String,
    details: Vec<(String, String)>,
    download: String,
    command: String,
    command_title: String,
    note: String,
    next: String,
}
struct Entry {
    name: String,
    url: String,
    kind: String,
    size: String,
    action: String,
}
impl Page {
    fn new(s: &State, title: &str, intro: &str) -> Self {
        Self {
            site: s.config.web.site_name.clone(),
            main_site: s.config.web.main_site_url.clone(),
            heading: title.into(),
            intro: intro.into(),
            prompt: "ls /repo".into(),
            version: xxc_aptd_core::VERSION,
            stats: vec![],
            search: false,
            query: String::new(),
            packages: vec![],
            entries: vec![],
            browser: false,
            parent: String::new(),
            directory: String::new(),
            details: vec![],
            download: String::new(),
            command: String::new(),
            command_title: "client setup / deb822".into(),
            note: String::new(),
            next: String::new(),
        }
    }
    fn response(self) -> Response {
        match self.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => {
                tracing::error!(error=%e,"template failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "Page rendering failed").into_response()
            }
        }
    }
}
pub fn router(state: State) -> Router {
    secure(
        Router::new()
            .route("/", get(home))
            .route(
                "/healthz",
                get(|| async { axum::Json(serde_json::json!({"status":"ok"})) }),
            )
            .route("/packages", get(packages))
            .route("/search", get(packages))
            .route("/packages/{name}", get(package))
            .route("/help", get(help))
            .route("/about", get(about))
            .route("/releases", get(release))
            .route("/releases/{suite}", get(release_suite))
            .route("/repo", get(repository_root))
            .route("/repo/", get(repository_root))
            .route("/repo/{*path}", get(repository_file))
            .route(
                "/favicon.svg",
                get(|| async {
                    (
                        [("content-type", "image/svg+xml")],
                        include_str!("../../../web/static/favicon.svg"),
                    )
                }),
            )
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
                "/static/site.js",
                get(|| async {
                    (
                        [("content-type", "text/javascript; charset=utf-8")],
                        include_str!("../../../web/static/js/site.js"),
                    )
                }),
            )
            .route(
                "/robots.txt",
                get(|| async {
                    (
                        [("content-type", "text/plain")],
                        "User-agent: *\nDisallow: /repo/pool/\nDisallow: /admin\nDisallow: /api/\n",
                    )
                }),
            )
            .route("/sitemap.xml", get(sitemap))
            .fallback(not_found)
            .layer(middleware::from_fn_with_state(state.clone(), validate_host))
            .with_state(state),
    )
}
async fn validate_host(App(s): App<State>, request: Request<Body>, next: Next) -> Response {
    let supplied = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<axum::http::uri::Authority>().ok());
    let configured = s
        .config
        .server
        .external_url
        .split_once("://")
        .map(|(_, v)| v)
        .unwrap_or("");
    let expected = configured.parse::<axum::http::uri::Authority>().ok();
    let allowed = supplied.is_some_and(|authority| {
        let host = authority.host().trim_matches(['[', ']']);
        host == "localhost"
            || host == "127.0.0.1"
            || host == "::1"
            || host == s.config.server.public_listen.ip().to_string()
            || expected
                .as_ref()
                .is_some_and(|value| value.host().eq_ignore_ascii_case(authority.host()))
    });
    if !allowed {
        return error(&s, StatusCode::BAD_REQUEST, "unrecognized host");
    }
    next.run(request).await
}
fn error(s: &State, status: StatusCode, title: &str) -> Response {
    let mut p = Page::new(s, title, "The requested resource is unavailable.");
    p.prompt = format!("exit {}", status.as_u16());
    let mut response = p.response();
    *response.status_mut() = status;
    response
}
async fn not_found(App(s): App<State>) -> Response {
    error(&s, StatusCode::NOT_FOUND, "404 / path not found")
}
async fn home(App(s): App<State>) -> Response {
    let clone = s.clone();
    let result = blocking(move || {
        Ok((
            clone.db.status()?,
            repository::current(&clone.config)?,
            clone.db.list(true, "", 0)?,
        ))
    })
    .await;
    let (status, current, packages) = match result {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error=%e,"home query failed");
            return error(
                &s,
                StatusCode::SERVICE_UNAVAILABLE,
                "repository unavailable",
            );
        }
    };
    let mut p = Page::new(
        &s,
        &format!("{}.", s.config.web.tagline.trim_end_matches('.')),
        &s.config.repository.description,
    );
    p.prompt = "apt update".into();
    p.search = true;
    p.stats = vec![
        ("package versions".into(), status["packages"].to_string()),
        ("suite".into(), s.config.repository.suite.clone()),
        (
            "architectures".into(),
            s.config.repository.architectures.join(" / "),
        ),
        (
            "repository".into(),
            if current.is_some() {
                "published"
            } else {
                "awaiting publication"
            }
            .into(),
        ),
    ];
    p.packages = packages.into_iter().take(6).collect();
    p.command = setup(&s);
    if let Some(m) = current {
        p.details = vec![
            ("published".into(), m.created),
            ("signing fingerprint".into(), m.fingerprint),
        ];
    }
    p.note="Verify the signing fingerprint through a separate trusted channel before installing the repository key.".into();
    p.response()
}
async fn packages(App(s): App<State>, Query(q): Query<crate::api::Search>) -> Response {
    if q.q.len() > 256 {
        return error(&s, StatusCode::BAD_REQUEST, "search query too long");
    }
    let mut p = Page::new(
        &s,
        "package index.",
        "Search published package names and descriptions.",
    );
    p.search = true;
    p.query = q.q.clone();
    let db = s.db.clone();
    let query = q.q.clone();
    match blocking(move || db.list(true, &query, q.page)).await {
        Ok(list) => {
            if list.len() == 50 {
                p.next = format!(
                    "/search?q={}&page={}",
                    utf8_percent_encode(&q.q, NON_ALPHANUMERIC),
                    q.page.saturating_add(1)
                );
            }
            p.packages = list;
            if p.packages.is_empty() {
                p.note = "No published packages match this search.".into();
            }
            p.response()
        }
        Err(e) => {
            tracing::error!(error=%e,"package search failed");
            error(&s, StatusCode::SERVICE_UNAVAILABLE, "search unavailable")
        }
    }
}
async fn package(App(s): App<State>, Path(name): Path<String>) -> Response {
    if !xxc_aptd_core::package::package_name(&name) {
        return error(&s, StatusCode::NOT_FOUND, "package not found");
    }
    let query = name.clone();
    let db = s.db.clone();
    let result = blocking(move || db.list(true, &query, 0)).await;
    let list = match result {
        Ok(list) => list
            .into_iter()
            .filter(|p| p.name == name)
            .collect::<Vec<_>>(),
        Err(_) => return error(&s, StatusCode::SERVICE_UNAVAILABLE, "package unavailable"),
    };
    let Some(pkg) = list.last() else {
        return error(&s, StatusCode::NOT_FOUND, "package not found");
    };
    let mut p = Page::new(&s, &pkg.name, &pkg.description);
    p.prompt = format!("apt show {}", pkg.name);
    p.command_title = "install package".into();
    p.command = format!("sudo apt install {}", pkg.name);
    p.details = vec![
        ("version".into(), pkg.version.clone()),
        ("architecture".into(), pkg.architecture.clone()),
        ("component".into(), pkg.component.clone()),
        ("download size".into(), format!("{} bytes", pkg.size)),
        ("sha256".into(), pkg.sha256.clone()),
        ("pool path".into(), pkg.filename.clone()),
    ];
    for key in [
        "section",
        "installed-size",
        "depends",
        "recommends",
        "suggests",
        "homepage",
    ] {
        if let Some(v) = pkg.fields.get(key) {
            p.details.push((key.into(), v.clone()));
        }
    }
    p.download = format!("/repo/{}", pkg.filename);
    p.packages = list;
    p.response()
}
fn sources(s: &State) -> String {
    format!(
        "Types: deb\nURIs: {}/repo\nSuites: {}\nComponents: {}\nArchitectures: {}\nSigned-By: /usr/share/keyrings/thugsred-archive-keyring.gpg\n",
        s.config.server.external_url,
        s.config.repository.suite,
        s.config.repository.components.join(" "),
        s.config.repository.architectures.join(" ")
    )
}
fn legacy(s: &State) -> String {
    format!(
        "deb [arch={} signed-by=/usr/share/keyrings/thugsred-archive-keyring.gpg] {}/repo {} {}\n",
        s.config.repository.architectures.join(","),
        s.config.server.external_url,
        s.config.repository.suite,
        s.config.repository.components.join(" ")
    )
}
fn setup(s: &State) -> String {
    format!(
        r#"(
set -eu
xxc_setup_dir="$(mktemp -d)"
trap 'rm -rf "$xxc_setup_dir"' EXIT
curl -fsSLo "$xxc_setup_dir/keyring.gpg" '{}/repo/{}'
gpg --show-keys --with-fingerprint "$xxc_setup_dir/keyring.gpg"
# Compare the fingerprint through another trusted channel before installing.
sudo install -d -m 0755 /usr/share/keyrings
sudo install -m 0644 "$xxc_setup_dir/keyring.gpg" /usr/share/keyrings/thugsred-archive-keyring.gpg
curl -fsSLo "$xxc_setup_dir/thugsred.sources" '{}/repo/thugsred.sources'
sudo install -m 0644 "$xxc_setup_dir/thugsred.sources" /etc/apt/sources.list.d/thugsred.sources
sudo apt update
)"#,
        s.config.server.external_url,
        s.config.signing.public_keyring_name,
        s.config.server.external_url
    )
}
async fn help(App(s): App<State>) -> Response {
    let mut p = Page::new(
        &s,
        "configure your client.",
        "Use a repository-specific keyring and a modern Deb822 source definition.",
    );
    p.command = setup(&s);
    p.details = vec![
        ("Deb822 / thugsred.sources".into(), sources(&s)),
        ("legacy / thugsred.list".into(), legacy(&s)),
    ];
    p.note="Run these commands on the APT client. Debian, Ubuntu, Kali and Parrot use APT, but each package's distribution and dependency compatibility must be checked separately. Never use apt-key. Do not install both source definitions.".into();
    p.response()
}
async fn about(App(s): App<State>) -> Response {
    let mut p = Page::new(
        &s,
        "infrastructure with attitude.",
        "XXC-APTD is a Debian APT repository daemon, management platform and package browser built as a THUGS(red) project by Kawaiipantsu.",
    );
    p.prompt = "xxc-aptd --about".into();
    p.details=vec![("purpose".into(),"The next generation of the infrastructure behind apt.thugs.red: standards-compliant publishing, package staging, OpenPGP signatures and atomic repository generations.".into()),("architecture".into(),"Plain HTTP on local interfaces. Unix administration. Standard Debian filesystem locations. TLS at the reverse proxy.".into()),("development status".into(),"0.1.0 vertical slice. Production acceptance and authenticated web administration remain under development.".into())];
    p.note="Developed by Kawaiipantsu. A THUGS(red) project.\nroot@apt:~$ serve packages, not bullshit.".into();
    p.response()
}
async fn release(App(s): App<State>) -> Response {
    release_page(s).await
}
async fn release_suite(App(s): App<State>, Path(suite): Path<String>) -> Response {
    if suite != s.config.repository.suite {
        return error(&s, StatusCode::NOT_FOUND, "suite not found");
    }
    release_page(s).await
}
async fn release_page(s: State) -> Response {
    let c = s.config.clone();
    let result = blocking(move || repository::current(&c)).await;
    let mut p = Page::new(
        &s,
        &s.config.repository.suite,
        "Signed archive publication.",
    );
    match result {
        Ok(Some(m)) => {
            p.details = vec![
                ("generation".into(), m.id),
                ("published".into(), m.created),
                ("fingerprint".into(), m.fingerprint),
                ("packages".into(), m.packages.len().to_string()),
            ];
            p.response()
        }
        Ok(None) => {
            p.note = "No generation has been published.".into();
            p.response()
        }
        Err(_) => error(
            &s,
            StatusCode::SERVICE_UNAVAILABLE,
            "repository unavailable",
        ),
    }
}
async fn repository_root(App(s): App<State>) -> Response {
    directory(s, String::new()).await
}
async fn directory(s: State, relative: String) -> Response {
    let c = s.config.clone();
    let rel = relative.clone();
    let result = blocking(move || {
        let path = repository::resolve(&c, &rel)?;
        let mut entries = Vec::new();
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || (rel.is_empty() && name != "pool" && name != "dists") {
                continue;
            }
            let child = format!(
                "{}{name}",
                if rel.is_empty() {
                    String::new()
                } else {
                    format!("{}/", rel.trim_end_matches('/'))
                }
            );
            let Ok(safe) = repository::resolve(&c, &child) else {
                continue;
            };
            let info = std::fs::metadata(safe)?;
            let is_dir = info.is_dir();
            let encoded = child
                .split('/')
                .map(|x| utf8_percent_encode(x, NON_ALPHANUMERIC).to_string())
                .collect::<Vec<_>>()
                .join("/");
            entries.push(Entry {
                name: format!("{name}{}", if is_dir { "/" } else { "" }),
                url: format!("/repo/{encoded}{}", if is_dir { "/" } else { "" }),
                kind: if is_dir {
                    "directory"
                } else if name.ends_with(".deb") {
                    "Debian package"
                } else {
                    "archive metadata"
                }
                .into(),
                size: if is_dir {
                    "—".into()
                } else {
                    format!("{} B", info.len())
                },
                action: if is_dir { "open →" } else { "download ↓" }.into(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    })
    .await;
    match result {
        Ok(entries) => {
            let mut p = Page::new(
                &s,
                "repository files.",
                "Browse the archive. File links return the original repository bytes.",
            );
            p.browser = true;
            p.entries = entries;
            p.directory = relative.clone();
            p.parent = if relative.is_empty() {
                "/".into()
            } else {
                let r = relative.trim_end_matches('/');
                format!("/repo/{}/", r.rsplit_once('/').map(|x| x.0).unwrap_or(""))
            };
            p.response()
        }
        Err(_) => error(&s, StatusCode::NOT_FOUND, "directory not found"),
    }
}
async fn repository_file(
    App(s): App<State>,
    OriginalUri(uri): OriginalUri,
    request: Request<Body>,
) -> Response {
    let encoded = uri.path().strip_prefix("/repo/").unwrap_or("");
    let relative = match percent_encoding::percent_decode_str(encoded).decode_utf8() {
        Ok(r) => r.into_owned(),
        Err(_) => return error(&s, StatusCode::BAD_REQUEST, "invalid path"),
    };
    if relative == "thugsred.sources" || relative == "thugsred.list" {
        let content = if relative.ends_with(".sources") {
            sources(&s)
        } else {
            legacy(&s)
        };
        return (
            [
                ("content-type", "text/plain; charset=utf-8"),
                ("cache-control", "no-cache"),
            ],
            content,
        )
            .into_response();
    }
    let key = if relative == s.config.signing.public_ascii_name
        || relative == "thugsred.gpg.key"
        || relative == "thugsred-archive-keyring.asc"
    {
        Some("public.asc")
    } else if relative == s.config.signing.public_keyring_name
        || relative == "thugsred-archive-keyring.gpg"
    {
        Some("public.gpg")
    } else {
        None
    };
    let c = s.config.clone();
    let rel = relative.clone();
    let path = blocking(move || {
        if let Some(key) = key {
            let m = repository::current(&c)?.ok_or_else(|| anyhow::anyhow!("No publication"))?;
            Ok(c.paths.repository.join(".generations").join(m.id).join(key))
        } else {
            repository::resolve(&c, &rel)
        }
    })
    .await;
    let path = match path {
        Ok(p) => p,
        Err(_) => return error(&s, StatusCode::NOT_FOUND, "path not found"),
    };
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return error(&s, StatusCode::NOT_FOUND, "path not found"),
    };
    if metadata.is_dir() {
        return directory(s, relative.trim_end_matches('/').into()).await;
    }
    let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
    let stamp = modified
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let etag = format!(
        "W/\"{:x}-{:x}-{:x}\"",
        metadata.len(),
        stamp.as_secs(),
        stamp.subsec_nanos()
    );
    let immutable = relative.starts_with("pool/") || relative.contains("/by-hash/");
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    if request
        .headers()
        .get("if-none-match")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',').any(|x| {
                x.trim().trim_start_matches("W/") == etag.trim_start_matches("W/")
                    || x.trim() == "*"
            })
        })
    {
        return (
            StatusCode::NOT_MODIFIED,
            [("etag", etag.as_str()), ("cache-control", cache)],
        )
            .into_response();
    }
    let mut request = request;
    // If-None-Match takes precedence over If-Modified-Since even on a miss.
    if request.headers().contains_key("if-none-match") {
        request.headers_mut().remove("if-modified-since");
    }
    let response = tower_http::services::ServeFile::new(&path)
        .oneshot(request)
        .await;
    let mut response = match response {
        Ok(r) => r.map(Body::new),
        Err(_) => return error(&s, StatusCode::INTERNAL_SERVER_ERROR, "file unavailable"),
    };
    let mime = if relative.ends_with(".deb") {
        "application/vnd.debian.binary-package"
    } else if relative.ends_with(".gz") {
        "application/gzip"
    } else if relative.ends_with(".xz") {
        "application/x-xz"
    } else if relative.ends_with(".gpg") {
        "application/octet-stream"
    } else {
        "text/plain; charset=utf-8"
    };
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static(mime));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static(cache));
    if let Ok(v) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert("etag", v);
    }
    response
}
async fn sitemap(App(s): App<State>) -> Response {
    let origin = &s.config.server.external_url;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">",
    );
    for path in ["/", "/packages", "/releases", "/help", "/about", "/repo/"] {
        xml.push_str(&format!("<url><loc>{origin}{path}</loc></url>"));
    }
    xml.push_str("</urlset>");
    ([("content-type", "application/xml; charset=utf-8")], xml).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn public_private_separation_and_headers() {
        let root = tempfile::tempdir().unwrap();
        let config = xxc_aptd_core::config::Config::initialize(
            &root.path().join("config"),
            Some(root.path()),
        )
        .unwrap();
        let db = xxc_aptd_core::db::Database::open(&config).unwrap();
        let app = router(State::new(config, db).unwrap());
        for path in [
            "/api/v1/status",
            "/admin",
            "/metrics",
            "/readyz",
            "/repo/../keys",
            "/repo/%2e%2e/keys",
            "/repo/%252e%252e/keys",
            "/repo/pool%5c..%5ckeys",
            "/repo/.generations/test",
        ] {
            let r = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("host", "localhost")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(!r.status().is_success(), "{path}");
        }
        let r = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers().contains_key("content-security-policy"));
    }
}

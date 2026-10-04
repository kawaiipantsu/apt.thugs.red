//! Bounded, asynchronous observation of public response bodies.
use crate::State;
use axum::{
    body::{Body, Bytes, HttpBody},
    extract::{ConnectInfo, State as App},
    http::{Method, Request},
    middleware::Next,
    response::Response,
};
use http_body::{Frame, SizeHint};
use std::{
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};
use tokio::sync::{mpsc, oneshot};
use xxc_aptd_core::{
    analytics::{ClientHasher, Event, today},
    config::Config,
    db::Database,
};

enum Message {
    Event(Event),
    Stop(oneshot::Sender<()>),
}
struct Inner {
    sender: Option<mpsc::Sender<Message>>,
    hasher: Option<ClientHasher>,
    dropped: Arc<AtomicU64>,
}
#[derive(Clone)]
pub struct Collector(Arc<Inner>);
impl Collector {
    pub fn start(config: &Config, db: Database) -> anyhow::Result<Self> {
        db.analytics_prune(today(), config.analytics.retention_days)?;
        let dropped = Arc::new(AtomicU64::new(0));
        if !config.analytics.enabled {
            return Ok(Self(Arc::new(Inner {
                sender: None,
                hasher: None,
                dropped,
            })));
        }
        let hasher = db.analytics_hasher()?;
        let (tx, mut rx) = mpsc::channel(4096);
        let lost = dropped.clone();
        let retention = config.analytics.retention_days;
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut batch = Vec::with_capacity(256);
            let mut pruned = today();
            loop {
                let mut stop = None;
                let mut closed = false;
                tokio::select! {
                    message=rx.recv()=>match message {
                        Some(Message::Event(event))=>{batch.push(event);if batch.len()<256 {continue;}}
                        Some(Message::Stop(done))=>stop=Some(done),
                        None=>closed=true,
                    },
                    _=timer.tick()=>{},
                }
                let now = today();
                if !batch.is_empty() || now != pruned {
                    let items = std::mem::take(&mut batch);
                    let count = items.len() as u64;
                    let db = db.clone();
                    let prune = now != pruned;
                    let result = tokio::task::spawn_blocking(move || {
                        db.analytics_record(&items)?;
                        if prune {
                            db.analytics_prune(now, retention)?;
                        }
                        Ok::<_, anyhow::Error>(())
                    })
                    .await;
                    if !matches!(result, Ok(Ok(()))) {
                        lost.fetch_add(count, Ordering::Relaxed);
                        tracing::warn!(
                            "Analytics batch could not be stored; HTTP serving continues"
                        );
                    } else {
                        pruned = now;
                    }
                }
                if let Some(done) = stop {
                    let _ = done.send(());
                    break;
                }
                if closed {
                    break;
                }
            }
        });
        Ok(Self(Arc::new(Inner {
            sender: Some(tx),
            hasher: Some(hasher),
            dropped,
        })))
    }
    pub fn dropped(&self) -> u64 {
        self.0.dropped.load(Ordering::Relaxed)
    }
    pub async fn shutdown(&self) {
        if let Some(sender) = &self.0.sender {
            let (tx, rx) = oneshot::channel();
            if sender.send(Message::Stop(tx)).await.is_ok() {
                let _ = rx.await;
            }
        }
    }
    fn record(&self, event: Event) {
        if let Some(sender) = &self.0.sender
            && sender.try_send(Message::Event(event)).is_err()
        {
            self.0.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Walk the chain from the immediate peer towards the first untrusted hop.
/// An untrusted peer or malformed/oversized chain cannot supply an identity.
fn client_address(
    peer: IpAddr,
    headers: &axum::http::HeaderMap,
    trusted: &[ipnet::IpNet],
) -> IpAddr {
    let peer = peer.to_canonical();
    let allowed = |ip: IpAddr| trusted.iter().any(|net| net.contains(&ip));
    if !allowed(peer) || headers.get_all("x-forwarded-for").iter().count() != 1 {
        return peer;
    }
    let Some(raw) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 1024)
    else {
        return peer;
    };
    let entries = raw
        .split(',')
        .map(|s| s.trim().parse::<IpAddr>().map(|ip| ip.to_canonical()))
        .collect::<Result<Vec<_>, _>>();
    let Ok(entries) = entries else {
        return peer;
    };
    if entries.is_empty() || entries.len() > 16 {
        return peer;
    }
    let mut current = peer;
    for entry in entries.iter().rev() {
        if !allowed(current) {
            break;
        }
        current = *entry;
    }
    current
}

pub async fn observe(App(s): App<State>, request: Request<Body>, next: Next) -> Response {
    let path = request.uri().path();
    if !s.config.analytics.enabled
        || request.method() != Method::GET
        || matches!(path, "/healthz" | "/robots.txt" | "/sitemap.xml")
        || path.starts_with("/admin")
        || path.starts_with("/api/")
    {
        return next.run(request).await;
    }
    let path = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .ok()
        .filter(|s| s.len() <= 2048)
        .map(|s| s.into_owned());
    let client = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .and_then(|peer| {
            s.analytics.0.hasher.as_ref().map(|hasher| {
                hasher.hash(client_address(
                    peer.0.ip(),
                    request.headers(),
                    &s.config.server.trusted_proxies,
                ))
            })
        });
    let response = next.run(request).await;
    let path = path.unwrap_or_default();
    let kind = if path.starts_with("/repo/pool/") && path.ends_with(".deb") {
        "packages"
    } else if path.starts_with("/repo/") {
        "repository"
    } else if path.starts_with("/static/") || path == "/favicon.svg" {
        "assets"
    } else {
        "pages"
    };
    let status = response.status().as_u16();
    let is_file = kind != "pages"
        && matches!(status, 200 | 206 | 304)
        && !response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|s| s.starts_with("text/html"));
    let package = if is_file && kind == "packages" {
        path.rsplit('/')
            .next()
            .and_then(|name| name.split('_').next())
            .unwrap_or("")
            .to_owned()
    } else {
        String::new()
    };
    let event = Event {
        day: today(),
        kind,
        client,
        asset: is_file.then_some(path),
        package,
        status,
        bytes: 0,
        completed: false,
    };
    let (parts, body) = response.into_parts();
    let expected = parts
        .headers
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .or(body.size_hint().exact());
    let mut observed = ObservedBody {
        expected,
        inner: body,
        event: Some(event),
        collector: s.analytics,
    };
    if observed.inner.is_end_stream() {
        observed.finish(true);
    }
    Response::from_parts(parts, Body::new(observed))
}
struct ObservedBody {
    expected: Option<u64>,
    inner: Body,
    event: Option<Event>,
    collector: Collector,
}
impl ObservedBody {
    fn finish(&mut self, completed: bool) {
        if let Some(mut event) = self.event.take() {
            event.completed = completed;
            self.collector.record(event);
        }
    }
}
impl HttpBody for ObservedBody {
    type Data = Bytes;
    type Error = axum::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(cx);
        match &result {
            Poll::Ready(Some(Ok(frame))) => {
                if let (Some(event), Some(data)) = (&mut self.event, frame.data_ref()) {
                    event.bytes = event.bytes.saturating_add(data.len() as u64);
                }
                // Hyper may stop polling at Content-Length without requesting EOF.
                if self.inner.is_end_stream()
                    || self
                        .event
                        .as_ref()
                        .is_some_and(|e| self.expected == Some(e.bytes))
                {
                    self.finish(true);
                }
            }
            Poll::Ready(Some(Err(_))) => self.finish(false),
            Poll::Ready(None) => self.finish(true),
            Poll::Pending => {}
        }
        result
    }
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
impl Drop for ObservedBody {
    fn drop(&mut self) {
        self.finish(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bodies_record_exact_length_completion_and_interrupted_streams() {
        use futures_util::{StreamExt, future::poll_fn, stream};
        for complete in [false, true] {
            let (sender, mut receiver) = mpsc::channel(4);
            let collector = Collector(Arc::new(Inner {
                sender: Some(sender),
                hasher: None,
                dropped: Arc::new(AtomicU64::new(0)),
            }));
            let data = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"chunk"))])
                .chain(stream::pending());
            let mut body = ObservedBody {
                inner: Body::from_stream(data),
                expected: Some(if complete { 5 } else { 100 }),
                collector,
                event: Some(Event {
                    day: today(),
                    kind: "packages",
                    client: None,
                    asset: None,
                    package: "fixture".into(),
                    status: 200,
                    bytes: 0,
                    completed: false,
                }),
            };
            assert!(
                poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                    .await
                    .is_some()
            );
            drop(body);
            let Message::Event(event) = receiver.recv().await.unwrap() else {
                panic!("expected observation")
            };
            assert_eq!(event.completed, complete);
            assert_eq!(event.bytes, 5);
            assert!(receiver.try_recv().is_err());
        }
    }
    #[test]
    fn forwarded_clients_require_explicit_trusted_peers_and_valid_chains() {
        let peer: IpAddr = "192.0.2.1".parse().unwrap();
        let trusted = vec!["192.0.2.1/32".parse().unwrap()];
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            "203.0.113.7, 198.51.100.8".parse().unwrap(),
        );
        assert_eq!(client_address(peer, &h, &[]), peer);
        assert_eq!(
            client_address(peer, &h, &trusted),
            "198.51.100.8".parse::<IpAddr>().unwrap()
        );
        h.insert("x-forwarded-for", "garbage, 198.51.100.8".parse().unwrap());
        assert_eq!(client_address(peer, &h, &trusted), peer);
        h.insert("x-forwarded-for", "203.0.113.7".parse().unwrap());
        h.append("x-forwarded-for", "198.51.100.8".parse().unwrap());
        assert_eq!(client_address(peer, &h, &trusted), peer);
    }
}

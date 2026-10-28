//! Bounded HTTP enrichment of observed endpoints.
//!
//! A task follows the scheduler's snapshots and probes candidate endpoints (see
//! `stackscan_core::probe`: the user's own processes that run in a project) with one `GET /`
//! each, at most [`MAX_CONCURRENT`] at a time and at most once per [`PROBE_INTERVAL`] per process
//! identity and port. The user may also ask for one probe of an endpoint that is not probed
//! automatically ([`Probes::request`], "try HTTP"). Results land in [`Probes`], which the scanner
//! thread reads when it assembles the next snapshot: probes never delay a scan, and every view
//! still reads one revisioned snapshot.
//!
//! One probe:
//! 1. Connect to the loopback address only (never a resolved name, never off-host).
//! 2. Wait briefly: a server that speaks first (SSH, SMTP, MySQL, banners) is not HTTP, and gets
//!    no request.
//! 3. Plain HTTP/1.1 `GET /`, `Host: localhost:<port>`, no credentials or cookies. The head must
//!    arrive within [`HEAD_TIMEOUT`]; at most [`BODY_LIMIT`] of the body is read, within
//!    [`BODY_TIMEOUT`]. Redirects are recorded (redacted), never followed.
//! 4. If the answer looked like TLS (a TLS record, a silent close, or a 400 that asks for HTTPS),
//!    retry over TLS 1.2/1.3 (rustls) verified for `localhost` against the roots the user's
//!    keychains trust (locally trusted development CAs included). Verification runs in process
//!    with no network access (no revocation or intermediate fetching). A failed verification is a
//!    visible result, never a reason to skip verification.

use std::{
    collections::{HashMap, HashSet},
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant, SystemTime},
};

use http_body_util::{BodyExt, Empty};
use hyper::{
    Request, StatusCode,
    body::Bytes,
    header::{ACCEPT, CONNECTION, CONTENT_TYPE, HOST, LOCATION, USER_AGENT},
};
use hyper_util::rt::TokioIo;
use stackscan_core::{
    model::{HttpProbe, Observation, ProbeResult, ProcessIdentity, Snapshot, TlsFailure},
    probe, redact,
    scan::unix_millis,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    sync::{Semaphore, watch},
    task::JoinSet,
};
use tokio_rustls::{TlsConnector, rustls};

use crate::scheduler::Latest;

/// How long a result is reused before the endpoint is probed again (scans run every 3 s).
pub const PROBE_INTERVAL: Duration = Duration::from_secs(15);
const MAX_CONCURRENT: usize = 4;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// How long a server may stay silent before it is treated as one that waits for the client.
const SPEAKS_FIRST_WINDOW: Duration = Duration::from_millis(150);
const HEAD_TIMEOUT: Duration = Duration::from_secs(2);
const BODY_TIMEOUT: Duration = Duration::from_secs(1);
/// Body bytes read at most; enough to find a `<title>`.
pub const BODY_LIMIT: usize = 64 * 1024;
const MAX_TITLE_CHARS: usize = 200;
const MAX_LOCATION_CHARS: usize = 2048;
/// `&` plus the longest entity decoded (`#x10FFFF`) plus `;`.
const MAX_ENTITY_BYTES: usize = 11;
/// The certificate name checked for TLS endpoints, and the host of every emitted URL.
const LOOPBACK_NAME: &str = "localhost";

/// A process identity and one of its ports.
pub type Key = (ProcessIdentity, u16);

/// Which processes may be probed automatically.
pub enum Scope {
    /// Every candidate (production).
    All,
    /// Only these PIDs; empty probes nothing. Debug builds' `--probe-only`, so that tests only
    /// ever probe their own fixtures, never the machine's real services.
    Only(HashSet<u32>),
}

impl Scope {
    fn allows(&self, pid: u32) -> bool {
        match self {
            Self::All => true,
            Self::Only(pids) => pids.contains(&pid),
        }
    }
}

struct Entry {
    probed_at_ms: u64,
    finished: Instant,
    requested: bool,
    result: ProbeResult,
}

#[derive(Default)]
struct BookState {
    results: HashMap<Key, Entry>,
    in_flight: HashSet<Key>,
}

/// Probe results by process identity and port, and the means to probe: shared by the prober task,
/// the scanner thread and the `POST /api/probe` handler.
pub struct Probes {
    state: Mutex<BookState>,
    scope: Scope,
    limit: Semaphore,
    tls: TlsClient,
}

impl Probes {
    #[must_use]
    pub fn new(scope: Scope) -> Self {
        Self {
            state: Mutex::default(),
            scope,
            limit: Semaphore::new(MAX_CONCURRENT),
            tls: TlsClient::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BookState> {
        // Every critical section leaves the maps consistent, so a poisoned lock is still usable.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Fills in known results for this observation's endpoints (automatic ones on candidates,
    /// requested ones on endpoints that allow requests) and forgets results for endpoints it no
    /// longer contains. Called by the scanner thread before publishing.
    pub fn annotate(&self, observation: &mut Observation) {
        let mut state = self.lock();
        let mut live = HashSet::new();
        for service in &mut observation.services {
            let identity = service.process.identity;
            for endpoint in &mut service.endpoints {
                let automatic = match endpoint.http {
                    HttpProbe::Pending => true,
                    HttpProbe::Skipped { reason } if reason.allows_request() => false,
                    _ => continue,
                };
                let key = (identity, endpoint.port);
                live.insert(key);
                if let Some(entry) = state.results.get(&key)
                    && (automatic || entry.requested)
                {
                    endpoint.http = probed(entry);
                }
            }
        }
        state.results.retain(|key, _| live.contains(key));
    }

    /// Candidates in scope with no fresh result and no probe running; each is now in flight.
    fn claim_due(&self, snapshot: &Snapshot, now: Instant) -> Vec<(Key, SocketAddr)> {
        let Some(observation) = &snapshot.observation else {
            return Vec::new();
        };
        let mut state = self.lock();
        let mut due = Vec::new();
        for service in &observation.services {
            if !self.scope.allows(service.process.identity.pid) {
                continue;
            }
            for endpoint in &service.endpoints {
                let Some(target) = probe::target(endpoint) else {
                    continue;
                };
                let key = (service.process.identity, endpoint.port);
                let fresh = state
                    .results
                    .get(&key)
                    .is_some_and(|entry| now.duration_since(entry.finished) < PROBE_INTERVAL);
                if !fresh && state.in_flight.insert(key) {
                    due.push((key, target));
                }
            }
        }
        due
    }

    async fn probe(&self, key: Key, target: SocketAddr, requested: bool) -> HttpProbe {
        let Ok(_permit) = self.limit.acquire().await else {
            return HttpProbe::Pending;
        };
        let probed_at_ms = unix_millis(SystemTime::now());
        let result = probe_endpoint(target, &self.tls).await;
        let mut state = self.lock();
        state.in_flight.remove(&key);
        let entry = Entry {
            probed_at_ms,
            finished: Instant::now(),
            requested,
            result,
        };
        let published = probed(&entry);
        state.results.insert(key, entry);
        published
    }

    /// One user-requested probe ("try HTTP") of an endpoint that may receive one (see
    /// `probe::requested_target`); the result also appears in the next snapshot, marked
    /// `requested`.
    pub async fn request(&self, key: Key, target: SocketAddr) -> HttpProbe {
        self.lock().in_flight.insert(key);
        self.probe(key, target, true).await
    }
}

fn probed(entry: &Entry) -> HttpProbe {
    HttpProbe::Probed {
        probed_at_ms: entry.probed_at_ms,
        requested: entry.requested,
        result: entry.result.clone(),
    }
}

/// Probes due endpoints of every new snapshot until `stop` turns true or the scheduler ends.
pub async fn run(
    probes: Arc<Probes>,
    mut snapshots: watch::Receiver<Latest>,
    mut stop: watch::Receiver<bool>,
) {
    // Dropping the set on return aborts probes still running.
    let mut running = JoinSet::new();
    loop {
        while running.try_join_next().is_some() {}
        let latest = snapshots.borrow_and_update().clone();
        if let Some(snapshot) = latest {
            for (key, target) in probes.claim_due(&snapshot, Instant::now()) {
                let probes = Arc::clone(&probes);
                running.spawn(async move {
                    probes.probe(key, target, false).await;
                });
            }
        }
        tokio::select! {
            changed = snapshots.changed() => if changed.is_err() { return },
            _ = stop.wait_for(|stopping| *stopping) => return,
        }
    }
}

/// The URL emitted for an endpoint that answered: always `http(s)://localhost:<port>/`, so no
/// probe-derived text can become a link elsewhere.
#[must_use]
pub fn open_url(tls: bool, port: u16) -> String {
    let scheme = if tls { "https" } else { "http" };
    format!("{scheme}://{LOOPBACK_NAME}:{port}/")
}

/// Probes one endpoint as described in the module documentation.
async fn probe_endpoint(target: SocketAddr, tls: &TlsClient) -> ProbeResult {
    let plain = match attempt(target, None).await {
        Attempt::Answered(answer) if answer.asks_for_https => Some(answer.result),
        Attempt::Answered(answer) => return answer.result,
        Attempt::Done(result) => return result,
        Attempt::MaybeTls => None,
    };
    match attempt(target, Some(tls)).await {
        Attempt::Answered(answer) => answer.result,
        Attempt::Done(failed @ ProbeResult::TlsFailed { .. }) => failed,
        Attempt::Done(result) => plain.unwrap_or(result),
        Attempt::MaybeTls => plain.unwrap_or(ProbeResult::Closed),
    }
}

/// rustls with the roots the user's keychains trust, loaded on the first TLS probe (a local
/// keychain read, no network) and kept. Certificates are verified by webpki in process.
#[derive(Default)]
struct TlsClient {
    config: tokio::sync::OnceCell<Result<Arc<rustls::ClientConfig>, String>>,
}

impl TlsClient {
    async fn connector(&self) -> Result<TlsConnector, String> {
        let config = self
            .config
            .get_or_init(|| async {
                tokio::task::spawn_blocking(client_config)
                    .await
                    .unwrap_or_else(|error| Err(error.to_string()))
            })
            .await;
        config.clone().map(TlsConnector::from)
    }
}

fn client_config() -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    // Unreadable individual certificates are skipped; with no roots at all every TLS endpoint
    // simply fails verification, visibly.
    let (_added, _ignored) =
        roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| error.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// A certificate rejection, or any other handshake failure.
fn tls_failure(error: &io::Error) -> (TlsFailure, String) {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(
            certificate @ (rustls::Error::InvalidCertificate(_)
            | rustls::Error::NoCertificatesPresented),
        ) => (TlsFailure::Certificate, certificate.to_string()),
        Some(other) => (TlsFailure::Handshake, other.to_string()),
        None => (TlsFailure::Handshake, error.to_string()),
    }
}

struct Answer {
    result: ProbeResult,
    /// A plain-HTTP 400 whose body mentions HTTPS (nginx, Go and others answer an HTTP request
    /// on a TLS port this way).
    asks_for_https: bool,
}

enum Attempt {
    Answered(Answer),
    /// Plain HTTP got a TLS record or a silent close: worth a TLS attempt.
    MaybeTls,
    Done(ProbeResult),
}

async fn attempt(target: SocketAddr, tls: Option<&TlsClient>) -> Attempt {
    let stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(target)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) => return Attempt::Done(ProbeResult::Refused),
        Err(_) => return Attempt::Done(ProbeResult::TimedOut),
    };
    let received = Arc::new(FirstByte::default());
    let Some(tls) = tls else {
        let mut peek = [0; 1];
        match tokio::time::timeout(SPEAKS_FIRST_WINDOW, stream.peek(&mut peek)).await {
            Ok(Ok(0) | Err(_)) => return Attempt::Done(ProbeResult::Closed),
            Ok(Ok(_)) => return Attempt::Done(ProbeResult::NotHttp),
            Err(_) => {}
        }
        let io = Recorded::new(stream, Arc::clone(&received));
        return match exchange(io, target.port(), false).await {
            Ok(answer) => Attempt::Answered(answer),
            Err(Failure::TimedOut) => Attempt::Done(ProbeResult::TimedOut),
            Err(Failure::Protocol) => match received.get() {
                None => Attempt::MaybeTls,
                Some(byte) if is_tls_record(byte) => Attempt::MaybeTls,
                Some(_) => Attempt::Done(ProbeResult::NotHttp),
            },
        };
    };

    let connector = match tls.connector().await {
        Ok(connector) => connector,
        Err(reason) => {
            return Attempt::Done(ProbeResult::TlsFailed {
                failure: TlsFailure::Handshake,
                reason,
            });
        }
    };
    let name = match rustls::pki_types::ServerName::try_from(LOOPBACK_NAME) {
        Ok(name) => name.to_owned(),
        Err(error) => {
            return Attempt::Done(ProbeResult::TlsFailed {
                failure: TlsFailure::Handshake,
                reason: error.to_string(),
            });
        }
    };
    let io = Recorded::new(stream, Arc::clone(&received));
    let handshake = tokio::time::timeout(HEAD_TIMEOUT, connector.connect(name, io)).await;
    let stream = match handshake {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            return Attempt::Done(match received.get() {
                Some(byte) if is_tls_record(byte) => {
                    let (failure, reason) = tls_failure(&error);
                    ProbeResult::TlsFailed { failure, reason }
                }
                Some(_) => ProbeResult::NotHttp,
                None => ProbeResult::Closed,
            });
        }
        Err(_) => return Attempt::Done(ProbeResult::TimedOut),
    };
    match exchange(stream, target.port(), true).await {
        Ok(answer) => Attempt::Answered(answer),
        Err(Failure::TimedOut) => Attempt::Done(ProbeResult::TimedOut),
        Err(Failure::Protocol) => Attempt::Done(ProbeResult::NotHttp),
    }
}

/// TLS content types for handshake (22) and alert (21) records.
fn is_tls_record(first: u8) -> bool {
    matches!(first, 0x15 | 0x16)
}

enum Failure {
    TimedOut,
    /// Not an HTTP response (or the connection ended before one).
    Protocol,
}

/// Aborts the connection driver when the exchange ends, however it ends.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn exchange<S>(io: S, port: u16, tls: bool) -> Result<Answer, Failure>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let head_deadline = tokio::time::Instant::now() + HEAD_TIMEOUT;
    let handshake = hyper::client::conn::http1::handshake(TokioIo::new(io));
    let (mut sender, connection) = tokio::time::timeout_at(head_deadline, handshake)
        .await
        .map_err(|_| Failure::TimedOut)?
        .map_err(|_| Failure::Protocol)?;
    let _driver = AbortOnDrop(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let request = Request::get("/")
        .header(HOST, format!("{LOOPBACK_NAME}:{port}"))
        .header(
            USER_AGENT,
            concat!("stackscan/", env!("CARGO_PKG_VERSION"), " (probe)"),
        )
        .header(ACCEPT, "text/html,*/*;q=0.8")
        .header(CONNECTION, "close")
        .body(Empty::<Bytes>::new())
        .map_err(|_| Failure::Protocol)?;
    let response = tokio::time::timeout_at(head_deadline, sender.send_request(request))
        .await
        .map_err(|_| Failure::TimedOut)?
        .map_err(|_| Failure::Protocol)?;

    let status = response.status();
    let location = response
        .headers()
        .get(LOCATION)
        .map(|value| clean_location(&String::from_utf8_lossy(value.as_bytes())));
    let is_html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("html"));
    let (body, body_truncated) = read_body(response.into_body()).await;
    let asks_for_https = !tls
        && status == StatusCode::BAD_REQUEST
        && String::from_utf8_lossy(&body)
            .to_ascii_lowercase()
            .contains("https");
    Ok(Answer {
        result: ProbeResult::Response {
            url: open_url(tls, port),
            status: status.as_u16(),
            title: if is_html { title_of(&body) } else { None },
            location,
            tls,
            body_truncated,
        },
        asks_for_https,
    })
}

/// At most [`BODY_LIMIT`] bytes within [`BODY_TIMEOUT`]; `true` when the body was cut short.
async fn read_body(mut body: hyper::body::Incoming) -> (Vec<u8>, bool) {
    let deadline = tokio::time::Instant::now() + BODY_TIMEOUT;
    let mut collected = Vec::new();
    loop {
        match tokio::time::timeout_at(deadline, body.frame()).await {
            Err(_) | Ok(Some(Err(_))) => return (collected, true),
            Ok(None) => return (collected, false),
            Ok(Some(Ok(frame))) => {
                if let Ok(data) = frame.into_data() {
                    let room = BODY_LIMIT - collected.len();
                    collected.extend_from_slice(&data[..data.len().min(room)]);
                    if data.len() >= room {
                        // Dropping the body closes the connection: the rest is never read.
                        return (collected, true);
                    }
                }
            }
        }
    }
}

fn clean_location(raw: &str) -> String {
    let (mut text, _) = redact::text(raw);
    if let Some((cut, _)) = text.char_indices().nth(MAX_LOCATION_CHARS) {
        text.truncate(cut);
        text.push('…');
    }
    text
}

/// The first `<title>` element's text, entity-decoded and whitespace-collapsed.
fn title_of(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let lower = text.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let decoded = decode_entities(&text[start..end]);
    let collapsed: String = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = collapsed.chars().take(MAX_TITLE_CHARS).collect();
    (!title.is_empty()).then_some(title)
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // Entities are short; looking further than that would make the scan quadratic.
        let decoded = rest
            .bytes()
            .take(MAX_ENTITY_BYTES)
            .position(|byte| byte == b';')
            .and_then(|end| {
                let entity = &rest[1..end];
                let character = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    _ => entity
                        .strip_prefix("#x")
                        .or_else(|| entity.strip_prefix("#X"))
                        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                        .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                        .and_then(char::from_u32),
                }?;
                Some((character, end))
            });
        if let Some((character, end)) = decoded {
            out.push(character);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The first byte the peer sent, if any (`usize::MAX` until then).
struct FirstByte(AtomicUsize);

impl Default for FirstByte {
    fn default() -> Self {
        Self(AtomicUsize::new(usize::MAX))
    }
}

impl FirstByte {
    fn get(&self) -> Option<u8> {
        u8::try_from(self.0.load(Ordering::Relaxed)).ok()
    }
}

/// A TCP stream that notes the first byte it receives, so a failed exchange can tell TLS, other
/// protocols and silence apart.
struct Recorded {
    inner: TcpStream,
    first: Arc<FirstByte>,
}

impl Recorded {
    fn new(inner: TcpStream, first: Arc<FirstByte>) -> Self {
        Self { inner, first }
    }
}

impl AsyncRead for Recorded {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Some(&byte) = buf.filled().get(before) {
            let _ = self.first.0.compare_exchange(
                usize::MAX,
                usize::from(byte),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
        polled
    }
}

impl AsyncWrite for Recorded {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_decoded_and_collapsed() {
        assert_eq!(
            title_of(b"<html><HEAD><Title lang=en>\n  A &amp; B &#60;c&#x3E; &bogus;  </title>"),
            Some("A & B <c> &bogus;".to_owned())
        );
        assert_eq!(title_of(b"<title>   </title>"), None);
        assert_eq!(title_of(b"<p>no title</p>"), None);
        let long = format!("<title>{}</title>", "x".repeat(500));
        assert_eq!(
            title_of(long.as_bytes()).map(|t| t.chars().count()),
            Some(MAX_TITLE_CHARS)
        );
    }

    #[test]
    fn redirect_locations_are_redacted_and_bounded() {
        assert_eq!(
            clean_location("https://idp.example/cb?code=1&access_token=abc"),
            "https://idp.example/cb?code=1&access_token=[redacted]"
        );
        let long = format!("/{}", "a".repeat(5000));
        assert_eq!(
            clean_location(&long).chars().count(),
            MAX_LOCATION_CHARS + 1
        );
    }

    #[test]
    fn emitted_urls_are_loopback_http_only() {
        for (tls, port) in [(false, 5173), (true, 8443), (false, 1)] {
            let url = open_url(tls, port);
            let rest = url
                .strip_prefix("http://localhost:")
                .or_else(|| url.strip_prefix("https://localhost:"))
                .expect("loopback http(s) URL");
            assert_eq!(rest, format!("{port}/"));
        }
    }
}

//! A controlled listener process for tests: binds the requested TCP sockets, prints one
//! [`Ready`] JSON line on stdout, then holds the sockets until stdin reaches EOF (its owner closed
//! it or died) or `--max-lifetime-secs` elapses.
//!
//! `--serve` picks what every accepted connection gets: a short non-HTTP banner sent before the
//! client says anything (the default), or one HTTP behaviour (see `protocol::Serve`). `--record` appends
//! what each connection sent to `received-<port>.log` in the working directory, so tests can see
//! whether (and what) a client wrote.
//!
//! `--busy` keeps one thread spinning, a controlled CPU workload for metrics tests. Arguments after
//! `--` are ignored: they give the process a chosen command line (fake secrets, markup).
//!
//! `--listen 127.0.0.1` binds one ephemeral port; `--listen 127.0.0.1,::1` binds one port shared by
//! both addresses. Repeat `--listen` for more ports. `--port` (once per group, in order) binds
//! the groups on those ports instead. `--title` replaces the default page title (sent as given).
//!
//! When `KOSSE_FIXTURE_ARGS` holds a JSON array of these options, they are read from it and the
//! whole command line is ignored, so what others observe (`ps`, Kossé's Inspect) is only the
//! command line the driver chose, without harness flags.
//!
//! Signals: by default `SIGTERM` ends the fixture; `--ignore-term` catches and ignores it (only
//! `SIGKILL` stops it). `--supervise` runs a supervisor that listens on nothing itself: it starts a
//! child fixture with the other arguments, reports the child's sockets in its own ready line, and
//! whenever the child exits starts a new one on the same port (retrying while the port is taken)
//! until its own stdin reaches EOF, when it kills its current child through the child's handle.

use std::{
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, ExitCode, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use kosse_fixtures::protocol::{BANNER, HUGE_BODY, REDIRECT_PATH, Ready, Serve as Mode, TITLE};

const CONFIG_ENV: &str = "KOSSE_FIXTURE_ARGS";
/// Attempts to find one port free on every address of a group before giving up.
const SHARED_PORT_ATTEMPTS: usize = 32;
/// Largest request head read (and recorded) per connection.
const MAX_REQUEST: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Pause between a supervised child's exit (or failed start) and the next attempt.
const RESPAWN_DELAY: Duration = Duration::from_millis(20);

#[derive(Debug, Parser)]
#[command(name = "kosse-fixture")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent command-line switches"
)]
struct Args {
    /// Comma-separated addresses sharing one ephemeral port. Repeat for more ports.
    #[arg(long = "listen", required = true, value_parser = parse_group)]
    groups: Vec<Vec<IpAddr>>,

    /// Exit on its own after this long, even if the owner never closes stdin.
    #[arg(long, default_value_t = 120)]
    max_lifetime_secs: u64,

    /// Keep one thread busy (about one core of CPU) until exit.
    #[arg(long)]
    busy: bool,

    /// What each accepted connection gets.
    #[arg(long, value_enum, default_value_t = Mode::Banner)]
    serve: Mode,

    /// Append the bytes each connection sent to `received-<port>.log` in the working directory.
    #[arg(long)]
    record: bool,

    /// Bind the `--listen` groups on these ports (one per group, in order) instead of ephemeral
    /// ones.
    #[arg(long)]
    port: Vec<u16>,

    /// The `<title>` of HTML pages, sent as given (the default carries an entity and markup).
    #[arg(long)]
    title: Option<String>,

    /// Catch and ignore `SIGTERM`.
    #[arg(long)]
    ignore_term: bool,

    /// Supervise a child fixture with the other arguments, restarting it on the same port.
    #[arg(long)]
    supervise: bool,

    /// Ignored; sets the command line others observe.
    #[arg(last = true)]
    _extra: Vec<String>,
}

/// Options from `KOSSE_FIXTURE_ARGS` (see the module docs), else from the command line.
fn options() -> Result<Args, String> {
    match std::env::var(CONFIG_ENV) {
        Ok(json) => {
            let list: Vec<String> = serde_json::from_str(&json)
                .map_err(|error| format!("{CONFIG_ENV} is not a JSON string array: {error}"))?;
            Args::try_parse_from(std::iter::once("kosse-fixture".to_owned()).chain(list))
                .map_err(|error| error.to_string())
        }
        Err(_) => Ok(Args::parse()),
    }
}

fn parse_group(value: &str) -> Result<Vec<IpAddr>, String> {
    value
        .split(',')
        .map(|address| {
            address
                .trim()
                .parse()
                .map_err(|error| format!("{address:?}: {error}"))
        })
        .collect()
}

fn main() -> ExitCode {
    let args = match options() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("kosse-fixture: {error}");
            return ExitCode::from(2);
        }
    };
    if !args.port.is_empty() && args.port.len() != args.groups.len() {
        eprintln!("kosse-fixture: give --port once per --listen group");
        return ExitCode::from(2);
    }
    if args.supervise {
        return supervise(&args);
    }
    if args.ignore_term
        && let Err(error) = ignore_term()
    {
        eprintln!("kosse-fixture: could not ignore SIGTERM: {error}");
        return ExitCode::FAILURE;
    }
    let tls = if args.serve == Mode::Tls {
        match self_signed_config() {
            Ok(config) => Some(config),
            Err(error) => {
                eprintln!("kosse-fixture: could not set up TLS: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let mut listeners = Vec::new();
    for (index, group) in args.groups.iter().enumerate() {
        match bind_group(group, args.port.get(index).copied()) {
            Ok(bound) => listeners.extend(bound),
            Err(error) => {
                eprintln!("kosse-fixture: could not bind {group:?}: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let ready = Ready {
        pid: std::process::id(),
        listeners: listeners
            .iter()
            .filter_map(|listener| listener.local_addr().ok())
            .collect(),
    };
    if ready.listeners.len() != listeners.len() || announce(&ready).is_err() {
        eprintln!("kosse-fixture: could not report readiness");
        return ExitCode::FAILURE;
    }

    for listener in listeners {
        let server = Server {
            mode: args.serve,
            record: args.record,
            tls: tls.clone(),
            title: Arc::from(args.title.as_deref().unwrap_or(TITLE)),
        };
        thread::spawn(move || server.accept(&listener));
    }
    if args.busy {
        thread::spawn(spin);
    }
    let lifetime = Duration::from_secs(args.max_lifetime_secs);
    thread::spawn(move || {
        thread::sleep(lifetime);
        std::process::exit(0);
    });
    // EOF means the owner closed stdin or died; either way this fixture is done.
    let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
    ExitCode::SUCCESS
}

/// Binds every address of `group` on one port (`fixed`, or an ephemeral one, retrying with a new
/// port on collision).
fn bind_group(group: &[IpAddr], fixed: Option<u16>) -> io::Result<Vec<TcpListener>> {
    let Some((&first, rest)) = group.split_first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty group"));
    };
    if let Some(port) = fixed {
        return group
            .iter()
            .map(|&address| TcpListener::bind((address, port)))
            .collect();
    }
    for _ in 0..SHARED_PORT_ATTEMPTS {
        let leader = TcpListener::bind((first, 0))?;
        let port = leader.local_addr()?.port();
        let mut bound = vec![leader];
        let mut collided = false;
        for &address in rest {
            match TcpListener::bind((address, port)) {
                Ok(listener) => bound.push(listener),
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                    collided = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if !collided {
            return Ok(bound);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "no port was free on every address",
    ))
}

/// Installs a `SIGTERM` handler that does nothing (tokio's, so no unsafe code here), before the
/// ready line is printed.
fn ignore_term() -> io::Result<()> {
    let (installed, result) = mpsc::channel();
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = installed.send(Err(error));
                return;
            }
        };
        runtime.block_on(async move {
            use tokio::signal::unix::{SignalKind, signal};
            match signal(SignalKind::terminate()) {
                Ok(mut terms) => {
                    let _ = installed.send(Ok(()));
                    while terms.recv().await.is_some() {}
                }
                Err(error) => {
                    let _ = installed.send(Err(error));
                }
            }
        });
    });
    result
        .recv()
        .unwrap_or_else(|_| Err(io::Error::other("signal thread ended")))
}

/// See the module docs. The supervisor never signals anything but its own unreaped child.
fn supervise(args: &Args) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            eprintln!("kosse-fixture: {error}");
            return ExitCode::FAILURE;
        }
    };
    let stopping = Arc::new(AtomicBool::new(false));
    {
        let stopping = Arc::clone(&stopping);
        thread::spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            stopping.store(true, Ordering::SeqCst);
        });
    }
    let deadline = Instant::now() + Duration::from_secs(args.max_lifetime_secs);
    let done = || stopping.load(Ordering::SeqCst) || Instant::now() >= deadline;
    let mut port = args.port.first().copied();
    let mut announced = false;
    while !done() {
        let Ok(mut child) = Command::new(&exe)
            .args(child_args(args, port))
            .env_remove(CONFIG_ENV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return ExitCode::FAILURE;
        };
        // `None`: it could not bind (the port is still taken) or start; try again shortly.
        let Some(ready) = child_ready(&mut child) else {
            let _ = child.wait();
            thread::sleep(RESPAWN_DELAY);
            continue;
        };
        port = ready.listeners.first().map(std::net::SocketAddr::port);
        if !announced {
            let own = Ready {
                pid: std::process::id(),
                listeners: ready.listeners,
            };
            if announce(&own).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return ExitCode::FAILURE;
            }
            announced = true;
        }
        loop {
            if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                break;
            }
            if done() {
                let _ = child.kill();
                let _ = child.wait();
                return ExitCode::SUCCESS;
            }
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(RESPAWN_DELAY);
    }
    ExitCode::SUCCESS
}

fn child_args(args: &Args, port: Option<u16>) -> Vec<String> {
    let mut child = Vec::new();
    for group in &args.groups {
        let joined: Vec<String> = group.iter().map(ToString::to_string).collect();
        child.push("--listen".to_owned());
        child.push(joined.join(","));
    }
    if let Some(port) = port {
        child.push("--port".to_owned());
        child.push(port.to_string());
    }
    if args.ignore_term {
        child.push("--ignore-term".to_owned());
    }
    if args.busy {
        child.push("--busy".to_owned());
    }
    if args.record {
        child.push("--record".to_owned());
    }
    if let Some(title) = &args.title {
        child.push("--title".to_owned());
        child.push(title.clone());
    }
    child.push("--serve".to_owned());
    child.push(args.serve.as_arg().to_owned());
    child.push("--max-lifetime-secs".to_owned());
    child.push(args.max_lifetime_secs.to_string());
    child
}

/// The child's ready line, or `None` if it exited (or wrote garbage) first.
fn child_ready(child: &mut Child) -> Option<Ready> {
    let stdout = child.stdout.take()?;
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

fn spin() {
    let mut counter: u64 = 0;
    loop {
        counter = std::hint::black_box(counter.wrapping_add(1));
    }
}

fn announce(ready: &Ready) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, ready)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

#[derive(Clone)]
struct Server {
    mode: Mode,
    record: bool,
    tls: Option<Arc<rustls::ServerConfig>>,
    title: Arc<str>,
}

impl Server {
    fn accept(&self, listener: &TcpListener) {
        let port = listener.local_addr().map_or(0, |addr| addr.port());
        for stream in listener.incoming().flatten() {
            let server = self.clone();
            thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let _ = server.handle(stream, port);
            });
        }
    }

    fn handle(&self, mut stream: TcpStream, port: u16) -> io::Result<()> {
        match (self.mode, &self.tls) {
            (Mode::Banner, _) => {
                stream.write_all(BANNER)?;
                if self.record {
                    // Whatever the client sends after the banner, until it closes.
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut received = Vec::new();
                    let _ = (&mut stream)
                        .take(MAX_REQUEST as u64)
                        .read_to_end(&mut received);
                    self.save(port, &received);
                }
                Ok(())
            }
            (Mode::Tls, Some(config)) => {
                let connection =
                    rustls::ServerConnection::new(Arc::clone(config)).map_err(io::Error::other)?;
                let mut tls = rustls::StreamOwned::new(connection, stream);
                let request = read_head(&mut tls);
                self.save(port, &request);
                respond(&mut tls, Mode::Title, port, &self.title)
            }
            (mode, _) => {
                let request = read_head(&mut stream);
                self.save(port, &request);
                if mode == Mode::Hang {
                    // Hold the connection without answering until the client gives up.
                    let _ = stream.set_read_timeout(None);
                    let _ = stream.read_to_end(&mut Vec::new());
                    return Ok(());
                }
                respond(&mut stream, mode, port, &self.title)
            }
        }
    }

    fn save(&self, port: u16, received: &[u8]) {
        if !self.record {
            return;
        }
        let path = PathBuf::from(format!("received-{port}.log"));
        if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = log.write_all(received);
            let _ = log.write_all(b"\n--\n");
        }
    }
}

/// Reads up to the end of the request head (or [`MAX_REQUEST`], EOF, an error or the timeout).
fn read_head(stream: &mut impl Read) -> Vec<u8> {
    let mut head = Vec::new();
    let mut buffer = [0; 1024];
    while head.len() < MAX_REQUEST && !head.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => head.extend_from_slice(&buffer[..read]),
        }
    }
    head
}

fn respond(stream: &mut impl Write, mode: Mode, port: u16, title: &str) -> io::Result<()> {
    let (status, headers, body): (&str, String, String) = match mode {
        Mode::NotFound => (
            "404 Not Found",
            "Content-Type: application/json\r\n".into(),
            r#"{"error":"not found"}"#.into(),
        ),
        Mode::Error => (
            "500 Internal Server Error",
            "Content-Type: text/plain\r\n".into(),
            "boom".into(),
        ),
        Mode::Redirect => (
            "302 Found",
            format!("Location: http://localhost:{port}{REDIRECT_PATH}\r\n"),
            String::new(),
        ),
        Mode::Huge => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {HUGE_BODY}\r\nConnection: close\r\n\r\n<title>huge</title>"
            )?;
            let chunk = [b'a'; 16 * 1024];
            let mut sent = 0;
            while sent < HUGE_BODY {
                stream.write_all(&chunk)?;
                sent += chunk.len();
            }
            return stream.flush();
        }
        Mode::Title | Mode::Tls | Mode::Banner | Mode::Hang => (
            "200 OK",
            "Content-Type: text/html; charset=utf-8\r\n".into(),
            format!(
                "<!doctype html><html><head><TITLE>\n  {title}\n</TITLE></head><body>ok</body></html>"
            ),
        ),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn self_signed_config() -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error>> {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(certified.signing_key.serialize_der().into());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(vec![certified.cert.der().clone()], key)?;
    Ok(Arc::new(config))
}

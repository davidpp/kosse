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
//! both addresses. Repeat `--listen` for more ports.

use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    thread,
    time::Duration,
};

use clap::Parser;
use stackscan_fixtures::protocol::{BANNER, HUGE_BODY, REDIRECT_PATH, Ready, Serve as Mode, TITLE};

/// Attempts to find one port free on every address of a group before giving up.
const SHARED_PORT_ATTEMPTS: usize = 32;
/// Largest request head read (and recorded) per connection.
const MAX_REQUEST: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(name = "stackscan-fixture")]
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

    /// Ignored; sets the command line others observe.
    #[arg(last = true)]
    _extra: Vec<String>,
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
    let args = Args::parse();
    let tls = if args.serve == Mode::Tls {
        match self_signed_config() {
            Ok(config) => Some(config),
            Err(error) => {
                eprintln!("stackscan-fixture: could not set up TLS: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let mut listeners = Vec::new();
    for group in &args.groups {
        match bind_group(group) {
            Ok(bound) => listeners.extend(bound),
            Err(error) => {
                eprintln!("stackscan-fixture: could not bind {group:?}: {error}");
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
        eprintln!("stackscan-fixture: could not report readiness");
        return ExitCode::FAILURE;
    }

    for listener in listeners {
        let server = Server {
            mode: args.serve,
            record: args.record,
            tls: tls.clone(),
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

/// Binds every address of `group` on one port, retrying with a new port on collision.
fn bind_group(group: &[IpAddr]) -> io::Result<Vec<TcpListener>> {
    let Some((&first, rest)) = group.split_first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty group"));
    };
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
                respond(&mut tls, Mode::Title, port)
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
                respond(&mut stream, mode, port)
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

fn respond(stream: &mut impl Write, mode: Mode, port: u16) -> io::Result<()> {
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
                "<!doctype html><html><head><TITLE>\n  {TITLE}\n</TITLE></head><body>ok</body></html>"
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

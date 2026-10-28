//! A controlled listener process for tests: binds the requested TCP sockets, prints one
//! [`Ready`] JSON line on stdout, then holds the sockets until stdin reaches EOF (its owner closed
//! it or died) or `--max-lifetime-secs` elapses. Every accepted connection gets a short non-HTTP
//! banner and is closed.
//!
//! `--listen 127.0.0.1` binds one ephemeral port; `--listen 127.0.0.1,::1` binds one port shared by
//! both addresses. Repeat `--listen` for more ports.

use std::{
    io::{self, Write},
    net::{IpAddr, TcpListener},
    process::ExitCode,
    thread,
    time::Duration,
};

use clap::Parser;
use stackscan_fixtures::protocol::Ready;

const BANNER: &[u8] = b"stackscan-fixture\n";
/// Attempts to find one port free on every address of a group before giving up.
const SHARED_PORT_ATTEMPTS: usize = 32;

#[derive(Debug, Parser)]
#[command(name = "stackscan-fixture")]
struct Args {
    /// Comma-separated addresses sharing one ephemeral port. Repeat for more ports.
    #[arg(long = "listen", required = true, value_parser = parse_group)]
    groups: Vec<Vec<IpAddr>>,

    /// Exit on its own after this long, even if the owner never closes stdin.
    #[arg(long, default_value_t = 120)]
    max_lifetime_secs: u64,
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
        thread::spawn(move || serve_banner(&listener));
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

fn announce(ready: &Ready) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, ready)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn serve_banner(listener: &TcpListener) {
    for mut stream in listener.incoming().flatten() {
        let _ = stream.write_all(BANNER);
    }
}

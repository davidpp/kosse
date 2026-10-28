//! Independent ground truth: what `lsof` reports as TCP LISTEN sockets for given PIDs.

use std::{
    collections::BTreeSet,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    process::Command,
};

const LSOF: &str = "/usr/sbin/lsof";

/// `(pid, address, port)`, with IPv4-mapped IPv6 addresses canonicalized to IPv4.
pub type ListenEntry = (u32, IpAddr, u16);

#[derive(Debug, thiserror::Error)]
pub enum LsofError {
    #[error("could not run lsof: {0}")]
    Run(#[from] io::Error),
    #[error("lsof failed ({status}): {stderr}")]
    Failed { status: String, stderr: String },
    #[error("unexpected lsof line {0:?}")]
    Parse(String),
}

/// TCP LISTEN sockets held by `pids`, restricted to those PIDs (`-a` ANDs the selectors).
pub fn tcp_listeners(pids: &[u32]) -> Result<BTreeSet<ListenEntry>, LsofError> {
    let pid_list: Vec<String> = pids.iter().map(ToString::to_string).collect();
    let output = Command::new(LSOF)
        .args([
            "-nP",
            "-a",
            "-p",
            &pid_list.join(","),
            "-iTCP",
            "-sTCP:LISTEN",
            "-Fptn",
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // lsof exits 1 when nothing matched; that is an empty answer, not a failure.
    if !output.status.success() && !stdout.trim().is_empty() {
        return Err(LsofError::Failed {
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    parse(&stdout)
}

/// Parses `-F ptn` field output: `p<pid>`, then per descriptor `f<fd>`, `t<IPv4|IPv6>`, `n<name>`.
fn parse(output: &str) -> Result<BTreeSet<ListenEntry>, LsofError> {
    let mut entries = BTreeSet::new();
    let mut pid = None;
    let mut ipv6 = false;
    for line in output.lines() {
        let parse_error = || LsofError::Parse(line.to_owned());
        let (field, value) = line.split_at_checked(1).ok_or_else(parse_error)?;
        match field {
            "p" => pid = Some(value.parse::<u32>().map_err(|_| parse_error())?),
            "t" => ipv6 = value == "IPv6",
            "n" => {
                let (host, port) = value.rsplit_once(':').ok_or_else(parse_error)?;
                let address = match host.trim_start_matches('[').trim_end_matches(']') {
                    "*" if ipv6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    "*" => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    literal => literal.parse().map_err(|_| parse_error())?,
                };
                let port = port.parse().map_err(|_| parse_error())?;
                entries.insert((pid.ok_or_else(parse_error)?, address.to_canonical(), port));
            }
            _ => {}
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_ipv6_and_wildcards() {
        let output = "p10\nf3\ntIPv4\nn127.0.0.1:5000\nf4\ntIPv6\nn[::1]:5000\nf5\ntIPv6\nn*:6000\np11\nf7\ntIPv4\nn*:7000\n";
        let entries: Vec<ListenEntry> = parse(output).expect("parse").into_iter().collect();
        assert_eq!(
            entries,
            [
                (10, IpAddr::V4(Ipv4Addr::LOCALHOST), 5000),
                (10, IpAddr::V6(Ipv6Addr::UNSPECIFIED), 6000),
                (10, IpAddr::V6(Ipv6Addr::LOCALHOST), 5000),
                (11, IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7000),
            ]
        );
    }
}

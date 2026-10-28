//! Which endpoints may receive an HTTP probe, and at which loopback address. The probing itself
//! (I/O, timeouts, TLS) lives in the app; this is the policy, shared by every view.
//!
//! Only loopback is ever probed: a wildcard bind is reached through the loopback address of its
//! family, a bind to a specific interface address is skipped. Automatic probes go only to the
//! current user's processes that run in a project ([`restrict`]); well-known non-HTTP ports and
//! servers are skipped without connecting. The user may still ask for a probe of a skipped
//! endpoint ([`requested_target`]), except another user's or a non-loopback one. The prober
//! refuses to send HTTP to a server that speaks first either way.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::model::{Endpoint, HttpProbe, ProbeSkip};

/// Ports of common protocols that are not HTTP (SSH, SMTP, DNS, POP3, IMAP, databases, caches,
/// message brokers, the Erlang port mapper, Zookeeper).
const NON_HTTP_PORTS: [u16; 22] = [
    22, 25, 53, 110, 143, 465, 587, 993, 995, 1433, 1521, 2181, 3306, 4369, 5432, 5672, 6379, 9042,
    9092, 11211, 27017, 33060,
];

/// Process names (as the kernel reports them) of servers that do not speak HTTP.
const NON_HTTP_PROCESSES: [&str; 11] = [
    "postgres",
    "postmaster",
    "mysqld",
    "mariadbd",
    "redis-server",
    "valkey-server",
    "mongod",
    "memcached",
    "sshd",
    "epmd",
    "dnsmasq",
];

/// The probe state a fresh endpoint starts in, judged from its sockets alone:
/// [`HttpProbe::Skipped`] or [`HttpProbe::Pending`]. See [`restrict`] for the owner rules.
#[must_use]
pub fn initial_state(port: u16, binds: &[IpAddr], process_name: Option<&str>) -> HttpProbe {
    match skip_reason(port, binds, process_name) {
        Some(reason) => HttpProbe::Skipped { reason },
        None => HttpProbe::Pending,
    }
}

fn skip_reason(port: u16, binds: &[IpAddr], process_name: Option<&str>) -> Option<ProbeSkip> {
    if loopback_address(binds).is_none() {
        return Some(ProbeSkip::NotOnLoopback);
    }
    if NON_HTTP_PORTS.contains(&port) {
        return Some(ProbeSkip::KnownNonHttpPort);
    }
    if process_name.is_some_and(|name| NON_HTTP_PROCESSES.contains(&name)) {
        return Some(ProbeSkip::KnownNonHttpProcess);
    }
    None
}

/// Narrows a [`HttpProbe::Pending`] endpoint to the automatic-probe rule: the process belongs to
/// the user Kossé runs as, and runs in a project.
#[must_use]
pub fn restrict(state: HttpProbe, own_user: bool, attributed: bool) -> HttpProbe {
    match state {
        HttpProbe::Pending if !own_user => HttpProbe::Skipped {
            reason: ProbeSkip::OtherUser,
        },
        HttpProbe::Pending if !attributed => HttpProbe::Skipped {
            reason: ProbeSkip::NotAttributed,
        },
        other => other,
    }
}

/// A profile declares this endpoint HTTP: a non-HTTP-looking port or process name no longer
/// skips it, but the owner and loopback rules still do ([`restrict`]).
#[must_use]
pub fn declared_http(state: HttpProbe, own_user: bool) -> HttpProbe {
    match state {
        HttpProbe::Skipped {
            reason: ProbeSkip::KnownNonHttpPort | ProbeSkip::KnownNonHttpProcess,
        } => restrict(HttpProbe::Pending, own_user, true),
        other => other,
    }
}

/// Where a user-requested probe ("try HTTP") may go: any endpoint except one skipped as
/// another user's or as not on loopback.
#[must_use]
pub fn requested_target(endpoint: &Endpoint) -> Option<SocketAddr> {
    if let HttpProbe::Skipped { reason } = endpoint.http
        && !reason.allows_request()
    {
        return None;
    }
    loopback_target(endpoint)
}

/// Where to probe an endpoint automatically: not skipped, at IPv4 loopback when the endpoint is reachable
/// there, otherwise IPv6 loopback (an IPv6-only listener).
#[must_use]
pub fn target(endpoint: &Endpoint) -> Option<SocketAddr> {
    if matches!(endpoint.http, HttpProbe::Skipped { .. }) {
        return None;
    }
    loopback_target(endpoint)
}

fn loopback_target(endpoint: &Endpoint) -> Option<SocketAddr> {
    let binds: Vec<IpAddr> = endpoint
        .bind_addresses
        .iter()
        .filter_map(|address| address.parse().ok())
        .collect();
    loopback_address(&binds).map(|ip| SocketAddr::new(ip, endpoint.port))
}

fn loopback_address(binds: &[IpAddr]) -> Option<IpAddr> {
    let canonical = || binds.iter().map(IpAddr::to_canonical);
    let reaches = |want: fn(&IpAddr) -> bool| canonical().any(|ip| want(&ip));
    if reaches(|ip| *ip == IpAddr::V4(Ipv4Addr::LOCALHOST) || *ip == Ipv4Addr::UNSPECIFIED) {
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    } else if reaches(|ip| *ip == IpAddr::V6(Ipv6Addr::LOCALHOST) || *ip == Ipv6Addr::UNSPECIFIED) {
        Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Exposure, TransportProtocol};

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter()
            .map(|ip| ip.parse().expect("test address"))
            .collect()
    }

    fn endpoint(port: u16, binds: &[&str], name: Option<&str>) -> Endpoint {
        Endpoint {
            protocol: TransportProtocol::Tcp,
            port,
            bind_addresses: binds.iter().map(|&b| b.to_owned()).collect(),
            exposure: Exposure::Loopback,
            http: initial_state(port, &ips(binds), name),
        }
    }

    #[test]
    fn loopback_and_wildcard_binds_are_probed_on_loopback() {
        let at = |binds: &[&str]| target(&endpoint(5173, binds, Some("node")));
        assert_eq!(
            at(&["127.0.0.1"]),
            Some("127.0.0.1:5173".parse().expect("addr"))
        );
        assert_eq!(
            at(&["0.0.0.0"]),
            Some("127.0.0.1:5173".parse().expect("addr"))
        );
        assert_eq!(
            at(&["127.0.0.1", "::1"]),
            Some("127.0.0.1:5173".parse().expect("addr"))
        );
        assert_eq!(at(&["::1"]), Some("[::1]:5173".parse().expect("addr")));
        assert_eq!(at(&["::"]), Some("[::1]:5173".parse().expect("addr")));
    }

    #[test]
    fn interface_only_binds_are_never_probed() {
        let lan = endpoint(8080, &["192.168.1.20"], Some("node"));
        assert_eq!(
            lan.http,
            HttpProbe::Skipped {
                reason: ProbeSkip::NotOnLoopback
            }
        );
        assert_eq!(target(&lan), None);
    }

    #[test]
    fn known_non_http_ports_and_servers_are_skipped() {
        assert_eq!(
            endpoint(5432, &["127.0.0.1"], Some("node")).http,
            HttpProbe::Skipped {
                reason: ProbeSkip::KnownNonHttpPort
            }
        );
        let redis = endpoint(7000, &["127.0.0.1"], Some("redis-server"));
        assert_eq!(
            redis.http,
            HttpProbe::Skipped {
                reason: ProbeSkip::KnownNonHttpProcess
            }
        );
        assert_eq!(target(&redis), None);
        assert_eq!(endpoint(3000, &["::1"], None).http, HttpProbe::Pending);
        // Skipped as a known non-HTTP server, but the user may still try HTTP.
        assert_eq!(
            requested_target(&redis),
            Some("127.0.0.1:7000".parse().expect("addr"))
        );
    }

    #[test]
    fn only_own_attributed_processes_are_probed_automatically() {
        let skipped = |reason| HttpProbe::Skipped { reason };
        assert_eq!(restrict(HttpProbe::Pending, true, true), HttpProbe::Pending);
        assert_eq!(
            restrict(HttpProbe::Pending, false, true),
            skipped(ProbeSkip::OtherUser)
        );
        assert_eq!(
            restrict(HttpProbe::Pending, true, false),
            skipped(ProbeSkip::NotAttributed)
        );
        let port_skip = skipped(ProbeSkip::KnownNonHttpPort);
        assert_eq!(restrict(port_skip.clone(), false, false), port_skip);

        let mut other = endpoint(3000, &["127.0.0.1"], Some("node"));
        other.http = restrict(other.http, false, true);
        assert_eq!(target(&other), None);
        assert_eq!(requested_target(&other), None, "never another user's");
        let mut unattributed = endpoint(3000, &["127.0.0.1"], Some("node"));
        unattributed.http = restrict(unattributed.http, true, false);
        assert_eq!(target(&unattributed), None);
        assert!(
            requested_target(&unattributed).is_some(),
            "try HTTP allowed"
        );
        let lan = endpoint(8080, &["192.168.1.20"], Some("node"));
        assert_eq!(requested_target(&lan), None);
    }
}

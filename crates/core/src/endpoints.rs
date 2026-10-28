//! Normalizes raw sockets into per-port endpoints: one entry per port, every bind address kept.

use std::{collections::BTreeMap, net::IpAddr};

use crate::{
    model::{Endpoint, Exposure, TransportProtocol},
    probe,
    sockets::ListenSocket,
};

/// Groups one process's sockets by port. IPv4 and IPv6 sockets on the same port (a typical
/// `localhost` dual bind) become one endpoint listing both addresses. Each endpoint starts as an
/// HTTP candidate or a skipped one (see [`probe`]).
pub fn endpoints_of<'a>(sockets: impl IntoIterator<Item = &'a ListenSocket>) -> Vec<Endpoint> {
    let mut by_port: BTreeMap<u16, Vec<IpAddr>> = BTreeMap::new();
    let mut process_name = None;
    for socket in sockets {
        process_name = process_name.or(socket.process_name.as_deref());
        by_port
            .entry(socket.port)
            .or_default()
            .push(socket.address.to_canonical());
    }
    by_port
        .into_iter()
        .map(|(port, mut addresses)| {
            // IpAddr orders every IPv4 address before IPv6.
            addresses.sort_unstable();
            addresses.dedup();
            Endpoint {
                protocol: TransportProtocol::Tcp,
                port,
                exposure: exposure_of(&addresses),
                http: probe::initial_state(port, &addresses, process_name),
                bind_addresses: addresses.iter().map(ToString::to_string).collect(),
            }
        })
        .collect()
}

/// The widest reach among the bind addresses.
#[must_use]
pub fn exposure_of(addresses: &[IpAddr]) -> Exposure {
    addresses
        .iter()
        .map(|address| match address.to_canonical() {
            address if address.is_unspecified() => Exposure::AllInterfaces,
            address if address.is_loopback() => Exposure::Loopback,
            _ => Exposure::Interface,
        })
        .max()
        .unwrap_or(Exposure::Loopback)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn socket(address: &str, port: u16) -> ListenSocket {
        ListenSocket {
            pid: 42,
            address: address.parse().expect("test address"),
            port,
            process_name: Some("fixture".into()),
            executable: None,
        }
    }

    #[test]
    fn dual_bind_on_one_port_is_one_endpoint_with_both_addresses() {
        let sockets = [socket("::1", 5173), socket("127.0.0.1", 5173)];
        let endpoints = endpoints_of(&sockets);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].port, 5173);
        assert_eq!(endpoints[0].bind_addresses, ["127.0.0.1", "::1"]);
        assert_eq!(endpoints[0].exposure, Exposure::Loopback);
    }

    #[test]
    fn mapped_and_plain_ipv4_binds_collapse_to_one_address() {
        let sockets = [socket("::ffff:127.0.0.1", 80), socket("127.0.0.1", 80)];
        assert_eq!(endpoints_of(&sockets)[0].bind_addresses, ["127.0.0.1"]);
    }

    #[test]
    fn ports_stay_separate_and_ordered() {
        let sockets = [
            socket("127.0.0.1", 9000),
            socket("127.0.0.1", 3000),
            socket("127.0.0.1", 9000),
        ];
        let ports: Vec<u16> = endpoints_of(&sockets).iter().map(|e| e.port).collect();
        assert_eq!(ports, [3000, 9000]);
    }

    #[test]
    fn exposure_is_the_widest_bind() {
        let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let lan: IpAddr = "192.168.1.20".parse().expect("test address");
        let mapped_wildcard = IpAddr::V6(Ipv4Addr::UNSPECIFIED.to_ipv6_mapped());
        assert_eq!(exposure_of(&[loopback]), Exposure::Loopback);
        assert_eq!(
            exposure_of(&[loopback, IpAddr::V6(Ipv6Addr::LOCALHOST)]),
            Exposure::Loopback
        );
        assert_eq!(exposure_of(&[loopback, lan]), Exposure::Interface);
        assert_eq!(
            exposure_of(&[lan, IpAddr::V6(Ipv6Addr::UNSPECIFIED)]),
            Exposure::AllInterfaces
        );
        assert_eq!(exposure_of(&[mapped_wildcard]), Exposure::AllInterfaces);
    }
}

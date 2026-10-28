//! TCP LISTEN sockets and the PIDs that hold them. The only module that touches the `listeners`
//! crate, so its API churn stays here.
//!
//! `listeners` walks every PID's file descriptors via libproc. It also returns UDP and connected
//! TCP sockets, which are filtered out here, and silently skips processes whose descriptors this
//! user may not list (other users' processes); [`crate::scan`] reports that gap as a warning.
//!
//! The workspace patches `listeners` (vendor/listeners) so a socket is kept even when its
//! process name is unreadable or not valid UTF-8; such a name arrives empty and becomes `None`.

use std::net::IpAddr;

/// One listening socket as held by one process. A socket shared by several processes (inherited
/// across `fork`) appears once per PID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListenSocket {
    pub pid: u32,
    /// Canonical: an IPv4-mapped IPv6 bind (`::ffff:127.0.0.1`) is reported as its IPv4 address.
    pub address: IpAddr,
    pub port: u16,
    /// Process name from the same libproc pass that found the socket (lossy UTF-8; the kernel
    /// truncates it to 31 bytes). `None` when the OS withheld it.
    pub process_name: Option<String>,
    /// Executable path, when the OS disclosed it.
    pub executable: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("could not enumerate TCP listeners: {0}")]
pub struct SocketsError(String);

/// Every TCP socket in LISTEN state that this user can see, ordered and without duplicates.
pub fn tcp_listen_sockets() -> Result<Vec<ListenSocket>, SocketsError> {
    let all = listeners::get_all().map_err(|error| SocketsError(error.to_string()))?;
    let mut sockets: Vec<ListenSocket> = all
        .into_iter()
        .filter(|listener| {
            listener.protocol == listeners::Protocol::TCP
                && listener.state == listeners::SocketState::Listen
        })
        .map(|listener| ListenSocket {
            pid: listener.process.pid,
            address: listener.socket.ip().to_canonical(),
            port: listener.socket.port(),
            process_name: Some(listener.process.name).filter(|name| !name.is_empty()),
            executable: Some(listener.process.path).filter(|path| !path.is_empty()),
        })
        .collect();
    sockets.sort();
    sockets.dedup();
    Ok(sockets)
}

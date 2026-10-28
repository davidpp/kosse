//! The line a fixture process prints on stdout once every socket is bound. Any driver (Rust
//! harness, or later the browser suite) waits for it instead of sleeping.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ready {
    pub pid: u32,
    /// Every listening socket, in the order the groups were requested.
    pub listeners: Vec<SocketAddr>,
}

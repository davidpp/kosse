//! Dashboard addresses the tray and the window open. The dashboard owns what each one shows; Stop
//! opens the dashboard's own confirmation, so no native surface ever signals a process itself.

use kosse_core::{contract::WindowRoute, model::ProcessIdentity};
use serde::{Deserialize, Serialize};

/// A process with a known start time: the only identity a deep link can name, because the
/// dashboard (like every action) matches PID and start time together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pid: u32,
    started_at_secs: u64,
}

impl Target {
    pub fn of(identity: ProcessIdentity) -> Option<Self> {
        Some(Self {
            pid: identity.pid,
            started_at_secs: identity.started_at_secs?,
        })
    }

    pub fn identity(self) -> ProcessIdentity {
        ProcessIdentity {
            pid: self.pid,
            started_at_secs: Some(self.started_at_secs),
        }
    }
}

/// One line on the window's stdin: show `route` under `title` and come forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Show {
    pub route: WindowRoute,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Dashboard,
    /// `/?inspect=<pid>.<startedAtSecs>`: the dashboard with that service's details open.
    Inspect(Target),
    /// `/?stop=<pid>.<startedAtSecs>`: the dashboard's Stop confirmation for that process.
    Stop(Target),
}

impl Route {
    /// The path and query on the instance's origin.
    pub fn path(self) -> String {
        match self {
            Self::Dashboard => "/".to_owned(),
            Self::Inspect(target) => format!("/?inspect={}.{}", target.pid, target.started_at_secs),
            Self::Stop(target) => format!("/?stop={}.{}", target.pid, target.started_at_secs),
        }
    }

    /// `base` is the instance URL, `http://127.0.0.1:<port>/`.
    pub fn url(self, base: &str) -> String {
        format!("{}{}", base.trim_end_matches('/'), self.path())
    }
}

/// The wire form, for `POST /api/window`.
impl From<WindowRoute> for Route {
    fn from(route: WindowRoute) -> Self {
        match route {
            WindowRoute::Dashboard => Self::Dashboard,
            WindowRoute::Inspect {
                pid,
                started_at_secs,
            } => Self::Inspect(Target {
                pid,
                started_at_secs,
            }),
            WindowRoute::Stop {
                pid,
                started_at_secs,
            } => Self::Stop(Target {
                pid,
                started_at_secs,
            }),
        }
    }
}

impl From<Route> for WindowRoute {
    fn from(route: Route) -> Self {
        match route {
            Route::Dashboard => Self::Dashboard,
            Route::Inspect(target) => Self::Inspect {
                pid: target.pid,
                started_at_secs: target.started_at_secs,
            },
            Route::Stop(target) => Self::Stop {
                pid: target.pid,
                started_at_secs: target.started_at_secs,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_links_name_pid_and_start_time() {
        let identity = ProcessIdentity {
            pid: 4242,
            started_at_secs: Some(1_761_000_000),
        };
        let target = Target::of(identity).expect("start time known");
        let base = "http://127.0.0.1:47831/";
        assert_eq!(Route::Dashboard.url(base), "http://127.0.0.1:47831/");
        assert_eq!(
            Route::Inspect(target).url(base),
            "http://127.0.0.1:47831/?inspect=4242.1761000000"
        );
        assert_eq!(
            Route::Stop(target).url(base),
            "http://127.0.0.1:47831/?stop=4242.1761000000"
        );
    }

    #[test]
    fn a_process_without_start_time_has_no_deep_link() {
        let identity = ProcessIdentity {
            pid: 4242,
            started_at_secs: None,
        };
        assert_eq!(Target::of(identity), None);
    }
}

//! One running instance: the loopback listener, its single scanner, the prober and the HTTP
//! routes. `serve`, the menu-bar app and `kosse open` all start exactly one of these.

use std::{future::Future, net::Ipv4Addr, sync::Arc, time::Duration};

use kosse_core::{contract::InstanceMode, profiles::CentralStore};
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};

use crate::{
    AppError,
    actions::{self, Actions},
    api::{AppState, WindowSink},
    cli::ServerArgs,
    customize::Customize,
    guard::{ActionToken, Policy},
    instance::{self, Found, Instance},
    prober::{self, Probes, Scope},
    scheduler::{SCAN_INTERVAL, Scheduler, SchedulerHandle},
    server::{self, ServeError},
};

/// How long shutdown waits for a scan in progress (normally milliseconds).
const SCANNER_STOP_DEADLINE: Duration = Duration::from_secs(3);

/// What a launch that wants the port got.
pub enum Claim {
    /// A verified Kossé of this user already serves it.
    Existing(Box<Instance>),
    Started(Running),
}

/// The menu-bar app's and `open`'s start: use the verified instance on the port, or start one.
/// A bind that loses a launch race (another Kossé bound between the check and the bind) looks
/// once more and reports that instance; anything else on the port is refused.
pub fn claim(
    runtime: &tokio::runtime::Runtime,
    args: &ServerArgs,
    mode: InstanceMode,
    window: Option<WindowSink>,
) -> Result<Claim, AppError> {
    let look = || match runtime.block_on(instance::find(args.port)) {
        Found::Kosse(existing) => Ok(Some(existing)),
        Found::Refused(refused) => Err(AppError::from(refused)),
        Found::Nothing => Ok(None),
    };
    if args.port != 0
        && let Some(existing) = look()?
    {
        return Ok(Claim::Existing(existing));
    }
    match runtime.block_on(Running::start(args, mode, window)) {
        Ok(running) => Ok(Claim::Started(running)),
        Err(AppError::Serve(ServeError::PortInUse { addr })) if args.port != 0 => match look()? {
            Some(existing) => Ok(Claim::Existing(existing)),
            None => Err(ServeError::PortInUse { addr }.into()),
        },
        Err(error) => Err(error),
    }
}

pub struct Running {
    listener: TcpListener,
    app: axum::Router,
    url: String,
    scheduler: Scheduler,
    probing: JoinHandle<()>,
    stopping: watch::Sender<bool>,
}

impl Running {
    /// Binds the port (refusing one another process serves) and starts the scanner. Owners with
    /// a native window pass a `window` sink.
    pub async fn start(
        args: &ServerArgs,
        mode: InstanceMode,
        window: Option<WindowSink>,
    ) -> Result<Self, AppError> {
        let listener = server::bind(args.port).await?;
        let addr = listener.local_addr().map_err(|source| ServeError::Bind {
            addr: (Ipv4Addr::LOCALHOST, args.port).into(),
            source,
        })?;
        let token = ActionToken::generate().map_err(AppError::Token)?;
        let policy = Arc::new(Policy::new(addr.port(), token));
        // The only scanner of this instance; every route and the tray read its snapshots.
        let probes = Arc::new(Probes::new(probe_scope(args)));
        let store = CentralStore::from_env()?;
        let scheduler = Scheduler::start(SCAN_INTERVAL, Arc::clone(&probes), store.clone())
            .map_err(AppError::Scanner)?;
        let (stopping, stopping_receiver) = watch::channel(false);
        let probing = tokio::spawn(prober::run(
            Arc::clone(&probes),
            scheduler.handle().subscribe(),
            stopping_receiver.clone(),
        ));
        let actions = Arc::new(Actions::new(action_scope(args)));
        let app = server::router(
            AppState::new(
                scheduler.handle(),
                policy,
                probes,
                actions,
                stopping_receiver,
            )
            .with_mode(mode, window),
            Customize::new(scheduler.handle(), store),
        );
        Ok(Self {
            listener,
            app,
            url: format!("http://{addr}/"),
            scheduler,
            probing,
            stopping,
        })
    }

    /// `http://127.0.0.1:<port>/`.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn snapshots(&self) -> SchedulerHandle {
        self.scheduler.handle()
    }

    /// Serves until SIGINT/SIGTERM or `quit`, then stops the prober and the scanner. Observed
    /// services are never touched.
    pub async fn run(
        self,
        quit: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), AppError> {
        let served = server::run(self.listener, self.app, self.stopping, quit).await;
        self.probing.abort();
        let scheduler = self.scheduler;
        // Waiting for a scan in progress blocks, so keep it off the async workers.
        let stopped =
            tokio::task::spawn_blocking(move || scheduler.shutdown(SCANNER_STOP_DEADLINE))
                .await
                .unwrap_or(false);
        if !stopped {
            report!("kosse: the scanner is still busy; exiting without waiting for it");
        }
        served?;
        Ok(())
    }
}

/// Automatic probing covers every candidate, except in debug builds given `--probe-only`.
fn probe_scope(args: &ServerArgs) -> Scope {
    #[cfg(debug_assertions)]
    if let Some(pids) = &args.probe_only {
        return Scope::Only(pids.0.iter().copied().collect());
    }
    let _ = args;
    Scope::All
}

/// Every eligible process may be signalled, except in debug builds given `--act-only`.
fn action_scope(args: &ServerArgs) -> actions::Scope {
    #[cfg(debug_assertions)]
    if let Some(pids) = &args.act_only {
        return actions::Scope::only(&pids.0);
    }
    let _ = args;
    actions::Scope::All
}

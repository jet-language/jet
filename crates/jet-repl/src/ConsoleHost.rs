//! Host attachment for the transport-neutral project console.
//!
//! The application and development hosts own their existing HTTP mux and
//! database graph.  This module only moves those adapters into a console
//! session; it never creates a listener, client, copied database, or fallback
//! implementation.

use super::{
    ConsoleDataBackend, ConsoleError, ConsoleOptions, ConsoleOutput, ConsoleSession,
    InProcessRouter,
};

/// The two host-owned seams that a console session may use.
///
/// A host can attach only the router, only the data backend, or both.  Missing
/// seams fail closed when their corresponding console command is used.
pub struct ConsoleHostAdapters {
    router: Option<Box<dyn InProcessRouter>>,
    data_backend: Option<Box<dyn ConsoleDataBackend>>,
}

impl ConsoleHostAdapters {
    pub fn new() -> Self {
        Self {
            router: None,
            data_backend: None,
        }
    }

    pub fn with_router<R>(mut self, router: R) -> Self
    where
        R: InProcessRouter + 'static,
    {
        self.router = Some(Box::new(router));
        self
    }

    pub fn with_data_backend<B>(mut self, backend: B) -> Self
    where
        B: ConsoleDataBackend + 'static,
    {
        self.data_backend = Some(Box::new(backend));
        self
    }

    pub fn set_router(&mut self, router: Box<dyn InProcessRouter>) {
        self.router = Some(router);
    }

    pub fn set_data_backend(&mut self, backend: Box<dyn ConsoleDataBackend>) {
        self.data_backend = Some(backend);
    }

    pub fn has_router(&self) -> bool {
        self.router.is_some()
    }

    pub fn has_data_backend(&self) -> bool {
        self.data_backend.is_some()
    }

    fn attach(self, session: &mut ConsoleSession) {
        if let Some(router) = self.router {
            session.set_router(router);
        }
        if let Some(backend) = self.data_backend {
            session.set_data_backend(backend);
        }
    }
}

impl Default for ConsoleHostAdapters {
    fn default() -> Self {
        Self::new()
    }
}

/// Open one session and inject the host-owned adapters before any command can
/// run.
pub fn open_attached(
    options: ConsoleOptions,
    adapters: ConsoleHostAdapters,
) -> Result<ConsoleSession, ConsoleError> {
    let mut session = ConsoleSession::open(options)?;
    adapters.attach(&mut session);
    Ok(session)
}

/// Run a scripted session with host adapters attached, then roll back any
/// uncommitted transaction exactly as `ConsoleSession::run` does.
pub fn run_attached(
    options: ConsoleOptions,
    adapters: ConsoleHostAdapters,
) -> Result<Vec<ConsoleOutput>, ConsoleError> {
    let script = options.script.clone();
    let mut session = open_attached(options, adapters)?;
    if let Some(script) = script {
        session.run_script(&script)?;
    }
    if !session.is_closed() && !session.is_cancelled() {
        session.close()?;
    }
    Ok(session.outputs().cloned().collect())
}

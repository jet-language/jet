// Dependency-free App server-function middleware vocabulary shared by the
// compiler's App graph and every generated runtime tier.

/// One vocabulary of request/function middleware hooks. The array below is
/// the ordering law; every App action composes these hooks in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppServerMiddleware {
    Context,
    Auth,
    Csp,
    Logging,
    Observability,
}

impl AppServerMiddleware {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Auth => "auth",
            Self::Csp => "csp",
            Self::Logging => "logging",
            Self::Observability => "observability",
        }
    }
}

/// Canonical middleware order for every checked App server function.
pub const APP_SERVER_FUNCTION_MIDDLEWARE: &[AppServerMiddleware] = &[
    AppServerMiddleware::Context,
    AppServerMiddleware::Auth,
    AppServerMiddleware::Csp,
    AppServerMiddleware::Logging,
    AppServerMiddleware::Observability,
];

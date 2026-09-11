//! Build-effect view generated from the canonical Prelude effect table.
//!
//! The build projection intentionally excludes runtime-only FFI, Browser, and
//! Secret roots while preserving the public `BuildEffect` identities.

// BEGIN GENERATED BUILD EFFECT DECLARATIONS
// Source: crates/jet-codegen/src/Prelude/Effects.jet
// Source SHA-256: 6271597730bff63a19dc93b24ab2ab06bb2cb54ff5b5c80042dcc3ac3f441bd2
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildEffect {
    Net,
    FS,
    IO,
    DB,
    Time,
    Rand,
    Env,
    Exec,
    Log,
    GPU,
}

impl BuildEffect {
    pub const ALL: [Self; 10] = [
        Self::Net,
        Self::FS,
        Self::IO,
        Self::DB,
        Self::Time,
        Self::Rand,
        Self::Env,
        Self::Exec,
        Self::Log,
        Self::GPU,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Net => "Net",
            Self::FS => "FS",
            Self::IO => "IO",
            Self::DB => "DB",
            Self::Time => "Time",
            Self::Rand => "Rand",
            Self::Env => "Env",
            Self::Exec => "Exec",
            Self::Log => "Log",
            Self::GPU => "GPU",
        }
    }

    pub const fn flag(self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::FS => "fs",
            Self::IO => "io",
            Self::DB => "db",
            Self::Time => "time",
            Self::Rand => "rand",
            Self::Env => "env",
            Self::Exec => "exec",
            Self::Log => "log",
            Self::GPU => "gpu",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.contains('.') {
            return None;
        }
        let canonical = crate::Authority::parse_root(value)?;
        Self::ALL
            .into_iter()
            .find(|effect| canonical == effect.name())
    }
}
// END GENERATED BUILD EFFECT DECLARATIONS

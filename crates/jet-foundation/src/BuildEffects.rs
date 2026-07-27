//! Closed authority vocabulary shared by build sema, policy, CLI, and runtime.

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
    pub const ALL: [BuildEffect; 10] = [
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

    pub fn name(self) -> &'static str {
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

    pub fn flag(self) -> &'static str {
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
        Self::ALL.into_iter().find(|effect| {
            value.eq_ignore_ascii_case(effect.name()) || value.eq_ignore_ascii_case(effect.flag())
        })
    }
}

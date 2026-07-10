//! Closed authority vocabulary shared by build sema, policy, CLI, and runtime.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuildEffect {
    Net,
    Fs,
    Io,
    Db,
    Time,
    Rand,
    Env,
    Exec,
    Log,
    Gpu,
}

impl BuildEffect {
    pub const ALL: [BuildEffect; 10] = [
        Self::Net,
        Self::Fs,
        Self::Io,
        Self::Db,
        Self::Time,
        Self::Rand,
        Self::Env,
        Self::Exec,
        Self::Log,
        Self::Gpu,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Net => "Net",
            Self::Fs => "Fs",
            Self::Io => "Io",
            Self::Db => "Db",
            Self::Time => "Time",
            Self::Rand => "Rand",
            Self::Env => "Env",
            Self::Exec => "Exec",
            Self::Log => "Log",
            Self::Gpu => "Gpu",
        }
    }

    pub fn flag(self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::Fs => "fs",
            Self::Io => "io",
            Self::Db => "db",
            Self::Time => "time",
            Self::Rand => "rand",
            Self::Env => "env",
            Self::Exec => "exec",
            Self::Log => "log",
            Self::Gpu => "gpu",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effect| {
            value.eq_ignore_ascii_case(effect.name()) || value.eq_ignore_ascii_case(effect.flag())
        })
    }
}

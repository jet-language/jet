//! Shared terminal color policy and semantic style roles.

/// How a user asked terminal color to be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl Default for ColorChoice {
    fn default() -> Self {
        Self::Auto
    }
}

impl ColorChoice {
    pub fn parse(value: &str) -> Self {
        match value {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::Auto,
        }
    }

    /// Explicit choice > `NO_COLOR` presence > `FORCE_COLOR` presence > TTY.
    pub fn resolve(self, is_tty: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto if std::env::var_os("NO_COLOR").is_some() => false,
            Self::Auto if std::env::var_os("FORCE_COLOR").is_some() => true,
            Self::Auto => is_tty,
        }
    }
}

/// One palette vocabulary shared by every Rust terminal surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    color: bool,
}

impl Theme {
    pub const ACCENT_SGR: &'static str = "1;96";
    pub const DIM_SGR: &'static str = "2;37";
    pub const SUCCESS_SGR: &'static str = "32";
    pub const WARN_SGR: &'static str = "33";
    pub const ERROR_SGR: &'static str = "31";
    pub const INVERT_SGR: &'static str = "48;5;24;97;1";
    pub const BORDER_SGR: &'static str = "90";

    pub const fn new(color: bool) -> Self {
        Self { color }
    }

    pub fn resolve(choice: ColorChoice, is_tty: bool) -> Self {
        Self::new(choice.resolve(is_tty))
    }

    pub const fn color(self) -> bool {
        self.color
    }

    pub fn paint(self, sgr: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{sgr}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn accent(self, text: &str) -> String {
        self.paint(Self::ACCENT_SGR, text)
    }
    pub fn dim(self, text: &str) -> String {
        self.paint(Self::DIM_SGR, text)
    }
    pub fn success(self, text: &str) -> String {
        self.paint(Self::SUCCESS_SGR, text)
    }
    pub fn warn(self, text: &str) -> String {
        self.paint(Self::WARN_SGR, text)
    }
    pub fn error(self, text: &str) -> String {
        self.paint(Self::ERROR_SGR, text)
    }
    pub fn invert(self, text: &str) -> String {
        self.paint(Self::INVERT_SGR, text)
    }
    pub fn border(self, text: &str) -> String {
        self.paint(Self::BORDER_SGR, text)
    }
    pub fn bold(self, text: &str) -> String {
        self.paint("1", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_semantic_and_plain_mode_is_byte_stable() {
        let plain = Theme::new(false);
        let color = Theme::new(true);
        for role in [
            Theme::accent,
            Theme::dim,
            Theme::success,
            Theme::warn,
            Theme::error,
            Theme::invert,
            Theme::border,
        ] {
            assert_eq!(role(plain, "x"), "x");
            assert!(role(color, "x").starts_with("\x1b["));
        }
    }

    #[test]
    fn color_precedence_is_proved_in_isolated_children() {
        const CHILD: &str = "JET_THEME_POLICY_CHILD";
        if let Ok(case) = std::env::var(CHILD) {
            let (choice, tty, expected) = match case.as_str() {
                "always-over-no" => (ColorChoice::Always, false, true),
                "never-over-force" => (ColorChoice::Never, true, false),
                "empty-no-over-force" => (ColorChoice::Auto, true, false),
                "force-off-tty" => (ColorChoice::Auto, false, true),
                "auto-tty" => (ColorChoice::Auto, true, true),
                _ => panic!("unknown child case"),
            };
            assert_eq!(choice.resolve(tty), expected);
            return;
        }

        for case in [
            "always-over-no",
            "never-over-force",
            "empty-no-over-force",
            "force-off-tty",
            "auto-tty",
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "Terminal::tests::color_precedence_is_proved_in_isolated_children",
                ])
                .env(CHILD, case)
                .env_remove("NO_COLOR")
                .env_remove("FORCE_COLOR");
            if matches!(case, "always-over-no" | "empty-no-over-force") {
                child.env("NO_COLOR", "");
            }
            if matches!(
                case,
                "never-over-force" | "empty-no-over-force" | "force-off-tty"
            ) {
                child.env("FORCE_COLOR", "1");
            }
            assert!(child.status().unwrap().success(), "policy child failed: {case}");
        }
    }
}

//! Typed release-build devtools presence policy (D-DX-PROD1).
//!
//! The policy is deliberately closed: only the three ratified spellings are
//! accepted, so a release build cannot silently widen its inspection surface.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReleaseInspect {
    /// Keep the bounded local value rail and published writer, without a route.
    #[default]
    Local,
    /// Keep the local rail and compile the read-only endpoint surface.
    ReadOnly,
    /// Strip the local value rail and endpoint surface from the release.
    None,
}

impl ReleaseInspect {
    /// Canonical `package.jet` field name.
    pub const FIELD: &'static str = "inspect";

    /// Canonical Jet source spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => ".Local",
            Self::ReadOnly => ".ReadOnly",
            Self::None => ".None",
        }
    }

    /// Stable value used for the compiler's release cfg projection.
    pub const fn cfg_value(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::ReadOnly => "readonly",
            Self::None => "none",
        }
    }

    /// Whether this form carries the bounded local value rail.
    pub const fn local_rail_enabled(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Whether this form compiles the read-only endpoint surface.
    pub const fn endpoint_enabled(self) -> bool {
        matches!(self, Self::ReadOnly)
    }

    /// Parse one of the ratified dotted enum values.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value.trim() {
            ".Local" => Ok(Self::Local),
            ".ReadOnly" => Ok(Self::ReadOnly),
            ".None" => Ok(Self::None),
            _ => Err("expected .Local, .ReadOnly, or .None"),
        }
    }
}

impl std::fmt::Display for ReleaseInspect {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseInspect;

    #[test]
    fn accepts_only_ratified_values() {
        assert_eq!(ReleaseInspect::parse(".Local"), Ok(ReleaseInspect::Local));
        assert_eq!(
            ReleaseInspect::parse(".ReadOnly"),
            Ok(ReleaseInspect::ReadOnly)
        );
        assert_eq!(ReleaseInspect::parse(".None"), Ok(ReleaseInspect::None));
        assert!(ReleaseInspect::parse("ReadOnly").is_err());
    }

    #[test]
    fn policy_projections_match_ratified_forms() {
        assert!(ReleaseInspect::Local.local_rail_enabled());
        assert!(!ReleaseInspect::Local.endpoint_enabled());
        assert!(ReleaseInspect::ReadOnly.local_rail_enabled());
        assert!(ReleaseInspect::ReadOnly.endpoint_enabled());
        assert!(!ReleaseInspect::None.local_rail_enabled());
        assert!(!ReleaseInspect::None.endpoint_enabled());
    }
}

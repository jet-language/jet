//! Typed release-devtools build and deployment policy (D-DX-PROD1).
//!
//! The manifest chooses which release code may exist. Deployment variables only
//! enable an endpoint already carried by that release; they cannot add code.
//! Values are deliberately split from the policy so inspect/explain can project
//! the safe facts without ever carrying the bearer token.

use std::fmt;
use std::net::IpAddr;

use super::Blocks::ReleaseInspect;

pub const INSPECT_ENABLE_ENV: &str = "JET_INSPECT";
pub const INSPECT_TOKEN_ENV: &str = "JET_INSPECT_TOKEN";
pub const INSPECT_ALLOWLIST_ENV: &str = "JET_INSPECT_ALLOWLIST";
const MAX_ALLOWLIST_ENTRIES: usize = 64;

/// The only release endpoint form.  Development hosts use their existing
/// session-bound routes and do not use this value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseDevtoolsEndpoint {
    Absent,
    ReadOnly,
}

impl ReleaseDevtoolsEndpoint {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::ReadOnly => "read_only",
        }
    }

    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::ReadOnly)
    }
}

/// Release endpoint authentication is never optional.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseDevtoolsAuthentication {
    BearerToken,
}

impl ReleaseDevtoolsAuthentication {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BearerToken => "bearer_token",
        }
    }
}

/// Network authority for the release endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseDevtoolsAuthority {
    LoopbackOrAllowlist,
}

impl ReleaseDevtoolsAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LoopbackOrAllowlist => "loopback_or_allowlist",
        }
    }
}

/// Package sites decide which values become observable.  The endpoint never
/// promotes an event's other fields into a payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseDevtoolsPublication {
    SiteGated,
}

impl ReleaseDevtoolsPublication {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SiteGated => "site_gated",
        }
    }
}

/// One exact IP address or CIDR network accepted by the deployment allowlist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleaseDevtoolsNetwork {
    address: IpAddr,
    prefix: u8,
}

impl ReleaseDevtoolsNetwork {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("inspect allowlist entries cannot be empty".to_string());
        }
        let (address, prefix) = if let Some((address, prefix)) = value.split_once('/') {
            let address = address
                .trim()
                .parse::<IpAddr>()
                .map_err(|_| format!("invalid inspect allowlist address `{address}`"))?;
            let prefix = prefix
                .trim()
                .parse::<u8>()
                .map_err(|_| format!("invalid inspect allowlist prefix `{prefix}`"))?;
            (address, prefix)
        } else {
            let address = value
                .parse::<IpAddr>()
                .map_err(|_| format!("invalid inspect allowlist address `{value}`"))?;
            let prefix = if address.is_ipv4() { 32 } else { 128 };
            (address, prefix)
        };
        let max_prefix = if address.is_ipv4() { 32 } else { 128 };
        if prefix > max_prefix {
            return Err(format!(
                "inspect allowlist prefix `/{prefix}` exceeds `/{max_prefix}`"
            ));
        }
        Ok(Self { address, prefix })
    }

    pub const fn address(self) -> IpAddr {
        self.address
    }

    pub const fn prefix(self) -> u8 {
        self.prefix
    }

    pub fn contains(self, candidate: IpAddr) -> bool {
        match (self.address, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let network = u32::from(network);
                let candidate = u32::from(candidate);
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - self.prefix as u32)
                };
                network & mask == candidate & mask
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let network = u128::from(network);
                let candidate = u128::from(candidate);
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - self.prefix as u32)
                };
                network & mask == candidate & mask
            }
            _ => false,
        }
    }
}

impl fmt::Display for ReleaseDevtoolsNetwork {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.address, self.prefix)
    }
}

/// Bounded, typed deployment allowlist.  It contains no hostnames and never
/// treats malformed input as an empty list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReleaseDevtoolsAllowlist(Vec<ReleaseDevtoolsNetwork>);

impl ReleaseDevtoolsAllowlist {
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut entries = Vec::new();
        for item in value.split(',').map(str::trim).filter(|item| !item.is_empty()) {
            if entries.len() == MAX_ALLOWLIST_ENTRIES {
                return Err(format!(
                    "inspect allowlist cannot contain more than {MAX_ALLOWLIST_ENTRIES} entries"
                ));
            }
            entries.push(ReleaseDevtoolsNetwork::parse(item)?);
        }
        Ok(Self(entries))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, address: IpAddr) -> bool {
        self.0.iter().copied().any(|network| network.contains(address))
    }

    pub fn as_slice(&self) -> &[ReleaseDevtoolsNetwork] {
        &self.0
    }
}

/// Deployment-only activation facts.  `token` stays private so callers must
/// ask for it explicitly, while `Debug` redacts it for inspect/log surfaces.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct ReleaseDevtoolsDeployment {
    enabled: bool,
    token: Option<String>,
    pub allowlist: ReleaseDevtoolsAllowlist,
}

impl fmt::Debug for ReleaseDevtoolsDeployment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReleaseDevtoolsDeployment")
            .field("enabled", &self.enabled)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("allowlist", &self.allowlist)
            .finish()
    }
}

impl ReleaseDevtoolsDeployment {
    pub fn from_env() -> Result<Self, String> {
        let enabled = std::env::var(INSPECT_ENABLE_ENV)
            .map(|value| value.trim() == "1")
            .unwrap_or(false);
        let token = std::env::var(INSPECT_TOKEN_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let allowlist = match std::env::var(INSPECT_ALLOWLIST_ENV) {
            Ok(value) => ReleaseDevtoolsAllowlist::parse(&value)?,
            Err(std::env::VarError::NotPresent) => ReleaseDevtoolsAllowlist::default(),
            Err(error) => {
                return Err(format!(
                    "could not read {INSPECT_ALLOWLIST_ENV}: {error}"
                ))
            }
        };
        Ok(Self {
            enabled,
            token,
            allowlist,
        })
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    pub fn with_values(
        enabled: bool,
        token: Option<String>,
        allowlist: ReleaseDevtoolsAllowlist,
    ) -> Self {
        Self {
            enabled,
            token: token.filter(|value| !value.trim().is_empty()),
            allowlist,
        }
    }
}

/// One folded release policy.  `inspect == None` is the ordinary development
/// host; `Some(.Local/.ReadOnly/.None)` is a selected release profile form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseDevtoolsPolicy {
    pub inspect: Option<ReleaseInspect>,
    pub endpoint: ReleaseDevtoolsEndpoint,
    pub authentication: ReleaseDevtoolsAuthentication,
    pub authority: ReleaseDevtoolsAuthority,
    pub publication: ReleaseDevtoolsPublication,
    pub local_rail: bool,
    pub panel_code: bool,
    pub stream_code: bool,
    pub deployment: ReleaseDevtoolsDeployment,
}

impl ReleaseDevtoolsPolicy {
    /// Existing `jet dev` hosts retain their session-bound writable protocol.
    pub fn development() -> Self {
        Self {
            inspect: None,
            endpoint: ReleaseDevtoolsEndpoint::Absent,
            authentication: ReleaseDevtoolsAuthentication::BearerToken,
            authority: ReleaseDevtoolsAuthority::LoopbackOrAllowlist,
            publication: ReleaseDevtoolsPublication::SiteGated,
            local_rail: true,
            panel_code: true,
            stream_code: true,
            deployment: ReleaseDevtoolsDeployment::default(),
        }
    }

    /// Project the already parsed `build.release.inspect` fact exactly once.
    pub fn from_manifest_profile(inspect: ReleaseInspect) -> Self {
        let (endpoint, local_rail, panel_code, stream_code) = match inspect {
            ReleaseInspect::Local => (
                ReleaseDevtoolsEndpoint::Absent,
                true,
                false,
                false,
            ),
            ReleaseInspect::ReadOnly => (
                ReleaseDevtoolsEndpoint::ReadOnly,
                true,
                true,
                true,
            ),
            ReleaseInspect::None => (
                ReleaseDevtoolsEndpoint::Absent,
                false,
                false,
                false,
            ),
        };
        Self {
            inspect: Some(inspect),
            endpoint,
            authentication: ReleaseDevtoolsAuthentication::BearerToken,
            authority: ReleaseDevtoolsAuthority::LoopbackOrAllowlist,
            publication: ReleaseDevtoolsPublication::SiteGated,
            local_rail,
            panel_code,
            stream_code,
            deployment: ReleaseDevtoolsDeployment::default(),
        }
    }

    pub fn with_deployment(mut self, deployment: ReleaseDevtoolsDeployment) -> Self {
        self.deployment = deployment;
        self
    }

    pub fn from_manifest_profile_with_env(
        inspect: ReleaseInspect,
    ) -> Result<Self, String> {
        Ok(Self::from_manifest_profile(inspect).with_deployment(
            ReleaseDevtoolsDeployment::from_env()?,
        ))
    }

    pub fn is_release(&self) -> bool {
        self.inspect.is_some()
    }

    pub fn endpoint_enabled(&self) -> bool {
        self.endpoint.is_enabled()
    }

    pub fn deployment_enabled(&self) -> bool {
        self.endpoint_enabled() && self.deployment.enabled()
    }

    pub fn peer_allowed(&self, address: IpAddr) -> bool {
        address.is_loopback() || self.deployment.allowlist.contains(address)
    }

    pub fn release_cfg_value(&self) -> Option<&'static str> {
        self.inspect.map(ReleaseInspect::cfg_value)
    }

    /// Redacted facts for `jet inspect`/`jet explain`; the bearer token and
    /// activation bit never cross this projection.
    pub fn facts(&self) -> ReleaseDevtoolsPolicyFacts {
        ReleaseDevtoolsPolicyFacts {
            inspect: self.inspect,
            endpoint: self.endpoint,
            authentication: self.authentication,
            authority: self.authority,
            publication: self.publication,
            allowlist: self.deployment.allowlist.clone(),
            local_rail: self.local_rail,
            panel_code: self.panel_code,
            stream_code: self.stream_code,
        }
    }
}

/// Safe policy facts suitable for inspect/explain output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseDevtoolsPolicyFacts {
    pub inspect: Option<ReleaseInspect>,
    pub endpoint: ReleaseDevtoolsEndpoint,
    pub authentication: ReleaseDevtoolsAuthentication,
    pub authority: ReleaseDevtoolsAuthority,
    pub publication: ReleaseDevtoolsPublication,
    pub allowlist: ReleaseDevtoolsAllowlist,
    pub local_rail: bool,
    pub panel_code: bool,
    pub stream_code: bool,
}
impl Default for ReleaseDevtoolsPolicy {
    fn default() -> Self {
        Self::development()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn release_forms_project_to_closed_typed_facts() {
        let local = ReleaseDevtoolsPolicy::from_manifest_profile(ReleaseInspect::Local);
        assert!(!local.endpoint_enabled());
        assert!(local.local_rail);
        assert!(!local.panel_code);
        assert!(!local.stream_code);

        let read_only =
            ReleaseDevtoolsPolicy::from_manifest_profile(ReleaseInspect::ReadOnly);
        assert!(read_only.endpoint_enabled());
        assert!(read_only.panel_code);
        assert!(read_only.stream_code);
        assert_eq!(read_only.facts().publication, ReleaseDevtoolsPublication::SiteGated);

        let none = ReleaseDevtoolsPolicy::from_manifest_profile(ReleaseInspect::None);
        assert!(!none.local_rail);
        assert!(!none.endpoint_enabled());
    }

    #[test]
    fn allowlist_supports_exact_addresses_and_networks() {
        let allowlist = ReleaseDevtoolsAllowlist::parse("10.24.3.0/24,2001:db8::/32").unwrap();
        assert!(allowlist.contains(IpAddr::V4(Ipv4Addr::new(10, 24, 3, 8))));
        assert!(!allowlist.contains(IpAddr::V4(Ipv4Addr::new(10, 24, 4, 8))));
        assert!(allowlist.contains(IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0, 0, 0, 0, 0, 1,
        ))));
        assert!(ReleaseDevtoolsAllowlist::parse("10.0.0.1/33").is_err());
    }

    #[test]
    fn deployment_debug_redacts_token() {
        let deployment = ReleaseDevtoolsDeployment::with_values(
            true,
            Some("secret-value".to_string()),
            ReleaseDevtoolsAllowlist::default(),
        );
        let debug = format!("{deployment:?}");
        assert!(!debug.contains("secret-value"));
        assert!(debug.contains("<redacted>"));
    }
}

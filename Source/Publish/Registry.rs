// ──────────────────────────────────────────────
// Private / mirror registry configuration
// ──────────────────────────────────────────────

/// Registry endpoint configuration. Mirrors proxy the public registry;
/// private registries host organisation-internal packages.
/// D-PKGS1: no hard-coded public infrastructure — registries are configured.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Registry name (used in `deps: { pkg: { registry: "private" } }` etc.)
    pub name: String,
    /// Base URL for the git registry index.
    pub url: String,
    /// If true, this registry mirrors the public one (proxies unknown packages).
    pub mirror: bool,
    /// If true, require signed metadata from this registry.
    pub require_signed: bool,
}

impl RegistryConfig {
    /// Build the default (well-known) public registry config.
    /// In v1, the URL is advisory — the registry ops are deferred (M8 out of scope).
    pub fn public_default() -> Self {
        Self {
            name: "jet".to_string(),
            url: "https://github.com/jet-lang/registry".to_string(),
            mirror: false,
            require_signed: false,
        }
    }

    /// Build a private mirror config.
    pub fn private(name: &str, url: &str, mirror: bool) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            mirror,
            require_signed: false,
        }
    }
}

/// Parse registry configs from a simple block in the manifest extra fields.
/// Format: `registries: { name: { url: "...", mirror: true } }`
/// This is the placeholder for the future registry section in pkg.jet.
pub fn parse_registries_from_env(
    env: &std::collections::HashMap<String, String>,
) -> Vec<RegistryConfig> {
    // Look for JET_REGISTRY_<NAME>_URL env vars.
    let mut result = Vec::new();
    for (key, url) in env {
        if let Some(suffix) = key.strip_prefix("JET_REGISTRY_") {
            if let Some(name) = suffix.strip_suffix("_URL") {
                let name_lc = name.to_lowercase();
                let mirror_key = format!("JET_REGISTRY_{}_MIRROR", name);
                let mirror = env.get(&mirror_key).map(|v| v == "true").unwrap_or(false);
                result.push(RegistryConfig::private(&name_lc, url, mirror));
            }
        }
    }
    if result.is_empty() {
        result.push(RegistryConfig::public_default());
    }
    result
}

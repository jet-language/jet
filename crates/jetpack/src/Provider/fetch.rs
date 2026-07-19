//! One policy-bound, credential-clean fetch path for native registries.

use super::Ctx;
use crate::PackageManifest::PackManifest;
use std::collections::BTreeSet;
#[cfg(test)]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub(super) struct Authority {
    provider: String,
    registry: String,
    allow: BTreeSet<String>,
    deny: BTreeSet<String>,
    curl: PathBuf,
}

impl Authority {
    pub(super) fn load(
        ctx: &Ctx<'_>,
        provider: &str,
        default_registry: &str,
        default_allow: &[&str],
    ) -> Result<Self, String> {
        let configured = ctx
            .project_dir
            .and_then(PackManifest::load)
            .transpose()
            .map_err(|error| format!("could not parse policy.providers: {error:?}"))?
            .and_then(|manifest| {
                manifest
                    .provider_policy
                    .into_iter()
                    .find(|authority| authority.provider == provider)
            });
        let (registry, allow, deny): (String, BTreeSet<String>, BTreeSet<String>) = if let Some(configured) = configured {
            (
                configured.registry,
                configured.allow.into_iter().collect(),
                configured.deny.into_iter().collect(),
            )
        } else {
            (
                default_registry.to_string(),
                default_allow.iter().map(|value| value.to_string()).collect(),
                BTreeSet::new(),
            )
        };
        let registry_authority = authority_of(&registry)?;
        let mut normalized_allow = BTreeSet::new();
        normalized_allow.insert(registry_authority);
        for value in allow {
            normalized_allow.insert(normalize_authority(&value)?);
        }
        let mut normalized_deny = BTreeSet::new();
        for value in deny {
            normalized_deny.insert(normalize_authority(&value)?);
        }
        if normalized_deny.iter().any(|value| normalized_allow.contains(value)) {
            return Err(format!(
                "policy.providers.{provider} denies an explicitly allowed authority"
            ));
        }
        let curl = which("curl").ok_or_else(|| "provisioned curl was not found".to_string())?;
        let result = Self {
            provider: provider.to_string(),
            registry,
            allow: normalized_allow,
            deny: normalized_deny,
            curl,
        };
        result.check_url(result.registry())?;
        Ok(result)
    }

    pub(super) fn registry(&self) -> &str {
        &self.registry
    }

    pub(super) fn provenance(&self) -> String {
        format!(
            "provider={} registry={} allow={} deny={}",
            self.provider,
            self.registry,
            self.allow.iter().cloned().collect::<Vec<_>>().join(","),
            self.deny.iter().cloned().collect::<Vec<_>>().join(","),
        )
    }

    pub(super) fn text(&self, url: &str, scratch: &Path) -> Result<String, String> {
        let path = scratch.join(format!("fetch-{}-metadata", stable_suffix(url)));
        self.to_path(url, &path, scratch)?;
        let bytes = std::fs::read(&path).map_err(|error| format!("could not read fetch: {error}"))?;
        String::from_utf8(bytes).map_err(|_| format!("metadata from `{url}` is not UTF-8"))
    }

    pub(super) fn to_path(
        &self,
        url: &str,
        destination: &Path,
        scratch: &Path,
    ) -> Result<(), String> {
        let mut current = url.to_string();
        for redirect in 0..=5 {
            self.check_url(&current)?;
            #[cfg(test)]
            if let Some(path) = file_path(&current) {
                    let root = file_path(&self.registry)
                        .ok_or_else(|| "file fetch requires a file registry".to_string())?;
                    if !safe_file_under(&root, &path) {
                        return Err("file fetch escaped provider registry root".to_string());
                    }
                    std::fs::copy(path, destination)
                        .map_err(|error| format!("could not copy registry fixture: {error}"))?;
                    return Ok(());
            }
            let headers = scratch.join(format!("fetch-{}-{redirect}.headers", stable_suffix(url)));
            let body = scratch.join(format!("fetch-{}-{redirect}.body", stable_suffix(url)));
            let mut command = hardened_curl(&self.curl);
            let output = command
                .args(["--dump-header"])
                .arg(&headers)
                .args(["--output"])
                .arg(&body)
                .args(["--write-out", "%{http_code}"])
                .arg(&current)
                .output()
                .map_err(|error| format!("could not start provisioned curl: {error}"))?;
            if !output.status.success() {
                return Err(format!("could not fetch `{current}`"));
            }
            let status = std::str::from_utf8(&output.stdout)
                .ok()
                .and_then(|value| value.trim().parse::<u16>().ok())
                .ok_or_else(|| "curl returned no HTTP status".to_string())?;
            if (200..300).contains(&status) {
                std::fs::rename(&body, destination)
                    .or_else(|_| std::fs::copy(&body, destination).map(|_| ()))
                    .map_err(|error| format!("could not preserve fetched bytes: {error}"))?;
                return Ok(());
            }
            if !matches!(status, 301 | 302 | 303 | 307 | 308) {
                return Err(format!("fetch `{current}` returned HTTP {status}"));
            }
            let raw = std::fs::read_to_string(&headers)
                .map_err(|error| format!("could not read redirect headers: {error}"))?;
            let location = raw
                .lines()
                .rev()
                .find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("location")))
                .map(|(_, value)| value.trim())
                .ok_or_else(|| format!("HTTP {status} from `{current}` had no Location"))?;
            current = redirect_url(&current, location)?;
            self.check_url(&current)?;
        }
        Err(format!("too many redirects fetching `{url}`"))
    }

    fn check_url(&self, url: &str) -> Result<(), String> {
        let authority = authority_of(url)?;
        if self.deny.contains(&authority) || !self.allow.contains(&authority) {
            return Err(format!(
                "policy.providers.{} does not authorize `{authority}`",
                self.provider
            ));
        }
        Ok(())
    }
}

fn hardened_curl(path: &Path) -> Command {
    let mut command = Command::new(path);
    command.args([
        "--disable",
        "--silent",
        "--show-error",
        "--max-time",
        "60",
        "--max-redirs",
        "0",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--noproxy",
        "*",
    ]);
    for name in [
        "http_proxy", "https_proxy", "all_proxy", "no_proxy", "HTTP_PROXY", "HTTPS_PROXY",
        "ALL_PROXY", "NO_PROXY", "CURL_HOME", "XDG_CONFIG_HOME", "NETRC",
    ] {
        command.env_remove(name);
    }
    command
}

fn authority_of(url: &str) -> Result<String, String> {
    if url.chars().any(char::is_control) {
        return Err("provider fetch URL contains control characters".to_string());
    }
    if let Some(rest) = url.strip_prefix("https://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
            return Err(format!("provider URL has invalid or credentialed authority `{authority}`"));
        }
        let mut authority = authority.to_ascii_lowercase();
        if authority.ends_with(":443") {
            authority.truncate(authority.len() - 4);
        } else if authority.contains(':') {
            return Err("provider HTTPS URL may only use port 443".to_string());
        }
        if !valid_host(&authority) {
            return Err(format!("provider URL has invalid authority `{authority}`"));
        }
        return Ok(authority);
    }
    #[cfg(test)]
    if url.starts_with("file://") {
        return Ok("file".to_string());
    }
    Err("provider fetch URL must use HTTPS".to_string())
}

fn valid_host(authority: &str) -> bool {
    authority.len() <= 253
        && authority.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn normalize_authority(value: &str) -> Result<String, String> {
    if value.contains("://") {
        authority_of(value)
    } else if value.is_empty() || value.contains(['/', '@', '\\', '?', '#']) {
        Err(format!("invalid provider authority `{value}`"))
    } else {
        authority_of(&format!("https://{value}"))
    }
}

fn redirect_url(current: &str, location: &str) -> Result<String, String> {
    if location.starts_with("https://") {
        return Ok(location.to_string());
    }
    if location.starts_with("//") {
        return Ok(format!("https:{location}"));
    }
    let authority = authority_of(current)?;
    if location.starts_with('/') {
        return Ok(format!("https://{authority}{location}"));
    }
    let base = current.rsplit_once('/').map(|(base, _)| base).unwrap_or(current);
    Ok(format!("{base}/{location}"))
}

#[cfg(test)]
fn file_path(url: &str) -> Option<PathBuf> {
    let raw = url.strip_prefix("file://")?;
    let (path, query) = raw.split_once('?').unwrap_or((raw, ""));
    let mut path = PathBuf::from(path);
    if path.file_name().and_then(|name| name.to_str()) == Some("_search") {
        let distribution = query
            .split('&')
            .find_map(|field| field.strip_prefix("q=distribution%3A"))
            .and_then(percent_decode)?;
        if distribution.is_empty()
            || !distribution
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
        {
            return None;
        }
        path.set_file_name(format!("_search-{distribution}"));
    }
    Some(path)
}

#[cfg(test)]
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
fn safe_file_under(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
        && path
            .strip_prefix(root)
            .is_ok_and(|relative| relative.components().all(|component| matches!(component, Component::Normal(_))))
}

fn stable_suffix(value: &str) -> String {
    crate::SHA256::sha256_hex(value.as_bytes())[..16].to_string()
}

fn which(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(tool))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_rejects_denied_redirects_and_credentials() {
        assert!(authority_of("https://user@example.test/x").is_err());
        assert!(authority_of("http://example.test/x").is_err());
        assert!(authority_of("https://bad host.example/x").is_err());
        assert!(authority_of("https://example.test/x\r\nInjected: yes").is_err());
        let authority = Authority {
            provider: "php".into(),
            registry: "https://repo.example.test".into(),
            allow: BTreeSet::from(["repo.example.test".into(), "dist.example.test".into()]),
            deny: BTreeSet::from(["blocked.example.test".into()]),
            curl: PathBuf::from("curl"),
        };
        assert!(authority.check_url("https://dist.example.test/a").is_ok());
        assert!(authority.check_url("https://blocked.example.test/a").is_err());
        assert!(authority.check_url("https://other.example.test/a").is_err());
        assert_eq!(
            redirect_url("https://repo.example.test/a/index", "https://dist.example.test/pkg").unwrap(),
            "https://dist.example.test/pkg"
        );
        assert!(authority.check_url(&redirect_url("https://repo.example.test/a", "https://blocked.example.test/pkg").unwrap()).is_err());
    }

    #[test]
    fn configured_provider_authority_replaces_defaults() {
        let dir = std::env::temp_dir().join(format!("jet-provider-authority-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            crate::PackageManifest::PackManifest::path_in(&dir),
            r#"payload: { name: "p", version: "0.1.0" }
policy: {
    providers: {
        ruby: {
            registry: "https://mirror.example.test",
            allow: ["dist.example.test"],
            deny: ["blocked.example.test"],
        },
    },
}
"#,
        ).unwrap();
        let ctx = Ctx { fixtures: None, store_dir: &dir, offline: false, project_dir: Some(&dir) };
        let authority = Authority::load(&ctx, "ruby", "https://index.rubygems.org", &["index.rubygems.org"]).unwrap();
        assert_eq!(authority.registry(), "https://mirror.example.test");
        assert!(authority.check_url("https://mirror.example.test/info/a").is_ok());
        assert!(authority.check_url("https://dist.example.test/a.gem").is_ok());
        assert!(authority.check_url("https://index.rubygems.org/info/a").is_err());
        assert!(authority.check_url("https://blocked.example.test/a.gem").is_err());
        assert!(authority.provenance().contains("registry=https://mirror.example.test"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn curl_command_disables_ambient_configuration_and_proxies() {
        let command = hardened_curl(Path::new("/usr/bin/curl"));
        assert_eq!(command.get_args().next().and_then(|value| value.to_str()), Some("--disable"));
        let removed = command
            .get_envs()
            .filter_map(|(name, value)| value.is_none().then(|| name.to_string_lossy().into_owned()))
            .collect::<BTreeSet<_>>();
        for name in ["http_proxy", "HTTPS_PROXY", "CURL_HOME", "XDG_CONFIG_HOME", "NETRC"] {
            assert!(removed.contains(name), "{name} was not removed");
        }
        assert!(!command.get_args().any(|value| value == "--netrc"));
    }
}

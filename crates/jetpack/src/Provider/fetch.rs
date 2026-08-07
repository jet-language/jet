//! One policy-bound, credential-clean fetch path for native registries.

use super::Ctx;
use crate::Package::PackageFacts;
use std::collections::BTreeSet;
use std::io::{BufRead, Read};
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

pub(super) const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_ARCHIVE_ENTRIES: u64 = 4096;
pub(super) const MAX_ARCHIVE_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_ARCHIVE_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const MAX_ARCHIVE_PATH_BYTES: usize = 4096;

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
        Self::load_inner(ctx, provider, default_registry, default_allow, true)
    }

    pub(super) fn load_for_cache(
        ctx: &Ctx<'_>,
        provider: &str,
        default_registry: &str,
        default_allow: &[&str],
    ) -> Result<Self, String> {
        Self::load_inner(ctx, provider, default_registry, default_allow, false)
    }

    fn load_inner(
        ctx: &Ctx<'_>,
        provider: &str,
        default_registry: &str,
        default_allow: &[&str],
        require_curl: bool,
    ) -> Result<Self, String> {
        let configured = ctx
            .project_dir
            .and_then(PackageFacts::load)
            .transpose()
            .map_err(|error| format!("could not parse policy.providers: {error:?}"))?
            .and_then(|manifest| {
                manifest
                    .policy
                    .providers
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
        let curl = if require_curl {
            which("curl").ok_or_else(|| "provisioned curl was not found".to_string())?
        } else {
            PathBuf::new()
        };
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
                    ensure_download_budget(&path)?;
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
                ensure_download_budget(&body)?;
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
        "--max-filesize",
        "67108864",
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
        "CURL_CA_BUNDLE", "SSL_CERT_FILE", "SSL_CERT_DIR",
    ] {
        command.env_remove(name);
    }
    command
}

pub(super) fn ensure_download_budget(path: &Path) -> Result<(), String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect fetched bytes: {error}"))?
        .len();
    if size > MAX_DOWNLOAD_BYTES {
        Err(format!("fetched object is {size} bytes; limit is {MAX_DOWNLOAD_BYTES}"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_tar_archive(path: &Path, gzip: bool) -> Result<(), String> {
    ensure_download_budget(path)?;
    preflight_tar_stream(path, gzip)?;
    let list_flag = if gzip { "-tzf" } else { "-tf" };
    let verbose_flag = if gzip { "-tvzf" } else { "-tvf" };
    let mut entries = 0u64;
    stream_tar_listing(path, list_flag, |entry| {
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err("source archive has too many entries".to_string());
        }
        if entry.is_empty() || !safe_archive_path(Path::new(entry.trim_end_matches('/'))) {
            return Err(format!("source archive contains unsafe path `{entry}`"));
        }
        Ok(())
    })?;
    let mut total = 0u64;
    stream_tar_listing(path, verbose_flag, |line| {
        if !matches!(line.as_bytes().first(), Some(b'-' | b'd')) {
            return Err("source archives may contain only regular files and directories".to_string());
        }
        let size = line
            .split_whitespace()
            .nth(2)
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| "source archive size listing is malformed".to_string())?;
        if size > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(format!("source archive entry is {size} bytes; per-entry limit is {MAX_ARCHIVE_ENTRY_BYTES}"));
        }
        total = total.checked_add(size).ok_or_else(|| "source archive expanded size overflowed".to_string())?;
        if total > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(format!("source archive expands beyond {MAX_ARCHIVE_TOTAL_BYTES} bytes"));
        }
        Ok(())
    })
}

fn preflight_tar_stream(path: &Path, gzip: bool) -> Result<(), String> {
    if gzip {
        let mut child = Command::new("gzip")
            .args(["-dc"])
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not start archive preflight: {error}"))?;
        let stdout = take_child_stdout(&mut child, "source is not a readable gzip tar archive")?;
        let result = inspect_tar_stream(stdout);
        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait().map_err(|error| format!("could not wait for archive preflight: {error}"))?;
        result?;
        if !status.success() {
            return Err("source is not a readable gzip tar archive".to_string());
        }
        Ok(())
    } else {
        inspect_tar_stream(std::fs::File::open(path).map_err(|error| format!("could not open archive: {error}"))?)
    }
}

fn take_child_stdout(child: &mut Child, error: &str) -> Result<ChildStdout, String> {
    if let Some(stdout) = child.stdout.take() {
        return Ok(stdout);
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(error.to_string())
}

fn inspect_tar_stream(mut input: impl Read) -> Result<(), String> {
    let mut header = [0u8; 512];
    let mut entries = 0u64;
    let mut total = 0u64;
    let mut pax_logical_size = None;
    loop {
        input.read_exact(&mut header).map_err(|error| format!("truncated tar header: {error}"))?;
        if header.iter().all(|byte| *byte == 0) {
            return Ok(());
        }
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err("source archive has too many entries".to_string());
        }
        let stored = tar_number(&header[124..136])?;
        let kind = header[156];
        let metadata = matches!(kind, b'x' | b'g' | b'L' | b'K');
        let logical = pax_logical_size.take().unwrap_or_else(|| {
            if kind == b'S' { tar_number(&header[483..495]).unwrap_or(stored) } else { stored }
        });
        if logical > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(format!("source archive entry is {logical} bytes; per-entry limit is {MAX_ARCHIVE_ENTRY_BYTES}"));
        }
        total = total.checked_add(logical).ok_or_else(|| "source archive expanded size overflowed".to_string())?;
        if total > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(format!("source archive expands beyond {MAX_ARCHIVE_TOTAL_BYTES} bytes"));
        }
        let mut payload = if metadata { Vec::with_capacity(stored as usize) } else { Vec::new() };
        read_tar_payload(&mut input, stored, if metadata { Some(&mut payload) } else { None })?;
        if matches!(kind, b'x' | b'g') {
            let text = std::str::from_utf8(&payload).map_err(|_| "tar PAX metadata is not UTF-8".to_string())?;
            pax_logical_size = text.lines().find_map(|line| {
                let (_, field) = line.split_once(' ')?;
                let (key, value) = field.split_once('=')?;
                matches!(key, "GNU.sparse.realsize" | "GNU.sparse.size").then(|| value.parse::<u64>().ok()).flatten()
            });
            if pax_logical_size.is_some_and(|size| size > MAX_ARCHIVE_ENTRY_BYTES) {
                return Err("sparse source archive entry exceeds the per-entry limit".to_string());
            }
        }
    }
}

fn tar_number(field: &[u8]) -> Result<u64, String> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut value = (field[0] & 0x7f) as u64;
        for byte in &field[1..] {
            value = value.checked_mul(256).and_then(|value| value.checked_add(*byte as u64))
                .ok_or_else(|| "tar size overflows".to_string())?;
        }
        Ok(value)
    } else {
        let text = std::str::from_utf8(field).map_err(|_| "tar size is not ASCII".to_string())?;
        let text = text.trim_matches(['\0', ' ']);
        if text.is_empty() { Ok(0) } else { u64::from_str_radix(text, 8).map_err(|_| "tar size is malformed".to_string()) }
    }
}

fn read_tar_payload(input: &mut impl Read, size: u64, capture: Option<&mut Vec<u8>>) -> Result<(), String> {
    let padded = size.checked_add(511).ok_or_else(|| "tar size overflows".to_string())? / 512 * 512;
    let mut remaining = padded;
    let mut buffer = [0u8; 8192];
    let mut capture = capture;
    while remaining > 0 {
        let take = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..take]).map_err(|error| format!("truncated tar payload: {error}"))?;
        if let Some(output) = capture.as_deref_mut() {
            let meaningful = size.saturating_sub(output.len() as u64).min(take as u64) as usize;
            output.extend_from_slice(&buffer[..meaningful]);
        }
        remaining -= take as u64;
    }
    Ok(())
}

fn stream_tar_listing(
    path: &Path,
    flag: &str,
    mut inspect: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let mut child = Command::new("tar")
        .arg(flag)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not inspect source archive: {error}"))?;
    let stdout = take_child_stdout(&mut child, "source is not a readable tar archive")?;
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)
            .map_err(|error| format!("could not read source archive listing: {error}"))?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_ARCHIVE_PATH_BYTES + 256 {
            let _ = child.kill();
            let _ = child.wait();
            return Err("source archive listing line is too long".to_string());
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        let text = std::str::from_utf8(&line)
            .map_err(|_| "source archive listing is not UTF-8".to_string())?;
        if let Err(reason) = inspect(text) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(reason);
        }
    }
    if child.wait().map_err(|error| format!("could not wait for archive inspector: {error}"))?.success() {
        Ok(())
    } else {
        Err("source is not a readable tar archive".to_string())
    }
}

fn safe_archive_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| matches!(component, Component::Normal(_)))
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
    fn missing_archive_pipe_kills_and_reaps_child() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let error = take_child_stdout(&mut child, "missing archive pipe")
            .err()
            .expect("missing stdout must fail");
        assert_eq!(error, "missing archive pipe");
        assert!(child.try_wait().unwrap().is_some(), "child must be reaped");
    }

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
            crate::Manifest::manifest_path_in(&dir),
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
        let expected = BTreeSet::from([
            "http_proxy".to_string(), "https_proxy".to_string(), "all_proxy".to_string(),
            "no_proxy".to_string(), "HTTP_PROXY".to_string(), "HTTPS_PROXY".to_string(),
            "ALL_PROXY".to_string(), "NO_PROXY".to_string(), "CURL_HOME".to_string(),
            "XDG_CONFIG_HOME".to_string(), "NETRC".to_string(), "CURL_CA_BUNDLE".to_string(),
            "SSL_CERT_FILE".to_string(), "SSL_CERT_DIR".to_string(),
        ]);
        assert_eq!(removed, expected);
        assert!(!command.get_args().any(|value| value == "--netrc"));
    }

    #[test]
    fn file_fetch_rejects_oversized_object_before_copy() {
        let dir = std::env::temp_dir().join(format!("jet-fetch-budget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("huge");
        std::fs::File::create(&source).unwrap().set_len(MAX_DOWNLOAD_BYTES + 1).unwrap();
        let authority = Authority {
            provider: "ruby".into(),
            registry: format!("file://{}", dir.display()),
            allow: BTreeSet::from(["file".into()]),
            deny: BTreeSet::new(),
            curl: PathBuf::new(),
        };
        let destination = dir.join("copied");
        assert!(authority.to_path(&format!("file://{}", source.display()), &destination, &dir).is_err());
        assert!(!destination.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}

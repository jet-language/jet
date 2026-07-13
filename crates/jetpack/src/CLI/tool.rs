//! D-JPK-TOOLRUN1=A: `jetpack tool run|install|list|uninstall`.
//!
//! Ephemeral `tool run` realizes a ref through every built-in provider and
//! execs its binary once (nothing stays on PATH). Persistent `tool install`
//! projects bins into `~/.jet/bin` with per-install generation metadata under
//! `~/.jet/tools/` — a minimal isolated install until the shared
//! D-JPK-PROFILE1 `jet profile` surface is the front door. A bin name that
//! collides with a project `#Task fn` is E1297 (JPK-TOOL-COLLIDE).

use super::parse::Parsed;
use super::realize::{classify_or_report, RunPlan};
use super::trust_env_build::compose_env;
use crate::Output::Theme;
use crate::RefSpec;
use crate::Shell;
use crate::Store;
use crate::Syntax;
use jet_env_model::ModuleEval;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `jetpack tool <verb> …`
pub(super) fn cmd_tool(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::TOOL_VERB_RUN => tool_run(theme, parsed),
        Some(v) if v == Syntax::TOOL_VERB_INSTALL => tool_install(theme, parsed),
        Some(v) if v == Syntax::TOOL_VERB_LIST => tool_list(theme),
        Some(v) if v == Syntax::TOOL_VERB_UNINSTALL => tool_uninstall(theme, parsed),
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a jetpack tool verb"),
                &format!(
                    "`jetpack tool` verbs are: {}.",
                    Syntax::TOOL_VERBS.join(", ")
                ),
                "try `jetpack tool run <ref>`, `jetpack tool install <ref>`, `jetpack tool list`, or `jetpack tool uninstall <name>`.",
            );
            2
        }
        None => {
            theme.error(
                "`jetpack tool` needs a verb",
                &format!(
                    "verbs are: {} — ephemeral run or persistent PATH install (D-JPK-TOOLRUN1).",
                    Syntax::TOOL_VERBS.join(", ")
                ),
                "try `jetpack tool run nixpkgs:ripgrep -- rg --version`.",
            );
            2
        }
    }
}

fn tool_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack tool run` needs a package ref",
            "ephemeral tool execution realizes one `<source>:<package>` and runs its binary once — nothing is left on PATH.",
            "try `jetpack tool run nixpkgs:ripgrep -- rg --version`.",
        );
        return 2;
    };
    if let Some(code) = reject_unavailable_provider(theme, raw) {
        return code;
    }
    let Ok(spec) = classify_or_report(theme, raw) else {
        return 2;
    };
    let roots = Store::resolve();
    let plan = RunPlan {
        refs: vec![spec.clone()],
        adapters: Vec::new(),
        table: RefSpec::SourceTable::empty(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
    };
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let program = match &parsed.command {
        Some(cmd) if !cmd.is_empty() => cmd.clone(),
        _ => vec![spec.short_name().to_string()],
    };
    theme.status(&format!(
        "tool run {} -> {} (ephemeral)",
        theme.bold(&spec.raw),
        theme.bold(program.first().map(String::as_str).unwrap_or("?"))
    ));
    Shell::run_command(&env, &program)
}

fn tool_install(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack tool install` needs a package ref",
            "persistent install realizes one `<source>:<package>` and projects its bins onto `~/.jet/bin` as a tools-profile generation.",
            "try `jetpack tool install nixpkgs:ripgrep`.",
        );
        return 2;
    };
    if let Some(code) = reject_unavailable_provider(theme, raw) {
        return code;
    }
    let Ok(spec) = classify_or_report(theme, raw) else {
        return 2;
    };
    let as_name = parsed.flags.as_name.as_deref();
    let project_dir = std::env::current_dir().unwrap_or_default();
    // Early collision check on the projected name (package short name or --as).
    let preview = as_name.unwrap_or(spec.short_name());
    if let Some((task, path)) = find_task_collision(&project_dir, preview) {
        report_collide(theme, preview, &task, &path, raw);
        return 2;
    }
    let roots = Store::resolve();
    let plan = RunPlan {
        refs: vec![spec.clone()],
        adapters: Vec::new(),
        table: RefSpec::SourceTable::empty(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
    };
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let bin_dir = env
        .bin_dirs
        .first()
        .map(PathBuf::from)
        .filter(|p| p.is_dir());
    let Some(bin_dir) = bin_dir else {
        theme.error(
            &format!("`{}` has no bin directory to install", spec.raw),
            "tool install projects package binaries onto PATH; this realization produced no `bin/`.",
            "pick a package that ships executables, or use `jetpack tool run` for a one-shot.",
        );
        return 2;
    };
    let bins = match collect_bins(&bin_dir, as_name) {
        Ok(bins) if !bins.is_empty() => bins,
        Ok(_) => {
            theme.error(
                &format!("`{}` ships no executables under bin/", spec.raw),
                "tool install needs at least one executable to project onto `~/.jet/bin`.",
                "use `jetpack tool run` for a one-shot, or pick a different package.",
            );
            return 2;
        }
        Err(e) => {
            theme.error(
                "couldn't read package bin directory",
                &e,
                "check the realized package output and retry.",
            );
            return 2;
        }
    };
    for (bin_name, _) in &bins {
        if bin_name == preview {
            continue;
        }
        if let Some((task, path)) = find_task_collision(&project_dir, bin_name) {
            report_collide(theme, bin_name, &task, &path, raw);
            return 2;
        }
    }
    match project_install(theme, &spec, &bins) {
        Ok((gen, version)) => {
            for (name, _) in &bins {
                let link = user_bin_dir().join(name);
                let ver = if version.is_empty() {
                    String::new()
                } else {
                    format!(" {version}")
                };
                theme.status(&format!(
                    "installed {}{ver}  ->  {}   (profile \"{}\", generation {})",
                    theme.bold(spec.short_name()),
                    link.display(),
                    Syntax::TOOL_PROFILE_NAME,
                    gen
                ));
            }
            0
        }
        Err(e) => {
            theme.error(
                "tool install failed",
                &e,
                "check permissions on `~/.jet` and retry.",
            );
            2
        }
    }
}

fn tool_list(theme: &Theme) -> i32 {
    let tools = match read_current_tools() {
        Ok(t) => t,
        Err(e) => {
            theme.error("couldn't read installed tools", &e, "check `~/.jet/tools`.");
            return 2;
        }
    };
    if tools.is_empty() {
        theme.status("no tools installed yet.");
        return 0;
    }
    println!("TOOL     VERSION  SOURCE  BIN");
    for t in tools {
        println!(
            "{:<8} {:<8} {:<7} {}",
            t.name,
            if t.version.is_empty() {
                "-"
            } else {
                &t.version
            },
            t.source,
            t.bins.join(",")
        );
    }
    0
}

fn tool_uninstall(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack tool uninstall` needs a tool name",
            "names match `jetpack tool list` (package short name or projected bin).",
            "try `jetpack tool uninstall ripgrep`.",
        );
        return 2;
    };
    match uninstall_tool(theme, name) {
        Ok(true) => {
            theme.status(&format!("removed {name}"));
            0
        }
        Ok(false) => {
            theme.error(
                &format!("`{name}` is not an installed tool"),
                "uninstall only removes packages previously added with `jetpack tool install`.",
                "run `jetpack tool list` to see what's installed.",
            );
            2
        }
        Err(e) => {
            theme.error("tool uninstall failed", &e, "check `~/.jet/tools` and retry.");
            2
        }
    }
}

fn reject_unavailable_provider(theme: &Theme, raw: &str) -> Option<i32> {
    let source = raw.split_once(Syntax::REF_SEPARATOR).map(|(s, _)| s)?;
    if !Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&source) {
        return None;
    }
    theme.error_coded(
        Syntax::TOOL_DIAG_PROVIDER,
        &format!("tool provider `{source}` isn't available yet"),
        &format!(
            "D-JPK-TOOLRUN1 runs tools across providers, but `{source}:…` has no hangar realization path yet (JPK-TOOL-PROVIDER). Built-in providers that work today: nixpkgs, github, path."
        ),
        &format!(
            "use a built-in ref (`nixpkgs:…`, `github:…`, `path:…`), or wait for the `{source}` provider to land."
        ),
    );
    Some(2)
}

fn report_collide(theme: &Theme, bin: &str, task: &str, path: &Path, raw: &str) {
    let rel = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| format!("./{s}"))
        .unwrap_or_else(|| path.display().to_string());
    theme.error_coded(
        Syntax::TOOL_DIAG_COLLIDE,
        &format!("`{bin}` is already a task in {rel}"),
        &format!(
            "the project task `{task}` wins in this directory, so the global tool would be shadowed here (JPK-TOOL-COLLIDE / D-JPK-TOOLRUN1)."
        ),
        &format!(
            "install under a different bin name  ->  jetpack tool install {raw} {} <other>\n     or just run it once                  ->  jetpack tool run {raw}",
            Syntax::TOOL_FLAG_AS
        ),
    );
}

/// Scan project `.jet` sources for `#Task fn <name>` matching `bin`.
fn find_task_collision(dir: &Path, bin: &str) -> Option<(String, PathBuf)> {
    let mut files = Vec::new();
    collect_jet_files(dir, &mut files, 0);
    for path in files {
        let Ok(src) = fs::read_to_string(&path) else {
            continue;
        };
        for name in task_names_in(&src) {
            if name == bin {
                return Some((name, path));
            }
        }
    }
    None
}

fn collect_jet_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "build" || name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_jet_files(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            == Some(Syntax::FILE_EXT)
        {
            out.push(path);
        }
    }
}

fn task_names_in(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = src.as_bytes();
    let needle = b"#Task";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let after = &src[i + needle.len()..];
            let trimmed = after.trim_start();
            // Optional `#Every(…)` between `#Task` and `fn`.
            let after_every = if trimmed.starts_with('#') {
                // skip one more marker + optional (…)
                let mut rest = trimmed;
                if let Some(idx) = rest.find("fn") {
                    rest = &rest[idx..];
                }
                rest
            } else {
                trimmed
            };
            let rest = after_every.trim_start();
            if let Some(rest) = rest.strip_prefix("fn") {
                let rest = rest.trim_start();
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.push(name);
                }
            }
            i += needle.len();
        } else {
            i += 1;
        }
    }
    names
}

fn collect_bins(bin_dir: &Path, as_name: Option<&str>) -> Result<Vec<(String, PathBuf)>, String> {
    let mut bins = Vec::new();
    for entry in fs::read_dir(bin_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.metadata().map_err(|e| e.to_string())?.permissions().mode();
            if mode & 0o111 == 0 {
                continue;
            }
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let link_name = as_name.unwrap_or(&file_name).to_string();
        bins.push((link_name, path));
        if as_name.is_some() {
            break;
        }
    }
    bins.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(bins)
}

#[derive(Clone)]
struct InstalledTool {
    name: String,
    version: String,
    source: String,
    reference: String,
    bins: Vec<String>,
    targets: Vec<String>,
}

fn user_jet_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(Syntax::CONFIG_DEFAULT_DIR)
}

fn user_bin_dir() -> PathBuf {
    user_jet_dir().join(Syntax::TOOL_BIN_DIR)
}

fn tools_state_dir() -> PathBuf {
    user_jet_dir().join(Syntax::TOOL_STATE_DIR)
}

fn generations_dir() -> PathBuf {
    tools_state_dir().join("generations")
}

fn profile_path() -> PathBuf {
    tools_state_dir().join("profile.json")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_current_generation() -> Result<u64, String> {
    let path = profile_path();
    if !path.is_file() {
        return Ok(0);
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(parse_json_u64(&text, "current").unwrap_or(0))
}

fn read_current_tools() -> Result<Vec<InstalledTool>, String> {
    let gen = read_current_generation()?;
    if gen == 0 {
        return Ok(Vec::new());
    }
    read_generation_tools(gen)
}

fn read_generation_tools(gen: u64) -> Result<Vec<InstalledTool>, String> {
    let path = generations_dir().join(gen.to_string()).join("meta.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(parse_tools_array(&text))
}

fn project_install(
    _theme: &Theme,
    spec: &RefSpec::RefSpec,
    bins: &[(String, PathBuf)],
) -> Result<(u64, String), String> {
    let mut tools = read_current_tools()?;
    let short = spec.short_name().to_string();
    tools.retain(|t| {
        t.name != short && t.bins.iter().all(|b| bins.iter().all(|(n, _)| n != b))
    });
    let version = Store::list(&Store::resolve())
        .into_iter()
        .find(|e| e.reference == spec.raw)
        .map(|e| e.version)
        .unwrap_or_default();
    let tool = InstalledTool {
        name: short,
        version: version.clone(),
        source: spec.source.label().to_string(),
        reference: spec.raw.clone(),
        bins: bins.iter().map(|(n, _)| n.clone()).collect(),
        targets: bins
            .iter()
            .map(|(_, p)| p.to_string_lossy().into_owned())
            .collect(),
    };
    tools.push(tool);
    let gen = write_generation(&tools)?;
    Ok((gen, version))
}

fn uninstall_tool(theme: &Theme, name: &str) -> Result<bool, String> {
    let _ = theme;
    let mut tools = read_current_tools()?;
    let before = tools.len();
    let removed: Vec<_> = tools
        .iter()
        .filter(|t| t.name == name || t.bins.iter().any(|b| b == name))
        .cloned()
        .collect();
    if removed.is_empty() {
        return Ok(false);
    }
    tools.retain(|t| t.name != name && t.bins.iter().all(|b| b != name));
    let gen = write_generation(&tools)?;
    let _ = gen;
    let _ = before;
    Ok(true)
}

fn write_generation(tools: &[InstalledTool]) -> Result<u64, String> {
    let prev = read_current_generation()?;
    let gen = prev + 1;
    let gen_dir = generations_dir().join(gen.to_string());
    fs::create_dir_all(&gen_dir).map_err(|e| e.to_string())?;
    let meta = format_generation_meta(gen, tools);
    fs::write(gen_dir.join("meta.json"), meta).map_err(|e| e.to_string())?;

    let bin_dir = user_bin_dir();
    fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    // Drop previous generation's projected links that we own, then recreate.
    if prev > 0 {
        if let Ok(old) = read_generation_tools(prev) {
            for t in old {
                for b in t.bins {
                    let link = bin_dir.join(&b);
                    if link.is_symlink() || link.is_file() {
                        let _ = fs::remove_file(&link);
                    }
                }
            }
        }
    }
    for t in tools {
        for (bin, target) in t.bins.iter().zip(t.targets.iter()) {
            let link = bin_dir.join(bin);
            if link.exists() || link.is_symlink() {
                let _ = fs::remove_file(&link);
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, &link).map_err(|e| e.to_string())?;
            }
            #[cfg(not(unix))]
            {
                fs::copy(target, &link).map_err(|e| e.to_string())?;
            }
        }
    }

    let profile = format!(
        "{{\n  \"name\": \"{}\",\n  \"current\": {}\n}}\n",
        Syntax::TOOL_PROFILE_NAME,
        gen
    );
    fs::create_dir_all(tools_state_dir()).map_err(|e| e.to_string())?;
    fs::write(profile_path(), profile).map_err(|e| e.to_string())?;
    Ok(gen)
}

fn format_generation_meta(gen: u64, tools: &[InstalledTool]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"generation\": {gen},\n"));
    out.push_str(&format!("  \"profile\": \"{}\",\n", Syntax::TOOL_PROFILE_NAME));
    out.push_str(&format!("  \"created_at\": {},\n", now_secs()));
    out.push_str("  \"tools\": [\n");
    for (i, t) in tools.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": {},\n", json_str(&t.name)));
        out.push_str(&format!("      \"version\": {},\n", json_str(&t.version)));
        out.push_str(&format!("      \"source\": {},\n", json_str(&t.source)));
        out.push_str(&format!("      \"reference\": {},\n", json_str(&t.reference)));
        out.push_str("      \"bins\": [");
        for (j, b) in t.bins.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str(&json_str(b));
        }
        out.push_str("],\n");
        out.push_str("      \"targets\": [");
        for (j, b) in t.targets.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str(&json_str(b));
        }
        out.push_str("]\n");
        out.push_str("    }");
        if i + 1 < tools.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

fn json_str(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
    )
}

fn parse_json_u64(text: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let idx = text.find(&needle)?;
    let after = &text[idx + needle.len()..];
    let after = after.trim_start().trim_start_matches(':').trim_start();
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn parse_tools_array(text: &str) -> Vec<InstalledTool> {
    let mut tools = Vec::new();
    // Split on tool objects by `"name":` occurrences inside the tools array.
    let Some(array_start) = text.find("\"tools\"") else {
        return tools;
    };
    let body = &text[array_start..];
    let mut rest = body;
    while let Some(name_idx) = rest.find("\"name\"") {
        let after_name = &rest[name_idx + 6..];
        let Some(name) = json_string_value(after_name) else {
            break;
        };
        let chunk_end = rest[name_idx + 6..]
            .find("\"name\"")
            .map(|i| name_idx + 6 + i)
            .unwrap_or(rest.len());
        let chunk = &rest[name_idx..chunk_end];
        let version = extract_json_string(chunk, "version").unwrap_or_default();
        let source = extract_json_string(chunk, "source").unwrap_or_default();
        let reference = extract_json_string(chunk, "reference").unwrap_or_default();
        let bins = extract_json_string_array(chunk, "bins");
        let targets = extract_json_string_array(chunk, "targets");
        tools.push(InstalledTool {
            name,
            version,
            source,
            reference,
            bins,
            targets,
        });
        rest = &rest[chunk_end..];
        if rest.is_empty() {
            break;
        }
    }
    tools
}

fn extract_json_string(chunk: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = chunk.find(&needle)?;
    json_string_value(&chunk[idx + needle.len()..])
}

fn json_string_value(after_key: &str) -> Option<String> {
    let after = after_key.trim_start().trim_start_matches(':').trim_start();
    let after = after.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = after.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn extract_json_string_array(chunk: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let Some(idx) = chunk.find(&needle) else {
        return Vec::new();
    };
    let after = &chunk[idx + needle.len()..];
    let Some(bracket) = after.find('[') else {
        return Vec::new();
    };
    let Some(end) = after[bracket + 1..].find(']') else {
        return Vec::new();
    };
    let inner = &after[bracket + 1..bracket + 1 + end];
    let parts: Vec<&str> = inner.split('"').collect();
    let mut out = Vec::new();
    let mut i = 1;
    while i < parts.len() {
        out.push(parts[i].to_string());
        i += 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_parse_plain_and_every() {
        let src = r#"
#Task fn serve() { }
#Every(5min) #Task fn lint() { }
fn run() { }
"#;
        let names = task_names_in(src);
        assert!(names.contains(&"serve".into()), "{names:?}");
        assert!(names.contains(&"lint".into()), "{names:?}");
        assert!(!names.iter().any(|n| n == "run"));
    }

    #[test]
    fn external_provider_list_covers_ballot_examples() {
        assert!(Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&"npm"));
        assert!(Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&"cargo"));
        assert!(Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&"pypi"));
    }
}

use super::parse::Parsed;
use super::workspace_sources::project_root;
use crate::Lock::{self, LockDiff, LockPackageChange, LockPackageChangeKind};
use crate::Output::Theme;
use crate::Store;
use crate::Syntax;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Default)]
pub(super) struct DiffSizes {
    pub(super) download: Option<i64>,
    pub(super) disk: Option<i64>,
}

pub(super) fn sizes_from_plan(plan: &crate::Provider::DownloadPlan) -> DiffSizes {
    sizes_from_bytes(plan.download_bytes, plan.disk_bytes)
}

pub(super) fn sizes_from_bytes(download: Option<u64>, disk: Option<u64>) -> DiffSizes {
    DiffSizes {
        download: download.map(signed_bytes),
        disk: disk.map(signed_bytes),
    }
}

fn signed_bytes(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

/// Print one diff when a command mutates the unified lock. The guard catches
/// early returns as well as successful realization, so a partial lock update is
/// still visible to the user.
pub(super) struct LockDiffGuard {
    theme: Theme,
    root: PathBuf,
    before: Option<Lock::LockFile>,
    sizes: DiffSizes,
}

impl LockDiffGuard {
    pub(super) fn new(theme: &Theme, root: &Path) -> Self {
        Self {
            theme: *theme,
            root: root.to_path_buf(),
            before: Lock::load(root),
            sizes: DiffSizes::default(),
        }
    }

    pub(super) fn set_sizes(&mut self, sizes: DiffSizes) {
        self.sizes = sizes;
    }
}

impl Drop for LockDiffGuard {
    fn drop(&mut self) {
        let after = Lock::load(&self.root);
        let diff = LockDiff::between(self.before.as_ref(), after.as_ref());
        if !diff.is_empty() {
            render_lock_diff(&self.theme, &diff, self.sizes);
        }
    }
}

/// `jetpack lock diff [--against rev]` compares the working lock with a git
/// revision. The default revision is `HEAD`, which makes the common dirty-lock
/// review path one short command.
pub(super) fn cmd_lock(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.positional.as_slice() != [Syntax::LOCK_DIFF_VERB] || parsed.command.is_some() {
        theme.error(
            "`jetpack lock` needs the `diff` subcommand",
            "the unified `.jet/lock` is inspected through one read-only diff surface",
            "run `jetpack lock diff [--against <rev>]`",
        );
        return 2;
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let root = project_root(&cwd);
    let current = match read_lock(&root) {
        Ok(lock) => lock,
        Err(error) => {
            theme.error(
                "could not read the current lock",
                &error,
                "restore a valid `.jet/lock` and run the diff again",
            );
            return 2;
        }
    };
    let revision = parsed
        .flags
        .lock_against
        .as_deref()
        .filter(|revision| !revision.is_empty())
        .unwrap_or("HEAD");
    let against = match read_lock_at_revision(&root, revision) {
        Ok(lock) => lock,
        Err(error) => {
            theme.error(
                "could not read the lock at the requested revision",
                &error,
                "use a commit, tag, or revision that contains `.jet/lock`",
            );
            return 2;
        }
    };
    let diff = LockDiff::between(Some(&against), Some(&current));
    let sizes = DiffSizes {
        disk: measured_disk_delta(&diff),
        ..DiffSizes::default()
    };
    if diff.is_empty() {
        theme.status("Lock unchanged.");
    } else {
        render_lock_diff(theme, &diff, sizes);
    }
    0
}

pub(super) fn render_lock_diff(theme: &Theme, diff: &LockDiff, sizes: DiffSizes) {
    theme.status("Lock changes");
    for line in render_lock_diff_lines(diff, sizes) {
        theme.detail(&line);
    }
}

fn render_lock_diff_lines(diff: &LockDiff, sizes: DiffSizes) -> Vec<String> {
    let disk = measured_disk_delta(diff).or(sizes.disk);
    let mut lines = Vec::with_capacity(diff.packages.len() + diff.channels.len() + 3);
    for change in &diff.packages {
        lines.push(render_package_change(change));
    }
    for change in &diff.channels {
        let (mark, before, after) = match (&change.before, &change.after) {
            (None, Some(after)) => ('+', "—", after.as_str()),
            (Some(before), None) => ('-', before.as_str(), "—"),
            (Some(before), Some(after)) => ('~', before.as_str(), after.as_str()),
            (None, None) => continue,
        };
        lines.push(format!("{mark} channel {} {before} → {after}", change.name));
    }
    lines.push(format!(
        "{} added, {} removed, {} updated",
        diff.added_count(),
        diff.removed_count(),
        diff.updated_count()
    ));
    lines.push(format!(
        "net download {} · net disk {}",
        render_delta(sizes.download),
        render_delta(disk)
    ));
    for change in &diff.packages {
        if let (Some(before), Some(after)) = (&change.before_trust, &change.after_trust) {
            if before != after {
                lines.push(format!(
                    "trust {}: {before} → {after}",
                    change.name
                ));
            }
        }
    }
    lines
}

fn render_package_change(change: &LockPackageChange) -> String {
    match change.kind {
        LockPackageChangeKind::Added => format!(
            "+ {} {} · trust {}",
            change.name,
            display_value(change.after_version.as_deref()),
            display_value(change.after_trust.as_deref())
        ),
        LockPackageChangeKind::Removed => format!(
            "- {} {} · trust {}",
            change.name,
            display_value(change.before_version.as_deref()),
            display_value(change.before_trust.as_deref())
        ),
        LockPackageChangeKind::Updated => format!(
            "~ {} {} → {}",
            change.name,
            display_value(change.before_version.as_deref()),
            display_value(change.after_version.as_deref())
        ),
    }
}

fn display_value(value: Option<&str>) -> &str {
    value.unwrap_or("—")
}

fn render_delta(value: Option<i64>) -> String {
    match value {
        Some(0) => "0 B".to_string(),
        Some(value) => {
            let sign = if value.is_negative() { '-' } else { '+' };
            format!("{sign}{}", crate::Output::human_size(value.unsigned_abs()))
        }
        None => "unknown".to_string(),
    }
}

fn measured_disk_delta(diff: &LockDiff) -> Option<i64> {
    let mut paths = BTreeSet::new();
    let mut delta = 0i128;
    let mut measured = false;
    for change in &diff.packages {
        match change.kind {
            LockPackageChangeKind::Added => {
                let bytes = output_size(change.after_output.as_ref())?;
                measured = true;
                if change
                    .after_output
                    .as_ref()
                    .is_some_and(|path| paths.insert(path.clone()))
                {
                    delta += i128::from(bytes);
                }
            }
            LockPackageChangeKind::Removed => {
                let bytes = output_size(change.before_output.as_ref())?;
                measured = true;
                if change
                    .before_output
                    .as_ref()
                    .is_some_and(|path| paths.insert(path.clone()))
                {
                    delta -= i128::from(bytes);
                }
            }
            LockPackageChangeKind::Updated => {
                if change.before_output == change.after_output {
                    continue;
                }
                let before = output_size(change.before_output.as_ref())?;
                let after = output_size(change.after_output.as_ref())?;
                measured = true;
                delta += i128::from(after) - i128::from(before);
            }
        }
    }
    if !measured {
        return None;
    }
    Some(delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

fn output_size(path: Option<&String>) -> Option<u64> {
    let path = path.filter(|path| !path.is_empty())?;
    let path = Path::new(path);
    if !path.exists() {
        return None;
    }
    Some(Store::dir_size(path))
}

fn read_lock(root: &Path) -> Result<Lock::LockFile, String> {
    let path = root.join(Syntax::UNIFIED_LOCK_FILE);
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("`{}`: {error}", path.display()))?;
    Lock::parse(&raw).map_err(|error| format!("`{}`: {error}", path.display()))
}

fn read_lock_at_revision(root: &Path, revision: &str) -> Result<Lock::LockFile, String> {
    if revision.is_empty()
        || revision.starts_with('-')
        || revision.contains(':')
        || revision.chars().any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!("invalid git revision `{revision}`"));
    }
    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", "--end-of-options"])
        .arg(format!("{revision}^{{commit}}"))
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !commit.status.success() {
        return Err(command_error("git rev-parse", &commit));
    }
    let commit = String::from_utf8_lossy(&commit.stdout).trim().to_string();
    let path = format!("{commit}:{}", Syntax::UNIFIED_LOCK_FILE);
    let lock = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", "--format=", "--no-ext-diff"])
        .arg(path)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !lock.status.success() {
        return Err(command_error("git show", &lock));
    }
    let raw = String::from_utf8_lossy(&lock.stdout);
    Lock::parse(&raw).map_err(|error| format!("lock at `{revision}`: {error}"))
}

fn command_error(command: &str, output: &std::process::Output) -> String {
    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if error.is_empty() {
        format!("{command} failed with {}", output.status)
    } else {
        format!("{command}: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(package: &str, version: &str, trust: &str) -> Lock::LockFile {
        let catalog = match trust {
            "signed" => ("official-signed", "verified"),
            "unverified-mapping" => ("local-unofficial", "unverified"),
            _ => ("", ""),
        };
        Lock::parse(&format!(
            "version = 1\n\n[[package]]\nname = \"{package}\"\nversion = \"{version}\"\nsource = {{ nix = \"{package}@nixpkgs\", output = \"\" }}\nfingerprint = \"fp\"\ndependencies = []\noutput-hash = \"hash\"\nplatform = \"x86_64-linux\"\ncatalog-tier = \"{}\"\ncatalog-trust = \"{}\"\nprovenance = \"\"\n",
            catalog.0, catalog.1
        ))
        .expect("test lock")
    }

    #[test]
    fn lock_diff_rendering_is_changelog_shaped() {
        let before = lock("alpha", "1.0.0", "signed");
        let after = lock("alpha", "1.1.0", "unverified-mapping");
        let mut after = after;
        after.packages.push(
            Lock::parse(
                "version = 1\n\n[[package]]\nname = \"beta\"\nversion = \"2.0.0\"\nsource = { path = \"beta\" }\nfingerprint = \"fp\"\ndependencies = []\n",
            )
            .expect("added package")
            .packages
            .remove(0),
        );
        let diff = LockDiff::between(Some(&before), Some(&after));
        let rendered = render_lock_diff_lines(
            &diff,
            DiffSizes {
                download: Some(1_500_000),
                disk: Some(-2_000_000),
            },
        )
        .join("\n");
        assert!(rendered.contains("~ alpha 1.0.0 → 1.1.0"));
        assert!(rendered.contains("+ beta 2.0.0"));
        assert!(rendered.contains("1 added, 0 removed, 1 updated"));
        assert!(rendered.contains("net download +1.5 MB · net disk -2.0 MB"));
        assert!(rendered.contains("trust alpha: signed → unverified-mapping"));
    }
}

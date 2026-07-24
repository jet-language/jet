//! D-TELEMETRY1=A: explicit, local-only toolchain report bundles.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const README: &str = "\
Jet local report bundle

This bundle was created only because you ran `jet report`.
Jet has not sent it anywhere.

Included:
- Jet version and supported edition information
- compiler target, operating-system family, and architecture
- the permanent zero-telemetry policy

Excluded:
- source code and source paths
- current directory and command history
- arguments and environment values
- hostname, username, machine identifiers, and network addresses
- crash data and package names
";

fn report_text() -> String {
    format!(
        "Jet local report\n\
policy: zero telemetry; no network transmission\n\
target: {}\n\
os: {}\n\
arch: {}\n\
{}",
        env!("JET_BUILD_TARGET"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        jet::Manifest::version_banner(),
    )
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    loop {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!("`{}` is not a trusted directory", path.display()));
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match create_private_dir(path) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        return Err(format!("could not create `{}` ({error})", path.display()));
                    }
                }
            }
            Err(error) => {
                return Err(format!("could not inspect `{}` ({error})", path.display()));
            }
        }
    }
}

fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("could not write `{}` ({error})", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("could not write `{}` ({error})", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("could not protect `{}` ({error})", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn restore_private_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect `{}` ({error})", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("`{}` is linked outside the report", path.display()));
    }
    if metadata.permissions().mode() & 0o7777 != mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| format!("could not protect `{}` ({error})", path.display()))?;
    }
    let restored = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect `{}` ({error})", path.display()))?;
    if restored.file_type().is_symlink() || restored.permissions().mode() & 0o7777 != mode {
        return Err(format!(
            "`{}` does not have private mode {:04o}",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn restore_private_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

fn validate_bundle(bundle: &Path, report: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(bundle)
        .map_err(|error| format!("could not inspect `{}` ({error})", bundle.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("`{}` is not a report directory", bundle.display()));
    }
    restore_private_mode(bundle, 0o700)?;
    let entries = fs::read_dir(bundle)
        .map_err(|error| format!("could not inspect `{}` ({error})", bundle.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not inspect `{}` ({error})", bundle.display()))?;
    if entries.len() != 2 {
        return Err(format!("existing report `{}` was changed", bundle.display()));
    }
    for (name, expected) in [("README.txt", README), ("report.txt", report)] {
        let path = bundle.join(name);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect `{}` ({error})", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("existing report `{}` was changed", path.display()));
        }
        restore_private_mode(&path, 0o600)?;
        if !fs::read_to_string(&path)
            .map(|contents| contents == expected)
            .unwrap_or(false)
        {
            return Err(format!("existing report `{}` was changed", path.display()));
        }
    }
    Ok(())
}

fn create_bundle() -> Result<PathBuf, String> {
    let report = report_text();
    let identity = jet::SHA256::sha256_hex(
        format!("README.txt\0{README}\0report.txt\0{report}").as_bytes(),
    );
    let jet_dir = PathBuf::from(".jet");
    ensure_directory(&jet_dir)?;
    let reports = jet_dir.join("reports");
    ensure_directory(&reports)?;
    let bundle = reports.join(&identity);
    match fs::symlink_metadata(&bundle) {
        Ok(_) => {
            validate_bundle(&bundle, &report)?;
            return Ok(bundle);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("could not inspect `{}` ({error})", bundle.display()));
        }
    }

    let staging = reports.join(format!(".{identity}.tmp-{}", std::process::id()));
    create_private_dir(&staging)
        .map_err(|error| format!("could not create `{}` ({error})", staging.display()))?;
    let staged = (|| {
        write_private(&staging.join("README.txt"), README)?;
        write_private(&staging.join("report.txt"), &report)?;
        Ok::<(), String>(())
    })();
    if let Err(error) = staged {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    match fs::symlink_metadata(&bundle) {
        Ok(_) => {
            let result = validate_bundle(&bundle, &report);
            let _ = fs::remove_dir_all(&staging);
            result?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = fs::rename(&staging, &bundle) {
                let _ = fs::remove_dir_all(&staging);
                if fs::symlink_metadata(&bundle).is_ok() {
                    validate_bundle(&bundle, &report)?;
                } else {
                    return Err(format!(
                        "could not finish report `{}` ({error})",
                        bundle.display()
                    ));
                }
            }
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("could not inspect `{}` ({error})", bundle.display()));
        }
    }
    Ok(bundle)
}

pub(crate) fn run_report(args: &[String]) -> i32 {
    if !args.is_empty() {
        eprintln!("error: `jet report` takes no arguments");
        eprintln!(" fix: run `jet report` to write a private local bundle");
        return jet::ExitCodes::USAGE;
    }
    match create_bundle() {
        Ok(path) => {
            println!("wrote local report bundle to {}", path.display());
            0
        }
        Err(error) => {
            eprintln!("error: could not create local report bundle");
            eprintln!(" why: {error}");
            eprintln!(" fix: check write access to the current directory, then run `jet report` again");
            jet::ExitCodes::USER_ERROR
        }
    }
}

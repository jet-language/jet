use super::parse::Parsed;
use crate::Output::Theme;
use crate::Store;

/// `jetpack shared-store install|enroll|broker` — optional authenticated shared store.
pub(super) fn cmd_shared_store(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str).unwrap_or("install") {
        "install" => match Store::install_shared_store(&Store::resolve()) {
            Ok(report) => {
                theme.status(&format!("installed shared-store config at {}", report.config.display()));
                if let Some(socket) = report.socket_unit {
                    theme.detail(&format!("socket unit: {}", socket.display()));
                }
                if let Some(service) = report.service_unit {
                    theme.detail(&format!("service unit: {}", service.display()));
                }
                0
            }
            Err(error) => {
                theme.error(
                    "shared-store install failed",
                    &error.to_string(),
                    "check the administrator-selected Jetpack root and keep the broker optional.",
                );
                2
            }
        },
        "broker" => {
            #[cfg(unix)]
            {
                let fd = parsed
                    .positional
                    .windows(2)
                    .find(|pair| pair[0] == "--fd")
                    .and_then(|pair| pair[1].parse::<i32>().ok())
                    .unwrap_or(3);
                return match Store::serve_shared_store_fd(&Store::resolve(), fd) {
                    Ok(()) => 0,
                    Err(error) => {
                        theme.error(
                            "shared-store broker failed",
                            &error.to_string(),
                            "the broker verifies one provenance-bound archive request and exits; reinstall the socket unit if it is stale.",
                        );
                        2
                    }
                };
            }
            #[cfg(not(unix))]
            {
                theme.error(
                    "shared-store broker is unavailable",
                    "this host has no Unix socket activation boundary",
                    "use the ordinary per-user Hangar on this host.",
                );
                2
            }
        }
        "status" => match Store::shared_store_config(&Store::resolve()) {
            Ok(Some(config)) => {
                theme.status(&format!("shared-store broker configured at {}", config.socket.display()));
                0
            }
            Ok(None) => {
                theme.status("shared-store broker is not installed.");
                0
            }
            Err(error) => {
                theme.error("shared-store status failed", &error.to_string(), "repair the administrator-installed shared-store config.");
                2
            }
        },
        "enroll" => {
            let Some(uid) = parsed.positional.get(1) else {
                theme.error(
                    "shared-store enrollment needs a uid",
                    "no user id was supplied",
                    "run `jet shared-store enroll <uid>` as the administrator.",
                );
                return 2;
            };
            let read_only = parsed.positional.iter().any(|item| item == "--read-only");
            let unknown = parsed
                .positional
                .iter()
                .skip(1)
                .any(|item| item != uid && item != "--read-only");
            if unknown {
                theme.error(
                    "shared-store enrollment has an unknown argument",
                    "enrollment accepts one uid and optional `--read-only`",
                    "run `jet shared-store enroll <uid> [--read-only]`.",
                );
                return 2;
            }
            match Store::enroll_shared_store(&Store::resolve(), uid, !read_only) {
                Ok(path) => {
                    theme.status(&format!("enrolled shared-store user {uid} at {}", path.display()));
                    0
                }
                Err(error) => {
                    theme.error(
                        "shared-store enrollment failed",
                        &error.to_string(),
                        "run the command as the administrator after installing the broker.",
                    );
                    2
                }
            }
        }
        other => {
            theme.error(
                &format!("`shared-store {other}` is not a command"),
                "the shared-store commands are `install`, `enroll`, `status`, and the socket-activated `broker` entrypoint.",
                "run `jet shared-store install`.",
            );
            2
        }
    }
}

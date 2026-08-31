// c-devserver (owner-directed 2026-07-01): `core.web.devserver` — a `.jet` file's
// own `jet dev` behavior as ordinary Jet code, via a real configurable value
// (the same shape as `core.ui.null_backend()`, see Prelude/Ui.rs), instead of
// `--port=<N>`/`--target=web` CLI flags. Std-only (I6): `TcpListener`, manual
// HTTP/1.1 parsing, mtime-poll watch, `std::process::Command` to shell out to
// the real `jet` binary for each rebuild. This file is compiled INTO the
// standalone native binary produced for `fn dev()` — it never links against
// jet-parser/jet-sema/jet-codegen/jet-driver, so a rebuild always goes through
// `jet build --target=web <file>` as a subprocess, mirroring the design of
// Source/CmdDevWeb.rs's `run_dev_web` (mtime-poll watch, atomic staged
// rebuild, polling live-reload) without literally calling its internal
// functions.
//
// Everything lives inside a private module (matching Prelude/CoreLib.rs's own
// `mod jet_std { ... }` convention) because this fragment is concatenated
// into ONE Rust module with every other prelude fragment — top-level `use`
// here would collide with e.g. Prelude/Scheduler.rs's own
// `TcpStream`/`Ordering`/`Arc`/`Duration` imports.
mod jet_devserver_impl {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Component, Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    /// Ports tried, in order, before giving up when no explicit `.port(n)`
    /// was set — mirrors `Source/CmdDevWeb.rs`'s `PORT_RANGE`.
    const JET_DEVSERVER_PORT_RANGE: std::ops::RangeInclusive<u16> = 8080..=8089;

    /// Live-reload poll interval — mirrors `Source/CmdDevWeb.rs`'s
    /// `LIVE_RELOAD_POLL_MS`.
    const JET_DEVSERVER_LIVE_RELOAD_POLL_MS: u64 = 400;
    const JET_DEVSERVER_MAX_LINE_BYTES: usize = 8 * 1024;
    const JET_DEVSERVER_MAX_HEADER_BYTES: usize = 32 * 1024;
    const JET_DEVSERVER_MAX_HEADER_COUNT: usize = 100;
    const JET_DEVSERVER_MAX_CONNECTION_THREADS: usize = 64;
    const JET_DEVSERVER_MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
    const JET_DEVSERVER_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
    const JET_DEVSERVER_MAX_CHILD_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
    const JET_DEVSERVER_REQUEST_DEADLINE: Duration = Duration::from_secs(10);
    static JET_DEVSERVER_STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct JetDevServerState {
        app_file: String,
        html_override: Option<String>,
        port: Option<u16>,
    }

    struct JetDevServerOutputRoot {
        directory: File,
        publication_lock: Arc<Mutex<()>>,
    }

    struct JetDevServerStaging {
        path: PathBuf,
        name: String,
        source_stem: String,
        directory: File,
    }

    impl JetDevServerOutputRoot {
        fn new(directory: File) -> Self {
            Self {
                directory,
                publication_lock: Arc::new(Mutex::new(())),
            }
        }

        fn clone_directory(&self) -> std::io::Result<Self> {
            Ok(Self {
                directory: self.directory.try_clone()?,
                publication_lock: Arc::clone(&self.publication_lock),
            })
        }
    }

    /// The value `core.web.devserver.for_app(...)` returns. Cheap to clone (an
    /// `Rc` handle around shared state) so every builder method can take
    /// `&self` and still hand back a `DevServer` for chaining, exactly like
    /// `core.ui.null_backend()`'s `JetNullBackend` handle.
    #[derive(Clone)]
    pub struct JetDevServer {
        state: std::rc::Rc<std::cell::RefCell<JetDevServerState>>,
    }

    pub fn jet_devserver_for_app(file: &str) -> JetDevServer {
        // Relative or absolute both work: canonicalize against the process's
        // working directory (inherited from wherever `jet dev` was invoked)
        // up front, so every later use — the watch loop, `.html()` sibling
        // resolution, the rebuild subprocess — sees one absolute path and
        // is immune to any cwd changes after this point. A path that doesn't
        // resolve is kept verbatim; `serve()` reports it with the original
        // spelling the user wrote.
        let resolved = std::fs::canonicalize(file)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.to_string());
        JetDevServer {
            state: std::rc::Rc::new(std::cell::RefCell::new(JetDevServerState {
                app_file: resolved,
                html_override: None,
                port: None,
            })),
        }
    }

    /// `devserver.app()` — the file `jet dev` launched, no path spelled out.
    /// `jet dev <file>` passes the canonical absolute path of `<file>` to
    /// this program in the `JET_DEV_FILE` environment variable
    /// (Source/CmdCompile.rs `run_dev_entry`); since the watched file and
    /// the file defining `fn dev()` are almost always the same file, this is
    /// the form to reach for first — `for_app(path)` is for the rarer case
    /// of watching a *different* entry file than the one that owns `dev()`.
    pub fn jet_devserver_app() -> JetDevServer {
        match std::env::var("JET_DEV_FILE") {
            Ok(file) if !file.is_empty() => jet_devserver_for_app(&file),
            _ => {
                eprintln!("error: `devserver.app()` only works under `jet dev`");
                eprintln!(
                    " why: it watches \"the file jet dev launched\", which the `jet dev` command passes in as JET_DEV_FILE — running this binary directly leaves that unset"
                );
                eprintln!(
                    " fix: run `jet dev <file.jet>`, or name the file explicitly with `devserver.for_app(\"path/to/app.jet\")`"
                );
                std::process::exit(1);
            }
        }
    }

    impl JetDevServer {
        /// Set the companion HTML page — takes priority over the `.jet`
        /// file's own `#HTML(...)` marker / `<stem>.html` sibling convention
        /// (both still apply, inside the `jet build --target=web`
        /// subprocess, when `.html` was never called).
        pub fn html(&self, path: String) -> JetDevServer {
            self.state.borrow_mut().html_override = Some(path);
            self.clone()
        }

        /// Set the port to bind. Out-of-range values fail loud immediately
        /// (same validation as the CLI's own `--port=<N>`), rather than
        /// silently wrapping into a bogus port.
        pub fn port(&self, n: i64) -> JetDevServer {
            if !(1..=65535).contains(&n) {
                eprintln!("error: `.port({})` isn't a valid port number", n);
                eprintln!(" fix: use a number from 1 to 65535");
                std::process::exit(1);
            }
            self.state.borrow_mut().port = Some(n as u16);
            self.clone()
        }

        /// Build once, serve `build/` with live-reload, then watch the app
        /// file and rebuild on every save. Blocks forever (Ctrl-C stops it)
        /// — the same contract as `jet dev <file> --target=web`.
        pub fn serve(&self) {
            let (app_file, html_override, port_pref) = {
                let s = self.state.borrow();
                (s.app_file.clone(), s.html_override.clone(), s.port)
            };

            if !Path::new(&app_file).exists() {
                eprintln!("error: can't find the file `{}`", app_file);
                eprintln!(" fix: check the path passed to `devserver.for_app(...)`");
                std::process::exit(1);
            }

            let output_root = match jet_devserver_rebuild(
                &app_file,
                html_override.as_deref(),
                false,
                None,
            ) {
                Some(root) => Arc::new(root),
                None => std::process::exit(1),
            };

            let version = Arc::new(AtomicU64::new(1));
            let listener = jet_devserver_bind(port_pref);
            let bound_port = listener
                .local_addr()
                .map(|a| a.port())
                .unwrap_or(*JET_DEVSERVER_PORT_RANGE.start());

            {
                let version = Arc::clone(&version);
                let output_root = Arc::clone(&output_root);
                thread::spawn(move || {
                    jet_devserver_serve_forever(listener, version, output_root)
                });
            }

            println!("App preview: http://127.0.0.1:{}/", bound_port);
            println!("watching {} … (Ctrl-C to stop)", app_file);
            let _ = std::io::stdout().flush();

            let mut last_mtime = jet_devserver_mtime(Path::new(&app_file));
            loop {
                thread::sleep(Duration::from_millis(120));
                let now = jet_devserver_mtime(Path::new(&app_file));
                if now != last_mtime {
                    last_mtime = now;
                    thread::sleep(Duration::from_millis(30));
                    if jet_devserver_rebuild(
                        &app_file,
                        html_override.as_deref(),
                        true,
                        Some(&output_root),
                    )
                    .is_some()
                    {
                        version.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    fn jet_devserver_mtime(path: &Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Bind the dev server's `TcpListener`. `Some(port)` binds that exact
    /// port and fails loud if it's taken; `None` scans
    /// `JET_DEVSERVER_PORT_RANGE`.
    fn jet_devserver_bind(port: Option<u16>) -> TcpListener {
        if let Some(port) = port {
            return match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => listener,
                Err(e) => {
                    eprintln!("error: couldn't bind to port {}: {}", port, e);
                    if e.kind() == std::io::ErrorKind::AddrInUse {
                        eprintln!(" fix: stop whatever's using port {}, or pick another with `.port(n)`", port);
                    }
                    std::process::exit(1);
                }
            };
        }
        for port in JET_DEVSERVER_PORT_RANGE {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => return listener,
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
                Err(e) => {
                    eprintln!("error: couldn't start the dev server: {}", e);
                    std::process::exit(1);
                }
            }
        }
        eprintln!(
            "error: every port from {} to {} is already in use",
            JET_DEVSERVER_PORT_RANGE.start(),
            JET_DEVSERVER_PORT_RANGE.end()
        );
        eprintln!(" fix: free one of those ports, or pick one explicitly with `.port(n)`");
        std::process::exit(1);
    }

    /// Compile `app_file` for the web target by shelling out to the real
    /// `jet` binary, and on success atomically replace `build/*` with the
    /// new output.
    ///
    /// The subprocess runs with its OWN working directory pointed at a fresh
    /// staging root, passing
    /// `app_file` as an absolute path so it resolves the same regardless of
    /// that working directory. The subprocess therefore writes into
    /// `<staging>/build/*` — untouched by any previous good build. Only after
    /// the complete build succeeds do held directory authorities publish the
    /// expected files into the real `build/`.
    fn jet_devserver_rebuild(
        app_file: &str,
        html_override: Option<&str>,
        is_rebuild: bool,
        held_output_root: Option<&JetDevServerOutputRoot>,
    ) -> Option<JetDevServerOutputRoot> {
        let abs_file = match std::fs::canonicalize(app_file) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: couldn't resolve `{}`: {}", app_file, e);
                return None;
            }
        };
        if let Err(e) = jet_devserver_validate_source(&abs_file) {
            eprintln!("error: source file is not safe to compile: {}", e);
            return None;
        }
        let out_dir = match jet_devserver_build_dir() {
            Ok(path) => path,
            Err(e) => {
                eprintln!("error: web output directory is not safe: {}", e);
                return None;
            }
        };
        let output_root = match held_output_root {
            Some(root) => match root.clone_directory() {
                Ok(root) => root,
                Err(e) => {
                    eprintln!("error: couldn't clone the held web output directory: {}", e);
                    return None;
                }
            },
            None => match jet_devserver_open_output_root(&out_dir) {
                Ok(root) => root,
                Err(e) => {
                    eprintln!("error: web output directory is not safe: {}", e);
                    return None;
                }
            },
        };
        let staging_name = format!(
            ".jet-devserver-staging-{}-{}",
            std::process::id(),
            JET_DEVSERVER_STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let staging_root = out_dir.join(&staging_name);
        let source_stem = jet_devserver_source_stem(&abs_file);
        let staging = match jet_devserver_create_staging(&output_root, &staging_root, &source_stem) {
            Ok(staging) => staging,
            Err(e) => {
                eprintln!("error: couldn't create a staging folder for the rebuild: {}", e);
                return None;
            }
        };

        // Prefer the exact `jet` binary that launched this program (passed
        // in as JET_BIN by `jet dev`'s `run_dev_entry`) over a bare PATH
        // lookup: PATH could resolve to a different jet version, or to a
        // cwd-sensitive wrapper that breaks under the staging cwd below.
        let jet_bin = std::env::var("JET_BIN").unwrap_or_else(|_| "jet".to_string());
        let mut command = Command::new(&jet_bin);
        command
            .args(["build", "--target=web"])
            .arg(&abs_file);
        let command_guard = match jet_devserver_prepare_build_command(&mut command, &staging) {
            Ok(guard) => guard,
            Err(e) => {
                eprintln!("error: couldn't hold the build staging directory: {}", e);
                let _ = jet_devserver_cleanup_staging(&output_root, &staging);
                return None;
            }
        };
        let out = jet_devserver_command_output_bounded(
            &mut command,
            JET_DEVSERVER_MAX_CHILD_OUTPUT_BYTES,
        );
        drop(command_guard);
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                eprintln!("error: couldn't run `{} build --target=web`: {}", jet_bin, e);
                eprintln!(" fix: make sure `jet` is on PATH, or run this program via `jet dev` (which passes its own path in JET_BIN)");
                let _ = jet_devserver_cleanup_staging(&output_root, &staging);
                return None;
            }
        };
        if !out.status.success() {
            if is_rebuild {
                eprintln!("\n— {} changed —", app_file);
            }
            // `jet build` is the real front end — it already rendered its
            // own diagnostics (I2: an ICE banner, or a normal front-end
            // error). Relay them verbatim rather than re-interpreting them
            // here.
            let _ = std::io::stderr().write_all(&out.stdout);
            let _ = std::io::stderr().write_all(&out.stderr);
            let _ = jet_devserver_cleanup_staging(&output_root, &staging);
            return None;
        }

        let mut html_bytes = None;
        if let Some(html_path) = html_override {
            let source_dir = Path::new(app_file).parent().unwrap_or(Path::new("."));
            match jet_devserver_read_inside(source_dir, html_path) {
                Ok(bytes) => {
                    html_bytes = Some(bytes);
                }
                Err(e) => {
                    eprintln!(
                        "error: `.html(\"{}\")` names a file that doesn't exist: {}",
                        html_path,
                        e
                    );
                    let _ = jet_devserver_cleanup_staging(&output_root, &staging);
                    return None;
                }
            }
        }

        if let Err(e) = jet_devserver_publish_build(&output_root, &staging, html_bytes.as_deref()) {
            eprintln!("error: couldn't finalize web build: {}", e);
            let _ = jet_devserver_cleanup_staging(&output_root, &staging);
            return None;
        }
        let _ = jet_devserver_cleanup_staging(&output_root, &staging);

        if is_rebuild {
            eprintln!("[dev] rebuilt after change to {}", app_file);
        }
        Some(output_root)
    }

    fn jet_devserver_command_output_bounded(
        command: &mut Command,
        limit: usize,
    ) -> std::io::Result<Output> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "build stdout pipe was not available")
        });
        let stderr = child.stderr.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "build stderr pipe was not available")
        });
        let (stdout, stderr) = match (stdout, stderr) {
            (Ok(stdout), Ok(stderr)) => (stdout, stderr),
            (stdout, stderr) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(stdout
                    .err()
                    .or_else(|| stderr.err())
                    .expect("missing build pipe error"));
            }
        };
        let budget = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let read_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stdout_join = jet_devserver_spawn_output_reader(
            stdout,
            limit,
            Arc::clone(&budget),
            Arc::clone(&exceeded),
            Arc::clone(&read_failed),
        );
        let stderr_join = jet_devserver_spawn_output_reader(
            stderr,
            limit,
            budget,
            Arc::clone(&exceeded),
            Arc::clone(&read_failed),
        );
        let status = loop {
            if exceeded.load(Ordering::Acquire) || read_failed.load(Ordering::Acquire) {
                let _ = child.kill();
                break child.wait()?;
            }
            match child.try_wait()? {
                Some(status) => break status,
                None => thread::sleep(Duration::from_millis(1)),
            }
        };
        let stdout = stdout_join
            .join()
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "build stdout reader panicked")
            })??;
        let stderr = stderr_join
            .join()
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "build stderr reader panicked")
            })??;
        if exceeded.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "build child output exceeds the aggregate output budget",
            ));
        }
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    fn jet_devserver_spawn_output_reader<R>(
        mut reader: R,
        limit: usize,
        budget: Arc<AtomicUsize>,
        exceeded: Arc<std::sync::atomic::AtomicBool>,
        read_failed: Arc<std::sync::atomic::AtomicBool>,
    ) -> thread::JoinHandle<std::io::Result<Vec<u8>>>
    where
        R: Read + Send + 'static,
    {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let count = match reader.read(&mut chunk) {
                    Ok(count) => count,
                    Err(error) => {
                        read_failed.store(true, Ordering::Release);
                        return Err(error);
                    }
                };
                if count == 0 {
                    return Ok(bytes);
                }
                let mut used = budget.load(Ordering::Acquire);
                let kept = loop {
                    let available = limit.saturating_sub(used);
                    let kept = available.min(count);
                    let next = used.saturating_add(kept);
                    match budget.compare_exchange(
                        used,
                        next,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break kept,
                        Err(next) => used = next,
                    }
                };
                if kept < count {
                    exceeded.store(true, Ordering::Release);
                    return Ok(bytes);
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
        })
    }

    fn jet_devserver_source_stem(path: &Path) -> String {
        path.file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "out".to_string())
            .replace('.', "_")
    }

    fn jet_devserver_build_dir() -> Result<PathBuf, String> {
        let cwd = std::fs::canonicalize(".").map_err(|e| e.to_string())?;
        let build = PathBuf::from("build");
        jet_devserver_ensure_directory(&build).map_err(|e| e.to_string())?;
        let real = std::fs::canonicalize(&build).map_err(|e| e.to_string())?;
        if !real.starts_with(&cwd) {
            return Err("build directory escapes the working directory".to_string());
        }
        Ok(real)
    }

    fn jet_devserver_ensure_directory(path: &Path) -> std::io::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "directory must not be a symlink",
            )),
            Ok(metadata) if metadata.is_dir() => Ok(()),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "path is not a directory",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    jet_devserver_ensure_directory(parent)?;
                }
                std::fs::create_dir(path)
            }
            Err(error) => Err(error),
        }
    }

    const JET_DEVSERVER_OUTPUT_FILES: [&str; 6] = [
        "web.manifest.json",
        "jet_dom_runtime.js",
        "app.js",
        "app_wasm.rs",
        "app.wasm",
        "index.html",
    ];
    const JET_DEVSERVER_STAGING_FILES: [&str; 9] = [
        "web.manifest.json",
        "jet_dom_runtime.js",
        "app.js",
        "app.js.map",
        "app_wasm.rs",
        "app.wasm",
        "app.wasm.map",
        "index.html",
        "jet-timing-backend.json",
    ];

    #[cfg(unix)]
    mod jet_devserver_unix_output {
        use super::*;
        use std::ffi::{CString, OsStr};
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

        const O_RDONLY: i32 = 0;
        const O_WRONLY: i32 = 1;
        const O_CREAT: i32 = 0o100;
        const O_EXCL: i32 = 0o200;
        const O_CLOEXEC: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
            0o2000000
        } else {
            0x01000000
        };
        const O_DIRECTORY: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
            0o200000
        } else {
            0x00100000
        };
        const O_NOFOLLOW: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
            0o400000
        } else {
            0x0100
        };
        const O_NONBLOCK: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
            0o4000
        } else {
            0x0004
        };
        const AT_REMOVEDIR: i32 = 0x200;

        unsafe extern "C" {
            fn mkdirat(directory: i32, path: *const i8, mode: u32) -> i32;
            fn openat(directory: i32, path: *const i8, flags: i32, mode: u32) -> i32;
            fn renameat(
                old_directory: i32,
                old_path: *const i8,
                new_directory: i32,
                new_path: *const i8,
            ) -> i32;
            fn unlinkat(directory: i32, path: *const i8, flags: i32) -> i32;
        }

        fn name(value: &OsStr) -> std::io::Result<CString> {
            CString::new(value.as_bytes()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "devserver path contains NUL",
                )
            })
        }

        fn check_regular(file: &File) -> std::io::Result<(u64, u64)> {
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver file is linked or not regular",
                ));
            }
            Ok((metadata.dev(), metadata.ino()))
        }

        pub(super) fn check_opened_file(file: &File) -> std::io::Result<()> {
            check_regular(file).map(|_| ())
        }

        pub(super) fn validate_source(path: &Path) -> std::io::Result<()> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
                .open(path)?;
            check_regular(&file).map(|_| ())
        }

        pub(super) fn open_root(path: &Path) -> std::io::Result<JetDevServerOutputRoot> {
            let expected = std::fs::metadata(path)?;
            let directory = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
                .open(path)?;
            let actual = directory.metadata()?;
            if !expected.is_dir()
                || actual.dev() != expected.dev()
                || actual.ino() != expected.ino()
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver output root changed during secure open",
                ));
            }
            Ok(JetDevServerOutputRoot::new(directory))
        }

        pub(super) fn open_dir_at(parent: &File, value: &OsStr) -> std::io::Result<File> {
            let value = name(value)?;
            let fd = unsafe {
                openat(
                    parent.as_raw_fd(),
                    value.as_ptr(),
                    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                    0,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(unsafe { File::from_raw_fd(fd) })
        }

        fn open_file_at(parent: &File, value: &OsStr, write: bool) -> std::io::Result<File> {
            let value = name(value)?;
            let flags = if write { O_WRONLY } else { O_RDONLY };
            let fd = unsafe {
                openat(
                    parent.as_raw_fd(),
                    value.as_ptr(),
                    flags | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK,
                    0,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(unsafe { File::from_raw_fd(fd) })
        }

        fn unlink_at(parent: &File, value: &OsStr, flags: i32) -> std::io::Result<()> {
            let value = name(value)?;
            if unsafe { unlinkat(parent.as_raw_fd(), value.as_ptr(), flags) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        fn check_existing(parent: &File, value: &OsStr) -> std::io::Result<bool> {
            match open_file_at(parent, value, false) {
                Ok(file) => {
                    check_regular(&file)?;
                    Ok(true)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error),
            }
        }

        fn rename_at(
            old_parent: &File,
            old_name: &OsStr,
            new_parent: &File,
            new_name: &OsStr,
        ) -> std::io::Result<()> {
            let old_name = name(old_name)?;
            let new_name = name(new_name)?;
            if unsafe {
                renameat(
                    old_parent.as_raw_fd(),
                    old_name.as_ptr(),
                    new_parent.as_raw_fd(),
                    new_name.as_ptr(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        fn create_publication_journal(parent: &File) -> std::io::Result<(String, File)> {
            for _ in 0..100 {
                let value = format!(
                    ".jet-devserver-publication-{}-{}",
                    std::process::id(),
                    JET_DEVSERVER_STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
                );
                let name = name(OsStr::new(&value))?;
                if unsafe { mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0 {
                    return match open_dir_at(parent, OsStr::new(&value)) {
                        Ok(directory) => Ok((value, directory)),
                        Err(error) => {
                            let _ = unlink_at(parent, OsStr::new(&value), AT_REMOVEDIR);
                            Err(error)
                        }
                    };
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "could not allocate a devserver publication journal",
            ))
        }

        fn clear_publication_journal(
            parent: &File,
            journal: &File,
            journal_name: &str,
            names: &[&str],
        ) {
            for value in names {
                let _ = unlink_at(journal, OsStr::new(value), 0);
            }
            let _ = unlink_at(parent, OsStr::new(journal_name), AT_REMOVEDIR);
        }

        fn rollback_publication(
            root: &File,
            journal: &File,
            backed_up: &[&str],
            published: &[&str],
        ) {
            for value in published.iter().rev() {
                let _ = unlink_at(root, OsStr::new(value), 0);
            }
            for value in backed_up.iter().rev() {
                let _ = rename_at(
                    journal,
                    OsStr::new(value),
                    root,
                    OsStr::new(value),
                );
            }
        }

        fn replace_file_at(parent: &File, value: &OsStr, bytes: &[u8]) -> std::io::Result<()> {
            check_existing(parent, value)?;
            let final_name = name(value)?;
            let temporary = format!(
                ".jet-devserver-output-{}-{}",
                std::process::id(),
                JET_DEVSERVER_STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let temporary_name = name(OsStr::new(&temporary))?;
            let fd = unsafe {
                openat(
                    parent.as_raw_fd(),
                    temporary_name.as_ptr(),
                    O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut temporary_file = unsafe { File::from_raw_fd(fd) };
            if let Err(error) = (|| {
                temporary_file.write_all(bytes)?;
                temporary_file.sync_all()?;
                let temporary_id = check_regular(&temporary_file)?;
                if unsafe {
                    renameat(
                        parent.as_raw_fd(),
                        temporary_name.as_ptr(),
                        parent.as_raw_fd(),
                        final_name.as_ptr(),
                    )
                } != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                let published = open_file_at(parent, value, false)?;
                if check_regular(&published)? != temporary_id {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "published devserver output identity changed",
                    ));
                }
                Ok::<(), std::io::Error>(())
            })() {
                let _ = unlink_at(parent, OsStr::new(&temporary), 0);
                return Err(error);
            }
            Ok(())
        }

        pub(super) fn create_staging(
            root: &JetDevServerOutputRoot,
            path: &Path,
            source_stem: &str,
        ) -> std::io::Result<JetDevServerStaging> {
            let name_value = path.file_name().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "staging has no name")
            })?;
            let name = name(name_value)?;
            if unsafe { mkdirat(root.directory.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let directory = open_dir_at(&root.directory, name_value)?;
            Ok(JetDevServerStaging {
                path: path.to_path_buf(),
                name: name_value.to_string_lossy().into_owned(),
                source_stem: source_stem.to_string(),
                directory,
            })
        }

        fn open_relative(mut current: File, relative: &Path) -> std::io::Result<File> {
            let components = relative.components().collect::<Vec<_>>();
            if components.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "empty relative path",
                ));
            }
            for component in &components[..components.len() - 1] {
                let Component::Normal(value) = component else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "devserver path is not relative",
                    ));
                };
                current = open_dir_at(&current, value)?;
            }
            let Component::Normal(value) = components.last().unwrap() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "devserver path is not relative",
                ));
            };
            let file = open_file_at(&current, value, false)?;
            check_regular(&file)?;
            Ok(file)
        }

        pub(super) fn open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
            open_relative(open_root(root)?.directory, relative)
        }

        pub(super) fn open_held_beneath(
            root: &JetDevServerOutputRoot,
            relative: &Path,
        ) -> std::io::Result<File> {
            open_relative(root.directory.try_clone()?, relative)
        }

        pub(super) fn publish_build(
            root: &JetDevServerOutputRoot,
            staging: &JetDevServerStaging,
            html: Option<&[u8]>,
        ) -> std::io::Result<()> {
            let build = open_dir_at(&staging.directory, OsStr::new("build"))?;
            if let Some(bytes) = html {
                replace_file_at(&build, OsStr::new("index.html"), bytes)?;
            }
            for value in JET_DEVSERVER_OUTPUT_FILES {
                let source = open_file_at(&build, OsStr::new(value), false)?;
                check_regular(&source)?;
                check_existing(&root.directory, OsStr::new(value))?;
            }
            let (journal_name, journal) = create_publication_journal(&root.directory)?;
            let mut backed_up = Vec::new();
            let mut published = Vec::new();
            let result = (|| {
                for value in JET_DEVSERVER_OUTPUT_FILES {
                    if check_existing(&root.directory, OsStr::new(value))? {
                        rename_at(
                            &root.directory,
                            OsStr::new(value),
                            &journal,
                            OsStr::new(value),
                        )?;
                        backed_up.push(value);
                    }
                }
                for value in JET_DEVSERVER_OUTPUT_FILES {
                    let source = open_file_at(&build, OsStr::new(value), false)?;
                    let source_id = check_regular(&source)?;
                    rename_at(
                        &build,
                        OsStr::new(value),
                        &root.directory,
                        OsStr::new(value),
                    )?;
                    published.push(value);
                    let published_file = open_file_at(&root.directory, OsStr::new(value), false)?;
                    if check_regular(&published_file)? != source_id {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "published devserver output identity changed",
                        ));
                    }
                }
                root.directory.sync_all()
            })();
            if let Err(error) = result {
                rollback_publication(&root.directory, &journal, &backed_up, &published);
                clear_publication_journal(
                    &root.directory,
                    &journal,
                    &journal_name,
                    &JET_DEVSERVER_OUTPUT_FILES,
                );
                return Err(error);
            }
            clear_publication_journal(
                &root.directory,
                &journal,
                &journal_name,
                &JET_DEVSERVER_OUTPUT_FILES,
            );
            Ok(())
        }

        pub(super) fn cleanup(
            root: &JetDevServerOutputRoot,
            staging: &JetDevServerStaging,
        ) -> std::io::Result<()> {
            if let Ok(build) = open_dir_at(&staging.directory, OsStr::new("build")) {
                for value in JET_DEVSERVER_STAGING_FILES {
                    let _ = unlink_at(&build, OsStr::new(value), 0);
                }
                let source = format!("{}.rs", staging.source_stem);
                let _ = unlink_at(&build, OsStr::new(&source), 0);
                let _ = unlink_at(&staging.directory, OsStr::new("build"), AT_REMOVEDIR);
            }
            unlink_at(&root.directory, OsStr::new(&staging.name), AT_REMOVEDIR)
        }

        pub(super) fn prepare_build_command(
            command: &mut Command,
            staging: &JetDevServerStaging,
        ) -> std::io::Result<Option<File>> {
            use std::os::unix::process::CommandExt;

            let directory = staging.directory.as_raw_fd();
            // SAFETY: `pre_exec` is the platform boundary for the child-only
            // directory change. The closure captures one live raw descriptor,
            // and calls only the async-signal-safe `fchdir` before exec.
            unsafe {
                command.pre_exec(move || {
                    unsafe extern "C" {
                        fn fchdir(directory: i32) -> i32;
                    }
                    if fchdir(directory) == 0 {
                        Ok(())
                    } else {
                        Err(std::io::Error::last_os_error())
                    }
                });
            }
            Ok(None)
        }
    }

    fn jet_devserver_read_inside(root: &Path, relative: &str) -> std::io::Result<Vec<u8>> {
        let relative = Path::new(relative);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
            || !jet_devserver_windows_components_are_safe(relative)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "HTML path must stay inside the source directory",
            ));
        }
        let file = jet_devserver_open_beneath(root, relative)?;
        jet_devserver_read_bounded(file, JET_DEVSERVER_MAX_RESPONSE_BYTES)
    }

    fn jet_devserver_serve_forever(
        listener: TcpListener,
        version: Arc<AtomicU64>,
        output_root: Arc<JetDevServerOutputRoot>,
    ) {
        let active_connections = Arc::new(AtomicUsize::new(0));
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                if !jet_devserver_try_acquire(&active_connections) {
                    drop(stream);
                    continue;
                }
                let version = Arc::clone(&version);
                let output_root = Arc::clone(&output_root);
                let active_connections = Arc::clone(&active_connections);
                thread::spawn(move || {
                    let _ = jet_devserver_handle_connection(stream, &version, &output_root);
                    active_connections.fetch_sub(1, Ordering::AcqRel);
                });
            }
        }
    }

    fn jet_devserver_try_acquire(active: &AtomicUsize) -> bool {
        let mut current = active.load(Ordering::Acquire);
        loop {
            if current >= JET_DEVSERVER_MAX_CONNECTION_THREADS {
                return false;
            }
            match active.compare_exchange(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(next) => current = next,
            }
        }
    }

    struct JetDevServerDeadlineStream {
        stream: TcpStream,
        deadline: Instant,
    }

    impl JetDevServerDeadlineStream {
        fn new(stream: TcpStream, deadline: Instant) -> Self {
            Self { stream, deadline }
        }

        fn try_clone(&self) -> std::io::Result<Self> {
            Ok(Self::new(self.stream.try_clone()?, self.deadline))
        }

        fn remaining(&self) -> std::io::Result<Duration> {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "devserver request deadline exceeded",
                ));
            }
            Ok(remaining)
        }
    }

    impl Read for JetDevServerDeadlineStream {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let remaining = self.remaining()?;
            self.stream.set_read_timeout(Some(remaining))?;
            self.stream.read(buffer)
        }
    }

    impl Write for JetDevServerDeadlineStream {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            let remaining = self.remaining()?;
            self.stream.set_write_timeout(Some(remaining))?;
            self.stream.write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let remaining = self.remaining()?;
            self.stream.set_write_timeout(Some(remaining))?;
            self.stream.flush()
        }
    }

    fn jet_devserver_read_line_bounded(
        reader: &mut impl BufRead,
        line: &mut String,
        limit: usize,
    ) -> std::io::Result<usize> {
        let read = reader
            .take(limit.saturating_add(1) as u64)
            .read_line(line)?;
        if read > limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "devserver request line exceeds 8 KiB",
            ));
        }
        Ok(read)
    }

    fn jet_devserver_handle_connection(
        stream: TcpStream,
        version: &AtomicU64,
        output_root: &JetDevServerOutputRoot,
    ) -> std::io::Result<()> {
        let deadline = Instant::now() + JET_DEVSERVER_REQUEST_DEADLINE;
        let mut stream = JetDevServerDeadlineStream::new(stream, deadline);
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        if jet_devserver_read_line_bounded(
            &mut reader,
            &mut request_line,
            JET_DEVSERVER_MAX_LINE_BYTES,
        )? == 0
        {
            return Ok(());
        }
        let mut header_bytes = 0usize;
        let mut header_count = 0usize;
        let mut content_length = None;
        let mut saw_authorization = false;
        let mut saw_content_length = false;
        let mut saw_host = false;
        let mut saw_origin = false;
        let mut saw_transfer_encoding = false;
        loop {
            let mut line = String::new();
            let read = jet_devserver_read_line_bounded(
                &mut reader,
                &mut line,
                JET_DEVSERVER_MAX_LINE_BYTES,
            )?;
            if read == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            header_count = header_count.checked_add(1).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "too many devserver headers",
                )
            })?;
            header_bytes = header_bytes.checked_add(read).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver headers exceed the request budget",
                )
            })?;
            if header_count > JET_DEVSERVER_MAX_HEADER_COUNT
                || header_bytes > JET_DEVSERVER_MAX_HEADER_BYTES
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver headers exceed the request budget",
                ));
            }
            let Some((name, value)) = line
                .trim_end_matches(['\r', '\n'])
                .split_once(':')
            else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed devserver header",
                ));
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name.is_empty()
                || name
                    .bytes()
                    .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed devserver header name",
                ));
            }
            let duplicate = match name.as_str() {
                "authorization" => {
                    let duplicate = saw_authorization;
                    saw_authorization = true;
                    duplicate
                }
                "content-length" => {
                    let duplicate = saw_content_length;
                    saw_content_length = true;
                    duplicate
                }
                "host" => {
                    let duplicate = saw_host;
                    saw_host = true;
                    duplicate
                }
                "origin" => {
                    let duplicate = saw_origin;
                    saw_origin = true;
                    duplicate
                }
                "transfer-encoding" => {
                    let duplicate = saw_transfer_encoding;
                    saw_transfer_encoding = true;
                    duplicate
                }
                _ => false,
            };
            if duplicate {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "duplicate devserver security header",
                ));
            }
            if name == "transfer-encoding" {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver does not accept transfer-encoded requests",
                ));
            }
            if name == "content-length" {
                content_length = Some(value.parse::<usize>().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid devserver content length",
                    )
                })?);
            }
        }

        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("");
        let request_version = parts.next().unwrap_or("");
        if method.is_empty()
            || target.is_empty()
            || request_version != "HTTP/1.1"
            || parts.next().is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed devserver request line",
            ));
        }
        let content_length = content_length.unwrap_or(0);
        if content_length > JET_DEVSERVER_MAX_REQUEST_BODY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "devserver request body exceeds 1 MiB",
            ));
        }
        if content_length != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "devserver GET requests must not contain a body",
            ));
        }

        if method != "GET" {
            return jet_devserver_write_response(
                &mut stream,
                "405 Method Not Allowed",
                "text/plain; charset=utf-8",
                b"jet dev's web server only handles GET",
            );
        }

        let path = target.split('?').next().unwrap_or("/");
        if path == "/__jet_dev_version" {
            let body = version.load(Ordering::SeqCst).to_string();
            return jet_devserver_write_response(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                body.as_bytes(),
            );
        }
        jet_devserver_serve_static(&mut stream, path, output_root)
    }

    fn jet_devserver_serve_static(
        stream: &mut impl Write,
        path: &str,
        output_root: &JetDevServerOutputRoot,
    ) -> std::io::Result<()> {
        let Some(rel) = jet_devserver_static_relative_path(path) else {
            return jet_devserver_write_response(
                stream,
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"bad path",
            );
        };
        let file_path = Path::new(rel);
        let is_index = file_path.file_name().and_then(|f| f.to_str()) == Some("index.html");
        let content_type = jet_devserver_content_type_for(&file_path);
        let script = is_index.then(jet_devserver_live_reload_script);
        let read_limit = script.as_ref().map_or(
            JET_DEVSERVER_MAX_RESPONSE_BYTES,
            |script| {
                JET_DEVSERVER_MAX_RESPONSE_BYTES
                    .checked_sub(script.len())
                    .unwrap_or(0)
            },
        );
        let bytes = {
            let _publication = output_root.publication_lock.lock().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "devserver publication lock poisoned",
                )
            })?;
            let file = match jet_devserver_open_held_beneath(output_root, Path::new(rel)) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let body = format!("not found: {}", path);
                    return jet_devserver_write_response(
                        stream,
                        "404 Not Found",
                        "text/plain; charset=utf-8",
                        body.as_bytes(),
                    );
                }
                Err(_) => {
                    return jet_devserver_write_response(
                        stream,
                        "400 Bad Request",
                        "text/plain; charset=utf-8",
                        b"bad path",
                    );
                }
            };
            match jet_devserver_read_bounded(file, read_limit) {
                Ok(bytes) => bytes,
                Err(_) => {
                    let body = format!("not found: {}", path);
                    return jet_devserver_write_response(
                        stream,
                        "404 Not Found",
                        "text/plain; charset=utf-8",
                        body.as_bytes(),
                    );
                }
            }
        };

        if let Some(script) = script {
            if std::str::from_utf8(&bytes).is_err() {
                let body = format!("not found: {}", path);
                return jet_devserver_write_response(
                    stream,
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    body.as_bytes(),
                );
            }
            if let Some(index) = jet_devserver_find_body_end(&bytes) {
                let parts = [&bytes[..index], script.as_bytes(), &bytes[index..]];
                return jet_devserver_write_response_parts(
                    stream,
                    "200 OK",
                    content_type,
                    &parts,
                );
            }
            let parts = [&bytes[..], script.as_bytes()];
            return jet_devserver_write_response_parts(stream, "200 OK", content_type, &parts);
        }
        jet_devserver_write_response(stream, "200 OK", content_type, &bytes)
    }

    fn jet_devserver_find_body_end(bytes: &[u8]) -> Option<usize> {
        const MARKER: &[u8] = b"</body>";
        bytes.windows(MARKER.len()).position(|window| {
            window.iter().zip(MARKER).all(|(actual, expected)| {
                actual.to_ascii_lowercase() == *expected
            })
        })
    }

    fn jet_devserver_read_bounded(
        file: File,
        limit: usize,
    ) -> std::io::Result<Vec<u8>> {
        jet_devserver_check_opened_file(&file)?;
        let metadata = file.metadata()?;
        let length_bytes = metadata.len();
        if length_bytes > limit as u64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("devserver response exceeds {limit} bytes"),
            ));
        }
        let length = length_bytes as usize;
        let mut bytes = Vec::with_capacity(length);
        let mut reader = file.take(length as u64);
        reader.read_to_end(&mut bytes)?;
        let final_length = reader.get_ref().metadata()?.len();
        if final_length != length_bytes || bytes.len() != length {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "devserver file changed while it was being read",
            ));
        }
        jet_devserver_check_opened_file(reader.get_ref())?;
        Ok(bytes)
    }

    fn jet_devserver_validate_relative(relative: &Path) -> std::io::Result<()> {
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || !jet_devserver_windows_components_are_safe(relative)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "devserver path is not relative",
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    mod jet_devserver_windows_output {
        use super::*;
        use std::ffi::{c_void, OsStr};
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;

        type Handle = *mut c_void;

        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const DELETE: u32 = 0x0001_0000;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_ALL: u32 = 0x0000_0007;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        const FILE_ATTRIBUTE_TAG_INFO_CLASS: i32 = 9;
        const FILE_RENAME_INFO_CLASS: i32 = 3;
        const FILE_DISPOSITION_INFO_CLASS: i32 = 4;

        #[repr(C)]
        struct ByHandleFileInformation {
            attributes: u32,
            creation_low: u32,
            creation_high: u32,
            access_low: u32,
            access_high: u32,
            write_low: u32,
            write_high: u32,
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }

        #[repr(C)]
        struct FileAttributeTagInfo {
            attributes: u32,
            reparse_tag: u32,
        }

        #[repr(C)]
        struct FileRenameInfo {
            replace_if_exists: i32,
            root_directory: Handle,
            file_name_length: u32,
            file_name: [u16; 1],
        }

        #[repr(C)]
        struct FileDispositionInfo {
            delete_file: u8,
        }

        unsafe extern "system" {
            fn GetFileAttributesW(name: *const u16) -> u32;
            fn GetFileInformationByHandle(
                file: Handle,
                info: *mut ByHandleFileInformation,
            ) -> i32;
            fn GetFileInformationByHandleEx(
                file: Handle,
                class: i32,
                info: *mut c_void,
                size: u32,
            ) -> i32;
            fn GetFinalPathNameByHandleW(
                file: Handle,
                path: *mut u16,
                path_len: u32,
                flags: u32,
            ) -> u32;
            fn SetFileInformationByHandle(
                file: Handle,
                class: i32,
                info: *mut c_void,
                size: u32,
            ) -> i32;
            fn FlushFileBuffers(file: Handle) -> i32;
        }

        fn wide(value: &OsStr) -> Vec<u16> {
            value.encode_wide().chain(std::iter::once(0)).collect()
        }

        fn normalize(path: String) -> String {
            let path = path.replace('/', "\\");
            let path = if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{path}")
            } else {
                path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
            };
            path.trim_end_matches(['\\', '/']).to_ascii_lowercase()
        }

        fn final_path(file: &File) -> std::io::Result<String> {
            let needed = unsafe {
                GetFinalPathNameByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, 0)
            };
            if needed == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut buffer = vec![0u16; needed as usize + 1];
            let written = unsafe {
                GetFinalPathNameByHandleW(
                    file.as_raw_handle(),
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    0,
                )
            };
            if written == 0 || written as usize >= buffer.len() {
                return Err(std::io::Error::last_os_error());
            }
            buffer.truncate(written as usize);
            Ok(String::from_utf16_lossy(&buffer))
        }

        fn file_id(file: &File) -> std::io::Result<(u64, u64, u32)> {
            let mut info = ByHandleFileInformation {
                attributes: 0,
                creation_low: 0,
                creation_high: 0,
                access_low: 0,
                access_high: 0,
                write_low: 0,
                write_high: 0,
                volume: 0,
                size_high: 0,
                size_low: 0,
                links: 0,
                index_high: 0,
                index_low: 0,
            };
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok((
                info.volume as u64,
                ((info.index_high as u64) << 32) | info.index_low as u64,
                info.links,
            ))
        }

        fn check_reparse_handle(file: &File) -> std::io::Result<()> {
            let mut info = FileAttributeTagInfo {
                attributes: 0,
                reparse_tag: 0,
            };
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FILE_ATTRIBUTE_TAG_INFO_CLASS,
                    (&mut info as *mut FileAttributeTagInfo).cast(),
                    std::mem::size_of::<FileAttributeTagInfo>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver path contains a Windows reparse point",
                ));
            }
            Ok(())
        }

        fn check_regular(file: &File) -> std::io::Result<(u64, u64)> {
            check_reparse_handle(file)?;
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver object is not a regular file",
                ));
            }
            let (volume, index, links) = file_id(file)?;
            if links != 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver file has multiple hard links",
                ));
            }
            Ok((volume, index))
        }

        pub(super) fn check_opened_file(file: &File) -> std::io::Result<()> {
            check_regular(file).map(|_| ())
        }

        pub(super) fn validate_source(path: &Path) -> std::io::Result<()> {
            reject_reparse_components(path)?;
            let file = open_existing(path, GENERIC_READ, false, false, false)?;
            check_regular(&file).map(|_| ())
        }

        fn check_directory(file: &File) -> std::io::Result<()> {
            check_reparse_handle(file)?;
            if !file.metadata()?.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver object is not a real directory",
                ));
            }
            Ok(())
        }

        fn is_reparse(path: &Path) -> std::io::Result<bool> {
            let value = wide(path.as_os_str());
            let attributes = unsafe { GetFileAttributesW(value.as_ptr()) };
            if attributes == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            Ok(attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        }

        fn reject_reparse_components(path: &Path) -> std::io::Result<()> {
            let normalized = path
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase();
            if normalized.starts_with("\\\\.\\")
                || normalized.starts_with("\\\\?\\")
                || normalized.starts_with("\\??\\")
                || normalized.starts_with("\\device\\")
                || normalized.starts_with("\\globalroot\\")
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver path contains a Windows device namespace",
                ));
            }
            let mut current = PathBuf::new();
            for component in path.components() {
                match component {
                    Component::Prefix(_) | Component::RootDir => {
                        current.push(component.as_os_str());
                    }
                    Component::CurDir => {}
                    Component::Normal(value) => {
                        if !super::jet_devserver_windows_component_is_safe(value) {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::PermissionDenied,
                                "devserver path contains an ADS, reserved, or invalid Windows name",
                            ));
                        }
                        current.push(value);
                        if is_reparse(&current)? {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::PermissionDenied,
                                "devserver path contains a Windows reparse point",
                            ));
                        }
                    }
                    Component::ParentDir => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "devserver path contains parent traversal",
                        ));
                    }
                }
            }
            Ok(())
        }

        fn open_existing_with_share(
            path: &Path,
            access: u32,
            write: bool,
            create_new: bool,
            directory: bool,
            share: u32,
        ) -> std::io::Result<File> {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            if write {
                options.write(true);
            }
            if create_new {
                options.create_new(true);
            }
            options
                .access_mode(access)
                .share_mode(share)
                .custom_flags(
                    FILE_FLAG_OPEN_REPARSE_POINT
                        | if directory {
                            FILE_FLAG_BACKUP_SEMANTICS
                        } else {
                            0
                        },
                )
                .open(path)
        }

        fn open_existing(
            path: &Path,
            access: u32,
            write: bool,
            create_new: bool,
            directory: bool,
        ) -> std::io::Result<File> {
            open_existing_with_share(
                path,
                access,
                write,
                create_new,
                directory,
                FILE_SHARE_ALL,
            )
        }

        fn open_root(path: &Path) -> std::io::Result<JetDevServerOutputRoot> {
            reject_reparse_components(path)?;
            let expected_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };
            let expected = normalize(expected_path.to_string_lossy().into_owned());
            let directory = open_existing_with_share(
                path,
                GENERIC_READ | GENERIC_WRITE,
                false,
                false,
                true,
                FILE_SHARE_ALL,
            )?;
            check_directory(&directory)?;
            if normalize(final_path(&directory)?) != expected {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver output root changed during secure open",
                ));
            }
            Ok(JetDevServerOutputRoot::new(directory))
        }

        fn is_child(parent: &str, candidate: &str) -> bool {
            candidate
                .strip_prefix(parent.trim_end_matches(['\\', '/']))
                .is_some_and(|tail| tail.starts_with('\\'))
        }

        fn parent_of(path: &str) -> Option<&str> {
            path.rsplit_once('\\').map(|(parent, _)| parent)
        }

        fn same_object(left: &File, right: &File) -> std::io::Result<bool> {
            let (left_volume, left_index, _) = file_id(left)?;
            let (right_volume, right_index, _) = file_id(right)?;
            Ok(left_volume == right_volume && left_index == right_index)
        }

        fn verify_parent(
            parent_path: &Path,
            parent_final: &str,
            parent: &File,
        ) -> std::io::Result<()> {
            if normalize(final_path(parent)?) != parent_final {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver parent moved while it was held",
                ));
            }
            reject_reparse_components(parent_path)?;
            let opened = open_existing(parent_path, GENERIC_READ, false, false, true)?;
            check_directory(&opened)?;
            if normalize(final_path(&opened)?) != parent_final || !same_object(parent, &opened)? {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver parent identity changed",
                ));
            }
            Ok(())
        }

        fn open_dir_in_parent(
            parent_path: &Path,
            parent_final: &str,
            value: &OsStr,
            delete: bool,
            parent: &File,
        ) -> std::io::Result<(File, String)> {
            let path = parent_path.join(value);
            reject_reparse_components(&path)?;
            let access = GENERIC_READ | if delete { DELETE } else { 0 };
            let directory = open_existing(&path, access, false, false, true)?;
            check_directory(&directory)?;
            let actual = normalize(final_path(&directory)?);
            verify_parent(parent_path, parent_final, parent)?;
            if parent_of(&actual) != Some(parent_final) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver directory escaped its held parent",
                ));
            }
            Ok((directory, actual))
        }

        fn open_file_in_parent(
            parent_path: &Path,
            parent_final: &str,
            value: &OsStr,
            delete: bool,
            parent: &File,
        ) -> std::io::Result<(File, (u64, u64))> {
            let path = parent_path.join(value);
            reject_reparse_components(&path)?;
            let access = GENERIC_READ | if delete { DELETE } else { 0 };
            let file = open_existing(&path, access, false, false, false)?;
            let id = check_regular(&file)?;
            let actual = normalize(final_path(&file)?);
            verify_parent(parent_path, parent_final, parent)?;
            if parent_of(&actual) != Some(parent_final) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver file escaped its held parent",
                ));
            }
            Ok((file, id))
        }

        fn create_publication_journal(
            root_path: &Path,
            root_final: &str,
            root: &File,
        ) -> std::io::Result<(PathBuf, String, File, String)> {
            for _ in 0..100 {
                let value = format!(
                    ".jet-devserver-publication-{}-{}",
                    std::process::id(),
                    JET_DEVSERVER_STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
                );
                let path = root_path.join(&value);
                match std::fs::create_dir(&path) {
                    Ok(()) => {
                        reject_reparse_components(&path)?;
                        match open_dir_in_parent(
                            root_path,
                            root_final,
                            OsStr::new(&value),
                            true,
                            root,
                        ) {
                            Ok((directory, actual)) => {
                                return Ok((path, value, directory, actual));
                            }
                            Err(error) => {
                                let _ = std::fs::remove_dir(&path);
                                return Err(error);
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "could not allocate a devserver publication journal",
            ))
        }

        fn clear_publication_journal(
            path: &Path,
            final_path: &str,
            directory: &File,
            names: &[&str],
        ) {
            for value in names {
                if let Ok((file, _)) =
                    open_file_in_parent(path, final_path, OsStr::new(value), true, directory)
                {
                    let _ = delete_handle(&file);
                }
            }
            let _ = delete_handle(directory);
        }

        fn rollback_publication(
            root_path: &Path,
            root_final: &str,
            root: &File,
            journal_path: &Path,
            journal_final: &str,
            journal: &File,
            backed_up: &[&str],
            published: &[&str],
        ) {
            for value in published.iter().rev() {
                if let Ok((file, _)) =
                    open_file_in_parent(root_path, root_final, OsStr::new(value), true, root)
                {
                    let _ = delete_handle(&file);
                }
            }
            for value in backed_up.iter().rev() {
                if let Ok((file, _)) = open_file_in_parent(
                    journal_path,
                    journal_final,
                    OsStr::new(value),
                    true,
                    journal,
                ) {
                    let _ = rename_into(&file, root, OsStr::new(value));
                }
            }
        }

        fn rename_into(source: &File, parent: &File, value: &OsStr) -> std::io::Result<()> {
            let file_name = value.encode_wide().collect::<Vec<_>>();
            let offset = std::mem::offset_of!(FileRenameInfo, file_name);
            let size = offset + file_name.len() * std::mem::size_of::<u16>();
            let mut storage = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let info = storage.as_mut_ptr().cast::<FileRenameInfo>();
            unsafe {
                (*info).replace_if_exists = 1;
                (*info).root_directory = parent.as_raw_handle();
                (*info).file_name_length = (file_name.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    file_name.as_ptr(),
                    (*info).file_name.as_mut_ptr(),
                    file_name.len(),
                );
                if SetFileInformationByHandle(
                    source.as_raw_handle(),
                    FILE_RENAME_INFO_CLASS,
                    storage.as_mut_ptr().cast(),
                    size as u32,
                ) == 0
                {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        }

        fn delete_handle(file: &File) -> std::io::Result<()> {
            let mut info = FileDispositionInfo { delete_file: 1 };
            if unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FILE_DISPOSITION_INFO_CLASS,
                    (&mut info as *mut FileDispositionInfo).cast(),
                    std::mem::size_of::<FileDispositionInfo>() as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        fn flush(file: &File) -> std::io::Result<()> {
            if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }

        fn replace_file_in_parent(
            parent_path: &Path,
            parent_final: &str,
            parent: &File,
            value: &OsStr,
            bytes: &[u8],
        ) -> std::io::Result<()> {
            verify_parent(parent_path, parent_final, parent)?;
            match open_file_in_parent(parent_path, parent_final, value, false, parent) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let temporary_name = format!(
                ".jet-devserver-output-{}-{}",
                std::process::id(),
                JET_DEVSERVER_STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let temporary_path = parent_path.join(&temporary_name);
            let mut temporary = open_existing(
                &temporary_path,
                GENERIC_READ | GENERIC_WRITE | DELETE,
                true,
                true,
                false,
            )?;
            if let Err(error) = verify_parent(parent_path, parent_final, parent) {
                let _ = delete_handle(&temporary);
                return Err(error);
            }
            let temporary_id = match check_regular(&temporary) {
                Ok(id) => id,
                Err(error) => {
                    let _ = delete_handle(&temporary);
                    return Err(error);
                }
            };
            if let Err(error) = (|| {
                temporary.write_all(bytes)?;
                temporary.sync_all()?;
                if check_regular(&temporary)? != temporary_id {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "temporary devserver output identity changed",
                    ));
                }
                rename_into(&temporary, parent, value)?;
                let (published, published_id) =
                    open_file_in_parent(parent_path, parent_final, value, true, parent)?;
                if published_id != temporary_id {
                    let _ = delete_handle(&published);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "published devserver output identity changed",
                    ));
                }
                Ok(())
            })() {
                let _ = delete_handle(&temporary);
                return Err(error);
            }
            Ok(())
        }

        pub(super) fn create_staging(
            root: &JetDevServerOutputRoot,
            path: &Path,
            source_stem: &str,
        ) -> std::io::Result<JetDevServerStaging> {
            let name = path.file_name().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "staging has no name")
            })?;
            let root_path = PathBuf::from(final_path(&root.directory)?);
            let staging_path = root_path.join(name);
            let root_final = normalize(final_path(&root.directory)?);
            verify_parent(&root_path, &root_final, &root.directory)?;
            std::fs::create_dir(&staging_path)?;
            reject_reparse_components(&staging_path)?;
            let (directory, actual) = open_dir_in_parent(
                &root_path,
                &root_final,
                name,
                false,
                &root.directory,
            )?;
            if !is_child(&root_final, &actual) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver staging escaped its output root",
                ));
            }
            Ok(JetDevServerStaging {
                path: staging_path,
                name: name.to_string_lossy().into_owned(),
                source_stem: source_stem.to_string(),
                directory,
            })
        }

        fn open_relative(
            mut current: File,
            mut current_path: PathBuf,
            relative: &Path,
        ) -> std::io::Result<File> {
            let mut current_final = normalize(final_path(&current)?);
            let components = relative.components().collect::<Vec<_>>();
            if components.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "empty relative path",
                ));
            }
            for component in &components[..components.len() - 1] {
                let Component::Normal(value) = component else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "devserver path is not relative",
                    ));
                };
                let (directory, actual) = open_dir_in_parent(
                    &current_path,
                    &current_final,
                    value,
                    false,
                    &current,
                )?;
                current_path.push(value);
                current = directory;
                current_final = actual;
            }
            let Component::Normal(value) = components.last().unwrap() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "devserver path is not relative",
                ));
            };
            let (file, _) = open_file_in_parent(
                &current_path,
                &current_final,
                value,
                false,
                &current,
            )?;
            let _ = current;
            Ok(file)
        }

        pub(super) fn open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
            let root_handle = open_root(root)?;
            let current_path = root.to_path_buf();
            open_relative(root_handle.directory, current_path, relative)
        }

        pub(super) fn open_held_beneath(
            root: &JetDevServerOutputRoot,
            relative: &Path,
        ) -> std::io::Result<File> {
            let current = root.directory.try_clone()?;
            let current_path = PathBuf::from(final_path(&current)?);
            open_relative(current, current_path, relative)
        }

        pub(super) fn publish_build(
            root: &JetDevServerOutputRoot,
            staging: &JetDevServerStaging,
            html: Option<&[u8]>,
        ) -> std::io::Result<()> {
            let root_path = PathBuf::from(final_path(&root.directory)?);
            let root_final = normalize(final_path(&root.directory)?);
            let staging_final = normalize(final_path(&staging.directory)?);
            let build_path = staging.path.join("build");
            let (build, build_final) = open_dir_in_parent(
                &staging.path,
                &staging_final,
                OsStr::new("build"),
                false,
                &staging.directory,
            )?;
            if let Some(bytes) = html {
                replace_file_in_parent(
                    &build_path,
                    &build_final,
                    &build,
                    OsStr::new("index.html"),
                    bytes,
                )?;
            }
            for value in JET_DEVSERVER_OUTPUT_FILES {
                let _ = open_file_in_parent(
                    &build_path,
                    &build_final,
                    OsStr::new(value),
                    false,
                    &build,
                )?;
                match open_file_in_parent(
                    &root_path,
                    &root_final,
                    OsStr::new(value),
                    false,
                    &root.directory,
                ) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            let (journal_path, journal_name, journal, journal_final) =
                create_publication_journal(&root_path, &root_final, &root.directory)?;
            let mut backed_up = Vec::new();
            let mut published = Vec::new();
            let result = (|| {
                for value in JET_DEVSERVER_OUTPUT_FILES {
                    match open_file_in_parent(
                        &root_path,
                        &root_final,
                        OsStr::new(value),
                        true,
                        &root.directory,
                    ) {
                        Ok((old, _)) => {
                            rename_into(&old, &journal, OsStr::new(value))?;
                            backed_up.push(value);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
                for value in JET_DEVSERVER_OUTPUT_FILES {
                    let (source, source_id) = open_file_in_parent(
                        &build_path,
                        &build_final,
                        OsStr::new(value),
                        true,
                        &build,
                    )?;
                    rename_into(&source, &root.directory, OsStr::new(value))?;
                    published.push(value);
                    let (_, published_id) = open_file_in_parent(
                        &root_path,
                        &root_final,
                        OsStr::new(value),
                        true,
                        &root.directory,
                    )?;
                    if published_id != source_id {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "published devserver output identity changed",
                        ));
                    }
                }
                flush(&root.directory)
            })();
            if let Err(error) = result {
                rollback_publication(
                    &root_path,
                    &root_final,
                    &root.directory,
                    &journal_path,
                    &journal_final,
                    &journal,
                    &backed_up,
                    &published,
                );
                clear_publication_journal(
                    &journal_path,
                    &journal_final,
                    &journal,
                    &JET_DEVSERVER_OUTPUT_FILES,
                );
                return Err(error);
            }
            clear_publication_journal(
                &journal_path,
                &journal_final,
                &journal,
                &JET_DEVSERVER_OUTPUT_FILES,
            );
            Ok(())
        }

        pub(super) fn cleanup(
            root: &JetDevServerOutputRoot,
            staging: &JetDevServerStaging,
        ) -> std::io::Result<()> {
            let root_path = PathBuf::from(final_path(&root.directory)?);
            let root_final = normalize(final_path(&root.directory)?);
            let staging_final = normalize(final_path(&staging.directory)?);
            if let Ok((build, build_final)) = open_dir_in_parent(
                &staging.path,
                &staging_final,
                OsStr::new("build"),
                true,
                &staging.directory,
            )
            {
                for value in JET_DEVSERVER_STAGING_FILES {
                    if let Ok((file, _)) =
                        open_file_in_parent(
                            &staging.path.join("build"),
                            &build_final,
                            OsStr::new(value),
                            true,
                            &build,
                        )
                    {
                        let _ = delete_handle(&file);
                    }
                }
                let source = format!("{}.rs", staging.source_stem);
                if let Ok((file, _)) = open_file_in_parent(
                    &staging.path.join("build"),
                    &build_final,
                    OsStr::new(&source),
                    true,
                    &build,
                ) {
                    let _ = delete_handle(&file);
                }
                let _ = delete_handle(&build);
            }
            let (staging_path, _) = open_dir_in_parent(
                &root_path,
                &root_final,
                OsStr::new(&staging.name),
                true,
                &root.directory,
            )?;
            delete_handle(&staging_path)
        }

        pub(super) fn prepare_build_command(
            command: &mut Command,
            staging: &JetDevServerStaging,
        ) -> std::io::Result<Option<File>> {
            let guard = open_existing_with_share(
                &staging.path,
                GENERIC_READ,
                false,
                false,
                true,
                FILE_SHARE_ALL,
            )?;
            check_directory(&guard)?;
            if !same_object(&guard, &staging.directory)? {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "devserver staging directory changed before the build",
                ));
            }
            command.current_dir(&staging.path);
            Ok(Some(guard))
        }
    }

    // I1: these helpers are the only platform boundary in the generated
    // server. Raw descriptors/handles stay inside them; callers receive only
    // owned files or held directory authorities.
    #[cfg(unix)]
    fn jet_devserver_open_output_root(path: &Path) -> std::io::Result<JetDevServerOutputRoot> {
        jet_devserver_unix_output::open_root(path)
    }

    #[cfg(unix)]
    fn jet_devserver_create_staging(
        root: &JetDevServerOutputRoot,
        path: &Path,
        source_stem: &str,
    ) -> std::io::Result<JetDevServerStaging> {
        jet_devserver_unix_output::create_staging(root, path, source_stem)
    }

    #[cfg(unix)]
    fn jet_devserver_publish_build(
        root: &JetDevServerOutputRoot,
        staging: &JetDevServerStaging,
        html: Option<&[u8]>,
    ) -> std::io::Result<()> {
        let _publication = root.publication_lock.lock().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "devserver publication lock poisoned",
            )
        })?;
        jet_devserver_unix_output::publish_build(root, staging, html)
    }

    #[cfg(unix)]
    fn jet_devserver_cleanup_staging(
        root: &JetDevServerOutputRoot,
        staging: &JetDevServerStaging,
    ) -> std::io::Result<()> {
        jet_devserver_unix_output::cleanup(root, staging)
    }

    #[cfg(unix)]
    fn jet_devserver_prepare_build_command(
        command: &mut Command,
        staging: &JetDevServerStaging,
    ) -> std::io::Result<Option<File>> {
        jet_devserver_unix_output::prepare_build_command(command, staging)
    }

    #[cfg(unix)]
    fn jet_devserver_open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
        jet_devserver_validate_relative(relative)?;
        jet_devserver_unix_output::open_beneath(root, relative)
    }

    #[cfg(unix)]
    fn jet_devserver_open_held_beneath(
        root: &JetDevServerOutputRoot,
        relative: &Path,
    ) -> std::io::Result<File> {
        jet_devserver_validate_relative(relative)?;
        jet_devserver_unix_output::open_held_beneath(root, relative)
    }

    #[cfg(unix)]
    fn jet_devserver_validate_source(path: &Path) -> std::io::Result<()> {
        jet_devserver_unix_output::validate_source(path)
    }

    #[cfg(unix)]
    fn jet_devserver_check_opened_file(file: &File) -> std::io::Result<()> {
        jet_devserver_unix_output::check_opened_file(file)
    }

    #[cfg(windows)]
    fn jet_devserver_open_output_root(path: &Path) -> std::io::Result<JetDevServerOutputRoot> {
        jet_devserver_windows_output::open_root(path)
    }

    #[cfg(windows)]
    fn jet_devserver_create_staging(
        root: &JetDevServerOutputRoot,
        path: &Path,
        source_stem: &str,
    ) -> std::io::Result<JetDevServerStaging> {
        jet_devserver_windows_output::create_staging(root, path, source_stem)
    }

    #[cfg(windows)]
    fn jet_devserver_publish_build(
        root: &JetDevServerOutputRoot,
        staging: &JetDevServerStaging,
        html: Option<&[u8]>,
    ) -> std::io::Result<()> {
        let _publication = root.publication_lock.lock().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "devserver publication lock poisoned",
            )
        })?;
        jet_devserver_windows_output::publish_build(root, staging, html)
    }

    #[cfg(windows)]
    fn jet_devserver_cleanup_staging(
        root: &JetDevServerOutputRoot,
        staging: &JetDevServerStaging,
    ) -> std::io::Result<()> {
        jet_devserver_windows_output::cleanup(root, staging)
    }

    #[cfg(windows)]
    fn jet_devserver_prepare_build_command(
        command: &mut Command,
        staging: &JetDevServerStaging,
    ) -> std::io::Result<Option<File>> {
        jet_devserver_windows_output::prepare_build_command(command, staging)
    }

    #[cfg(windows)]
    fn jet_devserver_open_beneath(root: &Path, relative: &Path) -> std::io::Result<File> {
        jet_devserver_validate_relative(relative)?;
        jet_devserver_windows_output::open_beneath(root, relative)
    }

    #[cfg(windows)]
    fn jet_devserver_open_held_beneath(
        root: &JetDevServerOutputRoot,
        relative: &Path,
    ) -> std::io::Result<File> {
        jet_devserver_validate_relative(relative)?;
        jet_devserver_windows_output::open_held_beneath(root, relative)
    }

    #[cfg(windows)]
    fn jet_devserver_validate_source(path: &Path) -> std::io::Result<()> {
        jet_devserver_windows_output::validate_source(path)
    }

    #[cfg(windows)]
    fn jet_devserver_check_opened_file(file: &File) -> std::io::Result<()> {
        jet_devserver_windows_output::check_opened_file(file)
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_secure_output_unavailable() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "secure devserver output is unavailable on this platform",
        )
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_open_output_root(_path: &Path) -> std::io::Result<JetDevServerOutputRoot> {
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_create_staging(
        _root: &JetDevServerOutputRoot,
        _path: &Path,
        _source_stem: &str,
    ) -> std::io::Result<JetDevServerStaging> {
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_publish_build(
        _root: &JetDevServerOutputRoot,
        _staging: &JetDevServerStaging,
        _html: Option<&[u8]>,
    ) -> std::io::Result<()> {
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_cleanup_staging(
        _root: &JetDevServerOutputRoot,
        _staging: &JetDevServerStaging,
    ) -> std::io::Result<()> {
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_prepare_build_command(
        _command: &mut Command,
        _staging: &JetDevServerStaging,
    ) -> std::io::Result<Option<File>> {
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_open_beneath(_root: &Path, relative: &Path) -> std::io::Result<File> {
        jet_devserver_validate_relative(relative)?;
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_open_held_beneath(
        _root: &JetDevServerOutputRoot,
        relative: &Path,
    ) -> std::io::Result<File> {
        jet_devserver_validate_relative(relative)?;
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_validate_source(_path: &Path) -> std::io::Result<()> {
        Err(jet_devserver_secure_output_unavailable())
    }

    #[cfg(all(not(unix), not(windows)))]
    fn jet_devserver_check_opened_file(_file: &File) -> std::io::Result<()> {
        Err(jet_devserver_secure_output_unavailable())
    }

    fn jet_devserver_static_relative_path(path: &str) -> Option<&str> {
        let relative = if path == "/" {
            "index.html"
        } else {
            path.trim_start_matches('/')
        };
        let bytes = relative.as_bytes();
        let windows_drive_prefix =
            bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        if path.contains("..")
            || relative.is_empty()
            || relative.starts_with('/')
            || relative.starts_with('\\')
            || relative.contains('\\')
            || windows_drive_prefix
            || Path::new(relative)
                .components()
                .any(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
            || !jet_devserver_windows_components_are_safe(Path::new(relative))
        {
            None
        } else {
            Some(relative)
        }
    }

    fn jet_devserver_windows_component_is_safe(name: &std::ffi::OsStr) -> bool {
        let text = name.to_string_lossy();
        if text.is_empty()
            || text
                .chars()
                .last()
                .is_some_and(|character| character == ' ' || character == '.')
            || text
                .chars()
                .any(|character| {
                    character.is_control()
                        || matches!(character, ':' | '\\' | '<' | '>' | '"' | '|' | '?' | '*')
                })
        {
            return false;
        }
        let stem = text.split('.').next().unwrap_or_default();
        let upper = stem.to_ascii_uppercase();
        !matches!(
            upper.as_str(),
            "CON"
                | "CONIN$"
                | "CONOUT$"
                | "PRN"
                | "AUX"
                | "NUL"
                | "CLOCK$"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
                | "COM¹"
                | "COM²"
                | "COM³"
                | "LPT¹"
                | "LPT²"
                | "LPT³"
        )
    }

    fn jet_devserver_windows_components_are_safe(relative: &Path) -> bool {
        relative.components().all(|component| {
            matches!(
                component,
                Component::Normal(name) if jet_devserver_windows_component_is_safe(name)
            )
        })
    }

    fn jet_devserver_content_type_for(path: &Path) -> &'static str {
        match path.extension().and_then(|e| e.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "application/javascript; charset=utf-8",
            Some("wasm") => "application/wasm",
            Some("json") => "application/json; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            _ => "application/octet-stream",
        }
    }

    fn jet_devserver_live_reload_script() -> String {
        format!(
            r#"<script>
(function () {{
  var jetDevVersion = null;
  setInterval(function () {{
    fetch("/__jet_dev_version", {{ cache: "no-store" }})
      .then(function (r) {{ return r.text(); }})
      .then(function (v) {{
        if (jetDevVersion === null) {{ jetDevVersion = v; return; }}
        if (v !== jetDevVersion) {{ location.reload(); }}
      }})
      .catch(function () {{}});
  }}, {poll_ms});
}})();
</script>
"#,
            poll_ms = JET_DEVSERVER_LIVE_RELOAD_POLL_MS
        )
    }

    fn jet_devserver_write_response(
        stream: &mut impl Write,
        status: &str,
        content_type: &str,
        body: &[u8],
    ) -> std::io::Result<()> {
        jet_devserver_write_response_parts(stream, status, content_type, &[body])
    }

    fn jet_devserver_write_response_parts(
        stream: &mut impl Write,
        status: &str,
        content_type: &str,
        parts: &[&[u8]],
    ) -> std::io::Result<()> {
        let body_len = parts.iter().try_fold(0usize, |total, part| {
            total.checked_add(part.len()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "devserver response length overflow",
                )
            })
        })?;
        if body_len > JET_DEVSERVER_MAX_RESPONSE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "devserver response exceeds the response budget",
            ));
        }
        let header = format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            status,
            content_type,
            body_len
        );
        stream.write_all(header.as_bytes())?;
        for part in parts {
            stream.write_all(part)?;
        }
        stream.flush()
    }
}

pub use jet_devserver_impl::{jet_devserver_app, jet_devserver_for_app, JetDevServer};

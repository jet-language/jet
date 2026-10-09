//! Seed-only regression proofs for the canonical borrowed UI-host boundary.
#![allow(dead_code, unused_imports)]

mod runtime {
    use jet_foundation::Devtools::{JetDevtoolsEnvelope, JetDevtoolsViewState};
    use jet_foundation::Outcome::{JetAbsent, JetOutcome};
    use jet_codegen::Codegen::{
        JET_CANONICAL_ARABIC_FONT_BYTES, JET_CANONICAL_FONT_BYTES,
        JET_CANONICAL_SYMBOLS_FONT_BYTES,
    };

    pub trait JetShow {
        fn jet_show(&self) -> String;
    }
    mod jet_std {
        pub fn jet_reactive_effect_rooted<F: Fn() + Send + Sync + 'static>(body: F) {
            body();
        }
    }
    pub mod jet_tui_kernel {
        include!("../../jet-foundation/src/Prelude/TuiKernel.rs");
    }
    include!("../src/Prelude/Core/HostServices.rs");
    include!("../src/Prelude/Ui.rs");
}

#[cfg(test)]
mod tests {
    use super::runtime::*;

    #[test]
    fn ui_host_scope_rejects_dangling_safe_guard() {
        // Compile the canonical implementation, never execute the UAF witness.
        // The original safe guard is now private; the second witness checks the
        // lifetime constraint even inside the audited unsafe installation seam.
        let source = include_str!("ui_host_scope.rs");
        let marker = "    include!(\"../src/Prelude/Ui.rs\");\n}";
        let witness = r#"
    fn dangling_internal_guard() {
        let scope = {
            let mut host = JetUiHeadlessHost::new();
            let mut host_ref: &mut dyn JetUiHost = &mut host;
            unsafe { jet_ui_host_scope(&mut host_ref) }
        };
        let _ = jet_ui_host_capabilities();
        drop(scope);
    }
}"#;
        assert!(source.contains(marker));
        let source = source.replace(marker, &format!("    include!(\"../src/Prelude/Ui.rs\");\n{witness}"));
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        // Absolute include paths keep the compiler probe on the same sources.
        let source = source
            .replace("\"../src/", &format!("\"{}/src/", env!("CARGO_MANIFEST_DIR")))
            .replace("\"../../jet-foundation/", &format!("\"{}/../jet-foundation/", env!("CARGO_MANIFEST_DIR")));
        let source = format!("{source}\nfn main() {{\n    let scope = {{\n        let mut host = runtime::JetUiHeadlessHost::new();\n        runtime::jet_ui_host_scope(&mut host)\n    }};\n    let _ = runtime::jet_ui_host_capabilities();\n    drop(scope);\n}}\n");
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let scratch = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("ui-host-scope-proof"))
            .join(format!("dangling-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("dangling.rs");
        std::fs::write(&input, source).unwrap();
        let mut command = std::process::Command::new("rustc");
        command.args(["--edition=2021", "--emit=metadata", "--crate-name=ui_host_scope_dangling"])
            .arg(&input).arg("--out-dir").arg(&scratch)
            .arg("-L").arg(format!("dependency={}", deps.display()))
            .current_dir(tests_dir);
        for name in ["jet_foundation", "jet_codegen"] {
            let mut libraries = std::fs::read_dir(&deps).unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(&format!("lib{name}-"))
                    && path.extension().is_some_and(|extension| extension == "rlib"))
                .collect::<Vec<_>>();
            libraries.sort();
            assert_eq!(libraries.len(), 1, "proof target must have exactly one {name} rlib");
            command.arg("--extern").arg(format!("{name}={}", libraries[0].display()));
        }
        let output = command.output().unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "dangling host witness unexpectedly compiled");
        assert!(diagnostics.contains("E0603"), "safe scope must be inaccessible: {diagnostics}");
        assert!(diagnostics.contains("E0597"), "guard must retain the host borrow: {diagnostics}");
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn ui_host_scope_rejects_reentry_and_recovers() {
        let mut host = JetUiHeadlessHost::new();
        jet_ui_with_host(&mut host, || {
            jet_ui_with_current_host(|outer| {
                let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    jet_ui_with_current_host(|inner| {
                        let _ = inner.capability_facts();
                    });
                })).expect_err("nested callback must not mint an aliased mutable host");
                let message = failure.downcast_ref::<String>().map(String::as_str)
                    .or_else(|| failure.downcast_ref::<&str>().copied()).unwrap();
                assert!(message.contains("UI host callback cannot re-enter current-host access"));
                let _ = outer.capability_facts();
            });
            let _ = jet_ui_host_capabilities();
        });
        // Unwinding out of a whole scope also restores TLS before host drop.
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            jet_ui_with_host(&mut host, || {
                jet_ui_with_current_host(|_| jet_ui_host_capabilities());
            });
        }));
        assert!(failure.is_err());
        let _ = jet_ui_host_capabilities();
        jet_ui_with_host(&mut host, || { let _ = jet_ui_host_capabilities(); });
    }
}

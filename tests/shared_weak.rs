//! D-SHARED-CYCLE1=C: Shared.Weak cycles — I9 parity + free-or-leak proof.

mod common;

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use jet::JitBackend::JitBackend;

const EXAMPLE: &str = include_str!("../examples/features/memory/shared_weak_cycle.jet");
const EXPECTED: &str = include_str!("../examples/features/expected/memory/shared_weak_cycle.out");

fn fixture(tag: &str, source: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = common::unique_tmp(tag);
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, source).unwrap();
    (root, path)
}

fn with_compiler_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name("shared-weak-parity".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

/// Prelude free-or-leak law: weak-only cycles free when strong roots drop.
#[test]
fn weak_only_cycle_frees_when_strong_roots_drop() {
    use std::sync::Mutex;

    struct DropCount(Arc<AtomicUsize>);
    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Node {
        _drop: DropCount,
        peer: Mutex<Option<Weak<Node>>>,
    }

    let drops = Arc::new(AtomicUsize::new(0));
    {
        let a = Arc::new(Node {
            _drop: DropCount(drops.clone()),
            peer: Mutex::new(None),
        });
        let b = Arc::new(Node {
            _drop: DropCount(drops.clone()),
            peer: Mutex::new(None),
        });
        *a.peer.lock().unwrap() = Some(Arc::downgrade(&b));
        *b.peer.lock().unwrap() = Some(Arc::downgrade(&a));
        assert_eq!(Arc::strong_count(&a), 1);
        assert!(a.peer.lock().unwrap().as_ref().unwrap().upgrade().is_some());
        drop(a);
        drop(b);
    }
    assert_eq!(drops.load(Ordering::SeqCst), 2, "weak cycle must free");
}

#[test]
fn orphan_weak_upgrade_is_none_after_strong_drop() {
    let w = {
        let a = Arc::new(7_i64);
        Arc::downgrade(&a)
    };
    assert!(w.upgrade().is_none());
}

#[test]
fn shared_weak_cycle_matches_aot_and_default_tiers() {
    with_compiler_stack(|| {
        assert!(common::have_rustc(), "Shared.Weak parity proof needs rustc");
        let expected = EXPECTED;
        let source = EXAMPLE;

        let (native_root, native_path) = fixture("jet_shared_weak_native", source);
        let compiled = jet::compile_with_path(source, native_path.to_str().unwrap())
            .unwrap_or_else(|diagnostics| {
                panic!(
                    "{}",
                    jet::render_diagnostics(native_path.to_str().unwrap(), source, &diagnostics)
                )
            });
        let rust_path = native_root.join("main.rs");
        let native_bin = native_root.join("main");
        fs::write(&rust_path, compiled.rust).unwrap();
        let built = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&rust_path)
            .arg("-o")
            .arg(&native_bin)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected Shared.Weak output:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let native = Command::new(&native_bin).output().unwrap();
        assert!(native.status.success(), "{native:?}");
        assert_eq!(String::from_utf8(native.stdout).unwrap(), expected);

        let (_, default_path) = fixture("jet_shared_weak_default", source);
        let mut bundle = jet::Loader::load_entry(default_path.to_str().unwrap()).unwrap();
        let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.severity,
                    jet::Diagnostics::Severity::Error
                )
            })
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "{errors:?}");

        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            jet::Interpreter::RunOutcome::Ran { stdout, .. } => {
                assert_eq!(stdout, expected);
            }
            jet::Interpreter::RunOutcome::Problems(diagnostics) => {
                panic!("default Shared.Weak tier failed: {diagnostics:?}")
            }
        }
    });
}

//! Mount the bootstrap Host/Runner module set from `JET_REPACK_SOURCE_ROOT`
//! (set by repack.sh) with the same module names and
//! re-exports `Source/lib.rs` and the packaged compiler crate use, so the
//! modules resolve identical `crate::` paths here. The root is also the
//! `BOOTSTRAP_CANONICAL_SOURCE_ROOT` the packaged source names in its
//! `#[path]` attributes.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=JET_REPACK_SOURCE_ROOT");
    // Set only when the modules are compiled into the packaged compiler crate.
    println!("cargo::rustc-check-cfg=cfg(jet_bootstrap_compiler_artifact)");
    let root = PathBuf::from(
        std::env::var_os("JET_REPACK_SOURCE_ROOT").expect("JET_REPACK_SOURCE_ROOT (set by repack.sh)"),
    );
    let root = root
        .canonicalize()
        .unwrap_or_else(|error| panic!("JET_REPACK_SOURCE_ROOT `{}`: {error}", root.display()));
    let path = |relative: &str| {
        let file = root.join(relative);
        assert!(file.is_file(), "bootstrap source `{}` is missing", file.display());
        format!("{:?}", file.to_string_lossy().as_ref())
    };
    let mut out = String::new();
    writeln!(
        out,
        "pub(crate) const BOOTSTRAP_CANONICAL_SOURCE_ROOT: &str = {:?};",
        root.to_string_lossy().as_ref()
    )
    .unwrap();
    writeln!(
        out,
        "#[doc(hidden)]\npub mod BootstrapBuildIdentity {{\n    include!({});\n}}",
        path("Compiler/Bootstrap/BuildIdentity.rs")
    )
    .unwrap();
    for (relative, module) in [
        ("Compiler/Bootstrap/Host/RuntimeMirCodec.rs", "compiler_bootstrap_runtime_mir_codec"),
        ("Compiler/Bootstrap/Host/RuntimeMir.rs", "compiler_bootstrap_runtime_mir"),
        ("Compiler/Bootstrap/Host/DiagnosticCodec.rs", "compiler_bootstrap_diagnostic_codec"),
        ("Compiler/Bootstrap/Host/EntryCodec.rs", "compiler_bootstrap_entry_codec"),
        ("Compiler/Bootstrap/Host/CompilerImage.rs", "compiler_bootstrap_compiler_image"),
        ("Compiler/Bootstrap/Host/Native.rs", "compiler_bootstrap_host"),
        ("Compiler/Bootstrap/Runner.rs", "compiler_bootstrap_runner"),
    ] {
        writeln!(out, "#[path = {}]\n#[allow(dead_code)]\nmod {module};", path(relative)).unwrap();
    }
    out.push_str(
        "#[allow(unused_imports)]\n\
         pub(crate) use compiler_bootstrap_entry_codec::{\n\
             BootstrapEntryCodec, BootstrapEntryPhysicalBindings, BootstrapEntryValue,\n\
         };\n\
         #[allow(unused_imports)]\n\
         pub(crate) use compiler_bootstrap_compiler_image::{\n\
             archive_compiler_image, restore_compiler_image, CompilerImageError, CompilerImageHeader,\n\
             RestoredCompilerImage,\n\
         };\n\
         #[allow(unused_imports)]\n\
         pub(crate) use compiler_bootstrap_runtime_mir::{\n\
             execute_source_mir, SourceMirExecution, SourceMirExecutionError,\n\
         };\n\
         #[allow(unused_imports)]\n\
         pub(crate) use compiler_bootstrap_host::{\n\
             append_bootstrap_host_glue, open_authorized_sources, AuthorizedSourceLease,\n\
             AuthorizedSourceSnapshot, BootstrapBindingDescriptor, BootstrapHostCodecError,\n\
         };\n\
         #[allow(unused_imports)]\n\
         pub(crate) use compiler_bootstrap_runner::{\n\
             bootstrap_artifact_build_id, invoke_bootstrap_entry, prepare_bootstrap_artifact,\n\
             prepare_bootstrap_artifact_from_aot, BootstrapArtifact,\n\
             BootstrapBackendArtifact, BootstrapFactoryTier, BootstrapJetCompileResult,\n\
             BootstrapRunError, BootstrapRunOutput, BootstrapSourceResume, BootstrapSourceResumeFactory,\n\
             BootstrapWebArtifacts,\n\
         };\n",
    );
    let target = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("host_modules.rs");
    std::fs::write(&target, out).unwrap_or_else(|error| panic!("cannot write `{}`: {error}", target.display()));
}

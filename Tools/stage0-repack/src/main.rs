//! Offline stage-zero repackaging.
//!
//! `stage0-repack <keep-dir> [<compiler-project>]` reruns only the Host/Runner
//! packaging half of stage zero (`emit_stage_zero_source` after MIR Rust
//! emission) from what a `JET_STAGE_ZERO_KEEP=<keep-dir>` run retained:
//!
//! - `reference.rs`: the unpackaged emitted Rust of the compiler,
//! - `stage-zero.image`: the compiler image, which holds the canonical MIR
//!   program, the compiler artifact, the factory root and the source-authority
//!   digest,
//! - `stage-zero.ffi`: the FFI bridge crate the emitted Rust names,
//! - the authorized compiler sources, whose authority digest the image binds:
//!   the second argument, else `<keep-dir>/compiler-project` when the run
//!   retained it, else the shared assembly
//!   `~/.cache/jet-luna/compiler-bootstrap/project`.
//!
//! It writes the fresh packaged source to `<keep-dir>/stage-zero.rs` and into
//! the kept backend project `<keep-dir>/stage-zero/` (`generated.rs`,
//! `src/main.rs`, `src/compiler.image` when the image changed). The unit
//! crates are a function of `reference.rs` alone, so only the main crate
//! changes.
//!
//! The harness `main` appended after packaging is `GENERATED_ARTIFACT_MAIN`
//! of the generator sources' `Compiler/Bootstrap/Tests.rs`, task-root regions
//! stripped unless the kept `stage-zero.stamp` says `task_roots=true`; it is
//! also written to `artifact-main.rs`. The first run derives
//! `stage-zero/main.prefix.rs` (the main crate's text before the packaged
//! suffix) from the kept run's own output, which is only sound while the
//! generators still produce it. It keeps the kept run's source as
//! `stage-zero.base.rs` and reports whether the fresh source is
//! byte-identical to it.

#![allow(non_snake_case)]

pub(crate) use jet_driver::{Authority, Codegen, Comptime, SHA256};

include!(concat!(env!("OUT_DIR"), "/host_modules.rs"));

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jet_foundation::Layout::TargetLayout;
use jet_foundation::MIR::MirArtifactId;
use Codegen::MIRRust::{MirRustConfig, MirRustExecutionConfig, MirRustTarget};

/// `BOOTSTRAP_ENTRY_RELATIVE` of the stage-zero harness.
const COMPILER_ENTRY_RELATIVE: &str = "src/compiler.jet";

type Failure = String;

fn main() {
    if let Err(error) = run() {
        eprintln!("stage0-repack: {error}");
        std::process::exit(1);
    }
}

fn read(path: &Path) -> Result<Vec<u8>, Failure> {
    fs::read(path).map_err(|error| format!("cannot read `{}`: {error}", path.display()))
}

fn read_text(path: &Path) -> Result<String, Failure> {
    fs::read_to_string(path).map_err(|error| format!("cannot read `{}`: {error}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    fs::write(path, bytes).map_err(|error| format!("cannot write `{}`: {error}", path.display()))
}

/// The fixed-layout head of a compiler image (`CompilerImage.rs`
/// `encode_archive`): artifact, source-authority digest and the canonical MIR
/// payload (the factory root is chosen again from the bindings). Packaging
/// re-archives the decoded program, and the caller compares that archive with
/// these bytes, which checks the whole round trip.
struct KeptImage<'a> {
    artifact: MirArtifactId,
    source_authority_digest: [u8; 32],
    payload: &'a [u8],
}

fn parse_image(bytes: &[u8]) -> Result<KeptImage<'_>, Failure> {
    let mut at = 0usize;
    let mut take = |len: usize| -> Result<&[u8], Failure> {
        let end = at.checked_add(len).filter(|end| *end <= bytes.len());
        let end = end.ok_or_else(|| "truncated compiler image".to_string())?;
        let slice = &bytes[at..end];
        at = end;
        Ok(slice)
    };
    let u64_at = |slice: &[u8]| u64::from_le_bytes(slice.try_into().expect("eight bytes"));
    if take(8)? != b"JETCIMG\0" {
        return Err("not a compiler image (bad magic)".into());
    }
    let format = u16::from_le_bytes(take(2)?.try_into().expect("two bytes"));
    if format != 4 {
        return Err(format!("compiler image format {format} is not the supported format 4"));
    }
    let _schema = take(2)?;
    let artifact = MirArtifactId(u64_at(take(8)?));
    let _entry_function = take(8)?;
    let source_authority_digest: [u8; 32] = take(32)?.try_into().expect("32 bytes");
    let _checksum = take(32)?;
    let identity_len = usize::try_from(u64_at(take(8)?)).map_err(|_| "identity length overflows")?;
    take(identity_len)?;
    let payload_len = usize::try_from(u64_at(take(8)?)).map_err(|_| "payload length overflows")?;
    let payload = take(payload_len)?;
    Ok(KeptImage {
        artifact,
        source_authority_digest,
        payload,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn first_difference(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| left.len().min(right.len()))
}

fn line_of(text: &[u8], offset: usize) -> usize {
    text[..offset.min(text.len())].iter().filter(|byte| **byte == b'\n').count() + 1
}

/// The harness `main` `Tests.rs` `append_generated_artifact_main` appends:
/// `GENERATED_ARTIFACT_MAIN` of the generator sources, without the task-root
/// regions unless the kept run compiled that fixture in.
fn generated_artifact_main(task_roots: bool) -> Result<String, Failure> {
    const BEGIN: &str = "// bootstrap:task-roots-begin";
    const END: &str = "// bootstrap:task-roots-end";
    let tests_path = Path::new(crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT).join("Compiler/Bootstrap/Tests.rs");
    let tests = read_text(&tests_path)?;
    let text = tests
        .split_once("const GENERATED_ARTIFACT_MAIN: &str = r#\"")
        .and_then(|(_, rest)| rest.split_once("\"#"))
        .map(|(text, _)| text)
        .ok_or_else(|| format!("`{}` has no `GENERATED_ARTIFACT_MAIN` raw string", tests_path.display()))?;
    if task_roots {
        return Ok(text.to_string());
    }
    let mut main = String::new();
    let mut rest = text;
    while let Some(begin) = rest.find(BEGIN) {
        main.push_str(&rest[..begin]);
        let end = rest[begin..]
            .find(END)
            .ok_or("`GENERATED_ARTIFACT_MAIN` has an unterminated task-root region")?;
        rest = &rest[begin + end + END.len()..];
    }
    main.push_str(rest);
    Ok(main)
}

fn run() -> Result<(), Failure> {
    let mut args = std::env::args_os().skip(1);
    let keep = PathBuf::from(args.next().ok_or("usage: stage0-repack <keep-dir> [<compiler-project>]")?);
    let compiler_project = match args.next() {
        Some(project) => PathBuf::from(project),
        None if keep.join("compiler-project").is_dir() => keep.join("compiler-project"),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is unset")?)
            .join(".cache/jet-luna/compiler-bootstrap/project"),
    };
    let project = keep.join("stage-zero");
    let started = Instant::now();
    let lap = |what: &str| eprintln!("stage0-repack: {what} ({:.1}s)", started.elapsed().as_secs_f64());
    eprintln!("stage0-repack: Host/Runner sources from `{}`", crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT);

    let image_bytes = read(&keep.join("stage-zero.image"))?;
    let kept = parse_image(&image_bytes)?;
    let program = jet_foundation::MIR::mir_program_from_image_bytes(kept.payload)
        .map_err(|error| format!("cannot decode the kept compiler MIR: {error}"))?;
    lap("decoded compiler MIR from stage-zero.image");

    let lease = crate::open_authorized_sources(&compiler_project, Path::new(COMPILER_ENTRY_RELATIVE))
        .map_err(|error| format!("cannot authorize `{}`: {error:?}", compiler_project.display()))?;
    let digest = crate::compiler_bootstrap_compiler_image::compiler_image_source_authority_digest(lease.snapshot());
    if digest != kept.source_authority_digest {
        return Err(format!(
            "the compiler sources in `{}` (authority {}) are not the ones the kept image was built from ({}); \
             reassemble them at the kept run's revision",
            compiler_project.display(),
            hex(&digest),
            hex(&kept.source_authority_digest),
        ));
    }

    // The bridge crate name is the only FFI fact MIR Rust emission and the
    // AOT metadata read (`stage-zero.ffi` keeps the crate name and directory).
    let ffi_record = read_text(&keep.join("stage-zero.ffi"))?;
    let ffi = ffi_record.split_once('\n').map(|(name, dir)| {
        let dir = PathBuf::from(dir.trim_end_matches('\n'));
        jet_foundation::AST::FfiLink {
            crate_name: name.to_string(),
            cache_identity: dir.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            provenance_path: PathBuf::new(),
            rlib_path: PathBuf::new(),
            cdylib_path: PathBuf::new(),
            target_deps_dir: PathBuf::new(),
            host_deps_dir: PathBuf::new(),
            helper_bin_path: None,
            secrets_helper_bin_path: None,
            handle_facts: Vec::new(),
            link_closure: Default::default(),
        }
    });

    // The same configuration `emit_stage_zero_source` packages with.
    let mut execution = MirRustExecutionConfig::for_artifact(kept.artifact);
    execution.ffi = ffi.as_ref();
    execution.emit_types = true;
    execution.emit_foreign = true;
    execution.emit_metadata = false;
    execution.emit_runtime = true;
    execution.prune_unreachable_codecs = true;
    let metadata_config = MirRustConfig {
        target: TargetLayout::host(),
        target_kind: MirRustTarget::Native,
        root_prefix: String::new(),
        execution: execution.clone(),
    };
    let prepare_config = MirRustConfig {
        target: metadata_config.target.clone(),
        target_kind: MirRustTarget::Native,
        root_prefix: "crate::".to_string(),
        execution,
    };
    let reference = read_text(&keep.join("reference.rs"))?;
    let reference_len = reference.len();
    let snapshot = lease.snapshot();
    let (program, metadata_config, prepare_config) = (&program, &metadata_config, &prepare_config);
    let artifact = jet_driver::run_compiler_work(move || {
        let metadata = Codegen::MIRRust::mir_rust_aot_metadata(program, metadata_config);
        crate::prepare_bootstrap_artifact_from_aot(reference, program, snapshot, prepare_config, &metadata)
            .map_err(|error| format!("stage-zero Host/Runner packaging failed: {error}"))
    })?;
    lap("packaged");
    let packaged = artifact.source;
    let image = artifact
        .compiler_image
        .ok_or("stage-zero Host/Runner packaging embedded no compiler image")?;

    // Derive the main crate prefix only the kept run's own output carries.
    let base_path = keep.join("stage-zero.base.rs");
    if !base_path.is_file() {
        fs::copy(keep.join("stage-zero.rs"), &base_path)
            .map_err(|error| format!("cannot keep the base `stage-zero.rs`: {error}"))?;
    }
    let prefix_path = project.join("main.prefix.rs");
    if !prefix_path.is_file() {
        let base = read_text(&base_path)?;
        if !base.starts_with(&packaged) {
            let at = first_difference(base.as_bytes(), packaged.as_bytes());
            return Err(format!(
                "first run: the packaged source differs from `{}` at line {}, so `stage-zero/main.prefix.rs` \
                 cannot be derived; run once with JET_REPACK_SOURCE_ROOT set to the generator sources of the \
                 kept run",
                base_path.display(),
                line_of(base.as_bytes(), at),
            ));
        }
        let old_tail = &base[reference_len..];
        let base_main_path = [project.join("main.rs.orig"), project.join("src/main.rs")]
            .into_iter()
            .find(|path| path.is_file())
            .ok_or("the kept backend project has no main crate source")?;
        let base_main = read_text(&base_main_path)?;
        let prefix = base_main.strip_suffix(old_tail).ok_or_else(|| {
            format!(
                "`{}` does not end with the packaged suffix of the base source; restore the main crate the \
                 kept run wrote",
                base_main_path.display()
            )
        })?;
        write(&prefix_path, prefix.as_bytes())?;
        lap("derived stage-zero/main.prefix.rs");
    }

    let stamp_path = keep.join("stage-zero.stamp");
    let task_roots = match read_text(&stamp_path)?.lines().find_map(|line| line.strip_prefix("task_roots=")) {
        Some("true") => true,
        Some("false") => false,
        _ => return Err(format!("`{}` has no `task_roots=true|false` line", stamp_path.display())),
    };
    let artifact_main = generated_artifact_main(task_roots)?;
    write(&keep.join("artifact-main.rs"), artifact_main.as_bytes())?;
    let mut source = packaged;
    source.push_str(&artifact_main);

    let base = read(&base_path)?;
    if source.as_bytes() == base.as_slice() {
        eprintln!("stage0-repack: stage-zero.rs is byte-identical to the base ({} bytes)", source.len());
    } else {
        let at = first_difference(source.as_bytes(), &base);
        eprintln!(
            "stage0-repack: stage-zero.rs differs from the base from line {} ({} bytes, base {})",
            line_of(source.as_bytes(), at),
            source.len(),
            base.len()
        );
    }
    drop(base);
    if image == image_bytes {
        eprintln!("stage0-repack: compiler image is byte-identical to stage-zero.image");
    } else {
        eprintln!("stage0-repack: compiler image changed; rewriting stage-zero.image");
        write(&keep.join("stage-zero.image"), &image)?;
        write(&project.join("src/compiler.image"), &image)?;
    }

    let tail = source
        .get(reference_len..)
        .ok_or("the packaged source does not extend reference.rs")?;
    let mut main_source = read_text(&prefix_path)?;
    main_source.push_str(tail);
    write(&keep.join("stage-zero.rs"), source.as_bytes())?;
    write(&project.join("generated.rs"), source.as_bytes())?;
    write(&project.join("src/main.rs"), main_source.as_bytes())?;
    lap("wrote stage-zero.rs, stage-zero/generated.rs and stage-zero/src/main.rs");
    Ok(())
}

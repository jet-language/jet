//! D-DEVR-PROD1=A: prepare redacted context for the shared development receipt
//! writer. This module hashes the run closure and inputs; it never stores their
//! values in the receipt environment.

use std::fs;
use std::path::{Path, PathBuf};

use jet::development_receipt as receipt;

const TARGET: &str = "native";

pub(crate) struct Context {
    directory: PathBuf,
    entry: String,
    source_digest: String,
    closure_digest: String,
    input_digest: String,
    input_count: usize,
    input_count_digest: String,
    target_digest: String,
}

pub(crate) fn prepare(file: &str, source: &str, program_args: &[&String]) -> Context {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let entry = project_relative_entry(Path::new(file), &cwd);
    let source_digest = content_digest(source.as_bytes());

    let mut closure = b"jet-production-closure-v1\0".to_vec();
    frame(&mut closure, entry.as_bytes());
    frame(&mut closure, source_digest.as_bytes());
    add_project_input(&mut closure, file, &cwd, "manifest");
    add_project_input(&mut closure, file, &cwd, "lock");
    let closure_digest = content_digest(&closure);

    let mut inputs = b"jet-production-inputs-v1\0".to_vec();
    for argument in program_args {
        frame(&mut inputs, argument.as_bytes());
    }
    let input_digest = content_digest(&inputs);
    let input_count = program_args.len();
    let input_count_digest = content_digest(input_count.to_string().as_bytes());
    let target_digest = content_digest(TARGET.as_bytes());

    let mut identity = b"jet-production-failure-v1\0".to_vec();
    for value in [
        entry.as_bytes(),
        source_digest.as_bytes(),
        closure_digest.as_bytes(),
        input_digest.as_bytes(),
        TARGET.as_bytes(),
    ] {
        frame(&mut identity, value);
    }
    let receipt_id = content_digest(&identity);
    let directory = cwd
        .join(".jet")
        .join("reports")
        .join(format!("production-{}", &receipt_id[7..23]));
    Context {
        directory,
        entry,
        source_digest,
        closure_digest,
        input_digest,
        input_count,
        input_count_digest,
        target_digest,
    }
}

impl Context {
    pub(crate) fn install(&self) {
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_DIRECTORY_ENV,
            &self.directory,
        );
        std::env::set_var(receipt::JET_DEVELOPMENT_RECEIPT_ENTRY_ENV, &self.entry);
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST_ENV,
            &self.source_digest,
        );
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST_ENV,
            &self.closure_digest,
        );
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST_ENV,
            &self.input_digest,
        );
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_ENV,
            self.input_count.to_string(),
        );
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST_ENV,
            &self.input_count_digest,
        );
        std::env::set_var(receipt::JET_DEVELOPMENT_RECEIPT_TARGET_ENV, TARGET);
        std::env::set_var(
            receipt::JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST_ENV,
            &self.target_digest,
        );
    }
}

fn project_relative_entry(file: &Path, cwd: &Path) -> String {
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    absolute
        .strip_prefix(cwd)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|path| !path.is_empty() && path != ".")
        .or_else(|| {
            file.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "<unknown>".into())
}

fn add_project_input(closure: &mut Vec<u8>, file: &str, cwd: &Path, kind: &str) {
    let source = Path::new(file);
    let start = source.parent().unwrap_or_else(|| Path::new("."));
    let Some(root) = jet::Loader::find_manifest_root(start) else {
        return;
    };
    let path = match kind {
        "manifest" => jet::Loader::manifest_path(&root),
        "lock" => Some(root.join(".jet").join("lock")),
        _ => None,
    };
    let Some(path) = path else {
        return;
    };
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    frame(closure, kind.as_bytes());
    let relative = path
        .strip_prefix(cwd)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| kind.into());
    frame(closure, relative.as_bytes());
    frame(closure, content_digest(&bytes).as_bytes());
}

fn content_digest(bytes: &[u8]) -> String {
    format!("sha256-{}", jet::SHA256::sha256_hex(bytes))
}

fn frame(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

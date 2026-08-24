// D-DEVR-LAW1=A / D-ECO-RECEIPT2=A: one connected record for every
// development act. This source is compiled into AOT programs and included by
// resident engines; hosts only marshal the record to durable storage.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevelopmentReceiptInput {
    pub name: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevelopmentReceiptPath {
    pub code: String,
    pub file: String,
    pub line: u32,
    pub function: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevelopmentReceipt {
    pub act: String,
    pub locked_closure: String,
    pub inputs: Vec<JetDevelopmentReceiptInput>,
    pub planned_action: String,
    pub outputs: Vec<JetDevelopmentReceiptInput>,
    pub activation_proof: String,
    pub parent_generation: String,
    pub witness: String,
    pub outcome: String,
    pub failure_path: Option<JetDevelopmentReceiptPath>,
}

impl JetDevelopmentReceipt {
    pub fn render(&self) -> String {
        let mut out = String::from("jet-development-receipt-v1\n");
        receipt_line(&mut out, "act", "", &self.act);
        receipt_line(&mut out, "closure", "", &self.locked_closure);
        for input in sorted_pairs(&self.inputs) {
            receipt_line(&mut out, "input", &input.name, &input.digest);
        }
        receipt_line(&mut out, "action", "planned", &self.planned_action);
        for output in sorted_pairs(&self.outputs) {
            receipt_line(&mut out, "output", &output.name, &output.digest);
        }
        receipt_line(&mut out, "activation-proof", "", &self.activation_proof);
        receipt_line(&mut out, "parent-generation", "", &self.parent_generation);
        receipt_line(&mut out, "witness", "", &self.witness);
        if let Some(path) = &self.failure_path {
            receipt_line(&mut out, "failure-path", "code", &path.code);
            receipt_line(&mut out, "failure-path", "file", &path.file);
            receipt_line(&mut out, "failure-path", "line", &path.line.to_string());
            receipt_line(&mut out, "failure-path", "function", &path.function);
        }
        receipt_line(&mut out, "outcome", "", &self.outcome);
        out
    }

    /// The stable identity of an act, independent of its witness and result.
    /// Inputs are sorted so callers cannot manufacture a second identity by
    /// presenting the same locked closure in a different order.
    pub fn identity_bytes(&self) -> Vec<u8> {
        let mut out = b"jet-development-act-v1\0".to_vec();
        frame(&mut out, &self.act);
        frame(&mut out, &self.locked_closure);
        for input in sorted_pairs(&self.inputs) {
            frame(&mut out, &input.name);
            frame(&mut out, &input.digest);
        }
        frame(&mut out, &self.planned_action);
        out
    }
}

/// The shared symbol used by hosts to serialize a receipt. The host owns the
/// filesystem transaction; this Prelude function owns the record bytes.
pub fn jet_development_receipt_render(receipt: &JetDevelopmentReceipt) -> String {
    receipt.render()
}

pub const JET_DEVELOPMENT_RECEIPT_DIRECTORY_ENV: &str = "JET_DEVELOPMENT_RECEIPT_DIRECTORY";
pub const JET_DEVELOPMENT_RECEIPT_ENTRY_ENV: &str = "JET_DEVELOPMENT_RECEIPT_ENTRY";
pub const JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST_ENV: &str = "JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST";
pub const JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST_ENV: &str =
    "JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST";
pub const JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST_ENV: &str = "JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST";
pub const JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_ENV: &str = "JET_DEVELOPMENT_RECEIPT_INPUT_COUNT";
pub const JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST_ENV: &str =
    "JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST";
pub const JET_DEVELOPMENT_RECEIPT_TARGET_ENV: &str = "JET_DEVELOPMENT_RECEIPT_TARGET";
pub const JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST_ENV: &str = "JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST";

/// Write one production failure through the canonical development receipt
/// serializer. Only digests, a project-relative entry, and a redacted failure
/// path reach the receipt; runtime message, source text, locals, argv values,
/// and environment values never do.
pub fn jet_production_failure_receipt_write(
    code: &str,
    file: &str,
    line: u32,
    function: &str,
) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (code, file, line, function);
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let directory = std::env::var(JET_DEVELOPMENT_RECEIPT_DIRECTORY_ENV).ok()?;
        let entry = std::env::var(JET_DEVELOPMENT_RECEIPT_ENTRY_ENV).ok()?;
        let source_digest = std::env::var(JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST_ENV).ok()?;
        let closure_digest = std::env::var(JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST_ENV).ok()?;
        let input_digest = std::env::var(JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST_ENV).ok()?;
        let input_count = std::env::var(JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_ENV)
            .ok()?
            .parse::<usize>()
            .ok()?;
        let input_count_digest =
            std::env::var(JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST_ENV).ok()?;
        let target = std::env::var(JET_DEVELOPMENT_RECEIPT_TARGET_ENV).ok()?;
        let target_digest = std::env::var(JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST_ENV).ok()?;
        let failure_file = receipt_path(file, &entry);
        let receipt = JetDevelopmentReceipt {
            act: "production-failure".into(),
            locked_closure: closure_digest,
            inputs: vec![
                receipt_input("entry", &source_digest),
                receipt_input("argv", &input_digest),
                receipt_input("argv-count", &input_count_digest),
                receipt_input("target", &target_digest),
            ],
            planned_action:
                "sha256-2baa4c66edeaf54e166ff66bdd4c914c3076a7621a98c90f9a577c9b825be3b3".into(),
            outputs: Vec::new(),
            activation_proof:
                "sha256-b8846cc6a6dfb698b445cafa1f316a814f70d56520598e8efcdbd42970999378".into(),
            parent_generation: String::new(),
            witness: format!(
                "jet-runtime;target={target};argv-count={input_count};redaction=by-construction"
            ),
            outcome: "failed".into(),
            failure_path: Some(JetDevelopmentReceiptPath {
                code: code.to_owned(),
                file: failure_file,
                line,
                function: if function.is_empty() {
                    "<unknown>".into()
                } else {
                    function.to_owned()
                },
            }),
        };
        let bytes = jet_development_receipt_render(&receipt).into_bytes();
        let directory = std::path::Path::new(&directory);
        if !ensure_receipt_directory(directory) {
            return None;
        }
        let path = directory.join("receipt");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return None;
            }
            Ok(_) => return Some(path.to_string_lossy().into_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = match options.open(&path) {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Some(path.to_string_lossy().into_owned());
            }
            Err(_) => return None,
        };
        if std::io::Write::write_all(&mut output, &bytes).is_err() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).is_err() {
                return None;
            }
        }
        Some(path.to_string_lossy().into_owned())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn receipt_input(name: &str, value: &str) -> JetDevelopmentReceiptInput {
    JetDevelopmentReceiptInput {
        name: name.into(),
        digest: value.into(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_receipt_directory(path: &std::path::Path) -> bool {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        loop {
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return false;
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let mut builder = std::fs::DirBuilder::new();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::DirBuilderExt;
                        builder.mode(0o700);
                    }
                    match builder.create(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            continue;
                        }
                        Err(_) => return false,
                    }
                }
                Err(_) => return false,
            }
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn receipt_path(file: &str, entry: &str) -> String {
    let raw = if file.is_empty() { entry } else { file };
    if raw.is_empty() || raw.starts_with('<') {
        return if raw.is_empty() {
            "<unknown>".into()
        } else {
            raw.into()
        };
    }
    let path = std::path::Path::new(raw);
    let relative = if path.is_absolute() {
        let Ok(cwd) = std::env::current_dir() else {
            return "<external>".into();
        };
        let Ok(relative) = path.strip_prefix(cwd) else {
            return "<external>".into();
        };
        relative.to_string_lossy().into_owned()
    } else {
        raw.into()
    };
    let relative = relative.replace('\\', "/");
    if relative.is_empty()
        || relative == "."
        || relative == ".."
        || relative.starts_with("../")
        || relative.starts_with('/')
    {
        "<external>".into()
    } else {
        relative
    }
}

pub fn is_content_address(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sorted_pairs(values: &[JetDevelopmentReceiptInput]) -> Vec<JetDevelopmentReceiptInput> {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    sorted
}

fn receipt_line(out: &mut String, kind: &str, name: &str, value: &str) {
    out.push_str(kind);
    out.push('\t');
    out.push_str(&hex(name));
    out.push('\t');
    out.push_str(&hex(value));
    out.push('\n');
}

fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn frame(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

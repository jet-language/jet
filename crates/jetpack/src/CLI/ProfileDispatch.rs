//! Native tools-profile dispatcher.
//!
//! Copies of the `jetpack` executable installed under `~/.jet/bin/<name>`
//! enter here before ordinary CLI dispatch. The dispatcher reads one durable,
//! checksummed generation pointer, validates the immutable generation and its
//! copied executable, then replaces itself with that executable. No shell or
//! batch parser participates in argv forwarding.

use crate::{JSON, SHA256, Store, Syntax};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(any(windows, test))]
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Component, Path, PathBuf};

pub(crate) const CURRENT_SCHEMA: &str = "jet-profile-current-v1";
pub(crate) const GENERATION_SCHEMA: &str = "jet-profile-generation-v2";
pub(crate) const PROFILE_OWNER: &str = "user";
pub(crate) const INVALID_DISPATCH_EXIT: i32 = 126;
pub(crate) const MISSING_DISPATCH_EXIT: i32 = 127;

const CURRENT_FILE: &str = "current";
const COMPLETE_FILE: &str = "complete";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_TOOLS: usize = 256;
const MAX_BINS: usize = 1024;
const MAX_STRING: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentPointer {
    pub(crate) generation: u64,
    pub(crate) witness: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationMetadata {
    pub(crate) generation: u64,
    pub(crate) created_at: u64,
    pub(crate) tools: Vec<GenerationTool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationTool {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: String,
    pub(crate) reference: String,
    pub(crate) output_hash: String,
    pub(crate) store_root: String,
    pub(crate) bins: Vec<String>,
    pub(crate) members: Vec<String>,
    /// SHA-256 of each immutable generation-owned executable, aligned with
    /// `bins` and `members`.
    pub(crate) projection_hashes: Vec<String>,
}

pub(crate) fn format_current_pointer(pointer: &CurrentPointer) -> io::Result<String> {
    if pointer.generation == 0 {
        return Err(invalid("profile generation is zero"));
    }
    validate_digest(&pointer.witness)?;
    let body = format!(
        "{CURRENT_SCHEMA}\ngeneration\t{}\nwitness\t{}\n",
        pointer.generation, pointer.witness
    );
    Ok(format!(
        "{body}checksum\tsha256-{}\n",
        SHA256::sha256_hex(body.as_bytes())
    ))
}

pub(crate) fn parse_current_pointer(text: &str) -> io::Result<CurrentPointer> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != CURRENT_SCHEMA || !text.ends_with('\n') {
        return Err(invalid("current pointer has wrong schema or field count"));
    }
    let generation = lines[1]
        .strip_prefix("generation\t")
        .ok_or_else(|| invalid("current pointer lacks generation"))?
        .parse::<u64>()
        .map_err(|_| invalid("current pointer generation is invalid"))?;
    if generation == 0 {
        return Err(invalid("current pointer generation is zero"));
    }
    let witness = lines[2]
        .strip_prefix("witness\t")
        .ok_or_else(|| invalid("current pointer lacks witness"))?;
    validate_digest(witness)?;
    let checksum = lines[3]
        .strip_prefix("checksum\tsha256-")
        .ok_or_else(|| invalid("current pointer lacks checksum"))?;
    validate_hex64(checksum, "current pointer checksum")?;
    let body_len = text
        .rfind("checksum\t")
        .ok_or_else(|| invalid("current pointer lacks checksum"))?;
    if SHA256::sha256_hex(text[..body_len].as_bytes()) != checksum {
        return Err(invalid("current pointer checksum mismatch"));
    }
    Ok(CurrentPointer {
        generation,
        witness: witness.to_string(),
    })
}

pub(crate) fn format_generation_metadata(metadata: &GenerationMetadata) -> io::Result<String> {
    validate_generation(metadata)?;
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"schema\": {},\n", json_string(GENERATION_SCHEMA)));
    out.push_str(&format!("  \"generation\": {},\n", metadata.generation));
    out.push_str(&format!("  \"owner\": {},\n", json_string(PROFILE_OWNER)));
    out.push_str(&format!(
        "  \"profile\": {},\n",
        json_string(Syntax::TOOL_PROFILE_NAME)
    ));
    out.push_str(&format!("  \"created_at\": {},\n", metadata.created_at));
    out.push_str("  \"tools\": [\n");
    for (index, tool) in metadata.tools.iter().enumerate() {
        out.push_str("    {\n");
        for (key, value) in [
            ("name", &tool.name),
            ("version", &tool.version),
            ("source", &tool.source),
            ("reference", &tool.reference),
            ("output_hash", &tool.output_hash),
            ("store_root", &tool.store_root),
        ]
        .into_iter()
        {
            out.push_str(&format!("      \"{key}\": {},\n", json_string(value)));
        }
        write_string_array(&mut out, "bins", &tool.bins, true);
        write_string_array(&mut out, "members", &tool.members, true);
        write_string_array(
            &mut out,
            "projection_hashes",
            &tool.projection_hashes,
            false,
        );
        out.push_str("    }");
        if index + 1 != metadata.tools.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    Ok(out)
}

pub(crate) fn parse_generation_metadata(
    text: &str,
    expected_generation: u64,
) -> io::Result<GenerationMetadata> {
    let JSON::JSONValue::Object(root) = JSON::parse(text).map_err(invalid)? else {
        return Err(invalid("profile metadata root is not an object"));
    };
    expect_exact_keys(
        &root,
        &["created_at", "generation", "owner", "profile", "schema", "tools"],
        "profile metadata",
    )?;
    if string_field(&root, "schema")? != GENERATION_SCHEMA
        || string_field(&root, "owner")? != PROFILE_OWNER
        || string_field(&root, "profile")? != Syntax::TOOL_PROFILE_NAME
    {
        return Err(invalid("profile metadata identity mismatch"));
    }
    let generation = integer_field(&root, "generation")?;
    if generation != expected_generation || generation == 0 {
        return Err(invalid("profile generation metadata disagrees with path"));
    }
    let created_at = integer_field(&root, "created_at")?;
    let JSON::JSONValue::Array(entries) = root
        .get("tools")
        .ok_or_else(|| invalid("profile metadata lacks tools"))?
    else {
        return Err(invalid("profile tools field is not an array"));
    };
    if entries.len() > MAX_TOOLS {
        return Err(invalid("profile tool count exceeds bound"));
    }
    let mut tools = Vec::with_capacity(entries.len());
    for entry in entries {
        let JSON::JSONValue::Object(tool) = entry else {
            return Err(invalid("profile tool entry is not an object"));
        };
        expect_exact_keys(
            tool,
            &[
                "bins",
                "members",
                "name",
                "output_hash",
                "projection_hashes",
                "reference",
                "source",
                "store_root",
                "version",
            ],
            "profile tool",
        )?;
        tools.push(GenerationTool {
            name: bounded_string(tool, "name")?,
            version: bounded_string(tool, "version")?,
            source: bounded_string(tool, "source")?,
            reference: bounded_string(tool, "reference")?,
            output_hash: bounded_string(tool, "output_hash")?,
            store_root: bounded_string(tool, "store_root")?,
            bins: string_array(tool, "bins")?,
            members: string_array(tool, "members")?,
            projection_hashes: string_array(tool, "projection_hashes")?,
        });
    }
    let metadata = GenerationMetadata {
        generation,
        created_at,
        tools,
    };
    validate_generation(&metadata)?;
    Ok(metadata)
}

pub(crate) fn generation_witness(metadata_text: &str, metadata: &GenerationMetadata) -> String {
    let targets = metadata
        .tools
        .iter()
        .map(|tool| tool.output_hash.as_str())
        .collect::<BTreeSet<_>>();
    let mut canonical = format!(
        "jet-profile-generation-witness-v1\nmetadata\t{}\n",
        SHA256::sha256_hex(metadata_text.as_bytes())
    );
    for digest in targets {
        canonical.push_str("target\t");
        canonical.push_str(digest);
        canonical.push('\n');
    }
    format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()))
}

/// Atomically install this running `jetpack` executable as one exact-name
/// dispatcher. Caller owns profile serialization and collision policy.
pub(crate) fn install_dispatcher(bin_dir: &Path, bin: &str) -> io::Result<PathBuf> {
    validate_bin_name(bin)?;
    ensure_directory_chain(bin_dir)?;
    let source = std::env::current_exe()?;
    validate_regular_file(&source)?;
    let physical = physical_bin_name(bin);
    let destination = bin_dir.join(&physical);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(invalid("dispatcher destination is not an owned regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let partial = bin_dir.join(format!(
        ".dispatcher-{physical}-{}.partial",
        std::process::id()
    ));
    remove_regular_partial(&partial)?;

    if fs::hard_link(&source, &partial).is_err() {
        copy_file_create_new(&source, &partial)?;
    }
    sync_and_match(&source, &partial)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(&source)?.permissions().mode();
        fs::set_permissions(&partial, fs::Permissions::from_mode(mode))?;
        fs::File::open(&partial)?.sync_all()?;
    }
    atomic_replace(&partial, &destination)?;
    Store::sync_store_directory(bin_dir)?;
    validate_regular_file(&destination)?;
    Ok(destination)
}

/// Returns `None` for ordinary `jetpack`; installed dispatcher invocations
/// return an exit code or replace the process on Unix.
#[doc(hidden)]
pub fn dispatch_current_process() -> Option<i32> {
    let executable = std::env::current_exe().ok()?;
    let invoked = std::env::args_os().next()?;
    let bin = Path::new(&invoked).file_name()?.to_str()?;
    if bin.eq_ignore_ascii_case("jetpack") || bin.eq_ignore_ascii_case("jetpack.exe") {
        return None;
    }
    validate_bin_name(bin).ok()?;
    let executable_file = fs::File::open(&executable).ok()?;
    let executable_identity = executable_file.metadata().ok()?;
    let (installed, _installed_file) =
        resolve_invoked_entry(Path::new(&invoked), &executable_identity)?;
    let bin_dir = installed.parent()?.to_path_buf();
    Some(match resolve_dispatch_target(&bin_dir, bin) {
        Ok(target) => execute_target(&target),
        Err(error) if error.kind() == io::ErrorKind::NotFound => MISSING_DISPATCH_EXIT,
        Err(_) => INVALID_DISPATCH_EXIT,
    })
}

fn resolve_invoked_entry(
    invoked: &Path,
    executable_identity: &fs::Metadata,
) -> Option<(PathBuf, fs::File)> {
    if invoked.is_absolute() {
        return eligible_dispatcher_entry(invoked, executable_identity);
    }
    let mut components = invoked.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return None;
    }
    resolve_path_entry(
        invoked,
        executable_identity,
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>()),
    )
}

fn resolve_path_entry(
    invoked: &Path,
    executable_identity: &fs::Metadata,
    directories: impl IntoIterator<Item = PathBuf>,
) -> Option<(PathBuf, fs::File)> {
    for directory in directories {
        let directory = if directory.is_absolute() {
            directory
        } else {
            std::env::current_dir().ok()?.join(directory)
        };
        for entry in path_entries(&directory, invoked, cfg!(windows)) {
            let Ok(file) = open_pinned_regular(&entry) else {
                continue;
            };
            if ensure_open_executable(&file).is_err() {
                continue;
            }
            if same_file_identity(executable_identity, &file.metadata().ok()?) {
                return validate_dispatcher_layout(entry).map(|entry| (entry, file));
            }
            // argv[0] can be preserved across a launcher's own PATH
            // resolution. Bind running file, not an earlier shadow.
        }
    }
    None
}

fn path_entries(directory: &Path, invoked: &Path, windows: bool) -> Vec<PathBuf> {
    let mut entries = vec![directory.join(invoked)];
    if windows && invoked.extension().is_none() {
        entries.push(directory.join(format!("{}.exe", invoked.to_string_lossy())));
    }
    entries
}

fn eligible_dispatcher_entry(
    entry: &Path,
    executable_identity: &fs::Metadata,
) -> Option<(PathBuf, fs::File)> {
    let file = open_pinned_regular(entry).ok()?;
    ensure_open_executable(&file).ok()?;
    if !same_file_identity(executable_identity, &file.metadata().ok()?) {
        return None;
    }
    validate_dispatcher_layout(entry.to_path_buf()).map(|entry| (entry, file))
}

fn validate_dispatcher_layout(entry: PathBuf) -> Option<PathBuf> {
    let bin_dir = entry.parent()?;
    if bin_dir.file_name()? != Syntax::TOOL_BIN_DIR
        || bin_dir.parent()?.file_name()? != Syntax::CONFIG_DEFAULT_DIR
    {
        return None;
    }
    Some(entry)
}

pub(crate) fn physical_bin_name(logical: &str) -> String {
    physical_bin_name_for(logical, cfg!(windows))
}

fn physical_bin_name_for(logical: &str, windows: bool) -> String {
    if windows && !logical.to_ascii_lowercase().ends_with(".exe") {
        format!("{logical}.exe")
    } else {
        logical.to_string()
    }
}

struct DispatchTarget {
    #[cfg(windows)]
    path: PathBuf,
    file: fs::File,
}

fn resolve_dispatch_target(bin_dir: &Path, bin: &str) -> io::Result<DispatchTarget> {
    validate_bin_name(bin)?;
    ensure_directory_chain(bin_dir)?;
    let tools = bin_dir
        .parent()
        .ok_or_else(|| invalid("profile bin directory has no parent"))?
        .join(Syntax::TOOL_STATE_DIR);
    ensure_directory_chain(&tools)?;
    let pointer_text = read_bounded_regular(&tools.join(CURRENT_FILE))?;
    let pointer = parse_current_pointer(&pointer_text)?;
    let generation_dir = tools.join("generations").join(pointer.generation.to_string());
    ensure_directory_chain(&generation_dir)?;
    let metadata_text = read_bounded_regular(&generation_dir.join("meta.json"))?;
    let metadata = parse_generation_metadata(&metadata_text, pointer.generation)?;
    let witness = generation_witness(&metadata_text, &metadata);
    if witness != pointer.witness {
        return Err(invalid("current pointer witness disagrees with generation"));
    }
    let complete = read_bounded_regular(&generation_dir.join(COMPLETE_FILE))?;
    if complete != format!("{witness}\n") {
        return Err(invalid("profile generation complete witness mismatch"));
    }
    let (tool, slot) = find_bin(&metadata, bin)?;
    let projection = generation_dir
        .join("bin")
        .join(physical_bin_name(&tool.bins[slot]));
    let mut projection_file = open_pinned_regular(&projection)?;
    let actual = format!("sha256-{}", sha256_open_file_hex(&mut projection_file)?);
    if actual != tool.projection_hashes[slot] {
        return Err(invalid("profile projection digest mismatch"));
    }
    validate_original_authority(tool, slot, &actual)?;
    ensure_open_executable(&projection_file)?;
    #[cfg(unix)]
    let projection_file = freeze_unix_projection(&mut projection_file, &actual)?;
    Ok(DispatchTarget {
        #[cfg(windows)]
        path: projection,
        file: projection_file,
    })
}

fn find_bin<'a>(
    metadata: &'a GenerationMetadata,
    bin: &str,
) -> io::Result<(&'a GenerationTool, usize)> {
    if let Some(found) = metadata
        .tools
        .iter()
        .find_map(|tool| tool.bins.iter().position(|candidate| candidate == bin).map(|i| (tool, i)))
    {
        return Ok(found);
    }
    #[cfg(windows)]
    if bin.to_ascii_lowercase().ends_with(".exe") {
        if let Some(found) = metadata.tools.iter().find_map(|tool| {
            tool.bins
                .iter()
                .position(|candidate| physical_bin_name(candidate).eq_ignore_ascii_case(bin))
                .map(|index| (tool, index))
        }) {
            return Ok(found);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "profile bin was removed",
    ))
}

fn validate_original_authority(tool: &GenerationTool, slot: usize, digest: &str) -> io::Result<()> {
    let roots = Store::Roots {
        root: PathBuf::from(&tool.store_root),
        dev_mode: false,
    };
    ensure_directory_chain(&roots.root)?;
    let entry = Store::list_checked(&roots)?
        .into_iter()
        .find(|entry| entry.reference == tool.reference && entry.envelope.output_hash == tool.output_hash)
        .ok_or_else(|| invalid("profile Store authority is unavailable"))?;
    let member = Path::new(&entry.bin).join(&tool.members[slot]);
    let mut member_file = open_pinned_regular(&member)?;
    let member_digest = format!("sha256-{}", sha256_open_file_hex(&mut member_file)?);
    if member_digest != digest {
        return Err(invalid("profile projection disagrees with Store member"));
    }
    ensure_open_executable(&member_file)
}

#[cfg(unix)]
fn execute_target(target: &DispatchTarget) -> i32 {
    use std::ffi::CString;
    use std::os::fd::AsRawFd as _;
    use std::os::unix::ffi::OsStrExt as _;

    unsafe extern "C" {
        fn dup(fd: std::os::raw::c_int) -> std::os::raw::c_int;
        fn close(fd: std::os::raw::c_int) -> std::os::raw::c_int;
        fn fexecve(
            fd: std::os::raw::c_int,
            argv: *const *const std::os::raw::c_char,
            envp: *const *const std::os::raw::c_char,
        ) -> std::os::raw::c_int;
    }
    let argv = std::env::args_os()
        .map(|argument| CString::new(argument.as_os_str().as_bytes()))
        .collect::<Result<Vec<_>, _>>();
    let environment = std::env::vars_os()
        .map(|(key, value)| {
            let mut wire = key.as_os_str().as_bytes().to_vec();
            wire.push(b'=');
            wire.extend_from_slice(value.as_os_str().as_bytes());
            CString::new(wire)
        })
        .collect::<Result<Vec<_>, _>>();
    let (Ok(argv), Ok(environment)) = (argv, environment) else {
        return INVALID_DISPATCH_EXIT;
    };
    let mut argv_pointers = argv.iter().map(|value| value.as_ptr()).collect::<Vec<_>>();
    argv_pointers.push(std::ptr::null());
    let mut environment_pointers = environment
        .iter()
        .map(|value| value.as_ptr())
        .collect::<Vec<_>>();
    environment_pointers.push(std::ptr::null());
    // The held no-follow file was hashed and remains open across this call.
    // dup clears CLOEXEC so shebang interpreters can reopen the script fd.
    // fexecve resolves the executable from that descriptor, never the path.
    let descriptor = unsafe { dup(target.file.as_raw_fd()) };
    if descriptor < 0 {
        return INVALID_DISPATCH_EXIT;
    }
    unsafe {
        fexecve(
            descriptor,
            argv_pointers.as_ptr(),
            environment_pointers.as_ptr(),
        );
        close(descriptor);
    }
    INVALID_DISPATCH_EXIT
}

#[cfg(windows)]
fn execute_target(target: &DispatchTarget) -> i32 {
    target_command(&target.path, std::env::args_os().skip(1))
        .status()
        .ok()
        .and_then(|status| status.code())
        .unwrap_or(INVALID_DISPATCH_EXIT)
}

#[cfg(not(any(unix, windows)))]
fn execute_target(_target: &DispatchTarget) -> i32 {
    INVALID_DISPATCH_EXIT
}

#[cfg(any(windows, test))]
fn target_command(
    target: &Path,
    arguments: impl IntoIterator<Item = OsString>,
) -> std::process::Command {
    let mut command = std::process::Command::new(target);
    command.args(arguments);
    command
}

fn validate_generation(metadata: &GenerationMetadata) -> io::Result<()> {
    if metadata.generation == 0 || metadata.tools.len() > MAX_TOOLS {
        return Err(invalid("profile generation exceeds bounds"));
    }
    let mut identities = BTreeSet::new();
    let mut bins = BTreeSet::new();
    let mut physical_bins = BTreeSet::new();
    for tool in &metadata.tools {
        for value in [
            &tool.name,
            &tool.version,
            &tool.source,
            &tool.reference,
            &tool.store_root,
        ] {
            validate_string(value)?;
        }
        validate_digest(&tool.output_hash)?;
        validate_store_root(&tool.store_root)?;
        if tool.bins.is_empty()
            || tool.bins.len() != tool.members.len()
            || tool.bins.len() != tool.projection_hashes.len()
        {
            return Err(invalid("profile tool has mismatched projection fields"));
        }
        if !identities.insert((&tool.name, &tool.reference)) {
            return Err(invalid("duplicate profile tool identity"));
        }
        for ((bin, member), digest) in tool
            .bins
            .iter()
            .zip(&tool.members)
            .zip(&tool.projection_hashes)
        {
            validate_bin_name(bin)?;
            validate_bin_name(member)?;
            validate_digest(digest)?;
            if !bins.insert(bin)
                || !physical_bins.insert(physical_bin_name_for(bin, true).to_ascii_lowercase())
            {
                return Err(invalid("duplicate profile bin"));
            }
            if bins.len() > MAX_BINS {
                return Err(invalid("profile bin count exceeds bound"));
            }
        }
    }
    if metadata.tools.windows(2).any(|pair| {
        (&pair[0].name, &pair[0].reference) >= (&pair[1].name, &pair[1].reference)
    }) {
        return Err(invalid("profile tools are not in canonical order"));
    }
    Ok(())
}

pub(crate) fn validate_bin_name(value: &str) -> io::Result<()> {
    if value.is_empty() || value.len() > 255 {
        return Err(invalid("profile bin name has invalid length"));
    }
    let mut components = Path::new(value).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+'))
    {
        return Err(invalid("profile bin name is not one normal component"));
    }
    let stem = value.split('.').next().unwrap_or(value).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || stem
            .strip_prefix("COM")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value))
        || stem
            .strip_prefix("LPT")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value));
    if reserved {
        return Err(invalid("profile bin name is reserved on Windows"));
    }
    if value.eq_ignore_ascii_case("jetpack") || value.eq_ignore_ascii_case("jetpack.exe") {
        return Err(invalid("profile bin name collides with the package engine"));
    }
    Ok(())
}

fn validate_store_root(value: &str) -> io::Result<()> {
    validate_string(value)?;
    let path = Path::new(value);
    if !path.is_absolute()
        || path.components().any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("profile Store authority is not absolute and normalized"));
    }
    Ok(())
}

fn validate_digest(value: &str) -> io::Result<()> {
    let hex = value
        .strip_prefix("sha256-")
        .ok_or_else(|| invalid("profile digest is not sha256"))?;
    validate_hex64(hex, "profile digest")
}

fn validate_hex64(value: &str, label: &str) -> io::Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid(format!("{label} is not canonical")));
    }
    Ok(())
}

fn validate_string(value: &str) -> io::Result<()> {
    if value.len() > MAX_STRING || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(invalid("profile string exceeds bounds"));
    }
    Ok(())
}

fn read_bounded_regular(path: &Path) -> io::Result<String> {
    validate_regular_file(path)?;
    let mut file = fs::File::open(path)?;
    let before = file.metadata()?;
    if before.len() > MAX_METADATA_BYTES {
        return Err(invalid("profile metadata exceeds byte bound"));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES || !same_file_metadata(&before, &file.metadata()?) {
        return Err(invalid("profile metadata changed while reading"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("profile metadata is not UTF-8"))
}

fn ensure_directory_chain(path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut cursor = PathBuf::new();
    for component in absolute.components() {
        cursor.push(component.as_os_str());
        if matches!(component, Component::RootDir | Component::Prefix(_)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&cursor)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("profile authority crosses non-directory or symlink"));
        }
        #[cfg(windows)]
        if is_windows_reparse(&metadata) {
            return Err(invalid("profile authority crosses Windows reparse point"));
        }
    }
    Ok(())
}

fn validate_regular_file(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| invalid("profile file has no parent"))?;
    ensure_directory_chain(parent)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("profile authority is not a no-follow regular file"));
    }
    #[cfg(windows)]
    if is_windows_reparse(&metadata) {
        return Err(invalid("profile file is a Windows reparse point"));
    }
    Ok(())
}

fn open_pinned_regular(path: &Path) -> io::Result<fs::File> {
    validate_regular_file(path)?;
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_SHARE_READ: u32 = 1;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)?
    };
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    let before = fs::symlink_metadata(path)?;
    let opened = file.metadata()?;
    if !same_file_metadata(&before, &opened) {
        return Err(invalid("profile file changed while opening"));
    }
    Ok(file)
}

fn sha256_open_file_hex(file: &mut fs::File) -> io::Result<String> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut hasher = SHA256::StreamingSha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(std::io::SeekFrom::Start(0))?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn freeze_unix_projection(source: &mut fs::File, expected: &str) -> io::Result<fs::File> {
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::fs::PermissionsExt as _;

    const MFD_CLOEXEC: u32 = 0x0001;
    const MFD_ALLOW_SEALING: u32 = 0x0002;
    const MFD_EXEC: u32 = 0x0010;
    const F_ADD_SEALS: std::os::raw::c_int = 1033;
    const F_SEAL_SEAL: std::os::raw::c_int = 0x0001;
    const F_SEAL_SHRINK: std::os::raw::c_int = 0x0002;
    const F_SEAL_GROW: std::os::raw::c_int = 0x0004;
    const F_SEAL_WRITE: std::os::raw::c_int = 0x0008;
    const EINVAL: i32 = 22;
    unsafe extern "C" {
        fn fcntl(fd: std::os::raw::c_int, command: std::os::raw::c_int, ...) -> std::os::raw::c_int;
    }
    let name = b"jet-profile-exec\0";
    let mut descriptor = create_memfd(
        name.as_ptr().cast(),
        MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_EXEC,
    );
    if descriptor
        .as_ref()
        .is_err_and(|error| error.raw_os_error() == Some(EINVAL))
    {
        // Kernels predating MFD_EXEC create executable memfds by default.
        descriptor = create_memfd(name.as_ptr().cast(), MFD_CLOEXEC | MFD_ALLOW_SEALING);
    }
    let descriptor = descriptor?;
    let mut frozen = unsafe { fs::File::from_raw_fd(descriptor) };
    source.seek(std::io::SeekFrom::Start(0))?;
    io::copy(source, &mut frozen)?;
    frozen.sync_all()?;
    frozen.set_permissions(fs::Permissions::from_mode(0o500))?;
    let seals = F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL;
    if unsafe { fcntl(frozen.as_raw_fd(), F_ADD_SEALS, seals) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let actual = format!("sha256-{}", sha256_open_file_hex(&mut frozen)?);
    if actual != expected {
        return Err(invalid("profile projection changed while freezing"));
    }
    ensure_open_executable(&frozen)?;
    Ok(frozen)
}

#[cfg(target_os = "linux")]
fn create_memfd(name: *const std::os::raw::c_char, flags: u32) -> io::Result<std::os::raw::c_int> {
    unsafe extern "C" {
        fn memfd_create(name: *const std::os::raw::c_char, flags: u32) -> std::os::raw::c_int;
    }
    let descriptor = unsafe { memfd_create(name, flags) };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(descriptor)
    }
}

#[cfg(target_os = "android")]
fn create_memfd(name: *const std::os::raw::c_char, flags: u32) -> io::Result<std::os::raw::c_int> {
    unsafe extern "C" {
        fn syscall(number: std::os::raw::c_long, ...) -> std::os::raw::c_long;
    }
    let number = android_memfd_syscall_number(std::env::consts::ARCH).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Android architecture has no audited memfd_create syscall number",
        )
    })?;
    let descriptor = unsafe { syscall(number, name, flags as std::os::raw::c_uint) };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(descriptor as std::os::raw::c_int)
    }
}

#[cfg(any(target_os = "android", test))]
fn android_memfd_syscall_number(architecture: &str) -> Option<std::os::raw::c_long> {
    match architecture {
        "aarch64" | "riscv64" => Some(279),
        "arm" => Some(385),
        "x86" => Some(356),
        "x86_64" => Some(319),
        _ => None,
    }
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android"))
))]
fn freeze_unix_projection(_source: &mut fs::File, _expected: &str) -> io::Result<fs::File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "profile dispatch needs a born-anonymous executable image on this platform",
    ))
}

#[cfg(windows)]
fn is_windows_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn ensure_open_executable(file: &fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if file.metadata()?.permissions().mode() & 0o111 == 0 {
            return Err(invalid("profile projection is not executable"));
        }
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

fn same_file_metadata(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        return left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec();
    }
    #[cfg(not(unix))]
    {
        left.len() == right.len() && left.modified().ok() == right.modified().ok()
    }
}

fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        return left.dev() == right.dev() && left.ino() == right.ino();
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        return left.volume_serial_number() == right.volume_serial_number()
            && left.file_index() == right.file_index();
    }
    #[cfg(not(any(unix, windows)))]
    {
        same_file_metadata(left, right)
    }
}

fn copy_file_create_new(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output)?;
    output.sync_all()
}

fn sync_and_match(source: &Path, destination: &Path) -> io::Result<()> {
    fs::File::open(destination)?.sync_all()?;
    if SHA256::sha256_file_hex(source)? != SHA256::sha256_file_hex(destination)? {
        return Err(invalid("dispatcher copy digest mismatch"));
    }
    Ok(())
}

fn remove_regular_partial(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => fs::remove_file(path),
        Ok(_) => Err(invalid("dispatcher partial is not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const MOVEFILE_WRITE_THROUGH: u32 = 8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }
    let mut existing = source.as_os_str().encode_wide().collect::<Vec<_>>();
    existing.push(0);
    let mut replacement = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    replacement.push(0);
    if unsafe {
        MoveFileExW(
            existing.as_ptr(),
            replacement.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            value if value.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", value as u32);
            }
            value => out.push(value),
        }
    }
    out.push('"');
    out
}

fn write_string_array(out: &mut String, key: &str, values: &[String], comma: bool) {
    out.push_str(&format!("      \"{key}\": ["));
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn expect_exact_keys(
    object: &BTreeMap<String, JSON::JSONValue>,
    expected: &[&str],
    label: &str,
) -> io::Result<()> {
    let actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err(invalid(format!("{label} has unknown or missing fields")));
    }
    Ok(())
}

fn string_field<'a>(object: &'a BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<&'a str> {
    object
        .get(key)
        .ok_or_else(|| invalid(format!("missing key `{key}`")))?
        .as_str()
        .map_err(invalid)
}

fn bounded_string(object: &BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<String> {
    let value = string_field(object, key)?;
    validate_string(value)?;
    Ok(value.to_string())
}

fn integer_field(object: &BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<u64> {
    let Some(JSON::JSONValue::Num(value)) = object.get(key) else {
        return Err(invalid(format!("profile field `{key}` is not a number")));
    };
    if !value.is_finite()
        || *value < 0.0
        || value.fract() != 0.0
        || *value > 9_007_199_254_740_991.0
    {
        return Err(invalid(format!("profile field `{key}` is not an exact integer")));
    }
    Ok(*value as u64)
}

fn string_array(object: &BTreeMap<String, JSON::JSONValue>, key: &str) -> io::Result<Vec<String>> {
    let Some(JSON::JSONValue::Array(values)) = object.get(key) else {
        return Err(invalid(format!("profile field `{key}` is not an array")));
    };
    values
        .iter()
        .map(|value| {
            let value = value.as_str().map_err(invalid)?;
            if value.len() > 255 {
                return Err(invalid(format!("profile field `{key}` exceeds bounds")));
            }
            Ok(value.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        format!("sha256-{}", byte.to_string().repeat(64))
    }

    fn metadata() -> GenerationMetadata {
        GenerationMetadata {
            generation: 7,
            created_at: 1,
            tools: vec![GenerationTool {
                name: "echo-args".into(),
                version: "1".into(),
                source: "path".into(),
                reference: "path:echo-args".into(),
                output_hash: digest('a'),
                store_root: if cfg!(windows) { "C:\\store".into() } else { "/store".into() },
                bins: vec!["echo-args".into()],
                members: vec!["echo-args".into()],
                projection_hashes: vec![digest('b')],
            }],
        }
    }

    #[test]
    fn current_pointer_rejects_bitflip_truncation_and_traversal() {
        let pointer = CurrentPointer { generation: 7, witness: digest('a') };
        let wire = format_current_pointer(&pointer).unwrap();
        assert_eq!(parse_current_pointer(&wire).unwrap(), pointer);
        assert!(parse_current_pointer(&wire.replace("generation\t7", "generation\t8")).is_err());
        assert!(parse_current_pointer(wire.trim_end()).is_err());
        assert!(parse_current_pointer(&wire.replace("generation\t7", "generation\t../7")).is_err());
    }

    #[test]
    fn generation_metadata_roundtrips_and_binds_projection() {
        let metadata = metadata();
        let wire = format_generation_metadata(&metadata).unwrap();
        assert_eq!(parse_generation_metadata(&wire, 7).unwrap(), metadata);
        let witness = generation_witness(&wire, &metadata);
        let changed = wire.replace(&digest('b'), &digest('c'));
        let changed_metadata = parse_generation_metadata(&changed, 7).unwrap();
        assert_ne!(witness, generation_witness(&changed, &changed_metadata));
    }

    #[test]
    fn names_reject_traversal_windows_reserved_and_case_collisions() {
        for invalid in ["", "../x", "a/b", "a\\b", "CON", "com1.exe", "nul.txt", "jetpack"] {
            assert!(validate_bin_name(invalid).is_err(), "accepted {invalid:?}");
        }
        let mut case_collision = metadata();
        case_collision.tools.push(GenerationTool {
            name: "other".into(),
            version: "1".into(),
            source: "path".into(),
            reference: "path:other".into(),
            output_hash: digest('c'),
            store_root: case_collision.tools[0].store_root.clone(),
            bins: vec!["ECHO-ARGS".into()],
            members: vec!["other".into()],
            projection_hashes: vec![digest('d')],
        });
        assert!(format_generation_metadata(&case_collision).is_err());

        let mut physical_collision = metadata();
        physical_collision.tools[0].bins = vec!["foo".into()];
        physical_collision.tools.push(GenerationTool {
            name: "other".into(),
            version: "1".into(),
            source: "path".into(),
            reference: "path:other".into(),
            output_hash: digest('c'),
            store_root: physical_collision.tools[0].store_root.clone(),
            bins: vec!["foo.exe".into()],
            members: vec!["other".into()],
            projection_hashes: vec![digest('d')],
        });
        assert!(format_generation_metadata(&physical_collision).is_err());
        assert!(validate_bin_name("jetpack.exe").is_err());
    }

    #[test]
    fn windows_physical_alias_mapping_is_exact_first() {
        assert_eq!(physical_bin_name_for("foo", true), "foo.exe");
        assert_eq!(physical_bin_name_for("foo.exe", true), "foo.exe");
        assert_eq!(physical_bin_name_for("foo.EXE", true), "foo.EXE");
        assert_eq!(physical_bin_name_for("foo", false), "foo");
        assert_eq!(
            path_entries(Path::new("C:\\bin"), Path::new("foo"), true),
            vec![PathBuf::from("C:\\bin/foo"), PathBuf::from("C:\\bin/foo.exe")]
        );
        assert_eq!(
            path_entries(Path::new("/bin"), Path::new("foo"), false),
            vec![PathBuf::from("/bin/foo")]
        );

        let mut metadata = metadata();
        metadata.tools[0].bins = vec!["foo".into()];
        assert_eq!(find_bin(&metadata, "foo").unwrap().1, 0);
        #[cfg(windows)]
        assert_eq!(find_bin(&metadata, "foo.exe").unwrap().1, 0);
    }

    #[test]
    fn android_memfd_syscall_table_is_audited_and_closed() {
        assert_eq!(android_memfd_syscall_number("aarch64"), Some(279));
        assert_eq!(android_memfd_syscall_number("arm"), Some(385));
        assert_eq!(android_memfd_syscall_number("riscv64"), Some(279));
        assert_eq!(android_memfd_syscall_number("x86"), Some(356));
        assert_eq!(android_memfd_syscall_number("x86_64"), Some(319));
        assert_eq!(android_memfd_syscall_number("mips"), None);
    }

    #[test]
    fn native_dispatch_exit_contract_is_stable() {
        assert_eq!(MISSING_DISPATCH_EXIT, 127);
        assert_eq!(INVALID_DISPATCH_EXIT, 126);
    }

    #[test]
    fn native_command_preserves_metacharacter_arguments() {
        let arguments = vec![
            OsString::from("plain"),
            OsString::from("a&b"),
            OsString::from("%PATH%"),
            OsString::from("semi;colon"),
            OsString::from("space value"),
        ];
        let command = target_command(Path::new("tool"), arguments.clone());
        assert_eq!(command.get_args().collect::<Vec<_>>(), arguments.iter().map(OsString::as_os_str).collect::<Vec<_>>());
    }

    #[test]
    fn removed_bin_is_stable_not_found() {
        let metadata = GenerationMetadata {
            generation: 1,
            created_at: 1,
            tools: Vec::new(),
        };
        let error = find_bin(&metadata, "removed").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(MISSING_DISPATCH_EXIT, 127);
    }

    #[test]
    fn native_dispatcher_install_is_synced_exact_binary() {
        let root = std::env::temp_dir().join(format!(
            "jet-profile-dispatch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let installed = install_dispatcher(&bin, "sample-tool").unwrap();
        assert_eq!(
            SHA256::sha256_file_hex(&installed).unwrap(),
            SHA256::sha256_file_hex(&std::env::current_exe().unwrap()).unwrap()
        );
        validate_regular_file(&installed).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replaced_projection_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("jet-profile-link-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("target");
        fs::write(&target, b"tool").unwrap();
        let projection = root.join("projection");
        symlink(&target, &projection).unwrap();
        assert!(validate_regular_file(&projection).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn pinned_projection_handle_survives_path_replacement() {
        let root = std::env::temp_dir().join(format!("jet-profile-pin-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let projection = root.join("projection");
        fs::write(&projection, b"verified").unwrap();
        let mut pinned = open_pinned_regular(&projection).unwrap();
        fs::rename(&projection, root.join("old")).unwrap();
        fs::write(&projection, b"replacement").unwrap();
        assert_eq!(
            sha256_open_file_hex(&mut pinned).unwrap(),
            SHA256::sha256_hex(b"verified")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn path_resolution_binds_inode_not_same_bytes_or_earlier_shadow() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = std::env::temp_dir().join(format!(
            "jet-profile-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let shadow_dir = root.join("shadow");
        let managed_dir = root.join(".jet/bin");
        fs::create_dir_all(&shadow_dir).unwrap();
        fs::create_dir_all(&managed_dir).unwrap();
        let shadow = shadow_dir.join("sample-tool");
        let managed = managed_dir.join("sample-tool");
        fs::write(&shadow, b"same bytes").unwrap();
        fs::write(&managed, b"same bytes").unwrap();
        fs::set_permissions(&shadow, fs::Permissions::from_mode(0o500)).unwrap();
        fs::set_permissions(&managed, fs::Permissions::from_mode(0o500)).unwrap();
        let identity = fs::File::open(&managed).unwrap().metadata().unwrap();
        let resolved = resolve_path_entry(
            Path::new("sample-tool"),
            &identity,
            vec![shadow_dir, managed_dir],
        )
        .unwrap();
        assert_eq!(resolved.0, managed);
        assert!(eligible_dispatcher_entry(&shadow, &identity).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn frozen_projection_never_accepts_concurrent_mutation() {
        use std::io::{Seek as _, Write as _};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Barrier};

        let root = std::env::temp_dir().join(format!(
            "jet-profile-freeze-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let projection = root.join("projection");
        let original = vec![b'a'; 8 * 1024 * 1024];
        fs::write(&projection, &original).unwrap();
        let expected = format!("sha256-{}", SHA256::sha256_hex(&original));
        let mut pinned = open_pinned_regular(&projection).unwrap();
        let start = Arc::new(Barrier::new(2));
        let running = Arc::new(AtomicBool::new(true));
        let writer_start = Arc::clone(&start);
        let writer_running = Arc::clone(&running);
        let writer_path = projection.clone();
        let writer = std::thread::spawn(move || {
            let mut file = fs::OpenOptions::new().write(true).open(writer_path).unwrap();
            writer_start.wait();
            let changed = [b'b'; 64 * 1024];
            while writer_running.load(Ordering::Relaxed) {
                file.seek(std::io::SeekFrom::Start(2 * 1024 * 1024)).unwrap();
                file.write_all(&changed).unwrap();
                file.flush().unwrap();
            }
        });
        start.wait();
        let frozen = freeze_unix_projection(&mut pinned, &expected);
        running.store(false, Ordering::Relaxed);
        writer.join().unwrap();
        if let Ok(mut frozen) = frozen {
            use std::os::unix::fs::MetadataExt as _;
            assert_eq!(frozen.metadata().unwrap().nlink(), 0);
            let mut write_attempt = frozen.try_clone().unwrap();
            assert!(write_attempt.write_all(b"mutate sealed image").is_err());
            fs::write(&projection, vec![b'c'; original.len()]).unwrap();
            assert_eq!(
                format!("sha256-{}", sha256_open_file_hex(&mut frozen).unwrap()),
                expected
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(all(
        unix,
        not(any(target_os = "linux", target_os = "android"))
    ))]
    #[test]
    fn unsupported_unix_dispatch_fails_closed_without_named_fallback() {
        let root = std::env::temp_dir().join(format!(
            "jet-profile-unsupported-{}",
            std::process::id()
        ));
        fs::write(&root, b"executable").unwrap();
        let mut source = fs::File::open(&root).unwrap();
        let error = freeze_unix_projection(&mut source, &digest('a')).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        fs::remove_file(root).unwrap();
    }
}

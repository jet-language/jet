// D-LIB-CALLGRANT1=A: one load-site adapter for pinned `.jetlib` artifacts.
// The caller grants read roots; identity, metadata, and ABI checks complete
// before the native payload is copied to a file or mapped by the OS.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetModScalar {
    Int,
    Float,
    Bool,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetModAccess {
    Read,
    Write,
    Move,
}

impl JetModAccess {
    fn from_tag(tag: u8) -> Result<Self, String> {
        match tag {
            0 => Ok(Self::Read),
            1 => Ok(Self::Write),
            2 => Ok(Self::Move),
            _ => Err(format!("unknown .jetlib access-convention tag {tag}")),
        }
    }
}

#[derive(Clone, Debug)]
struct JetModExport {
    name: String,
    symbol: String,
    scalar: JetModScalar,
    params: u32,
    conventions: Vec<JetModAccess>,
    pointer: usize,
}

struct JetLibHeader<'a> {
    compiler_version: String,
    compiler_build: String,
    library_name: String,
    entry: Option<String>,
    target: String,
    target_triple: String,
    linker_identity: String,
    abi_identity: String,
    abi_version: u32,
    exports: Vec<JetModExport>,
    effects: Vec<String>,
    payload_digest: String,
    payload: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct JetModGrant {
    pub read: Vec<String>,
}

/// D-MOD-ERR1=A: every load refusal is one of four cases. The provider
/// reports it as `<Case>: <reason>`, and Core/Mod/Mod.jet turns the case word
/// into the matching `ModError` case, so every tier shares one mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetModFailure {
    Denied,
    NotFound,
    Invalid,
    Unsupported,
}

impl JetModFailure {
    fn tag(self, reason: impl std::fmt::Display) -> String {
        let case = match self {
            Self::Denied => "Denied",
            Self::NotFound => "NotFound",
            Self::Invalid => "Invalid",
            Self::Unsupported => "Unsupported",
        };
        format!("{case}: {reason}")
    }

    fn from_io(error: &std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::NotFound {
            Self::NotFound
        } else {
            Self::Invalid
        }
    }
}

pub struct JetMod {
    handle: usize,
    exports: Vec<JetModExport>,
    payload_path: std::path::PathBuf,
}

impl Drop for JetMod {
    fn drop(&mut self) {
        if self.handle != 0 {
            jet_mod_native::close(self.handle);
            self.handle = 0;
        }
        let _ = std::fs::remove_file(&self.payload_path);
    }
}

const JETLIB_MAGIC: &[u8] = b"jet-jetlib-v3\0";
const JETLIB_ABI_VERSION: u32 = 2;
const JETLIB_ABI_IDENTITY: &str =
    "jet.library.abi.v2;call=extern-c;scalar=homogeneous;access=read-write-move;text=jet-text-v1;ptr-len=checked-utf8";
const JETLIB_MAX_HEADER_ITEMS: usize = 4096;
const JETLIB_MAX_HEADER_FIELD_BYTES: usize = 1024 * 1024;
const JETLIB_MAX_EXPORT_PARAMS: usize = 4096;

fn jetlib_take<'a>(bytes: &'a [u8], what: &str) -> Result<(&'a [u8], &'a [u8]), String> {
    if bytes.len() < 4 {
        return Err(format!("truncated .jetlib header ({what} length)"));
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let rest = &bytes[4..];
    if len > JETLIB_MAX_HEADER_FIELD_BYTES {
        return Err(format!(".jetlib header {what} is too large"));
    }
    if rest.len() < len {
        return Err(format!("truncated .jetlib header ({what})"));
    }
    Ok((&rest[..len], &rest[len..]))
}

fn jetlib_string<'a>(
    bytes: &'a [u8],
    what: &str,
) -> Result<(String, &'a [u8]), String> {
    let (value, rest) = jetlib_take(bytes, what)?;
    let value = std::str::from_utf8(value)
        .map_err(|_| format!(".jetlib header {what} is not UTF-8"))?;
    Ok((value.to_string(), rest))
}

fn jetlib_optional_string<'a>(
    bytes: &'a [u8],
    what: &str,
) -> Result<(Option<String>, &'a [u8]), String> {
    let Some((&present, rest)) = bytes.split_first() else {
        return Err(format!("truncated .jetlib header ({what} presence)"));
    };
    match present {
        0 => Ok((None, rest)),
        1 => jetlib_string(rest, what).map(|(value, rest)| (Some(value), rest)),
        _ => Err(format!("invalid .jetlib {what} presence tag {present}")),
    }
}

fn jetlib_u32<'a>(bytes: &'a [u8], what: &str) -> Result<(u32, &'a [u8]), String> {
    if bytes.len() < 4 {
        return Err(format!("truncated .jetlib header ({what})"));
    }
    Ok((
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        &bytes[4..],
    ))
}

fn jetlib_count<'a>(bytes: &'a [u8], what: &str) -> Result<(usize, &'a [u8]), String> {
    let (count, rest) = jetlib_u32(bytes, &format!("{what} count"))?;
    let count = usize::try_from(count)
        .map_err(|_| format!(".jetlib {what} count is too large"))?;
    if count > JETLIB_MAX_HEADER_ITEMS {
        return Err(format!(".jetlib header has too many {what} entries"));
    }
    Ok((count, rest))
}

fn jetlib_header(bytes: &[u8]) -> Result<JetLibHeader<'_>, String> {
    let mut rest = bytes
        .strip_prefix(JETLIB_MAGIC)
        .ok_or_else(|| "not a .jetlib artifact (bad magic)".to_string())?;
    let (compiler_version, next) = jetlib_string(rest, "compiler version")?;
    rest = next;
    let (compiler_build, next) = jetlib_string(rest, "compiler build")?;
    rest = next;
    let (library_name, next) = jetlib_string(rest, "library name")?;
    rest = next;
    let (entry, next) = jetlib_optional_string(rest, "output entry")?;
    rest = next;
    let (target, next) = jetlib_string(rest, "target")?;
    rest = next;
    let (target_triple, next) = jetlib_string(rest, "target triple")?;
    rest = next;
    let (linker_identity, next) = jetlib_string(rest, "linker identity")?;
    rest = next;
    let (abi_identity, next) = jetlib_string(rest, "ABI identity")?;
    rest = next;
    let (abi_version, next) = jetlib_u32(rest, "ABI version")?;
    rest = next;

    let (export_count, next) = jetlib_count(rest, "export")?;
    rest = next;
    let mut exports = Vec::with_capacity(export_count);
    for _ in 0..export_count {
        let (name, next) = jetlib_string(rest, "export name")?;
        rest = next;
        let (symbol, next) = jetlib_string(rest, "export symbol")?;
        rest = next;
        let Some((&tag, next)) = rest.split_first() else {
            return Err("truncated .jetlib header (export scalar)".to_string());
        };
        rest = next;
        let scalar = match tag {
            0 => JetModScalar::Int,
            1 => JetModScalar::Float,
            2 => JetModScalar::Bool,
            3 => JetModScalar::Text,
            _ => return Err(format!("unknown .jetlib scalar tag {tag}")),
        };
        let (params, next) = jetlib_u32(rest, "export parameter count")?;
        rest = next;
        let params = usize::try_from(params)
            .map_err(|_| ".jetlib export has too many parameters".to_string())?;
        if params > JETLIB_MAX_EXPORT_PARAMS {
            return Err(".jetlib export has too many parameters".to_string());
        }
        let mut conventions = Vec::with_capacity(params);
        for _ in 0..params {
            let Some((&tag, next)) = rest.split_first() else {
                return Err("truncated .jetlib header (access convention)".to_string());
            };
            rest = next;
            conventions.push(JetModAccess::from_tag(tag)?);
        }
        exports.push(JetModExport {
            name,
            symbol,
            scalar,
            params: params as u32,
            conventions,
            pointer: 0,
        });
    }

    let (effect_count, next) = jetlib_count(rest, "effect")?;
    rest = next;
    let mut effects = Vec::with_capacity(effect_count);
    for _ in 0..effect_count {
        let (effect, next) = jetlib_string(rest, "effect")?;
        rest = next;
        effects.push(effect);
    }

    let (payload_digest, next) = jetlib_string(rest, "payload digest")?;
    rest = next;

    if rest.len() < 8 {
        return Err("truncated .jetlib artifact (payload length)".to_string());
    }
    let payload_len = u64::from_be_bytes(rest[..8].try_into().unwrap());
    let payload_len = usize::try_from(payload_len)
        .map_err(|_| "the .jetlib payload is too large for this host".to_string())?;
    let payload = &rest[8..];
    if payload.len() != payload_len {
        return Err(format!(
            "truncated .jetlib artifact (payload declares {payload_len} bytes, found {})",
            payload.len()
        ));
    }
    Ok(JetLibHeader {
        compiler_version,
        compiler_build,
        library_name,
        entry,
        target,
        target_triple,
        linker_identity,
        abi_identity,
        abi_version,
        exports,
        effects,
        payload_digest,
        payload,
    })
}

fn current_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn payload_digest(payload: &[u8]) -> String {
    let digest = jet_sha256_raw(payload);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("sha256-{hex}")
}

fn valid_payload_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn c_symbol(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert(0, '_');
    }
    out
}

/// Refusals come back tagged (D-MOD-ERR1=A): another compiler, ABI or target
/// is `Unsupported`, an effect outside the grant is `Denied`, and malformed
/// metadata is `Invalid`.
fn check_before_map(header: &JetLibHeader<'_>) -> Result<(), String> {
    use JetModFailure::{Denied, Invalid, Unsupported};
    // Compiler identity is deliberately first. A foreign artifact must fail
    // E1338 before any effect or ABI claim is trusted.
    if header.compiler_version != __JET_COMPILER_VERSION {
        return Err(Unsupported.tag(format!(
            "E1338: library `{}` was built by Jet `{}`, but the loader uses Jet `{}`",
            header.library_name, header.compiler_version, __JET_COMPILER_VERSION
        )));
    }
    if header.abi_version != JETLIB_ABI_VERSION {
        return Err(Unsupported.tag(format!(
            "E1341: library `{}` uses unsupported .jetlib ABI version {}; this loader accepts ABI version {}",
            header.library_name, header.abi_version, JETLIB_ABI_VERSION
        )));
    }
    if header.library_name.is_empty() || header.library_name.contains('\0') {
        return Err(Invalid.tag("E1341: .jetlib metadata has no valid Library name"));
    }
    if header
        .entry
        .as_deref()
        .is_some_and(|entry| entry.is_empty() || entry.contains('\0'))
    {
        return Err(Invalid.tag("E1341: .jetlib metadata has an invalid output entry"));
    }
    for (label, value) in [
        ("compiler build", &header.compiler_build),
        ("target triple", &header.target_triple),
        ("linker identity", &header.linker_identity),
    ] {
        if value.is_empty() || value.contains('\0') {
            return Err(Invalid.tag(format!("E1341: .jetlib metadata has no valid {label}")));
        }
    }
    if header.abi_identity != JETLIB_ABI_IDENTITY {
        return Err(Unsupported.tag(format!(
            "E1341: library `{}` uses unsupported native ABI identity `{}`",
            header.library_name, header.abi_identity
        )));
    }
    if !valid_payload_digest(&header.payload_digest) {
        return Err(Invalid.tag(format!(
            "E1341: library `{}` has no valid native payload digest",
            header.library_name
        )));
    }
    let target = current_target();
    if header.target != target {
        return Err(Unsupported.tag(format!(
            "E1341: library `{}` targets `{}`, but this loader targets `{target}`",
            header.library_name, header.target
        )));
    }
    if header.exports.is_empty() {
        return Err(Invalid.tag(format!(
            "E1341: library `{}` has no exported functions",
            header.library_name
        )));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut symbols = std::collections::BTreeSet::new();
    for export in &header.exports {
        if export.name.is_empty() || export.name.contains('\0') {
            return Err(Invalid.tag("E1341: .jetlib metadata has an invalid export name"));
        }
        if export.symbol.is_empty()
            || export.symbol.contains('\0')
            || export.symbol != c_symbol(&export.name)
        {
            return Err(Invalid.tag(format!(
                "E1341: .jetlib export `{}` has an invalid native symbol `{}`",
                export.name, export.symbol
            )));
        }
        if !names.insert(export.name.clone()) {
            return Err(Invalid.tag(format!(
                "E1341: .jetlib metadata repeats export `{}`",
                export.name
            )));
        }
        if !symbols.insert(export.symbol.clone()) {
            return Err(Invalid.tag(format!(
                "E1341: .jetlib metadata repeats native symbol `{}`",
                export.symbol
            )));
        }
        if export.conventions.len() != export.params as usize
            || export.conventions.len() > JETLIB_MAX_EXPORT_PARAMS
        {
            return Err(Invalid.tag(format!(
                "E1341: .jetlib export `{}` has invalid access-convention metadata",
                export.name
            )));
        }
    }
    for effect in &header.effects {
        // D-LIB-CALLGRANT1=A exposes filesystem roots as the load-site grant;
        // no other ambient effect has a load-site spelling on this surface.
        if effect != "FS" && !effect.starts_with("FS.") {
            return Err(Denied.tag(format!(
                "E1339: library `{}` declares `{effect}`, which this load site does not grant",
                header.library_name
            )));
        }
    }
    Ok(())
}

pub fn jet_mod_call_int(mod_: &JetMod, name: &str, args: &[i64]) -> Result<i64, String> {
    let export = mod_
        .exports
        .iter()
        .find(|export| export.name == name)
        .ok_or_else(|| format!("library has no export `{name}`"))?;
    if export.scalar != JetModScalar::Int {
        return Err(format!(
            "library export `{name}` is not an Int export"
        ));
    }
    if export.params as usize != args.len() {
        return Err(format!(
            "library export `{name}` expects {} arguments, got {}",
            export.params,
            args.len()
        ));
    }
    jet_mod_native::call_int(export.pointer, args)
}

pub fn jet_mod_on_tick(mod_: &JetMod, dt: i64) -> Result<i64, String> {
    jet_mod_call_int(mod_, "on_tick", &[dt])
}

// JET_VETTED_UNSAFE_BEGIN: jet_mod_native
// AUDIT: D-LIB-CALLGRANT1=A's only native boundary. It maps a checked payload
// with the OS loader (dlopen/dlsym/dlclose/dlerror or LoadLibraryW/
// GetProcAddress/FreeLibrary/GetLastError) and calls a resolved Int export
// through its C ABI address. Safe Rust cannot express foreign loader handles
// or calls through a raw function address. Caller invariants: `open` receives
// only a staged payload that already passed the identity, ABI, grant and
// digest checks; each handle is closed exactly once, after its last call;
// `call_int` receives an address `symbol` resolved from a live handle for an
// export whose checked metadata fixes an Int result and `args.len()`
// parameters. Violating them maps unchecked code, unloads code that is still
// called, or calls through a dangling or mistyped pointer: undefined behavior.
mod jet_mod_native {
    #[cfg(unix)]
    mod os {
        use std::ffi::{c_char, c_void, CString};
        use std::os::unix::ffi::OsStrExt;

        #[cfg(target_os = "linux")]
        #[link(name = "dl")]
        unsafe extern "C" {
            fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
            fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
            fn dlclose(handle: *mut c_void) -> i32;
            fn dlerror() -> *mut c_char;
        }

        #[cfg(not(target_os = "linux"))]
        unsafe extern "C" {
            fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
            fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
            fn dlclose(handle: *mut c_void) -> i32;
            fn dlerror() -> *mut c_char;
        }

        const RTLD_NOW: i32 = 2;

        pub(crate) const EXTENSION: &str = if cfg!(target_os = "macos") { "dylib" } else { "so" };

        fn last_error() -> String {
            // SAFETY: `dlerror` returns a process-owned NUL-terminated message
            // or null. It is copied immediately and the pointer is not kept.
            unsafe {
                let ptr = dlerror();
                if ptr.is_null() {
                    return "native loader failed".to_string();
                }
                std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        }

        pub(crate) fn open(path: &std::path::Path) -> Result<usize, String> {
            let path_c = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| "library path contains NUL".to_string())?;
            // SAFETY: the path is NUL-terminated; RTLD_NOW resolves every
            // dependency before any export is called.
            let handle = unsafe { dlopen(path_c.as_ptr(), RTLD_NOW) };
            if handle.is_null() {
                Err(last_error())
            } else {
                Ok(handle as usize)
            }
        }

        pub(crate) fn symbol(handle: usize, symbol: &str) -> Result<usize, String> {
            let symbol = CString::new(symbol)
                .map_err(|_| "native library symbol contains NUL".to_string())?;
            // SAFETY: both calls receive a live handle and a NUL-terminated symbol.
            unsafe {
                dlerror();
                let pointer = dlsym(handle as *mut c_void, symbol.as_ptr());
                if pointer.is_null() {
                    Err(last_error())
                } else {
                    Ok(pointer as usize)
                }
            }
        }

        pub(crate) fn close(handle: usize) {
            // SAFETY: the handle came from `dlopen` and is closed exactly once.
            unsafe {
                let _ = dlclose(handle as *mut c_void);
            }
        }
    }

    #[cfg(windows)]
    mod os {
        use std::ffi::{c_char, c_void, CString};
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LoadLibraryW(path: *const u16) -> *mut c_void;
            fn GetProcAddress(handle: *mut c_void, name: *const c_char) -> *mut c_void;
            fn FreeLibrary(handle: *mut c_void) -> i32;
            fn GetLastError() -> u32;
        }

        pub(crate) const EXTENSION: &str = "dll";

        fn last_error() -> String {
            // SAFETY: GetLastError is thread-local and has no pointer lifetime.
            std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32).to_string()
        }

        pub(crate) fn open(path: &std::path::Path) -> Result<usize, String> {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            // SAFETY: the path is a valid NUL-terminated UTF-16 string.
            let handle = unsafe { LoadLibraryW(wide.as_ptr()) };
            if handle.is_null() {
                Err(last_error())
            } else {
                Ok(handle as usize)
            }
        }

        pub(crate) fn symbol(handle: usize, symbol: &str) -> Result<usize, String> {
            let symbol = CString::new(symbol)
                .map_err(|_| "native library symbol contains NUL".to_string())?;
            // SAFETY: handle is live and symbol is NUL-terminated ASCII metadata.
            let pointer = unsafe { GetProcAddress(handle as *mut c_void, symbol.as_ptr()) };
            if pointer.is_null() {
                Err(last_error())
            } else {
                Ok(pointer as usize)
            }
        }

        pub(crate) fn close(handle: usize) {
            // SAFETY: the handle came from LoadLibraryW and is closed exactly once.
            unsafe {
                let _ = FreeLibrary(handle as *mut c_void);
            }
        }
    }

    pub(super) const SUPPORTED: bool = cfg!(any(unix, windows));

    #[cfg(any(unix, windows))]
    pub(super) use os::{open, symbol, EXTENSION};

    #[cfg(not(any(unix, windows)))]
    pub(super) const EXTENSION: &str = "bin";

    #[cfg(not(any(unix, windows)))]
    pub(super) fn open(_path: &std::path::Path) -> Result<usize, String> {
        Err("native loader is unavailable on this target".to_string())
    }

    #[cfg(not(any(unix, windows)))]
    pub(super) fn symbol(_handle: usize, _symbol: &str) -> Result<usize, String> {
        Err("native loader is unavailable on this target".to_string())
    }

    pub(super) fn close(handle: usize) {
        if handle == 0 {
            return;
        }
        #[cfg(any(unix, windows))]
        os::close(handle);
    }

    pub(super) fn call_int(pointer: usize, args: &[i64]) -> Result<i64, String> {
        if pointer == 0 {
            return Err("native export has a null function address".to_string());
        }
        // The metadata gate has already fixed the scalar and parameter count.
        // Each arm names the exact C ABI signature instead of guessing from a
        // loader-side convention. The language's native scalar surface caps
        // practical calls at eight parameters; metadata lookup stays generic.
        // SAFETY: `pointer` came from `symbol` for the checked export, and the
        // selected function type matches the checked argument count.
        unsafe {
            match args {
                [] => Ok(std::mem::transmute::<usize, extern "C" fn() -> i64>(pointer)()),
                [a] => Ok(std::mem::transmute::<usize, extern "C" fn(i64) -> i64>(pointer)(*a)),
                [a, b] => Ok(std::mem::transmute::<usize, extern "C" fn(i64, i64) -> i64>(
                    pointer,
                )(*a, *b)),
                [a, b, c] => Ok(std::mem::transmute::<
                    usize,
                    extern "C" fn(i64, i64, i64) -> i64,
                >(pointer)(*a, *b, *c)),
                [a, b, c, d] => Ok(std::mem::transmute::<
                    usize,
                    extern "C" fn(i64, i64, i64, i64) -> i64,
                >(pointer)(*a, *b, *c, *d)),
                [a, b, c, d, e] => Ok(std::mem::transmute::<
                    usize,
                    extern "C" fn(i64, i64, i64, i64, i64) -> i64,
                >(pointer)(*a, *b, *c, *d, *e)),
                [a, b, c, d, e, f] => Ok(std::mem::transmute::<
                    usize,
                    extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(*a, *b, *c, *d, *e, *f)),
                [a, b, c, d, e, f, g] => Ok(std::mem::transmute::<
                    usize,
                    extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(*a, *b, *c, *d, *e, *f, *g)),
                [a, b, c, d, e, f, g, h] => Ok(std::mem::transmute::<
                    usize,
                    extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64,
                >(pointer)(*a, *b, *c, *d, *e, *f, *g, *h)),
                _ => Err("native Int export has more than eight parameters".to_string()),
            }
        }
    }
}
// JET_VETTED_UNSAFE_END: jet_mod_native

static JET_MOD_NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A staged payload file that is removed unless a loaded JetMod keeps it.
struct JetModStagedPayload {
    path: Option<std::path::PathBuf>,
}

impl JetModStagedPayload {
    fn keep(mut self) -> std::path::PathBuf {
        self.path.take().expect("staged payload path missing")
    }
}

impl Drop for JetModStagedPayload {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn stage_payload(payload: &[u8]) -> Result<JetModStagedPayload, String> {
    use std::io::Write;
    let serial = JET_MOD_NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "jet-mod-{}-{serial}.{}",
        std::process::id(),
        jet_mod_native::EXTENSION
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("cannot stage library payload: {error}"))?;
    let staged = JetModStagedPayload { path: Some(path) };
    let result = file
        .write_all(payload)
        .map_err(|error| format!("cannot stage library payload: {error}"))
        .and_then(|()| {
            file.flush()
                .map_err(|error| format!("cannot flush library payload: {error}"))
        });
    drop(file);
    result?;
    Ok(staged)
}

fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Tagged (D-MOD-ERR1=A): a symlink or reparse point anywhere on the path is
/// a grant refusal (`Denied`); failing to inspect the path is `Invalid`.
fn reject_reparse_components(path: &std::path::Path, label: &str) -> Result<(), String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| JetModFailure::Invalid.tag(format!("cannot resolve {label}: {error}")))?
            .join(path)
    };
    let mut current = std::path::PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(JetModFailure::Invalid.tag(format!("cannot inspect {label}: {error}")))
            }
        };
        if is_reparse_point(&metadata) {
            return Err(JetModFailure::Denied.tag(format!("{label} contains a symlink or reparse point")));
        }
    }
    Ok(())
}

/// Tagged (D-MOD-ERR1=A): nothing at the path is `NotFound`.
fn canonical_revalidated(
    path: &std::path::Path,
    label: &str,
) -> Result<std::path::PathBuf, String> {
    let resolve = |error: std::io::Error| {
        JetModFailure::from_io(&error)
            .tag(format!("cannot resolve {label} `{}`: {error}", path.display()))
    };
    reject_reparse_components(path, label)?;
    let first = std::fs::canonicalize(path).map_err(resolve)?;
    reject_reparse_components(path, label)?;
    let second = std::fs::canonicalize(path).map_err(resolve)?;
    if first != second {
        return Err(JetModFailure::Invalid.tag(format!("{label} changed while it was being resolved")));
    }
    Ok(second)
}

/// Tagged (D-MOD-ERR1=A): an empty grant or a path outside every read root
/// is `Denied`.
fn granted_path(path: &std::path::Path, grant: &JetModGrant) -> Result<std::path::PathBuf, String> {
    if grant.read.is_empty() {
        return Err(JetModFailure::Denied.tag("Mod.load requires a non-empty `read` grant"));
    }
    let canonical = canonical_revalidated(path, "library path")?;
    if !std::fs::metadata(&canonical)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
    {
        return Err(JetModFailure::Invalid.tag(format!(
            "library path `{}` is not a regular file",
            path.display()
        )));
    }
    for root in &grant.read {
        let root = canonical_revalidated(std::path::Path::new(root), "read grant root")?;
        if !std::fs::metadata(&root)
            .map(|metadata| metadata.is_dir() || metadata.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let root_again = canonical_revalidated(&root, "read grant root")?;
        let canonical_again = canonical_revalidated(path, "library path")?;
        if root != root_again || canonical != canonical_again {
            return Err(JetModFailure::Invalid.tag(
                "library path or read grant root changed while it was being checked",
            ));
        }
        if canonical == root || canonical.strip_prefix(&root).is_ok() {
            return Ok(canonical);
        }
    }
    Err(JetModFailure::Denied.tag(format!(
        "library path `{}` is outside the granted read roots",
        path.display()
    )))
}

/// Every refusal is tagged (D-MOD-ERR1=A): the grant and metadata checks tag
/// their own; a file that cannot be read, parsed, staged or mapped is
/// `Invalid`; a target without a native loader is `Unsupported`.
pub fn jet_mod_load(path: &str, grant: &JetModGrant) -> Result<JetMod, String> {
    if !jet_mod_native::SUPPORTED {
        return Err(JetModFailure::Unsupported.tag(format!(
            "Mod.load is not supported on target `{}`",
            std::env::consts::OS
        )));
    }
    let invalid = |reason: String| JetModFailure::Invalid.tag(reason);
    let source = granted_path(std::path::Path::new(path), grant)?;
    let bytes = std::fs::read(&source).map_err(|error| {
        JetModFailure::from_io(&error)
            .tag(format!("cannot read library `{}`: {error}", source.display()))
    })?;
    let header = jetlib_header(&bytes).map_err(invalid)?;
    check_before_map(&header)?;
    if payload_digest(header.payload) != header.payload_digest {
        return Err(invalid(format!(
            "cannot map library payload: `{}` has a content digest that does not match its metadata",
            header.library_name
        )));
    }
    let staged = stage_payload(header.payload).map_err(invalid)?;
    let staged_path = staged.path.as_deref().expect("staged payload path missing");
    let handle = jet_mod_native::open(staged_path)
        .map_err(|error| invalid(format!("cannot map library payload: {error}")))?;

    let mut exports = header.exports.clone();
    for export in &mut exports {
        match jet_mod_native::symbol(handle, &export.symbol) {
            Ok(pointer) => export.pointer = pointer,
            Err(error) => {
                jet_mod_native::close(handle);
                return Err(invalid(format!(
                    "E1341: library `{}` metadata names `{}`, but the payload has no `{}` export: {error}",
                    header.library_name, export.name, export.symbol
                )));
            }
        }
    }
    if header
        .exports
        .iter()
        .any(|export| export.scalar == JetModScalar::Text)
    {
        if let Err(error) = jet_mod_native::symbol(handle, "jet_text_free") {
            jet_mod_native::close(handle);
            return Err(invalid(format!(
                "E1341: library `{}` has Text exports but no `jet_text_free` allocator release export: {error}",
                header.library_name
            )));
        }
    }
    Ok(JetMod {
        handle,
        exports,
        payload_path: staged.keep(),
    })
}

// The parsed borrow stays internal; metadata moves out and payload bytes
// remain in their one owning buffer until the existing staging operation.
struct JetModLoadContext {
    bytes: Vec<u8>,
    payload_offset: usize,
    header: Option<JetLibHeader<'static>>,
    staged: Option<JetModStagedPayload>,
    handle: usize,
}
impl Drop for JetModLoadContext {
    fn drop(&mut self) {
        if self.handle != 0 {
            jet_mod_native::close(self.handle);
            self.handle = 0;
        }
        // The staged field drops AFTER close, matching load's old error path.
    }
}
thread_local! {
    static JET_MOD_LOAD_CONTEXTS: std::cell::RefCell<Vec<Option<JetModLoadContext>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}
fn jet_mod_with_context<R>(id: i64, f: impl FnOnce(&mut JetModLoadContext) -> R) -> R {
    JET_MOD_LOAD_CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        f(contexts.get_mut((id - 1) as usize).and_then(Option::as_mut).expect("live Core loader context"))
    })
}
fn jet_mod_supported_raw() -> bool { jet_mod_native::SUPPORTED }
fn jet_mod_os_raw() -> String { std::env::consts::OS.to_string() }
fn jet_mod_read_context_raw(path: &String, grant: &JetModGrant) -> Result<i64, String> {
    let source = granted_path(std::path::Path::new(path), grant)?;
    let bytes = std::fs::read(&source).map_err(|error| JetModFailure::from_io(&error)
        .tag(format!("cannot read library `{}`: {error}", source.display())))?;
    Ok(JET_MOD_LOAD_CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let context = JetModLoadContext { bytes, payload_offset: 0, header: None, staged: None, handle: 0 };
        if let Some(index) = contexts.iter().position(Option::is_none) {
            contexts[index] = Some(context);
            (index + 1) as i64
        } else {
            contexts.push(Some(context));
            contexts.len() as i64
        }
    }))
}
fn jet_mod_parse_context_raw(id: i64) -> Result<(), String> {
    jet_mod_with_context(id, |context| {
        let header = jetlib_header(&context.bytes)?;
        context.payload_offset = context.bytes.len() - header.payload.len();
        context.header = Some(JetLibHeader {
            compiler_version: header.compiler_version, compiler_build: header.compiler_build,
            library_name: header.library_name, entry: header.entry, target: header.target,
            target_triple: header.target_triple, linker_identity: header.linker_identity,
            abi_identity: header.abi_identity, abi_version: header.abi_version,
            exports: header.exports, effects: header.effects, payload_digest: header.payload_digest,
            payload: &[],
        });
        Ok(())
    })
}
fn jet_mod_check_context_raw(id: i64) -> Result<(), String> {
    jet_mod_with_context(id, |context| check_before_map(context.header.as_ref().expect("parsed header")))
}
fn jet_mod_context_name_raw(id: i64) -> String {
    jet_mod_with_context(id, |context| context.header.as_ref().expect("parsed header").library_name.clone())
}
fn jet_mod_context_digest_raw(id: i64) -> bool {
    jet_mod_with_context(id, |context| payload_digest(&context.bytes[context.payload_offset..]) ==
        context.header.as_ref().expect("parsed header").payload_digest)
}
fn jet_mod_stage_context_raw(id: i64) -> Result<(), String> {
    jet_mod_with_context(id, |context| {
        context.staged = Some(stage_payload(&context.bytes[context.payload_offset..])?);
        Ok(())
    })
}
fn jet_mod_open_context_raw(id: i64) -> Result<(), String> {
    jet_mod_with_context(id, |context| {
        context.handle = jet_mod_native::open(context.staged.as_ref().expect("staged payload")
            .path.as_deref().expect("staged payload path missing"))?;
        Ok(())
    })
}
fn jet_mod_exports_count_raw(id: i64) -> i64 {
    jet_mod_with_context(id, |context| context.header.as_ref().expect("parsed header").exports.len() as i64)
}
fn jet_mod_export_info_raw(id: i64, index: i64) -> (String, String, bool) {
    jet_mod_with_context(id, |context| {
        let export = &context.header.as_ref().expect("parsed header").exports[index as usize];
        (export.name.clone(), export.symbol.clone(), export.scalar == JetModScalar::Text)
    })
}
fn jet_mod_resolve_export_raw(id: i64, index: i64) -> Result<(), String> {
    jet_mod_with_context(id, |context| {
        let export = &mut context.header.as_mut().expect("parsed header").exports[index as usize];
        export.pointer = jet_mod_native::symbol(context.handle, &export.symbol)?;
        Ok(())
    })
}
fn jet_mod_resolve_symbol_raw(id: i64, symbol: &String) -> Result<(), String> {
    jet_mod_with_context(id, |context| jet_mod_native::symbol(context.handle, symbol).map(|_| ()))
}
fn jet_mod_close_context_raw(id: i64) {
    JET_MOD_LOAD_CONTEXTS.with(|contexts| {
        if let Some(slot) = contexts.borrow_mut().get_mut((id - 1) as usize) { drop(slot.take()); }
    });
}
fn jet_mod_finish_context_raw(id: i64) -> JetMod {
    JET_MOD_LOAD_CONTEXTS.with(|contexts| {
        let mut context = contexts.borrow_mut()[(id - 1) as usize].take().expect("live loader context");
        let module = JetMod {
            handle: context.handle,
            exports: std::mem::take(&mut context.header.as_mut().expect("parsed header").exports),
            payload_path: context.staged.take().expect("staged payload").keep(),
        };
        context.handle = 0;
        module
    })
}

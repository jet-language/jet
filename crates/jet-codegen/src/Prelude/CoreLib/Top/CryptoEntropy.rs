// D-CRYPTO-RNG1=A: single fail-closed cryptographic entropy provider.
// Keep this file std-only: codegen embeds it in native programs and jetpack
// embeds the exact same source in the generated crypto bridge.

mod jet_crypto_entropy {

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum JetCryptoError {
    InvalidLength { operation: &'static str, parameter: &'static str, expected: &'static str, actual: usize },
    InvalidEncoding { operation: &'static str, value_kind: &'static str },
    UnsupportedVersion { operation: &'static str, version: u32 },
    UnsupportedAlgorithm { operation: &'static str, algorithm: String },
    OpenFailed,
    NonContributoryKey,
    OutputLength { operation: &'static str, minimum: usize, maximum: usize, actual: i64 },
    PasswordPolicy { reason: &'static str },
    EntropyUnavailable,
    ResourceUnavailable { resource: &'static str },
    Internal { incident_id: &'static str },
}

impl std::fmt::Display for JetCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength { operation, parameter, expected, actual } => write!(f, "{operation}: {parameter} must be {expected}; got {actual}"),
            Self::InvalidEncoding { operation, value_kind } => write!(f, "{operation}: {value_kind} is not canonical"),
            Self::UnsupportedVersion { operation, version } => write!(f, "{operation}: version {version} is not supported"),
            Self::UnsupportedAlgorithm { operation, algorithm } => write!(f, "{operation}: algorithm {algorithm} is not supported"),
            Self::OpenFailed => f.write_str("encrypted data could not be opened"),
            Self::NonContributoryKey => f.write_str("X25519 peer key does not contribute to a shared secret"),
            Self::OutputLength { operation, minimum, maximum, actual } => write!(f, "{operation}: output length must be {minimum}..{maximum}; got {actual}"),
            Self::PasswordPolicy { .. } => f.write_str("password hash is outside Jet's accepted policy"),
            Self::EntropyUnavailable => f.write_str("the operating system could not provide cryptographic randomness"),
            Self::ResourceUnavailable { resource } => write!(f, "{resource} is unavailable for this cryptographic operation"),
            Self::Internal { incident_id } => write!(f, "Jet could not preserve a cryptographic invariant; incident {incident_id}"),
        }
    }
}

pub type JetCryptoEntropyError = JetCryptoError;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetCryptoEntropyStep {
    Filled(usize),
    Interrupted,
    Failed,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetCryptoWasiAttemptEvent {
    Created,
    ProviderReturned(u16),
    Zeroized,
    Released,
    Returned,
}

#[cfg(not(test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JetCryptoEntropyStep {
    Filled(usize),
    Interrupted,
    Failed,
}

#[cfg(test)]
thread_local! {
    static JET_CRYPTO_ENTROPY_TEST_PROVIDER: std::cell::RefCell<
        Option<Box<dyn FnMut(&mut [u8]) -> JetCryptoEntropyStep>>
    > = std::cell::RefCell::new(None);
    static JET_CRYPTO_ZEROIZE_TEST_OBSERVER: std::cell::RefCell<
        Option<Box<dyn FnMut(&[u8])>>
    > = std::cell::RefCell::new(None);
    static JET_CRYPTO_WASI_ATTEMPT_TEST_OBSERVER: std::cell::RefCell<
        Option<Box<dyn FnMut(JetCryptoWasiAttemptEvent, usize, &[u8])>>
    > = std::cell::RefCell::new(None);
}

#[cfg(test)]
pub fn jet_crypto_entropy_set_test_provider(
    provider: impl FnMut(&mut [u8]) -> JetCryptoEntropyStep + 'static,
) {
    JET_CRYPTO_ENTROPY_TEST_PROVIDER.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(provider));
    });
}

#[cfg(test)]
pub fn jet_crypto_entropy_clear_test_provider() {
    JET_CRYPTO_ENTROPY_TEST_PROVIDER.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

#[cfg(test)]
pub fn jet_crypto_entropy_set_zeroize_test_observer(
    observer: impl FnMut(&[u8]) + 'static,
) {
    JET_CRYPTO_ZEROIZE_TEST_OBSERVER.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(observer));
    });
}

#[cfg(test)]
pub fn jet_crypto_entropy_clear_zeroize_test_observer() {
    JET_CRYPTO_ZEROIZE_TEST_OBSERVER.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

#[cfg(test)]
pub fn jet_crypto_entropy_set_wasi_attempt_test_observer(
    observer: impl FnMut(JetCryptoWasiAttemptEvent, usize, &[u8]) + 'static,
) {
    JET_CRYPTO_WASI_ATTEMPT_TEST_OBSERVER.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(observer));
    });
}

#[cfg(test)]
pub fn jet_crypto_entropy_clear_wasi_attempt_test_observer() {
    JET_CRYPTO_WASI_ATTEMPT_TEST_OBSERVER.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

#[cfg(test)]
fn jet_crypto_entropy_observe_wasi_attempt(
    event: JetCryptoWasiAttemptEvent,
    generation: usize,
    bytes: &[u8],
) {
    JET_CRYPTO_WASI_ATTEMPT_TEST_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut() {
            observer(event, generation, bytes);
        }
    });
}

pub(crate) fn jet_crypto_entropy_zeroize(bytes: &mut [u8]) {
    for byte in &mut *bytes {
        // D-CRYPTO-RNG1 requires a write the optimizer cannot elide.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    #[cfg(test)]
    JET_CRYPTO_ZEROIZE_TEST_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut() {
            observer(bytes);
        }
    });
}

#[derive(Clone)]
pub(crate) struct JetCryptoSecretBytes(Vec<u8>);

impl JetCryptoSecretBytes {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub(crate) fn as_vec(&self) -> &Vec<u8> {
        &self.0
    }
}

impl Drop for JetCryptoSecretBytes {
    fn drop(&mut self) {
        jet_crypto_entropy_zeroize(&mut self.0);
    }
}

#[cfg(test)]
pub fn jet_crypto_entropy_unsupported_for_test(
    out: &mut [u8],
) -> Result<(), JetCryptoEntropyError> {
    jet_crypto_entropy_zeroize(out);
    Err(JetCryptoEntropyError::EntropyUnavailable)
}

fn jet_crypto_entropy_fill_loop(
    out: &mut [u8],
    mut provider: impl FnMut(&mut [u8]) -> JetCryptoEntropyStep,
) -> Result<(), JetCryptoEntropyError> {
    let mut filled = 0usize;
    while filled < out.len() {
        match provider(&mut out[filled..]) {
            JetCryptoEntropyStep::Filled(n) if n > 0 && n <= out.len() - filled => {
                filled += n;
            }
            JetCryptoEntropyStep::Interrupted => {
                jet_crypto_entropy_zeroize(&mut out[filled..]);
            }
            JetCryptoEntropyStep::Filled(_) | JetCryptoEntropyStep::Failed => {
                jet_crypto_entropy_zeroize(out);
                return Err(JetCryptoEntropyError::EntropyUnavailable);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn jet_crypto_entropy_fill_with(
    out: &mut [u8],
    provider: impl FnMut(&mut [u8]) -> JetCryptoEntropyStep,
) -> Result<(), JetCryptoEntropyError> {
    jet_crypto_entropy_fill_loop(out, provider)
}

#[cfg(all(
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn jet_crypto_entropy_fill_native(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    unsafe extern "C" {
        fn getrandom(buffer: *mut std::ffi::c_void, length: usize, flags: u32) -> isize;
    }
    jet_crypto_entropy_fill_loop(out, |suffix| {
        let result = unsafe { getrandom(suffix.as_mut_ptr().cast(), suffix.len(), 0) };
        if result > 0 {
            JetCryptoEntropyStep::Filled(result as usize)
        } else if result < 0 && std::io::Error::last_os_error().raw_os_error() == Some(4) {
            JetCryptoEntropyStep::Interrupted
        } else {
            JetCryptoEntropyStep::Failed
        }
    })
}

#[cfg(all(
    target_os = "macos",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn jet_crypto_entropy_fill_native(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecRandomCopyBytes(rnd: *const std::ffi::c_void, count: usize, bytes: *mut u8) -> i32;
    }
    jet_crypto_entropy_fill_loop(out, |suffix| {
        let status = unsafe { SecRandomCopyBytes(std::ptr::null(), suffix.len(), suffix.as_mut_ptr()) };
        if status == 0 {
            JetCryptoEntropyStep::Filled(suffix.len())
        } else {
            JetCryptoEntropyStep::Failed
        }
    })
}

#[cfg(all(
    target_os = "windows",
    target_env = "msvc",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn jet_crypto_entropy_fill_native(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut std::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    jet_crypto_entropy_fill_loop(out, |suffix| {
        let status = unsafe {
            BCryptGenRandom(std::ptr::null_mut(), suffix.as_mut_ptr(), suffix.len() as u32, 2)
        };
        if status == 0 {
            JetCryptoEntropyStep::Filled(suffix.len())
        } else {
            JetCryptoEntropyStep::Failed
        }
    })
}

#[cfg(all(target_os = "wasi", target_arch = "wasm32"))]
fn jet_crypto_entropy_wasi_attempt(out: &mut [u8]) -> Result<(), u16> {
    #[link(wasm_import_module = "wasi_snapshot_preview1")]
    unsafe extern "C" {
        fn random_get(buffer: *mut u8, length: usize) -> u16;
    }
    let errno = unsafe { random_get(out.as_mut_ptr(), out.len()) };
    if errno == 0 { Ok(()) } else { Err(errno) }
}

#[cfg(any(test, all(target_os = "wasi", target_arch = "wasm32")))]
fn jet_crypto_entropy_wasi_with(
    count: usize,
    mut provider: impl FnMut(&mut [u8]) -> u16,
) -> Result<Vec<u8>, JetCryptoEntropyError> {
    const ERRNO_INTR: u16 = 27;
    for generation in 0..17usize {
        let mut allocation = vec![0u8; count];
        #[cfg(test)]
        jet_crypto_entropy_observe_wasi_attempt(
            JetCryptoWasiAttemptEvent::Created,
            generation,
            &allocation,
        );
        let errno = provider(&mut allocation);
        #[cfg(test)]
        jet_crypto_entropy_observe_wasi_attempt(
            JetCryptoWasiAttemptEvent::ProviderReturned(errno),
            generation,
            &allocation,
        );
        if errno == 0 {
            #[cfg(test)]
            jet_crypto_entropy_observe_wasi_attempt(
                JetCryptoWasiAttemptEvent::Returned,
                generation,
                &allocation,
            );
            return Ok(allocation);
        }
        jet_crypto_entropy_zeroize(&mut allocation);
        #[cfg(test)]
        jet_crypto_entropy_observe_wasi_attempt(
            JetCryptoWasiAttemptEvent::Zeroized,
            generation,
            &allocation,
        );
        drop(allocation);
        #[cfg(test)]
        jet_crypto_entropy_observe_wasi_attempt(
            JetCryptoWasiAttemptEvent::Released,
            generation,
            &[],
        );
        if errno != ERRNO_INTR || generation == 16 {
            return Err(JetCryptoEntropyError::EntropyUnavailable);
        }
    }
    unreachable!()
}

#[cfg(test)]
pub fn jet_crypto_entropy_wasi_with_for_test(
    count: usize,
    provider: impl FnMut(&mut [u8]) -> u16,
) -> Result<Vec<u8>, JetCryptoEntropyError> {
    jet_crypto_entropy_wasi_with(count, provider)
}

#[cfg(not(any(
    all(
        target_os = "linux",
        target_env = "gnu",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(
        target_os = "macos",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(
        target_os = "windows",
        target_env = "msvc",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
    all(target_os = "wasi", target_arch = "wasm32")
)))]
fn jet_crypto_entropy_fill_native(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    jet_crypto_entropy_zeroize(out);
    Err(JetCryptoEntropyError::EntropyUnavailable)
}

#[cfg(not(all(target_os = "wasi", target_arch = "wasm32")))]
fn jet_crypto_entropy_fill_platform(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    #[cfg(test)]
    {
        let scripted = JET_CRYPTO_ENTROPY_TEST_PROVIDER.with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.as_mut()
                .map(|provider| jet_crypto_entropy_fill_loop(out, provider))
        });
        if let Some(result) = scripted {
            return result;
        }
    }
    jet_crypto_entropy_fill_native(out)
}

pub fn jet_crypto_entropy_bytes(count: i64) -> Result<Vec<u8>, JetCryptoEntropyError> {
    if count < 0 {
        return Err(JetCryptoEntropyError::InvalidLength { operation: "core.crypto.random.bytes", parameter: "count", expected: "non-negative", actual: count.unsigned_abs() as usize });
    }
    if count > 1_048_576 {
        return Err(JetCryptoEntropyError::OutputLength { operation: "core.crypto.random.bytes", minimum: 0, maximum: 1_048_576, actual: count });
    }
    if count == 0 {
        return Ok(Vec::new());
    }

    #[cfg(all(target_os = "wasi", target_arch = "wasm32"))]
    {
        return jet_crypto_entropy_wasi_with(count as usize, |out| {
            match jet_crypto_entropy_wasi_attempt(out) {
                Ok(()) => 0,
                Err(errno) => errno,
            }
        });
    }

    #[cfg(not(all(target_os = "wasi", target_arch = "wasm32")))]
    {
        let mut out = vec![0u8; count as usize];
        jet_crypto_entropy_fill_platform(&mut out)?;
        Ok(out)
    }
}

pub fn jet_crypto_entropy_fill(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    let mut fresh = match jet_crypto_entropy_bytes(out.len() as i64) {
        Ok(fresh) => fresh,
        Err(error) => {
            jet_crypto_entropy_zeroize(out);
            return Err(error);
        }
    };
    out.copy_from_slice(&fresh);
    jet_crypto_entropy_zeroize(&mut fresh);
    Ok(())
}
}

pub use jet_crypto_entropy::jet_crypto_entropy_bytes;
#[allow(unused_imports)]
pub use jet_crypto_entropy::JetCryptoError;
#[allow(unused_imports)]
pub(crate) use jet_crypto_entropy::jet_crypto_entropy_zeroize;
#[allow(unused_imports)]
pub(crate) use jet_crypto_entropy::JetCryptoSecretBytes;

#[cfg(test)]
#[allow(unused_imports)]
pub use jet_crypto_entropy::{
    jet_crypto_entropy_clear_test_provider, jet_crypto_entropy_fill_with,
    jet_crypto_entropy_clear_zeroize_test_observer, jet_crypto_entropy_set_test_provider,
    jet_crypto_entropy_set_zeroize_test_observer, jet_crypto_entropy_unsupported_for_test,
    jet_crypto_entropy_clear_wasi_attempt_test_observer,
    jet_crypto_entropy_set_wasi_attempt_test_observer, jet_crypto_entropy_wasi_with_for_test,
    JetCryptoEntropyStep, JetCryptoWasiAttemptEvent,
};

// D-CRYPTO-RNG1=A: the core.crypto.random.bytes shim lives beside its entropy
// provider. It previously sat in Top/Process.rs, which only emits for
// core.process, so a crypto-only program called a symbol that was absent.

// D-CRYPTO-RNG1=A: cryptographic bytes use the shared fail-closed OS provider.
// Edition 2026 keeps this infallible Rust shim; failure takes the registered
// E3001 path and never returns weak or partial bytes.
pub(crate) fn jet_crypto_entropy_fail_closed(
    operation: &str,
    error: JetCryptoEntropyError,
) -> ! {
    let internal = matches!(&error, JetCryptoEntropyError::Internal { .. });
    jet_abort_diagnostic(jet_render_e3001_crypto(
        &format!("{operation}: {error}"),
        internal,
    ))
}

pub(crate) fn jet_std_crypto_random_bytes(n: i64) -> Vec<u8> {
    match jet_crypto_entropy_bytes(n) {
        Ok(bytes) => bytes,
        Err(error) => jet_crypto_entropy_fail_closed("core.crypto.random.bytes", error),
    }
}

fn jet_crypto_uuid_format(bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

pub(crate) fn jet_crypto_uuid_v4_result() -> Result<String, JetCryptoEntropyError> {
    let mut bytes = [0u8; 16];
    jet_crypto_entropy_fill(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(jet_crypto_uuid_format(&bytes))
}

pub(crate) fn jet_crypto_uuid_v7_result(
    timestamp_ms: i64,
) -> Result<String, JetCryptoEntropyError> {
    let mut bytes = [0u8; 16];
    let timestamp = timestamp_ms as u64;
    bytes[0] = (timestamp >> 40) as u8;
    bytes[1] = (timestamp >> 32) as u8;
    bytes[2] = (timestamp >> 24) as u8;
    bytes[3] = (timestamp >> 16) as u8;
    bytes[4] = (timestamp >> 8) as u8;
    bytes[5] = timestamp as u8;
    jet_crypto_entropy_fill(&mut bytes[6..])?;
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(jet_crypto_uuid_format(&bytes))
}

pub(crate) fn jet_crypto_uuid_v4() -> String {
    match jet_crypto_uuid_v4_result() {
        Ok(uuid) => uuid,
        Err(error) => jet_crypto_entropy_fail_closed("core.uuid.v4", error),
    }
}

pub(crate) fn jet_crypto_uuid_v7(timestamp_ms: i64) -> String {
    match jet_crypto_uuid_v7_result(timestamp_ms) {
        Ok(uuid) => uuid,
        Err(error) => jet_crypto_entropy_fail_closed("core.uuid.v7", error),
    }
}

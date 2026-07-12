// D-CRYPTO-RNG1=A: single fail-closed cryptographic entropy provider.
// Keep this file std-only: codegen embeds it in native programs and jetpack
// embeds the exact same source in the generated crypto bridge.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetCryptoEntropyError {
    NegativeLength,
    TooLarge,
    Unavailable,
}

impl std::fmt::Display for JetCryptoEntropyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NegativeLength => "cryptographic random length cannot be negative",
            Self::TooLarge => "one cryptographic random request is limited to 1048576 bytes",
            Self::Unavailable => "the operating system could not provide cryptographic randomness",
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetCryptoEntropyStep {
    Filled(usize),
    Interrupted,
    Failed,
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

fn jet_crypto_entropy_zeroize(bytes: &mut [u8]) {
    for byte in bytes {
        // D-CRYPTO-RNG1 requires a write the optimizer cannot elide.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
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
            JetCryptoEntropyStep::Interrupted => {}
            JetCryptoEntropyStep::Filled(_) | JetCryptoEntropyStep::Failed => {
                jet_crypto_entropy_zeroize(out);
                return Err(JetCryptoEntropyError::Unavailable);
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
    Err(JetCryptoEntropyError::Unavailable)
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
        return Err(JetCryptoEntropyError::NegativeLength);
    }
    if count > 1_048_576 {
        return Err(JetCryptoEntropyError::TooLarge);
    }
    if count == 0 {
        return Ok(Vec::new());
    }

    #[cfg(all(target_os = "wasi", target_arch = "wasm32"))]
    {
        const ERRNO_INTR: u16 = 27;
        for attempt in 0..17 {
            let mut out = vec![0u8; count as usize];
            match jet_crypto_entropy_wasi_attempt(&mut out) {
                Ok(()) => return Ok(out),
                Err(errno) => {
                    jet_crypto_entropy_zeroize(&mut out);
                    if errno != ERRNO_INTR || attempt == 16 {
                        return Err(JetCryptoEntropyError::Unavailable);
                    }
                }
            }
        }
        unreachable!()
    }

    #[cfg(not(all(target_os = "wasi", target_arch = "wasm32")))]
    {
        let mut out = vec![0u8; count as usize];
        jet_crypto_entropy_fill_platform(&mut out)?;
        Ok(out)
    }
}

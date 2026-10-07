// Focused acceptance witness using ONLY a real produced Jet runtime candidate.
use jet_codegen::ParentRuntimeLibrary::{admit, embedded_admission, Refusal};
use std::path::Path;

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        arguments.len(),
        2,
        "usage: parent_runtime_admission OUT EXPECTED_RECEIPT_SHA256"
    );
    let out = Path::new(&arguments[0]);
    let pin = &arguments[1];
    assert_eq!(pin.len(), 64);
    let mut hash = [0u8; 32];
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&pin[index * 2..index * 2 + 2], 16).expect("SHA-256 hex");
    }
    let receipt = std::fs::read(out.join("runtime-shared.json")).expect("producer receipt");
    let embedded =
        embedded_admission().expect("host was built with the actual producer's admission source");
    assert_eq!(embedded.receipt_bytes, receipt.as_slice());
    assert_eq!(embedded.expected_receipt_sha256, hash);
    assert_eq!(
        embedded.expected_library_sha256,
        jet_codegen::SHA256::sha256(
            &std::fs::read(out.join("jet_runtime.so")).expect("actual produced library")
        )
    );
    assert!(!embedded.library_install_relative.starts_with('/'));
    let mut tampered = receipt.clone();
    tampered.push(b' ');
    assert!(matches!(
        admit(&out.join("jet_runtime.so"), &tampered, &hash),
        Err(Refusal::RuntimeLibraryReceiptHashMismatch)
    ));
    assert!(matches!(
        admit(&out.join("missing-runtime.so"), &receipt, &hash),
        Err(Refusal::RuntimeLibraryLoadFailed)
    ));
    let library =
        admit(&out.join("jet_runtime.so"), &receipt, &hash).expect("admitted Jet runtime");
    let again = admit(&out.join("jet_runtime.so"), &receipt, &hash).expect("same runtime identity");
    assert!(
        std::ptr::eq(library, again),
        "same pin must reuse the process runtime without another constructor call"
    );
    assert_eq!(library.receipt_identity(), &hash);
    assert_eq!(
        library.library_identity(),
        &embedded.expected_library_sha256
    );
    assert!(matches!(
        library.symbol("jet_core_shared_0123456789abcdef"),
        Err(Refusal::RuntimeLibraryExportSealMismatch)
    ));
    assert!(matches!(
        admit(&out.join("jet_runtime.so"), &tampered, &hash),
        Err(Refusal::RuntimeLibraryReceiptHashMismatch)
    ));
    let new_hash = jet_codegen::SHA256::sha256(&tampered);
    assert!(matches!(
        admit(&out.join("jet_runtime.so"), &tampered, &new_hash),
        Err(Refusal::RuntimeLibraryIdentityMismatch)
    ));
    // JET_VETTED_UNSAFE_BEGIN: parent_runtime_admission_witness
    // AUDIT: the pinned runtime manifest declares alloc/free's exact C ABI.
    // Successful allocation witnesses the real LINK_INIT allocator initialization.
    let allocate: unsafe extern "C" fn(i64, i64) -> i64 =
        unsafe { std::mem::transmute(library.symbol("jet_rt_alloc").expect("allocator export")) };
    let free: unsafe extern "C" fn(i64, i64, i64) =
        unsafe { std::mem::transmute(library.symbol("jet_rt_free").expect("free export")) };
    let pointer = unsafe { allocate(64, 8) };
    assert_ne!(pointer, 0);
    assert_eq!(pointer & 7, 0);
    unsafe {
        free(pointer, 64, 8);
    }
    // JET_VETTED_UNSAFE_END: parent_runtime_admission_witness
    println!("parent runtime admission: exact seals, allocator constructor, same-identity reuse, named refusals OK");
}

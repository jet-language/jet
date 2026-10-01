//! `jet-aot-rt`: the prebuilt runner behind Cranelift dev builds (#3953).
//!
//! `jet build` without `--release` copies this executable and appends the
//! compiled program image; the copy verifies and runs that image. It is a bin
//! of the root package so it carries the same `JET_RUNNER_BUILD_ID` that the
//! compiler stamps into every image, and a runner from another build refuses
//! the image instead of running it.

// D-ALLOC-PROGRAM1=A: the same host allocator the `jet` executable installs,
// so a program's checked allocator policy behaves identically in every tier.
#[global_allocator]
static JET_HOST_ALLOCATOR: jet_codegen::program_allocator::JetHostProgramAllocator =
    jet_codegen::program_allocator::JetHostProgramAllocator;

fn main() {
    jet_foundation::Diagnostics::install_ice_panic_hook();
    std::process::exit(jet_jit::run_dev_aot_executable(env!("JET_RUNNER_BUILD_ID")));
}

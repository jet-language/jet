// Shared core.sys platform-family fact for AOT, resident JIT, and ambient interpreter.

pub(crate) fn jet_std_os_family() -> String {
    std::env::consts::FAMILY.to_string()
}

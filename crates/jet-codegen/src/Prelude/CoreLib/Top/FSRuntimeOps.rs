// Filesystem operations shared by AOT, JIT, and the interpreter.
//
// The surrounding engine supplies `jet_std`, `jet_fault_should_fail`, and the
// raw `jet_fs_*` kernels. Error classification and fault policy live here.

pub(crate) fn jet_std_fs_rename(
    from: &String,
    to: &String,
) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    jet_fs_rename(from, to)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Write, from, error))
}

pub(crate) fn jet_std_fs_glob(
    pattern: &String,
) -> Result<Vec<String>, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(pattern.clone()),
            "fault injected: FS.Read",
        ));
    }
    jet_fs_glob(pattern)
        .map_err(|error| jet_std::io_error_at(jet_std::IOOperation::Read, pattern, error))
}

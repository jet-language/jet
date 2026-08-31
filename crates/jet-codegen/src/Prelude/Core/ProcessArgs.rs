// Process argument projection shared by AOT, JIT, and interpreter adapters.
// The first argv element identifies the launched program; the public args view
// is a fresh owned list of the remaining values.

pub(crate) fn jet_process_args_view(argv: Vec<String>) -> Vec<String> {
    argv.into_iter().skip(1).collect()
}

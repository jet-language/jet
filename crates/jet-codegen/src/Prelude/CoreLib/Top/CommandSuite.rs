/// D-CMD-OVERRIDE1=C: root adapters expose the shared suite constructors and
/// runners without moving their semantics into an execution engine.
fn jet_test_suite_install(runner: jet_std::JetSuiteRunner) {
    jet_std::jet_test_suite_install(runner)
}
fn jet_test_suite_new() -> jet_std::JetTestSuite {
    jet_std::jet_test_suite_new()
}
fn jet_test_suite_run(suite: &mut jet_std::JetTestSuite) -> i64 {
    jet_std::jet_test_suite_run(suite)
}
fn jet_test_suite_status() -> i64 {
    jet_std::jet_test_suite_status()
}
fn jet_bench_suite_install(runner: jet_std::JetSuiteRunner) {
    jet_std::jet_bench_suite_install(runner)
}
fn jet_bench_suite_new() -> jet_std::JetBenchSuite {
    jet_std::jet_bench_suite_new()
}
fn jet_bench_suite_run(suite: &mut jet_std::JetBenchSuite) -> i64 {
    jet_std::jet_bench_suite_run(suite)
}
fn jet_bench_suite_status() -> i64 {
    jet_std::jet_bench_suite_status()
}

impl JetShow for jet_std::JetTestSuite {
    fn jet_show(&self) -> String {
        format!("TestSuite {{ iteration: {}, result: {} }}", self.iteration, self.result)
    }
}

impl JetShow for jet_std::JetBenchSuite {
    fn jet_show(&self) -> String {
        format!("BenchSuite {{ iteration: {}, result: {} }}", self.iteration, self.result)
    }
}

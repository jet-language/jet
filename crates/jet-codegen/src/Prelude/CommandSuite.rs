/// D-CMD-OVERRIDE1=C: the ordinary value handed to an expert command
/// override.  The callback is installed by the command harness; the value
/// itself stays a small, copyable snapshot so the same Prelude source works
/// in AOT, the resident JIT, and the TIR evaluator.
pub type JetSuiteRunner = fn() -> (i64, i64);

#[derive(Clone, Copy, Debug)]
pub struct JetTestSuite {
    pub iteration: i64,
    pub result: i64,
    pub runner: Option<JetSuiteRunner>,
}

#[derive(Clone, Copy, Debug)]
pub struct JetBenchSuite {
    pub iteration: i64,
    pub result: i64,
    pub runner: Option<JetSuiteRunner>,
}

thread_local! {
    static JET_TEST_RUNNER: std::cell::Cell<Option<JetSuiteRunner>> = const { std::cell::Cell::new(None) };
    static JET_TEST_ITERATION: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    static JET_TEST_RESULT: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    static JET_BENCH_RUNNER: std::cell::Cell<Option<JetSuiteRunner>> = const { std::cell::Cell::new(None) };
    static JET_BENCH_ITERATION: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
    static JET_BENCH_RESULT: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

pub fn jet_test_suite_install(runner: JetSuiteRunner) {
    JET_TEST_RUNNER.with(|slot| slot.set(Some(runner)));
    JET_TEST_ITERATION.with(|slot| slot.set(0));
    JET_TEST_RESULT.with(|slot| slot.set(0));
}

pub fn jet_test_suite_new() -> JetTestSuite {
    JetTestSuite {
        iteration: JET_TEST_ITERATION.with(std::cell::Cell::get),
        result: JET_TEST_RESULT.with(std::cell::Cell::get),
        runner: JET_TEST_RUNNER.with(std::cell::Cell::get),
    }
}

pub fn jet_test_suite_run(suite: &mut JetTestSuite) -> i64 {
    let Some(runner) = suite.runner else {
        return suite.result;
    };
    let (iteration, result) = runner();
    suite.iteration = iteration;
    suite.result = result;
    JET_TEST_ITERATION.with(|slot| slot.set(iteration));
    JET_TEST_RESULT.with(|slot| slot.set(result));
    result
}

pub fn jet_test_suite_status() -> i64 {
    JET_TEST_RESULT.with(std::cell::Cell::get)
}

pub fn jet_bench_suite_install(runner: JetSuiteRunner) {
    JET_BENCH_RUNNER.with(|slot| slot.set(Some(runner)));
    JET_BENCH_ITERATION.with(|slot| slot.set(0));
    JET_BENCH_RESULT.with(|slot| slot.set(0));
}

pub fn jet_bench_suite_new() -> JetBenchSuite {
    JetBenchSuite {
        iteration: JET_BENCH_ITERATION.with(std::cell::Cell::get),
        result: JET_BENCH_RESULT.with(std::cell::Cell::get),
        runner: JET_BENCH_RUNNER.with(std::cell::Cell::get),
    }
}

pub fn jet_bench_suite_run(suite: &mut JetBenchSuite) -> i64 {
    let Some(runner) = suite.runner else {
        return suite.result;
    };
    let (iteration, result) = runner();
    suite.iteration = iteration;
    suite.result = result;
    JET_BENCH_ITERATION.with(|slot| slot.set(iteration));
    JET_BENCH_RESULT.with(|slot| slot.set(result));
    result
}

pub fn jet_bench_suite_status() -> i64 {
    JET_BENCH_RESULT.with(std::cell::Cell::get)
}

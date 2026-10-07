// Native test adapters only marshal checked callbacks and owned runtime handles.
// Outcome classification, capture, expected failures and reporting remain in the
// canonical test prelude shared with emitted harnesses. Native callbacks run on
// scheduler tasks and return before the shared stop rail resumes a panic.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_test {
    use super::*;
    use super::jet_c_abi::{guard, view, JetCString};

    type TestTask = jet_std::JetTask<JetTestOutcome>;
    type Invoke = unsafe extern "C" fn(*mut u8) -> *mut u8;

    struct Callback { env: usize, invoke: usize, release: usize }
    impl Callback {
        unsafe fn call(&self) -> *mut u8 {
            let invoke: Invoke = std::mem::transmute(self.invoke);
            jet_native_callback_body(|| invoke(self.env as *mut u8))
        }
    }
    impl Drop for Callback {
        fn drop(&mut self) {
            unsafe {
                let release: unsafe extern "C" fn(*mut u8) = std::mem::transmute(self.release);
                super::jet_c_abi::jet_rt_session_shield_enter();
                release(self.env as *mut u8);
                super::jet_c_abi::jet_rt_session_shield_leave();
            }
        }
    }

    // `invoke` produces one owned canonical outcome. It is called on a task;
    // genuine fatal traps never return an outcome and invalidate the child.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_test_spawn(env: *mut u8, invoke: usize, release: usize) -> *mut TestTask {
        guard(|| {
            let callback = Callback { env: env as usize, invoke, release };
            Box::into_raw(Box::new(TestTask::spawn(move || {
                let result = callback.call() as *mut JetTestOutcome;
                assert!(!result.is_null(), "native test wrapper returned no outcome");
                *Box::from_raw(result)
            })))
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_test_join(task: *mut TestTask) -> *mut JetTestOutcome {
        guard(|| {
            let task = *Box::from_raw(task);
            match task.join() {
                Ok(outcome) => Box::into_raw(Box::new(outcome)),
                Err(failure) => panic!("native test wrapper failed outside jet_test_run: {failure:?}"),
            }
        })
    }

    // `invoke` returns null for success or one owned diagnostic String for a
    // checked failure. No Rust unwind crosses the callback's native frames.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_test_unit(name: JetCString, expected: u8, env: *mut u8, invoke: usize) -> *mut JetTestOutcome {
        guard(|| {
            jet_test_install_panic_hook();
            let expected = expected != 0;
            let result = jet_evidence_with_expectation(expected, || jet_test_run(expected, || {
                let invoke: Invoke = std::mem::transmute(invoke);
                let error = jet_native_callback_body(|| invoke(env)) as JetCString;
                if error.is_null() { Ok(()) } else { Err(*Box::from_raw(error)) }
            }));
            Box::into_raw(Box::new(jet_test_unit_outcome(view(name).to_owned(), expected, result)))
        })
    }

    // Lossless record marshaling lets the one-child host retain outcomes across
    // a fatal child restart. The host never classifies or formats test outcomes.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_test_outcome_wire(outcome: *mut JetTestOutcome) -> JetCString {
        guard(|| {
            let outcome = Box::from_raw(outcome);
            let flags = u8::from(outcome.ok) | (u8::from(outcome.expected_failure) << 1) | (u8::from(outcome.skipped) << 2);
            use std::fmt::Write;
            let mut wire = String::with_capacity(88 + outcome.name.len() + outcome.stdout.len() + outcome.stderr.len());
            write!(&mut wire, "{flags} {} {} {} ", outcome.name.len(), outcome.stdout.len(), outcome.stderr.len()).expect("writing a String cannot fail");
            match outcome.property_cases {
                Some(cases) => writeln!(&mut wire, "{cases}").expect("writing a String cannot fail"),
                None => wire.push_str("-\n"),
            }
            wire.push_str(&outcome.name);
            wire.push_str(&outcome.stdout);
            wire.push_str(&outcome.stderr);
            Box::into_raw(Box::new(wire))
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_test_finish_wire(wire: JetCString) -> u8 {
        guard(|| {
            let mut wire = view(wire);
            let mut results = Vec::new();
            while !wire.is_empty() {
                let (header, body) = wire.split_once('\n').expect("native test record header is absent");
                let mut fields = header.split(' ');
                let mut field = || fields.next().expect("native test record header has wrong arity");
                let flags: u8 = field().parse().expect("native test record flags are invalid");
                assert!(flags < 8, "native test record flags are invalid");
                let name_len: usize = field().parse().expect("native test name length is invalid");
                let stdout_len: usize = field().parse().expect("native test stdout length is invalid");
                let stderr_len: usize = field().parse().expect("native test stderr length is invalid");
                let case_field = field();
                let cases = if case_field == "-" { None } else { Some(case_field.parse::<u64>().expect("native property count is invalid")) };
                assert!(fields.next().is_none(), "native test record header has wrong arity");
                let (name, body) = body.split_at_checked(name_len).expect("native test name is truncated");
                let (stdout, body) = body.split_at_checked(stdout_len).expect("native test stdout is truncated");
                let (stderr, rest) = body.split_at_checked(stderr_len).expect("native test stderr is truncated");
                results.push(JetTestOutcome { name: name.to_owned(), ok: flags & 1 != 0, expected_failure: flags & 2 != 0, skipped: flags & 4 != 0, stdout: stdout.to_owned(), stderr: stderr.to_owned(), property_cases: cases });
                wire = rest;
            }
            u8::from(jet_test_finish(results))
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_test_print(text: JetCString, _flush: u8) {
        guard(|| jet_test_print(view(text).to_owned()));
    }
}

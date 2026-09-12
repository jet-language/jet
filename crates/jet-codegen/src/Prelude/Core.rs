
/// The ordinary library-defined contract for a checked String-backed type.
/// Sema resolves implementations through the normal trait registry; every
/// execution tier calls these same pure methods.
pub trait CheckedText {
    type Error;
    fn check(text: &String) -> Result<(), Self::Error>;
    fn encode_hole<T: JetShow>(value: &T) -> String;
}

/// D-PERSIST-DEVSTATE1=A: AOT's persistent slot is an interior-mutable
/// Prelude cell. The generated module binding stays safe Rust; the execution
/// engine only reads and writes this one storage abstraction.
pub struct JetPersistCell<T> {
    value: std::sync::Mutex<Option<T>>,
}

impl<T> JetPersistCell<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: std::sync::Mutex::new(Some(value)),
        }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Option<T>> {
        self.value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.guard()
            .as_ref()
            .expect("MIR persistent value is uninitialized")
            .clone()
    }

    pub fn take(&self) -> T {
        self.guard()
            .take()
            .expect("MIR persistent value is uninitialized")
    }

    pub fn set(&self, value: T) {
        *self.guard() = Some(value);
    }
}

impl JetShow for JetDate {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetShow for JetLocalTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetShow for JetPeriod {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetShow for JetDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetShow for JetZone {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetShow for JetZonedDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetDate {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetDate {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetLocalTime {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetLocalTime {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetPeriod {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetPeriod {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetDateTime {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetDateTime {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetZone {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetZone {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDisplay for JetZonedDateTime {
    fn jet_display(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetZonedDateTime {
    fn jet_debug(&self) -> String {
        self.to_string_fmt()
    }
}

// D-TIME-INSTANT-SPLIT1=A: Date/LocalTime/DateTime/ZonedDateTime are fixed
// runtime carriers. Keep their Jet comparison implementations here with the
// carriers, so the optional Core crate does not violate Rust's orphan rule.
fn jet_time_ordering(ordering: std::cmp::Ordering) -> __jet_Ordering {
    match ordering {
        std::cmp::Ordering::Less => __jet_Ordering::__jet_Less,
        std::cmp::Ordering::Equal => __jet_Ordering::__jet_Equal,
        std::cmp::Ordering::Greater => __jet_Ordering::__jet_Greater,
    }
}

impl __jet_Equatable for JetDate {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetDate {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

impl __jet_Equatable for JetLocalTime {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetLocalTime {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

impl __jet_Equatable for JetDateTime {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetDateTime {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.cmp(rhs))
    }
}

// ZonedDateTime `==` is instant plus zone identity. Temporal keeps that
// value equality distinct from its separate `equals` distinction.
impl __jet_Equatable for JetZonedDateTime {
    fn equal(&self, rhs: &Self) -> bool {
        self == rhs
    }
}

impl __jet_Comparable for JetZonedDateTime {
    fn compare(&self, rhs: &Self) -> __jet_Ordering {
        jet_time_ordering(self.instant.cmp(&rhs.instant))
    }
}
// D-PARCAPTURE1=D: one failure-aware wrapper around the shared indexed
// collection scheduler. Chunk boundaries and result order live in the shared
// Prelude collection seam; this wrapper only carries the AOT failure rail.

struct JetParaFailure {
    index: usize,
    payload: Box<dyn std::any::Any + Send + 'static>,
}

enum JetParaRuntimeFailure {
    Simple {
        code: &'static str,
        file: String,
        line: u32,
        fn_name: String,
        src_line: String,
        msg: String,
    },
    Rich {
        file: String,
        line: u32,
        fn_name: String,
        src_line: String,
        col: u32,
        caret_len: u32,
        msg: String,
        locals: String,
    },
    Diagnostic {
        rendered: String,
    },
    Contract {
        file: String,
        line: u32,
        clause_kw: String,
        msg: String,
    },
    SchedulerFatal {
        msg: String,
    },
}

impl JetParaRuntimeFailure {
    fn raise(self) -> ! {
        match self {
            Self::Simple {
                code,
                file,
                line,
                fn_name,
                src_line,
                msg,
            } => jet_runtime_stop_with_context(code, &file, line, &fn_name, &src_line, &msg),
            Self::Rich {
                file,
                line,
                fn_name,
                src_line,
                col,
                caret_len,
                msg,
                locals,
            } => jet_panic_rich(
                &file, line, &fn_name, &src_line, col, caret_len, &msg, &locals,
            ),
            Self::Diagnostic { rendered } => jet_runtime_diagnostic(rendered),
            Self::Contract {
                file,
                line,
                clause_kw,
                msg,
            } => jet_contract_fail(&file, line, &clause_kw, &msg),
            Self::SchedulerFatal { msg } => jet_runtime_stop("E3001", "", 0, &msg),
        }
    }
}

thread_local! {
    pub static JET_PARA_DEFER_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn jet_para_call<R, F>(index: usize, f: F) -> Result<R, JetParaFailure>
where
    F: FnOnce() -> R,
{
    let result = JET_PARA_DEFER_FAILURE.with(|defer| {
        let previous = defer.replace(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        defer.set(previous);
        result
    });
    result.map_err(|payload| JetParaFailure { index, payload })
}

fn jet_para_raise_failure(failure: JetParaFailure) -> ! {
    match failure.payload.downcast::<JetParaRuntimeFailure>() {
        Ok(failure) => (*failure).raise(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn jet_list_para_chunks<R, F>(len: usize, worker_limit: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(std::ops::Range<usize>) -> Result<R, JetParaFailure> + Sync,
{
    #[cfg(jet_para_test_workers)]
    let worker_cap = 3;
    #[cfg(not(jet_para_test_workers))]
    let worker_cap = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let indexed = jet_list_para_chunks_kernel(len, worker_limit, worker_cap, f);
    let mut results = Vec::with_capacity(indexed.len());
    let mut first_failure: Option<JetParaFailure> = None;
    for (_, outcome) in indexed {
        match outcome {
            Ok(result) => results.push(result),
            Err(failure)
                if first_failure
                    .as_ref()
                    .is_none_or(|first| failure.index < first.index) =>
            {
                first_failure = Some(failure);
            }
            Err(_) => {}
        }
    }
    if let Some(failure) = first_failure {
        jet_para_raise_failure(failure);
    }
    results
}

fn jet_list_para_map<T, U, F>(xs: Vec<T>, f: F, limit: i64) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync,
{
    let worker_limit = usize::try_from(limit).unwrap_or(usize::MAX).max(1);
    jet_list_para_chunks(xs.len(), worker_limit, |range| {
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            out.push(jet_para_call(index, || f(&xs[index]))?);
        }
        Ok(out)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn jet_list_para_flags<T, F>(xs: &[T], f: F) -> Vec<bool>
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
{
    jet_list_para_chunks(xs.len(), usize::MAX, |range| {
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            out.push(jet_para_call(index, || f(&xs[index]))?);
        }
        Ok(out)
    })
    .into_iter()
    .flatten()
    .collect()
}

fn jet_list_para_filter<T, F>(xs: Vec<T>, f: F) -> Vec<T>
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
{
    let keep = jet_list_para_flags(&xs, f);
    xs.into_iter()
        .zip(keep)
        .filter_map(|(x, keep)| keep.then_some(x))
        .collect()
}

fn jet_list_para_partition<T, F, R, O>(xs: Vec<T>, f: F, out: O) -> R
where
    T: Sync,
    F: Fn(&T) -> bool + Sync,
    O: FnOnce(Vec<T>, Vec<T>) -> R,
{
    let matches = jet_list_para_flags(&xs, f);
    let mut false_items = Vec::new();
    let mut true_items = Vec::new();
    for (item, matched) in xs.into_iter().zip(matches) {
        if matched {
            true_items.push(item);
        } else {
            false_items.push(item);
        }
    }
    out(false_items, true_items)
}

fn jet_list_para_fold<T, U, S, F, M>(xs: Vec<T>, seed: S, step: F, merge: M) -> U
where
    T: Sync,
    U: Send,
    S: Fn() -> U + Sync,
    F: Fn(&U, &T) -> U + Sync,
    M: Fn(&U, &U) -> U + Sync,
{
    let partials = jet_list_para_chunks(xs.len(), usize::MAX, |range| {
        let start = range.start;
        let mut acc = jet_para_call(start, &seed)?;
        for index in range {
            acc = jet_para_call(index, || step(&acc, &xs[index]))?;
        }
        Ok((start, acc))
    });
    if partials.is_empty() {
        return seed();
    }
    jet_list_para_merge_tree(partials, |(left_index, left), (_, right)| {
        jet_para_call(left_index, || merge(&left, &right))
            .map(|merged| (left_index, merged))
    })
    .unwrap_or_else(|failure| jet_para_raise_failure(failure))
    .1
}


/// D-FRED1=A: bridge allocation checks the byte extent before touching the
/// retained wasm-side buffer. The pointer ABI is 32-bit, so an allocation
/// larger than the addressable linear-memory offset is rejected explicitly.
#[cfg(target_arch = "wasm32")]
#[inline]
fn jet_web_d_fred_checked_len(len: u32, element_size: usize) -> usize {
    let len = len as usize;
    let bytes = len
        .checked_mul(element_size)
        .expect("Web D-FRED input byte size overflow");
    assert!(
        bytes <= u32::MAX as usize,
        "Web D-FRED input exceeds wasm32 address space"
    );
    len
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static JET_WEB_D_FRED_F32_INPUT: std::cell::RefCell<Vec<f32>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_web_d_fred_f32_alloc(len: u32) -> u32 {
    JET_WEB_D_FRED_F32_INPUT.with(|cell| {
        let mut values = cell.borrow_mut();
        let len = jet_web_d_fred_checked_len(len, std::mem::size_of::<f32>());
        values.resize(len, 0.0);
        values.as_mut_ptr() as usize as u32
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_web_d_fred_f32_reduce(ptr: u32, len: u32, seed: f32) -> f32 {
    JET_WEB_D_FRED_F32_INPUT.with(|cell| {
        let values = cell.borrow();
        assert_eq!(ptr as usize, values.as_ptr() as usize, "invalid Web D-FRED input pointer");
        assert!(len as usize <= values.len(), "invalid Web D-FRED input length");
        jet_simd_reduce_fixed(&values[..len as usize], seed)
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_web_d_fred_f32_free(ptr: u32) {
    JET_WEB_D_FRED_F32_INPUT.with(|cell| {
        let mut values = cell.borrow_mut();
        assert_eq!(ptr as usize, values.as_ptr() as usize, "invalid Web D-FRED input pointer");
        values.clear();
        values.shrink_to_fit();
    });
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static JET_WEB_D_FRED_F64_INPUT: std::cell::RefCell<Vec<f64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_web_d_fred_f64_alloc(len: u32) -> u32 {
    JET_WEB_D_FRED_F64_INPUT.with(|cell| {
        let mut values = cell.borrow_mut();
        let len = jet_web_d_fred_checked_len(len, std::mem::size_of::<f64>());
        values.resize(len, 0.0);
        values.as_mut_ptr() as usize as u32
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_web_d_fred_f64_reduce(ptr: u32, len: u32, seed: f64) -> f64 {
    JET_WEB_D_FRED_F64_INPUT.with(|cell| {
        let values = cell.borrow();
        assert_eq!(ptr as usize, values.as_ptr() as usize, "invalid Web D-FRED input pointer");
        assert!(len as usize <= values.len(), "invalid Web D-FRED input length");
        jet_simd_reduce_fixed(&values[..len as usize], seed)
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_web_d_fred_f64_free(ptr: u32) {
    JET_WEB_D_FRED_F64_INPUT.with(|cell| {
        let mut values = cell.borrow_mut();
        assert_eq!(ptr as usize, values.as_ptr() as usize, "invalid Web D-FRED input pointer");
        values.clear();
        values.shrink_to_fit();
    });
}

// D-FIDELITY-API1=A: runtime-global fidelity signal. App code decides policy.
const JET_PERF_DEFAULT_FIDELITY_BITS: u32 = 1065353216; // 1.0f32 bits
static JET_PERF_FIDELITY: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(JET_PERF_DEFAULT_FIDELITY_BITS);
fn jet_perf_fidelity() -> f64 {
    let bits = JET_PERF_FIDELITY.load(std::sync::atomic::Ordering::SeqCst);
    f32::from_bits(bits) as f64
}
fn jet_perf_default_fidelity() -> f64 {
    f32::from_bits(JET_PERF_DEFAULT_FIDELITY_BITS) as f64
}
fn jet_perf_store_fidelity(v: f64) {
    JET_PERF_FIDELITY.store((v as f32).to_bits(), std::sync::atomic::Ordering::SeqCst);
}
fn jet_perf_override_fidelity(v: f64) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 || v > 1.0 {
        return Err(format!(
            "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
            v
        ));
    }
    jet_perf_store_fidelity(v);
    Ok(())
}
fn jet_perf_reset_fidelity() {
    JET_PERF_FIDELITY.store(
        JET_PERF_DEFAULT_FIDELITY_BITS,
        std::sync::atomic::Ordering::SeqCst,
    );
}

impl JetShow for JetHyperLogLog {
    fn jet_show(&self) -> String {
        format!("HyperLogLog(count={})", self.count())
    }
}

impl JetShow for JetTDigest {
    fn jet_show(&self) -> String {
        "TDigest".to_string()
    }
}

impl JetShow for JetCountMinSketch {
    fn jet_show(&self) -> String {
        "CountMinSketch".to_string()
    }
}

impl JetShow for JetReservoirSampler {
    fn jet_show(&self) -> String {
        format!("ReservoirSampler(n={})", self.parts().2)
    }
}

struct JetTestExpectFrame {
    scope: u64,
    expected: Option<String>,
    stop: Option<String>,
}

thread_local! {
    pub static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JET_INTERRUPT_HANDLER_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static JET_TEST_EXPECT_FAIL: std::cell::RefCell<Vec<JetTestExpectFrame>> = const { std::cell::RefCell::new(Vec::new()) };
    static JET_TEST_EXPECT_COMPLETED: std::cell::RefCell<Vec<(u64, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    static JET_TEST_TIMEOUTS: std::cell::RefCell<Vec<(u64, std::time::Instant, i64, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
    static JET_TEST_WHOLE_SKIP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn jet_test_expect_fail_enter_scope(scope: u64, expected: Option<&str>) {
    JET_TEST_EXPECT_FAIL.with(|frames| {
        frames.borrow_mut().push(JetTestExpectFrame {
            scope,
            expected: expected.map(str::to_owned),
            stop: None,
        });
    });
}

pub fn jet_test_expect_fail_leave_scope(scope: u64) -> Option<String> {
    if let Some(result) = JET_TEST_EXPECT_COMPLETED.with(|completed| {
        let mut completed = completed.borrow_mut();
        completed
            .iter()
            .rposition(|(candidate, _)| *candidate == scope)
            .map(|index| completed.remove(index).1)
    }) {
        return Some(result);
    }
    JET_TEST_EXPECT_FAIL.with(|frames| {
        let mut frames = frames.borrow_mut();
        frames
            .iter()
            .rposition(|frame| frame.scope == scope)
            .and_then(|index| frames.remove(index).stop)
    })
}

pub fn jet_test_record_stop(code: &str) {
    JET_TEST_EXPECT_FAIL.with(|frames| {
        for frame in frames.borrow_mut().iter_mut() {
            if frame.stop.is_none() {
                frame.stop = Some(code.to_string());
            }
        }
    });
}

pub fn jet_test_expect_fail_matching_scope() -> Option<u64> {
    JET_TEST_EXPECT_FAIL.with(|frames| {
        frames.borrow().iter().rev().find_map(|frame| {
            let code = frame.stop.as_deref()?;
            (frame.expected.as_deref().is_none_or(|expected| expected == code)).then_some(frame.scope)
        })
    })
}

pub fn jet_test_expect_fail_catch_scope() -> Option<u64> {
    let caught = JET_TEST_EXPECT_FAIL.with(|frames| {
        let mut frames = frames.borrow_mut();
        let index = frames.iter().rposition(|frame| {
            let Some(code) = frame.stop.as_deref() else {
                return false;
            };
            frame.expected.as_deref().is_none_or(|expected| expected == code)
        })?;
        let frame = frames.remove(index);
        frames.truncate(index);
        for parent in frames.iter_mut() {
            parent.stop = None;
        }
        Some((frame.scope, frame.stop.unwrap_or_default(), index))
    });
    if let Some((scope, code, depth)) = caught {
        JET_TEST_TIMEOUTS.with(|timeouts| {
            timeouts.borrow_mut().retain(|(_, _, _, entered_depth)| *entered_depth <= depth);
        });
        JET_TEST_EXPECT_COMPLETED.with(|completed| completed.borrow_mut().push((scope, code)));
        Some(scope)
    } else {
        None
    }
}

pub fn jet_test_expect_fail_abort() {
    JET_TEST_EXPECT_FAIL.with(|frames| frames.borrow_mut().clear());
    JET_TEST_EXPECT_COMPLETED.with(|completed| completed.borrow_mut().clear());
}

pub fn jet_test_expect_fail_unmet(expected: Option<&str>) -> ! {
    let message = jet_test_expect_fail_message(expected);
    jet_runtime_stop("E3001", "", 0, &message)
}

pub fn jet_test_timeout_enter(scope: u64, limit_ns: i64) {
    let depth = JET_TEST_EXPECT_FAIL.with(|frames| frames.borrow().len());
    JET_TEST_TIMEOUTS.with(|timeouts| {
        timeouts
            .borrow_mut()
            .push((scope, std::time::Instant::now(), limit_ns, depth));
    });
}

pub fn jet_test_timeout_leave(scope: u64) -> Option<(i64, i64)> {
    JET_TEST_TIMEOUTS.with(|timeouts| {
        let mut timeouts = timeouts.borrow_mut();
        let index = timeouts.iter().rposition(|(candidate, _, _, _)| *candidate == scope)?;
        let (_, started, limit_ns, _) = timeouts.remove(index);
        let elapsed_ns = started.elapsed().as_nanos().min(i64::MAX as u128) as i64;
        Some((elapsed_ns, limit_ns))
    })
}

pub fn jet_test_timeout_abort() {
    JET_TEST_TIMEOUTS.with(|timeouts| timeouts.borrow_mut().clear());
}

pub fn jet_test_timeout_failure(elapsed_ns: i64, limit_ns: i64) -> ! {
    let message = jet_test_timeout_message(elapsed_ns, limit_ns);
    jet_runtime_stop("E3001", "", 0, &message)
}

pub fn jet_test_skip_scope(whole_test: bool) {
    if whole_test {
        JET_TEST_WHOLE_SKIP.with(|skipped| skipped.set(true));
    }
}

pub fn jet_test_take_whole_skip() -> bool {
    JET_TEST_WHOLE_SKIP.with(|skipped| skipped.replace(false))
}

pub fn jet_test_skip_abort() {
    JET_TEST_WHOLE_SKIP.with(|skipped| skipped.set(false));
}

/// D-FAIL-BREACH1=A: stop before native recursion can exhaust the process
/// stack. The frame is a small Prelude-owned depth token; every engine either
/// emits this call or uses the evaluator's equivalent guard.
struct JetStackFrame;

impl Drop for JetStackFrame {
    fn drop(&mut self) {
        jet_runtime_stack_leave();
    }
}

fn jet_stack_enter(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
) -> JetStackFrame {
    let overflow = jet_runtime_stack_enter();
    if overflow {
        jet_runtime_stop_with_context(
            "E3012",
            file,
            line,
            fn_name,
            src_line,
            &jet_stack_overflow_message(fn_name),
        );
    }
    JetStackFrame
}

pub fn jet_scheduler_task_panic_enter() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(true));
}

pub fn jet_scheduler_task_panic_leave() {
    JET_IN_SCHEDULER_TASK.with(|c| c.set(false));
}

fn jet_scheduler_in_task() -> bool {
    JET_IN_SCHEDULER_TASK.with(|c| c.get())
}

pub fn jet_interrupt_handler_panic_enter() {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
}

pub fn jet_interrupt_handler_panic_leave() {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
}

fn jet_runtime_should_unwind() -> bool {
    jet_scheduler_in_task()
        || jet_interrupt_handler_should_unwind()
        || JET_TEST_EXPECT_FAIL.with(|frames| !frames.borrow().is_empty())
}

fn jet_interrupt_handler_should_unwind() -> bool {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.get() != 0)
}

fn jet_scheduler_panic_should_unwind() -> bool {
    jet_runtime_should_unwind()
}

struct JetRuntimeExit;

struct JetRenderedRuntimeStop {
    rendered: String,
    exit_code: i32,
}

/// Raise a program-side stop through the unwind carrier that THIS frame's
/// catcher reads (I9; D-CONC-FAIL1=A "a joined child panic becomes
/// `.Panicked(reason)`").
///
/// A scheduler task frame is caught inside `Prelude/Scheduler.rs` — by
/// `jet_scheduler_catch_task_unwind` for a spawned body, by
/// `jet_scheduler_wait_without_unwind` for a native wait boundary — and both
/// publish the child value out of a `String` payload
/// (`jet_scheduler_panic_message`, `JetSchedulerWait::Panicked`). So a task
/// frame carries the stop's own message and nothing else: the `reason` in
/// `.Panicked(reason)` is a value the PROGRAM computed, and a runtime that put
/// its own stand-in text there would publish a wrong answer, not a vaguer one.
/// The other tiers already carry the program's message into that same frame —
/// the resident JIT re-raises it (`jet-jit/src/Concurrency.rs`
/// `spawn_with_runtime`, from the `msg` recorded by `jet_jit_rich_panic`), and
/// the interpreter carries it as the child's E0953 `what`
/// (`Codegen/TIR/eval/exprs.rs` `eval_require_failure`).
///
/// Every other unwinding frame keeps the typed report: an `#Interrupt`
/// handler, `jet test`'s expect-fail region, and the top-level
/// `jet_runtime_boundary` each want the whole rendered stop and its exit code,
/// and `jet_runtime_boundary` deliberately treats loose panic text as a host
/// fault rather than a user stop.
fn jet_runtime_stop_unwind(rendered: String, exit_code: i32, message: &str) -> ! {
    if jet_scheduler_in_task() {
        std::panic::resume_unwind(Box::new(message.to_string()));
    }
    std::panic::resume_unwind(Box::new(JetRenderedRuntimeStop {
        rendered,
        exit_code,
    }))
}

/// A user-requested process stop unwinds through the same boundary as a
/// runtime report. This gives guards and deferred resource closes a chance to
/// run before the final native exit.
struct JetExplicitExit {
    code: i32,
}

// Process-edge callbacks live beside the one boundary so a program that does
// not import core.sys still has a complete entry wrapper. The OS adapter only
// registers callbacks; it never installs a second native exit path.
mod jet_runtime_atexit {
    use std::cell::RefCell;

    thread_local! {
        static HANDLERS: RefCell<Vec<Box<dyn Fn() + 'static>>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn register<F>(handler: F)
    where
        F: Fn() + 'static,
    {
        HANDLERS.with(|handlers| {
            let mut handlers = handlers.borrow_mut();
            super::jet_runtime_register_atexit(&mut handlers, Box::new(handler));
        });
    }

    pub(super) fn run() {
        HANDLERS.with(|handlers| {
            let mut handlers = handlers.borrow_mut();
            super::jet_runtime_drain_atexit(&mut handlers, |handler| handler());
        });
    }
}

fn jet_std_os_atexit<F>(handler: std::rc::Rc<F>)
where
    F: Fn() + 'static,
{
    jet_runtime_atexit::register(move || handler());
}

fn jet_std_os_run_atexit() {
    jet_runtime_atexit::run();
}

fn jet_runtime_report_parked_tasks() -> Option<i32> {
    let report = jet_observe_parked_tasks_report()?;
    eprint!("{}", report.rendered);
    Some(report.exit_code)
}

/// The only native process-exit boundary for generated programs. Lexical
/// cleanup has already happened while the stop unwound to this point; process
/// callbacks run here before the final native exit.
fn jet_runtime_process_exit(code: i32, report: Option<&str>) -> ! {
    jet_std_os_run_atexit();
    jet_runtime_report_parked_tasks();
    jet_observe_drain_after_exit();
    if let Some(report) = report {
        eprint!("{report}");
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    std::process::exit(code)
}

fn jet_runtime_boundary<F, T>(run: F) -> T
where
    F: FnOnce() -> T,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => {
            jet_std_os_run_atexit();
            let parked_exit = jet_runtime_report_parked_tasks();
            jet_observe_drain_after_exit();
            if let Some(code) = parked_exit {
                std::process::exit(code);
            }
            value
        }
        Err(payload) => {
            // Only the private foreign-boundary marker below converts a String
            // payload into a user stop. A rendered runtime report crosses this
            // boundary as a typed report or private envelope; arbitrary panic
            // text remains a host/compiler fault.
            //
            // This is the one place the marker is read, and it is the only E3001
            // renderer a shared Prelude source can reach without naming a
            // registered-row adapter. Its producers are boundaries whose source
            // is embedded into crates that carry no such adapter: the generated
            // bridge crate's `ffi_panic` (jet-pkg-model/src/FFI.rs) and the
            // fail-closed OS entropy shim (Prelude/CoreLib/Top/CryptoEntropy.rs,
            // which is also embedded verbatim into that bridge). Those sources
            // must not build a report themselves: the bridge strips Foundation's
            // `jet_render_runtime_stop` wrapper and never regenerates it, so a
            // report built there is a call with no definition (I2 — rustc E0425
            // inside Jet's own generated crate, reported as E0705 against the
            // user's `extern rust` line).
            if let Some(message) = payload
                .downcast_ref::<String>()
                .and_then(|message| message.strip_prefix("__jet_ffi_runtime__: "))
            {
                let report = jet_runtime_stop_report(
                    "E3001", "", 0, "", "", 1, 1, message, "",
                );
                jet_runtime_process_exit(report.exit_code, Some(&report.rendered));
            }
            match payload.downcast::<JetRuntimeDiagnostic>() {
                Ok(report) => {
                    jet_runtime_process_exit(report.exit_code, Some(&report.rendered));
                }
                Err(payload) => match payload.downcast::<JetRenderedRuntimeStop>() {
                    Ok(report) => {
                        jet_runtime_process_exit(report.exit_code, Some(&report.rendered));
                    }
                    Err(payload) => match payload.downcast::<JetExplicitExit>() {
                        Ok(exit) => jet_runtime_process_exit(exit.code, None),
                        Err(payload) if payload.is::<JetRuntimeExit>() => jet_runtime_panic_exit(),
                        Err(payload) => std::panic::resume_unwind(payload),
                    },
                },
            }
        }
    }
}

/// Catch the private marker emitted by a generated foreign bridge while the
/// caller's Jet frame is still active. This keeps FFI stops on the one report
/// carrier and preserves the caller's Jet source facts; the process boundary
/// remains the fallback for bridges that cannot carry a call-site frame.
fn jet_ffi_runtime_call<F, T>(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    run: F,
) -> T
where
    F: FnOnce() -> T,
{
    jet_scheduler_world_reject_uncontrolled("foreign");
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(payload) => {
            if let Some(message) = payload
                .downcast_ref::<String>()
                .and_then(|message| message.strip_prefix("__jet_ffi_runtime__: "))
            {
                let report = jet_runtime_stop_report(
                    "E3014", file, line, fn_name, src_line, 1, 1, message, "",
                );
                jet_runtime_stop_unwind(report.rendered, report.exit_code, message);
            }
            std::panic::resume_unwind(payload)
        }
    }
}


/// The callback edge is below foreign code, so it cannot return a Rust unwind
/// to the caller. Preserve Jet's typed runtime report when one exists and
/// convert every other panic to a terminal E3001 report. Returning a default
/// callback value would fabricate foreign success; aborting this process is
/// the only fail-closed result for a callback with no error channel.
fn jet_ffi_callback_boundary<F, T>(run: F) -> T
where
    F: FnOnce() -> T,
{
    jet_scheduler_world_reject_uncontrolled("foreign");
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(payload) => jet_ffi_callback_panic(payload),
    }
}

fn jet_ffi_callback_panic(payload: Box<dyn std::any::Any + Send>) -> ! {
    match payload.downcast::<JetRuntimeDiagnostic>() {
        Ok(report) => jet_runtime_process_exit(report.exit_code, Some(&report.rendered)),
        Err(payload) => match payload.downcast::<JetRenderedRuntimeStop>() {
            Ok(report) => jet_runtime_process_exit(report.exit_code, Some(&report.rendered)),
            Err(payload) => match payload.downcast::<JetExplicitExit>() {
                Ok(exit) => jet_runtime_process_exit(exit.code, None),
                Err(payload) if payload.is::<JetRuntimeExit>() => jet_runtime_process_exit(70, None),
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("foreign callback panicked");
                    let report = jet_runtime_stop_report(
                        "E3001", "", 0, "", "", 1, 1, message, "",
                    );
                    jet_runtime_process_exit(report.exit_code, Some(&report.rendered));
                }
            },
        },
    }
}

/// A cryptographic random draw is intentionally not replayed by a
/// deterministic world. Route the canonical AOT Core entry through this
/// boundary so it cannot silently fall back to operating-system entropy.
fn jet_std_crypto_random_bytes_controlled(n: i64) -> Vec<u8> {
    jet_scheduler_world_reject_uncontrolled("entropy");
    jet_std_crypto_random_bytes(n)
}

/// The same classification, on the one thread that is NOT the program's entry
/// boundary: the interrupt dispatcher.
///
/// A Jet interrupt handler runs on the dispatcher thread — the count and the
/// drain rule belong to `Prelude/CoreLib/Top/Interrupt.rs`, the thread and the
/// `Arc<dyn Fn>` storage to that file's sibling `jet_os_interrupt` adapter in
/// `Prelude/CoreLib/Top/FSIoEnvOsTesting.rs` — so every control transfer a
/// handler raises unwinds into that thread's `catch_unwind` and can never reach
/// `jet_runtime_boundary` above. Handing those payloads straight to the
/// scheduler's loose-panic reporter, which knows only deadline unwinds and
/// panic text, dropped both meanings a handler can carry: `process.exit`
/// stopped ending the process (a signalled program then ran forever with its
/// handlers already done) and a handler's stop printed no report at all.
///
/// Two of the terminal rules differ from the entry boundary's, on purpose. An
/// explicit exit still ends the process: that is what the program asked for,
/// and which thread asked is not part of the request. A stop only *reports*
/// here: the drain still owes every later handler its turn in registration
/// order, so one failing handler must not cancel the ones registered after it.
///
/// `report_other` is the tail of that chain, and it is a parameter for a
/// structural reason rather than a stylistic one. This file is embedded in
/// EVERY generated program; the payloads outside the list above belong to parts
/// that are not. A blown deadline is `Prelude/Scheduler.rs`'s
/// `JetDeadlineUnwind`, reported by that part's `jet_report_caught_unwind`, and
/// the scheduler ships only when a program reaches it
/// (`Codegen/mod.rs::push_core_runtime`) — never for `Codegen/mod.rs::emit`,
/// `emit_tests`, or the wasm module. So the adapter, the one place that has both
/// parts in scope, names the tail; this file names nothing it does not own, and
/// a program with no scheduler cannot inherit a dangling reference to one (I2).
/// The tail is a required argument, not a leftover payload handed back to a
/// caller that may drop it: dropping it is exactly how a handler's control
/// transfer went unfinished before.
fn jet_interrupt_handler_unwind(
    payload: Box<dyn std::any::Any + Send>,
    report_other: impl FnOnce(Box<dyn std::any::Any + Send>),
) {
    let payload = match payload.downcast::<JetExplicitExit>() {
        Ok(exit) => jet_runtime_process_exit(exit.code, None),
        Err(payload) => payload,
    };
    let payload = match payload.downcast::<JetRuntimeDiagnostic>() {
        Ok(report) => return jet_runtime_caught_stop(&report.rendered),
        Err(payload) => payload,
    };
    match payload.downcast::<JetRenderedRuntimeStop>() {
        Ok(report) => jet_runtime_caught_stop(&report.rendered),
        Err(payload) if payload.is::<JetRuntimeExit>() => jet_runtime_panic_exit(),
        Err(payload) => report_other(payload),
    }
}

fn jet_entry_error_text<E: std::fmt::Display>(error: &E) -> String {
    error.to_string()
}

fn jet_entry_error_text_jet<E: JetDisplay>(error: &E) -> String {
    error.jet_display()
}

fn jet_entry_error_text_show<E: JetShow>(error: &E) -> String {
    error.jet_show()
}

fn jet_entry_report(error: String) -> String {
    jet_journey_report(&error)
}

fn jet_entry_error_exit(error: String) -> ! {
    let report = jet_entry_report(error);
    eprint!("{report}");
    jet_runtime_explicit_exit(1)
}

/// The default `Err` reaches the edge as its structured Prelude value. Keep
/// the report projection intact until this one edge so code, identity, cause,
/// context, conversions, and the source journey cannot be recovered from
/// `Display` text after the fact.
fn jet_entry_error_exit_jet(error: JetErr) -> ! {
    eprint!("{}", jet_error_report(&error).render());
    jet_runtime_explicit_exit(1)
}

// D-FAIL-EDGE1: a selected Service has one native log record at its edge.
// The report stays the same Jet text; only the service transport adds a
// target field and JSON string framing.
fn jet_service_edge_report(error: String) -> ! {
    jet_service_edge_report_rendered(jet_entry_report(error))
}

fn jet_service_edge_report_jet(error: JetErr) -> ! {
    jet_service_edge_report_rendered(jet_error_report(&error).render())
}

fn jet_service_edge_report_rendered(report: String) -> ! {
    let mut quoted = String::with_capacity(report.len() + 2);
    quoted.push('"');
    for ch in report.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            ch if ch.is_control() => quoted.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => quoted.push(ch),
        }
    }
    quoted.push('"');
    eprintln!("{{\"target\":\"service\",\"report\":{quoted}}}");
    jet_runtime_explicit_exit(1)
}

fn jet_runtime_exit() -> ! {
    std::panic::resume_unwind(Box::new(JetRuntimeExit))
}

fn jet_runtime_explicit_exit(code: i64) -> ! {
    std::panic::resume_unwind(Box::new(JetExplicitExit {
        code: jet_runtime_exit_code(code),
    }))
}

fn jet_runtime_panic_exit() -> ! {
    jet_runtime_process_exit(70, None)
}

fn jet_runtime_stop(code: &'static str, file: &str, line: u32, msg: &str) -> ! {
    if code == JET_C_INT_RANGE_CODE {
        jet_entry_error_exit(jet_c_int_range_report());
    }
    jet_runtime_stop_with_context(code, file, line, "", "", msg)
}

fn jet_scheduler_runtime_stop(msg: &str) -> ! {
    jet_runtime_stop("E3001", "", 0, msg)
}

fn jet_scheduler_runtime_stop_with_report(report: String) -> ! {
    jet_test_record_stop("E3001");
    if jet_runtime_should_unwind() {
        std::panic::resume_unwind(Box::new(JetRenderedRuntimeStop {
            rendered: report,
            exit_code: 70,
        }));
    }
    eprint!("{}", report);
    jet_runtime_exit();
}

fn jet_runtime_caught_stop(message: &str) {
    eprint!("{message}");
    if !message.ends_with('\n') {
        eprintln!();
    }
}

fn jet_sentry_runtime_stop(
    code: &'static str,
    file: &str,
    line: u32,
    gate: &str,
    operation: &str,
    obligation: &str,
    obligation_status: &str,
    foreign_component: Option<&str>,
    foreign_fenced: Option<bool>,
    detail: &str,
) -> ! {
    jet_test_record_stop(code);
    jet_proof_record(2, 1, code, detail, file, line);
    let report = jet_render_runtime_sentry_with_context(
        match code {
            "R0801" => "R0801",
            "R0802" => "R0802",
            "R0803" => "R0803",
            _ => "R0801",
        },
        file,
        line,
        gate,
        operation,
        obligation,
        detail,
        obligation_status,
        foreign_component,
        foreign_fenced,
    );
    if jet_runtime_should_unwind() {
        jet_stream_record_failure_report(report.rendered.clone());
    }
    jet_runtime_stop_unwind(report.rendered, report.exit_code, detail)
}

fn jet_runtime_stop_with_context(
    code: &'static str,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    msg: &str,
) -> ! {
    jet_test_record_stop(code);
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Simple {
            code,
            file: file.to_string(),
            line,
            fn_name: fn_name.to_string(),
            src_line: src_line.to_string(),
            msg: msg.to_string(),
        }));
    }
    jet_proof_record(2, 1, code, msg, file, line);
    let _ = jet_production_failure_receipt_write(code, file, line, fn_name);
    let report =
        jet_runtime_stop_report(code, file, line, fn_name, src_line, 1, 1, msg, "");
    if jet_runtime_should_unwind() {
        jet_stream_record_failure_report(report.rendered.clone());
    }
    jet_runtime_stop_unwind(report.rendered, report.exit_code, msg)
}

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    jet_runtime_stop("E3001", file, line, msg)
}

fn jet_arithmetic_stop(file: &str, line: u32, msg: &str) -> ! {
    jet_runtime_stop(JET_ARITHMETIC_CODE, file, line, msg)
}

fn jet_todo_stop(file: &str, line: u32, expected_type: &str) -> ! {
    jet_runtime_stop(
        "E3011",
        file,
        line,
        &jet_todo_message(file, line, expected_type),
    )
}

fn jet_runtime_diagnostic(rendered: String) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Diagnostic { rendered }));
    }
    if jet_interrupt_handler_should_unwind() {
        std::panic::resume_unwind(Box::new(JetRenderedRuntimeStop {
            rendered,
            exit_code: 70,
        }));
    }
    eprintln!("{}", rendered);
    jet_runtime_exit();
}
/// E3005 (D-PREPOST1): a `#Pre`/`#Post` contract clause failed at runtime.
/// `clause_kw` is `"Pre"`/`"Post"`; `msg` is the clause's own message text
/// (the second argument to `#Pre(cond, "msg")`/`#Post(cond, "msg")`).
#[allow(dead_code)] // only called from generated code that has a #Pre/#Post
fn jet_contract_fail(file: &str, line: u32, clause_kw: &str, msg: &str) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Contract {
            file: file.to_string(),
            line,
            clause_kw: clause_kw.to_string(),
            msg: msg.to_string(),
        }));
    }
    let report = jet_contract_report(clause_kw, msg, file, line);
    jet_proof_record(2, 1, "E3005", &report.what, file, line);
    std::panic::resume_unwind(Box::new(report))
}

/// Private structured producer channel used only when `jet prove` launches a
/// test harness. Length framing keeps user strings opaque; terminal text is
/// never parsed as evidence.
fn jet_proof_record(kind: u8, state: u8, name: &str, message: &str, file: &str, line: u32) {
    if state == 1 && jet_test_expect_fail_matching_scope().is_some() {
        jet_evidence_with_expectation(true, || {
            jet_evidence_record_write(kind, state, name, message, file, line);
        });
    } else {
        jet_evidence_record_write(kind, state, name, message, file, line);
    }
}
// D-INTBIG1/D-NUMOPS1: plain arithmetic on a fixed-width integer traps on
// overflow (safe by default) — a silent corruption becomes a caught bug. The
// operation table itself lives in `Core/FixedArithmetic.rs`; this file only
// supplies the typed method adapter used by generated Rust. Exact default
// `Int` uses packed Prelude helpers. Floats and `#Numeric` distinct types keep
// plain Rust operators.
impl JetFixedArithmeticError {
    fn message(self) -> String {
        match self {
            Self::AddOverflow => {
                "This addition overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::SubOverflow => {
                "This subtraction overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::MulOverflow => {
                "This multiplication overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::DivideZero => "divided by zero".to_string(),
            Self::DivisionOverflow => {
                "This division overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::RemainderOverflow => "Attempt to calculate the remainder with overflow".to_string(),
            Self::PowerNegative => {
                "A negative exponent has no whole-number result (make the base a Float to raise it to a negative power)"
                    .to_string()
            }
            Self::PowerOverflow => {
                "This power overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::RotateNegative => "A rotation count cannot be negative".to_string(),
            Self::ShiftOutOfRange {
                direction,
                count,
                bits,
            } => format!(
                "Shifting {direction} by {count} bits is out of range (this type is {bits} bits wide)"
            ),
            Self::UnknownOperation => "This fixed-width arithmetic operation is unsupported".to_string(),
        }
    }
}

#[inline(always)]
fn jet_fixed_error_stop(
    error: JetFixedArithmeticError,
    file: &str,
    line: u32,
) -> ! {
    let message = error.to_string();
    jet_arithmetic_stop(file, line, &message)
}

/// D-INTBIG1/D-NUMOPS1: plain arithmetic on a fixed-width integer traps on
trait JetArith: Copy {
    fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_wrapping_add(self, rhs: Self) -> Self;
    fn jet_wrapping_sub(self, rhs: Self) -> Self;
    fn jet_wrapping_mul(self, rhs: Self) -> Self;
    fn jet_saturating_add(self, rhs: Self) -> Self;
    fn jet_saturating_sub(self, rhs: Self) -> Self;
    fn jet_saturating_mul(self, rhs: Self) -> Self;
    fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self;
    // D-NUMOPS1: a shift by a bit-count `>=` the value's width is undefined in C
    // and a panic in Rust — Jet traps it cleanly instead. The count comes in as
    // an `i128` so any integer width (signed or unsigned) reaches here losslessly.
    fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_rotate_left(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_rotate_right(self, bits: i128, file: &str, line: u32) -> Self;
}

fn jet_fixed_value(result: JetFixedArithmeticResult, file: &str, line: u32) -> i64 {
    match result {
        JetFixedArithmeticResult::Value(value) => value,
        JetFixedArithmeticResult::Absent => jet_arithmetic_stop(
            file,
            line,
            "This checked fixed-width operation has no result",
        ),
        JetFixedArithmeticResult::Trap(error) => {
            let message = error.to_string();
            jet_arithmetic_stop(file, line, &message)
        }
    }
}

macro_rules! jet_arith_impl {
    ($(($t:ty, $signed:expr)),*) => { $(
        impl JetArith for $t {
            #[inline(always)]
            fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self {
                match self.checked_add(rhs) {
                    Some(value) => value,
                    None => jet_arithmetic_stop(
                        file,
                        line,
                        "This addition overflows the value's type (the result is outside its range)",
                    ),
                }
            }
            #[inline(always)]
            fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self {
                match self.checked_sub(rhs) {
                    Some(value) => value,
                    None => jet_arithmetic_stop(
                        file,
                        line,
                        "This subtraction overflows the value's type (the result is outside its range)",
                    ),
                }
            }
            #[inline(always)]
            fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self {
                match self.checked_mul(rhs) {
                    Some(value) => value,
                    None => jet_arithmetic_stop(
                        file,
                        line,
                        "This multiplication overflows the value's type (the result is outside its range)",
                    ),
                }
            }
            #[inline(always)]
            fn jet_wrapping_add(self, rhs: Self) -> Self {
                self.wrapping_add(rhs)
            }
            #[inline(always)]
            fn jet_wrapping_sub(self, rhs: Self) -> Self {
                self.wrapping_sub(rhs)
            }
            #[inline(always)]
            fn jet_wrapping_mul(self, rhs: Self) -> Self {
                self.wrapping_mul(rhs)
            }
            #[inline(always)]
            fn jet_saturating_add(self, rhs: Self) -> Self {
                self.saturating_add(rhs)
            }
            #[inline(always)]
            fn jet_saturating_sub(self, rhs: Self) -> Self {
                self.saturating_sub(rhs)
            }
            #[inline(always)]
            fn jet_saturating_mul(self, rhs: Self) -> Self {
                self.saturating_mul(rhs)
            }
            fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(
                    self as i64, rhs as i128, JET_FIXED_OP_DIV, JET_FIXED_MODE_TRAP,
                    $signed, <$t>::BITS as u8, $signed,
                ), file, line) as $t
            }
            fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(
                    self as i64, rhs as i128, JET_FIXED_OP_REM, JET_FIXED_MODE_TRAP,
                    $signed, <$t>::BITS as u8, $signed,
                ), file, line) as $t
            }
            fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(
                    self as i64, bits, JET_FIXED_OP_SHL, JET_FIXED_MODE_TRAP,
                    $signed, <$t>::BITS as u8, true,
                ), file, line) as $t
            }
            fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(
                    self as i64, bits, JET_FIXED_OP_SHR, JET_FIXED_MODE_TRAP,
                    $signed, <$t>::BITS as u8, true,
                ), file, line) as $t
            }
            fn jet_rotate_left(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(
                    self as i64, bits, JET_FIXED_OP_ROTATE_LEFT, JET_FIXED_MODE_TRAP,
                    $signed, <$t>::BITS as u8, true,
                ), file, line) as $t
            }
            fn jet_rotate_right(self, bits: i128, file: &str, line: u32) -> Self {
                jet_fixed_value(jet_fixed_arithmetic(
                    self as i64, bits, JET_FIXED_OP_ROTATE_RIGHT, JET_FIXED_MODE_TRAP,
                    $signed, <$t>::BITS as u8, true,
                ), file, line) as $t
            }
        }
    )* };
}
jet_arith_impl!(
    (i8, true), (i16, true), (i32, true), (i64, true),
    (u8, false), (u16, false), (u32, false), (u64, false)
);
#[inline(always)]
fn jet_fixed_option<T>(
    result: JetFixedArithmeticResult,
    map: impl FnOnce(i64) -> T,
) -> Option<T> {
    match result {
        JetFixedArithmeticResult::Value(value) => Some(map(value)),
        JetFixedArithmeticResult::Absent | JetFixedArithmeticResult::Trap(_) => None,
    }
}

// These wrappers are intentionally concrete at the Prelude boundary.  The
// operation, overflow mode, and fixed width are encoded by the function item,
// so MIR never passes a runtime operation or type descriptor to this kernel.
macro_rules! jet_fixed_route_kernels {
    (
        $t:ty, $signed:expr, $bits:expr;
        trap: {
            $trap_add:ident, $trap_sub:ident, $trap_mul:ident,
            $trap_div:ident, $trap_rem:ident, $trap_floor_div:ident,
            $trap_mod:ident, $trap_pow:ident, $trap_shl:ident, $trap_shr:ident
        };
        wrapping: {
            $wrapping_add:ident, $wrapping_sub:ident, $wrapping_mul:ident,
            $wrapping_div:ident, $wrapping_pow:ident
        };
        saturating: {
            $saturating_add:ident, $saturating_sub:ident, $saturating_mul:ident,
            $saturating_div:ident, $saturating_pow:ident
        };
        checked: {
            $checked_add:ident, $checked_sub:ident, $checked_mul:ident,
            $checked_div:ident, $checked_rem:ident, $checked_pow:ident
        };
        rotate: {
            $rotate_left:ident, $rotate_right:ident
        }
    ) => {
        #[inline(always)]
        pub(crate) fn $trap_add(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_add(left, right, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_sub(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_sub(left, right, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_mul(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_mul(left, right, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_div(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_div(left, right, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_rem(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_rem(left, right, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_floor_div(left: $t, right: $t, file: &str, line: u32) -> $t {
            jet_fixed_value(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_FLOOR_DIV,
                    JET_FIXED_MODE_TRAP,
                    $signed,
                    $bits,
                    $signed,
                ),
                file,
                line,
            ) as $t
        }
        #[inline(always)]
        pub(crate) fn $trap_mod(left: $t, right: $t, file: &str, line: u32) -> $t {
            jet_fixed_value(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_MOD,
                    JET_FIXED_MODE_TRAP,
                    $signed,
                    $bits,
                    $signed,
                ),
                file,
                line,
            ) as $t
        }
        #[inline(always)]
        pub(crate) fn $trap_pow(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetPow>::jet_pow(left, right as i128, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_shl(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_shl(left, right as i128, file, line)
        }
        #[inline(always)]
        pub(crate) fn $trap_shr(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_shr(left, right as i128, file, line)
        }

        #[inline(always)]
        pub(crate) fn $wrapping_add(left: $t, right: $t) -> $t {
            <$t as JetArith>::jet_wrapping_add(left, right)
        }
        #[inline(always)]
        pub(crate) fn $wrapping_sub(left: $t, right: $t) -> $t {
            <$t as JetArith>::jet_wrapping_sub(left, right)
        }
        #[inline(always)]
        pub(crate) fn $wrapping_mul(left: $t, right: $t) -> $t {
            <$t as JetArith>::jet_wrapping_mul(left, right)
        }
        #[inline(always)]
        pub(crate) fn $wrapping_div(left: $t, right: $t, file: &str, line: u32) -> $t {
            jet_fixed_value(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_DIV,
                    JET_FIXED_MODE_WRAPPING,
                    $signed,
                    $bits,
                    $signed,
                ),
                file,
                line,
            ) as $t
        }
        #[inline(always)]
        pub(crate) fn $wrapping_pow(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetPow>::jet_wrapping_pow(left, right as i128, file, line)
        }

        #[inline(always)]
        pub(crate) fn $saturating_add(left: $t, right: $t) -> $t {
            <$t as JetArith>::jet_saturating_add(left, right)
        }
        #[inline(always)]
        pub(crate) fn $saturating_sub(left: $t, right: $t) -> $t {
            <$t as JetArith>::jet_saturating_sub(left, right)
        }
        #[inline(always)]
        pub(crate) fn $saturating_mul(left: $t, right: $t) -> $t {
            <$t as JetArith>::jet_saturating_mul(left, right)
        }
        #[inline(always)]
        pub(crate) fn $saturating_div(left: $t, right: $t, file: &str, line: u32) -> $t {
            jet_fixed_value(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_DIV,
                    JET_FIXED_MODE_SATURATING,
                    $signed,
                    $bits,
                    $signed,
                ),
                file,
                line,
            ) as $t
        }
        #[inline(always)]
        pub(crate) fn $saturating_pow(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetPow>::jet_saturating_pow(left, right as i128, file, line)
        }

        #[inline(always)]
        pub(crate) fn $checked_add(left: $t, right: $t) -> Option<$t> {
            jet_fixed_option(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_ADD,
                    JET_FIXED_MODE_CHECKED,
                    $signed,
                    $bits,
                    $signed,
                ),
                |value| value as $t,
            )
        }
        #[inline(always)]
        pub(crate) fn $checked_sub(left: $t, right: $t) -> Option<$t> {
            jet_fixed_option(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_SUB,
                    JET_FIXED_MODE_CHECKED,
                    $signed,
                    $bits,
                    $signed,
                ),
                |value| value as $t,
            )
        }
        #[inline(always)]
        pub(crate) fn $checked_mul(left: $t, right: $t) -> Option<$t> {
            jet_fixed_option(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_MUL,
                    JET_FIXED_MODE_CHECKED,
                    $signed,
                    $bits,
                    $signed,
                ),
                |value| value as $t,
            )
        }
        #[inline(always)]
        pub(crate) fn $checked_div(left: $t, right: $t) -> Option<$t> {
            jet_fixed_option(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_DIV,
                    JET_FIXED_MODE_CHECKED,
                    $signed,
                    $bits,
                    $signed,
                ),
                |value| value as $t,
            )
        }
        #[inline(always)]
        pub(crate) fn $checked_rem(left: $t, right: $t) -> Option<$t> {
            jet_fixed_option(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_REM,
                    JET_FIXED_MODE_CHECKED,
                    $signed,
                    $bits,
                    $signed,
                ),
                |value| value as $t,
            )
        }
        #[inline(always)]
        pub(crate) fn $checked_pow(left: $t, right: $t) -> Option<$t> {
            jet_fixed_option(
                jet_fixed_arithmetic(
                    left as i64,
                    right as i128,
                    JET_FIXED_OP_POW,
                    JET_FIXED_MODE_CHECKED,
                    $signed,
                    $bits,
                    $signed,
                ),
                |value| value as $t,
            )
        }

        #[inline(always)]
        pub(crate) fn $rotate_left(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_rotate_left(left, right as i128, file, line)
        }
        #[inline(always)]
        pub(crate) fn $rotate_right(left: $t, right: $t, file: &str, line: u32) -> $t {
            <$t as JetArith>::jet_rotate_right(left, right as i128, file, line)
        }
    };
}

jet_fixed_route_kernels!(
    i8, true, 8;
    trap: {
        jet_i8_trap_add, jet_i8_trap_sub, jet_i8_trap_mul, jet_i8_trap_div,
        jet_i8_trap_rem, jet_i8_trap_floor_div, jet_i8_trap_mod, jet_i8_trap_pow,
        jet_i8_trap_shl, jet_i8_trap_shr
    };
    wrapping: {
        jet_i8_wrapping_add, jet_i8_wrapping_sub, jet_i8_wrapping_mul,
        jet_i8_wrapping_div, jet_i8_wrapping_pow
    };
    saturating: {
        jet_i8_saturating_add, jet_i8_saturating_sub, jet_i8_saturating_mul,
        jet_i8_saturating_div, jet_i8_saturating_pow
    };
    checked: {
        jet_i8_checked_add, jet_i8_checked_sub, jet_i8_checked_mul,
        jet_i8_checked_div, jet_i8_checked_rem, jet_i8_checked_pow
    };
    rotate: { jet_i8_rotate_left, jet_i8_rotate_right }
);
jet_fixed_route_kernels!(
    i16, true, 16;
    trap: {
        jet_i16_trap_add, jet_i16_trap_sub, jet_i16_trap_mul, jet_i16_trap_div,
        jet_i16_trap_rem, jet_i16_trap_floor_div, jet_i16_trap_mod, jet_i16_trap_pow,
        jet_i16_trap_shl, jet_i16_trap_shr
    };
    wrapping: {
        jet_i16_wrapping_add, jet_i16_wrapping_sub, jet_i16_wrapping_mul,
        jet_i16_wrapping_div, jet_i16_wrapping_pow
    };
    saturating: {
        jet_i16_saturating_add, jet_i16_saturating_sub, jet_i16_saturating_mul,
        jet_i16_saturating_div, jet_i16_saturating_pow
    };
    checked: {
        jet_i16_checked_add, jet_i16_checked_sub, jet_i16_checked_mul,
        jet_i16_checked_div, jet_i16_checked_rem, jet_i16_checked_pow
    };
    rotate: { jet_i16_rotate_left, jet_i16_rotate_right }
);
jet_fixed_route_kernels!(
    i32, true, 32;
    trap: {
        jet_i32_trap_add, jet_i32_trap_sub, jet_i32_trap_mul, jet_i32_trap_div,
        jet_i32_trap_rem, jet_i32_trap_floor_div, jet_i32_trap_mod, jet_i32_trap_pow,
        jet_i32_trap_shl, jet_i32_trap_shr
    };
    wrapping: {
        jet_i32_wrapping_add, jet_i32_wrapping_sub, jet_i32_wrapping_mul,
        jet_i32_wrapping_div, jet_i32_wrapping_pow
    };
    saturating: {
        jet_i32_saturating_add, jet_i32_saturating_sub, jet_i32_saturating_mul,
        jet_i32_saturating_div, jet_i32_saturating_pow
    };
    checked: {
        jet_i32_checked_add, jet_i32_checked_sub, jet_i32_checked_mul,
        jet_i32_checked_div, jet_i32_checked_rem, jet_i32_checked_pow
    };
    rotate: { jet_i32_rotate_left, jet_i32_rotate_right }
);
jet_fixed_route_kernels!(
    i64, true, 64;
    trap: {
        jet_i64_trap_add, jet_i64_trap_sub, jet_i64_trap_mul, jet_i64_trap_div,
        jet_i64_trap_rem, jet_i64_trap_floor_div, jet_i64_trap_mod, jet_i64_trap_pow,
        jet_i64_trap_shl, jet_i64_trap_shr
    };
    wrapping: {
        jet_i64_wrapping_add, jet_i64_wrapping_sub, jet_i64_wrapping_mul,
        jet_i64_wrapping_div, jet_i64_wrapping_pow
    };
    saturating: {
        jet_i64_saturating_add, jet_i64_saturating_sub, jet_i64_saturating_mul,
        jet_i64_saturating_div, jet_i64_saturating_pow
    };
    checked: {
        jet_i64_checked_add, jet_i64_checked_sub, jet_i64_checked_mul,
        jet_i64_checked_div, jet_i64_checked_rem, jet_i64_checked_pow
    };
    rotate: { jet_i64_rotate_left, jet_i64_rotate_right }
);
jet_fixed_route_kernels!(
    u8, false, 8;
    trap: {
        jet_u8_trap_add, jet_u8_trap_sub, jet_u8_trap_mul, jet_u8_trap_div,
        jet_u8_trap_rem, jet_u8_trap_floor_div, jet_u8_trap_mod, jet_u8_trap_pow,
        jet_u8_trap_shl, jet_u8_trap_shr
    };
    wrapping: {
        jet_u8_wrapping_add, jet_u8_wrapping_sub, jet_u8_wrapping_mul,
        jet_u8_wrapping_div, jet_u8_wrapping_pow
    };
    saturating: {
        jet_u8_saturating_add, jet_u8_saturating_sub, jet_u8_saturating_mul,
        jet_u8_saturating_div, jet_u8_saturating_pow
    };
    checked: {
        jet_u8_checked_add, jet_u8_checked_sub, jet_u8_checked_mul,
        jet_u8_checked_div, jet_u8_checked_rem, jet_u8_checked_pow
    };
    rotate: { jet_u8_rotate_left, jet_u8_rotate_right }
);
jet_fixed_route_kernels!(
    u16, false, 16;
    trap: {
        jet_u16_trap_add, jet_u16_trap_sub, jet_u16_trap_mul, jet_u16_trap_div,
        jet_u16_trap_rem, jet_u16_trap_floor_div, jet_u16_trap_mod, jet_u16_trap_pow,
        jet_u16_trap_shl, jet_u16_trap_shr
    };
    wrapping: {
        jet_u16_wrapping_add, jet_u16_wrapping_sub, jet_u16_wrapping_mul,
        jet_u16_wrapping_div, jet_u16_wrapping_pow
    };
    saturating: {
        jet_u16_saturating_add, jet_u16_saturating_sub, jet_u16_saturating_mul,
        jet_u16_saturating_div, jet_u16_saturating_pow
    };
    checked: {
        jet_u16_checked_add, jet_u16_checked_sub, jet_u16_checked_mul,
        jet_u16_checked_div, jet_u16_checked_rem, jet_u16_checked_pow
    };
    rotate: { jet_u16_rotate_left, jet_u16_rotate_right }
);
jet_fixed_route_kernels!(
    u32, false, 32;
    trap: {
        jet_u32_trap_add, jet_u32_trap_sub, jet_u32_trap_mul, jet_u32_trap_div,
        jet_u32_trap_rem, jet_u32_trap_floor_div, jet_u32_trap_mod, jet_u32_trap_pow,
        jet_u32_trap_shl, jet_u32_trap_shr
    };
    wrapping: {
        jet_u32_wrapping_add, jet_u32_wrapping_sub, jet_u32_wrapping_mul,
        jet_u32_wrapping_div, jet_u32_wrapping_pow
    };
    saturating: {
        jet_u32_saturating_add, jet_u32_saturating_sub, jet_u32_saturating_mul,
        jet_u32_saturating_div, jet_u32_saturating_pow
    };
    checked: {
        jet_u32_checked_add, jet_u32_checked_sub, jet_u32_checked_mul,
        jet_u32_checked_div, jet_u32_checked_rem, jet_u32_checked_pow
    };
    rotate: { jet_u32_rotate_left, jet_u32_rotate_right }
);
jet_fixed_route_kernels!(
    u64, false, 64;
    trap: {
        jet_u64_trap_add, jet_u64_trap_sub, jet_u64_trap_mul, jet_u64_trap_div,
        jet_u64_trap_rem, jet_u64_trap_floor_div, jet_u64_trap_mod, jet_u64_trap_pow,
        jet_u64_trap_shl, jet_u64_trap_shr
    };
    wrapping: {
        jet_u64_wrapping_add, jet_u64_wrapping_sub, jet_u64_wrapping_mul,
        jet_u64_wrapping_div, jet_u64_wrapping_pow
    };
    saturating: {
        jet_u64_saturating_add, jet_u64_saturating_sub, jet_u64_saturating_mul,
        jet_u64_saturating_div, jet_u64_saturating_pow
    };
    checked: {
        jet_u64_checked_add, jet_u64_checked_sub, jet_u64_checked_mul,
        jet_u64_checked_div, jet_u64_checked_rem, jet_u64_checked_pow
    };
    rotate: { jet_u64_rotate_left, jet_u64_rotate_right }
);
/// E3001 (E2-M12, D-OBS1/D-OBS2): rich panic report — includes the function name,
/// a source-line context box, and (in debug builds only) safe local variable values.
/// `col` is 1-based; `caret_len` covers the highlighted span in the source line.
/// `locals` is an empty string in release builds; "x = 1, y = false" in debug builds.
fn jet_panic_rich(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    msg: &str,
    locals: &str,
) -> ! {
    jet_test_record_stop("E3001");
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Rich {
            file: file.to_string(),
            line,
            fn_name: fn_name.to_string(),
            src_line: src_line.to_string(),
            col,
            caret_len,
            msg: msg.to_string(),
            locals: locals.to_string(),
        }));
    }
    jet_proof_record(2, 1, "E3001", msg, file, line);
    let _ = jet_production_failure_receipt_write("E3001", file, line, fn_name);
    let report = jet_runtime_stop_report(
        "E3001", file, line, fn_name, src_line, col, caret_len, msg, locals,
    );
    if jet_runtime_should_unwind() {
        jet_stream_record_failure_report(report.rendered.clone());
    }
    jet_runtime_stop_unwind(report.rendered, report.exit_code, msg)
}
/// Render a test assertion failure through the same rich diagnostic formatter
/// used by runtime stops. Test adapters and the generated harness call this
/// helper instead of maintaining a second report shape.
pub(crate) fn jet_test_failure_message(
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    msg: &str,
    locals: &str,
) -> String {
    jet_runtime_stop_report(
        "E3001", file, line, fn_name, src_line, col, caret_len, msg, locals,
    )
    .rendered
}

/// Canonical checked equality condition used by semantic `require_eq`.
fn jet_eq<T: PartialEq>(left: &T, right: &T) -> bool {
    left == right
}
/// Rich source-context form of `#require`.
fn jet_require(
    condition: bool,
    msg: &str,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    locals: &str,
) {
    if !condition {
        jet_panic_rich(file, line, fn_name, src_line, col, caret_len, msg, locals);
    }
}

/// Rich source-context form of `#require_eq`.
///
/// Equality is evaluated by the checked binary `Eq` route. This formatter
/// consumes that condition plus adapter-produced canonical debug strings.
fn jet_require_eq(
    condition: bool,
    left_debug: &str,
    right_debug: &str,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    locals: &str,
) {
    if !condition {
        let msg = format!("expected: {right_debug}, got: {left_debug}");
        jet_panic_rich(
            file, line, fn_name, src_line, col, caret_len, &msg, locals,
        );
    }
}

/// Non-panicking test carrier for rich source-context `#require`.
fn jet_test_require(
    condition: bool,
    msg: &str,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    locals: &str,
) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(jet_test_failure_message(
            file, line, fn_name, src_line, col, caret_len, msg, locals,
        ))
    }
}

/// Non-panicking test carrier for rich source-context `#require_eq`.
fn jet_test_require_eq(
    condition: bool,
    left_debug: &str,
    right_debug: &str,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
    locals: &str,
) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        let msg = format!("expected: {right_debug}, got: {left_debug}");
        Err(jet_test_failure_message(
            file, line, fn_name, src_line, col, caret_len, &msg, locals,
        ))
    }
}

/// E3002 / D-FAIL-CTX1: `?`-propagation trace.
///
/// Consecutive identical frames (same fn + file + line) collapse — Go wrap-noise
/// lesson — while each distinct site keeps its identity (Elixir lesson).
fn jet_trace_err<T, E>(r: Result<T, E>, file: &str, line: u32, fn_name: &str) -> Result<T, E> {
    if r.is_err() {
        jet_journey_frame(file, line, fn_name, || String::new());
    } else {
        jet_journey_reset();
    }
    r
}

fn jet_trace_err_note<T, E, F: FnOnce() -> String>(
    r: Result<T, E>,
    file: &str,
    line: u32,
    fn_name: &str,
    note: F,
) -> Result<T, E> {
    if r.is_err() {
        jet_journey_frame(file, line, fn_name, note);
    } else {
        jet_journey_reset();
    }
    r
}

// D-FIXARR1: index/unpack/slice helpers accept `&[T]` so that both growable
// `Vec<T>` and fixed-size `[T; N]` stack arrays coerce in without `.to_vec()`.
#[inline(always)]
fn jet_index_vec<T: Clone>(xs: &[T], i: i64, file: &str, line: u32) -> T {
    jet_index_vec_ref(xs, i, file, line).clone()
}
#[inline(always)]
fn jet_index_vec_ref<'a, T>(xs: &'a [T], i: i64, file: &str, line: u32) -> &'a T {
    jet_fixed_list_index(xs.len(), i, |index| &xs[index])
        .unwrap_or_else(|error| jet_arithmetic_stop(file, line, &error.message()))
}
#[inline(always)]
fn jet_index_vec_mut<'a, T>(
    xs: &'a mut [T],
    i: i64,
    file: &str,
    line: u32,
) -> &'a mut T {
    jet_fixed_list_index(xs.len(), i, |index| &mut xs[index])
        .unwrap_or_else(|error| jet_arithmetic_stop(file, line, &error.message()))
}
#[inline(always)]
fn jet_index_vec_set<T>(xs: &mut [T], i: i64, value: T, file: &str, line: u32) {
    let index = jet_fixed_list_index(xs.len(), i, |index| index)
        .unwrap_or_else(|error| jet_arithmetic_stop(file, line, &error.message()));
    xs[index] = value;
}

fn jet_unpack_vec<T: Clone>(xs: &[T], want: usize, i: usize, file: &str, line: u32) -> T {
    if xs.len() != want {
        jet_panic(
            file,
            line,
            &format!(
                "this pattern needs exactly {} item{}, but the list has {}",
                want,
                if want == 1 { "" } else { "s" },
                xs.len()
            ),
        );
    }
    xs[i].clone()
}
fn jet_slice_vec<T: Clone>(xs: &[T], a: i64, b: i64, file: &str, line: u32) -> Vec<T> {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't slice {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    xs[a as usize..=b as usize].to_vec()
}
fn jet_checked_range_bounds(
    len: i64,
    range: &JetRange,
    action: &str,
    file: &str,
    line: u32,
) -> std::ops::Range<usize> {
    let Some((start, end)) =
        jet_range_bounds(range.start, range.end, range.exclusive, len)
    else {
        jet_panic(
            file,
            line,
            &format!(
                "can't {} {} items from {} to {} ({})",
                action,
                len,
                range.start,
                range.end,
                if range.exclusive { "exclusive" } else { "inclusive" }
            ),
        );
    };
    start as usize..end as usize
}

trait JetSliceRange {
    type Output;

    fn slice_range(&self, range: &JetRange, file: &str, line: u32) -> Self::Output;
}

fn jet_slice_vec_range<T: Clone>(
    xs: &[T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> Vec<T> {
    xs[jet_checked_range_bounds(xs.len() as i64, range, "slice", file, line)].to_vec()
}

impl<T: Clone> JetSliceRange for [T] {
    type Output = Vec<T>;

    fn slice_range(&self, range: &JetRange, file: &str, line: u32) -> Self::Output {
        jet_slice_vec_range(self, range, file, line)
    }
}

impl<T: Clone> JetSliceRange for Vec<T> {
    type Output = Vec<T>;

    fn slice_range(&self, range: &JetRange, file: &str, line: u32) -> Self::Output {
        jet_slice_vec_range(self, range, file, line)
    }
}

impl JetSliceRange for String {
    type Output = String;

    fn slice_range(&self, range: &JetRange, file: &str, line: u32) -> Self::Output {
        jet_string_slice_value(self, range.start, range.end, range.exclusive)
            .unwrap_or_else(|message| jet_panic(file, line, &message))
    }
}

fn jet_slice_range<T: JetSliceRange + ?Sized>(
    xs: &T,
    range: &JetRange,
    file: &str,
    line: u32,
) -> T::Output {
    xs.slice_range(range, file, line)
}
// D-DYNARRAY1 / D-SHAPE-PLACE1: range places produce zero-copy windows.
// Their bounds share `jet_range_bounds` with owned slicing and every engine.
// The returned lifetime is tied to `xs`; sema proves the window cannot outlive
// the owner or survive a storage-changing mutation.
fn jet_checked_view_window(
    start: i64,
    end: i64,
    exclusive: bool,
    len: i64,
    file: &str,
    line: u32,
) -> (i64, i64) {
    match jet_checked_view_bounds(start, end, exclusive, len) {
        Ok(bounds) => bounds,
        Err(message) => jet_panic(file, line, &message),
    }
}

fn jet_view_new<'a, T>(xs: &'a [T], a: i64, b: i64, file: &str, line: u32) -> &'a [T] {
    let (start, end) = jet_checked_view_window(a, b, false, xs.len() as i64, file, line);
    &xs[start as usize..end as usize]
}

fn jet_view_mut_new<'a, T>(
    xs: &'a mut [T],
    a: i64,
    b: i64,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let (start, end) = jet_checked_view_window(a, b, false, xs.len() as i64, file, line);
    &mut xs[start as usize..end as usize]
}

fn jet_views_mut_new<'a, T>(
    xs: &'a mut [T],
    ranges: &[(i64, i64, u32)],
    file: &str,
) -> Vec<&'a mut [T]> {
    let len = xs.len() as i64;
    let mut ordered = Vec::with_capacity(ranges.len());
    for (index, &(start, end, line)) in ranges.iter().enumerate() {
        let (start, end) = jet_checked_view_window(start, end, false, len, file, line);
        ordered.push((start as usize, end as usize, index));
    }
    jet_views_mut_from_windows(xs, ordered, file)
}

fn jet_views_mut_from_windows<'a, T>(
    xs: &'a mut [T],
    mut ordered: Vec<(usize, usize, usize)>,
    file: &str,
) -> Vec<&'a mut [T]> {
    ordered.sort_by_key(|&(start, end, _)| (start, end));
    if ordered.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        jet_panic(file, 0, "mutable view ranges overlap");
    }

    let mut pieces = Vec::with_capacity(ordered.len());
    let mut tail = xs;
    let mut offset = 0usize;
    for (start, end, index) in ordered {
        let (_, from_start) = tail.split_at_mut(start - offset);
        let (selected, after) = from_start.split_at_mut(end - start);
        pieces.push((index, selected));
        tail = after;
        offset = end;
    }
    pieces.sort_by_key(|(index, _)| *index);
    pieces.into_iter().map(|(_, selected)| selected).collect()
}

fn jet_views_mut_range_new<'a, T>(
    xs: &'a mut [T],
    ranges: &[(JetRange, u32)],
    file: &str,
) -> Vec<&'a mut [T]> {
    let len = xs.len() as i64;
    let windows = ranges
        .iter()
        .enumerate()
        .map(|(index, (range, line))| {
            let (start, end) = jet_checked_view_window(
                range.start,
                range.end,
                range.exclusive,
                len,
                file,
                *line,
            );
            (start as usize, end as usize, index)
        })
        .collect::<Vec<_>>();
    jet_views_mut_from_windows(xs, windows, file)
}

// D-MEMDISJOINT1=A: runtime disjointness is proved once, before any mutable
// view exists. These helpers return the same Error family for bounds and
// overlap failures; engines only marshal their arguments and results.
fn jet_split_write<T>(
    xs: &mut [T],
    mid: i64,
) -> Result<(&mut [T], &mut [T]), String> {
    jet_disjoint_split_bounds(xs.len(), mid)?;
    Ok(xs.split_at_mut(mid as usize))
}

fn jet_get_disjoint_write<'a, T>(
    xs: &'a mut [T],
    indices: &[i64],
) -> Result<Vec<&'a mut [T]>, String> {
    let ordered = jet_disjoint_index_bounds(xs.len(), indices)?;
    let mut views = Vec::with_capacity(ordered.len());
    let mut tail = xs;
    let mut offset = 0usize;
    for (start, end, position) in ordered {
        let (_, from_index) = tail.split_at_mut(start - offset);
        let (selected, after) = from_index.split_at_mut(end - start);
        views.push((position, selected));
        tail = after;
        offset = end;
    }
    views.sort_by_key(|(position, _)| *position);
    Ok(views.into_iter().map(|(_, view)| view).collect())
}

fn jet_edit_disjoint<T, F>(xs: &mut [T], indices: &[i64], edit: F) -> Result<(), String>
where
    F: FnOnce(&mut [T], &mut [T]),
{
    if indices.len() != 2 {
        return Err("edit_disjoint needs exactly two indexes".to_string());
    }
    let mut views = jet_get_disjoint_write(xs, indices)?;
    let right = views.pop().expect("two disjoint views");
    let left = views.pop().expect("two disjoint views");
    edit(left, right);
    Ok(())
}

fn jet_view_range_new<'a, T>(
    xs: &'a [T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a [T] {
    let (start, end) = jet_checked_view_window(
        range.start,
        range.end,
        range.exclusive,
        xs.len() as i64,
        file,
        line,
    );
    &xs[start as usize..end as usize]
}

fn jet_view_mut_range_new<'a, T>(
    xs: &'a mut [T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let (start, end) = jet_checked_view_window(
        range.start,
        range.end,
        range.exclusive,
        xs.len() as i64,
        file,
        line,
    );
    &mut xs[start as usize..end as usize]
}

fn jet_check_view_bounds(len: i64, a: i64, b: i64, file: &str, line: u32) {
    let _ = jet_checked_view_window(a, b, false, len, file, line);
}
// D-DYNARRAY1: View<T> read-only closure surface. `xs` is already a borrow
// (never `.clone()`d to an owned `Vec` first, unlike the `jet_list_*` family
// above) — folding/mapping a view touches no allocation beyond the result.
fn jet_view_fold<T, U, F>(xs: &[T], init: U, mut f: F) -> U
where
    F: FnMut(&U, &T) -> U,
{
    let mut acc = init;
    for x in xs {
        acc = f(&acc, x);
    }
    acc
}
fn jet_view_map<T, U, F>(xs: &[T], f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(f).collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct JetMap<K, V>(std::sync::Arc<std::collections::BTreeMap<K, V>>);

impl<K, V> JetMap<K, V> {
    fn new() -> Self {
        Self(std::sync::Arc::new(std::collections::BTreeMap::new()))
    }
}
impl<K: Ord, V> JetMap<K, V> {
    fn from_pairs(entries: Vec<(K, V)>) -> Self {
        entries.into_iter().collect()
    }
}

/// Build a typed Jet map from one already-lowered runtime entry vector.
///
/// The checked TIR producer has already evaluated each key and value. This
/// kernel only commits those pairs to the canonical copy-on-write map.
#[inline(always)]
fn jet_data_entries_to_map<K: Ord, V>(entries: Vec<(K, V)>) -> JetMap<K, V> {
    JetMap::from_pairs(entries)
}

// Codegen lowers map construction from a sequence of pairs to
// `.into_iter().collect()`, so the map has to be buildable from its own pairs.
// Without this, decoding a table into a typed map emitted Rust that rustc
// rejected (I2).
impl<K: Ord, V> FromIterator<(K, V)> for JetMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
        Self(std::sync::Arc::new(
            pairs.into_iter().collect::<std::collections::BTreeMap<K, V>>(),
        ))
    }
}

impl<K, V> std::ops::Deref for JetMap<K, V> {
    type Target = std::collections::BTreeMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K: Ord + Clone, V: Clone> std::ops::DerefMut for JetMap<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

/// Borrow map storage once for a proven unique mutation region. The caller
/// keeps the returned borrow inside that region; ordinary `JetMap` mutation
/// retains copy-on-write semantics everywhere else.
#[inline(always)]
fn jet_map_make_mut<K: Ord + Clone, V: Clone>(
    m: &mut JetMap<K, V>,
) -> &mut std::collections::BTreeMap<K, V> {
    std::sync::Arc::make_mut(&mut m.0)
}

impl<'a, K: Ord, V> IntoIterator for &'a JetMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::btree_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

fn jet_map_into_entries<K: Ord + Clone, V: Clone>(m: JetMap<K, V>) -> Vec<(K, V)> {
    match std::sync::Arc::try_unwrap(m.0) {
        Ok(map) => {
            let mut entries = Vec::with_capacity(map.len());
            entries.extend(map);
            entries
        }
        Err(shared) => {
            let mut entries = Vec::with_capacity(shared.len());
            entries.extend(shared.iter().map(|(k, v)| (k.clone(), v.clone())));
            entries
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JetRemoveBy {
    Val,
    Slot,
}

fn jet_index_map<M, K: Ord + Clone + JetShow, V: Clone>(
    m: &M,
    k: &K,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
) -> V
where
    M: std::ops::Deref<Target = std::collections::BTreeMap<K, V>>,
{
    match m.get(k) {
        Some(v) => v.clone(),
        None => jet_panic_rich(
            file,
            line,
            fn_name,
            src_line,
            col,
            caret_len,
            &jet_missing_map_key_value(k.jet_show()),
            "",
        ),
    }
}

/// Borrow one map value without materializing a clone. This is the read-side
/// counterpart to `jet_index_map_mut`, used when a view receiver is a nested
/// map/list place whose storage must outlive the borrowed view.
fn jet_index_map_ref<'a, M, K: Ord + Clone + JetShow + 'a, V>(
    m: &'a M,
    k: &K,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
) -> &'a V
where
    M: std::ops::Deref<Target = std::collections::BTreeMap<K, V>>,
{
    match m.get(k) {
        Some(value) => value,
        None => jet_panic_rich(
            file,
            line,
            fn_name,
            src_line,
            col,
            caret_len,
            &jet_missing_map_key_value(k.jet_show()),
            "",
        ),
    }
}

fn jet_index_map_mut<'a, M, K: Ord + Clone + JetShow + 'a, V>(
    m: &'a mut M,
    k: K,
    file: &str,
    line: u32,
    fn_name: &str,
    src_line: &str,
    col: u32,
    caret_len: u32,
) -> &'a mut V
where
    M: std::ops::DerefMut<Target = std::collections::BTreeMap<K, V>>,
{
    match m.get_mut(&k) {
        Some(value) => value,
        None => jet_panic_rich(
            file,
            line,
            fn_name,
            src_line,
            col,
            caret_len,
            &jet_missing_map_key_value(k.jet_show()),
            "",
        ),
    }
}
#[inline(always)]
fn jet_map_insert<M, K: Ord + Clone, V: Clone>(m: &mut M, k: K, v: V)
where
    M: std::ops::DerefMut<Target = std::collections::BTreeMap<K, V>>,
{
    m.insert(k, v);
}

#[inline(always)]
fn jet_map_add_new<M, K: Ord + Clone, V: Clone>(m: &mut M, k: K, v: V) -> bool
where
    M: std::ops::DerefMut<Target = std::collections::BTreeMap<K, V>>,
{
    if m.contains_key(&k) {
        return false;
    }
    m.insert(k, v);
    true
}

/// Update one map entry through the map's storage seam. The closure receives
/// the existing value without cloning it, so a read/compute/insert update can
/// perform one tree lookup and one key evaluation while retaining the same
/// ordered-map and copy-on-write semantics as `jet_map_insert`.
#[inline(always)]
fn jet_map_update<M, K: Ord, V, F>(m: &mut M, k: K, f: F)
where
    M: std::ops::DerefMut<Target = std::collections::BTreeMap<K, V>>,
    F: FnOnce(Option<&V>) -> V,
{
    match m.entry(k) {
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let value = f(Some(entry.get()));
            entry.insert(value);
        }
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(f(None));
        }
    }
}

/// Update a text-keyed map without allocating a replacement key for an
/// existing entry. The ordinary update path owns its key before entering the
/// tree; this borrowed form keeps the same ordered-map semantics while making
/// the common read/compute/write loop pay for a key clone only on insertion.
#[inline(always)]
fn jet_map_update_string<V, F>(
    m: &mut std::collections::BTreeMap<String, V>,
    key: &String,
    f: F,
)
where
    F: FnOnce(Option<&V>) -> V,
{
    if let Some(existing) = m.get_mut(key) {
        let next = {
            let existing = &*existing;
            f(Some(existing))
        };
        *existing = next;
    } else {
        m.insert(key.clone(), f(None));
    }
}

trait JetStringBytesMap<V> {
    fn update_bytes<F>(&mut self, bytes: &[u8], f: F) -> Result<(), ()>
    where
        F: FnOnce(Option<&V>) -> V;
}

impl<V> JetStringBytesMap<V> for std::collections::BTreeMap<String, V> {
    #[inline(always)]
    fn update_bytes<F>(&mut self, bytes: &[u8], f: F) -> Result<(), ()>
    where
        F: FnOnce(Option<&V>) -> V,
    {
        let key = std::str::from_utf8(bytes).map_err(|_| ())?;
        if let Some(existing) = self.get_mut(key) {
            let next = f(Some(&*existing));
            *existing = next;
        } else {
            self.insert(key.to_owned(), f(None));
        }
        Ok(())
    }
}

impl<V: Clone> JetStringBytesMap<V> for JetMap<String, V> {
    #[inline(always)]
    fn update_bytes<F>(&mut self, bytes: &[u8], f: F) -> Result<(), ()>
    where
        F: FnOnce(Option<&V>) -> V,
    {
        std::sync::Arc::make_mut(&mut self.0).update_bytes(bytes, f)
    }
}

impl<V, M: JetStringBytesMap<V> + ?Sized> JetStringBytesMap<V> for &mut M {
    #[inline(always)]
    fn update_bytes<F>(&mut self, bytes: &[u8], f: F) -> Result<(), ()>
    where
        F: FnOnce(Option<&V>) -> V,
    {
        (**self).update_bytes(bytes, f)
    }
}

/// The count loop has already proved that its map owns the only backing
/// allocation. Use a fixed, non-cryptographic hasher only inside that sealed
/// region; the map is not an input-facing general-purpose hash table.
#[derive(Clone, Default)]
struct JetStringCountHasher {
    hash: u64,
}

impl JetStringCountHasher {
    #[inline(always)]
    fn add(&mut self, value: u64) {
        self.hash = self
            .hash
            .wrapping_add(value)
            .wrapping_mul(0xf1357aea2e62a9c5);
    }
}

impl std::hash::Hasher for JetStringCountHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.hash.rotate_left(26)
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        let mut value = bytes.len() as u64;
        for &byte in bytes {
            value = value.rotate_left(5) ^ u64::from(byte);
        }
        self.add(value);
    }

    #[inline(always)]
    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    #[inline(always)]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }
}

type JetStringCountBuildHasher = std::hash::BuildHasherDefault<JetStringCountHasher>;

/// A fresh, uniquely-owned ordered String:Int map can count through a hash
/// table while a loop has no other access to the map. Keys stay as validated
/// UTF-8 bytes during the loop; Drop restores the canonical sorted
/// representation before the map is visible again.
struct JetStringCountBuilder<'a> {
    target: &'a mut std::collections::BTreeMap<String, i64>,
    short_counts: std::collections::HashMap<u64, i64, JetStringCountBuildHasher>,
    long_counts: std::collections::HashMap<Vec<u8>, i64, JetStringCountBuildHasher>,
}

impl<'a> JetStringCountBuilder<'a> {
    #[inline(always)]
    fn short_key(bytes: &[u8]) -> Option<u64> {
        if bytes.len() > 7 {
            return None;
        }
        if bytes.len() == 6 {
            return Some(
                (6u64 << 56)
                    | u64::from(bytes[0])
                    | (u64::from(bytes[1]) << 8)
                    | (u64::from(bytes[2]) << 16)
                    | (u64::from(bytes[3]) << 24)
                    | (u64::from(bytes[4]) << 32)
                    | (u64::from(bytes[5]) << 40),
            );
        }
        let mut key = (bytes.len() as u64) << 56;
        for (index, byte) in bytes.iter().enumerate() {
            key |= u64::from(*byte) << (index * 8);
        }
        Some(key)
    }

    fn short_string(key: u64) -> String {
        let len = (key >> 56) as usize;
        let bytes = (0..len)
            .map(|index| ((key >> (index * 8)) & 0xff) as u8)
            .collect();
        String::from_utf8(bytes).expect("JetStringCountBuilder stores only valid UTF-8 keys")
    }

    fn new(target: &'a mut std::collections::BTreeMap<String, i64>) -> Self {
        Self::with_source_capacity(target, 0)
    }

    fn with_source_capacity(
        target: &'a mut std::collections::BTreeMap<String, i64>,
        source_bytes: usize,
    ) -> Self {
        let estimated_distinct = (source_bytes / 64).min(16_384);
        let mut short_counts = std::collections::HashMap::with_capacity_and_hasher(
            estimated_distinct,
            JetStringCountBuildHasher::default(),
        );
        let mut long_counts = std::collections::HashMap::default();
        for (key, value) in std::mem::take(target) {
            let bytes = key.into_bytes();
            if let Some(short) = Self::short_key(&bytes) {
                short_counts.insert(short, value);
            } else {
                long_counts.insert(bytes, value);
            }
        }
        Self {
            target,
            short_counts,
            long_counts,
        }
    }
}

impl JetStringBytesMap<i64> for JetStringCountBuilder<'_> {
    #[inline(always)]
    fn update_bytes<F>(&mut self, bytes: &[u8], f: F) -> Result<(), ()>
    where
        F: FnOnce(Option<&i64>) -> i64,
    {
        if let Some(key) = Self::short_key(bytes) {
            if let Some(existing) = self.short_counts.get_mut(&key) {
                *existing = f(Some(&*existing));
            } else {
                std::str::from_utf8(bytes).map_err(|_| ())?;
                self.short_counts.insert(key, f(None));
            }
        } else if let Some(existing) = self.long_counts.get_mut(bytes) {
            *existing = f(Some(&*existing));
        } else {
            std::str::from_utf8(bytes).map_err(|_| ())?;
            self.long_counts.insert(bytes.to_owned(), f(None));
        }
        Ok(())
    }
}

impl Drop for JetStringCountBuilder<'_> {
    fn drop(&mut self) {
        self.target.extend(
            std::mem::take(&mut self.short_counts)
                .into_iter()
                .map(|(key, value)| (Self::short_string(key), value)),
        );
        self.target.extend(
            std::mem::take(&mut self.long_counts)
                .into_iter()
                .map(|(bytes, value)| {
                    (
                        String::from_utf8(bytes)
                            .expect("JetStringCountBuilder stores only valid UTF-8 keys"),
                        value,
                    )
                }),
        );
    }
}

/// Update a text-keyed map from a borrowed UTF-8 byte span. Strict decoding
/// happens before lookup; existing keys need no temporary String.
#[inline(always)]
fn jet_map_update_string_bytes<M, V, F>(m: &mut M, bytes: &[u8], f: F) -> Result<(), ()>
where
    M: JetStringBytesMap<V>,
    F: FnOnce(Option<&V>) -> V,
{
    m.update_bytes(bytes, f)
}

// BTreeMap has no stable fallible reservation API. Keep this representation
// step at the map seam; the shared Prelude owns the AllocError projection.
fn jet_map_try_insert_storage<K: Ord + Clone, V: Clone>(
    m: &mut JetMap<K, V>,
    k: K,
    v: V,
) -> Result<Option<V>, ()> {
    Ok(m.insert(k, v))
}

/// D-MAP-MERGE1=E: merge `other` into a clone of `left`. Right wins on shared keys.
fn jet_map_merge<K: Ord + Clone, V: Clone>(
    left: &JetMap<K, V>,
    other: &JetMap<K, V>,
) -> JetMap<K, V> {
    let mut out = left.clone();
    let storage = jet_map_make_mut(&mut out);
    for (k, v) in other {
        storage.insert(k.clone(), v.clone());
    }
    out
}

/// D-MAP-MERGE1=E: merge with an explicit conflict callback `(key, left, right) -> V`.
fn jet_map_merge_with<K: Ord + Clone, V: Clone, F>(
    left: &JetMap<K, V>,
    other: &JetMap<K, V>,
    conflict: F,
) -> JetMap<K, V>
where
    F: Fn(&K, V, V) -> V,
{
    let mut out = left.clone();
    let storage = jet_map_make_mut(&mut out);
    for (k, right) in other {
        match storage.remove(k) {
            Some(left_v) => {
                let resolved = conflict(k, left_v, right.clone());
                storage.insert(k.clone(), resolved);
            }
            None => {
                storage.insert(k.clone(), right.clone());
            }
        }
    }
    out
}
// D-LISTMAP1: the view owns the map's Arc and advances by key. This keeps the
// iterator `'static` without copying the BTreeMap (or borrowing through a
// short-lived local Arc). Each pull clones only the yielded item; the map stays
// shared and untouched until a mutation triggers Arc::make_mut.
struct JetMapKeys<K, V> {
    map: std::sync::Arc<std::collections::BTreeMap<K, V>>,
    last: Option<K>,
}

impl<K: Ord + Clone, V> Iterator for JetMapKeys<K, V> {
    type Item = K;

    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.last.as_ref() {
            Some(last) => self
                .map
                .range((std::ops::Bound::Excluded(last), std::ops::Bound::Unbounded))
                .next(),
            None => self.map.iter().next(),
        }?;
        let key = next.0.clone();
        self.last = Some(key.clone());
        Some(key)
    }
}

struct JetMapValues<K, V> {
    map: std::sync::Arc<std::collections::BTreeMap<K, V>>,
    last: Option<K>,
}

impl<K: Ord + Clone, V: Clone> Iterator for JetMapValues<K, V> {
    type Item = V;

    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.last.as_ref() {
            Some(last) => self
                .map
                .range((std::ops::Bound::Excluded(last), std::ops::Bound::Unbounded))
                .next(),
            None => self.map.iter().next(),
        }?;
        let key = next.0.clone();
        let value = next.1.clone();
        self.last = Some(key);
        Some(value)
    }
}

fn jet_map_keys<K: Ord + Clone + 'static, V: 'static>(m: &JetMap<K, V>) -> JetIter<K> {
    JetIter(Box::new(JetMapKeys {
        map: std::sync::Arc::clone(&m.0),
        last: None,
    }))
}

fn jet_map_values<K: Ord + Clone + 'static, V: Clone + 'static>(m: &JetMap<K, V>) -> JetIter<V> {
    JetIter(Box::new(JetMapValues {
        map: std::sync::Arc::clone(&m.0),
        last: None,
    }))
}

fn jet_list_remove_value_with_location<T: Clone + PartialEq>(
    xs: &mut Vec<T>,
    value: T,
    _file: &str,
    _line: u32,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(jet_list_remove_value_kernel(xs, value))
}

fn jet_list_remove_slot_with_location<T: Clone>(xs: &mut Vec<T>, i: i64, file: &str, line: u32) -> JetOutcome<T, JetAbsent> {
    match jet_list_remove_slot_kernel(xs, i) {
        Ok(value) => Ok(value),
        Err(message) => jet_arithmetic_stop(file, line, &message),
    }
}

fn jet_list_insert_with_location<T>(xs: &mut Vec<T>, index: i64, value: T, file: &str, line: u32) {
    match jet_list_insert_kernel(xs, index, value) {
        Ok(()) => {}
        Err(error) => {
            let message = error.message();
            jet_runtime_stop(error.code(), file, line, &message);
        }
    }
}

#[inline(always)]
fn jet_list_remove_value<T: Clone + PartialEq>(
    xs: &mut Vec<T>,
    value: T,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(jet_list_remove_value_kernel(xs, value))
}

#[inline(always)]
fn jet_list_remove_slot<T: Clone>(
    xs: &mut Vec<T>,
    index: i64,
) -> JetOutcome<T, JetAbsent> {
    match jet_list_remove_slot_kernel(xs, index) {
        Ok(value) => Ok(value),
        Err(message) => jet_arithmetic_stop("<core.collections>", 0, &message),
    }
}

#[inline(always)]
fn jet_list_insert<T>(xs: &mut Vec<T>, index: i64, value: T) {
    match jet_list_insert_kernel(xs, index, value) {
        Ok(()) => {}
        Err(error) => {
            let message = error.message();
            jet_runtime_stop(error.code(), "<core.collections>", 0, &message);
        }
    }
}

// D-LISTREMOVE1/F (criterion c6 on #1481): PriorityQueue.remove reuses List's
// exact value/slot selector shape. `BinaryHeap` has no native indexed or
// value-search removal, so both forms round-trip through an owned `Vec` —
// sorted highest-first, the same canonical order `peek`/`to_sorted_list`
// already publish (and the one the TIR-eval/comptime twin uses), so `.Slot`
// means the same position on every execution tier (I9).
fn jet_priority_queue_remove_value<T: Ord>(
    pq: &mut std::collections::BinaryHeap<T>,
    value: T,
) -> JetOutcome<T, JetAbsent> {
    jet_priority_queue_remove_value_kernel(pq, value)
}

fn jet_priority_queue_remove_slot<T: Ord>(
    pq: &mut std::collections::BinaryHeap<T>,
    i: i64,
    file: &str,
    line: u32,
) -> JetOutcome<T, JetAbsent> {
    match jet_priority_queue_remove_slot_kernel(pq, i, file, line) {
        Ok(outcome) => outcome,
        Err(message) => jet_panic(file, line, &message),
    }
}

fn jet_list_count<T: PartialEq>(xs: &[T], value: &T) -> i64 {
    jet_list_count_kernel(xs, value)
}
fn jet_list_count_where<T, F>(xs: &[T], predicate: F) -> i64
where
    F: FnMut(&T) -> bool,
{
    jet_list_count_where_kernel(xs, predicate)
}

fn jet_list_update_first<T, F>(
    xs: &mut Vec<T>,
    predicate: F,
    replacement: T,
) -> bool
where
    F: FnMut(&T) -> bool,
{
    jet_list_update_first_kernel(xs, predicate, replacement)
}

fn jet_list_concat<T: Clone>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = left.to_vec();
    out.extend(right.iter().cloned());
    out
}
fn jet_fixed_list_concat<
    T: Clone,
    const LEFT: usize,
    const RIGHT: usize,
    const TOTAL: usize,
>(
    left: &[T; LEFT],
    right: &[T; RIGHT],
) -> [T; TOTAL] {
    debug_assert_eq!(TOTAL, LEFT + RIGHT);
    std::array::from_fn(|index| {
        if index < LEFT {
            left[index].clone()
        } else {
            right[index - LEFT].clone()
        }
    })
}

fn jet_char_len(s: &String) -> i64 {
    s.chars().count() as i64
}
// Eager materialize of the same pieces as `jet_iter_string_split` (AOT `String.split`
// emits the lazy helper; this Vec form remains for hosts that need a list handle).
fn jet_string_split(s: &String, sep: &str) -> Vec<String> {
    s.split(sep).map(|x| x.to_string()).collect()
}
// D-STR-AFTER1: first-occurrence substring split. `sep` absent -> the whole
// original string (both sides agree, mirroring `.replace`'s no-match-is-identity
// convention — no `Option`/empty-string special case to unwrap).
fn jet_string_after(s: &str, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[i + sep.len()..].to_string(),
        None => s.to_string(),
    }
}
fn jet_string_before(s: &str, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[..i].to_string(),
        None => s.to_string(),
    }
}
// D-MEM1 stage S5 (2026-07-04): zero-copy siblings of `jet_string_after`/
// `_before`/(inline `.trim()`) — a genuine borrow into `s`'s own buffer, no
// allocation, instead of a fresh owned `String`. Used ONLY when sema proves
// (E2307, `Binding::string_view`) the resulting binding can't outlive `s`'s
// scope — the same D-DYNARRAY1 soundness proof `View<T>`/`jet_view_new`
// already uses, applied to strings. `s: &str` (not `&String`) so a call
// chain of these composes without a materialize step in between.
fn jet_string_after_view<'a>(s: &'a str, sep: &str) -> &'a str {
    match s.find(sep) {
        Some(i) => &s[i + sep.len()..],
        None => s,
    }
}
fn jet_string_before_view<'a>(s: &'a str, sep: &str) -> &'a str {
    match s.find(sep) {
        Some(i) => &s[..i],
        None => s,
    }
}
fn jet_string_trim_view(s: &str) -> &str {
    jet_unicode_trim_view(s)
}
fn jet_string_lines(s: &String) -> Vec<String> {
    s.lines().map(|x| x.to_string()).collect()
}
fn jet_string_slice(s: &String, a: i64, b: i64, file: &str, line: u32) -> String {
    jet_string_slice_value(s, a, b, false)
        .unwrap_or_else(|message| jet_panic(file, line, &message))
}
fn jet_string_slice_builtin(s: &String, a: i64, b: i64) -> String {
    jet_string_slice_value(s, a, b, false)
        .unwrap_or_else(|message| jet_panic("<core.builtin>", 0, &message))
}
fn jet_list_each<T, F, I>(xs: I, f: F)
where
    I: IntoIterator<Item = T>,
    F: Fn(&T),
{
    for x in xs {
        f(&x);
    }
}
fn jet_list_each_mut<T, F, I>(xs: I, mut f: F)
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T),
{
    for x in xs {
        f(&x);
    }
}
fn jet_list_find<T, F, I>(xs: I, mut f: F) -> JetOutcome<T, JetAbsent>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    jet_outcome_of(xs.into_iter().find(|x| f(x)))
}
fn jet_list_any<T, F, I>(xs: I, mut f: F) -> bool
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    xs.into_iter().any(|x| f(&x))
}
fn jet_list_all<T, F, I>(xs: I, mut f: F) -> bool
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> bool,
{
    xs.into_iter().all(|x| f(&x))
}
fn jet_list_reduce<T, U, F, I>(xs: I, init: U, mut f: F) -> U
where
    I: IntoIterator<Item = T>,
    F: FnMut(&U, &T) -> U,
{
    xs.into_iter().fold(init, |acc, x| f(&acc, &x))
}
// Float has no Rust `Ord` implementation because NaN makes `partial_cmp`
// return `None`. The shared FloatOrdering Prelude part supplies Jet's total
// sort comparator; this wrapper only converts it to the generated enum.
fn jet_float_ordering(left: f64, right: f64) -> __jet_Ordering {
    match jet_float_sort_cmp(left, right) {
        std::cmp::Ordering::Less => __jet_Ordering::__jet_Less,
        std::cmp::Ordering::Equal => __jet_Ordering::__jet_Equal,
        std::cmp::Ordering::Greater => __jet_Ordering::__jet_Greater,
    }
}

fn jet_list_sort_by_compare<T, F>(xs: &mut Vec<T>, mut f: F)
where
    F: FnMut(&T, &T) -> __jet_Ordering,
{
    jet_list_sort_by_compare_kernel(xs, |left, right| match f(left, right) {
        __jet_Ordering::__jet_Less => std::cmp::Ordering::Less,
        __jet_Ordering::__jet_Equal => std::cmp::Ordering::Equal,
        __jet_Ordering::__jet_Greater => std::cmp::Ordering::Greater,
    });
}
fn jet_map_each<K: Ord, V, F>(m: JetMap<K, V>, mut f: F)
where
    F: FnMut(&K, &V),
{
    for (k, v) in &m {
        f(k, v);
    }
}

// #1477 Map ledger surface
fn jet_map_copy<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>) -> JetMap<K, V> { jet_map_copy_kernel(m) }
fn jet_map_equal<K: Ord + PartialEq, V: PartialEq>(a: &JetMap<K, V>, b: &JetMap<K, V>) -> bool { jet_map_equal_kernel(a, b) }
fn jet_map_first_key<K: Ord + Clone, V>(m: &JetMap<K, V>) -> JetOutcome<K, JetAbsent> { jet_map_first_key_kernel(m) }
fn jet_map_to_list<K: Ord + Clone, V: Clone, R>(m: &JetMap<K, V>, build: impl Fn(K, V) -> R) -> Vec<R> {
    jet_map_entries_kernel(m).into_iter().map(|(k, v)| build(k, v)).collect()
}
fn jet_map_any<K: Ord, V, F>(m: JetMap<K, V>, mut f: F) -> bool where F: FnMut(&K, &V) -> bool {
    m.iter().any(|(k, v)| f(k, v))
}
fn jet_map_all<K: Ord, V, F>(m: JetMap<K, V>, mut f: F) -> bool where F: FnMut(&K, &V) -> bool {
    m.iter().all(|(k, v)| f(k, v))
}
fn jet_map_filter<K: Ord + Clone, V: Clone, F>(m: JetMap<K, V>, mut f: F) -> JetMap<K, V>
where F: FnMut(&K, &V) -> bool {
    JetMap(std::sync::Arc::new(m.iter().filter(|(k,v)| f(k,v)).map(|(k,v)|(k.clone(),v.clone())).collect()))
}
fn jet_map_map_values<K: Ord + Clone, V, U, F>(m: JetMap<K, V>, mut f: F) -> JetMap<K, U>
where F: FnMut(&K, &V) -> U {
    JetMap(std::sync::Arc::new(m.iter().map(|(k,v)|(k.clone(), f(k,v))).collect()))
}
fn jet_map_fold<K: Ord, V, U, F>(m: JetMap<K, V>, init: U, mut f: F) -> U
where F: FnMut(&U, &K, &V) -> U {
    let mut acc = init;
    for (k, v) in &m {
        acc = f(&acc, k, v);
    }
    acc
}
fn jet_map_flat_map<K: Ord + Clone, V: Clone, F>(m: JetMap<K, V>, mut f: F) -> JetMap<K, V>
where F: FnMut(&K, &V) -> JetMap<K, V> {
    let mut out = JetMap::new();
    let storage = jet_map_make_mut(&mut out);
    for (k, v) in &m {
        for (ik, iv) in f(k, v).iter() {
            storage.insert(ik.clone(), iv.clone());
        }
    }
    out
}
fn jet_map_max_value<K: Ord, V: Ord + Clone>(m: &JetMap<K, V>) -> JetOutcome<V, JetAbsent> { jet_map_max_value_kernel(m) }
fn jet_map_min_value<K: Ord, V: Ord + Clone>(m: &JetMap<K, V>) -> JetOutcome<V, JetAbsent> { jet_map_min_value_kernel(m) }
fn jet_map_intersection<K: Ord + Clone, V: Clone>(left: &JetMap<K, V>, right: &JetMap<K, V>) -> JetMap<K, V> {
    jet_map_intersection_kernel(left, right)
}
fn jet_map_slice_keys<K: Ord + Clone, V: Clone>(m: &JetMap<K, V>, keys: Vec<K>) -> JetMap<K, V> {
    jet_map_slice_keys_kernel(m, keys)
}
fn jet_map_from_keys<K: Ord + Clone, V: Clone>(keys: Vec<K>, default: V) -> JetMap<K, V> {
    jet_map_from_keys_kernel(keys, default)
}
fn jet_map_contains_value<K: Ord, V: PartialEq>(m: &JetMap<K, V>, needle: &V) -> bool {
    jet_map_contains_value_kernel(m, needle)
}
fn jet_map_pop_first<K: Ord + Clone, V: Clone>(m: &mut JetMap<K, V>) -> JetOutcome<V, JetAbsent> {
    jet_map_pop_first_kernel(m)
}
fn jet_ordering_then(first: &__jet_Ordering, second: &__jet_Ordering) -> __jet_Ordering {
    match first {
        __jet_Ordering::__jet_Equal => *second,
        _ => *first,
    }
}

fn jet_ordering_reverse(value: &__jet_Ordering) -> __jet_Ordering {
    match value {
        __jet_Ordering::__jet_Less => __jet_Ordering::__jet_Greater,
        __jet_Ordering::__jet_Greater => __jet_Ordering::__jet_Less,
        __jet_Ordering::__jet_Equal => __jet_Ordering::__jet_Equal,
    }
}

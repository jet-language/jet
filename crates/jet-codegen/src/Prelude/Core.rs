impl JetShow for JetDate {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetDate {
    fn jet_debug(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetShow for JetLocalTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetLocalTime {
    fn jet_debug(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetShow for JetPeriod {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetShow for JetInstant {
    fn jet_show(&self) -> String {
        "Instant".to_string()
    }
}

impl JetShow for JetDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}

impl JetDebug for JetDateTime {
    fn jet_debug(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetShow for JetZone {
    fn jet_show(&self) -> String {
        self.name.clone()
    }
}

impl JetShow for JetZonedDateTime {
    fn jet_show(&self) -> String {
        self.to_string_fmt()
    }
}
// D-PARCAPTURE1=D: one bounded indexed engine for every explicit parallel
// collection adapter. Chunk boundaries are fixed so scheduling cannot affect
// result order or `para_fold`'s merge tree; the number of worker threads is
// bounded by the host's available parallelism.
const JET_PARA_CHUNK_ITEMS: usize = 64;

struct JetParaFailure {
    index: usize,
    payload: Box<dyn std::any::Any + Send + 'static>,
}

enum JetParaRuntimeFailure {
    Simple {
        file: String,
        line: u32,
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
            Self::Simple { file, line, msg } => jet_panic(&file, line, &msg),
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
            Self::SchedulerFatal { msg } => jet_runtime_diagnostic(format!("panic: {msg}")),
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

fn jet_list_para_chunks<R, F>(len: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(std::ops::Range<usize>) -> Result<R, JetParaFailure> + Sync,
{
    let chunk_count = len.div_ceil(JET_PARA_CHUNK_ITEMS);
    if chunk_count == 0 {
        return Vec::new();
    }
    #[cfg(jet_para_test_workers)]
    let worker_count = 3.min(chunk_count);
    #[cfg(not(jet_para_test_workers))]
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(chunk_count);
    // A single chunk is the safe serial fast path. Keep the indexed chunk
    // boundaries when a host exposes only one CPU; para_fold's seed/merge
    // semantics depend on those boundaries even without parallel workers.
    if chunk_count == 1 {
        return match f(0..len) {
            Ok(result) => vec![result],
            Err(failure) => jet_para_raise_failure(failure),
        };
    }
    let mut indexed = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        let f = &f;
        for worker in 0..worker_count {
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                for chunk in (worker..chunk_count).step_by(worker_count) {
                    let start = chunk * JET_PARA_CHUNK_ITEMS;
                    let end = (start + JET_PARA_CHUNK_ITEMS).min(len);
                    out.push((chunk, f(start..end)));
                }
                out
            }));
        }
        let mut indexed = Vec::with_capacity(chunk_count);
        for handle in handles.into_iter().rev() {
            match handle.join() {
                Ok(results) => indexed.extend(results),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
        indexed
    });
    indexed.sort_unstable_by_key(|(chunk, _)| *chunk);
    let mut results = Vec::with_capacity(chunk_count);
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

fn jet_list_para_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync,
{
    jet_list_para_chunks(xs.len(), |range| {
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
    jet_list_para_chunks(xs.len(), |range| {
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
    let mut partials = jet_list_para_chunks(xs.len(), |range| {
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
    while partials.len() > 1 {
        let mut next = Vec::with_capacity(partials.len().div_ceil(2));
        let mut iter = partials.into_iter();
        while let Some((left_index, left)) = iter.next() {
            match iter.next() {
                Some((_, right)) => match jet_para_call(left_index, || merge(&left, &right)) {
                    Ok(merged) => next.push((left_index, merged)),
                    Err(failure) => jet_para_raise_failure(failure),
                },
                None => next.push((left_index, left)),
            }
        }
        partials = next;
    }
    partials.pop().expect("non-empty parallel fold lost its result").1
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

thread_local! {
    pub static JET_IN_SCHEDULER_TASK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static JET_INTERRUPT_HANDLER_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
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
    jet_scheduler_in_task() || jet_interrupt_handler_should_unwind()
}

fn jet_interrupt_handler_should_unwind() -> bool {
    JET_INTERRUPT_HANDLER_DEPTH.with(|depth| depth.get() != 0)
}

fn jet_scheduler_panic_should_unwind() -> bool {
    jet_runtime_should_unwind()
}

struct JetRuntimeExit;

fn jet_runtime_boundary<F, T>(run: F) -> T
where
    F: FnOnce() -> T,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => value,
        Err(payload) if payload.is::<JetRuntimeExit>() => std::process::exit(70),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn jet_runtime_exit() -> ! {
    std::panic::resume_unwind(Box::new(JetRuntimeExit))
}

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Simple {
            file: file.to_string(),
            line,
            msg: msg.to_string(),
        }));
    }
    jet_proof_record(2, 1, "panic", msg, file, line);
    if jet_runtime_should_unwind() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    jet_runtime_exit();
}

fn jet_runtime_diagnostic(rendered: String) -> ! {
    if JET_PARA_DEFER_FAILURE.with(|defer| defer.get()) {
        std::panic::resume_unwind(Box::new(JetParaRuntimeFailure::Diagnostic { rendered }));
    }
    if jet_interrupt_handler_should_unwind() {
        panic!("{}", rendered);
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
    if jet_runtime_should_unwind() {
        panic!(
            "#{} contract failed: {} (at {}:{})",
            clause_kw, msg, file, line
        );
    }
    eprintln!("#{} contract failed: {}", clause_kw, msg);
    eprintln!("  --> {}:{}", file, line);
    jet_runtime_exit();
}

/// Private structured producer channel used only when `jet prove` launches a
/// test harness. Length framing keeps user strings opaque; terminal text is
/// never parsed as evidence.
fn jet_proof_record(kind: u8, state: u8, name: &str, message: &str, file: &str, line: u32) {
    let Ok(path) = std::env::var("JET_TEST_PROOF_REPORT") else { return };
    let Ok(mut report) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
    use std::io::Write as _;
    if report.metadata().map(|m| m.len() == 0).unwrap_or(false) {
        let _ = report.write_all(b"JETTEST2");
    }
    let _ = report.write_all(&[kind, state]);
    let _ = report.write_all(&(line as u64).to_be_bytes());
    for bytes in [name.as_bytes(), message.as_bytes(), file.as_bytes()] {
        let _ = report.write_all(&(bytes.len() as u64).to_be_bytes());
        let _ = report.write_all(bytes);
    }
    let _ = report.flush();
}
// D-NUMOPS1: plain integer arithmetic traps on overflow (safe by default) — a
// silent corruption becomes a caught bug. Each arithmetic operator on a fixed-width
// integer lowers to one of these, which panic with the source location instead
// of wrapping. `wrapping(…)`/`saturating(…)`/`checked(…)` opt out at the use
// site. Floats and `#Numeric` distinct types keep the plain Rust operators.
trait JetArith: Copy {
    fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self;
    fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self;
    // D-NUMOPS1: a shift by a bit-count `>=` the value's width is undefined in C
    // and a panic in Rust — Jet traps it cleanly instead. The count comes in as
    // an `i128` so any integer width (signed or unsigned) reaches here losslessly.
    fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self;
    fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self;
}
macro_rules! jet_arith_impl {
    ($($t:ty),*) => { $(
        impl JetArith for $t {
            fn jet_add(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_add(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this addition overflows the value's type (the result is outside its range)")))
            }
            fn jet_sub(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_sub(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this subtraction overflows the value's type (the result is outside its range)")))
            }
            fn jet_mul(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_mul(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this multiplication overflows the value's type (the result is outside its range)")))
            }
            fn jet_div(self, rhs: Self, file: &str, line: u32) -> Self {
                self.checked_div(rhs).unwrap_or_else(|| jet_panic(file, line,
                    &format!("this division can't be done (dividing by zero, or overflow)")))
            }
            fn jet_rem(self, rhs: Self, file: &str, line: u32) -> Self {
                if rhs == 0 {
                    jet_panic(file, line, "divided by zero");
                }
                self.checked_rem(rhs).unwrap_or_else(|| jet_panic(file, line,
                    "attempt to calculate the remainder with overflow"))
            }
            fn jet_shl(self, bits: i128, file: &str, line: u32) -> Self {
                let w = (Self::BITS) as i128;
                if bits < 0 || bits >= w {
                    jet_panic(file, line, &format!(
                        "shifting left by {} bits is out of range (this type is {} bits wide)", bits, w));
                }
                self << (bits as u32)
            }
            fn jet_shr(self, bits: i128, file: &str, line: u32) -> Self {
                let w = (Self::BITS) as i128;
                if bits < 0 || bits >= w {
                    jet_panic(file, line, &format!(
                        "shifting right by {} bits is out of range (this type is {} bits wide)", bits, w));
                }
                self >> (bits as u32)
            }
        }
    )* };
}
jet_arith_impl!(i8, i16, i32, i64, u8, u16, u32, u64);
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
    jet_proof_record(2, 1, "panic", msg, file, line);
    let line_s = line.to_string();
    let margin = line_s.len();
    let pad = " ".repeat(margin);
    eprintln!("panic: {}", msg);
    eprintln!("  --> {}:{} in {}", file, line, fn_name);
    eprintln!("   {}|", pad);
    eprintln!("{} | {}", line_s, src_line);
    let col_offset = col.saturating_sub(1) as usize;
    let caret = "^".repeat(caret_len.max(1) as usize);
    eprintln!("   {}| {}{}", pad, " ".repeat(col_offset), caret);
    if !locals.is_empty() {
        eprintln!("locals: {}", locals);
    }
    if jet_runtime_should_unwind() {
        panic!("{} (at {}:{})", msg, file, line);
    }
    jet_runtime_exit();
}
/// E3002 / D-ERRCTX1=D: `?`-propagation trace in **dev** builds.
///
/// Gate is `not(jet_release)` (set by `--release` / `--profile=release`), not
/// `debug_assertions`: the default `jet run` profile passes `-O`, which turns
/// debug assertions off while still being a daily-driver /dev/ build.
///
/// Consecutive identical frames (same fn + file + line) collapse — Go wrap-noise
/// lesson — while each distinct site keeps its identity (Elixir lesson).
thread_local! {
    static JET_ERR_TRACE_LAST: std::cell::RefCell<Option<(String, String, u32)>> =
        const { std::cell::RefCell::new(None) };
}
fn jet_trace_err<T, E>(r: Result<T, E>, file: &str, line: u32, fn_name: &str) -> Result<T, E> {
    if cfg!(not(jet_release)) {
        if r.is_err() {
            let site = (fn_name.to_string(), file.to_string(), line);
            let fresh = JET_ERR_TRACE_LAST.with(|last| {
                let mut slot = last.borrow_mut();
                if slot.as_ref() == Some(&site) {
                    false
                } else {
                    *slot = Some(site);
                    true
                }
            });
            if fresh {
                eprintln!(
                    "error propagated from: {} ({}:{}) via ?",
                    fn_name, file, line
                );
            }
        } else {
            JET_ERR_TRACE_LAST.with(|last| *last.borrow_mut() = None);
        }
    }
    r
}
// D-FIXARR1: index/unpack/slice helpers accept `&[T]` so that both growable
// `Vec<T>` and fixed-size `[T; N]` stack arrays coerce in without `.to_vec()`.
fn jet_index_vec<T: Clone>(xs: &[T], i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    xs[i as usize].clone()
}
fn jet_index_vec_mut<'a, T>(
    xs: &'a mut [T],
    i: i64,
    file: &str,
    line: u32,
) -> &'a mut T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    &mut xs[i as usize]
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

fn jet_slice_range<T: Clone>(
    xs: &[T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> Vec<T> {
    xs[jet_checked_range_bounds(xs.len() as i64, range, "slice", file, line)].to_vec()
}
// D-DYNARRAY1 / D-SHAPE-PLACE1: range places produce zero-copy windows.
// Their bounds share `jet_range_bounds` with owned slicing and every engine.
// The returned lifetime is tied to `xs`; sema proves the window cannot outlive
// the owner or survive a storage-changing mutation.
fn jet_view_new<'a, T>(xs: &'a [T], a: i64, b: i64, file: &str, line: u32) -> &'a [T] {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    &xs[a as usize..=b as usize]
}

fn jet_view_mut_new<'a, T>(
    xs: &'a mut [T],
    a: i64,
    b: i64,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
    &mut xs[a as usize..=b as usize]
}

fn jet_views_mut_new<'a, T>(
    xs: &'a mut [T],
    ranges: &[(i64, i64, u32)],
    file: &str,
) -> Vec<&'a mut [T]> {
    let len = xs.len() as i64;
    let mut ordered = Vec::with_capacity(ranges.len());
    for (index, &(start, end, line)) in ranges.iter().enumerate() {
        if start < 0 || end < 0 || start > end || end >= len {
            jet_panic(
                file,
                line,
                &format!(
                    "can't view {} items from {} to {} (inclusive)",
                    len, start, end
                ),
            );
        }
        ordered.push((start as usize, end as usize + 1, index));
    }
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
    let bounds = ranges
        .iter()
        .map(|(range, line)| {
            let checked =
                jet_checked_range_bounds(xs.len() as i64, range, "view", file, *line);
            (checked.start as i64, checked.end as i64 - 1, *line)
        })
        .collect::<Vec<_>>();
    jet_views_mut_new(xs, &bounds, file)
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
    &xs[jet_checked_range_bounds(xs.len() as i64, range, "view", file, line)]
}

fn jet_view_mut_range_new<'a, T>(
    xs: &'a mut [T],
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a mut [T] {
    let bounds = jet_checked_range_bounds(xs.len() as i64, range, "view", file, line);
    &mut xs[bounds]
}

fn jet_check_view_bounds(len: i64, a: i64, b: i64, file: &str, line: u32) {
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!("can't view {} items from {} to {} (inclusive)", len, a, b),
        );
    }
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

fn jet_index_map<K: Ord + Clone, V: Clone>(
    m: &JetMap<K, V>,
    k: &K,
    file: &str,
    line: u32,
) -> V {
    match m.get(k) {
        Some(v) => v.clone(),
        None => jet_panic(file, line, &format!("the map has no entry for this key")),
    }
}
fn jet_map_insert<K: Ord + Clone, V: Clone>(m: &mut JetMap<K, V>, k: K, v: V) {
    m.insert(k, v);
}

/// D-MAP-MERGE1=E: merge `other` into a clone of `left`. Right wins on shared keys.
fn jet_map_merge<K: Ord + Clone, V: Clone>(
    left: &JetMap<K, V>,
    other: &JetMap<K, V>,
) -> JetMap<K, V> {
    let mut out = left.clone();
    for (k, v) in other {
        out.insert(k.clone(), v.clone());
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
    for (k, right) in other {
        match out.remove(k) {
            Some(left_v) => {
                let resolved = conflict(k, left_v, right.clone());
                out.insert(k.clone(), resolved);
            }
            None => {
                out.insert(k.clone(), right.clone());
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

fn jet_list_remove_value<T: Clone + PartialEq>(
    xs: &mut Vec<T>,
    value: T,
    _file: &str,
    _line: u32,
) -> JetOutcome<T, JetAbsent> {
    jet_outcome_of(
        xs.iter()
            .position(|item| *item == value)
            .map(|index| xs.remove(index)),
    )
}

fn jet_list_remove_slot<T: Clone>(xs: &mut Vec<T>, i: i64, file: &str, line: u32) -> JetOutcome<T, JetAbsent> {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(
            file,
            line,
            &format!(
                "the list has {} items, so position {} doesn't exist",
                len, i
            ),
        );
    }
    Ok(xs.remove(i as usize))
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
    xs.iter().filter(|item| *item == value).count() as i64
}

fn jet_list_concat<T: Clone>(left: &[T], right: &[T]) -> Vec<T> {
    let mut out = left.to_vec();
    out.extend(right.iter().cloned());
    out
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
fn jet_string_after(s: &String, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[i + sep.len()..].to_string(),
        None => s.clone(),
    }
}
fn jet_string_before(s: &String, sep: &str) -> String {
    match s.find(sep) {
        Some(i) => s[..i].to_string(),
        None => s.clone(),
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
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(
            file,
            line,
            &format!(
                "can't slice {} characters from {} to {} (inclusive)",
                len, a, b
            ),
        );
    }
    chars[a as usize..=b as usize].iter().collect()
}
fn jet_list_map<T, U, F>(xs: Vec<T>, f: F) -> Vec<U>
where
    F: Fn(&T) -> U,
{
    xs.iter().map(f).collect()
}
fn jet_list_map_mut<T, U, F>(xs: Vec<T>, mut f: F) -> Vec<U>
where
    F: FnMut(&T) -> U,
{
    xs.iter().map(|x| f(x)).collect()
}
fn jet_list_filter<T, F>(xs: Vec<T>, mut f: F) -> Vec<T>
where
    F: FnMut(&T) -> bool,
{
    xs.into_iter().filter(|x| f(x)).collect()
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
fn jet_list_each_ref<T, F>(xs: &Vec<T>, mut f: F)
where
    F: FnMut(&T),
{
    for x in xs.iter() {
        f(x);
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
fn jet_list_sort_by<T, K: Ord, F>(xs: &mut Vec<T>, f: F)
where
    F: FnMut(&T) -> K,
{
    xs.sort_by_key(f);
}
fn jet_list_reduce<T, U, F, I>(xs: I, init: U, mut f: F) -> U
where
    I: IntoIterator<Item = T>,
    F: FnMut(&U, &T) -> U,
{
    xs.into_iter().fold(init, |acc, x| f(&acc, &x))
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
    for (k, v) in &m {
        for (ik, iv) in f(k, v).iter() {
            out.insert(ik.clone(), iv.clone());
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

fn jet_process_spec_timeout(
    mut spec: jet_std::ProcessSpec,
    timeout: &jet_std::Duration,
) -> jet_std::ProcessSpec {
    spec.timeout_ms = Some(timeout.ms.max(0));
    spec
}
fn jet_process_spec_output_limit(
    mut spec: jet_std::ProcessSpec,
    output_limit: i64,
) -> jet_std::ProcessSpec {
    spec.output_limit = Some(output_limit.max(0));
    spec
}
fn jet_process_spec_detached(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.detached = true;
    spec
}
fn jet_process_stdio(mode: &jet_std::ProcessStreamMode) -> std::process::Stdio {
    match mode {
        // `Stream` and `Capture` both pipe — they differ only in which Jet API
        // is meant to drain the pipe (see `ProcessStreamMode` in CommonTypes.rs).
        jet_std::ProcessStreamMode::Stream | jet_std::ProcessStreamMode::Capture => {
            std::process::Stdio::piped()
        }
        jet_std::ProcessStreamMode::Inherit => std::process::Stdio::inherit(),
    }
}
fn jet_process_command(
    spec: &jet_std::ProcessSpec,
) -> Result<std::process::Command, jet_std::IOError> {
    if spec.cmd.is_empty() {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            None,
            None,
            Some("process command needs at least one word".to_string()),
        )));
    }
    let mut command = std::process::Command::new(&spec.cmd[0]);
    command.args(&spec.cmd[1..]);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    // D-ENV-MUTATE1=A: clone one logical-environment snapshot under its read
    // lock, then compose ProcessSpec overrides in owned memory. Every launch is
    // untorn and never rereads the mutable host environment.
    let mut child_env = if spec.env_clear {
        Vec::new()
    } else {
        jet_std_env_snapshot_raw()
    };
    for (name, value) in &spec.env_set {
        jet_env_validate_name(name).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve, Some(name.clone()), None, Some(error.jet_show()),
        )))?;
        jet_env_validate_value(value).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve, Some(name.clone()), None, Some(error.jet_show()),
        )))?;
        let os_name = std::ffi::OsString::from(name);
        child_env.retain(|(candidate, _)| {
            !jet_env_key_eq(candidate.as_os_str(), os_name.as_os_str())
        });
        child_env.push((os_name, std::ffi::OsString::from(value)));
    }
    for name in &spec.env_remove {
        jet_env_validate_name(name).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve, Some(name.clone()), None, Some(error.jet_show()),
        )))?;
        let name = std::ffi::OsStr::new(name);
        child_env.retain(|(candidate, _)| !jet_env_key_eq(candidate.as_os_str(), name));
    }
    command.env_clear();
    command.envs(child_env);
    // D-PROCESS1=A: no `.stdin(...)` call (default) closes the child's stdin —
    // no accidental terminal/parent-stdin inheritance.
    command.stdin(match &spec.stdin {
        Some(mode) => jet_process_stdio(mode),
        None => std::process::Stdio::null(),
    });
    command.stdout(jet_process_stdio(&spec.stdout));
    command.stderr(jet_process_stdio(&spec.stderr));
    Ok(command)
}
fn jet_process_spec_spawn(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    let mut command = jet_process_command(spec)?;
    if spec.detached {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, spec.cmd.first().cloned(), error)
    })?;
    Ok(jet_std::ProcessChild {
        stdin: std::rc::Rc::new(std::cell::RefCell::new(child.stdin.take())),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(
            child.stdout.take().map(std::io::BufReader::new),
        )),
        stderr: std::rc::Rc::new(std::cell::RefCell::new(
            child.stderr.take().map(std::io::BufReader::new),
        )),
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(child))),
        timeout_ms: spec.timeout_ms,
        started: std::time::Instant::now(),
    })
}
fn jet_process_drain_reader<R>(
    reader: Option<std::io::BufReader<R>>,
) -> Option<std::thread::JoinHandle<std::io::Result<String>>>
where
    R: std::io::Read + Send + 'static,
{
    reader.map(|mut reader| {
        std::thread::spawn(move || {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut reader, &mut text)?;
            Ok(text)
        })
    })
}
fn jet_process_start_output_drain(
    child: &jet_std::ProcessChild,
) -> (
    Option<std::thread::JoinHandle<std::io::Result<String>>>,
    Option<std::thread::JoinHandle<std::io::Result<String>>>,
) {
    let stdout = child.stdout.borrow_mut().take();
    let stderr = child.stderr.borrow_mut().take();
    (
        jet_process_drain_reader(stdout),
        jet_process_drain_reader(stderr),
    )
}
fn jet_process_finish_output_drain(
    drain: Option<std::thread::JoinHandle<std::io::Result<String>>>,
    stream: &'static str,
) -> Result<String, jet_std::IOError> {
    let Some(drain) = drain else {
        return Ok(String::new());
    };
    drain
        .join()
        .map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Read,
                Some(stream.to_string()),
                "process output reader panicked",
            )
        })?
        .map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Read,
                Some(stream.to_string()),
                error,
            )
        })
}
fn jet_process_collect_output(
    drains: (
        Option<std::thread::JoinHandle<std::io::Result<String>>>,
        Option<std::thread::JoinHandle<std::io::Result<String>>>,
    ),
) -> Result<(String, String), jet_std::IOError> {
    let output = jet_process_finish_output_drain(drains.0, "process stdout")?;
    let errors = jet_process_finish_output_drain(drains.1, "process stderr")?;
    Ok((output, errors))
}
fn jet_process_spec_run_inner(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    let child = jet_process_spec_spawn(spec)?;
    let result = jet_process_child_wait(&child)?;
    if let Some(limit) = spec.output_limit {
        if (result.output.len() + result.errors.len()) as i64 > limit {
            return Err(jet_std::IOError::other(jet_std::IOOperation::Read, None, "process output exceeded output_limit"));
        }
    }
    Ok(result)
}
fn jet_process_spec_run(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    jet_process_spec_run_inner(spec)
}
fn jet_process_child_id(child: &jet_std::ProcessChild) -> i64 {
    child
        .inner
        .borrow()
        .as_ref()
        .map(|c| c.id() as i64)
        .unwrap_or(0)
}
fn jet_process_child_wait(
    child: &jet_std::ProcessChild,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    // Capture pipes must be drained while the child runs. Waiting first can
    // deadlock when either pipe fills; stdout and stderr need independent
    // readers because a child may fill both concurrently. Stream consumers
    // keep their earlier reads, and wait drains only the remaining bytes.
    let drains = jet_process_start_output_drain(child);
    let mut timed_out = false;
    let status = loop {
        let mut slot = child.inner.borrow_mut();
        let Some(inner) = slot.as_mut() else {
            let (output, errors) = jet_process_collect_output(drains)?;
            return Ok(jet_std::ProcessResult {
                code: 0,
                success: true,
                signal: None,
                timed_out: false,
                output,
                errors,
            });
        };
        if let Some(status) = inner.try_wait().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))? {
            break status;
        }
        if let Some(timeout) = child.timeout_ms {
            if child.started.elapsed() >= std::time::Duration::from_millis(timeout as u64) {
                inner.kill().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
                timed_out = true;
                break inner.wait().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
            }
        }
        drop(slot);
        // D-TASKRUNTIME1=A: process waits are scheduler wait points. Parking
        // here keeps the worker available and makes inherited cancellation and
        // deadlines wake the wait exactly like channel, timer, and I/O waits.
        jet_scheduler_park_ms("process wait", 10);
    };
    child.inner.borrow_mut().take();
    let (output, errors) = jet_process_collect_output(drains)?;
    let code = status.code().unwrap_or(-1) as i64;
    Ok(jet_std::ProcessResult {
        code,
        success: status.success(),
        signal: None,
        timed_out,
        output,
        errors,
    })
}
fn jet_process_child_kill(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    if let Some(inner) = child.inner.borrow_mut().as_mut() {
        inner.kill().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
    }
    Ok(())
}
fn jet_process_child_terminate(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    jet_process_child_kill(child)
}
fn jet_process_child_interrupt(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    jet_process_child_kill(child)
}
// D-PROCESS1=A: `child.stdin` is a writer handle (`.write(text)`); `child.stdout`/
// `child.stderr` are streaming reader handles consumed only via
// `loop line; child.stdout.lines() { ... }` (mirrors `FileReader`/`StdinHandle`
// — sema restricts the field access + `.lines()` result to that position, E2502).
fn jet_process_stdin_write(
    handle: &std::rc::Rc<std::cell::RefCell<Option<std::process::ChildStdin>>>,
    text: &String,
) -> Result<(), jet_std::IOError> {
    if let Some(stdin) = handle.borrow_mut().as_mut() {
        std::io::Write::write_all(stdin, text.as_bytes()).map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Write, Some("process stdin".to_string()), error))?;
    }
    Ok(())
}
fn jet_process_child_read_line<R: std::io::Read>(
    reader: &mut Option<std::io::BufReader<R>>,
) -> Result<Option<String>, jet_std::IOError> {
    let Some(reader) = reader.as_mut() else {
        return Ok(None);
    };
    let mut line = String::new();
    let n = std::io::BufRead::read_line(reader, &mut line).map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Read, Some("process output".to_string()), error))?;
    if n == 0 {
        Ok(None)
    } else {
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }
}
fn jet_process_stream_next_line<R: std::io::Read>(
    handle: &std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<R>>>>,
) -> Result<Option<String>, jet_std::IOError> {
    jet_process_child_read_line(&mut handle.borrow_mut())
}

fn jet_std_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
fn jet_std_math_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}
fn jet_std_math_floor(x: f64) -> f64 {
    x.floor()
}
fn jet_std_math_ceil(x: f64) -> f64 {
    x.ceil()
}
fn jet_std_math_round(x: f64) -> i64 {
    x.round() as i64
}
fn jet_std_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}
fn jet_std_math_checked_pow(base: i64, exp: i64) -> Option<i64> {
    if exp < 0 {
        return None;
    }
    base.checked_pow(exp as u32)
}
fn jet_std_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}
fn jet_std_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
fn jet_std_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_std_math_gcd(a, b)).saturating_mul(b).abs()
    }
}
// D-FLOATW1 (ratified 2026-06-22): F32 variants — sqrt(F32)->F32, pow(F32,F32)->F32 etc.
// F32 is a real precision choice, not just storage; no silent widening to f64 (I3).
fn jet_std_math_sqrt_f32(x: f32) -> f32 {
    x.sqrt()
}
fn jet_std_math_pow_f32(a: f32, b: f32) -> f32 {
    a.powf(b)
}
fn jet_std_math_floor_f32(x: f32) -> f32 {
    x.floor()
}
fn jet_std_math_ceil_f32(x: f32) -> f32 {
    x.ceil()
}

thread_local! { static JET_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173); }
fn jet_rng_next() -> u64 {
    JET_RNG.with(|cell| {
        let mut x = cell.get();
        x ^= x << 7;
        x ^= x >> 9;
        x = x.wrapping_mul(0x9e3779b97f4a7c15);
        cell.set(x);
        x
    })
}
fn jet_std_random_seed(n: i64) {
    JET_RNG.with(|cell| cell.set(n as u64));
}
fn jet_std_random_int(low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (jet_rng_next() % ((high - low + 1) as u64)) as i64
}
fn jet_std_random_float() -> f64 {
    (jet_rng_next() as f64) / (u64::MAX as f64)
}
fn jet_std_random_float_open() -> f64 {
    let x = jet_std_random_float();
    if x <= 0.0 { f64::MIN_POSITIVE } else { x }
}
fn jet_std_random_float_range(low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * jet_std_random_float()
}
fn jet_std_random_bool(p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        jet_std_random_float() < p
    }
}
fn jet_std_random_normal(mean: f64, stddev: f64) -> f64 {
    let u1 = jet_std_random_float_open();
    let u2 = jet_std_random_float();
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}
fn jet_std_random_exponential(lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -jet_std_random_float_open().ln() / lambda
}
fn jet_std_random_pick<T: Clone>(xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[jet_std_random_int(0, xs.len() as i64 - 1) as usize].clone())
    }
}
fn jet_std_random_weighted_pick<T: Clone>(xs: &Vec<T>, weights: &Vec<f64>) -> Option<T> {
    if xs.is_empty() || xs.len() != weights.len() {
        return None;
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut needle = jet_std_random_float_range(0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Some(item.clone());
        }
        needle -= w;
    }
    xs.last().cloned()
}
fn jet_std_random_sample<T: Clone>(xs: &Vec<T>, k: i64) -> Vec<T> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.clone();
    for i in 0..want {
        let j = jet_std_random_int(i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}
fn jet_std_random_shuffle<T>(xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_std_random_int(0, i as i64) as usize;
        xs.swap(i, j);
    }
}
// D-RANDSPLIT1=A: PRNG bytes via the ambient SplitMix64 state — fast, seedable,
// NOT cryptographically secure. Use for simulation, testing, or shuffles only.
fn jet_std_random_bytes(n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(jet_rng_next() as u8);
    }
    out
}
fn jet_std_random_split(seed: i64) -> jet_std::Rng {
    let mixed = (seed as u64) ^ jet_rng_next().rotate_left(17);
    jet_std::Rng { state: mixed }
}
// D-CRYPTO-RNG1=A: cryptographic bytes use the shared fail-closed OS provider.
// Edition 2026 keeps this infallible Rust shim; failure takes the ratified
// E3001/exit-70 compatibility path and never returns weak or partial bytes.
fn jet_std_crypto_random_bytes(n: i64) -> Vec<u8> {
    match jet_crypto_entropy_bytes(n) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("Error [E3001]: panic: core.crypto.random.bytes: {error}");
            std::process::exit(70);
        }
    }
}

//! Concrete browser adapter for the package provider. Link the wasm imports
//! returned by OnnxRuntimeWeb.js, then bind the instance's exports. No JS model
//! API or compiler syntax is defined here. Both runtimes use the parent module's
//! tensor checks, ONNX dtype IDs, provenance, and backend error classifier.

pub const HOST_JAVASCRIPT: &str = include_str!("OnnxRuntimeWeb.js");
pub const WORKER_JAVASCRIPT: &str = include_str!("OnnxRuntimeWebWorker.js");

#[cfg(target_arch = "wasm32")]
pub use wasm::BrowserWebRuntime;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::task::{Context, Poll, Waker};

    #[link(wasm_import_module = "jet_onnx_web")]
    extern "C" {
        fn start(job: u32, session: u32, operation: u32, ptr: *const u8, len: u32);
        fn cancel(job: u32);
        fn release(session: u32);
        pub(super) fn now() -> f64;
    }

    struct Reply {
        bytes: Vec<u8>,
        allocated: bool,
        finished: bool,
        failed_allocation: bool,
        waker: Option<Waker>,
    }

    thread_local! {
        static NEXT_ID: Cell<u32> = const { Cell::new(1) };
        static REPLIES: RefCell<BTreeMap<u32, Reply>> = const { RefCell::new(BTreeMap::new()) };
    }

    fn id(package: &str) -> Result<u32, ModelError> {
        NEXT_ID.with(|next| {
            let id = next.get();
            let successor = id.checked_add(1).ok_or_else(|| ModelError::ResourceLimit {
                package: package.into(), reason: "browser model handle space exhausted".into(),
            })?;
            next.set(successor);
            Ok(id)
        })
    }

    // The host writes only buffers allocated and owned by this bridge, never a
    // caller-selected Rust pointer. The allocation remains live until finish.
    #[no_mangle]
    pub extern "C" fn jet_onnx_web_reply_alloc(job: u32, length: u32) -> u32 {
        REPLIES.with(|replies| {
            let mut replies = replies.borrow_mut();
            let Some(reply) = replies.get_mut(&job) else { return 0; };
            if reply.allocated || reply.finished || length == 0 { return 0; }
            if reply.bytes.try_reserve_exact(length as usize).is_err() {
                reply.failed_allocation = true;
                return 0;
            }
            reply.bytes.resize(length as usize, 0);
            reply.allocated = true;
            reply.bytes.as_mut_ptr() as u32
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_onnx_web_reply_finish(job: u32) {
        let wake = REPLIES.with(|replies| {
            let mut replies = replies.borrow_mut();
            let reply = replies.get_mut(&job)?;
            reply.finished = true;
            reply.waker.take()
        });
        if let Some(wake) = wake { wake.wake(); }
    }

    struct Pending<'a> {
        job: u32,
        package: &'a str,
        cancellation: Option<&'a CancellationToken>,
        completed: bool,
    }

    impl<'a> Pending<'a> {
        fn start(package: &'a str, session: u32, operation: u32, request: &[u8], cancellation: Option<&'a CancellationToken>) -> Result<Self, ModelError> {
            let len = u32::try_from(request.len()).map_err(|_| ModelError::BufferLimit {
                package: package.into(), bytes: request.len() as u64,
            })?;
            let job = id(package)?;
            REPLIES.with(|replies| replies.borrow_mut().insert(job, Reply {
                bytes: Vec::new(), allocated: false, finished: false, failed_allocation: false, waker: None,
            }));
            let pending = Self { job, package, cancellation, completed: false };
            // SAFETY: the embedding copies the request before this import
            // returns. Completion arrives asynchronously through owned slots.
            unsafe { start(job, session, operation, request.as_ptr(), len); }
            Ok(pending)
        }
    }

    impl Future for Pending<'_> {
        type Output = Result<Vec<u8>, ModelError>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if let Some(token) = self.cancellation {
                let mut waiters = token.0.waiters.lock().unwrap_or_else(|e| e.into_inner());
                if token.is_cancelled() {
                    // Drop kills foreign work, even if ORT currently occupies
                    // its worker's event loop. No polling timer or spin loop.
                    return Poll::Ready(Err(ModelError::Cancelled { package: self.package.into() }));
                }
                waiters.insert(self.job, cx.waker().clone());
            }
            let result = REPLIES.with(|replies| {
                let mut replies = replies.borrow_mut();
                let reply = replies.get_mut(&self.job).expect("owned browser reply");
                if !reply.finished {
                    if !reply.waker.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
                        reply.waker = Some(cx.waker().clone());
                    }
                    return None;
                }
                Some(replies.remove(&self.job).expect("owned browser reply"))
            });
            match result {
                None => Poll::Pending,
                Some(reply) => {
                    self.completed = true;
                    Poll::Ready(if reply.failed_allocation {
                        Err(ModelError::ResourceLimit { package: self.package.into(), reason: "browser reply allocation failed".into() })
                    } else { Ok(reply.bytes) })
                }
            }
        }
    }

    impl Drop for Pending<'_> {
        fn drop(&mut self) {
            if let Some(token) = self.cancellation {
                token.0.waiters.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.job);
            }
            if !self.completed {
                REPLIES.with(|replies| replies.borrow_mut().remove(&self.job));
                // SAFETY: job IDs never alias, and cancellation is idempotent.
                unsafe { cancel(self.job); }
            }
        }
    }

    pub struct BrowserWebRuntime;

    struct Session {
        id: u32,
        package: ModelPackage,
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // SAFETY: the host releases only this owned session's worker and
            // retained model bytes. It must not throw from this import.
            unsafe { release(self.id); }
        }
    }

    impl WebRuntime for BrowserWebRuntime {
        fn open<'a>(&'a self, request: WebOpenRequest<'a>) -> WebFuture<'a, Box<dyn WebSessionHandle>> {
            Box::pin(async move {
                let package = request.package;
                if request.provenance.runtime_sha256 != ONNX_RUNTIME_WEB_ARTIFACT_SHA256 {
                    return Err(provenance_error(&package.package, "browser runtime is not the admitted web package"));
                }
                let session = Session { id: id(&package.package)?, package: package.clone() };
                let mut packet = Packet::default();
                packet.text(ONNX_RUNTIME_VERSION)?;
                packet.text(ONNX_RUNTIME_LICENSE)?;
                packet.text(ONNX_RUNTIME_WEB_ARTIFACT_SHA256)?;
                // Select the CPU-only build, not JSEP/WebGPU or a fallback list.
                packet.text("package/dist/ort-wasm-simd-threaded.mjs")?;
                packet.text("package/dist/ort-wasm-simd-threaded.wasm")?;
                packet.text(&request.provenance.render())?;
                packet.blob(request.graph)?;
                let bytes = Pending::start(&package.package, session.id, 0, &packet.0, Some(request.cancellation))?.await?;
                let mut reply = match Reader::new(&bytes, &package.package, "CreateSession")? {
                    RuntimeReply::Success(reply) => reply,
                    RuntimeReply::BufferLimit { bytes, .. } => {
                        return Err(ModelError::BufferLimit { package: package.package.clone(), bytes });
                    }
                };
                for expected in [&package.contract.inputs, &package.contract.outputs] {
                    let count = reply.u32()? as usize;
                    if count != expected.len() {
                        return Err(ModelError::SignatureMismatch { package: package.package.clone(), expected: format!("{} endpoints", expected.len()), actual: format!("{count} endpoints") });
                    }
                    for expected in expected {
                        let name = reply.text()?;
                        let dtype = reply.dtype()?;
                        let dimensions = reply.dimensions()?;
                        validate_declared_signature(package, expected, &name, RuntimeSignature { dtype, dimensions })?;
                    }
                }
                reply.end()?;
                Ok(Box::new(session) as Box<dyn WebSessionHandle>)
            })
        }
    }

    impl WebSessionHandle for Session {
        fn run<'a>(&'a mut self, inputs: &'a [TensorData], cancellation: &'a CancellationToken) -> WebFuture<'a, Vec<TensorData>> {
            Box::pin(async move {
                let input_bytes = validate_inputs(&self.package, inputs, cancellation)?;
                let mut packet = Packet::default();
                // This allowance is computed by the contract owner. The host
                // uses it only to avoid copying an inadmissible foreign buffer.
                packet.u64(self.package.contract.max_buffer_bytes.unwrap_or(u64::MAX).saturating_sub(input_bytes));
                packet.count(inputs.len())?;
                for input in inputs {
                    packet.text(&input.spec.name)?;
                    packet.u32(map_tensor_dtype(input.spec.dtype) as u32);
                    packet.count(input.spec.shape.dimensions.len())?;
                    for dimension in &input.spec.shape.dimensions {
                        let TensorDimension::Static(value) = dimension else { unreachable!("validated runtime shape") };
                        if *value > u64::from(u32::MAX) {
                            return Err(ModelError::ResourceLimit { package: self.package.package.clone(), reason: "tensor dimension exceeds the pinned wasm32 ABI".into() });
                        }
                        packet.u64(*value);
                    }
                    packet.blob(&input.bytes)?;
                }
                let bytes = Pending::start(&self.package.package, self.id, 1, &packet.0, Some(cancellation))?.await?;
                let mut reply = match Reader::new(&bytes, &self.package.package, "Run")? {
                    RuntimeReply::Success(reply) => reply,
                    RuntimeReply::BufferLimit { bytes, outputs: Some(outputs) } => {
                        // A rejected transfer still carries its complete
                        // observed endpoint metadata. Check the contract
                        // before either output-only or combined limits so a
                        // signature error cannot be replaced by a buffer one.
                        self.package.check_outputs(&outputs)?;
                        self.package.check_buffer(bytes)?;
                        let total = input_bytes.checked_add(bytes).ok_or_else(|| ModelError::BufferLimit {
                            package: self.package.package.clone(), bytes: u64::MAX,
                        })?;
                        self.package.check_buffer(total)?;
                        return Err(backend_error(&self.package.package, WEB_RUNTIME_ARTIFACT_PATH, "host rejected an admissible output transfer"));
                    }
                    RuntimeReply::BufferLimit { bytes, outputs: None } => {
                        self.package.check_buffer(bytes)?;
                        let total = input_bytes.checked_add(bytes).ok_or_else(|| ModelError::BufferLimit {
                            package: self.package.package.clone(), bytes: u64::MAX,
                        })?;
                        self.package.check_buffer(total)?;
                        return Err(backend_error(&self.package.package, WEB_RUNTIME_ARTIFACT_PATH, "host rejected an admissible output transfer"));
                    }
                };
                let count = reply.u32()? as usize;
                if count != self.package.contract.outputs.len() { return Err(reply.error("runtime output count differs from the package")); }
                let mut outputs = Vec::with_capacity(count);
                for _ in 0..count {
                    let name = reply.text()?;
                    let dtype = reply.dtype()?;
                    let dimensions = reply.dimensions()?.into_iter().map(|d| u64::try_from(d).map_err(|_| reply.error("negative output dimension"))).collect::<Result<Vec<_>, _>>()?;
                    let bytes = reply.blob()?.to_vec();
                    outputs.push(TensorData { spec: TensorSpec::runtime(name, dtype, dimensions), bytes });
                }
                reply.end()?;
                let output_bytes = validate_outputs(&self.package, &outputs)?;
                let total_bytes = input_bytes.checked_add(output_bytes).ok_or_else(|| ModelError::BufferLimit {
                    package: self.package.package.clone(), bytes: u64::MAX,
                })?;
                self.package.check_buffer(total_bytes)?;
                Ok(outputs)
            })
        }
    }

    enum RuntimeReply<'a> {
        Success(Reader<'a>),
        BufferLimit { bytes: u64, outputs: Option<Vec<TensorSpec>> },
    }

    #[derive(Default)]
    struct Packet(Vec<u8>);
    impl Packet {
        fn u32(&mut self, value: u32) { self.0.extend_from_slice(&value.to_le_bytes()); }
        fn u64(&mut self, value: u64) { self.0.extend_from_slice(&value.to_le_bytes()); }
        fn count(&mut self, value: usize) -> Result<(), ModelError> {
            self.u32(u32::try_from(value).map_err(|_| backend_error("<browser>", WEB_RUNTIME_ARTIFACT_PATH, "browser packet exceeds wasm32 ABI"))?);
            Ok(())
        }
        fn blob(&mut self, bytes: &[u8]) -> Result<(), ModelError> { self.count(bytes.len())?; self.0.extend_from_slice(bytes); Ok(()) }
        fn text(&mut self, value: &str) -> Result<(), ModelError> { self.blob(value.as_bytes()) }
    }

    struct Reader<'a> { bytes: &'a [u8], package: &'a str }
    impl<'a> Reader<'a> {
        fn new(bytes: &'a [u8], package: &'a str, operation: &str) -> Result<RuntimeReply<'a>, ModelError> {
            let mut reader = Self { bytes, package };
            match reader.u32()? {
                0 => Ok(RuntimeReply::Success(reader)),
                1 => { let code = reader.u32()? as i32; let message = reader.text()?; Err(classify_backend_failure(package, operation, code, &message)) }
                2 => { let reason = reader.text()?; Err(provenance_error(package, reason)) }
                3 => { let reason = reader.text()?; Err(ModelError::ResourceLimit { package: package.into(), reason }) }
                4 => {
                    let bytes = reader.u64()?;
                    let count = reader.u32()?;
                    if count == u32::MAX {
                        reader.end()?;
                        return Ok(RuntimeReply::BufferLimit { bytes, outputs: None });
                    }
                    let count = count as usize;
                    if count > reader.bytes.len() / 12 {
                        return Err(reader.error("truncated browser output metadata"));
                    }
                    let mut outputs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let name = reader.text()?;
                        let dtype = reader.dtype()?;
                        let dimensions = reader
                            .dimensions()?
                            .into_iter()
                            .map(|dimension| u64::try_from(dimension).map_err(|_| reader.error("negative output dimension")))
                            .collect::<Result<Vec<_>, _>>()?;
                        outputs.push(TensorSpec::runtime(name, dtype, dimensions));
                    }
                    reader.end()?;
                    Ok(RuntimeReply::BufferLimit { bytes, outputs: Some(outputs) })
                }
                _ => Err(reader.error("unknown browser reply status")),
            }
        }
        fn error(&self, reason: &str) -> ModelError { backend_error(self.package, WEB_RUNTIME_ARTIFACT_PATH, reason) }
        fn take(&mut self, length: usize) -> Result<&'a [u8], ModelError> {
            if length > self.bytes.len() { return Err(self.error("truncated browser reply")); }
            let (value, rest) = self.bytes.split_at(length); self.bytes = rest; Ok(value)
        }
        fn u32(&mut self) -> Result<u32, ModelError> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
        fn u64(&mut self) -> Result<u64, ModelError> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
        fn blob(&mut self) -> Result<&'a [u8], ModelError> { let n = self.u32()? as usize; self.take(n) }
        fn text(&mut self) -> Result<String, ModelError> { String::from_utf8(self.blob()?.to_vec()).map_err(|_| self.error("invalid browser UTF-8")) }
        fn dtype(&mut self) -> Result<TensorDType, ModelError> { map_onnx_dtype(self.u32()? as i32).ok_or_else(|| self.error("runtime endpoint dtype is outside the tensor contract")) }
        fn dimensions(&mut self) -> Result<Vec<i64>, ModelError> {
            let count = self.u32()? as usize;
            if count > self.bytes.len() / 8 { return Err(self.error("truncated browser dimensions")); }
            (0..count).map(|_| self.u64().map(|d| d as i64)).collect()
        }
        fn end(&self) -> Result<(), ModelError> { if self.bytes.is_empty() { Ok(()) } else { Err(self.error("trailing browser reply bytes")) } }
    }

}
#[cfg(target_arch = "wasm32")]
pub(super) fn now_ns() -> u64 {
    // SAFETY: performance.now is the embedding's monotonic clock, in ns.
    unsafe { wasm::now() as u64 }
}

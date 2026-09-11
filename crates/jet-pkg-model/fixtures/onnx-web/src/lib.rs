//! Offline, model-independent proof entrypoint. The browser and native binary
//! consume the identical request packet and call the real package/provider API.
//! No fixture model, operator implementation, or expected output is embedded.
use jet_rt::model::{
    EmbeddingBatch, EmbeddingIndex, ModelArtifact, ModelContract, ModelDescriptor, ModelError,
    ModelPackage, ModelSource, TensorDType, TensorDimension, TensorSpec,
};
use jet_rt::model::provider::{
    CancellationToken, OnnxRuntimePolicy, OnnxRuntimeProvider, OutputPolicy, RuntimePin,
    TensorData, UnavailablePeakMemory,
};
use std::collections::BTreeMap;

fn error(reason: impl Into<String>) -> ModelError {
    ModelError::Artifact { package: "<proof>".into(), path: "<request>".into(), reason: reason.into() }
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ModelError> {
        if n > self.0.len() { return Err(error("truncated proof packet")); }
        let (bytes, rest) = self.0.split_at(n); self.0 = rest; Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32, ModelError> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn u64(&mut self) -> Result<u64, ModelError> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    fn blob(&mut self) -> Result<&'a [u8], ModelError> { let n = self.u32()? as usize; self.take(n) }
    fn text(&mut self) -> Result<String, ModelError> { String::from_utf8(self.blob()?.to_vec()).map_err(|_| error("invalid proof UTF-8")) }
    fn count(&mut self) -> Result<usize, ModelError> {
        let n = self.u32()? as usize;
        if n > self.0.len() { return Err(error("proof count exceeds packet")); }
        Ok(n)
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);
impl Writer {
    fn u32(&mut self, n: u32) { self.0.extend_from_slice(&n.to_le_bytes()); }
    fn u64(&mut self, n: u64) { self.0.extend_from_slice(&n.to_le_bytes()); }
    fn blob(&mut self, bytes: &[u8]) { self.u32(bytes.len().try_into().expect("proof reply fits wasm32")); self.0.extend_from_slice(bytes); }
    fn text(&mut self, text: &str) { self.blob(text.as_bytes()); }
}

fn read_artifact(input: &mut Reader<'_>) -> Result<ModelArtifact, ModelError> {
    Ok(ModelArtifact {
        path: input.text()?,
        sha256: input.text()?,
    })
}

fn read_optional_u64(input: &mut Reader<'_>) -> Result<Option<u64>, ModelError> {
    let value = input.u64()?;
    Ok((value != u64::MAX).then_some(value))
}

fn read_tensor(input: &mut Reader<'_>) -> Result<TensorSpec, ModelError> {
    let name = input.text()?;
    let dtype = TensorDType::parse(&input.text()?).ok_or_else(|| error("unknown descriptor tensor dtype"))?;
    let mut dimensions = Vec::new();
    for _ in 0..input.count()? {
        dimensions.push(match input.u32()? {
            0 => TensorDimension::Static(input.u64()?),
            1 => TensorDimension::Dynamic {
                name: input.text()?,
                min: input.u64()?,
                max: input.u64()?,
            },
            _ => return Err(error("unknown descriptor dimension kind")),
        });
    }
    Ok(TensorSpec {
        name,
        dtype,
        shape: jet_rt::model::TensorShape { dimensions },
    })
}

fn read_tensor_list(input: &mut Reader<'_>) -> Result<Vec<TensorSpec>, ModelError> {
    let mut tensors = Vec::new();
    for _ in 0..input.count()? {
        tensors.push(read_tensor(input)?);
    }
    Ok(tensors)
}

fn read_descriptor(input: &mut Reader<'_>, output: &str) -> Result<ModelDescriptor, ModelError> {
    let package = input.text()?;
    let signature_name = match input.u32()? {
        0 => None,
        1 => Some(input.text()?),
        _ => return Err(error("invalid descriptor signature marker")),
    };
    let package_version = input.text()?;
    let license = input.text()?;
    let graph = read_artifact(input)?;
    let weights = read_artifact(input)?;
    let tokenizer = read_artifact(input)?;
    let adapter = match input.u32()? {
        0 => None,
        1 => Some(read_artifact(input)?),
        _ => return Err(error("invalid descriptor adapter marker")),
    };
    let preprocessing = input.text()?;
    let pooling = input.text()?;
    let normalization = input.text()?;
    let output_meaning = input.text()?;
    let metric = input.text()?;
    let provider = input.text()?;
    let custom_operators = match input.u32()? {
        0 => false,
        1 => true,
        _ => return Err(error("invalid descriptor custom-operator marker")),
    };
    Ok(ModelDescriptor {
        package,
        output: output.to_string(),
        signature_name,
        package_version,
        license,
        graph,
        weights,
        tokenizer,
        adapter,
        preprocessing,
        pooling,
        normalization,
        output_meaning,
        metric,
        contract: ModelContract {
            inputs: read_tensor_list(input)?,
            outputs: read_tensor_list(input)?,
            provider,
            custom_operators,
            max_context: read_optional_u64(input)?,
            max_batch: read_optional_u64(input)?,
            max_buffer_bytes: read_optional_u64(input)?,
        },
    })
}

fn embedding_tensor(batch: &EmbeddingBatch) -> Result<TensorData, ModelError> {
    let dimension = usize::try_from(batch.space().dimension())
        .map_err(|_| error("embedding dimension exceeds host size"))?;
    if batch.values().is_empty()
        || batch
            .values()
            .iter()
            .any(|value| value.len() != dimension || value.iter().any(|item| !item.is_finite()))
    {
        return Err(error("provider returned an invalid embedding batch"));
    }
    let mut bytes = Vec::with_capacity(
        batch
            .values()
            .len()
            .checked_mul(dimension)
            .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
            .ok_or_else(|| error("embedding output byte count overflows"))?,
    );
    for value in batch.values() {
        for item in value {
            bytes.extend_from_slice(&item.to_le_bytes());
        }
    }
    TensorData::new(
        TensorSpec::runtime("embedding", TensorDType::F32, [batch.values().len() as u64, dimension as u64]),
        bytes,
    )
}

pub fn failure(error: ModelError) -> Vec<u8> {
    let mut out = Writer::default(); out.u32(1); out.text(error.code()); out.text(&format!("{error:?}")); out.0
}

pub async fn execute(bytes: Vec<u8>, runtime: Option<std::path::PathBuf>, cancellation: CancellationToken) -> Result<Vec<u8>, ModelError> {
    let mut input = Reader(&bytes);
    let output = input.text()?;
    let descriptor = read_descriptor(&mut input, &output)?;
    let output_policy = match input.text()?.as_str() {
        "exact" => OutputPolicy::Exact,
        "approximate" => OutputPolicy::Approximate,
        _ => return Err(error("proof requires exact or approximate output policy")),
    };
    let warm_runs = input.u32()? as usize;
    let resume_after_cancel = input.u32()? != 0;
    let mut operators = Vec::new();
    for _ in 0..input.count()? { operators.push(input.text()?); }
    let mut artifacts = BTreeMap::new();
    for _ in 0..input.count()? {
        let name = input.text()?; let data = input.blob()?.to_vec();
        if artifacts.insert(name, data).is_some() { return Err(error("duplicate proof artifact")); }
    }
    let mut inputs = Vec::new();
    for _ in 0..input.count()? {
        let name = input.text()?;
        let dtype = TensorDType::parse(&input.text()?).ok_or_else(|| error("unknown tensor dtype"))?;
        let mut shape = Vec::new();
        for _ in 0..input.count()? { shape.push(input.u64()?); }
        inputs.push(TensorData::new(TensorSpec::runtime(name, dtype, shape), input.blob()?.to_vec())?);
    }
    let documents = if input.0.is_empty() {
        None
    } else {
        match input.u32()? {
            0 => None,
            1 => {
                let mut documents = Vec::new();
                for _ in 0..input.count()? {
                    documents.push(input.text()?);
                }
                Some(documents)
            }
            _ => return Err(error("invalid document request marker")),
        }
    };
    if !input.0.is_empty() { return Err(error("trailing proof packet bytes")); }
    let package = ModelPackage::from_descriptor(descriptor)?;
    let policy = OnnxRuntimePolicy::cpu(operators)?.with_output_policy(output_policy);
    #[cfg(not(target_arch = "wasm32"))]
    let provider = OnnxRuntimeProvider::native(RuntimePin::official_linux_x64(runtime.ok_or_else(|| error("native runtime path is required"))?)?, policy)?;
    #[cfg(target_arch = "wasm32")]
    let provider = {
        let _ = runtime;
        OnnxRuntimeProvider::Web(jet_rt::model::provider::WebOnnxProvider::browser(policy)?)
    };
    let mut session = package.open_with(ModelSource::Bytes(&artifacts), &provider, &cancellation).await?;
    let (outputs, resumed) = if let Some(documents) = documents {
        let first = session.embed_documents(&documents, &cancellation).await;
        let (embedding, resumed) = match first {
            Err(ModelError::Cancelled { .. }) if resume_after_cancel => (
                session.embed_documents(&documents, &CancellationToken::new()).await?,
                true,
            ),
            other => (other?, false),
        };
        let mut index = EmbeddingIndex::new(&package)?;
        index.insert(&embedding)?;
        let _ = index.nearest(&embedding, 1)?;
        (vec![embedding_tensor(&embedding)?], resumed)
    } else {
        let first = session.run(&inputs, &cancellation).await;
        match first {
            Err(ModelError::Cancelled { .. }) if resume_after_cancel => (
                session.run(&inputs, &CancellationToken::new()).await?,
                true,
            ),
            other => (other?, false),
        }
    };
    let mut out = Writer::default(); out.u32(0);
    out.text(&package.identity.digest()); out.text(&session.provenance().render());
    out.u32(u32::from(resumed)); out.u32(outputs.len() as u32);
    for tensor in outputs {
        out.text(&tensor.spec.name); out.text(&tensor.spec.dtype.to_string());
        out.u32(tensor.spec.shape.dimensions.len() as u32);
        for dim in tensor.spec.shape.dimensions {
            let jet_rt::model::TensorDimension::Static(dim) = dim else { unreachable!("validated output") };
            out.u64(dim);
        }
        out.blob(&tensor.bytes);
    }
    if warm_runs > 0 {
        let measured = provider.measure(&package, ModelSource::Bytes(&artifacts), &inputs, warm_runs, &mut UnavailablePeakMemory).await?;
        out.text(&format!("{measured:?}"));
    } else { out.text(""); }
    Ok(out.0)
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    #[link(wasm_import_module = "jet_onnx_proof")]
    extern "C" { fn schedule(); fn finished(); }
    type Task = Pin<Box<dyn Future<Output = Result<Vec<u8>, ModelError>>>>;
    thread_local! {
        static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static TASK: RefCell<Option<Task>> = const { RefCell::new(None) };
        static CANCEL: RefCell<CancellationToken> = RefCell::new(CancellationToken::new());
    }
    struct Schedule;
    impl Wake for Schedule {
        fn wake(self: Arc<Self>) { unsafe { schedule(); } }
        fn wake_by_ref(self: &Arc<Self>) { unsafe { schedule(); } }
    }
    #[no_mangle]
    pub extern "C" fn proof_alloc(length: u32) -> u32 {
        INPUT.with(|input| {
            let mut input = input.borrow_mut();
            if !input.is_empty() || input.try_reserve_exact(length as usize).is_err() { return 0; }
            input.resize(length as usize, 0); input.as_mut_ptr() as u32
        })
    }
    #[no_mangle]
    pub extern "C" fn proof_start() {
        let input = INPUT.with(|input| std::mem::take(&mut *input.borrow_mut()));
        let token = CancellationToken::new(); CANCEL.with(|c| *c.borrow_mut() = token.clone());
        TASK.with(|task| { assert!(task.borrow().is_none(), "one proof request at a time"); *task.borrow_mut() = Some(Box::pin(execute(input, None, token))); });
        proof_poll();
    }
    #[no_mangle]
    pub extern "C" fn proof_poll() {
        let Some(mut task) = TASK.with(|task| task.borrow_mut().take()) else { return; };
        let waker = Waker::from(Arc::new(Schedule));
        match task.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Pending => TASK.with(|slot| *slot.borrow_mut() = Some(task)),
            Poll::Ready(result) => {
                OUTPUT.with(|out| *out.borrow_mut() = result.unwrap_or_else(failure));
                unsafe { finished(); }
            }
        }
    }
    #[no_mangle]
    pub extern "C" fn proof_cancel() { CANCEL.with(|c| c.borrow().cancel()); }
    #[no_mangle]
    pub extern "C" fn proof_drop() { TASK.with(|task| task.borrow_mut().take()); }
    #[no_mangle]
    pub extern "C" fn proof_result_ptr() -> u32 { OUTPUT.with(|out| out.borrow().as_ptr() as u32) }
    #[no_mangle]
    pub extern "C" fn proof_result_len() -> u32 { OUTPUT.with(|out| out.borrow().len() as u32) }
}

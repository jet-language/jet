// D-DX-QUEUE1=A: native bindings are thin adapters over the canonical queue
// state machine. They preserve the four-slot route ABI used by JIT while the
// shared ServiceAuthority fragment remains independent of native codec traits.
/// Native service start keeps the generated callable name while supplying the
/// checked JobSpec queue adapter to the shared service lifecycle.
fn jet_services_start(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    jet_services_start_with_worker_dispatcher(tree, |_name, handler, endpoint| {
        let scope = jet_services_execution_scope(endpoint)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler();
            jet_job_service_queue_tick_registered(endpoint, 16).map(|_| ())
        }));
        drop(scope);
        match result {
            Ok(result) => result,
            Err(_) => Err(JetServiceError::Unknown(
                "service worker callback panicked".to_string(),
            )),
        }
    })
}

/// The shared CBOR codec normalizes kernel failures into `EncodingError`.
/// Keep that conversion at the queue boundary so a payload encoding failure
/// becomes the queue's typed service error instead of a panic or an unwrap.
fn jet_job_queue_cbor_error(error: jet_std::EncodingError) -> JetServiceError {
    let location = if error.path.is_empty() {
        format!("byte {}", error.byte_offset)
    } else {
        format!("{} at byte {}", error.path, error.byte_offset)
    };
    JetServiceError::Policy(format!(
        "job payload cannot be encoded as canonical CBOR ({:?}, {location}): {}",
        error.kind, error.reason
    ))
}

fn jet_job_queue_encode_payload<T: __jet_Encode>(
    job: &str,
    payload: &T,
) -> Result<JetJobPayload, JetServiceError> {
    let bytes =
        jet_enc_cbor_to_bytes_canonical(payload).map_err(jet_job_queue_cbor_error)?;
    // Source-level enqueue does not publish payload bytes. Publication remains
    // an explicit queue-policy decision and is still bounded by the queue.
    JetJobPayload::new(job, bytes, false)
}

pub fn jet_job_queue_enqueue<T: __jet_Encode>(
    queue: &mut JetJobQueue<'static>,
    job: &str,
    payload: &T,
    key: Option<String>,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    let payload = jet_job_queue_encode_payload(job, payload)?;
    let request = match key {
        Some(key) => JetJobEnqueue::new(job, payload).with_key(key),
        None => JetJobEnqueue::new(job, payload),
    };
    queue.enqueue(request)
}

pub fn jet_job_queue_delay<T: __jet_Encode>(
    queue: &mut JetJobQueue<'static>,
    job: String,
    payload: T,
    duration: std::time::Duration,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    let payload = jet_job_queue_encode_payload(&job, &payload)?;
    let milliseconds = i64::try_from(duration.as_millis())
        .map_err(|_| service_authority_error("queue delay duration is outside the supported range"))?;
    queue.enqueue_delayed(job, payload, milliseconds, None, None)
}

/// Run one tick from a generated static checked JobSpec table.
pub fn jet_job_service_queue_tick_specs(
    endpoint: &JetServiceEndpoint,
    jobs: &[JetJobSpec],
    limit: usize,
) -> Result<usize, JetServiceError> {
    if limit == 0
        || !jobs
            .iter()
            .any(|job| job.payload_type.is_some() && job.queue_invoke.is_some())
    {
        return Ok(0);
    }
    jet_job_service_queue_tick_dispatch(endpoint, limit, |job_type, payload| {
        let Some(job) = jobs.iter().find(|job| {
            job.entry.name == job_type
                && job.payload_type == Some(payload.type_id.as_str())
        }) else {
            return Err(JetJobError {
                type_id: payload.type_id.clone(),
                reason: "unknown_job_payload".to_string(),
                detail: Some(format!(
                    "no checked #Job `{job_type}` accepts payload schema `{}`",
                    payload.type_id
                )),
            });
        };
        let Some(invoke) = job.queue_invoke else {
            return Err(JetJobError {
                type_id: payload.type_id.clone(),
                reason: "missing_invocation_adapter".to_string(),
                detail: Some(format!(
                    "checked #Job `{}` has no queue adapter",
                    job.entry.name
                )),
            });
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(payload)))
            .map_err(|_| JetJobError {
                type_id: payload.type_id.clone(),
                reason: "panic".to_string(),
                detail: Some(format!("checked #Job `{}` panicked", job.entry.name)),
            })
            .and_then(|result| result)
    })
}

pub fn jet_job_service_queue_tick_registered(
    endpoint: &JetServiceEndpoint,
    limit: usize,
) -> Result<usize, JetServiceError> {
    let Some(jobs) = jet_job_active_specs() else {
        return Err(JetServiceError::Unavailable(
            "checked job registry is not active for service queue dispatch".to_string(),
        ));
    };
    jet_job_service_queue_tick_specs(endpoint, jobs, limit)
}

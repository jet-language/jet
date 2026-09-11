// D-FOUND-REALTIME1=A: one fixed-rate callback clock for every native tier.
//
// The callback owns one reusable sample buffer.  Deadlines are absolute points
// on the monotonic Instant clock, not `now + period` sleeps, so fractional
// nanosecond periods do not accumulate drift.  The worker records bounded
// scalar facts only; it never allocates or writes output on the callback path.

const JET_REALTIME_NANOS_PER_SECOND: u128 = 1_000_000_000;

/// A bounded single-producer/single-consumer sample carrier for adapters whose
/// callback boundary must not allocate. Writers publish fixed f64 bit cells;
/// the owner drains a slot into an ordinary Vec after dequeue.
const JET_REALTIME_SAMPLE_RING_SLOTS: usize = 2;

#[derive(Debug)]
struct JetRealtimeSampleSlot {
    values: Box<[std::sync::atomic::AtomicU64]>,
}

#[derive(Debug)]
pub struct JetRealtimeSampleRing {
    slots: Box<[JetRealtimeSampleSlot]>,
    read_sequence: std::sync::atomic::AtomicU64,
    write_sequence: std::sync::atomic::AtomicU64,
}

impl JetRealtimeSampleRing {
    pub fn new(frame_count: usize) -> Self {
        assert!(frame_count > 0, "real-time sample ring needs a positive frame count");
        let slots = (0..JET_REALTIME_SAMPLE_RING_SLOTS)
            .map(|_| JetRealtimeSampleSlot {
                values: (0..frame_count)
                    .map(|_| std::sync::atomic::AtomicU64::new(0))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            read_sequence: std::sync::atomic::AtomicU64::new(0),
            write_sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Publish a fixed zero-filled sample slot without allocation or blocking.
    pub fn publish_zeroed(&self) -> bool {
        let sequence = self.write_sequence.load(std::sync::atomic::Ordering::Relaxed);
        let read = self.read_sequence.load(std::sync::atomic::Ordering::Acquire);
        if sequence.wrapping_sub(read) >= self.slots.len() as u64 {
            return false;
        }
        let slot = &self.slots[(sequence % self.slots.len() as u64) as usize];
        for value in &slot.values {
            value.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        self.write_sequence
            .store(sequence.wrapping_add(1), std::sync::atomic::Ordering::Release);
        true
    }

    /// Publish samples into a preallocated slot without allocation or blocking.
    pub fn publish(&self, samples: &[f64]) -> bool {
        let sequence = self.write_sequence.load(std::sync::atomic::Ordering::Relaxed);
        let read = self.read_sequence.load(std::sync::atomic::Ordering::Acquire);
        if sequence.wrapping_sub(read) >= self.slots.len() as u64 {
            return false;
        }
        let slot = &self.slots[(sequence % self.slots.len() as u64) as usize];
        if slot.values.len() != samples.len() {
            return false;
        }
        for (cell, &sample) in slot.values.iter().zip(samples) {
            cell.store(sample.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
        self.write_sequence
            .store(sequence.wrapping_add(1), std::sync::atomic::Ordering::Release);
        true
    }

    /// Drain one published slot. Allocation is deliberately owner-side.
    pub fn pop(&self) -> Option<Vec<f64>> {
        let sequence = self.read_sequence.load(std::sync::atomic::Ordering::Relaxed);
        let write = self.write_sequence.load(std::sync::atomic::Ordering::Acquire);
        if sequence == write {
            return None;
        }
        let slot = &self.slots[(sequence % self.slots.len() as u64) as usize];
        let values = slot
            .values
            .iter()
            .map(|value| f64::from_bits(value.load(std::sync::atomic::Ordering::Relaxed)))
            .collect();
        self.read_sequence
            .store(sequence.wrapping_add(1), std::sync::atomic::Ordering::Release);
        Some(values)
    }
}

/// The bounded facts emitted by one real-time callback stream.
///
/// `start_identity` and `end_identity` are monotonic nanoseconds from the
/// Prelude clock.  `end_identity == 0` means that a live stream has not yet
/// reached its cancellation boundary.  No callback history is retained: each
/// fact is a saturating scalar, so receipt size is independent of run length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetRealtimeReceipt {
    pub requested_rate_hz: i64,
    pub requested_frames: i64,
    pub completed_callbacks: i64,
    pub completed_frames: i64,
    pub missed: i64,
    pub max_lateness_ns: i64,
    pub start_identity: i64,
    pub end_identity: i64,
}

impl JetRealtimeReceipt {
    pub fn render_json(&self) -> String {
        format!(
            "{{\"requested_rate_hz\":{},\"requested_frames\":{},\"completed_callbacks\":{},\"completed_frames\":{},\"missed\":{},\"max_lateness_ns\":{},\"start_identity\":{},\"end_identity\":{}}}",
            self.requested_rate_hz,
            self.requested_frames,
            self.completed_callbacks,
            self.completed_frames,
            self.missed,
            self.max_lateness_ns,
            self.start_identity,
            self.end_identity,
        )
    }

    pub fn render(&self) -> String {
        format!(
            "RealtimeReceipt(rate_hz={}, frames={}, callbacks={}, completed_frames={}, missed={}, max_lateness_ns={}, start_identity={}, end_identity={})",
            self.requested_rate_hz,
            self.requested_frames,
            self.completed_callbacks,
            self.completed_frames,
            self.missed,
            self.max_lateness_ns,
            self.start_identity,
            self.end_identity,
        )
    }

    pub fn frames_per_buffer(&self) -> i64 {
        self.requested_frames
    }
}

struct JetRealtimeShared {
    requested_rate_hz: i64,
    requested_frames: i64,
    start: JetInstant,
    start_identity: i64,
    next_callback: std::sync::atomic::AtomicU64,
    next_deadline_identity: std::sync::atomic::AtomicI64,
    completed_callbacks: std::sync::atomic::AtomicI64,
    completed_frames: std::sync::atomic::AtomicI64,
    missed: std::sync::atomic::AtomicI64,
    max_lateness_ns: std::sync::atomic::AtomicI64,
    end_identity: std::sync::atomic::AtomicI64,
    cancelled: std::sync::atomic::AtomicBool,
    callback_skipped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// A running fixed-rate callback.  Dropping or cancelling it stops the worker
/// and joins it before the stream becomes unreachable, so callback state and
/// the reusable buffer cannot outlive the returned value.
#[derive(Clone)]
pub struct JetRealtimeStream {
    shared: std::sync::Arc<JetRealtimeShared>,
    worker: std::sync::Arc<std::sync::Mutex<Option<std::thread::JoinHandle<()>>>>,
}

/// Return the absolute nanosecond offset of callback `index` from stream start.
///
/// The rational period is retained until the final division.  For example,
/// 256 frames at 48 kHz alternate 5,333,333 and 5,333,334 ns intervals rather
/// than truncating every interval and drifting over the run.
pub fn jet_rt_deadline_offset_ns(rate_hz: i64, frames_per_buffer: i64, index: u128) -> i64 {
    if rate_hz <= 0 || frames_per_buffer <= 0 {
        return 0;
    }
    let frame_numerator = (frames_per_buffer as u128).saturating_mul(JET_REALTIME_NANOS_PER_SECOND);
    let numerator = frame_numerator.saturating_mul(index);
    numerator
        .checked_div(rate_hz as u128)
        .unwrap_or(u128::MAX)
        .min(i64::MAX as u128) as i64
}

fn jet_rt_deadline(
    start: JetInstant,
    start_identity: i64,
    rate_hz: i64,
    frames_per_buffer: i64,
    index: u128,
) -> (JetInstant, i64) {
    let offset = jet_rt_deadline_offset_ns(rate_hz, frames_per_buffer, index);
    (
        start.plus_duration_ns(offset),
        start_identity.saturating_add(offset),
    )
}

fn jet_rt_atomic_saturating_add(
    value: &std::sync::atomic::AtomicI64,
    increment: i64,
) {
    let _ = value.fetch_update(
        std::sync::atomic::Ordering::AcqRel,
        std::sync::atomic::Ordering::Acquire,
        |current| Some(current.saturating_add(increment)),
    );
}

fn jet_rt_atomic_max(value: &std::sync::atomic::AtomicI64, candidate: i64) {
    let mut current = value.load(std::sync::atomic::Ordering::Acquire);
    while current < candidate {
        match value.compare_exchange_weak(
            current,
            candidate,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

fn jet_rt_run<F>(
    shared: std::sync::Arc<JetRealtimeShared>,
    mut callback: F,
    buffer: Vec<f64>,
) where
    F: FnMut(&Vec<f64>) + Send + 'static,
{
    // The schedule keeps the value form for the native Instant API while the
    // atomics publish only bounded scalar identities to readers.
    let (first_deadline, _) = jet_rt_deadline(
        shared.start,
        shared.start_identity,
        shared.requested_rate_hz,
        shared.requested_frames,
        1,
    );
    let mut schedule = JetRealtimeSchedule {
        start: shared.start,
        next_deadline: first_deadline,
        end: None,
    };
    let mut callback_index = 1u128;

    while !shared
        .cancelled
        .load(std::sync::atomic::Ordering::Acquire)
    {
        let deadline_identity = shared
            .next_deadline_identity
            .load(std::sync::atomic::Ordering::Acquire);
        jet_time_sleep_until(&schedule.next_deadline);
        if shared
            .cancelled
            .load(std::sync::atomic::Ordering::Acquire)
        {
            break;
        }

        let started_identity = jet_time_monotonic_now_ns();
        let lateness = started_identity
            .saturating_sub(deadline_identity)
            .max(0);
        // `missed` counts deadline misses and callbacks skipped by a
        // nonblocking adapter boundary.
        if lateness > 0 {
            jet_rt_atomic_saturating_add(&shared.missed, 1);
            jet_rt_atomic_max(&shared.max_lateness_ns, lateness);
        }

        // The buffer is allocated before the worker starts and is reused for
        // every invocation.  No receipt collection, channel send, or output
        // operation is performed here.
        let callback_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callback(&buffer);
        }))
            .is_ok();
        if !callback_ok {
            shared
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            break;
        }

        let callback_skipped = shared
            .callback_skipped
            .swap(false, std::sync::atomic::Ordering::AcqRel);
        if callback_skipped {
            if lateness == 0 {
                jet_rt_atomic_saturating_add(&shared.missed, 1);
            }
        } else {
            jet_rt_atomic_saturating_add(&shared.completed_callbacks, 1);
            jet_rt_atomic_saturating_add(&shared.completed_frames, shared.requested_frames);
        }

        callback_index = callback_index.saturating_add(1);
        let next_index = callback_index.min(u64::MAX as u128) as u64;
        shared
            .next_callback
            .store(next_index, std::sync::atomic::Ordering::Release);
        let (next_deadline, next_identity) = jet_rt_deadline(
            schedule.start,
            shared.start_identity,
            shared.requested_rate_hz,
            shared.requested_frames,
            callback_index,
        );
        schedule.next_deadline = next_deadline;
        shared
            .next_deadline_identity
            .store(next_identity, std::sync::atomic::Ordering::Release);
    }

    schedule.end = Some(JetInstant::now());
    if schedule.end.is_some() {
        let end_identity = jet_time_monotonic_now_ns();
        shared
            .end_identity
            .store(end_identity, std::sync::atomic::Ordering::Release);
    }
}

/// The worker's value-form clock.  `JetInstant` remains the scheduling source;
/// atomics above are only the cross-thread observation rail.
struct JetRealtimeSchedule {
    start: JetInstant,
    next_deadline: JetInstant,
    end: Option<JetInstant>,
}

/// Start a callback that receives one reusable `frames_per_buffer` sample
/// buffer on every absolute deadline.  The worker continues until `cancel` or
/// drop; the callback itself owns no timer, counter, or output allocation.
pub fn jet_rt_callback<F>(
    rate_hz: i64,
    frames_per_buffer: i64,
    callback: F,
) -> JetRealtimeStream
where
    F: FnMut(&Vec<f64>) + Send + 'static,
{
    jet_rt_callback_with_skip(
        rate_hz,
        frames_per_buffer,
        callback,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}

/// Start a callback with a caller-owned signal for nonblocking adapter skips.
pub fn jet_rt_callback_with_skip<F>(
    rate_hz: i64,
    frames_per_buffer: i64,
    callback: F,
    callback_skipped: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> JetRealtimeStream
where
    F: FnMut(&Vec<f64>) + Send + 'static,
{
    assert!(rate_hz > 0, "real-time callback rate must be positive");
    jet_scheduler_world_reject_uncontrolled("foreign");
    assert!(frames_per_buffer > 0, "real-time callback frame count must be positive");
    let frame_count = usize::try_from(frames_per_buffer)
        .expect("real-time callback frame count is outside the platform range");
    let buffer = vec![0.0_f64; frame_count];
    let start = JetInstant::now();
    let start_identity = jet_time_monotonic_now_ns();
    let (_, first_deadline_identity) = jet_rt_deadline(
        start,
        start_identity,
        rate_hz,
        frames_per_buffer,
        1,
    );
    callback_skipped.store(false, std::sync::atomic::Ordering::Release);
    let shared = std::sync::Arc::new(JetRealtimeShared {
        requested_rate_hz: rate_hz,
        requested_frames: frames_per_buffer,
        start,
        start_identity,
        next_callback: std::sync::atomic::AtomicU64::new(1),
        next_deadline_identity: std::sync::atomic::AtomicI64::new(first_deadline_identity),
        completed_callbacks: std::sync::atomic::AtomicI64::new(0),
        completed_frames: std::sync::atomic::AtomicI64::new(0),
        missed: std::sync::atomic::AtomicI64::new(0),
        max_lateness_ns: std::sync::atomic::AtomicI64::new(0),
        end_identity: std::sync::atomic::AtomicI64::new(0),
        cancelled: std::sync::atomic::AtomicBool::new(false),
        callback_skipped,
    });
    let worker_shared = shared.clone();
    let worker = std::thread::spawn(move || jet_rt_run(worker_shared, callback, buffer));
    JetRealtimeStream {
        shared,
        worker: std::sync::Arc::new(std::sync::Mutex::new(Some(worker))),
    }
}

impl JetRealtimeStream {
    /// Return the next absolute monotonic callback deadline.
    pub(crate) fn next_deadline(&self) -> JetInstant {
        let index = self
            .shared
            .next_callback
            .load(std::sync::atomic::Ordering::Acquire) as u128;
        jet_rt_deadline(
            self.shared.start,
            self.shared.start_identity,
            self.shared.requested_rate_hz,
            self.shared.requested_frames,
            index,
        )
        .0
    }

    /// Snapshot bounded callback facts without waiting for the worker.
    pub fn receipt(&self) -> JetRealtimeReceipt {
        JetRealtimeReceipt {
            requested_rate_hz: self.shared.requested_rate_hz,
            requested_frames: self.shared.requested_frames,
            completed_callbacks: self
                .shared
                .completed_callbacks
                .load(std::sync::atomic::Ordering::Acquire),
            completed_frames: self
                .shared
                .completed_frames
                .load(std::sync::atomic::Ordering::Acquire),
            missed: self
                .shared
                .missed
                .load(std::sync::atomic::Ordering::Acquire),
            max_lateness_ns: self
                .shared
                .max_lateness_ns
                .load(std::sync::atomic::Ordering::Acquire),
            start_identity: self.shared.start_identity,
            end_identity: self
                .shared
                .end_identity
                .load(std::sync::atomic::Ordering::Acquire),
        }
    }

    /// Request cancellation and wait for the callback boundary to finish.
    pub fn cancel(&mut self) {
        self.shared
            .cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        self.join_worker();
    }

    pub fn is_cancelled(&self) -> bool {
        self.shared
            .cancelled
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn join_worker(&self) {
        let mut worker_slot = match self.worker.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(worker) = worker_slot.take() {
            let _ = worker.join();
        }
    }
}

/// Borrowing adapters keep observation methods from consuming the running stream.
pub fn jet_rt_next_deadline(stream: &JetRealtimeStream) -> JetInstant {
    stream.next_deadline()
}

pub fn jet_rt_receipt(stream: &JetRealtimeStream) -> JetRealtimeReceipt {
    stream.receipt()
}

pub fn jet_rt_cancel(mut stream: JetRealtimeStream) {
    stream.cancel();
}

pub fn jet_rt_is_cancelled(stream: &JetRealtimeStream) -> bool {
    stream.is_cancelled()
}

impl Drop for JetRealtimeStream {
    fn drop(&mut self) {
        if std::sync::Arc::strong_count(&self.worker) == 1 {
            self.shared
                .cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            self.join_worker();
        }
    }
}

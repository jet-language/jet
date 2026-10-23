// D-FOUND-REALTIME1=A: Web keeps one fixed-rate callback clock and the
// same absolute-deadline receipt facts as the native Prelude kernel.
const JET_REALTIME_NANOS_PER_SECOND = 1000000000n;

function jet_rt_deadline_offset_ns(rate, frames, index) {
  const rateValue = BigInt(rate);
  const frameValue = BigInt(frames);
  if (rateValue <= 0n || frameValue <= 0n) return 0n;
  return frameValue * JET_REALTIME_NANOS_PER_SECOND * BigInt(index) / rateValue;
}

function jet_rt_now_ns() {
  return typeof jet_time_monotonic_now_ns === "function"
    ? BigInt(jet_time_monotonic_now_ns())
    : 0n;
}

function jet_std_time_sleep_duration_ns(nanos) {
  const value = BigInt(nanos);
  if (value <= 0n) return undefined;
  const world = globalThis.__jet_deterministic_world;
  if (world) {
    return new Promise((resolve) => world.schedule_timer(value, resolve));
  }
  const milliseconds = Number(value) / 1000000;
  return new Promise((resolve) => setTimeout(resolve, Math.min(2147483647, milliseconds)));
}
// The checked Core route names the nominal Duration adapter. Web values carry
// the same nanosecond BigInt directly, so only the AOT side needs extraction.
function jet_std_time_sleep_duration(duration) {
  return jet_std_time_sleep_duration_ns(duration);
}

function jet_time_sleep_until(deadline) {
  return jet_std_time_sleep_duration_ns(BigInt(deadline) - jet_rt_now_ns());
}

class JetWebRealtimeStream {
  constructor(rate, frames, callback) {
    this.requested_rate_hz = BigInt(rate);
    this.requested_frames = BigInt(frames);
    if (this.requested_rate_hz <= 0n || this.requested_frames <= 0n) {
      throw new Error("real-time callback rate and frames must be positive");
    }
    const frameCount = Number(this.requested_frames);
    if (!Number.isSafeInteger(frameCount) || frameCount <= 0) {
      throw new Error("real-time callback frame count is outside the platform range");
    }
    if (globalThis.__jet_deterministic_world) {
      throw new Error("E3404: deterministic world has no controlled realtime provider");
    }
    this.callback = callback;
    this.run_once_callback = () => this.run_once();
    this.buffer = new Array(frameCount).fill(0);
    this.start_identity = jet_rt_now_ns();
    this.next_callback = 1n;
    this.next_deadline = this.start_identity
      + jet_rt_deadline_offset_ns(this.requested_rate_hz, this.requested_frames, 1n);
    this.completed_callbacks = 0n;
    this.completed_frames = 0n;
    this.missed = 0n;
    this.max_lateness_ns = 0n;
    this.end_identity = 0n;
    this.cancelled = false;
    this.timer = undefined;
    this.schedule();
  }

  schedule() {
    if (this.cancelled) return;
    const remaining = this.next_deadline - jet_rt_now_ns();
    const milliseconds = Number(remaining > 0n ? remaining : 0n) / 1000000;
    this.timer = setTimeout(this.run_once_callback, Math.min(2147483647, milliseconds));
  }

  run_once() {
    this.timer = undefined;
    if (this.cancelled) return;
    const started = jet_rt_now_ns();
    const lateness = started > this.next_deadline ? started - this.next_deadline : 0n;
    if (lateness > 0n) {
      this.missed += 1n;
      if (lateness > this.max_lateness_ns) this.max_lateness_ns = lateness;
    }
    try {
      this.callback(this.buffer);
      this.completed_callbacks += 1n;
      this.completed_frames += this.requested_frames;
    } catch (error) {
      this.cancelled = true;
      this.end_identity = jet_rt_now_ns();
      throw error;
    }
    this.next_callback += 1n;
    this.next_deadline = this.start_identity
      + jet_rt_deadline_offset_ns(
        this.requested_rate_hz,
        this.requested_frames,
        this.next_callback,
      );
    this.schedule();
  }

  receipt() {
    return {
      requested_rate_hz: this.requested_rate_hz,
      requested_frames: this.requested_frames,
      completed_callbacks: this.completed_callbacks,
      completed_frames: this.completed_frames,
      missed: this.missed,
      max_lateness_ns: this.max_lateness_ns,
      start_identity: this.start_identity,
      end_identity: this.end_identity,
    };
  }

  cancel() {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    this.cancelled = true;
    if (this.end_identity === 0n) this.end_identity = jet_rt_now_ns();
  }

  is_cancelled() {
    return this.cancelled;
  }
}

function jet_rt_callback(rate, frames, callback) {
  return new JetWebRealtimeStream(rate, frames, callback);
}

function jet_rt_next_deadline(stream) {
  return stream.next_deadline;
}

function jet_rt_receipt(stream) {
  return stream.receipt();
}

function jet_rt_cancel(stream) {
  stream.cancel();
}

function jet_rt_is_cancelled(stream) {
  return stream.is_cancelled();
}

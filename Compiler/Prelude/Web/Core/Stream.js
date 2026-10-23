// D-STREAM-EVENTTIME1: Web adapter for the shared event-time Stream kernel.
// The source generator remains the only transport; these functions only
// marshal values into the same event/key/window state machine as native tiers.

function jet_stream_event_time_ns(value) {
  if (typeof value === "bigint" || typeof value === "number") {
    return BigInt(value) * JET_TIME_NS_SECOND;
  }
  return jet_time_date_time_total_ns(value);
}

function jet_stream_duration_ns(value) {
  let current = value;
  if (current != null && current.tag === "Ok") current = current.values[0];
  return BigInt(current);
}

function jet_stream_window_floor_start_ns(value, width) {
  const quotient = BigInt(value) / BigInt(width);
  const remainder = BigInt(value) % BigInt(width);
  return (remainder < 0n ? quotient - 1n : quotient) * BigInt(width);
}

function jet_stream_with_event_time(stream, eventTime) {
  return (function* () {
    for (const value of stream) {
      const seconds = BigInt(eventTime(value));
      yield {
        value,
        event_time: jet_time_date_time_from_seconds(seconds),
      };
    }
  })();
}

function jet_stream_with_event_time_ns(stream, eventTime) {
  return (function* () {
    for (const value of stream) {
      yield {
        value,
        event_time: eventTime(value),
      };
    }
  })();
}

function jet_stream_key_by(stream, keyer) {
  return (function* () {
    for (const event of stream) {
      yield { key: keyer(event.value), event };
    }
  })();
}

function jet_keyed_stream_window(stream, windowDuration, watermarkDuration, late) {
  const windowNs = jet_stream_duration_ns(windowDuration);
  const allowedLatenessNs = jet_stream_duration_ns(watermarkDuration);
  if (windowNs <= 0n) throw new Error("stream window duration must be positive");
  if (allowedLatenessNs < 0n) throw new Error("stream allowed lateness must be nonnegative");
  const lateTag = late && typeof late === "object" ? late.tag : late;
  if (lateTag !== "Drop" && lateTag !== "SideOutput") {
    throw new Error("stream.window received an unknown late-event disposition");
  }
  const lateDisposition = lateTag === "Drop" ? "drop" : "side_output";

  return (function* () {
    const keys = [];
    const active = [];
    const sideOutput = [];
    let maxEventTime = null;
    let watermark = null;

    const keyIndex = (key) => {
      const found = keys.findIndex((known) => Object.is(known, key) || known === key);
      if (found >= 0) return found;
      keys.push(key);
      return keys.length - 1;
    };
    const takeReady = (ready) => {
      const completed = [];
      const retained = [];
      for (const pending of active) {
        if (ready(pending)) {
          const endNs = pending.startNs + windowNs;
          completed.push({
            key: keys[pending.keyIndex],
            start: jet_time_date_time_from_nanoseconds(pending.startNs),
            end: jet_time_date_time_from_nanoseconds(endNs),
            events: pending.events,
          });
        } else {
          retained.push(pending);
        }
      }
      active.length = 0;
      active.push(...retained);
      return completed;
    };
    const ingest = (key, event) => {
      const eventNs = jet_stream_event_time_ns(event.event_time);
      const wasLate = watermark !== null && eventNs < watermark;
      if (maxEventTime === null || eventNs > maxEventTime) {
        maxEventTime = eventNs;
        const candidate = eventNs - allowedLatenessNs;
        watermark = watermark === null || candidate > watermark ? candidate : watermark;
      }
      if (wasLate) {
        if (lateDisposition === "side_output") sideOutput.push(event);
        return;
      }
      const index = keyIndex(key);
      const startNs = jet_stream_window_floor_start_ns(eventNs, windowNs);
      const existing = active.find(
        (pending) => pending.keyIndex === index && pending.startNs === startNs,
      );
      if (existing) {
        existing.events.push(event);
        return;
      }
      const pending = { keyIndex: index, startNs, events: [event] };
      const position = active.findIndex(
        (candidate) => candidate.keyIndex > index
          || (candidate.keyIndex === index && candidate.startNs > startNs),
      );
      if (position < 0) active.push(pending);
      else active.splice(position, 0, pending);
    };

    for (const keyed of stream) {
      ingest(keyed.key, keyed.event);
      const ready = takeReady((pending) => pending.startNs + windowNs <= watermark);
      yield* ready;
    }
    yield* takeReady(() => true);
  })();
}

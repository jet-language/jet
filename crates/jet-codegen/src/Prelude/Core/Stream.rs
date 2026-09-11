// D-STREAM-EVENTTIME1: one event-time state machine for keyed tumbling windows.
//
// `JetStream` remains the only scheduler-backed stream.  The types below wrap
// that pull source; they do not create another producer or callback transport.
// Event-time arithmetic uses the canonical UTC `JetDateTime` carrier and exact
// nanosecond `i128` intermediates.  Each ready batch is ordered by first-seen
// key, then ascending window start; the lazy wrapper emits batches at each
// watermark advance and flushes the remaining batch at end of input.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetStreamEvent<T> {
    pub(crate) value: T,
    pub(crate) event_time: JetDateTime,
}

impl<T> JetStreamEvent<T> {
    pub fn new(value: T, event_time: JetDateTime) -> Self {
        Self { value, event_time }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn event_time(&self) -> &JetDateTime {
        &self.event_time
    }

    pub fn into_parts(self) -> (T, JetDateTime) {
        (self.value, self.event_time)
    }
}

pub(crate) fn jet_stream_event<T>(value: T, event_time: JetDateTime) -> JetStreamEvent<T> {
    JetStreamEvent::new(value, event_time)
}
/// Event-time callback results use the same source-facing timestamp shapes as
/// the rest of the time API.  `Int` is Unix seconds; `DateTime` remains exact.
pub(crate) trait JetStreamEventTime {
    fn into_event_time(self) -> JetDateTime;
}

impl JetStreamEventTime for i64 {
    fn into_event_time(self) -> JetDateTime {
        JetDateTime::from_timestamp(self)
    }
}

impl JetStreamEventTime for JetDateTime {
    fn into_event_time(self) -> JetDateTime {
        self
    }
}

/// Convert the source-facing duration carrier without copying or re-deciding
/// units in a tier adapter.
pub(crate) trait JetStreamDuration {
    fn stream_nanoseconds(self) -> i64;
}

impl JetStreamDuration for Duration {
    fn stream_nanoseconds(self) -> i64 {
        self.as_nanos() as i64
    }
}

impl JetStreamDuration for i64 {
    fn stream_nanoseconds(self) -> i64 {
        self
    }
}

pub(crate) fn jet_stream_with_event_time<T, F, R>(
    mut stream: JetStream<T>,
    mut event_time: F,
) -> JetStream<JetStreamEvent<T>>
where
    T: Send + 'static,
    F: FnMut(&T) -> R + Send + 'static,
    R: JetStreamEventTime + Send + 'static,
{
    jet_stream_task(move |sender| {
        while let Some(value) = stream.pull_checked() {
            let timestamp = event_time(&value).into_event_time();
            if !sender.send_stream(JetStreamEvent::new(value, timestamp)) {
                break;
            }
        }
    })
}

/// Resident JIT's scalar ABI carrier for an `Int` event-time callback.
pub fn jet_stream_with_event_time_i64<T, F>(
    stream: JetStream<T>,
    event_time: F,
) -> JetStream<JetStreamEvent<T>>
where
    T: Send + 'static,
    F: FnMut(&T) -> i64 + Send + 'static,
{
    jet_stream_with_event_time(stream, event_time)
}

/// Resident JIT's exact `DateTime` ABI carrier: a checked DateTime value is
/// already represented as Unix nanoseconds in the scalar callback word.
pub fn jet_stream_with_event_time_ns<T, F>(
    mut stream: JetStream<T>,
    mut event_time: F,
) -> JetStream<JetStreamEvent<T>>
where
    T: Send + 'static,
    F: FnMut(&T) -> i64 + Send + 'static,
{
    jet_stream_task(move |sender| {
        while let Some(value) = stream.pull_checked() {
            let timestamp = JetDateTime::from_unix_nanoseconds(event_time(&value));
            if !sender.send_stream(JetStreamEvent::new(value, timestamp)) {
                break;
            }
        }
    })
}


/// The explicit policy for an event whose timestamp is before the current
/// watermark.  A side-output event never enters a main window; it remains on
/// the windowed stream's state for inspection or later routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetLateEventDisposition {
    Drop,
    SideOutput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetStreamEventDisposition {
    Accepted,
    Dropped,
    SideOutput,
}

#[derive(Debug)]
struct JetPendingWindow<T> {
    key_index: usize,
    start_ns: i128,
    events: Vec<JetStreamEvent<T>>,
}

/// A completed tumbling window.  Events retain source order inside the window;
/// each emission batch is first-seen key, then ascending window start.
#[derive(Debug)]
pub struct JetStreamWindow<K, T> {
    pub(crate) key: K,
    pub(crate) start: JetDateTime,
    pub(crate) end: JetDateTime,
    pub(crate) events: Vec<JetStreamEvent<T>>,
}

impl<K, T> JetStreamWindow<K, T> {
    pub fn key(&self) -> &K {
        &self.key
    }

    pub fn start(&self) -> &JetDateTime {
        &self.start
    }

    pub fn end(&self) -> &JetDateTime {
        &self.end
    }

    pub fn events(&self) -> &[JetStreamEvent<T>] {
        &self.events
    }

    pub fn into_parts(self) -> (K, JetDateTime, JetDateTime, Vec<JetStreamEvent<T>>) {
        (self.key, self.start, self.end, self.events)
    }
}
pub struct JetEventTimeState<K, T> {
    window_ns: i64,
    allowed_lateness_ns: i64,
    late_disposition: JetLateEventDisposition,
    keys: Vec<K>,
    active: Vec<JetPendingWindow<T>>,
    side_output: Vec<JetStreamEvent<T>>,
    max_event_time: Option<JetDateTime>,
    watermark: Option<JetDateTime>,
}

impl<K, T> JetEventTimeState<K, T>
where
    K: Clone + PartialEq,
{
    pub fn new(
        window_ns: i64,
        allowed_lateness_ns: i64,
        late_disposition: JetLateEventDisposition,
    ) -> Result<Self, String> {
        if window_ns <= 0 {
            return Err("stream window duration must be positive".to_string());
        }
        if allowed_lateness_ns < 0 {
            return Err("stream allowed lateness must be nonnegative".to_string());
        }
        Ok(Self {
            window_ns,
            allowed_lateness_ns,
            late_disposition,
            keys: Vec::new(),
            active: Vec::new(),
            side_output: Vec::new(),
            max_event_time: None,
            watermark: None,
        })
    }

    /// Ingest one event and return the disposition observed by the main
    /// window.  An event exactly at the watermark is on time; only timestamps
    /// strictly before it are late.  That boundary keeps a watermark at 7
    /// eligible for the timestamp-7 event in the canonical fixture.
    pub fn ingest(
        &mut self,
        key: K,
        event: JetStreamEvent<T>,
    ) -> JetStreamEventDisposition {
        let event_ns = event.event_time.total_nanoseconds();
        let was_late = self
            .watermark
            .as_ref()
            .is_some_and(|watermark| event_ns < watermark.total_nanoseconds());

        if self
            .max_event_time
            .as_ref()
            .is_none_or(|max| event_ns > max.total_nanoseconds())
        {
            self.max_event_time = Some(event.event_time.clone());
            self.advance_watermark();
        }

        if was_late {
            return match self.late_disposition {
                JetLateEventDisposition::Drop => JetStreamEventDisposition::Dropped,
                JetLateEventDisposition::SideOutput => {
                    self.side_output.push(event);
                    JetStreamEventDisposition::SideOutput
                }
            };
        }
        let key_index = self.key_index(&key);

        let start_ns = jet_stream_window_start_ns(
            event.event_time.total_nanoseconds(),
            self.window_ns,
        );
        if let Some(window) = self
            .active
            .iter_mut()
            .find(|window| window.key_index == key_index && window.start_ns == start_ns)
        {
            window.events.push(event);
        } else {
            let position = self
                .active
                .iter()
                .position(|window| {
                    window.key_index > key_index
                        || (window.key_index == key_index && window.start_ns > start_ns)
                })
                .unwrap_or(self.active.len());
            self.active.insert(
                position,
                JetPendingWindow {
                    key_index,
                    start_ns,
                    events: vec![event],
                },
            );
        }
        JetStreamEventDisposition::Accepted
    }

    pub fn watermark(&self) -> Option<JetDateTime> {
        self.watermark.clone()
    }

    pub fn max_event_time(&self) -> Option<JetDateTime> {
        self.max_event_time.clone()
    }

    pub fn late_disposition(&self) -> JetLateEventDisposition {
        self.late_disposition
    }

    pub fn key_order(&self) -> &[K] {
        &self.keys
    }

    pub fn side_output(&self) -> &[JetStreamEvent<T>] {
        &self.side_output
    }

    pub fn take_side_output(&mut self) -> Vec<JetStreamEvent<T>> {
        std::mem::take(&mut self.side_output)
    }

    /// Remove windows whose end is at or before the monotonic watermark.  The
    /// caller owns the returned values; the state keeps only windows that may
    /// still receive an on-time event.
    pub fn take_ready_windows(&mut self) -> Vec<JetStreamWindow<K, T>> {
        let Some(watermark) = self.watermark.as_ref() else {
            return Vec::new();
        };
        let watermark_ns = watermark.total_nanoseconds();
        let window_ns = self.window_ns;
        self.take_windows_where(|window| {
            window_end_ns(window.start_ns, window_ns) <= watermark_ns
        })
    }

    /// End-of-stream closes every remaining active window without changing the
    /// public watermark.  The latter remains `max_event_time - lateness`, which
    /// is the stream's progress fact even when the source has terminated.
    pub fn finish(&mut self) -> Vec<JetStreamWindow<K, T>> {
        self.take_windows_where(|_| true)
    }

    fn key_index(&mut self, key: &K) -> usize {
        if let Some(index) = self.keys.iter().position(|known| known == key) {
            return index;
        }
        self.keys.push(key.clone());
        self.keys.len() - 1
    }

    fn advance_watermark(&mut self) {
        let Some(max_event_time) = self.max_event_time.as_ref() else {
            return;
        };
        let candidate = jet_stream_datetime_from_ns(
            max_event_time
                .total_nanoseconds()
                .saturating_sub(self.allowed_lateness_ns as i128),
        );
        if self.watermark.as_ref().is_none_or(|watermark| {
            candidate.total_nanoseconds() > watermark.total_nanoseconds()
        }) {
            self.watermark = Some(candidate);
        }
    }

    fn take_windows_where<F>(&mut self, mut ready: F) -> Vec<JetStreamWindow<K, T>>
    where
        F: FnMut(&JetPendingWindow<T>) -> bool,
    {
        let window_ns = self.window_ns;
        let keys = &self.keys;
        let mut completed = Vec::new();
        let mut retained = Vec::with_capacity(self.active.len());
        for window in self.active.drain(..) {
            if ready(&window) {
                let end_ns = window_end_ns(window.start_ns, window_ns);
                let key = keys[window.key_index].clone();
                completed.push(JetStreamWindow {
                    key,
                    start: jet_stream_datetime_from_ns(window.start_ns),
                    end: jet_stream_datetime_from_ns(end_ns),
                    events: window.events,
                });
            } else {
                retained.push(window);
            }
        }
        self.active = retained;
        completed
    }
}

impl<K, T> JetEventTimeState<K, T> {
    pub fn window_ns(&self) -> i64 {
        self.window_ns
    }

    pub fn allowed_lateness_ns(&self) -> i64 {
        self.allowed_lateness_ns
    }
}

/// A keyed view over the existing scheduler-backed stream.  No producer task
/// is created: pulling this view pulls and acknowledges the one source stream.
pub struct JetKeyedStream<K, T> {
    stream: JetStream<JetStreamEvent<T>>,
    keyer: Box<dyn FnMut(&T) -> K + Send>,
}

impl<K, T> JetKeyedStream<K, T>
where
    K: Send + 'static,
    T: Send + 'static,
{
    pub fn next_keyed(&mut self) -> Option<(K, JetStreamEvent<T>)> {
        let event = self.stream.pull_checked()?;
        let key = (self.keyer)(&event.value);
        Some((key, event))
    }

    pub fn window(
        self,
        window_ns: i64,
        allowed_lateness_ns: i64,
        late_disposition: JetLateEventDisposition,
    ) -> Result<JetWindowedStream<K, T>, String>
    where
        K: Clone + PartialEq,
    {
        JetEventTimeState::new(window_ns, allowed_lateness_ns, late_disposition).map(|state| {
            JetWindowedStream {
                source: Some(self),
                state,
                ready: std::collections::VecDeque::new(),
                finished: false,
            }
        })
    }
}

impl<K, T> Iterator for JetKeyedStream<K, T>
where
    K: Send + 'static,
    T: Send + 'static,
{
    type Item = (K, JetStreamEvent<T>);

    fn next(&mut self) -> Option<Self::Item> {
        self.next_keyed()
    }
}

/// A lazy event-time window view over one `JetKeyedStream`.
pub struct JetWindowedStream<K, T> {
    source: Option<JetKeyedStream<K, T>>,
    state: JetEventTimeState<K, T>,
    ready: std::collections::VecDeque<JetStreamWindow<K, T>>,
    finished: bool,
}

impl<K, T> JetWindowedStream<K, T>
where
    K: Clone + PartialEq + Send + 'static,
    T: Send + 'static,
{
    pub fn pull_window(&mut self) -> Option<JetStreamWindow<K, T>> {
        loop {
            if let Some(window) = self.ready.pop_front() {
                return Some(window);
            }
            if self.finished {
                return None;
            }
            let next = self.source.as_mut().and_then(JetKeyedStream::next_keyed);
            let Some((key, event)) = next else {
                self.source.take();
                self.ready.extend(self.state.finish());
                self.finished = true;
                continue;
            };
            self.state.ingest(key, event);
            self.ready.extend(self.state.take_ready_windows());
        }
    }

    pub fn watermark(&self) -> Option<JetDateTime> {
        self.state.watermark()
    }

    pub fn max_event_time(&self) -> Option<JetDateTime> {
        self.state.max_event_time()
    }

    pub fn late_disposition(&self) -> JetLateEventDisposition {
        self.state.late_disposition()
    }

    pub fn side_output(&self) -> &[JetStreamEvent<T>] {
        self.state.side_output()
    }

    pub fn take_side_output(&mut self) -> Vec<JetStreamEvent<T>> {
        self.state.take_side_output()
    }

    pub fn key_order(&self) -> &[K] {
        self.state.key_order()
    }
}

impl<K, T> Iterator for JetWindowedStream<K, T>
where
    K: Clone + PartialEq + Send + 'static,
    T: Send + 'static,
{
    type Item = JetStreamWindow<K, T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.pull_window()
    }
}

impl<T> JetStream<JetStreamEvent<T>>
where
    T: Send + 'static,
{
    pub fn key_by<K, F>(self, keyer: F) -> JetKeyedStream<K, T>
    where
        K: Send + 'static,
        F: FnMut(&T) -> K + Send + 'static,
    {
        JetKeyedStream {
            stream: self,
            keyer: Box::new(keyer),
        }
    }
}

pub fn jet_stream_key_by<T, K, F>(
    stream: JetStream<JetStreamEvent<T>>,
    keyer: F,
) -> JetKeyedStream<K, T>
where
    T: Send + 'static,
    K: Send + 'static,
    F: FnMut(&T) -> K + Send + 'static,
{
    stream.key_by(keyer)
}

pub(crate) fn jet_keyed_stream_window<K, T, D>(
    stream: JetKeyedStream<K, T>,
    window: D,
    watermark: D,
    late_disposition: JetLateEventDisposition,
) -> JetStream<JetStreamWindow<K, T>>
where
    K: Clone + PartialEq + Send + 'static,
    T: Send + 'static,
    D: JetStreamDuration + Send + 'static,
{
    let window_ns = window.stream_nanoseconds();
    let watermark_ns = watermark.stream_nanoseconds();
    jet_stream_task(move |sender| {
        let mut windows = match stream.window(window_ns, watermark_ns, late_disposition) {
            Ok(windows) => windows,
            Err(error) => {
                sender.fail_with(error);
                return;
            }
        };
        while let Some(window) = windows.pull_window() {
            if !sender.send_stream(window) {
                break;
            }
        }
    })
}

/// Resident JIT's scalar duration ABI carrier for a window operator.
pub fn jet_keyed_stream_window_i64<K, T>(
    stream: JetKeyedStream<K, T>,
    window: i64,
    watermark: i64,
    late_disposition: JetLateEventDisposition,
) -> JetStream<JetStreamWindow<K, T>>
where
    K: Clone + PartialEq + Send + 'static,
    T: Send + 'static,
{
    jet_keyed_stream_window(stream, window, watermark, late_disposition)
}

fn jet_stream_window_start_ns(event_ns: i128, window_ns: i64) -> i128 {
    event_ns.div_euclid(window_ns as i128) * window_ns as i128
}

fn window_end_ns(start_ns: i128, window_ns: i64) -> i128 {
    start_ns.saturating_add(window_ns as i128)
}

fn jet_stream_datetime_from_ns(total_ns: i128) -> JetDateTime {
    let secs = total_ns
        .div_euclid(1_000_000_000)
        .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    let nanos = total_ns.rem_euclid(1_000_000_000) as u32;
    JetDateTime::from_timestamp_ns(secs, nanos)
}

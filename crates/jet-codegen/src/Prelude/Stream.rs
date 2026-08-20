// D-CONC-STREAM1=A / D-CANCELMODEL1=C: one pull-gated Stream protocol for
// emitted programs and the resident JIT adapter. This file is included by
// both scheduler substrates so suspension, cancellation, cleanup, and
// completion cannot drift by tier.

pub fn jet_stream<T: Send>() -> (JetStreamSender<T>, JetStream<T>) {
    let values = JetSchedulerChannel::new();
    let acknowledgements = JetSchedulerChannel::new();
    let completion = JetSchedulerChannel::<JetStreamCompletion>::new();
    let failure_report = std::sync::Arc::new(std::sync::Mutex::new(None));
    let value_tx = values.sender();
    let acknowledgement_tx = acknowledgements.sender();
    let completion_tx = completion.sender();
    (
        JetStreamSender {
            values: value_tx,
            acknowledgements,
            completion: completion_tx,
            failed: std::sync::atomic::AtomicBool::new(false),
            failure_report: failure_report.clone(),
        },
        JetStream {
            values: Some(values),
            acknowledgements: Some(acknowledgement_tx),
            completion: Some(completion),
            pending: false,
            failed: false,
            failure_report,
            producer_task: None,
        },
    )
}

/// Start a generator as a child task of its stream. The stream owns the task
/// handle; dropping the consumer therefore uses the same cancellation and
/// wait-point unwind as every other task.
pub fn jet_stream_task<T, F>(producer: F) -> JetStream<T>
where
    T: Send + 'static,
    F: FnOnce(&JetStreamSender<T>) + Send + 'static,
{
    let (sender, mut stream) = jet_stream();
    let control = JetTaskControl::new();
    let join = jet_scheduler_spawn_with_control(
        move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                producer(&sender);
            }));
            if let Err(payload) = result {
                // A cancel is not a producer failure, so it never sets the
                // failure flag — but it still resumes, so the task reports
                // `Cancelled` and `sender`'s Drop still signals completion on
                // the way out. Dropping `sender` during that unwind is safe:
                // its completion send is a wait point reached from cleanup, and
                // the cleanup rule in `Prelude/Scheduler.rs` defers the cancel
                // there instead of raising a second time (#2007, #2019).
                if !jet_scheduler_is_cancel_unwind(payload.as_ref()) {
                    sender.fail();
                }
                std::panic::resume_unwind(payload);
            }
        },
        control.clone(),
    );
    stream.attach_task(control, join);
    stream
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetStreamCompletion {
    Completed,
    Failed(Option<String>),
}

pub struct JetStream<T> {
    values: Option<JetSchedulerChannel<T>>,
    acknowledgements: Option<JetSchedulerSender<()>>,
    completion: Option<JetSchedulerChannel<JetStreamCompletion>>,
    pending: bool,
    failed: bool,
    failure_report: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    producer_task: Option<JetStreamTask>,
}

struct JetStreamTask {
    control: std::sync::Arc<JetTaskControl>,
    drain: Option<Box<dyn FnOnce() + Send>>,
}

impl JetStreamTask {
    fn new<T: Send + 'static>(
        control: std::sync::Arc<JetTaskControl>,
        join: JetSchedulerJoin<T>,
    ) -> Self {
        Self {
            control,
            drain: Some(Box::new(move || join.drain())),
        }
    }

    fn cancel(&self) {
        self.control.cancel();
    }

    fn drain(mut self) {
        if let Some(drain) = self.drain.take() {
            drain();
        }
    }
}

impl<T: Send> JetStream<T> {
    /// Attach the scheduler child that produces this stream. Runtime adapters
    /// use this after their ABI handle has been created.
    pub fn attach_task<J: Send + 'static>(
        &mut self,
        control: std::sync::Arc<JetTaskControl>,
        join: JetSchedulerJoin<J>,
    ) {
        self.producer_task = Some(JetStreamTask::new(control, join));
    }

    /// Pull one value. The acknowledgement for the preceding value is sent
    /// first, which is the exact suspension boundary after `yield`.
    pub fn pull(&mut self) -> Option<T> {
        if self.pending {
            let acknowledgement = self.acknowledgements.as_ref()?;
            if !acknowledgement.send(()) {
                self.observe_completion();
                return None;
            }
            self.pending = false;
        }
        let Some(values) = self.values.as_ref() else {
            return None;
        };
        let Some(value) = values.receive() else {
            self.observe_completion();
            return None;
        };
        self.pending = true;
        Some(value)
    }

    fn observe_completion(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        match completion.receive() {
            Some(JetStreamCompletion::Failed(report)) => {
                self.failed = true;
                if let Some(report) = report {
                    *self
                        .failure_report
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report);
                }
            }
            _ => {}
        }
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn failure_report(&self) -> Option<String> {
        self.failure_report
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

pub struct JetStreamIter<T> {
    stream: JetStream<T>,
}

impl<T: Send> Iterator for JetStreamIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.stream.pull();
        if value.is_none() && self.stream.failed() {
            if let Some(report) = self.stream.failure_report() {
                jet_scheduler_runtime_stop_with_report(report);
            }
            jet_scheduler_runtime_stop("stream producer failed");
        }
        value
    }
}

impl<T: Send> IntoIterator for JetStream<T> {
    type Item = T;
    type IntoIter = JetStreamIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        JetStreamIter { stream: self }
    }
}

impl<T> Drop for JetStream<T> {
    fn drop(&mut self) {
        // D-CONC-STREAM1=A: dropping the iterator cancels its producer, which
        // is an ordinary scheduler child, and the shared task wait point
        // performs the unwind — so every producer cleanup runs before the
        // consumer endpoints disappear. There is no stream-local shutdown to
        // fall back to: a stream with no attached child has no producer to
        // stop, and its cancellation fact is that child's `JetTaskControl`.
        //
        // Cancel, then close, then join. The order is the whole law:
        //   * cancel first sets the fact before the producer can resume, so a
        //     producer parked at `yield` unwinds there and never runs the code
        //     after it;
        //   * closing the consumer endpoints releases a producer inside
        //     `#Shield`, which parks on the real event only and ignores the
        //     cancel wake (`Prelude/Scheduler.rs::jet_scheduler_yield`). It
        //     finishes its critical section and unwinds at the shield's exit,
        //     exactly as a shielded task cancelled at any other wait point
        //     does. Joining before the close would wait on a producer that is
        //     waiting on us;
        //   * the join then observes the child's outcome, and the completion
        //     receive picks up the notification its sender sent while
        //     unwinding.
        let producer = self.producer_task.take();
        if let Some(task) = &producer {
            task.cancel();
        }
        let _ = self.acknowledgements.take();
        let _ = self.values.take();
        if let Some(task) = producer {
            task.drain();
        }
        if let Some(completion) = self.completion.take() {
            let _ = completion.receive();
        }
    }
}

pub struct JetStreamSender<T> {
    values: JetSchedulerSender<T>,
    acknowledgements: JetSchedulerChannel<()>,
    completion: JetSchedulerSender<JetStreamCompletion>,
    failed: std::sync::atomic::AtomicBool,
    failure_report: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl<T: Send> JetStreamSender<T> {
    /// Send one value and wait for the next consumer pull. A dropped consumer
    /// closes the acknowledgement channel and returns `false`.
    pub fn send_stream(&self, value: T) -> bool {
        self.values.send(value) && self.acknowledgements.receive().is_some()
    }

    pub fn fail(&self) {
        self.failed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn fail_with(&self, report: String) {
        *self
            .failure_report
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(report);
        self.fail();
    }
}

impl<T> Drop for JetStreamSender<T> {
    fn drop(&mut self) {
        // Completion is signalled after the generator's lexical cleanup has
        // finished, so a dropped consumer can wait for the cleanup boundary.
        // The send below is a wait point running inside drop glue: when the
        // producer is cancelled, the cleanup rule in `Prelude/Scheduler.rs`
        // defers that cancel so this notification is delivered rather than
        // traded for a second unwind (#2007).
        let completion = if self
            .failed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            let mut failure_report = self
                .failure_report
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if failure_report.is_none() {
                *failure_report = jet_stream_take_failure_report();
            }
            let report = failure_report.clone();
            JetStreamCompletion::Failed(report)
        } else {
            JetStreamCompletion::Completed
        };
        let _ = self.completion.send(completion);
    }
}

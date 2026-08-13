// D-STREAMYIELD1: one pull-gated Stream protocol for emitted programs and the
// resident JIT adapter. This file is included by both scheduler substrates so
// suspension, cancellation, cleanup, and completion cannot drift by tier.

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
        },
    )
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
}

impl<T: Send> JetStream<T> {
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
        // Drop consumer handles first. This closes the acknowledgement channel
        // and wakes a producer blocked after its last accepted pull.
        let _ = self.acknowledgements.take();
        let _ = self.values.take();
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

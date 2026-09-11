// D-DX-JOBS-PANEL1 / card #2452: one typed, bounded job observation plane.
//
// This module records only lifecycle metadata.  Job arguments and arbitrary
// payloads have no representation here; a host receives these facts through
// the existing `jet.devtools.v1` envelope and cannot widen the record.
// Durable queue adapters publish the same `Job` metadata after commit; this
// module remains a projection and never becomes a second queue authority.
const JET_DEVTOOLS_JOB_PANEL_MAX_LABELS: usize = 32;
const JET_DEVTOOLS_JOB_PANEL_MAX_TEXT_BYTES: usize = 16 * 1024;
const JET_DEVTOOLS_JOB_PANEL_THROUGHPUT_WINDOW_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsJobPanelLifecycleState {
    Enqueued,
    Started,
    Retrying,
    Completed,
    Failed,
}

impl JetDevtoolsJobPanelLifecycleState {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
            Self::Started => "started",
            Self::Retrying => "retrying",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }

    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Enqueued | Self::Retrying)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsJobPanelEventKind {
    Enqueue,
    Start,
    Retry,
    Complete,
    Fail,
}

impl JetDevtoolsJobPanelEventKind {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Enqueue => "enqueue",
            Self::Start => "start",
            Self::Retry => "retry",
            Self::Complete => "complete",
            Self::Fail => "fail",
        }
    }
}

/// A closed failure vocabulary keeps failure facts useful without retaining a
/// runtime error string that could contain arguments or unpublished payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsJobPanelFailureKind {
    Error,
    Cancelled,
    WorkerLost,
    TimedOut,
}

impl JetDevtoolsJobPanelFailureKind {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::WorkerLost => "worker_lost",
            Self::TimedOut => "timed_out",
        }
    }
}

/// The only label value that enters a job-panel fact.  There is deliberately
/// no object, list, argument, or payload variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetDevtoolsJobPanelLabelValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

impl JetDevtoolsJobPanelLabelValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub const fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    pub const fn boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    fn render_json(&self, out: &mut String) {
        match self {
            Self::Text(value) => jet_devtools_job_panel_render_text(value, out),
            Self::Integer(value) => out.push_str(&value.to_string()),
            Self::Boolean(value) => out.push_str(if *value { "true" } else { "false" }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsJobPanelLabel {
    pub key: String,
    pub value: JetDevtoolsJobPanelLabelValue,
}

impl JetDevtoolsJobPanelLabel {
    pub fn new(key: impl Into<String>, value: JetDevtoolsJobPanelLabelValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, JetDevtoolsJobPanelLabelValue::text(value))
    }

    pub fn integer(key: impl Into<String>, value: i64) -> Self {
        Self::new(key, JetDevtoolsJobPanelLabelValue::integer(value))
    }

    pub fn boolean(key: impl Into<String>, value: bool) -> Self {
        Self::new(key, JetDevtoolsJobPanelLabelValue::boolean(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsJobPanelLabelFilter {
    pub key: String,
    pub value: JetDevtoolsJobPanelLabelValue,
}

impl JetDevtoolsJobPanelLabelFilter {
    pub fn new(key: impl Into<String>, value: JetDevtoolsJobPanelLabelValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    pub fn matches(&self, labels: &[JetDevtoolsJobPanelLabel]) -> bool {
        labels
            .iter()
            .any(|label| label.key == self.key && label.value == self.value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsJobPanelFact {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub kind: JetDevtoolsJobPanelEventKind,
    pub state: JetDevtoolsJobPanelLifecycleState,
    pub job_id: String,
    pub name: String,
    pub labels: Vec<JetDevtoolsJobPanelLabel>,
    pub queue: String,
    pub attempts: u32,
    pub duration_ms: Option<u64>,
    pub worker: Option<String>,
    pub request_id: Option<String>,
    pub failure: Option<JetDevtoolsJobPanelFailureKind>,
}

impl JetDevtoolsJobPanelFact {
    /// Marshal one typed lifecycle fact into the existing event boundary.
    /// `from_parts` is used only with fields rendered by this module; callers
    /// cannot supply a JSON fragment.
    pub fn to_protocol_event(&self, source: impl Into<String>) -> Result<JetDevtoolsEvent, String> {
        JetDevtoolsEvent::from_parts(
            self.timestamp_ms,
            source,
            "Job",
            self.job_id.clone(),
            self.render_fields_json(),
        )
    }

    pub fn render_fields_json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        jet_devtools_job_panel_render_key_value_text("event", self.kind.wire(), &mut out);
        jet_devtools_job_panel_render_key_value_text("state", self.state.wire(), &mut out);
        jet_devtools_job_panel_render_key_value_text("job_id", &self.job_id, &mut out);
        jet_devtools_job_panel_render_key_value_text("name", &self.name, &mut out);
        jet_devtools_job_panel_render_key_value_text("queue", &self.queue, &mut out);
        jet_devtools_job_panel_render_key_value_u64("attempts", u64::from(self.attempts), &mut out);
        jet_devtools_job_panel_render_key_value_optional_u64("duration_ms", self.duration_ms, &mut out);
        jet_devtools_job_panel_render_key_value_optional_text("worker", self.worker.as_deref(), &mut out);
        jet_devtools_job_panel_render_key_value_optional_text(
            "request_id",
            self.request_id.as_deref(),
            &mut out,
        );
        match self.failure {
            Some(failure) => jet_devtools_job_panel_render_key_value_text("failure", failure.wire(), &mut out),
            None => jet_devtools_job_panel_render_key_value_null("failure", &mut out),
        }
        jet_devtools_job_panel_render_labels(&self.labels, &mut out);
        out.push('}');
        out
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsJobPanelJobProjection {
    pub job_id: String,
    pub name: String,
    pub labels: Vec<JetDevtoolsJobPanelLabel>,
    pub queue: String,
    pub state: JetDevtoolsJobPanelLifecycleState,
    pub attempts: u32,
    pub duration_ms: Option<u64>,
    pub worker: Option<String>,
    pub request_id: Option<String>,
    pub failure: Option<JetDevtoolsJobPanelFailureKind>,
    /// The timestamp of the latest lifecycle fact for this job.
    pub freshness_ms: u64,
}

impl JetDevtoolsJobPanelJobProjection {
    pub fn request_correlation(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsJobPanelQueueProjection {
    pub queue: String,
    pub depth: u64,
    pub wait_ms: u64,
    /// Completed jobs retained in the fixed one-minute observation window.
    pub throughput: u64,
    pub workers: u64,
    pub capacity: u64,
    pub paused: bool,
    /// The latest timestamp observed for this queue configuration or a job.
    pub freshness_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsJobPanelProjection {
    pub jobs: Vec<JetDevtoolsJobPanelJobProjection>,
    pub queues: Vec<JetDevtoolsJobPanelQueueProjection>,
    pub history: Vec<JetDevtoolsJobPanelFact>,
    pub freshness_ms: u64,
}

impl JetDevtoolsJobPanelProjection {
    pub fn jobs_for_label(
        &self,
        filter: &JetDevtoolsJobPanelLabelFilter,
    ) -> Vec<JetDevtoolsJobPanelJobProjection> {
        self.jobs
            .iter()
            .filter(|job| filter.matches(&job.labels))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug)]
struct JetDevtoolsJobPanelJobRecord {
    projection: JetDevtoolsJobPanelJobProjection,
    waiting_since_ms: Option<u64>,
    started_at_ms: Option<u64>,
}

#[derive(Clone, Debug)]
struct JetDevtoolsJobPanelQueueRecord {
    name: String,
    capacity: u64,
    paused: bool,
    /// Queue counters come from a bounded durable snapshot when one is
    /// available.  Worker count is intentionally absent: it is derived only
    /// from the canonical lifecycle records below.
    snapshot_depth: Option<u64>,
    snapshot_wait_ms: Option<u64>,
    snapshot_throughput: Option<u64>,
    freshness_ms: u64,
}

/// One deterministic job lifecycle state machine and its bounded observation
/// history.  Records are kept in first-observed order instead of a hash map so
/// every host sees the same job and queue order.
#[derive(Clone, Debug)]
pub struct JetDevtoolsJobPanelState {
    max_history: usize,
    next_sequence: u64,
    last_timestamp_ms: Option<u64>,
    jobs: Vec<JetDevtoolsJobPanelJobRecord>,
    queues: Vec<JetDevtoolsJobPanelQueueRecord>,
    history: std::collections::VecDeque<JetDevtoolsJobPanelFact>,
}

impl Default for JetDevtoolsJobPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl JetDevtoolsJobPanelState {
    pub fn new() -> Self {
        Self::with_capacity(JET_DEVTOOLS_MAX_HISTORY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            max_history: capacity.clamp(1, JET_DEVTOOLS_MAX_HISTORY),
            next_sequence: 1,
            last_timestamp_ms: None,
            jobs: Vec::new(),
            queues: Vec::new(),
            history: std::collections::VecDeque::new(),
        }
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn jobs(&self) -> Vec<JetDevtoolsJobPanelJobProjection> {
        self.jobs.iter().map(|job| job.projection.clone()).collect()
    }

    pub fn jobs_filtered(
        &self,
        filter: Option<&JetDevtoolsJobPanelLabelFilter>,
    ) -> Vec<JetDevtoolsJobPanelJobProjection> {
        self.jobs
            .iter()
            .filter(|job| filter.map_or(true, |wanted| wanted.matches(&job.projection.labels)))
            .map(|job| job.projection.clone())
            .collect()
    }

    pub fn job(&self, job_id: &str) -> Option<JetDevtoolsJobPanelJobProjection> {
        self.jobs
            .iter()
            .find(|job| job.projection.job_id == job_id)
            .map(|job| job.projection.clone())
    }

    pub fn history(&self) -> Vec<JetDevtoolsJobPanelFact> {
        self.history.iter().cloned().collect()
    }

    pub fn latest_fact(&self) -> Option<JetDevtoolsJobPanelFact> {
        self.history.back().cloned()
    }
    /// Consume one typed lifecycle stream without introducing a second job
    /// identity or schedule model.  Queue snapshots are not part of this
    /// operation; workers are therefore derived from the `start` facts that
    /// the panel already retains.
    pub fn apply_fact(&mut self, fact: JetDevtoolsJobPanelFact) -> Result<(), String> {
        let expected_state = match fact.kind {
            JetDevtoolsJobPanelEventKind::Enqueue => JetDevtoolsJobPanelLifecycleState::Enqueued,
            JetDevtoolsJobPanelEventKind::Start => JetDevtoolsJobPanelLifecycleState::Started,
            JetDevtoolsJobPanelEventKind::Retry => JetDevtoolsJobPanelLifecycleState::Retrying,
            JetDevtoolsJobPanelEventKind::Complete => JetDevtoolsJobPanelLifecycleState::Completed,
            JetDevtoolsJobPanelEventKind::Fail => JetDevtoolsJobPanelLifecycleState::Failed,
        };
        if fact.state != expected_state {
            return Err(format!(
                "jet.devtools.v1 jobs {} fact has state {}, expected {}",
                fact.kind.wire(),
                fact.state.wire(),
                expected_state.wire()
            ));
        }
        match fact.kind {
            JetDevtoolsJobPanelEventKind::Enqueue => self.enqueue(
                fact.job_id,
                fact.name,
                fact.queue,
                fact.labels,
                fact.request_id,
                fact.timestamp_ms,
            ),
            JetDevtoolsJobPanelEventKind::Start => {
                let worker = fact.worker.ok_or_else(|| {
                    "jet.devtools.v1 jobs start fact must identify its worker".to_string()
                })?;
                self.start(&fact.job_id, fact.timestamp_ms, worker)
            }
            JetDevtoolsJobPanelEventKind::Retry => {
                self.retry(&fact.job_id, fact.timestamp_ms)
            }
            JetDevtoolsJobPanelEventKind::Complete => {
                self.complete(&fact.job_id, fact.timestamp_ms)
            }
            JetDevtoolsJobPanelEventKind::Fail => self.fail_with(
                &fact.job_id,
                fact.timestamp_ms,
                fact.failure
                    .unwrap_or(JetDevtoolsJobPanelFailureKind::Error),
            ),
        }
    }


    /// Add or replace the queue's typed capacity and pause facts.  This is
    /// configuration metadata, not a durable queue implementation.
    pub fn configure_queue(
        &mut self,
        queue: impl Into<String>,
        capacity: u64,
        paused: bool,
        timestamp_ms: u64,
    ) -> Result<(), String> {
        let queue = queue.into();
        jet_devtools_job_panel_validate_text(&queue, "queue")?;
        self.finish_timestamp(timestamp_ms)?;
        if let Some(existing) = self.queues.iter_mut().find(|entry| entry.name == queue) {
            existing.capacity = capacity;
            existing.paused = paused;
            existing.freshness_ms = timestamp_ms;
            self.last_timestamp_ms = Some(timestamp_ms);
            return Ok(());
        }
        if self.queues.len() >= self.max_history {
            return Err("jet.devtools.v1 jobs queue history is full".to_string());
        }
        self.queues.push(JetDevtoolsJobPanelQueueRecord {
            name: queue,
            capacity,
            paused,
            snapshot_depth: None,
            snapshot_wait_ms: None,
            snapshot_throughput: None,
            freshness_ms: timestamp_ms,
        });
        self.last_timestamp_ms = Some(timestamp_ms);
        Ok(())
    }
    /// Consume one bounded queue snapshot beside the canonical lifecycle
    /// records.  Worker count is deliberately absent: the panel derives it
    /// from `start` facts, so an omitted snapshot field cannot turn into an
    /// authoritative zero or erase a worker already observed.
    pub fn observe_queue_status(
        &mut self,
        queue: impl Into<String>,
        capacity: u64,
        paused: bool,
        depth: u64,
        wait_ms: u64,
        throughput: u64,
        timestamp_ms: u64,
    ) -> Result<(), String> {
        let queue = queue.into();
        jet_devtools_job_panel_validate_text(&queue, "queue")?;
        self.finish_timestamp(timestamp_ms)?;
        if let Some(existing) = self.queues.iter_mut().find(|entry| entry.name == queue) {
            existing.capacity = capacity;
            existing.paused = paused;
            existing.snapshot_depth = Some(depth);
            existing.snapshot_wait_ms = Some(wait_ms);
            existing.snapshot_throughput = Some(throughput);
            existing.freshness_ms = timestamp_ms;
            self.last_timestamp_ms = Some(timestamp_ms);
            return Ok(());
        }
        if self.queues.len() >= self.max_history {
            return Err("jet.devtools.v1 jobs queue history is full".to_string());
        }
        self.queues.push(JetDevtoolsJobPanelQueueRecord {
            name: queue,
            capacity,
            paused,
            snapshot_depth: Some(depth),
            snapshot_wait_ms: Some(wait_ms),
            snapshot_throughput: Some(throughput),
            freshness_ms: timestamp_ms,
        });
        self.last_timestamp_ms = Some(timestamp_ms);
        Ok(())
    }


    pub fn set_queue_paused(
        &mut self,
        queue: &str,
        paused: bool,
        timestamp_ms: u64,
    ) -> Result<(), String> {
        let index = self.queue_index(queue).ok_or_else(|| {
            format!("jet.devtools.v1 jobs queue `{queue}` has not been configured")
        })?;
        self.finish_timestamp(timestamp_ms)?;
        let entry = &mut self.queues[index];
        entry.paused = paused;
        entry.freshness_ms = timestamp_ms;
        self.last_timestamp_ms = Some(timestamp_ms);
        Ok(())
    }

    pub fn enqueue(
        &mut self,
        job_id: impl Into<String>,
        name: impl Into<String>,
        queue: impl Into<String>,
        labels: Vec<JetDevtoolsJobPanelLabel>,
        request_id: Option<String>,
        timestamp_ms: u64,
    ) -> Result<(), String> {
        let job_id = job_id.into();
        let name = name.into();
        let queue = queue.into();
        jet_devtools_job_panel_validate_text(&job_id, "job_id")?;
        jet_devtools_job_panel_validate_text(&name, "name")?;
        jet_devtools_job_panel_validate_text(&queue, "queue")?;
        jet_devtools_job_panel_validate_labels(&labels)?;
        jet_devtools_job_panel_validate_optional_text(request_id.as_deref(), "request_id")?;
        if self.jobs.iter().any(|job| job.projection.job_id == job_id) {
            return Err(format!("jet.devtools.v1 jobs already contains `{job_id}`"));
        }
        self.finish_timestamp(timestamp_ms)?;
        self.check_job_room()?;
        self.check_queue_room(&queue)?;
        self.ensure_job_room()?;
        self.ensure_queue(&queue, timestamp_ms)?;
        self.last_timestamp_ms = Some(timestamp_ms);
        let projection = JetDevtoolsJobPanelJobProjection {
            job_id: job_id.clone(),
            name: name.clone(),
            labels: labels.clone(),
            queue: queue.clone(),
            state: JetDevtoolsJobPanelLifecycleState::Enqueued,
            attempts: 0,
            duration_ms: None,
            worker: None,
            request_id: request_id.clone(),
            failure: None,
            freshness_ms: timestamp_ms,
        };
        self.jobs.push(JetDevtoolsJobPanelJobRecord {
            projection,
            waiting_since_ms: Some(timestamp_ms),
            started_at_ms: None,
        });
        self.record_fact(JetDevtoolsJobPanelFact {
            sequence: 0,
            timestamp_ms,
            kind: JetDevtoolsJobPanelEventKind::Enqueue,
            state: JetDevtoolsJobPanelLifecycleState::Enqueued,
            job_id,
            name,
            labels,
            queue,
            attempts: 0,
            duration_ms: None,
            worker: None,
            request_id,
            failure: None,
        })
    }

    pub fn start(
        &mut self,
        job_id: &str,
        timestamp_ms: u64,
        worker: impl Into<String>,
    ) -> Result<(), String> {
        let worker = worker.into();
        jet_devtools_job_panel_validate_text(&worker, "worker")?;
        let index = self.job_index(job_id)?;
        let current = self.jobs[index].projection.state;
        if !matches!(
            current,
            JetDevtoolsJobPanelLifecycleState::Enqueued
                | JetDevtoolsJobPanelLifecycleState::Retrying
        ) {
            return self.invalid_transition(job_id, current, JetDevtoolsJobPanelEventKind::Start);
        }
        self.accept_timestamp(timestamp_ms)?;
        let record = &mut self.jobs[index];
        if record.projection.attempts == 0 {
            record.projection.attempts = 1;
        }
        record.projection.state = JetDevtoolsJobPanelLifecycleState::Started;
        record.projection.worker = Some(worker.clone());
        record.projection.freshness_ms = timestamp_ms;
        record.waiting_since_ms = None;
        record.started_at_ms = Some(timestamp_ms);
        self.record_fact(self.fact_from_record(
            index,
            JetDevtoolsJobPanelEventKind::Start,
            timestamp_ms,
            None,
        ))
    }

    pub fn retry(&mut self, job_id: &str, timestamp_ms: u64) -> Result<(), String> {
        let index = self.job_index(job_id)?;
        let current = self.jobs[index].projection.state;
        if current != JetDevtoolsJobPanelLifecycleState::Started {
            return self.invalid_transition(job_id, current, JetDevtoolsJobPanelEventKind::Retry);
        }
        self.accept_timestamp(timestamp_ms)?;
        let record = &mut self.jobs[index];
        record.projection.state = JetDevtoolsJobPanelLifecycleState::Retrying;
        record.projection.attempts = record.projection.attempts.saturating_add(1);
        record.projection.duration_ms = None;
        record.projection.freshness_ms = timestamp_ms;
        record.waiting_since_ms = Some(timestamp_ms);
        record.started_at_ms = None;
        self.record_fact(self.fact_from_record(
            index,
            JetDevtoolsJobPanelEventKind::Retry,
            timestamp_ms,
            None,
        ))
    }

    pub fn complete(&mut self, job_id: &str, timestamp_ms: u64) -> Result<(), String> {
        self.finish(job_id, timestamp_ms, JetDevtoolsJobPanelEventKind::Complete, None)
    }

    pub fn fail(&mut self, job_id: &str, timestamp_ms: u64) -> Result<(), String> {
        self.finish(
            job_id,
            timestamp_ms,
            JetDevtoolsJobPanelEventKind::Fail,
            Some(JetDevtoolsJobPanelFailureKind::Error),
        )
    }

    pub fn fail_with(
        &mut self,
        job_id: &str,
        timestamp_ms: u64,
        failure: JetDevtoolsJobPanelFailureKind,
    ) -> Result<(), String> {
        self.finish(
            job_id,
            timestamp_ms,
            JetDevtoolsJobPanelEventKind::Fail,
            Some(failure),
        )
    }

    pub fn queue_projection(
        &self,
        queue: &str,
        now_ms: u64,
        filter: Option<&JetDevtoolsJobPanelLabelFilter>,
    ) -> Option<JetDevtoolsJobPanelQueueProjection> {
        let config = self.queues.iter().find(|entry| entry.name == queue)?;
        Some(self.queue_projection_from(config, now_ms, filter))
    }

    pub fn queues(
        &self,
        now_ms: u64,
        filter: Option<&JetDevtoolsJobPanelLabelFilter>,
    ) -> Vec<JetDevtoolsJobPanelQueueProjection> {
        self.queues
            .iter()
            .filter_map(|queue| {
                let has_match = self.jobs.iter().any(|job| {
                    job.projection.queue == queue.name
                        && filter.map_or(true, |wanted| wanted.matches(&job.projection.labels))
                });
                if filter.is_some() && !has_match {
                    None
                } else {
                    Some(self.queue_projection_from(queue, now_ms, filter))
                }
            })
            .collect()
    }

    pub fn project(
        &self,
        now_ms: u64,
        filter: Option<&JetDevtoolsJobPanelLabelFilter>,
    ) -> JetDevtoolsJobPanelProjection {
        let jobs = self.jobs_filtered(filter);
        let queues = self.queues(now_ms, filter);
        let freshness_ms = self
            .last_timestamp_ms
            .unwrap_or(0)
            .max(queues.iter().map(|queue| queue.freshness_ms).max().unwrap_or(0));
        JetDevtoolsJobPanelProjection {
            jobs,
            queues,
            history: self.history(),
            freshness_ms,
        }
    }

    pub fn protocol_events(&self, source: impl Into<String>) -> Result<Vec<JetDevtoolsEvent>, String> {
        let source = source.into();
        self.history
            .iter()
            .map(|fact| fact.to_protocol_event(source.clone()))
            .collect()
    }

    fn finish(
        &mut self,
        job_id: &str,
        timestamp_ms: u64,
        kind: JetDevtoolsJobPanelEventKind,
        failure: Option<JetDevtoolsJobPanelFailureKind>,
    ) -> Result<(), String> {
        let index = self.job_index(job_id)?;
        let current = self.jobs[index].projection.state;
        if current != JetDevtoolsJobPanelLifecycleState::Started {
            return self.invalid_transition(job_id, current, kind);
        }
        self.accept_timestamp(timestamp_ms)?;
        let record = &mut self.jobs[index];
        let duration_ms = record
            .started_at_ms
            .map(|started| timestamp_ms.saturating_sub(started));
        record.projection.state = if failure.is_some() {
            JetDevtoolsJobPanelLifecycleState::Failed
        } else {
            JetDevtoolsJobPanelLifecycleState::Completed
        };
        record.projection.duration_ms = duration_ms;
        record.projection.failure = failure;
        record.projection.freshness_ms = timestamp_ms;
        record.waiting_since_ms = None;
        self.record_fact(self.fact_from_record(index, kind, timestamp_ms, duration_ms))
    }

    fn fact_from_record(
        &self,
        index: usize,
        kind: JetDevtoolsJobPanelEventKind,
        timestamp_ms: u64,
        duration_ms: Option<u64>,
    ) -> JetDevtoolsJobPanelFact {
        let projection = &self.jobs[index].projection;
        JetDevtoolsJobPanelFact {
            sequence: 0,
            timestamp_ms,
            kind,
            state: projection.state,
            job_id: projection.job_id.clone(),
            name: projection.name.clone(),
            labels: projection.labels.clone(),
            queue: projection.queue.clone(),
            attempts: projection.attempts,
            duration_ms: duration_ms.or(projection.duration_ms),
            worker: projection.worker.clone(),
            request_id: projection.request_id.clone(),
            failure: projection.failure,
        }
    }

    fn record_fact(&mut self, mut fact: JetDevtoolsJobPanelFact) -> Result<(), String> {
        fact.sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "jet.devtools.v1 jobs event sequence exhausted".to_string())?;
        if self.history.len() == self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(fact);
        Ok(())
    }

    fn finish_timestamp(&self, timestamp_ms: u64) -> Result<(), String> {
        if let Some(previous) = self.last_timestamp_ms {
            if timestamp_ms < previous {
                return Err("jet.devtools.v1 jobs timestamps must be monotonic".to_string());
            }
        }
        Ok(())
    }

    fn accept_timestamp(&mut self, timestamp_ms: u64) -> Result<(), String> {
        self.finish_timestamp(timestamp_ms)?;
        self.last_timestamp_ms = Some(timestamp_ms);
        Ok(())
    }

    fn ensure_queue(&mut self, queue: &str, timestamp_ms: u64) -> Result<(), String> {
        if let Some(existing) = self.queues.iter_mut().find(|entry| entry.name == queue) {
            existing.freshness_ms = existing.freshness_ms.max(timestamp_ms);
            return Ok(());
        }
        self.queues.push(JetDevtoolsJobPanelQueueRecord {
            name: queue.to_string(),
            capacity: 1,
            paused: false,
            snapshot_depth: None,
            snapshot_wait_ms: None,
            snapshot_throughput: None,
            freshness_ms: timestamp_ms,
        });
        Ok(())
    }

    fn check_queue_room(&self, queue: &str) -> Result<(), String> {
        if self.queue_index(queue).is_none() && self.queues.len() >= self.max_history {
            return Err("jet.devtools.v1 jobs queue history is full".to_string());
        }
        Ok(())
    }

    fn check_job_room(&self) -> Result<(), String> {
        if self.jobs.len() < self.max_history
            || self
                .jobs
                .iter()
                .any(|job| job.projection.state.is_terminal())
        {
            Ok(())
        } else {
            Err("jet.devtools.v1 jobs history is full of active jobs".to_string())
        }
    }

    fn ensure_job_room(&mut self) -> Result<(), String> {
        if self.jobs.len() < self.max_history {
            return Ok(());
        }
        if let Some(index) = self
            .jobs
            .iter()
            .position(|job| job.projection.state.is_terminal())
        {
            self.jobs.remove(index);
            Ok(())
        } else {
            Err("jet.devtools.v1 jobs history is full of active jobs".to_string())
        }
    }

    fn queue_index(&self, queue: &str) -> Option<usize> {
        self.queues.iter().position(|entry| entry.name == queue)
    }

    fn job_index(&self, job_id: &str) -> Result<usize, String> {
        self.jobs
            .iter()
            .position(|job| job.projection.job_id == job_id)
            .ok_or_else(|| format!("jet.devtools.v1 jobs has no job `{job_id}`"))
    }

    fn invalid_transition(
        &self,
        job_id: &str,
        state: JetDevtoolsJobPanelLifecycleState,
        event: JetDevtoolsJobPanelEventKind,
    ) -> Result<(), String> {
        Err(format!(
            "jet.devtools.v1 jobs cannot apply {} to `{job_id}` from {}",
            event.wire(),
            state.wire()
        ))
    }

    fn queue_projection_from(
        &self,
        config: &JetDevtoolsJobPanelQueueRecord,
        now_ms: u64,
        filter: Option<&JetDevtoolsJobPanelLabelFilter>,
    ) -> JetDevtoolsJobPanelQueueProjection {
        let mut depth = 0u64;
        let mut workers = 0u64;
        let mut oldest_waiting_since = None;
        for job in self.jobs.iter().filter(|job| {
            job.projection.queue == config.name
                && filter.map_or(true, |wanted| wanted.matches(&job.projection.labels))
        }) {
            if job.projection.state.is_pending() {
                depth = depth.saturating_add(1);
                if let Some(waiting_since) = job.waiting_since_ms {
                    oldest_waiting_since = Some(
                        oldest_waiting_since
                            .map_or(waiting_since, |oldest: u64| oldest.min(waiting_since)),
                    );
                }
            }
            if job.projection.state == JetDevtoolsJobPanelLifecycleState::Started {
                workers = workers.saturating_add(1);
            }
        }
        let derived_wait_ms = oldest_waiting_since
            .map(|started| now_ms.saturating_sub(started))
            .unwrap_or(0);
        let cutoff = now_ms.saturating_sub(JET_DEVTOOLS_JOB_PANEL_THROUGHPUT_WINDOW_MS);
        let derived_throughput = self
            .history
            .iter()
            .filter(|fact| {
                fact.queue == config.name
                    && fact.kind == JetDevtoolsJobPanelEventKind::Complete
                    && fact.timestamp_ms >= cutoff
                    && filter.map_or(true, |wanted| wanted.matches(&fact.labels))
            })
            .count() as u64;
        let depth = config.snapshot_depth.unwrap_or(depth);
        let wait_ms = config.snapshot_wait_ms.unwrap_or(derived_wait_ms);
        let throughput = config.snapshot_throughput.unwrap_or(derived_throughput);
        let freshness_ms = config.freshness_ms.max(
            self.jobs
                .iter()
                .filter(|job| {
                    job.projection.queue == config.name
                        && filter.map_or(true, |wanted| wanted.matches(&job.projection.labels))
                })
                .map(|job| job.projection.freshness_ms)
                .max()
                .unwrap_or(0),
        );
        JetDevtoolsJobPanelQueueProjection {
            queue: config.name.clone(),
            depth,
            wait_ms,
            throughput,
            workers,
            capacity: config.capacity,
            paused: config.paused,
            freshness_ms,
        }
    }
}

fn jet_devtools_job_panel_validate_optional_text(
    value: Option<&str>,
    field: &str,
) -> Result<(), String> {
    if let Some(value) = value {
        jet_devtools_job_panel_validate_text(value, field)?;
    }
    Ok(())
}

fn jet_devtools_job_panel_validate_text(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("jet.devtools.v1 jobs {field} must not be empty"));
    }
    if value.len() > JET_DEVTOOLS_JOB_PANEL_MAX_TEXT_BYTES {
        return Err(format!("jet.devtools.v1 jobs {field} exceeds the Prelude limit"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("jet.devtools.v1 jobs {field} contains a control character"));
    }
    Ok(())
}

fn jet_devtools_job_panel_validate_labels(
    labels: &[JetDevtoolsJobPanelLabel],
) -> Result<(), String> {
    if labels.len() > JET_DEVTOOLS_JOB_PANEL_MAX_LABELS {
        return Err("jet.devtools.v1 jobs has too many labels".to_string());
    }
    for (index, label) in labels.iter().enumerate() {
        jet_devtools_job_panel_validate_text(&label.key, "label key")?;
        if ["args", "argv", "argument", "arguments", "body", "payload"]
            .iter()
            .any(|reserved| label.key.eq_ignore_ascii_case(reserved))
        {
            return Err(format!(
                "jet.devtools.v1 jobs label {index} uses a reserved payload name"
            ));
        }
        if labels[index + 1..].iter().any(|other| other.key == label.key) {
            return Err(format!(
                "jet.devtools.v1 jobs contains duplicate label `{}`",
                label.key
            ));
        }
        if let JetDevtoolsJobPanelLabelValue::Text(value) = &label.value {
            jet_devtools_job_panel_validate_text(value, "label value")?;
        }
    }
    Ok(())
}

fn jet_devtools_job_panel_render_text(value: &str, out: &mut String) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

fn jet_devtools_job_panel_render_key(out: &mut String, key: &str) {
    if out.len() > 1 {
        out.push(',');
    }
    jet_devtools_job_panel_render_text(key, out);
    out.push(':');
}

fn jet_devtools_job_panel_render_key_value_text(key: &str, value: &str, out: &mut String) {
    jet_devtools_job_panel_render_key(out, key);
    jet_devtools_job_panel_render_text(value, out);
}

fn jet_devtools_job_panel_render_key_value_u64(key: &str, value: u64, out: &mut String) {
    jet_devtools_job_panel_render_key(out, key);
    out.push_str(&value.to_string());
}

fn jet_devtools_job_panel_render_key_value_optional_u64(
    key: &str,
    value: Option<u64>,
    out: &mut String,
) {
    jet_devtools_job_panel_render_key(out, key);
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn jet_devtools_job_panel_render_key_value_optional_text(
    key: &str,
    value: Option<&str>,
    out: &mut String,
) {
    jet_devtools_job_panel_render_key(out, key);
    match value {
        Some(value) => jet_devtools_job_panel_render_text(value, out),
        None => out.push_str("null"),
    }
}

fn jet_devtools_job_panel_render_key_value_null(key: &str, out: &mut String) {
    jet_devtools_job_panel_render_key(out, key);
    out.push_str("null");
}

fn jet_devtools_job_panel_render_labels(labels: &[JetDevtoolsJobPanelLabel], out: &mut String) {
    jet_devtools_job_panel_render_key(out, "labels");
    out.push('{');
    for (index, label) in labels.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        jet_devtools_job_panel_render_text(&label.key, out);
        out.push(':');
        label.value.render_json(out);
    }
    out.push('}');
}

#[cfg(test)]
mod jet_devtools_jobs_panel_tests {
    use super::*;

    #[test]
    fn lifecycle_is_ordered_and_rejects_terminal_reuse() {
        let mut state = JetDevtoolsJobPanelState::with_capacity(8);
        state
            .enqueue(
                "job-1",
                "Sync",
                "default",
                vec![JetDevtoolsJobPanelLabel::text("tenant", "acme")],
                Some("request-1".to_string()),
                10,
            )
            .unwrap();
        state.start("job-1", 20, "worker-1").unwrap();
        state.retry("job-1", 30).unwrap();
        state.start("job-1", 40, "worker-2").unwrap();
        state.complete("job-1", 50).unwrap();
        assert_eq!(
            state.history().iter().map(|fact| fact.kind).collect::<Vec<_>>(),
            vec![
                JetDevtoolsJobPanelEventKind::Enqueue,
                JetDevtoolsJobPanelEventKind::Start,
                JetDevtoolsJobPanelEventKind::Retry,
                JetDevtoolsJobPanelEventKind::Start,
                JetDevtoolsJobPanelEventKind::Complete,
            ]
        );
        assert_eq!(state.job("job-1").unwrap().attempts, 2);
        assert_eq!(state.job("job-1").unwrap().duration_ms, Some(10));
        assert!(state.fail("job-1", 60).is_err());
    }

    #[test]
    fn queue_projection_has_metrics_and_label_filter_has_no_payload_path() {
        let mut state = JetDevtoolsJobPanelState::new();
        state.configure_queue("default", 4, false, 1).unwrap();
        state
            .enqueue(
                "job-1",
                "Sync",
                "default",
                vec![JetDevtoolsJobPanelLabel::integer("shard", 2)],
                Some("req-1".to_string()),
                10,
            )
            .unwrap();
        state.start("job-1", 20, "worker-1").unwrap();
        state.complete("job-1", 30).unwrap();
        let projection = state.project(
            40,
            Some(&JetDevtoolsJobPanelLabelFilter::new(
                "shard",
                JetDevtoolsJobPanelLabelValue::integer(2),
            )),
        );
        assert_eq!(projection.queues[0].throughput, 1);
        assert_eq!(projection.queues[0].capacity, 4);
        assert_eq!(projection.jobs[0].request_correlation(), Some("req-1"));
        let event = state.protocol_events("test").unwrap().pop().unwrap();
        assert!(event.fields_json().contains("labels"));
        assert!(!event.fields_json().contains("arguments"));
        assert!(!event.fields_json().contains("payload"));
    }

    #[test]
    fn history_is_bounded_in_sequence_order() {
        let mut state = JetDevtoolsJobPanelState::with_capacity(2);
        for index in 0..3 {
            state
                .enqueue(
                    format!("job-{index}"),
                    "Sync",
                    "default",
                    Vec::new(),
                    None,
                    index * 3,
                )
                .unwrap();
            state.start(&format!("job-{index}"), index * 3 + 1, "worker").unwrap();
            state.complete(&format!("job-{index}"), index * 3 + 2).unwrap();
        }
        assert_eq!(state.history_len(), 2);
        assert_eq!(state.history()[0].sequence, 8);
        assert_eq!(state.history()[1].sequence, 9);
    }
}

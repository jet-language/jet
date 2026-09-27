//! Per-session transport for retained Source callback invocations.
//!
//! This module deliberately does not know how a callback is evaluated. A
//! request owns only a command, task context, and one-shot reply channel; the
//! session owner pumps requests and uses its own retained Source/Eval graph to
//! execute them. Payloads and identities are session-local: there is no
//! process-wide callback table and no machine pointer in this transport.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};

/// Session-local identity for one retained callback payload.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceCallbackId(u64);

impl SourceCallbackId {
    /// Return the opaque session-local identity carried by this lease.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Session-local identity for one queued invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceCallbackRequestId(u64);

impl SourceCallbackRequestId {
    /// Return the opaque session-local identity carried by this request.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Transport and helper-execution failures. Evaluation and task policy stay
/// outside this enum; ordinary Source outcomes travel through the generic
/// reply value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceCallbackError {
    /// The session has finished retirement and accepts no new work.
    Closed,
    /// Retirement has started and accepts no new payloads or invocations.
    Retiring,
    /// A lease or event referred to an identity that is no longer present.
    UnknownCallback,
    /// A release event was observed before its invocation/lease count drained.
    ReleaseNotReady,
    /// A payload is temporarily owned by an owner-side borrow or release event.
    PayloadBusy,
    /// The one-shot reply receiver was dropped before a response was sent.
    ReplyClosed,
    /// A reply value is still owned by the pump and must be taken before
    /// cleanup can be committed.
    ReplyValueBusy,
    /// The retained payload mutex was poisoned by its owner thread.
    PayloadPoisoned,
    /// The session-local checked identity space is exhausted.
    IdExhausted,
    /// Native helper dispatch or transport failed before a Source result existed.
    ExecutionFailed { detail: String },
}
impl fmt::Display for SourceCallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("Source callback session is closed"),
            Self::Retiring => f.write_str("Source callback session is retiring"),
            Self::UnknownCallback => f.write_str("unknown Source callback lease"),
            Self::ReleaseNotReady => f.write_str("Source callback release is not ready"),
            Self::PayloadBusy => f.write_str("Source callback payload is temporarily owned"),
            Self::ReplyClosed => f.write_str("Source callback reply receiver is closed"),
            Self::ReplyValueBusy => f.write_str("Source callback reply value is still owned"),
            Self::PayloadPoisoned => f.write_str("Source callback payload lock is poisoned"),
            Self::IdExhausted => f.write_str("Source callback identity space is exhausted"),
            Self::ExecutionFailed { detail } => {
                write!(f, "Source callback execution failed: {detail}")
            }
        }
    }
}

impl std::error::Error for SourceCallbackError {}

/// Failure while sending a one-shot invocation response.
#[derive(Debug, Eq, PartialEq)]
pub enum SourceCallbackReplyError<R> {
    /// The callback owner already sent one response. The rejected value stays
    /// owned by this error for Source cleanup.
    AlreadySent(Result<R, SourceCallbackError>),
    /// The producer stopped waiting. The exact result is queued to the owner
    /// pump's ReplyCleanup event and remains an invocation obligation until
    /// that event commits Source cleanup.
    Disconnected,
}

impl<R> fmt::Display for SourceCallbackReplyError<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadySent(_) => f.write_str("Source callback reply was already sent"),
            Self::Disconnected => {
                f.write_str("Source callback reply receiver is disconnected")
            }
        }
    }
}

impl<R: fmt::Debug + Send + 'static> std::error::Error for SourceCallbackReplyError<R> {}


/// The owned command/context returned when enqueue is rejected. The caller
/// must decide how to clean up those Source values; this transport never drops
/// a rejected logical command implicitly.
pub struct SourceCallbackEnqueueError<C, X> {
    pub error: SourceCallbackError,
    pub command: C,
    pub context: X,
}

impl<C, X> SourceCallbackEnqueueError<C, X> {
    /// Split the error into its transport reason and owned command envelope.
    pub fn into_parts(self) -> (SourceCallbackError, C, X) {
        (self.error, self.command, self.context)
    }
}

impl<C, X> fmt::Debug for SourceCallbackEnqueueError<C, X>
where
    C: fmt::Debug,
    X: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceCallbackEnqueueError")
            .field("error", &self.error)
            .field("command", &self.command)
            .field("context", &self.context)
            .finish()
    }
}

/// The owned payload returned when registration is rejected.
pub struct SourceCallbackRegisterError<P> {
    pub error: SourceCallbackError,
    pub payload: P,
}

impl<P> SourceCallbackRegisterError<P> {
    /// Split the error into its transport reason and owned payload.
    pub fn into_parts(self) -> (SourceCallbackError, P) {
        (self.error, self.payload)
    }
}

impl<P> fmt::Debug for SourceCallbackRegisterError<P>
where
    P: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceCallbackRegisterError")
            .field("error", &self.error)
            .field("payload", &self.payload)
            .finish()
    }
}

/// Counts that must reach zero before a session can become retired.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceCallbackDrainStatus {
    /// Live callback lease aliases, including aliases held by producers.
    pub producers: usize,
    /// Invocations that have been dequeued but whose event is still alive.
    pub inflight: usize,
    /// Events waiting in the owner pump's queue.
    pub pending: usize,
    /// Owner-side payload borrows that must return their temporary roots.
    pub borrows: usize,
    /// Release events waiting for the owner to drop their retained payload.
    pub releases: usize,
}

impl SourceCallbackDrainStatus {
    fn drained(self) -> bool {
        self.producers == 0
            && self.inflight == 0
            && self.pending == 0
            && self.borrows == 0
            && self.releases == 0
    }
}

/// Type-erased callback-owner job retained by the origin lifecycle tracker.
pub type SourceCallbackJob = Box<
    dyn FnOnce() -> Result<(), jet_codegen::scheduler::JetTaskFailure> + Send + 'static,
>;

/// Result of draining callback-owner jobs.
#[derive(Debug)]
pub enum SourceCallbackJobDrainOutcome {
    /// The actual callback-session lifecycle has not closed, or jobs remain.
    Pending {
        open_callback_sessions: usize,
        pending_jobs: usize,
    },
    /// Every submitted job has been joined; failures remain typed and complete.
    Complete {
        failures: Vec<jet_codegen::scheduler::JetTaskFailure>,
    },
}

/// Shared object-safe seam for callback jobs owned by one origin runtime.
///
/// Implementations must balance `session_started` and `session_finished`.
/// `close_admission` prevents new registrations but does not reject cleanup or
/// Ready jobs from sessions that already hold a registration. A drain reports
/// `Pending` until real callback sessions have closed and all queued jobs are
/// consumed. Jobs are joined in batches outside tracker locks; all job and join
/// failures are accumulated rather than short-circuiting. If a drain unwinds,
/// active and unjoined handles and their completion debt must remain available
/// for the next drain. `Complete` is valid only once no callback session or
/// job remains; accumulated failures remain available for repeat reporting.
pub trait SourceCallbackJobOwner: Send + Sync {
    /// Admit one callback session before creating its owned session state.
    /// On success, acquire exactly one session registration; on error, change
    /// no registration count.
    fn session_started(&self) -> Result<(), String>;
    /// Balance one admitted session after normal or abandonment cleanup
    /// completes; missing registrations are an invariant error, never saturated.
    fn session_finished(&self);
    /// Seal session admission while existing sessions may still submit jobs.
    fn close_admission(&self);
    /// Return the exact owned job if admission is rejected. Existing
    /// registrations may submit cleanup and Ready jobs after close.
    fn submit(&self, job: SourceCallbackJob) -> Result<(), SourceCallbackJob>;
    fn drain(&self) -> SourceCallbackJobDrainOutcome;
}

/// Balanced registration tying one actual callback session to its job owner.
#[must_use = "retain the registration until callback-session cleanup completes"]
pub struct SourceCallbackJobSessionRegistration {
    owner: Arc<dyn SourceCallbackJobOwner>,
}

impl SourceCallbackJobSessionRegistration {
    /// Register before creating the owned callback session.
    pub fn start(owner: Arc<dyn SourceCallbackJobOwner>) -> Result<Self, String> {
        owner.session_started()?;
        Ok(Self { owner })
    }

    /// Submit cleanup or Ready work for this still-registered session.
    pub fn submit(&self, job: SourceCallbackJob) -> Result<(), SourceCallbackJob> {
        self.owner.submit(job)
    }
}

impl Drop for SourceCallbackJobSessionRegistration {
    fn drop(&mut self) {
        self.owner.session_finished();
    }
}

/// Retirement was requested before all producers and callback events drained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceCallbackRetireError {
    NotDrained(SourceCallbackDrainStatus),
}

impl fmt::Display for SourceCallbackRetireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDrained(status) => write!(
                f,
                "Source callback session is not drained (producers={}, inflight={}, pending={}, borrows={}, releases={})",
                status.producers,
                status.inflight,
                status.pending,
                status.borrows,
                status.releases
            ),
        }
    }
}

impl std::error::Error for SourceCallbackRetireError {}

/// A one-shot reply returned to the producer of a queued invocation.
///
/// The result is stored in a transport-owned cell rather than directly in a
/// channel queue.  This matters for Source values: a successful notification
/// only means that the result became available, not that the producer took it.
/// Dropping this reply therefore transfers an undelivered successful value to
/// an owner-pump cleanup event instead of letting Rust drop it in `mpsc`.
pub struct SourceCallbackReply<R> {
    cell: Arc<ReplyCell<R>>,
}

impl<R> SourceCallbackReply<R> {
    /// Wait for the owner pump to publish this invocation's result.
    pub fn recv(self) -> Result<R, SourceCallbackError> {
        self.cell.take_blocking()
    }

    /// Poll this reply without blocking.
    pub fn try_recv(&self) -> Result<Option<R>, SourceCallbackError> {
        match self.cell.try_take()? {
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        }
    }

    /// Wait for a bounded interval for the owner pump to publish this result.
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<R>, SourceCallbackError> {
        match self.cell.take_timeout(timeout)? {
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        }
    }
}

impl<R> Drop for SourceCallbackReply<R> {
    fn drop(&mut self) {
        self.cell.drop_receiver();
    }
}

enum ReplyCellState<R> {
    Pending,
    Ready(Option<Result<R, SourceCallbackError>>),
    ReceiverGone,
    Consumed,
}

/// Shared one-shot result state. `on_lost` transfers an unreceived result to
/// owner-side Source cleanup without retaining the callback state while an
/// invocation is still queued.
struct ReplyCell<R> {
    state: Mutex<ReplyCellState<R>>,
    wake: Condvar,
    completion: Arc<InvocationCompletion>,
    state_anchor: Mutex<Option<Arc<dyn std::any::Any + Send + Sync>>>,
    on_publish:
        Arc<dyn Fn() -> Option<Arc<dyn std::any::Any + Send + Sync>> + Send + Sync>,
    on_lost: Arc<dyn Fn(Result<R, SourceCallbackError>) + Send + Sync>,
}

impl<R: Send + 'static> ReplyCell<R> {
    fn send(
        &self,
        result: Result<R, SourceCallbackError>,
    ) -> Result<(), Result<R, SourceCallbackError>> {
        let state_anchor = (self.on_publish)();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *state {
            ReplyCellState::Pending => {
                *self
                    .state_anchor
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = state_anchor;
                *state = ReplyCellState::Ready(Some(result));
                self.wake.notify_all();
                Ok(())
            }
            ReplyCellState::ReceiverGone | ReplyCellState::Consumed => Err(result),
            ReplyCellState::Ready(_) => Err(result),
        }
    }

    fn sender_drop(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*state {
            ReplyCellState::Pending => {
                *state = ReplyCellState::Ready(Some(Err(SourceCallbackError::ReplyClosed)));
                self.wake.notify_all();
            }
            ReplyCellState::ReceiverGone => self.completion.reply_done(),
            ReplyCellState::Ready(_) | ReplyCellState::Consumed => {}
        }
    }

    fn release_state_anchor(&self) {
        self.state_anchor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }

    fn take_blocking(&self) -> Result<R, SourceCallbackError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            match &mut *state {
                ReplyCellState::Pending => {
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                ReplyCellState::Ready(result) => {
                    let result = result
                        .take()
                        .expect("Source callback reply result was consumed twice");
                    *state = ReplyCellState::Consumed;
                    drop(state);
                    self.completion.reply_done();
                    self.release_state_anchor();
                    return result;
                }
                ReplyCellState::ReceiverGone | ReplyCellState::Consumed => {
                    return Err(SourceCallbackError::ReplyClosed);
                }
            }
        }
    }

    fn try_take(&self) -> Result<Option<Result<R, SourceCallbackError>>, SourceCallbackError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *state {
            ReplyCellState::Pending => Ok(None),
            ReplyCellState::Ready(result) => {
                let result = result
                    .take()
                    .expect("Source callback reply result was consumed twice");
                *state = ReplyCellState::Consumed;
                drop(state);
                self.completion.reply_done();
                self.release_state_anchor();
                Ok(Some(result))
            }
            ReplyCellState::ReceiverGone | ReplyCellState::Consumed => {
                Err(SourceCallbackError::ReplyClosed)
            }
        }
    }

    fn take_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<Result<R, SourceCallbackError>>, SourceCallbackError> {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            match &mut *state {
                ReplyCellState::Pending => {
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        return Ok(None);
                    }
                    let remaining = deadline.saturating_duration_since(now);
                    let (next, wait) = self
                        .wake
                        .wait_timeout(state, remaining)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    state = next;
                    if wait.timed_out() && matches!(&*state, ReplyCellState::Pending) {
                        return Ok(None);
                    }
                }
                ReplyCellState::Ready(result) => {
                    let result = result
                        .take()
                        .expect("Source callback reply result was consumed twice");
                    *state = ReplyCellState::Consumed;
                    drop(state);
                    self.completion.reply_done();
                    self.release_state_anchor();
                    return Ok(Some(result));
                }
                ReplyCellState::ReceiverGone | ReplyCellState::Consumed => {
                    return Err(SourceCallbackError::ReplyClosed);
                }
            }
        }
    }

    fn drop_receiver(&self) {
        let (lost, reply_done, release_state_anchor) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &mut *state {
                ReplyCellState::Pending => {
                    *state = ReplyCellState::ReceiverGone;
                    (None, false, false)
                }
                ReplyCellState::Ready(result) => {
                    let result = result
                        .take()
                        .expect("Source callback reply result was consumed twice");
                    *state = ReplyCellState::ReceiverGone;
                    if result.is_ok() {
                        (Some(result), false, true)
                    } else {
                        (None, true, true)
                    }
                }
                ReplyCellState::ReceiverGone | ReplyCellState::Consumed => {
                    (None, false, false)
                }
            }
        };
        if let Some(result) = lost {
            (self.on_lost)(result);
        }
        if reply_done {
            self.completion.reply_done();
        }
        if release_state_anchor {
            self.release_state_anchor();
        }
    }
}


/// Shared completion claim for one callback invocation.  Input cleanup and
/// reply delivery are independent obligations; callback retirement waits for
/// both claims, including when a responder is detached from its event.
struct InvocationCompletion {
    inputs_done: std::sync::atomic::AtomicBool,
    reply_done: std::sync::atomic::AtomicBool,
    finished: std::sync::atomic::AtomicBool,
    finish: Box<dyn Fn() + Send + Sync>,
}

impl InvocationCompletion {
    fn inputs_done(&self) {
        self.inputs_done.store(true, std::sync::atomic::Ordering::Release);
        self.try_finish();
    }

    fn reply_done(&self) {
        self.reply_done.store(true, std::sync::atomic::Ordering::Release);
        self.try_finish();
    }

    fn try_finish(&self) {
        if self.inputs_done.load(std::sync::atomic::Ordering::Acquire)
            && self.reply_done.load(std::sync::atomic::Ordering::Acquire)
            && !self.finished.swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            (self.finish)();
        }
    }
}

/// Owner-side half of a one-shot invocation reply.
pub struct SourceCallbackResponder<R> {
    reply: Option<Arc<ReplyCell<R>>>,
    completion: Arc<InvocationCompletion>,
    state_anchor: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

impl<R: Send + 'static> SourceCallbackResponder<R> {
    /// Publish either the callback value or a transport-level failure.
    pub fn send(
        &mut self,
        result: Result<R, SourceCallbackError>,
    ) -> Result<(), SourceCallbackReplyError<R>> {
        let Some(reply) = self.reply.take() else {
            return Err(SourceCallbackReplyError::AlreadySent(result));
        };
        match reply.send(result) {
            Ok(()) => {
                self.state_anchor.take();
                Ok(())
            }
            Err(result) => {
                (reply.on_lost)(result);
                self.state_anchor.take();
                Err(SourceCallbackReplyError::Disconnected)
            }
        }
    }
    /// Publish a successful callback value.
    pub fn reply(&mut self, value: R) -> Result<(), SourceCallbackReplyError<R>> {
        self.send(Ok(value))
    }

    /// Publish a transport-level failure without inventing a Source semantic
    /// failure value.
    pub fn reject(
        &mut self,
        error: SourceCallbackError,
    ) -> Result<(), SourceCallbackReplyError<R>> {
        self.send(Err(error))
    }

    /// Whether this response has already been consumed.
    pub fn is_sent(&self) -> bool {
        self.reply.is_none()
    }
}

impl<R> Drop for SourceCallbackResponder<R> {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            // A pending receiver must still be able to observe ReplyClosed;
            // its completion claim is released when that receiver consumes or
            // abandons the terminal result.
            reply.sender_drop();
        }
    }
}

enum PendingEvent<C, X, R> {
    Invoke {
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        command: C,
        context: X,
        responder: SourceCallbackResponder<R>,
    },
    Cleanup {
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        command: Option<C>,
        context: Option<X>,
        completion: Arc<InvocationCompletion>,
    },
    ReplyCleanup {
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        result: Result<R, SourceCallbackError>,
        completion: Arc<InvocationCompletion>,
    },
    Release {
        callback: SourceCallbackId,
    },
}
/// Values left in a session when its last transport owner is abandoned. The
/// origin callback must move these packets to a live Source owner for semantic
/// cleanup; the transport does not interpret or Rust-drop them as a fallback.
pub enum SourceCallbackAbandonedEvent<C, X, R> {
    Invoke {
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        command: C,
        context: X,
    },
    Cleanup {
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        command: Option<C>,
        context: Option<X>,
    },
    ReplyCleanup {
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        result: Result<R, SourceCallbackError>,
    },
}

/// Complete owned handoff for a session whose final transport owner is being
/// dropped. Its callback must route every payload/event to the per-origin
/// Source cleanup owner without retaining this session or creating a cycle.
pub struct SourceCallbackAbandonment<P, C, X, R> {
    callbacks: HashMap<SourceCallbackId, CallbackEntry<P>>,
    events: VecDeque<PendingEvent<C, X, R>>,
}

impl<P, C, X, R> SourceCallbackAbandonment<P, C, X, R> {
    pub fn with_payload<T>(
        &mut self,
        callback: SourceCallbackId,
        body: impl FnOnce(&mut P) -> T,
    ) -> Option<T> {
        let payload = self.callbacks.get_mut(&callback)?.payload.as_mut()?;
        Some(body(payload))
    }

    pub fn next_payload(&mut self) -> Option<(SourceCallbackId, P)> {
        loop {
            let callback = self.callbacks.keys().next().copied()?;
            let mut entry = self.callbacks.remove(&callback)?;
            if let Some(payload) = entry.payload.take() {
                return Some((callback, payload));
            }
        }
    }

    pub fn next_event(&mut self) -> Option<SourceCallbackAbandonedEvent<C, X, R>> {
        loop {
            match self.events.pop_front()? {
                PendingEvent::Invoke {
                    callback,
                    request,
                    command,
                    context,
                    responder,
                } => {
                    drop(responder);
                    return Some(SourceCallbackAbandonedEvent::Invoke {
                        callback,
                        request,
                        command,
                        context,
                    });
                }
                PendingEvent::Cleanup {
                    callback,
                    request,
                    command,
                    context,
                    completion: _,
                } => {
                    return Some(SourceCallbackAbandonedEvent::Cleanup {
                        callback,
                        request,
                        command,
                        context,
                    });
                }
                PendingEvent::ReplyCleanup {
                    callback,
                    request,
                    result,
                    completion: _,
                } => {
                    return Some(SourceCallbackAbandonedEvent::ReplyCleanup {
                        callback,
                        request,
                        result,
                    });
                }
                PendingEvent::Release { .. } => {}
            }
        }
    }
}




#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionPhase {
    Open,
    Retiring,
    Retired,
}

struct CallbackEntry<P> {
    payload: Option<P>,
    leases: usize,
    inflight: usize,
    borrows: usize,
    release_requested: bool,
    release_in_queue: bool,
}


/// Weak handle passed to the scheduler notification hook. Each hook invocation
/// corresponds to one queued event; the owner must schedule one `try_next` per
/// notification or explicitly drain the session. A worker can capture a handle
/// without making the callback session own that worker or itself.
pub struct SourceCallbackReadyHandle<P, C, X, R> {
    state: Weak<Mutex<CallbackState<P, C, X, R>>>,
}

impl<P, C, X, R> Clone for SourceCallbackReadyHandle<P, C, X, R> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<P, C, X, R> SourceCallbackReadyHandle<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    /// Pop and materialize exactly one ready event for an independent owner
    /// task. Returns `None` if the session was dropped or another task won.
    /// Owners must schedule one call per ready notification or drain the session.
    pub fn try_next(&self) -> Option<SourceCallbackEvent<P, C, X, R>> {
        let state = self.state.upgrade()?;
        let session = SourceCallbackSession { state };
        session.try_next()
    }
}

fn notify_ready<P, C, X, R>(state: &Arc<Mutex<CallbackState<P, C, X, R>>>) {
    let on_ready = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .on_ready
        .clone();
    on_ready(SourceCallbackReadyHandle {
        state: Arc::downgrade(state),
    });
}
struct CallbackState<P, C, X, R> {
    phase: SessionPhase,
    next_callback: Option<u64>,
    next_request: Option<u64>,
    callbacks: HashMap<SourceCallbackId, CallbackEntry<P>>,
    queue: VecDeque<PendingEvent<C, X, R>>,
    deferred: HashMap<SourceCallbackId, VecDeque<PendingEvent<C, X, R>>>,
    wake: Arc<Condvar>,
    on_ready: Arc<dyn Fn(SourceCallbackReadyHandle<P, C, X, R>) + Send + Sync>,
    on_abandoned: Arc<dyn Fn(SourceCallbackAbandonment<P, C, X, R>) + Send + Sync>,
}

impl<P, C, X, R> Drop for CallbackState<P, C, X, R> {
    fn drop(&mut self) {
        let mut events = std::mem::take(&mut self.queue);
        for mut deferred in std::mem::take(&mut self.deferred).into_values() {
            events.append(&mut deferred);
        }
        (self.on_abandoned)(SourceCallbackAbandonment {
            callbacks: std::mem::take(&mut self.callbacks),
            events,
        });
    }
}

impl<P, C, X, R> CallbackState<P, C, X, R> {
    fn status(&self) -> SourceCallbackDrainStatus {
        SourceCallbackDrainStatus {
            producers: self.callbacks.values().map(|entry| entry.leases).sum(),
            inflight: self.callbacks.values().map(|entry| entry.inflight).sum(),
            pending: self.queue.len()
                + self.deferred.values().map(VecDeque::len).sum::<usize>(),
            borrows: self.callbacks.values().map(|entry| entry.borrows).sum(),
            releases: self
                .callbacks
                .values()
                .filter(|entry| entry.release_requested)
                .count(),
        }
    }

    fn next_callback_id(&mut self) -> Result<SourceCallbackId, SourceCallbackError> {
        let current = self.next_callback.take().ok_or(SourceCallbackError::IdExhausted)?;
        self.next_callback = current.checked_add(1);
        Ok(SourceCallbackId(current))
    }

    fn next_request_id(&mut self) -> Result<SourceCallbackRequestId, SourceCallbackError> {
        let current = self.next_request.take().ok_or(SourceCallbackError::IdExhausted)?;
        self.next_request = current.checked_add(1);
        Ok(SourceCallbackRequestId(current))
    }

    fn queue_release(&mut self, callback: SourceCallbackId) -> bool {
        let should_queue = {
            let Some(entry) = self.callbacks.get_mut(&callback) else {
                return false;
            };
            if entry.release_requested
                || entry.leases != 0
                || entry.inflight != 0
                || entry.borrows != 0
                || entry.payload.is_none()
            {
                false
            } else {
                entry.release_requested = true;
                entry.release_in_queue = true;
                true
            }
        };
        if should_queue {
            self.queue.push_back(PendingEvent::Release { callback });
            self.wake.notify_one();
        }
        should_queue
    }

    fn finish_invocation(&mut self, callback: SourceCallbackId) -> bool {
        let should_release = {
            let Some(entry) = self.callbacks.get_mut(&callback) else {
                return false;
            };
            if entry.inflight != 0 {
                entry.inflight -= 1;
            }
            entry.leases == 0 && entry.inflight == 0 && entry.borrows == 0
        };
        let queued = should_release && self.queue_release(callback);
        self.wake.notify_all();
        queued
    }

    fn queue_cleanup(
        &mut self,
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        command: Option<C>,
        context: Option<X>,
        completion: Arc<InvocationCompletion>,
    ) -> bool {
        if !self.callbacks.contains_key(&callback) {
            return false;
        }
        self.queue.push_back(PendingEvent::Cleanup {
            callback,
            request,
            command,
            context,
            completion,
        });
        self.wake.notify_one();
        true
    }

    fn queue_reply_cleanup(
        &mut self,
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        result: Result<R, SourceCallbackError>,
        completion: Arc<InvocationCompletion>,
    ) -> bool {
        if !self.callbacks.contains_key(&callback) {
            return false;
        }
        self.queue.push_back(PendingEvent::ReplyCleanup {
            callback,
            request,
            result,
            completion,
        });
        self.wake.notify_one();
        true
    }
}

/// Owner-side invocation event. Dropping an unanswered event queues its still
/// owned command/context as a cleanup event rather than silently dropping them.
pub struct SourceCallbackInvocation<P, C, X, R> {
    state: Arc<Mutex<CallbackState<P, C, X, R>>>,
    callback: SourceCallbackId,
    request: SourceCallbackRequestId,
    command: Option<C>,
    context: Option<X>,
    responder: Option<SourceCallbackResponder<R>>,
    completion: Arc<InvocationCompletion>,
    deferred: bool,
}

impl<P, C, X, R> SourceCallbackInvocation<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    /// Callback payload lease selected for this invocation.
    pub fn callback_id(&self) -> SourceCallbackId {
        self.callback
    }

    /// Session-local request identity for diagnostics and correlation.
    pub fn request_id(&self) -> SourceCallbackRequestId {
        self.request
    }

    /// Borrow the queued command without taking ownership from the event.
    pub fn command(&self) -> &C {
        self.command
            .as_ref()
            .expect("Source callback command was taken once")
    }

    /// Borrow the preserved task/scope context without taking ownership.
    pub fn context(&self) -> &X {
        self.context
            .as_ref()
            .expect("Source callback context was taken once")
    }

    /// Take the command for owner-side invocation. The owner then owns it and
    /// must perform any semantic cleanup if the invocation is abandoned.
    pub fn take_command(&mut self) -> C {
        self.command
            .take()
            .expect("Source callback command was taken once")
    }

    /// Take the task/scope context for owner-side invocation.
    pub fn take_context(&mut self) -> X {
        self.context
            .take()
            .expect("Source callback context was taken once")
    }

    /// Take the reply responder for an adapter that owns response dispatch.
    pub fn take_responder(&mut self) -> Option<SourceCallbackResponder<R>> {
        self.responder.take()
    }

    /// Send one response for this invocation. If the producer disconnected,
    /// dropping this event queues any still-owned command/context for cleanup.
    pub fn respond(
        &mut self,
        result: Result<R, SourceCallbackError>,
    ) -> Result<(), SourceCallbackReplyError<R>> {
        match self.responder.as_mut() {
            Some(responder) => responder.send(result),
            None => Err(SourceCallbackReplyError::AlreadySent(result)),
        }
    }

    /// Invoke this callback while its unique retained payload is borrowed.
    ///
    /// If another owner currently has the payload, this invocation is parked
    /// atomically and requeued only when that borrow returns. In that case this
    /// method returns `PayloadBusy`; it does not notify workers or spin.
    pub fn try_with_payload_mut<T>(
        mut self,
        body: impl FnOnce(&mut P, &mut Self) -> T,
    ) -> Result<T, SourceCallbackError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.callbacks.get_mut(&self.callback) else {
            drop(state);
            return Err(SourceCallbackError::UnknownCallback);
        };
        let payload = if entry.payload.is_some() {
            entry.borrows += 1;
            entry.payload.take()
        } else {
            None
        };
        let Some(payload) = payload else {
            if self.command.is_none() || self.context.is_none() || self.responder.is_none() {
                drop(state);
                return Err(SourceCallbackError::ReplyValueBusy);
            }
            let pending = PendingEvent::Invoke {
                callback: self.callback,
                request: self.request,
                command: self.command.take().expect("deferred callback command is present"),
                context: self.context.take().expect("deferred callback context is present"),
                responder: self
                    .responder
                    .take()
                    .expect("deferred callback responder is present"),
            };
            state
                .deferred
                .entry(self.callback)
                .or_default()
                .push_back(pending);
            self.deferred = true;
            drop(state);
            return Err(SourceCallbackError::PayloadBusy);
        };
        drop(state);

        let mut root = PayloadRoot {
            state: self.state.clone(),
            callback: self.callback,
            payload: Some(payload),
        };
        let result = body(
            root.payload
                .as_mut()
                .expect("borrowed callback payload is present"),
            &mut self,
        );
        drop(root);
        Ok(result)
    }
}

impl<P, C, X, R> Drop for SourceCallbackInvocation<P, C, X, R> {
    fn drop(&mut self) {
        if self.deferred {
            return;
        }
        // Dropping the event always discharges its input side separately from
        // the reply claim. In particular, a successful response does not
        // permit still-owned command/context values to bypass Source cleanup.
        self.responder.take();
        let command = self.command.take();
        let context = self.context.take();
        if command.is_some() || context.is_some() {
            let queued = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.queue_cleanup(
                    self.callback,
                    self.request,
                    command,
                    context,
                    self.completion.clone(),
                )
            };
            if queued {
                notify_ready(&self.state);
            }
        } else {
            self.completion.inputs_done();
        }
    }
}

/// Owner-side cleanup event for a disconnected or abandoned invocation.
pub struct SourceCallbackCleanup<P, C, X, R> {
    state: Arc<Mutex<CallbackState<P, C, X, R>>>,
    callback: SourceCallbackId,
    request: SourceCallbackRequestId,
    command: Option<C>,
    context: Option<X>,
    completion: Arc<InvocationCompletion>,
    completed: bool,
}

impl<P, C, X, R> SourceCallbackCleanup<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    /// Session-local callback identity whose command needs cleanup.
    pub fn callback_id(&self) -> SourceCallbackId {
        self.callback
    }

    /// Session-local request identity for diagnostics and correlation.
    pub fn request_id(&self) -> SourceCallbackRequestId {
        self.request
    }

    /// Borrow the command still owned by this cleanup event, if the producer
    /// already took it from the original invocation then it is `None` here.
    pub fn command(&self) -> Option<&C> {
        self.command.as_ref()
    }

    /// Borrow the preserved task/scope context, if still owned by transport.
    pub fn context(&self) -> Option<&X> {
        self.context.as_ref()
    }

    /// Take the command for owner-side cleanup.
    pub fn take_command(&mut self) -> Option<C> {
        self.command.take()
    }

    /// Take the context for owner-side cleanup.
    pub fn take_context(&mut self) -> Option<X> {
        self.context.take()
    }
    /// Commit cleanup after the owner has handled any command/context values.
    pub fn complete(mut self) {
        self.completed = true;
        self.completion.inputs_done();
    }
}

impl<P, C, X, R> Drop for SourceCallbackCleanup<P, C, X, R> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let command = self.command.take();
        let context = self.context.take();
        if command.is_some() || context.is_some() {
            let queued = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.queue_cleanup(
                    self.callback,
                    self.request,
                    command,
                    context,
                    self.completion.clone(),
                )
            };
            if queued {
                notify_ready(&self.state);
            }
        } else {
            self.completion.inputs_done();
        }
    }
}
/// Owner-side cleanup event for a reply value that was published but never
/// received by its producer. The result stays in this envelope until Source
/// explicitly takes it.
pub struct SourceCallbackReplyCleanup<R> {
    callback: SourceCallbackId,
    request: SourceCallbackRequestId,
    result: Option<Result<R, SourceCallbackError>>,
    completion: Arc<InvocationCompletion>,
    on_lost: Arc<dyn Fn(Result<R, SourceCallbackError>) + Send + Sync>,
    completed: bool,
}

impl<R: Send + 'static> SourceCallbackReplyCleanup<R> {
    pub fn callback_id(&self) -> SourceCallbackId {
        self.callback
    }

    pub fn request_id(&self) -> SourceCallbackRequestId {
        self.request
    }

    pub fn result(&self) -> &Result<R, SourceCallbackError> {
        self.result
            .as_ref()
            .expect("Source callback reply cleanup result was taken once")
    }

    pub fn take_result(&mut self) -> Result<R, SourceCallbackError> {
        self.result
            .take()
            .expect("Source callback reply cleanup result was taken once")
    }

    pub fn complete(mut self) -> Result<(), SourceCallbackError> {
        if self.result.is_some() {
            return Err(SourceCallbackError::ReplyValueBusy);
        }
        self.completed = true;
        self.completion.reply_done();
        Ok(())
    }
}

impl<R: Send + 'static> Drop for SourceCallbackReplyCleanup<R> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        if let Some(result) = self.result.take() {
            (self.on_lost)(result);
        } else {
            self.completion.reply_done();
        }
    }
}


/// Owner-side release event. It owns the original registered payload exactly
/// once. The owner runs Source/Eval cleanup on `payload_mut`, then calls
/// `complete`; dropping an uncompleted event requeues that same payload.
pub struct SourceCallbackRelease<P, C, X, R> {
    state: Arc<Mutex<CallbackState<P, C, X, R>>>,
    callback: SourceCallbackId,
    payload: Option<P>,
    completed: bool,
}

impl<P, C, X, R> SourceCallbackRelease<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    /// Callback payload lease selected for release.
    pub fn callback_id(&self) -> SourceCallbackId {
        self.callback
    }

    /// Borrow the original payload without taking the session mutex.
    pub fn payload(&self) -> &P {
        self.payload
            .as_ref()
            .expect("Source callback release payload was taken once")
    }

    /// Mutably borrow the original payload for owner-side Source/Eval drop.
    pub fn payload_mut(&mut self) -> &mut P {
        self.payload
            .as_mut()
            .expect("Source callback release payload was taken once")
    }

    /// Commit owner-side payload cleanup and remove this callback from the
    /// session. The payload is dropped only after the session mutex is free.
    pub fn complete(mut self) -> Result<(), SourceCallbackError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = state.callbacks.get(&self.callback) else {
            return Err(SourceCallbackError::UnknownCallback);
        };
        if entry.leases != 0
            || entry.inflight != 0
            || entry.borrows != 0
            || !entry.release_requested
            || entry.release_in_queue
            || entry.payload.is_some()
        {
            return Err(SourceCallbackError::ReleaseNotReady);
        }
        state.callbacks.remove(&self.callback);
        state.wake.notify_all();
        drop(state);
        self.completed = true;
        // `self.payload` is dropped after the mutex guard has gone away.
        Ok(())
    }
}

impl<P, C, X, R> Drop for SourceCallbackRelease<P, C, X, R> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let Some(mut payload) = self.payload.take() else {
            return;
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut payload = Some(payload);
        let requeued = if let Some(entry) = state.callbacks.get_mut(&self.callback) {
            if entry.payload.is_none() {
                entry.payload = payload.take();
                entry.release_in_queue = true;
                true
            } else {
                false
            }
        } else {
            false
        };
        if requeued {
            state.queue.push_back(PendingEvent::Release {
                callback: self.callback,
            });
            state.wake.notify_one();
            drop(state);
            notify_ready(&self.state);
            return;
        }
        drop(state);
        drop(payload);
    }
}
/// transports generic commands, context, replies, cleanup, and final-release
/// notices.
pub enum SourceCallbackEvent<P, C, X, R> {
    Invoke(SourceCallbackInvocation<P, C, X, R>),
    Cleanup(SourceCallbackCleanup<P, C, X, R>),
    ReplyCleanup(SourceCallbackReplyCleanup<R>),
    Release(SourceCallbackRelease<P, C, X, R>),
}

/// Temporary owner-side payload root. It removes the payload from session
/// state before invoking the caller's closure, then restores it on every exit
/// path, including panic unwinding. This prevents a session mutex from being
/// held across Source semantic work and prevents an uncounted escaped borrow.
struct PayloadRoot<P, C, X, R> {
    state: Arc<Mutex<CallbackState<P, C, X, R>>>,
    callback: SourceCallbackId,
    payload: Option<P>,
}

impl<P, C, X, R> Drop for PayloadRoot<P, C, X, R> {
    fn drop(&mut self) {
        let Some(payload) = self.payload.take() else {
            return;
        };
        let mut payload = Some(payload);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (restored, should_release) =
            if let Some(entry) = state.callbacks.get_mut(&self.callback) {
                if entry.payload.is_none() {
                    entry.payload = payload.take();
                    entry.borrows = entry.borrows.saturating_sub(1);
                    (
                        true,
                        entry.leases == 0 && entry.inflight == 0 && entry.borrows == 0,
                    )
                } else {
                    (false, false)
                }
            } else {
                (false, false)
            };
        if restored {
            let mut deferred = state.deferred.remove(&self.callback).unwrap_or_default();
            let deferred_count = deferred.len();
            state.queue.append(&mut deferred);
            if deferred_count != 0 {
                state.wake.notify_all();
            }
            let queued = should_release && state.queue_release(self.callback);
            let ready_notifications = deferred_count + if queued { 1 } else { 0 };
            drop(state);
            for _ in 0..ready_notifications {
                notify_ready(&self.state);
            }
            return;
        }
        drop(state);
        drop(payload);
    }
}

/// One session-owned callback transport. Clones share only this session's
/// state; there is no process-global callback table or machine pointer.
pub struct SourceCallbackSession<P, C, X = (), R = ()> {
    state: Arc<Mutex<CallbackState<P, C, X, R>>>,
}

impl<P, C, X, R> Clone for SourceCallbackSession<P, C, X, R> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<P, C, X, R> SourceCallbackSession<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    /// Create an open, empty callback transport with an origin-owned cleanup
    /// sink for any payloads or events left when its last owner is abandoned.
    /// The sink must not retain this session or capture origin Machine state;
    /// cross-thread users should enqueue checked sendable owner packets.
    pub fn new(
        on_abandoned: impl Fn(SourceCallbackAbandonment<P, C, X, R>) + Send + Sync + 'static,
        on_ready: impl Fn(SourceCallbackReadyHandle<P, C, X, R>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(CallbackState {
                phase: SessionPhase::Open,
                next_callback: Some(1),
                next_request: Some(1),
                callbacks: HashMap::new(),
                queue: VecDeque::new(),
                deferred: HashMap::new(),
                wake: Arc::new(Condvar::new()),
                on_ready: Arc::new(on_ready),
                on_abandoned: Arc::new(on_abandoned),
            })),
        }
    }

    /// Register one retained Source callback payload and return its first
    /// counted lease alias. A rejected registration returns its original
    /// payload to the caller.
    pub fn register(
        &self,
        payload: P,
    ) -> Result<SourceCallbackLease<P, C, X, R>, SourceCallbackRegisterError<P>> {
        let mut state = self.lock_state();
        if let Err(error) = match state.phase {
            SessionPhase::Open => Ok(()),
            SessionPhase::Retiring => Err(SourceCallbackError::Retiring),
            SessionPhase::Retired => Err(SourceCallbackError::Closed),
        } {
            return Err(SourceCallbackRegisterError { error, payload });
        }
        let callback = match state.next_callback_id() {
            Ok(callback) => callback,
            Err(error) => return Err(SourceCallbackRegisterError { error, payload }),
        };
        state.callbacks.insert(
            callback,
            CallbackEntry {
                payload: Some(payload),
                leases: 1,
                inflight: 0,
                borrows: 0,
                release_requested: false,
                release_in_queue: false,
            },
        );
        Ok(SourceCallbackLease {
            state: self.state.clone(),
            callback,
            _types: std::marker::PhantomData,
        })
    }

    /// Mark the session as no longer accepting new payloads or invocations.
    /// Existing producer leases and queued work are deliberately not canceled.
    pub fn begin_retire(&self) {
        let mut state = self.lock_state();
        if state.phase == SessionPhase::Open {
            state.phase = SessionPhase::Retiring;
            state.wake.notify_all();
        }
    }

    /// Finish retirement if every producer, invocation, payload borrow, and
    /// release event has drained. Calling this too early leaves the session in
    /// Retiring so the owner can pump and try again.
    pub fn retire(&self) -> Result<(), SourceCallbackRetireError> {
        let mut state = self.lock_state();
        if state.phase == SessionPhase::Open {
            state.phase = SessionPhase::Retiring;
        }
        if state.phase == SessionPhase::Retired {
            return Ok(());
        }
        let status = state.status();
        if !status.drained() || !state.callbacks.is_empty() {
            state.wake.notify_all();
            return Err(SourceCallbackRetireError::NotDrained(status));
        }
        state.phase = SessionPhase::Retired;
        state.wake.notify_all();
        Ok(())
    }

    /// Return whether this session has completed logical retirement.
    pub fn is_retired(&self) -> bool {
        self.lock_state().phase == SessionPhase::Retired
    }

    /// Return current producer/in-flight/pending/borrow/release counts.
    pub fn drain_status(&self) -> SourceCallbackDrainStatus {
        self.lock_state().status()
    }

    /// Pop one event for the owner pump without blocking.
    pub fn try_next(&self) -> Option<SourceCallbackEvent<P, C, X, R>> {
        let pending = self.lock_state().queue.pop_front()?;
        Some(self.wrap_event(pending))
    }

    /// Wait for the next owner-pump event. Returns `None` only after logical
    /// retirement and complete queue drain.
    pub fn next(&self) -> Option<SourceCallbackEvent<P, C, X, R>> {
        let mut state = self.lock_state();
        loop {
            if let Some(pending) = state.queue.pop_front() {
                drop(state);
                return Some(self.wrap_event(pending));
            }
            if state.phase == SessionPhase::Retired {
                return None;
            }
            state = state
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Read the retained payload without keeping the session mutex held while
    /// the callback runs. The temporary root returns before this method exits.
    pub fn with_payload<T>(
        &self,
        callback: SourceCallbackId,
        body: impl FnOnce(&P) -> T,
    ) -> Result<T, SourceCallbackError> {
        let mut root = self.take_payload_root(callback)?;
        let result = body(root.payload.as_ref().expect("payload root is present"));
        drop(root);
        Ok(result)
    }

    /// Mutably borrow the retained payload without holding the session mutex
    /// across Source/Eval work.
    pub fn with_payload_mut<T>(
        &self,
        callback: SourceCallbackId,
        body: impl FnOnce(&mut P) -> T,
    ) -> Result<T, SourceCallbackError> {
        let mut root = self.take_payload_root(callback)?;
        let result = body(root.payload.as_mut().expect("payload root is present"));
        drop(root);
        Ok(result)
    }

    fn take_payload_root(
        &self,
        callback: SourceCallbackId,
    ) -> Result<PayloadRoot<P, C, X, R>, SourceCallbackError> {
        let mut state = self.lock_state();
        let entry = state
            .callbacks
            .get_mut(&callback)
            .ok_or(SourceCallbackError::UnknownCallback)?;
        let payload = entry.payload.take().ok_or(SourceCallbackError::PayloadBusy)?;
        entry.borrows += 1;
        drop(state);
        Ok(PayloadRoot {
            state: self.state.clone(),
            callback,
            payload: Some(payload),
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, CallbackState<P, C, X, R>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wrap_event(&self, pending: PendingEvent<C, X, R>) -> SourceCallbackEvent<P, C, X, R> {
        match pending {
            PendingEvent::Invoke {
                callback,
                request,
                command,
                context,
                responder,
            } => {
                let mut responder = responder;
                responder.state_anchor = Some(self.state.clone());
                let completion = responder.completion.clone();
                SourceCallbackEvent::Invoke(SourceCallbackInvocation {
                    state: self.state.clone(),
                    callback,
                    request,
                    command: Some(command),
                    context: Some(context),
                    responder: Some(responder),
                    completion,
                    deferred: false,
                })
            },
            PendingEvent::Cleanup {
                callback,
                request,
                command,
                context,
                completion,
            } => SourceCallbackEvent::Cleanup(SourceCallbackCleanup {
                state: self.state.clone(),
                callback,
                request,
                command,
                context,
                completion,
                completed: false,
            }),
            PendingEvent::ReplyCleanup {
                callback,
                request,
                result,
                completion,
            } => {
                let state = self.state.clone();
                let requeue_completion = completion.clone();
                let on_lost: Arc<dyn Fn(Result<R, SourceCallbackError>) + Send + Sync> =
                    Arc::new(move |result| {
                        let queued = {
                            let mut state = state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            state.queue_reply_cleanup(
                                callback,
                                request,
                                result,
                                requeue_completion.clone(),
                            )
                        };
                        if queued {
                            notify_ready(&state);
                        }
                    });
                SourceCallbackEvent::ReplyCleanup(SourceCallbackReplyCleanup {
                    callback,
                    request,
                    result: Some(result),
                    completion,
                    on_lost,
                    completed: false,
                })
            }
            PendingEvent::Release { callback } => {
                let mut state = self.lock_state();
                let entry = state
                    .callbacks
                    .get_mut(&callback)
                    .expect("Source callback release entry disappeared");
                entry.release_in_queue = false;
                let payload = entry
                    .payload
                    .take()
                    .expect("Source callback release payload disappeared");
                drop(state);
                SourceCallbackEvent::Release(SourceCallbackRelease {
                    state: self.state.clone(),
                    callback,
                    payload: Some(payload),
                    completed: false,
                })
            }
        }
    }
}


/// Counted alias to one session-retained callback payload. Cloning this lease
/// explicitly increments the transport's producer count; the payload itself
/// is never cloned. The final drop queues owner-side Source cleanup.
pub struct SourceCallbackLease<P, C, X = (), R = ()> {
    state: Arc<Mutex<CallbackState<P, C, X, R>>>,
    callback: SourceCallbackId,
    _types: std::marker::PhantomData<fn(C, X, R)>,
}

impl<P, C, X, R> Clone for SourceCallbackLease<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    fn clone(&self) -> Self {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state
            .callbacks
            .get_mut(&self.callback)
            .expect("Source callback lease cloned after release");
        assert!(
            !entry.release_requested,
            "Source callback lease cloned after release was queued"
        );
        entry.leases += 1;
        drop(state);
        Self {
            state: self.state.clone(),
            callback: self.callback,
            _types: std::marker::PhantomData,
        }
    }
}

impl<P, C, X, R> SourceCallbackLease<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    /// Session-local payload identity for owner-side payload access.
    pub fn callback_id(&self) -> SourceCallbackId {
        self.callback
    }

    /// Queue one invocation using this counted producer lease. Rejection
    /// returns the original command/context to the caller.
    pub fn enqueue(
        &self,
        command: C,
        context: X,
    ) -> Result<SourceCallbackReply<R>, SourceCallbackEnqueueError<C, X>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let error = match state.phase {
            SessionPhase::Open => None,
            SessionPhase::Retiring => Some(SourceCallbackError::Retiring),
            SessionPhase::Retired => Some(SourceCallbackError::Closed),
        };
        if let Some(error) = error {
            return Err(SourceCallbackEnqueueError {
                error,
                command,
                context,
            });
        }
        let request = match state.next_request_id() {
            Ok(request) => request,
            Err(error) => {
                return Err(SourceCallbackEnqueueError {
                    error,
                    command,
                    context,
                })
            }
        };
        let Some(entry) = state.callbacks.get_mut(&self.callback) else {
            return Err(SourceCallbackEnqueueError {
                error: SourceCallbackError::UnknownCallback,
                command,
                context,
            });
        };
        if entry.release_requested {
            return Err(SourceCallbackEnqueueError {
                error: SourceCallbackError::UnknownCallback,
                command,
                context,
            });
        }
        let callback = self.callback;
        let completion_state = Arc::downgrade(&self.state);
        let completion = Arc::new(InvocationCompletion {
            inputs_done: std::sync::atomic::AtomicBool::new(false),
            reply_done: std::sync::atomic::AtomicBool::new(false),
            finished: std::sync::atomic::AtomicBool::new(false),
            finish: Box::new(move || {
                if let Some(completion_state) = completion_state.upgrade() {
                    let queued = {
                        let mut state = completion_state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        state.finish_invocation(callback)
                    };
                    if queued {
                        notify_ready(&completion_state);
                    }
                }
            }),
        });
        let lost_state = Arc::downgrade(&self.state);
        let lost_completion = completion.clone();
        let lost_callback = self.callback;
        let lost_request = request;
        let on_lost: Arc<dyn Fn(Result<R, SourceCallbackError>) + Send + Sync> =
            Arc::new(move |result| {
                if let Some(lost_state) = lost_state.upgrade() {
                    let queued = {
                        let mut state = lost_state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        state.queue_reply_cleanup(
                            lost_callback,
                            lost_request,
                            result,
                            lost_completion.clone(),
                        )
                    };
                    if queued {
                        notify_ready(&lost_state);
                    }
                }
            });
        let publish_state = Arc::downgrade(&self.state);
        let on_publish: Arc<
            dyn Fn() -> Option<Arc<dyn std::any::Any + Send + Sync>> + Send + Sync,
        > = Arc::new(move || {
            publish_state
                .upgrade()
                .map(|state| state as Arc<dyn std::any::Any + Send + Sync>)
        });
        let cell = Arc::new(ReplyCell {
            state: Mutex::new(ReplyCellState::Pending),
            wake: Condvar::new(),
            completion: completion.clone(),
            state_anchor: Mutex::new(None),
            on_publish,
            on_lost,
        });
        entry.inflight += 1;
        state.queue.push_back(PendingEvent::Invoke {
            callback: self.callback,
            request,
            command,
            context,
            responder: SourceCallbackResponder {
                reply: Some(cell.clone()),
                completion,
                state_anchor: None,
            },
        });
        state.wake.notify_one();
        drop(state);
        notify_ready(&self.state);
        Ok(SourceCallbackReply { cell })
    }

    /// Queue an already-owned command/context for owner-side cleanup after a
    /// producer rejected an invocation. This route intentionally works while
    /// the session is retiring and does not consume the invocation request-ID
    /// space; request ID zero marks this cleanup-only obligation.
    pub fn queue_rejected_cleanup(
        &self,
        command: C,
        context: X,
    ) -> Result<(), SourceCallbackEnqueueError<C, X>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let error = match state.phase {
            SessionPhase::Retired => Some(SourceCallbackError::Closed),
            SessionPhase::Open | SessionPhase::Retiring => None,
        };
        if let Some(error) = error {
            return Err(SourceCallbackEnqueueError {
                error,
                command,
                context,
            });
        }
        let Some(entry) = state.callbacks.get_mut(&self.callback) else {
            return Err(SourceCallbackEnqueueError {
                error: SourceCallbackError::UnknownCallback,
                command,
                context,
            });
        };
        if entry.release_requested {
            return Err(SourceCallbackEnqueueError {
                error: SourceCallbackError::UnknownCallback,
                command,
                context,
            });
        }
        let callback = self.callback;
        let completion_state = Arc::downgrade(&self.state);
        let completion = Arc::new(InvocationCompletion {
            inputs_done: std::sync::atomic::AtomicBool::new(false),
            reply_done: std::sync::atomic::AtomicBool::new(true),
            finished: std::sync::atomic::AtomicBool::new(false),
            finish: Box::new(move || {
                if let Some(completion_state) = completion_state.upgrade() {
                    let queued = {
                        let mut state = completion_state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        state.finish_invocation(callback)
                    };
                    if queued {
                        notify_ready(&completion_state);
                    }
                }
            }),
        });
        entry.inflight += 1;
        state.queue.push_back(PendingEvent::Cleanup {
            callback,
            request: SourceCallbackRequestId(0),
            command: Some(command),
            context: Some(context),
            completion,
        });
        state.wake.notify_one();
        drop(state);
        notify_ready(&self.state);
        Ok(())
    }

    /// Read the retained payload without keeping the session mutex held while
    /// the callback runs.
    pub fn with_payload<T>(
        &self,
        body: impl FnOnce(&P) -> T,
    ) -> Result<T, SourceCallbackError> {
        let mut root = self.take_payload_root()?;
        let result = body(root.payload.as_ref().expect("payload root is present"));
        drop(root);
        Ok(result)
    }

    /// Mutably borrow the retained payload without holding the session mutex
    /// across Source/Eval work.
    pub fn with_payload_mut<T>(
        &self,
        body: impl FnOnce(&mut P) -> T,
    ) -> Result<T, SourceCallbackError> {
        let mut root = self.take_payload_root()?;
        let result = body(root.payload.as_mut().expect("payload root is present"));
        drop(root);
        Ok(result)
    }

    fn take_payload_root(&self) -> Result<PayloadRoot<P, C, X, R>, SourceCallbackError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state
            .callbacks
            .get_mut(&self.callback)
            .ok_or(SourceCallbackError::UnknownCallback)?;
        let payload = entry.payload.take().ok_or(SourceCallbackError::PayloadBusy)?;
        entry.borrows += 1;
        drop(state);
        Ok(PayloadRoot {
            state: self.state.clone(),
            callback: self.callback,
            payload: Some(payload),
        })
    }
}

impl<P, C, X, R> Drop for SourceCallbackLease<P, C, X, R> {
    fn drop(&mut self) {
        let queued = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(entry) = state.callbacks.get_mut(&self.callback) else {
                return;
            };
            if entry.leases != 0 {
                entry.leases -= 1;
            }
            let should_release = entry.leases == 0 && entry.inflight == 0 && entry.borrows == 0;
            let queued = should_release && state.queue_release(self.callback);
            state.wake.notify_all();
            queued
        };
        if queued {
            notify_ready(&self.state);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct DropProbe {
        drops: Arc<AtomicUsize>,
        on_drop: Option<Arc<dyn Fn() + Send + Sync>>,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            if let Some(on_drop) = self.on_drop.take() {
                on_drop();
            }
        }
    }

    fn take_event<P, C, X, R>(
        session: &SourceCallbackSession<P, C, X, R>,
    ) -> SourceCallbackEvent<P, C, X, R>
    where
        P: Send + 'static,
        C: Send + 'static,
        X: Send + 'static,
        R: Send + 'static,
    {
        match session.try_next() {
            Some(event) => event,
            None => panic!("expected a Source callback event"),
        }
    }

    fn test_session<P, C, X, R>() -> SourceCallbackSession<P, C, X, R>
    where
        P: Send + 'static,
        C: Send + 'static,
        X: Send + 'static,
        R: Send + 'static,
    {
        SourceCallbackSession::new(
            |mut abandoned| {
                let mut had_values = false;
                while abandoned.next_event().is_some() {
                    had_values = true;
                }
                while let Some((_, payload)) = abandoned.next_payload() {
                    had_values = true;
                    drop(payload);
                }
                assert!(!had_values, "test left callback ownership abandoned");
            },
            |_| {},
        )
    }

    #[test]
    fn ready_hook_notifies_new_invocation_cleanup_and_release_events() {
        let notifications = Arc::new(AtomicUsize::new(0));
        let sink = notifications.clone();
        let session: SourceCallbackSession<(), (), (), ()> = SourceCallbackSession::new(
            |mut abandoned| {
                assert!(abandoned.next_event().is_none());
                assert!(abandoned.next_payload().is_none());
            },
            move |_ready| {
                sink.fetch_add(1, Ordering::SeqCst);
            },
        );
        let lease = session.register(()).expect("register callback payload");
        let reply = lease.enqueue((), ()).expect("enqueue callback");
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected queued invocation"),
        };
        invocation.respond(Ok(())).expect("publish callback result");
        drop(invocation);
        assert_eq!(notifications.load(Ordering::SeqCst), 2);
        reply.recv().expect("receive callback result");
        match take_event(&session) {
            SourceCallbackEvent::Cleanup(cleanup) => cleanup.complete(),
            _ => panic!("dropping invocation must queue its input cleanup"),
        }
        drop(lease);
        assert_eq!(notifications.load(Ordering::SeqCst), 3);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("complete payload release");
            }
            _ => panic!("final lease drop must queue payload release"),
        }
        session.begin_retire();
        session.retire().expect("retire drained callback session");
    }

    #[test]
    fn helper_execution_failure_reply_preserves_detail_and_drains_inputs() {
        let session: SourceCallbackSession<(), (), (), String> = test_session();
        let lease = session.register(()).expect("register callback");
        let reply = lease.enqueue((), ()).expect("enqueue callback");
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected queued invocation"),
        };
        let failure = SourceCallbackError::ExecutionFailed {
            detail: "helper result decode failed".to_string(),
        };
        invocation
            .respond(Err(failure.clone()))
            .expect("publish helper execution failure");
        assert_eq!(reply.recv(), Err(failure.clone()));
        assert_eq!(session.drain_status().inflight, 1);
        drop(invocation);

        match take_event(&session) {
            SourceCallbackEvent::Cleanup(mut cleanup) => cleanup.complete(),
            _ => panic!("invocation inputs require Source cleanup"),
        }
        assert_eq!(session.drain_status().inflight, 0);
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("release callback payload");
            }
            _ => panic!("final lease drop must queue payload release"),
        }
        session.begin_retire();
        session.retire().expect("retire drained callback session");
    }

    #[test]
    fn contended_invocation_defers_until_payload_release_without_blocking_other_callbacks() {
        let notifications = Arc::new(AtomicUsize::new(0));
        let ready_handles = Arc::new(Mutex::new(VecDeque::new()));
        let count_sink = notifications.clone();
        let ready_sink = ready_handles.clone();
        let session: SourceCallbackSession<String, String, String, String> =
            SourceCallbackSession::new(
                |mut abandoned| {
                    assert!(abandoned.next_event().is_none());
                    assert!(abandoned.next_payload().is_none());
                },
                move |ready| {
                    count_sink.fetch_add(1, Ordering::SeqCst);
                    ready_sink
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push_back(ready);
                },
            );
        let next_ready = || {
            let ready = ready_handles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front()
                .expect("one ready notification per event");
            ready.try_next().expect("ready handle owns a queued event")
        };
        let lease_a = session.register("callback-a".to_string()).expect("register A");
        let callback_a = lease_a.callback_id();
        let lease_b = session.register("callback-b".to_string()).expect("register B");
        let reply_a1 = lease_a
            .enqueue("a-first".to_string(), "scope".to_string())
            .expect("enqueue first A invocation");
        let reply_a2 = lease_a
            .enqueue("a-second".to_string(), "scope".to_string())
            .expect("enqueue second A invocation");
        let reply_b = lease_b
            .enqueue("b-only".to_string(), "scope".to_string())
            .expect("enqueue B invocation");
        assert_eq!(notifications.load(Ordering::SeqCst), 3);

        let invocation_a1 = match next_ready() {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected first A invocation"),
        };
        let invocation_a2 = match next_ready() {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected second A invocation"),
        };
        let invocation_b = match next_ready() {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected B invocation"),
        };

        session
            .with_payload_mut(callback_a, |payload| {
                assert_eq!(payload, "callback-a");
                assert_eq!(
                    invocation_a1.try_with_payload_mut::<()>(|_, _| panic!("busy callback ran")),
                    Err(SourceCallbackError::PayloadBusy)
                );
                assert_eq!(notifications.load(Ordering::SeqCst), 3);
                assert_eq!(session.drain_status().pending, 1);

                invocation_b
                    .try_with_payload_mut(|payload, invocation| {
                        assert_eq!(payload, "callback-b");
                        invocation
                            .respond(Ok("b-result".to_string()))
                            .expect("respond through independent callback");
                    })
                    .expect("different callback payload remains available");
                assert_eq!(reply_b.recv().expect("receive B result"), "b-result");
                match next_ready() {
                    SourceCallbackEvent::Cleanup(mut cleanup) => cleanup.complete(),
                    _ => panic!("B invocation requires input cleanup"),
                }

                assert_eq!(
                    invocation_a2.try_with_payload_mut::<()>(|_, _| panic!("busy callback ran")),
                    Err(SourceCallbackError::PayloadBusy)
                );
                assert_eq!(notifications.load(Ordering::SeqCst), 4);
                assert_eq!(session.drain_status().pending, 2);
            })
            .expect("borrow A payload");
        assert_eq!(notifications.load(Ordering::SeqCst), 6);
        assert_eq!(
            ready_handles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            2
        );

        let invocation_a1 = match next_ready() {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("payload release must requeue first A invocation"),
        };
        invocation_a1
            .try_with_payload_mut(|payload, invocation| {
                assert_eq!(payload, "callback-a");
                invocation
                    .respond(Ok(invocation.command().clone()))
                    .expect("respond through first A invocation");
            })
            .expect("first deferred A invocation");
        assert_eq!(reply_a1.recv().expect("receive first A result"), "a-first");

        let invocation_a2 = match next_ready() {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("payload release must requeue second A invocation"),
        };
        invocation_a2
            .try_with_payload_mut(|payload, invocation| {
                assert_eq!(payload, "callback-a");
                invocation
                    .respond(Ok(invocation.command().clone()))
                    .expect("respond through second A invocation");
            })
            .expect("second deferred A invocation");
        assert_eq!(reply_a2.recv().expect("receive second A result"), "a-second");

        for _ in 0..2 {
            match next_ready() {
                SourceCallbackEvent::Cleanup(mut cleanup) => cleanup.complete(),
                _ => panic!("A invocations require input cleanup"),
            }
        }
        drop(lease_a);
        drop(lease_b);
        for _ in 0..2 {
            match next_ready() {
                SourceCallbackEvent::Release(release) => {
                    release.complete().expect("release callback payload");
                }
                _ => panic!("final callback lease drop must queue release"),
            }
        }
        session.begin_retire();
        session.retire().expect("retire drained callback session");
    }

    #[test]
    fn never_invoked_lease_releases_original_payload_once() {
        let drops = Arc::new(AtomicUsize::new(0));
        let session: SourceCallbackSession<DropProbe, (), (), ()> =
            test_session();
        let lease = match session.register(DropProbe {
            drops: drops.clone(),
            on_drop: None,
        }) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        let callback = lease.callback_id();

        session.begin_retire();
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(mut release) => {
                assert_eq!(release.callback_id(), callback);
                assert_eq!(release.payload().drops.load(Ordering::SeqCst), 0);
                assert!(release.complete().is_ok());
            }
            _ => panic!("lease drop must enqueue a release event"),
        }

        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(session.retire().is_ok());
    }

    #[test]
    fn queued_invocation_and_payload_root_defer_release_and_retirement() {
        let drops = Arc::new(AtomicUsize::new(0));
        let session: SourceCallbackSession<DropProbe, i32, String, ()> =
            test_session();
        let lease = match session.register(DropProbe {
            drops: drops.clone(),
            on_drop: None,
        }) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        let callback = lease.callback_id();
        let reply = match lease.enqueue(7, "task".to_string()) {
            Ok(reply) => reply,
            Err(_) => panic!("enqueue unexpectedly failed"),
        };

        session.begin_retire();
        drop(lease);
        assert_eq!(session.drain_status().inflight, 1);
        session
            .with_payload(callback, |_payload| {
                assert_eq!(session.drain_status().borrows, 1);
                assert!(session.retire().is_err());

                match take_event(&session) {
                    SourceCallbackEvent::Invoke(invocation) => drop(invocation),
                    _ => panic!("queued invocation must be pumped first"),
                }
                match take_event(&session) {
                    SourceCallbackEvent::Cleanup(mut cleanup) => cleanup.complete(),
                    _ => panic!("dropped invocation must enqueue cleanup"),
                }
                assert_eq!(reply.try_recv(), Err(SourceCallbackError::ReplyClosed));
                assert_eq!(session.drain_status().inflight, 0);
                assert_eq!(session.drain_status().borrows, 1);
            })
            .expect("payload root borrow unexpectedly failed");

        assert_eq!(session.drain_status().releases, 1);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                assert!(release.complete().is_ok());
            }
            _ => panic!("payload/root drain must enqueue release"),
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(session.retire().is_ok());
    }

    #[test]
    fn dropped_invocation_queues_owned_command_and_context_cleanup() {
        let session: SourceCallbackSession<(), String, String, ()> =
            test_session();
        let lease = match session.register(()) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        let reply = match lease.enqueue("command".to_string(), "scope".to_string()) {
            Ok(reply) => reply,
            Err(_) => panic!("enqueue unexpectedly failed"),
        };
        let invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected queued invocation"),
        };
        drop(reply);
        drop(invocation);

        match take_event(&session) {
            SourceCallbackEvent::Cleanup(mut cleanup) => {
                assert_eq!(cleanup.take_command(), Some("command".to_string()));
                assert_eq!(cleanup.take_context(), Some("scope".to_string()));
                cleanup.complete();
            }
            _ => panic!("dropped invocation must preserve cleanup ownership"),
        }
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => assert!(release.complete().is_ok()),
            _ => panic!("final lease must enqueue release"),
        }
    }

    #[test]
    fn delivered_reply_receiver_drop_queues_owned_result_cleanup() {
        let session: SourceCallbackSession<(), String, String, String> =
            test_session();
        let lease = session.register(()).expect("register callback");
        let reply = lease
            .enqueue("command".to_string(), "scope".to_string())
            .expect("enqueue callback");
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected invocation"),
        };
        invocation
            .respond(Ok("return".to_string()))
            .expect("publish callback result");
        drop(invocation);
        drop(reply);

        match take_event(&session) {
            SourceCallbackEvent::Cleanup(mut cleanup) => {
                assert_eq!(cleanup.take_command(), Some("command".to_string()));
                assert_eq!(cleanup.take_context(), Some("scope".to_string()));
                cleanup.complete();
            }
            _ => panic!("reply must preserve invocation input cleanup"),
        }
        match take_event(&session) {
            SourceCallbackEvent::ReplyCleanup(mut cleanup) => {
                assert_eq!(cleanup.take_result(), Ok("return".to_string()));
                cleanup.complete().expect("complete reply cleanup");
            }
            _ => panic!("dropped reply must enqueue result cleanup"),
        }
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("complete callback release");
            }
            _ => panic!("callback release must wait for reply cleanup"),
        }
        assert!(session.retire().is_ok());
    }

    #[test]
    fn detached_responder_retains_invocation_until_reply_is_received() {
        let session: SourceCallbackSession<(), String, String, String> =
            test_session();
        let lease = session.register(()).expect("register callback");
        let reply = lease
            .enqueue("command".to_string(), "scope".to_string())
            .expect("enqueue callback");
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected invocation"),
        };
        let mut responder = invocation.take_responder().expect("detach responder");
        drop(invocation);
        match take_event(&session) {
            SourceCallbackEvent::Cleanup(mut cleanup) => cleanup.complete(),
            _ => panic!("detached invocation must preserve input cleanup"),
        }
        assert_eq!(session.drain_status().inflight, 1);
        responder.reply("return".to_string()).expect("send detached reply");
        assert_eq!(reply.recv().expect("receive detached reply"), "return");
        assert_eq!(session.drain_status().inflight, 0);
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("complete callback release");
            }
            _ => panic!("release must wait for detached responder"),
        }
        assert!(session.retire().is_ok());
    }

    #[test]
    fn registration_enqueue_and_reply_failures_return_owned_values() {
        let session: SourceCallbackSession<String, String, String, String> =
            test_session();
        session.begin_retire();
        match session.register("payload".to_string()) {
            Err(error) => {
                assert_eq!(error.error, SourceCallbackError::Retiring);
                assert_eq!(error.payload, "payload");
            }
            Ok(_) => panic!("retiring session accepted a payload"),
        }

        let session: SourceCallbackSession<(), String, String, String> =
            test_session();
        let lease = match session.register(()) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        session.begin_retire();
        match lease.enqueue("command".to_string(), "scope".to_string()) {
            Err(error) => {
                assert_eq!(error.error, SourceCallbackError::Retiring);
                assert_eq!(error.command, "command");
                assert_eq!(error.context, "scope");
            }
            Ok(_) => panic!("retiring session accepted an invocation"),
        }
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => assert!(release.complete().is_ok()),
            _ => panic!("lease drop must enqueue release"),
        }

        assert!(session.retire().is_ok());

        let session: SourceCallbackSession<(), String, String, String> =
            test_session();
        let lease = match session.register(()) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        let reply = match lease.enqueue("command".to_string(), "scope".to_string()) {
            Ok(reply) => reply,
            Err(_) => panic!("enqueue unexpectedly failed"),
        };
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected queued invocation"),
        };
        drop(reply);
        assert!(matches!(
            invocation.respond(Ok("return".to_string())),
            Err(SourceCallbackReplyError::Disconnected)
        ));
        drop(invocation.take_command());
        drop(invocation.take_context());
        drop(invocation);
        drop(lease);
        assert_eq!(session.drain_status().inflight, 1);
        assert_eq!(session.drain_status().releases, 0);
        match take_event(&session) {
            SourceCallbackEvent::ReplyCleanup(mut cleanup) => {
                assert_eq!(cleanup.take_result(), Ok("return".to_string()));
                cleanup.complete().expect("complete disconnected reply cleanup");
            }
            _ => panic!("disconnected reply must queue owner cleanup"),
        }
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("complete callback release");
            }
            _ => panic!("reply cleanup must precede final callback release"),
        }
        assert!(session.retire().is_ok());
    }

    #[test]
    fn already_sent_response_returns_the_rejected_owned_value() {
        let session: SourceCallbackSession<(), String, String, String> = test_session();
        let lease = session.register(()).expect("register callback");
        let reply = lease
            .enqueue("command".to_string(), "scope".to_string())
            .expect("enqueue callback");
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected invocation"),
        };
        invocation
            .respond(Ok("accepted".to_string()))
            .expect("publish first response");
        let rejected = match invocation.respond(Ok("rejected".to_string())) {
            Err(SourceCallbackReplyError::AlreadySent(Ok(value))) => value,
            _ => panic!("already-sent response did not return its value"),
        };
        assert_eq!(rejected, "rejected");
        drop(rejected);
        drop(invocation.take_command());
        drop(invocation.take_context());
        drop(invocation);
        drop(lease);
        assert_eq!(session.drain_status().inflight, 1);
        assert_eq!(reply.recv().expect("receive first response"), "accepted");
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("complete callback release");
            }
            _ => panic!("reply completion must precede final callback release"),
        }
        assert!(session.retire().is_ok());
    }

    #[test]
    fn abandoned_session_hands_off_unpumped_values_without_a_state_cycle() {
        let handed_off = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = handed_off.clone();
        let session = SourceCallbackSession::<String, String, String, String>::new(
            move |mut abandoned| {
                let mut packets = Vec::new();
                while let Some(event) = abandoned.next_event() {
                    match event {
                        SourceCallbackAbandonedEvent::Invoke {
                            callback,
                            request,
                            command,
                            context,
                        } => {
                            let payload = abandoned
                                .with_payload(callback, |payload| payload.clone())
                                .expect("invocation payload");
                            packets.push(format!(
                                "invoke:{}:{}:{command}:{context}:{payload}",
                                callback.get(),
                                request.get()
                            ));
                        }
                        SourceCallbackAbandonedEvent::Cleanup {
                            command, context, ..
                        } => packets.push(format!("cleanup:{command:?}:{context:?}")),
                        SourceCallbackAbandonedEvent::ReplyCleanup { result, .. } => {
                            packets.push(format!("reply:{result:?}"));
                        }
                    }
                }
                while let Some((callback, payload)) = abandoned.next_payload() {
                    packets.push(format!("payload:{}:{payload}", callback.get()));
                }
                *sink.lock().expect("abandonment sink lock") = packets;
            },
        );
        let lease = session.register("payload".to_string()).expect("register callback");
        let reply = lease
            .enqueue("command".to_string(), "scope".to_string())
            .expect("enqueue invocation");

        drop(reply);
        drop(lease);
        drop(session);

        assert_eq!(
            *handed_off.lock().expect("abandonment sink lock"),
            vec!["invoke:1:1:command:scope:payload", "payload:1:payload"]
        );
    }

    #[test]
    fn rejected_enqueue_can_be_handed_to_owner_cleanup_while_retiring() {
        let session: SourceCallbackSession<(), String, String, ()> =
            test_session();
        let lease = session.register(()).expect("register callback");
        session.begin_retire();
        lease
            .queue_rejected_cleanup("command".to_string(), "scope".to_string())
            .expect("queue rejected owned invocation");
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Cleanup(mut cleanup) => {
                assert_eq!(cleanup.request_id().get(), 0);
                assert_eq!(cleanup.take_command(), Some("command".to_string()));
                assert_eq!(cleanup.take_context(), Some("scope".to_string()));
                cleanup.complete();
            }
            _ => panic!("rejected invocation must preserve cleanup ownership"),
        }
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => {
                release.complete().expect("complete callback release");
            }
            _ => panic!("cleanup must drain before callback release"),
        }
        assert!(session.retire().is_ok());
    }

    #[test]
    fn payload_and_release_cleanup_reenter_without_session_mutex() {
        let drops = Arc::new(AtomicUsize::new(0));
        let reentered = Arc::new(AtomicUsize::new(0));
        let session: SourceCallbackSession<DropProbe, (), (), ()> =
            test_session();
        let callback_session = session.clone();
        let callback_reentered = reentered.clone();
        let on_drop: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = callback_session.drain_status();
            callback_reentered.fetch_add(1, Ordering::SeqCst);
        });
        let lease = match session.register(DropProbe {
            drops: drops.clone(),
            on_drop: Some(on_drop),
        }) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        let callback = lease.callback_id();

        let borrows = session
            .with_payload_mut(callback, |_payload| session.drain_status().borrows)
            .expect("reentrant payload callback unexpectedly failed");
        assert_eq!(borrows, 1);

        session.begin_retire();
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => assert!(release.complete().is_ok()),
            _ => panic!("final lease must enqueue release"),
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(reentered.load(Ordering::SeqCst), 1);
        assert!(session.retire().is_ok());
    }

    #[test]
    fn callback_and_request_ids_fail_instead_of_wrapping() {
        let session: SourceCallbackSession<(), (), (), ()> =
            test_session();
        {
            let mut state = session.lock_state();
            state.next_callback = Some(u64::MAX);
        }
        let lease = match session.register(()) {
            Ok(lease) => lease,
            Err(_) => panic!("maximum callback identity should still be usable"),
        };
        assert_eq!(lease.callback_id().get(), u64::MAX);
        match session.register(()) {
            Err(error) => assert_eq!(error.error, SourceCallbackError::IdExhausted),
            Ok(_) => panic!("callback identity wrapped after exhaustion"),
        }
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => assert!(release.complete().is_ok()),
            _ => panic!("lease drop must enqueue release"),
        }

        let session: SourceCallbackSession<(), (), (), ()> =
            test_session();
        let lease = match session.register(()) {
            Ok(lease) => lease,
            Err(_) => panic!("registration unexpectedly failed"),
        };
        {
            let mut state = session.lock_state();
            state.next_request = Some(u64::MAX);
        }
        let reply = match lease.enqueue((), ()) {
            Ok(reply) => reply,
            Err(_) => panic!("maximum request identity should still be usable"),
        };
        let mut invocation = match take_event(&session) {
            SourceCallbackEvent::Invoke(invocation) => invocation,
            _ => panic!("expected queued invocation"),
        };
        assert_eq!(invocation.request_id().get(), u64::MAX);
        match lease.enqueue((), ()) {
            Err(error) => {
                assert_eq!(error.error, SourceCallbackError::IdExhausted);
                assert_eq!(error.command, ());
                assert_eq!(error.context, ());
            }
            Ok(_) => panic!("request identity wrapped after exhaustion"),
        }
        assert!(invocation.respond(Ok(())).is_ok());
        drop(invocation);
        drop(reply);
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => assert!(release.complete().is_ok()),
            _ => panic!("lease drop must enqueue release"),
        }
    }
}

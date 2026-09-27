//! Per-session transport for retained Source callback invocations.
//!
//! This module deliberately does not know how a callback is evaluated. A
//! request owns only a command, task context, and one-shot reply channel; the
//! session owner pumps requests and uses its own retained Source/Eval graph to
//! execute them. Payloads and identities are session-local: there is no
//! process-wide callback table and no machine pointer in this transport.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

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

/// Transport failures. Evaluation and task policy stay outside this enum;
/// callers carry those outcomes through the generic reply value or context.
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
    /// The retained payload mutex was poisoned by its owner thread.
    PayloadPoisoned,
    /// The session-local checked identity space is exhausted.
    IdExhausted,
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
            Self::PayloadPoisoned => f.write_str("Source callback payload lock is poisoned"),
            Self::IdExhausted => f.write_str("Source callback identity space is exhausted"),
        }
    }
}

impl std::error::Error for SourceCallbackError {}

/// Failure while sending a one-shot invocation response. A disconnected
/// receiver returns the exact owned result envelope so the owner can perform
/// Source-level cleanup instead of relying on Rust `Drop`.
#[derive(Debug, Eq, PartialEq)]
pub enum SourceCallbackReplyError<R> {
    /// The callback owner already sent one response.
    AlreadySent,
    /// The native producer stopped waiting. The contained result remains
    /// owned by the pump.
    Disconnected(Result<R, SourceCallbackError>),
}

impl<R> fmt::Display for SourceCallbackReplyError<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadySent => f.write_str("Source callback reply was already sent"),
            Self::Disconnected(_) => {
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

/// Shared one-shot result state.  `on_lost` owns the only path that queues a
/// successful-but-unreceived `R` for Source cleanup.
struct ReplyCell<R> {
    state: Mutex<ReplyCellState<R>>,
    wake: Condvar,
    completion: Arc<InvocationCompletion>,
    on_lost: Arc<dyn Fn(Result<R, SourceCallbackError>) + Send + Sync>,
}

impl<R: Send + 'static> ReplyCell<R> {
    fn send(
        &self,
        result: Result<R, SourceCallbackError>,
    ) -> Result<(), Result<R, SourceCallbackError>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *state {
            ReplyCellState::Pending => {
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
        if matches!(&*state, ReplyCellState::Pending) {
            *state = ReplyCellState::Ready(Some(Err(SourceCallbackError::ReplyClosed)));
            self.wake.notify_all();
        }
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
                    return Ok(Some(result));
                }
                ReplyCellState::ReceiverGone | ReplyCellState::Consumed => {
                    return Err(SourceCallbackError::ReplyClosed);
                }
            }
        }
    }

    fn drop_receiver(&self) {
        let lost = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match &mut *state {
                ReplyCellState::Pending => {
                    *state = ReplyCellState::ReceiverGone;
                    self.completion.reply_done();
                    None
                }
                ReplyCellState::Ready(result) => {
                    let result = result
                        .take()
                        .expect("Source callback reply result was consumed twice");
                    *state = ReplyCellState::ReceiverGone;
                    if result.is_ok() {
                        Some(result)
                    } else {
                        self.completion.reply_done();
                        None
                    }
                }
                ReplyCellState::ReceiverGone | ReplyCellState::Consumed => None,
            }
        };
        if let Some(result) = lost {
            (self.on_lost)(result);
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
}

impl<R: Send + 'static> SourceCallbackResponder<R> {
    /// Publish either the callback value or a transport-level failure.
    pub fn send(
        &mut self,
        result: Result<R, SourceCallbackError>,
    ) -> Result<(), SourceCallbackReplyError<R>> {
        let reply = self
            .reply
            .take()
            .ok_or(SourceCallbackReplyError::AlreadySent)?;
        match reply.send(result) {
            Ok(()) => Ok(()),
            Err(result) => {
                self.completion.reply_done();
                Err(SourceCallbackReplyError::Disconnected(result))
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

struct CallbackState<P, C, X, R> {
    phase: SessionPhase,
    next_callback: Option<u64>,
    next_request: Option<u64>,
    callbacks: HashMap<SourceCallbackId, CallbackEntry<P>>,
    queue: VecDeque<PendingEvent<C, X, R>>,
    wake: Arc<Condvar>,
}

impl<P, C, X, R> CallbackState<P, C, X, R> {
    fn status(&self) -> SourceCallbackDrainStatus {
        SourceCallbackDrainStatus {
            producers: self.callbacks.values().map(|entry| entry.leases).sum(),
            inflight: self.callbacks.values().map(|entry| entry.inflight).sum(),
            pending: self.queue.len(),
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

    fn queue_release(&mut self, callback: SourceCallbackId) {
        let should_queue = {
            let Some(entry) = self.callbacks.get_mut(&callback) else {
                return;
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
    }

    fn finish_invocation(&mut self, callback: SourceCallbackId) {
        let should_release = {
            let Some(entry) = self.callbacks.get_mut(&callback) else {
                return;
            };
            if entry.inflight != 0 {
                entry.inflight -= 1;
            }
            entry.leases == 0 && entry.inflight == 0 && entry.borrows == 0
        };
        if should_release {
            self.queue_release(callback);
        }
        self.wake.notify_all();
    }

    fn queue_cleanup(
        &mut self,
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        command: Option<C>,
        context: Option<X>,
        completion: Arc<InvocationCompletion>,
    ) {
        if !self.callbacks.contains_key(&callback) {
            return;
        }
        self.queue.push_back(PendingEvent::Cleanup {
            callback,
            request,
            command,
            context,
            completion,
        });
        self.wake.notify_one();
    }

    fn queue_reply_cleanup(
        &mut self,
        callback: SourceCallbackId,
        request: SourceCallbackRequestId,
        result: Result<R, SourceCallbackError>,
        completion: Arc<InvocationCompletion>,
    ) {
        if !self.callbacks.contains_key(&callback) {
            return;
        }
        self.queue.push_back(PendingEvent::ReplyCleanup {
            callback,
            request,
            result,
            completion,
        });
        self.wake.notify_one();
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
        let responder = self
            .responder
            .as_mut()
            .ok_or(SourceCallbackReplyError::AlreadySent)?;
        responder.send(result)
    }
}

impl<P, C, X, R> Drop for SourceCallbackInvocation<P, C, X, R> {
    fn drop(&mut self) {
        // Dropping the event always discharges its input side separately from
        // the reply claim.  In particular, a successful response does not
        // permit still-owned command/context values to bypass Source cleanup.
        self.responder.take();
        let command = self.command.take();
        let context = self.context.take();
        if command.is_some() || context.is_some() {
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
            );
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
            );
        } else {
            self.completion.inputs_done();
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
        }
        if requeued {
            // Ownership moved back into the session. Do not drop it here.
            return;
        }
        // `payload` is dropped after this function releases the mutex guard.
        drop(state);
        drop(payload);
    }
}

/// One owner-pump event. The pump owns semantic execution; this enum only
/// transports generic commands, context, replies, cleanup, and final-release
/// notices.
pub enum SourceCallbackEvent<P, C, X, R> {
    Invoke(SourceCallbackInvocation<P, C, X, R>),
    Cleanup(SourceCallbackCleanup<P, C, X, R>),
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
            if should_release {
                state.queue_release(self.callback);
            }
            return;
        }
        // The payload can only be absent here after an invariant violation;
        // drop it outside the state mutex rather than while holding the lock.
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
    /// Create an open, empty callback transport.
    pub fn new() -> Self {
        Self::default()
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
                let response_state = responder.state.clone();
                SourceCallbackEvent::Invoke(SourceCallbackInvocation {
                    state: self.state.clone(),
                    callback,
                    request,
                    command: Some(command),
                    context: Some(context),
                    responder: Some(responder),
                    response_state,
                })
            },
            PendingEvent::Cleanup {
                callback,
                request,
                command,
                context,
            } => SourceCallbackEvent::Cleanup(SourceCallbackCleanup {
                state: self.state.clone(),
                callback,
                request,
                command,
                context,
                completed: false,
            }),
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

impl<P, C, X, R> Default for SourceCallbackSession<P, C, X, R>
where
    P: Send + 'static,
    C: Send + 'static,
    X: Send + 'static,
    R: Send + 'static,
{
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(CallbackState {
                phase: SessionPhase::Open,
                next_callback: Some(1),
                next_request: Some(1),
                callbacks: HashMap::new(),
                queue: VecDeque::new(),
                wake: Arc::new(Condvar::new()),
            })),
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
        let (sender, receiver) = mpsc::sync_channel(1);
        entry.inflight += 1;
        state.queue.push_back(PendingEvent::Invoke {
            callback: self.callback,
            request,
            command,
            context,
            responder: SourceCallbackResponder {
                sender: Some(sender),
                state: Arc::new(AtomicU8::new(REPLY_PENDING)),
            },
        });
        state.wake.notify_one();
        Ok(SourceCallbackReply { receiver })
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
        if should_release {
            state.queue_release(self.callback);
        }
        state.wake.notify_all();
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

    #[test]
    fn never_invoked_lease_releases_original_payload_once() {
        let drops = Arc::new(AtomicUsize::new(0));
        let session: SourceCallbackSession<DropProbe, (), (), ()> =
            SourceCallbackSession::new();
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
            SourceCallbackSession::new();
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
        assert_eq!(reply.try_recv(), Err(SourceCallbackError::ReplyClosed));
        assert!(session.retire().is_ok());
    }

    #[test]
    fn dropped_invocation_queues_owned_command_and_context_cleanup() {
        let session: SourceCallbackSession<(), String, String, ()> =
            SourceCallbackSession::new();
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
    fn registration_enqueue_and_reply_failures_return_owned_values() {
        let session: SourceCallbackSession<String, String, String, String> =
            SourceCallbackSession::new();
        session.begin_retire();
        match session.register("payload".to_string()) {
            Err(error) => {
                assert_eq!(error.error, SourceCallbackError::Retiring);
                assert_eq!(error.payload, "payload");
            }
            Ok(_) => panic!("retiring session accepted a payload"),
        }

        let session: SourceCallbackSession<(), String, String, String> =
            SourceCallbackSession::new();
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
            SourceCallbackSession::new();
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
        match invocation.respond(Ok("return".to_string())) {
            Err(SourceCallbackReplyError::Disconnected(Ok(value))) => {
                assert_eq!(value, "return");
            }
            _ => panic!("disconnected reply did not return its owned value"),
        }
        drop(invocation);
        match take_event(&session) {
            SourceCallbackEvent::Cleanup(mut cleanup) => {
                assert_eq!(cleanup.take_command(), Some("command".to_string()));
                assert_eq!(cleanup.take_context(), Some("scope".to_string()));
                cleanup.complete();
            }
            _ => panic!("disconnected invocation must enqueue cleanup"),
        }
        drop(lease);
        match take_event(&session) {
            SourceCallbackEvent::Release(release) => assert!(release.complete().is_ok()),
            _ => panic!("final lease must enqueue release"),
        }
    }

    #[test]
    fn payload_and_release_cleanup_reenter_without_session_mutex() {
        let drops = Arc::new(AtomicUsize::new(0));
        let reentered = Arc::new(AtomicUsize::new(0));
        let session: SourceCallbackSession<DropProbe, (), (), ()> =
            SourceCallbackSession::new();
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
            SourceCallbackSession::new();
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
            SourceCallbackSession::new();
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

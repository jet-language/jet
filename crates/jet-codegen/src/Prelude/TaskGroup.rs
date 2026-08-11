// D-TASKSCOPE1=A / D-TASKGROUP-PARAM1=A: canonical task-group ownership.
// This exact Prelude source is compiled for JIT hosts and embedded in AOT
// programs. Engines supply only representation-specific cancel/join adapters.
thread_local! {
    static JET_TASK_DEADLINE_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn jet_task_deadline_mark_pending() {
    JET_TASK_DEADLINE_PENDING.with(|pending| pending.set(true));
}

pub fn jet_task_deadline_pending() -> bool {
    JET_TASK_DEADLINE_PENDING.with(|pending| pending.get())
}

pub fn jet_task_deadline_clear_pending() {
    JET_TASK_DEADLINE_PENDING.with(|pending| pending.set(false));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetTaskFailure {
    Cancelled,
    DeadlineBlown,
    Panicked(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetTaskCancellation {
    pub code: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub fix: &'static str,
}

pub fn jet_task_cancellation() -> JetTaskCancellation {
    JetTaskCancellation {
        code: "E3004",
        what: "task cancelled at a cooperative wait point",
        why: "the task control plane requested cancellation before this wait completed",
        fix: "handle `TaskFailure.Cancelled`, or use `#Shield` around a cancellation-sensitive wait",
    }
}

/// Map an engine's child-completion code onto the canonical failure rail.
/// The surrounding engine only marshals the resulting enum into its value
/// representation.
pub fn jet_task_failure_from_code(code: &str, reason: String) -> JetTaskFailure {
    match code {
        "E3004" => JetTaskFailure::Cancelled,
        "E3003" => JetTaskFailure::DeadlineBlown,
        _ => JetTaskFailure::Panicked(reason),
    }
}

/// One ABI spelling for the typed failure rail. Engines may pack the returned
/// tag beside their representation-specific reason handle, but the failure
/// meaning and tag values live here with `JetTaskFailure`.
pub fn jet_task_failure_abi(
    failure: JetTaskFailure,
    encode_reason: impl FnOnce(String) -> u64,
) -> u64 {
    match failure {
        JetTaskFailure::Cancelled => 0,
        JetTaskFailure::DeadlineBlown => 1,
        JetTaskFailure::Panicked(reason) => (encode_reason(reason) << 8) | 2,
    }
}

/// D-CONC-SPAWN1=D: explicit group limits share one clamping rule on every
/// execution tier. `None` means no admission bound; an explicit value below
/// one is the smallest bounded group.
pub fn jet_task_group_limit_defaulted(limit: Option<i64>) -> Option<usize> {
    limit.map(|limit| limit.max(1) as usize)
}

#[derive(Debug)]
struct JetTaskGroupSlots {
    limit: usize,
    active: std::sync::Mutex<usize>,
    closing: std::sync::atomic::AtomicBool,
    wake: std::sync::Condvar,
}

impl JetTaskGroupSlots {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            active: std::sync::Mutex::new(0),
            closing: std::sync::atomic::AtomicBool::new(false),
            wake: std::sync::Condvar::new(),
        }
    }

    fn close(&self) {
        let _active = self.active.lock().unwrap();
        self.closing
            .store(true, std::sync::atomic::Ordering::Release);
        self.wake.notify_all();
    }

    fn acquire(self: &std::sync::Arc<Self>) -> Option<JetTaskGroupPermit> {
        let mut active = self.active.lock().unwrap();
        while *active >= self.limit
            && !self
                .closing
                .load(std::sync::atomic::Ordering::Acquire)
        {
            active = self.wake.wait(active).unwrap();
        }
        if self
            .closing
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return None;
        }
        *active += 1;
        Some(JetTaskGroupPermit {
            slots: self.clone(),
        })
    }
}

#[derive(Debug)]
pub struct JetTaskGroupPermit {
    slots: std::sync::Arc<JetTaskGroupSlots>,
}

impl Drop for JetTaskGroupPermit {
    fn drop(&mut self) {
        let mut active = self.slots.active.lock().unwrap();
        *active = active.saturating_sub(1);
        self.slots.wake.notify_one();
    }
}

#[derive(Debug)]
pub struct JetTaskGroupRuntime<T> {
    children: std::sync::Mutex<Vec<T>>,
    slots: Option<std::sync::Arc<JetTaskGroupSlots>>,
    closing: std::sync::atomic::AtomicBool,
}

impl<T> JetTaskGroupRuntime<T> {
    pub fn new() -> Self {
        Self::new_defaulted(None)
    }

    /// Construct the runtime policy for a canonical `task.group` limit.
    /// Engines pass the source-level default through this one Prelude symbol.
    pub fn new_defaulted(limit: Option<i64>) -> Self {
        Self {
            children: std::sync::Mutex::new(Vec::new()),
            slots: jet_task_group_limit_defaulted(limit)
                .map(|limit| std::sync::Arc::new(JetTaskGroupSlots::new(limit))),
            closing: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn acquire(&self) -> Option<JetTaskGroupPermit> {
        self.slots.as_ref().and_then(|slots| slots.acquire())
    }

    pub fn register(&self, child: T) {
        self.children.lock().unwrap().push(child);
    }

    fn begin_close(&self) -> bool {
        if self
            .closing
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return false;
        }
        if let Some(slots) = &self.slots {
            // A child that is already being drained may still reach a nested
            // spawn. Let it register without waiting for the permit held by
            // the child being drained; the close loop will consume it.
            slots.close();
        }
        true
    }

    fn close_with_mode<C, J>(&self, cancel_children: bool, mut cancel: C, mut join: J)
    where
        C: FnMut(&T),
        J: FnMut(T),
    {
        if !self.begin_close() {
            return;
        }
        // A child may register another child through the shared lexical group
        // handle while it is being joined. Drain until the shared queue is
        // empty so lexical close covers that nested work too.
        loop {
            let children = std::mem::take(&mut *self.children.lock().unwrap());
            if children.is_empty() {
                break;
            }
            if cancel_children {
                for child in &children {
                    cancel(child);
                }
            }
            for child in children {
                // D-CONC-FAIL1=A: lexical close joins and discards child outcomes.
                // A child failure remains observable only through that child's
                // explicit `join()`/combinator result; it must not escape the
                // group's cleanup boundary or terminate the parent.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| join(child)));
            }
        }
    }

    pub fn close_with<J>(&self, join: J)
    where
        J: FnMut(T),
    {
        self.close_with_mode(false, |_| {}, join);
    }

    pub fn close_with_cancel<C, J>(&self, cancel: C, join: J)
    where
        C: FnMut(&T),
        J: FnMut(T),
    {
        self.close_with_mode(true, cancel, join);
    }
}

#[derive(Clone, Copy)]
pub enum JetTaskSelectMode {
    All,
    Race,
    Any,
}

pub enum JetTaskDecision<T, E> {
    Wait,
    Finish(Result<Vec<T>, E>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTaskWaitInterrupt<D> {
    Deadline(D),
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTaskDeadline {
    pub what: String,
    pub why: String,
    pub fix: String,
}

impl JetTaskDeadline {
    pub fn render(&self) -> String {
        format!(
            "Error [E3003]: {}\n Why: {}\n Fix: {}",
            self.what, self.why, self.fix
        )
    }
}

pub fn jet_task_deadline(wait_kind: &str) -> JetTaskDeadline {
    JetTaskDeadline {
        what: format!("deadline exceeded while waiting in {wait_kind}"),
        why: "this wait point observed the task context deadline from `#Context(deadline: …)`"
            .to_string(),
        fix: "raise the deadline budget or shorten the work before this wait point".to_string(),
    }
}

/// Return the canonical deadline value only when a wait point has expired.
/// Hosts provide their remaining-time observation; this Prelude owns the
/// boundary comparison and the resulting wait kind.
pub fn jet_task_deadline_if_expired(
    remaining_ms: Option<i64>,
    wait_kind: &str,
) -> Option<JetTaskDeadline> {
    remaining_ms
        .filter(|remaining| *remaining <= 0)
        .map(|_| jet_task_deadline(wait_kind))
}

/// Canonical parent wait-point policy. A shield defers both interrupts;
/// otherwise an expired deadline lands before a pending cancellation.
pub fn jet_task_wait_policy<D>(
    deadline: Option<D>,
    cancelled: bool,
    shielded: bool,
) -> Result<(), JetTaskWaitInterrupt<D>> {
    if shielded {
        return Ok(());
    }
    if let Some(deadline) = deadline {
        return Err(JetTaskWaitInterrupt::Deadline(deadline));
    }
    if cancelled {
        return Err(JetTaskWaitInterrupt::Cancelled);
    }
    Ok(())
}

/// Canonical all/race/any result policy. Engines only report completed task
/// outcomes and apply cancellation/drain when this policy says to finish.
pub struct JetTaskSelectPolicy<T, E> {
    mode: JetTaskSelectMode,
    pending: usize,
    values: Vec<Option<T>>,
    first_error: Option<(u128, E)>,
}

impl<T, E> JetTaskSelectPolicy<T, E> {
    pub fn new(mode: JetTaskSelectMode, count: usize) -> Self {
        if count == 0 {
            unreachable!("sema must reject an empty task group combinator");
        }
        Self {
            mode,
            pending: count,
            values: (0..count).map(|_| None).collect(),
            first_error: None,
        }
    }

    pub fn settle(
        &mut self,
        order: u128,
        index: usize,
        result: Result<T, E>,
    ) -> JetTaskDecision<T, E> {
        self.pending -= 1;
        match self.mode {
            JetTaskSelectMode::Any => {
                JetTaskDecision::Finish(result.map(|value| vec![value]))
            }
            JetTaskSelectMode::Race => match result {
                Ok(value) => JetTaskDecision::Finish(Ok(vec![value])),
                Err(error) => {
                    if self
                        .first_error
                        .as_ref()
                        .is_none_or(|(first, _)| order < *first)
                    {
                        self.first_error = Some((order, error));
                    }
                    if self.pending == 0 {
                        JetTaskDecision::Finish(Err(
                            self.first_error.take().expect("race recorded an error").1,
                        ))
                    } else {
                        JetTaskDecision::Wait
                    }
                }
            },
            JetTaskSelectMode::All => match result {
                Err(error) => JetTaskDecision::Finish(Err(error)),
                Ok(value) => {
                    self.values[index] = Some(value);
                    if self.pending == 0 {
                        JetTaskDecision::Finish(Ok(
                            self.values
                                .iter_mut()
                                .map(|value| value.take().expect("all result missing"))
                                .collect(),
                        ))
                    } else {
                        JetTaskDecision::Wait
                    }
                }
            },
        }
    }
}

/// Canonical all/race/any wait loop. Engines only marshal their task handle,
/// completion, cancellation, and drain operations into these callbacks.
pub fn jet_task_select<Task, T, E>(
    tasks: Vec<Task>,
    mode: JetTaskSelectMode,
    mut wait_check: impl FnMut() -> Result<(), E>,
    mut completion_order: impl FnMut(&Task) -> Option<u128>,
    mut try_complete: impl FnMut(&mut Task) -> Option<Result<T, E>>,
    mut cancel: impl FnMut(&Task),
    mut drain: impl FnMut(Task),
) -> Result<Vec<T>, E> {
    if tasks.is_empty() {
        unreachable!("sema must reject an empty task group combinator");
    }
    let mut tasks = tasks.into_iter().map(Some).collect::<Vec<_>>();
    let mut policy = JetTaskSelectPolicy::new(mode, tasks.len());
    loop {
        if let Err(error) = wait_check() {
            for task in tasks.iter().flatten() {
                cancel(task);
            }
            for task in tasks.into_iter().flatten() {
                drain(task);
            }
            return Err(error);
        }
        let next = tasks
            .iter()
            .enumerate()
            .filter_map(|(index, task)| {
                task.as_ref()
                    .and_then(&mut completion_order)
                    .map(|order| (order, index))
            })
            .min();
        if let Some((order, index)) = next {
            let result = tasks[index].as_mut().and_then(&mut try_complete);
            if let Some(result) = result {
                tasks[index] = None;
                if let JetTaskDecision::Finish(result) = policy.settle(order, index, result) {
                    if result.is_err() || !matches!(mode, JetTaskSelectMode::All) {
                        for task in tasks.iter().flatten() {
                            cancel(task);
                        }
                    }
                    for task in tasks.into_iter().flatten() {
                        drain(task);
                    }
                    return result;
                }
            }
        }
        std::thread::yield_now();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        jet_task_select, jet_task_wait_policy, JetTaskDecision, JetTaskGroupRuntime,
        JetTaskSelectMode, JetTaskSelectPolicy, JetTaskWaitInterrupt,
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn close_joins_all_without_cancelling() {
        let group = JetTaskGroupRuntime::new();
        group.register(1);
        group.register(2);
        group.register(3);
        let events = Arc::new(Mutex::new(Vec::new()));
        let join_events = events.clone();
        group.close_with(|child| {
            join_events.lock().unwrap().push(format!("join {child}"));
        });
        assert_eq!(
            *events.lock().unwrap(),
            ["join 1", "join 2", "join 3"]
        );
    }

    #[test]
    fn shared_selection_policy_preserves_all_order_and_race_any_laws() {
        let mut all = JetTaskSelectPolicy::new(JetTaskSelectMode::All, 2);
        assert!(matches!(all.settle(1, 1, Ok::<_, &str>(20)), JetTaskDecision::Wait));
        match all.settle(2, 0, Ok(10)) {
            JetTaskDecision::Finish(Ok(values)) => assert_eq!(values, [10, 20]),
            _ => panic!("all did not finish in input order"),
        }

        let mut race = JetTaskSelectPolicy::new(JetTaskSelectMode::Race, 2);
        assert!(matches!(
            race.settle(1, 0, Err::<i32, _>("first failed")),
            JetTaskDecision::Wait
        ));
        match race.settle(2, 1, Ok(22)) {
            JetTaskDecision::Finish(Ok(values)) => assert_eq!(values, [22]),
            _ => panic!("race did not select its first success"),
        }

        let mut any = JetTaskSelectPolicy::new(JetTaskSelectMode::Any, 2);
        match any.settle(1, 1, Err::<i32, _>("first failed")) {
            JetTaskDecision::Finish(Err(error)) => assert_eq!(error, "first failed"),
            _ => panic!("any did not expose its first completion"),
        }
    }

    #[test]
    fn shared_wait_loop_cancels_and_drains_after_an_error() {
        struct Task {
            id: i32,
            order: u128,
            result: Option<Result<i32, &'static str>>,
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancel_events = events.clone();
        let drain_events = events.clone();
        let result = jet_task_select(
            vec![
                Task {
                    id: 1,
                    order: 0,
                    result: Some(Err("failed")),
                },
                Task {
                    id: 2,
                    order: 1,
                    result: Some(Ok(2)),
                },
            ],
            JetTaskSelectMode::All,
            || Ok(()),
            |task| Some(task.order),
            |task| task.result.take(),
            |task| cancel_events.lock().unwrap().push(("cancel", task.id)),
            |task| drain_events.lock().unwrap().push(("drain", task.id)),
        );
        assert_eq!(result, Err("failed"));
        assert_eq!(*events.lock().unwrap(), [("cancel", 2), ("drain", 2)]);
    }

    #[test]
    fn parent_wait_policy_defers_a_shield_and_prefers_a_deadline() {
        assert_eq!(
            jet_task_wait_policy(Some("deadline"), true, false),
            Err(JetTaskWaitInterrupt::Deadline("deadline"))
        );
        assert_eq!(
            jet_task_wait_policy::<&str>(None, true, false),
            Err(JetTaskWaitInterrupt::Cancelled)
        );
        assert_eq!(jet_task_wait_policy(Some("deadline"), true, true), Ok(()));
    }
}

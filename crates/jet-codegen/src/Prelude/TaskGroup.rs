// D-TASKSCOPE1=A / D-TASKGROUP-PARAM1=A: canonical task-group ownership.
// This exact Prelude source is compiled for JIT hosts and embedded in AOT
// programs. Engines supply only representation-specific cancel/join adapters.
pub struct JetTaskGroupRuntime<T> {
    children: std::sync::Mutex<Vec<T>>,
}

impl<T> JetTaskGroupRuntime<T> {
    pub fn new() -> Self {
        Self {
            children: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, child: T) {
        self.children.lock().unwrap().push(child);
    }

    pub fn close_with<C, J>(&self, mut cancel: C, mut join: J)
    where
        C: FnMut(&T),
        J: FnMut(T),
    {
        let children = std::mem::take(&mut *self.children.lock().unwrap());
        for child in &children {
            cancel(child);
        }
        let mut first_panic = None;
        for child in children {
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| join(child)))
            {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                }
            }
        }
        if let Some(payload) = first_panic {
            std::panic::resume_unwind(payload);
        }
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
        assert!(count > 0, "task selection needs at least one task");
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

#[cfg(test)]
mod tests {
    use super::{
        JetTaskDecision, JetTaskGroupRuntime, JetTaskSelectMode, JetTaskSelectPolicy,
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn close_cancels_all_then_joins_all_before_first_panic() {
        let group = JetTaskGroupRuntime::new();
        group.register(1);
        group.register(2);
        group.register(3);
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancel_events = events.clone();
        let join_events = events.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            group.close_with(
                |child| cancel_events.lock().unwrap().push(format!("cancel {child}")),
                |child| {
                    join_events.lock().unwrap().push(format!("join {child}"));
                    if child != 3 {
                        panic!("failed {child}");
                    }
                },
            );
        }));
        assert!(result.is_err());
        assert_eq!(
            *events.lock().unwrap(),
            ["cancel 1", "cancel 2", "cancel 3", "join 1", "join 2", "join 3"]
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
}

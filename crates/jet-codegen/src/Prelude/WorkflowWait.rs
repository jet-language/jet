/// D-SERVICE-WORKFLOW1=D: the workflow Prelude owns the recorded wait-point
/// decision. Engines only supply this small result carrier at the scheduler
/// boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetServiceWorkflowWait<T> {
    Ready(T),
    Cancelled,
    Deadline(String),
    Panicked(String),
}

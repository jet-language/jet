/// D-SERVICE-WORKFLOW1=D / I9: the workflow Prelude records the wait before
/// calling this one scheduler adapter. AOT and resident engines marshal their
/// scheduler result into the shared workflow carrier.
fn jet_services_workflow_sleep_wait(nanos: i64) -> JetServiceWorkflowWait<()> {
    match jet_scheduler_wait_without_unwind(|| jet_std_time_sleep_duration_ns(nanos)) {
        JetSchedulerWait::Ready(()) => JetServiceWorkflowWait::Ready(()),
        JetSchedulerWait::Cancelled => JetServiceWorkflowWait::Cancelled,
        JetSchedulerWait::Deadline(reason) => JetServiceWorkflowWait::Deadline(reason),
        JetSchedulerWait::Panicked(reason) => JetServiceWorkflowWait::Panicked(reason),
    }
}

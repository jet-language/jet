use jet_devserver::Devtools::{
    JetDevtoolsEnvelope, JetDevtoolsEvent, JetDevtoolsHostKind, JetDevtoolsPanelCapability,
    JetDevtoolsPanelCatalog, JetDevtoolsPanelId,
};

fn queue_status_event() -> JetDevtoolsEvent {
    JetDevtoolsEvent::from_parts(
        42,
        "job-runtime",
        "Job",
        "authority:default",
        r#"{"event":"status","state":"status","job_id":"authority:default","name":"default","queue":"default","attempts":0,"duration_ms":null,"worker":null,"request_id":null,"failure":null,"reason":null,"sequence":0,"authority":"authority","queued":1,"running":0,"retrying":0,"completed":0,"failed":0,"dead_lettered":0,"cancelled":0,"depth":1,"wait_ms":0,"throughput":0,"capacity":8,"paused":false,"freshness_ms":0,"labels":{}}"#,
    )
    .expect("queue status event must satisfy the canonical envelope contract")
}

#[test]
fn durable_queue_observations_are_consumed_by_jobs_panel() {
    let event = queue_status_event();
    let catalog = JetDevtoolsPanelCatalog::canonical().expect("canonical panel catalog");
    assert_eq!(catalog.panel_for_event(&event), Some(JetDevtoolsPanelId::Jobs));
    assert!(event.payload_json().is_none());

    let mut envelope = JetDevtoolsEnvelope::new("queue-session", 1);
    envelope.push(event);
    let grants = JetDevtoolsPanelCapability::ALL.to_vec();
    let panels = catalog.project(&envelope, JetDevtoolsHostKind::Headless, &grants);
    let jobs = panels
        .iter()
        .find(|panel| panel.availability.descriptor.id == JetDevtoolsPanelId::Jobs)
        .expect("Jobs panel descriptor");
    assert!(jobs.availability.is_available());
    assert_eq!(jobs.events.len(), 1);
    assert_eq!(jobs.events[0].kind(), "Job");
    assert!(jobs.events[0].fields_json().contains("\"depth\":1"));
    assert!(jobs.events[0].payload_json().is_none());
}

#[test]
fn typed_job_lifecycle_facts_share_the_same_jobs_route() {
    let fact = jet_devserver::Devtools::JetDevtoolsJobPanelFact {
        sequence: 1,
        timestamp_ms: 7,
        kind: jet_devserver::Devtools::JetDevtoolsJobPanelEventKind::Enqueue,
        state: jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState::Enqueued,
        job_id: "job-1".to_string(),
        name: "Sync".to_string(),
        labels: Vec::new(),
        queue: "default".to_string(),
        attempts: 0,
        duration_ms: None,
        worker: None,
        request_id: Some("request-1".to_string()),
        failure: None,
    };
    let event = fact
        .to_protocol_event("job-runtime")
        .expect("typed lifecycle fact must marshal through the shared event boundary");
    let catalog = JetDevtoolsPanelCatalog::canonical().expect("canonical panel catalog");
    assert_eq!(catalog.panel_for_event(&event), Some(JetDevtoolsPanelId::Jobs));
    assert!(event.payload_json().is_none());
    assert!(event.fields_json().contains("\"request_id\":\"request-1\""));

    let mut state = jet_devserver::Devtools::JetDevtoolsJobPanelState::new();
    state
        .apply_fact(fact)
        .expect("typed lifecycle fact must be consumed by the Jobs panel state");
    assert_eq!(
        state.job("job-1").unwrap().request_correlation(),
        Some("request-1")
    );
}

#[test]
fn absent_snapshot_workers_preserve_lifecycle_worker_facts() {
    let mut state = jet_devserver::Devtools::JetDevtoolsJobPanelState::new();
    state
        .enqueue("job-1", "Sync", "default", Vec::new(), None, 10)
        .expect("lifecycle enqueue");
    state
        .start("job-1", 20, "worker-1")
        .expect("lifecycle start");
    state
        .observe_queue_status("default", 8, false, 1, 10, 2, 30)
        .expect("queue snapshot");

    let projection = state
        .queue_projection("default", 40, None)
        .expect("Jobs queue projection");
    assert_eq!(projection.depth, 1);
    assert_eq!(projection.wait_ms, 10);
    assert_eq!(projection.throughput, 2);
    assert_eq!(projection.workers, 1);
}

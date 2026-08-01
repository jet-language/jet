// ── D-SERVICE1=D (#444): sema-known structured service tree over taskgroups ─
// Beginner topology: named tree → workers with typed mailboxes → OneForOne
// restart policy. Delivery is at-most-once with Full under capacity (D-SERVICE-
// DELIVERY1). Engines marshal into these Prelude symbols only (I9).

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetServiceRestart {
    OneForOne,
    OneForAll,
    RestForOne,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetServiceDelivery {
    AtMostOnce,
    DurableAtLeastOnce,
}

#[derive(Clone, Debug)]
struct JetServiceEndpoint {
    tree: String,
    worker: String,
    generation: i64,
}

#[derive(Clone, Debug)]
struct JetServiceMailbox {
    endpoint: JetServiceEndpoint,
    capacity: i64,
    depth: i64,
    messages: Vec<String>,
}

#[derive(Clone, Debug)]
struct JetServiceWorker {
    name: String,
    endpoint: JetServiceEndpoint,
    mailbox: JetServiceMailbox,
    restarts: i64,
    running: bool,
}

#[derive(Clone, Debug)]
struct JetServiceGroup {
    name: String,
    restart: JetServiceRestart,
    workers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetServiceStateAdapter {
    Empty,
    Snapshot,
    EventLog,
}

#[derive(Clone, Debug)]
struct JetServiceWorkflow {
    id: String,
    run_id: i64,
    version: i64,
    steps: Vec<String>,
    history: Vec<String>,
}

#[derive(Clone, Debug)]
struct JetServiceTree {
    name: String,
    generation: i64,
    delivery: JetServiceDelivery,
    restart: JetServiceRestart,
    workers: Vec<JetServiceWorker>,
    groups: Vec<JetServiceGroup>,
    started: bool,
    state_adapter: JetServiceStateAdapter,
    snapshot: Option<String>,
    event_log: Vec<String>,
    dead_letters: Vec<String>,
    idempotency_seen: Vec<String>,
    directory: Vec<(String, JetServiceEndpoint)>,
    draining: Vec<String>,
    workflows: Vec<JetServiceWorkflow>,
    chaos_fails: i64,
    previous_generation: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetServiceError {
    Full(String),
    Ambiguous(String),
    Unknown(String),
    NotStarted(String),
    Policy(String),
}

impl JetShow for JetServiceRestart {
    fn jet_show(&self) -> String {
        match self {
            JetServiceRestart::OneForOne => "OneForOne".to_string(),
            JetServiceRestart::OneForAll => "OneForAll".to_string(),
            JetServiceRestart::RestForOne => "RestForOne".to_string(),
        }
    }
}

impl JetShow for JetServiceDelivery {
    fn jet_show(&self) -> String {
        match self {
            JetServiceDelivery::AtMostOnce => "AtMostOnce".to_string(),
            JetServiceDelivery::DurableAtLeastOnce => "DurableAtLeastOnce".to_string(),
        }
    }
}

impl JetShow for JetServiceEndpoint {
    fn jet_show(&self) -> String {
        format!(
            "Endpoint({}/{}@g{})",
            self.tree, self.worker, self.generation
        )
    }
}

impl JetShow for JetServiceError {
    fn jet_show(&self) -> String {
        match self {
            JetServiceError::Full(m)
            | JetServiceError::Ambiguous(m)
            | JetServiceError::Unknown(m)
            | JetServiceError::NotStarted(m)
            | JetServiceError::Policy(m) => m.clone(),
        }
    }
}

impl JetShow for JetServiceTree {
    fn jet_show(&self) -> String {
        format!(
            "ServiceTree(name={}, workers={}, started={}, restart={}, delivery={})",
            self.name,
            self.workers.len(),
            self.started,
            self.restart.jet_show(),
            self.delivery.jet_show()
        )
    }
}

fn jet_services_tree(name: String) -> JetServiceTree {
    JetServiceTree {
        name,
        generation: 1,
        delivery: JetServiceDelivery::AtMostOnce,
        restart: JetServiceRestart::OneForOne,
        workers: Vec::new(),
        groups: Vec::new(),
        started: false,
        state_adapter: JetServiceStateAdapter::Empty,
        snapshot: None,
        event_log: Vec::new(),
        dead_letters: Vec::new(),
        idempotency_seen: Vec::new(),
        directory: Vec::new(),
        draining: Vec::new(),
        workflows: Vec::new(),
        chaos_fails: 0,
        previous_generation: 0,
    }
}

fn jet_services_restart_one_for_one() -> JetServiceRestart {
    JetServiceRestart::OneForOne
}

fn jet_services_restart_one_for_all() -> JetServiceRestart {
    JetServiceRestart::OneForAll
}

fn jet_services_restart_rest_for_one() -> JetServiceRestart {
    JetServiceRestart::RestForOne
}

fn jet_services_delivery_at_most_once() -> JetServiceDelivery {
    JetServiceDelivery::AtMostOnce
}

fn jet_services_delivery_durable() -> JetServiceDelivery {
    JetServiceDelivery::DurableAtLeastOnce
}

fn jet_services_set_restart(
    tree: &mut JetServiceTree,
    restart: JetServiceRestart,
) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change restart policy after start".to_string(),
        ));
    }
    tree.restart = restart;
    Ok(())
}

fn jet_services_worker(
    tree: &mut JetServiceTree,
    name: String,
    capacity: i64,
) -> Result<JetServiceEndpoint, JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot add workers after start".to_string(),
        ));
    }
    if capacity < 0 {
        return Err(JetServiceError::Policy(
            "mailbox capacity must be non-negative".to_string(),
        ));
    }
    if tree.workers.iter().any(|w| w.name == name) {
        return Err(JetServiceError::Unknown(format!(
            "worker `{name}` already exists in tree `{}`",
            tree.name
        )));
    }
    let endpoint = JetServiceEndpoint {
        tree: tree.name.clone(),
        worker: name.clone(),
        generation: tree.generation,
    };
    let mailbox = JetServiceMailbox {
        endpoint: endpoint.clone(),
        capacity,
        depth: 0,
        messages: Vec::new(),
    };
    tree.workers.push(JetServiceWorker {
        name,
        endpoint: endpoint.clone(),
        mailbox,
        restarts: 0,
        running: false,
    });
    Ok(endpoint)
}

fn jet_services_group(
    tree: &mut JetServiceTree,
    name: String,
    workers: Vec<String>,
) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot add groups after start".to_string(),
        ));
    }
    for w in &workers {
        if !tree.workers.iter().any(|worker| &worker.name == w) {
            return Err(JetServiceError::Unknown(format!(
                "group `{name}` references unknown worker `{w}`"
            )));
        }
    }
    tree.groups.push(JetServiceGroup {
        name,
        restart: tree.restart.clone(),
        workers,
    });
    Ok(())
}

fn jet_services_start(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    if tree.workers.is_empty() {
        return Err(JetServiceError::Policy(
            "service tree has no workers".to_string(),
        ));
    }
    for worker in &mut tree.workers {
        worker.running = true;
    }
    tree.started = true;
    Ok(())
}

fn jet_services_stop(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    for worker in &mut tree.workers {
        worker.running = false;
        worker.mailbox.messages.clear();
        worker.mailbox.depth = 0;
    }
    tree.started = false;
    Ok(())
}

fn jet_services_find_worker_mut<'a>(
    tree: &'a mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<&'a mut JetServiceWorker, JetServiceError> {
    tree.workers
        .iter_mut()
        .find(|w| w.name == endpoint.worker && w.endpoint.tree == endpoint.tree)
        .ok_or_else(|| {
            JetServiceError::Unknown(format!(
                "endpoint {}/{} is not in this tree",
                endpoint.tree, endpoint.worker
            ))
        })
}

fn jet_services_send(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
    message: String,
) -> Result<(), JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    let worker = jet_services_find_worker_mut(tree, endpoint)?;
    if !worker.running {
        return Err(JetServiceError::NotStarted(format!(
            "worker `{}` is not running",
            worker.name
        )));
    }
    if worker.mailbox.capacity > 0 && worker.mailbox.depth >= worker.mailbox.capacity {
        return Err(JetServiceError::Full(format!(
            "mailbox for `{}` is full (capacity {})",
            worker.name, worker.mailbox.capacity
        )));
    }
    worker.mailbox.messages.push(message);
    worker.mailbox.depth = worker.mailbox.messages.len() as i64;
    Ok(())
}

fn jet_services_receive(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<String, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    let worker = jet_services_find_worker_mut(tree, endpoint)?;
    if worker.mailbox.messages.is_empty() {
        return Err(JetServiceError::Ambiguous(format!(
            "mailbox for `{}` is empty",
            worker.name
        )));
    }
    let message = worker.mailbox.messages.remove(0);
    worker.mailbox.depth = worker.mailbox.messages.len() as i64;
    Ok(message)
}

fn jet_services_mailbox_depth(
    tree: &JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<i64, JetServiceError> {
    tree.workers
        .iter()
        .find(|w| w.name == endpoint.worker && w.endpoint.tree == endpoint.tree)
        .map(|w| w.mailbox.depth)
        .ok_or_else(|| {
            JetServiceError::Unknown(format!(
                "endpoint {}/{} is not in this tree",
                endpoint.tree, endpoint.worker
            ))
        })
}

fn jet_services_fail_worker(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    let restart = tree.restart.clone();
    let worker_name = endpoint.worker.clone();
    match restart {
        JetServiceRestart::OneForOne => {
            let worker = jet_services_find_worker_mut(tree, endpoint)?;
            worker.restarts += 1;
            worker.running = true;
            worker.mailbox.messages.clear();
            worker.mailbox.depth = 0;
        }
        JetServiceRestart::OneForAll => {
            for worker in &mut tree.workers {
                worker.restarts += 1;
                worker.running = true;
                worker.mailbox.messages.clear();
                worker.mailbox.depth = 0;
            }
        }
        JetServiceRestart::RestForOne => {
            let start = tree
                .workers
                .iter()
                .position(|w| w.name == worker_name)
                .ok_or_else(|| {
                    JetServiceError::Unknown(format!(
                        "endpoint {}/{} is not in this tree",
                        endpoint.tree, endpoint.worker
                    ))
                })?;
            for worker in tree.workers.iter_mut().skip(start) {
                worker.restarts += 1;
                worker.running = true;
                worker.mailbox.messages.clear();
                worker.mailbox.depth = 0;
            }
        }
    }
    Ok(())
}

fn jet_services_restarts(
    tree: &JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<i64, JetServiceError> {
    tree.workers
        .iter()
        .find(|w| w.name == endpoint.worker && w.endpoint.tree == endpoint.tree)
        .map(|w| w.restarts)
        .ok_or_else(|| {
            JetServiceError::Unknown(format!(
                "endpoint {}/{} is not in this tree",
                endpoint.tree, endpoint.worker
            ))
        })
}

fn jet_services_endpoint_show(endpoint: &JetServiceEndpoint) -> String {
    endpoint.jet_show()
}

fn jet_services_tree_show(tree: &JetServiceTree) -> String {
    tree.jet_show()
}

fn jet_services_set_delivery(
    tree: &mut JetServiceTree,
    delivery: JetServiceDelivery,
) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change delivery after start".to_string(),
        ));
    }
    tree.delivery = delivery;
    Ok(())
}

fn jet_services_send_durable(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
    message: String,
    idempotency_key: String,
) -> Result<(), JetServiceError> {
    if tree.delivery != JetServiceDelivery::DurableAtLeastOnce {
        return Err(JetServiceError::Policy(
            "send_durable requires DurableAtLeastOnce delivery".to_string(),
        ));
    }
    if idempotency_key.is_empty() {
        return Err(JetServiceError::Policy(
            "durable send requires a non-empty idempotency key".to_string(),
        ));
    }
    if tree.idempotency_seen.iter().any(|k| k == &idempotency_key) {
        return Ok(());
    }
    match jet_services_send(tree, endpoint, message) {
        Ok(()) => {
            tree.idempotency_seen.push(idempotency_key);
            Ok(())
        }
        Err(JetServiceError::Full(m)) => {
            tree.dead_letters
                .push(format!("{idempotency_key}:{m}"));
            Err(JetServiceError::Full(m))
        }
        Err(e) => Err(e),
    }
}

fn jet_services_dead_letter_count(tree: &JetServiceTree) -> i64 {
    tree.dead_letters.len() as i64
}

fn jet_services_drain_dead_letters(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    let n = tree.dead_letters.len() as i64;
    tree.dead_letters.clear();
    Ok(n)
}

fn jet_services_set_state_empty(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change state adapter after start".to_string(),
        ));
    }
    tree.state_adapter = JetServiceStateAdapter::Empty;
    Ok(())
}

fn jet_services_set_state_snapshot(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change state adapter after start".to_string(),
        ));
    }
    tree.state_adapter = JetServiceStateAdapter::Snapshot;
    Ok(())
}

fn jet_services_set_state_event_log(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change state adapter after start".to_string(),
        ));
    }
    tree.state_adapter = JetServiceStateAdapter::EventLog;
    Ok(())
}

fn jet_services_commit_snapshot(
    tree: &mut JetServiceTree,
    payload: String,
) -> Result<(), JetServiceError> {
    if tree.state_adapter != JetServiceStateAdapter::Snapshot {
        return Err(JetServiceError::Policy(
            "commit_snapshot requires Snapshot state adapter".to_string(),
        ));
    }
    tree.snapshot = Some(payload);
    Ok(())
}

fn jet_services_restore_snapshot(tree: &JetServiceTree) -> Result<String, JetServiceError> {
    match &tree.snapshot {
        Some(s) => Ok(s.clone()),
        None => Err(JetServiceError::Policy(
            "no snapshot committed".to_string(),
        )),
    }
}

fn jet_services_append_event(
    tree: &mut JetServiceTree,
    event: String,
) -> Result<(), JetServiceError> {
    if tree.state_adapter != JetServiceStateAdapter::EventLog {
        return Err(JetServiceError::Policy(
            "append_event requires EventLog state adapter".to_string(),
        ));
    }
    tree.event_log.push(event);
    Ok(())
}

fn jet_services_event_count(tree: &JetServiceTree) -> i64 {
    tree.event_log.len() as i64
}

fn jet_services_replay_events(tree: &JetServiceTree) -> String {
    tree.event_log.join("|")
}

fn jet_services_workflow_start(
    tree: &mut JetServiceTree,
    id: String,
    version: i64,
) -> Result<i64, JetServiceError> {
    if version < 1 {
        return Err(JetServiceError::Policy(
            "workflow version must be >= 1".to_string(),
        ));
    }
    let run_id = (tree.workflows.len() as i64) + 1;
    tree.workflows.push(JetServiceWorkflow {
        id,
        run_id,
        version,
        steps: Vec::new(),
        history: vec![format!("start@v{version}")],
    });
    Ok(run_id)
}

fn jet_services_workflow_step(
    tree: &mut JetServiceTree,
    run_id: i64,
    step: String,
) -> Result<(), JetServiceError> {
    let wf = tree
        .workflows
        .iter_mut()
        .find(|w| w.run_id == run_id)
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))?;
    wf.steps.push(step.clone());
    wf.history.push(format!("step:{step}"));
    Ok(())
}

fn jet_services_workflow_history(
    tree: &JetServiceTree,
    run_id: i64,
) -> Result<String, JetServiceError> {
    tree.workflows
        .iter()
        .find(|w| w.run_id == run_id)
        .map(|w| w.history.join("|"))
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))
}

fn jet_services_directory_register(
    tree: &mut JetServiceTree,
    name: String,
    endpoint: JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    if endpoint.generation != tree.generation {
        return Err(JetServiceError::Policy(format!(
            "endpoint generation {} does not match tree generation {}",
            endpoint.generation, tree.generation
        )));
    }
    tree.directory.retain(|(n, _)| n != &name);
    tree.directory.push((name, endpoint));
    Ok(())
}

fn jet_services_directory_resolve(
    tree: &JetServiceTree,
    name: &String,
) -> Result<JetServiceEndpoint, JetServiceError> {
    tree.directory
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, ep)| ep.clone())
        .ok_or_else(|| JetServiceError::Unknown(format!("directory has no entry `{name}`")))
}

fn jet_services_directory_generation(tree: &JetServiceTree) -> i64 {
    tree.generation
}

fn jet_services_drain_worker(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    let name = {
        let worker = jet_services_find_worker_mut(tree, endpoint)?;
        worker.running = false;
        worker.name.clone()
    };
    if !tree.draining.iter().any(|n| n == &name) {
        tree.draining.push(name);
    }
    Ok(())
}

fn jet_services_handoff_generation(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    tree.previous_generation = tree.generation;
    tree.generation += 1;
    for worker in &mut tree.workers {
        worker.endpoint.generation = tree.generation;
    }
    tree.draining.clear();
    Ok(tree.generation)
}

fn jet_services_rollback_generation(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    if tree.previous_generation <= 0 {
        return Err(JetServiceError::Policy(
            "no previous generation to roll back to".to_string(),
        ));
    }
    tree.generation = tree.previous_generation;
    tree.previous_generation = 0;
    for worker in &mut tree.workers {
        worker.endpoint.generation = tree.generation;
    }
    Ok(tree.generation)
}

fn jet_services_chaos_fail(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    tree.chaos_fails += 1;
    Ok(tree.chaos_fails)
}

fn jet_services_observe(tree: &JetServiceTree) -> String {
    format!(
        "Observe(workers={}, started={}, generation={}, dead_letters={}, events={}, chaos={}, draining={})",
        tree.workers.len(),
        tree.started,
        tree.generation,
        tree.dead_letters.len(),
        tree.event_log.len(),
        tree.chaos_fails,
        tree.draining.len()
    )
}

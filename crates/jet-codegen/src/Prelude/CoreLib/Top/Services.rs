// ── D-SERVICE1=D (#444): sema-known structured service tree over taskgroups ─
// Beginner topology: named tree → workers with typed mailboxes → OneForOne
// restart policy. Delivery is at-most-once with Full under capacity (D-SERVICE-
// DELIVERY1). Engines marshal into these Prelude symbols only (I9).

const MAX_SERVICE_NAME: usize = 256;
const MAX_SERVICE_MESSAGE: usize = 1024 * 1024;
const MAX_SERVICE_WORKERS: usize = 4096;
const MAX_SERVICE_CAPACITY: i64 = 1_000_000;
const MAX_SERVICE_IDEMPOTENCY: usize = 100_000;
const MAX_SERVICE_DEAD_LETTERS: usize = 100_000;
const MAX_SERVICE_STATE_RECORDS: usize = 100_000;
const MAX_SERVICE_WORKFLOW_STEPS: usize = 100_000;
const MAX_SERVICE_MESSAGES: usize = 100_000;

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetServiceEndpoint {
    tree: String,
    worker: String,
    generation: i64,
}

struct JetServiceMailbox {
    endpoint: JetServiceEndpoint,
    capacity: i64,
    depth: i64,
    messages: Vec<String>,
    sender: std::sync::mpsc::SyncSender<String>,
    receiver: std::sync::mpsc::Receiver<String>,
}

impl Clone for JetServiceMailbox {
    fn clone(&self) -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel(
            usize::try_from(self.capacity.max(1)).unwrap_or(1),
        );
        for message in &self.messages {
            sender
                .try_send(message.clone())
                .expect("validated service mailbox must fit its bounded channel");
        }
        Self {
            endpoint: self.endpoint.clone(),
            capacity: self.capacity,
            depth: self.depth,
            messages: self.messages.clone(),
            sender,
            receiver,
        }
    }
}

impl std::fmt::Debug for JetServiceMailbox {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetServiceMailbox")
            .field("endpoint", &self.endpoint)
            .field("capacity", &self.capacity)
            .field("depth", &self.depth)
            .field("messages", &self.messages)
            .finish()
    }
}

fn jet_services_new_mailbox(
    endpoint: JetServiceEndpoint,
    capacity: i64,
    messages: Vec<String>,
) -> Result<JetServiceMailbox, JetServiceError> {
    let channel_capacity = usize::try_from(capacity).map_err(|_| {
        JetServiceError::Policy("mailbox capacity is outside the platform range".to_string())
    })?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(channel_capacity);
    if messages.len() > channel_capacity {
        return Err(JetServiceError::Policy(
            "mailbox messages exceed the bounded channel capacity".to_string(),
        ));
    }
    for message in &messages {
        sender.try_send(message.clone()).map_err(|_| {
            JetServiceError::Policy("mailbox channel could not restore its queued messages".to_string())
        })?;
    }
    let depth = i64::try_from(messages.len()).map_err(|_| {
        JetServiceError::Policy("mailbox depth is outside the platform range".to_string())
    })?;
    Ok(JetServiceMailbox {
        endpoint,
        capacity,
        depth,
        messages,
        sender,
        receiver,
    })
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
    idempotency_seen: Vec<(String, JetServiceEndpoint, String)>,
    directory: Vec<(String, JetServiceEndpoint, String)>,
    directory_key: Vec<u8>,
    draining: Vec<String>,
    workflows: Vec<JetServiceWorkflow>,
    task_group: std::sync::Arc<JetTaskGroupRuntime<String>>,
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
        directory_key: Vec::new(),
        draining: Vec::new(),
        workflows: Vec::new(),
        task_group: std::sync::Arc::new(JetTaskGroupRuntime::new()),
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
    for group in &mut tree.groups {
        group.restart = tree.restart.clone();
    }
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
    if capacity <= 0 || capacity > MAX_SERVICE_CAPACITY {
        return Err(JetServiceError::Policy(
            "mailbox capacity must be positive and bounded".to_string(),
        ));
    }
    if name.trim().is_empty()
        || name.chars().any(char::is_control)
        || name.len() > MAX_SERVICE_NAME
    {
        return Err(JetServiceError::Policy(
            "worker name must be non-empty and visible".to_string(),
        ));
    }
    if tree.workers.len() >= MAX_SERVICE_WORKERS {
        return Err(JetServiceError::Policy(
            "service tree worker limit exceeded".to_string(),
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
    let mailbox = jet_services_new_mailbox(endpoint.clone(), capacity, Vec::new())?;
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
    if name.trim().is_empty()
        || name.chars().any(char::is_control)
        || name.len() > MAX_SERVICE_NAME
        || workers.is_empty()
        || workers.len() > MAX_SERVICE_WORKERS
    {
        return Err(JetServiceError::Policy(
            "service group needs a visible name and at least one worker".to_string(),
        ));
    }
    if tree.groups.iter().any(|group| group.name == name) {
        return Err(JetServiceError::Policy(format!(
            "group `{name}` already exists"
        )));
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
    if tree.started {
        return Err(JetServiceError::Policy(
            "service tree is already started".to_string(),
        ));
    }
    jet_services_validate_tree(tree)?;
    if tree.name.trim().is_empty()
        || tree.name.chars().any(char::is_control)
        || tree.name.len() > MAX_SERVICE_NAME
    {
        return Err(JetServiceError::Policy(
            "service tree name must be non-empty and visible".to_string(),
        ));
    }
    if tree.workers.is_empty() {
        return Err(JetServiceError::Policy(
            "service tree has no workers".to_string(),
        ));
    }
    match &tree.state_adapter {
        JetServiceStateAdapter::Empty => {
            if tree.snapshot.is_some() || !tree.event_log.is_empty() {
                return Err(JetServiceError::Policy(
                    "Empty state adapter cannot contain durable records".to_string(),
                ));
            }
        }
        JetServiceStateAdapter::Snapshot => {
            if !tree.event_log.is_empty() {
                return Err(JetServiceError::Policy(
                    "Snapshot state adapter cannot contain event-log records".to_string(),
                ));
            }
            if let Some(snapshot) = &tree.snapshot {
                jet_services_read_state_record("v1:", snapshot)?;
            }
        }
        JetServiceStateAdapter::EventLog => {
            if tree.snapshot.is_some() {
                return Err(JetServiceError::Policy(
                    "EventLog state adapter cannot contain a snapshot".to_string(),
                ));
            }
            if tree.event_log.len() > MAX_SERVICE_STATE_RECORDS {
                return Err(JetServiceError::Policy(
                    "event-log record limit exceeded".to_string(),
                ));
            }
            for event in &tree.event_log {
                jet_services_read_state_record("v1:", event)?;
            }
        }
    }
    let group = std::sync::Arc::new(JetTaskGroupRuntime::new());
    for worker in &mut tree.workers {
        worker.running = true;
        group.register(worker.name.clone());
    }
    tree.task_group = group;
    tree.started = true;
    Ok(())
}

fn jet_services_stop(tree: &mut JetServiceTree) -> Result<(), JetServiceError> {
    let group = std::mem::replace(
        &mut tree.task_group,
        std::sync::Arc::new(JetTaskGroupRuntime::new()),
    );
    group.close_with(|_| {}, |_| {});
    let preserve_mailboxes = tree.delivery == JetServiceDelivery::DurableAtLeastOnce;
    for worker in &mut tree.workers {
        worker.running = false;
        if !preserve_mailboxes {
            while worker.mailbox.receiver.try_recv().is_ok() {}
            worker.mailbox.messages.clear();
        }
        worker.mailbox.depth = worker.mailbox.messages.len() as i64;
    }
    if tree.state_adapter == JetServiceStateAdapter::Empty {
        // Empty means a restart starts with no state.  Snapshot and EventLog
        // are the explicit durable alternatives.
        tree.snapshot = None;
        tree.event_log.clear();
        tree.dead_letters.clear();
        tree.idempotency_seen.clear();
        tree.directory.clear();
        tree.directory_key.clear();
        tree.workflows.clear();
    }
    tree.started = false;
    Ok(())
}

fn jet_services_restart_worker(worker: &mut JetServiceWorker, preserve_mailbox: bool) {
    worker.restarts = worker.restarts.saturating_add(1);
    worker.running = true;
    if !preserve_mailbox {
        while worker.mailbox.receiver.try_recv().is_ok() {}
        worker.mailbox.messages.clear();
    }
    worker.mailbox.depth = worker.mailbox.messages.len() as i64;
}

fn jet_services_validate_endpoint(
    tree: &JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    if endpoint.tree != tree.name || endpoint.generation != tree.generation {
        return Err(JetServiceError::Policy(format!(
            "stale service endpoint {}/{}@g{} (current generation {})",
            endpoint.tree, endpoint.worker, endpoint.generation, tree.generation
        )));
    }
    Ok(())
}

fn jet_services_validate_tree(tree: &JetServiceTree) -> Result<(), JetServiceError> {
    if tree.name.trim().is_empty()
        || tree.name.chars().any(char::is_control)
        || tree.name.len() > MAX_SERVICE_NAME
        || tree.generation < 1
        || tree.previous_generation < 0
        || tree.workers.len() > MAX_SERVICE_WORKERS
        || tree.groups.len() > MAX_SERVICE_WORKERS
        || tree.dead_letters.len() > MAX_SERVICE_DEAD_LETTERS
        || tree.event_log.len() > MAX_SERVICE_STATE_RECORDS
        || tree.workflows.len() > MAX_SERVICE_WORKFLOW_STEPS
    {
        return Err(JetServiceError::Policy(
            "service tree metadata is invalid or exceeds its limit".to_string(),
        ));
    }
    let worker_exists = |name: &str, generation: i64| {
        tree.workers.iter().any(|worker| {
            worker.name == name
                && worker.endpoint.tree == tree.name
                && worker.endpoint.generation == generation
        })
    };
    for worker in &tree.workers {
        if worker.name.trim().is_empty()
            || worker.name.chars().any(char::is_control)
            || worker.name.len() > MAX_SERVICE_NAME
            || worker.endpoint.tree != tree.name
            || worker.endpoint.worker != worker.name
            || worker.endpoint.generation != tree.generation
            || worker.restarts < 0
            || worker.mailbox.endpoint != worker.endpoint
            || worker.mailbox.capacity <= 0
            || worker.mailbox.capacity > MAX_SERVICE_CAPACITY
            || worker.mailbox.depth < 0
            || worker.mailbox.depth as usize != worker.mailbox.messages.len()
            || worker.mailbox.messages.len() > MAX_SERVICE_MESSAGES
            || worker.mailbox.messages.len() > worker.mailbox.capacity as usize
            || worker
                .mailbox
                .messages
                .iter()
                .any(|message| {
                    message.len() > MAX_SERVICE_MESSAGE || message.chars().any(char::is_control)
                })
        {
            return Err(JetServiceError::Policy(
                "service worker or mailbox state is invalid".to_string(),
            ));
        }
    }
    for group in &tree.groups {
        if group.name.trim().is_empty()
            || group.name.chars().any(char::is_control)
            || group.name.len() > MAX_SERVICE_NAME
            || group.workers.is_empty()
            || group.workers.len() > MAX_SERVICE_WORKERS
            || group
                .workers
                .iter()
                .any(|name| !worker_exists(name, tree.generation))
        {
            return Err(JetServiceError::Policy(
                "service group state is invalid".to_string(),
            ));
        }
    }
    match &tree.state_adapter {
        JetServiceStateAdapter::Empty => {
            if tree.snapshot.is_some() || !tree.event_log.is_empty() {
                return Err(JetServiceError::Policy(
                    "Empty state adapter cannot contain durable records".to_string(),
                ));
            }
        }
        JetServiceStateAdapter::Snapshot => {
            if !tree.event_log.is_empty() {
                return Err(JetServiceError::Policy(
                    "Snapshot state adapter cannot contain event-log records".to_string(),
                ));
            }
            if let Some(snapshot) = &tree.snapshot {
                if snapshot.len() > MAX_SERVICE_MESSAGE {
                    return Err(JetServiceError::Policy(
                        "snapshot record exceeds the state record limit".to_string(),
                    ));
                }
                jet_services_read_state_record("v1:", snapshot)?;
            }
        }
        JetServiceStateAdapter::EventLog => {
            if tree.snapshot.is_some() {
                return Err(JetServiceError::Policy(
                    "EventLog state adapter cannot contain a snapshot".to_string(),
                ));
            }
            for event in &tree.event_log {
                if event.len() > MAX_SERVICE_MESSAGE {
                    return Err(JetServiceError::Policy(
                        "event record exceeds the state record limit".to_string(),
                    ));
                }
                jet_services_read_state_record("v1:", event)?;
            }
        }
    }
    let mut idempotency_keys = Vec::new();
    for (key, endpoint, message) in &tree.idempotency_seen {
        if key.is_empty()
            || key.len() > MAX_SERVICE_NAME
            || key.chars().any(char::is_control)
            || message.len() > MAX_SERVICE_MESSAGE
            || message.chars().any(char::is_control)
            || endpoint.tree != tree.name
            || endpoint.generation != tree.generation
            || !worker_exists(&endpoint.worker, endpoint.generation)
            || idempotency_keys.iter().any(|seen| seen == key)
        {
            return Err(JetServiceError::Policy(
                "durable idempotency state is invalid".to_string(),
            ));
        }
        idempotency_keys.push(key.clone());
    }
    if tree.directory_key.len() != 0 && tree.directory_key.len() != 32 {
        return Err(JetServiceError::Policy(
            "service directory key must be 32 bytes".to_string(),
        ));
    }
    if tree.directory.len() > MAX_SERVICE_WORKERS {
        return Err(JetServiceError::Policy(
            "service directory entry limit exceeded".to_string(),
        ));
    }
    let mut directory_names = Vec::new();
    for (name, endpoint, signature) in &tree.directory {
        if tree.directory_key.is_empty()
            || name.trim().is_empty()
            || name.chars().any(char::is_control)
            || name.len() > MAX_SERVICE_NAME
            || endpoint.tree != tree.name
            || endpoint.generation != tree.generation
            || !worker_exists(&endpoint.worker, endpoint.generation)
            || signature.len() != 64
            || !signature.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || directory_names.iter().any(|seen| seen == name)
            || *signature != jet_services_directory_signature(tree, name, endpoint)
        {
            return Err(JetServiceError::Policy(
                "service directory state is invalid".to_string(),
            ));
        }
        directory_names.push(name.clone());
    }
    if tree.draining.len() > tree.workers.len()
        || tree
            .draining
            .iter()
            .any(|name| !worker_exists(name, tree.generation))
    {
        return Err(JetServiceError::Policy(
            "service draining state is invalid".to_string(),
        ));
    }
    let mut workflow_ids = Vec::new();
    for workflow in &tree.workflows {
        if workflow.id.trim().is_empty()
            || workflow.id.chars().any(char::is_control)
            || workflow.id.len() > MAX_SERVICE_NAME
            || workflow.run_id <= 0
            || workflow.version < 1
            || workflow.steps.len() > MAX_SERVICE_WORKFLOW_STEPS
            || workflow.history.len() > MAX_SERVICE_WORKFLOW_STEPS
            || workflow.steps.iter().any(|step| {
                step.trim().is_empty()
                    || step.chars().any(char::is_control)
                    || step.len() > MAX_SERVICE_MESSAGE
            })
            || workflow.history.iter().any(|entry| {
                entry.trim().is_empty()
                    || entry.chars().any(char::is_control)
                    || entry.len() > MAX_SERVICE_MESSAGE
            })
            || workflow_ids.iter().any(|id| id == &workflow.id)
        {
            return Err(JetServiceError::Policy(
                "service workflow state is invalid".to_string(),
            ));
        }
        workflow_ids.push(workflow.id.clone());
    }
    if tree.chaos_fails < 0 {
        return Err(JetServiceError::Policy(
            "service chaos counter is invalid".to_string(),
        ));
    }
    Ok(())
}

fn jet_services_find_worker_mut<'a>(
    tree: &'a mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<&'a mut JetServiceWorker, JetServiceError> {
    jet_services_validate_endpoint(tree, endpoint)?;
    tree.workers
        .iter_mut()
        .find(|w| {
            w.name == endpoint.worker
                && w.endpoint.tree == endpoint.tree
                && w.endpoint.generation == endpoint.generation
        })
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
    if message.len() > MAX_SERVICE_MESSAGE || message.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "service message exceeds the 1 MiB limit".to_string(),
        ));
    }
    let draining = tree.draining.iter().any(|name| name == &endpoint.worker);
    let worker = jet_services_find_worker_mut(tree, endpoint)?;
    if !worker.running || draining {
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
    match worker.mailbox.sender.try_send(message.clone()) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            return Err(JetServiceError::Full(format!(
                "mailbox for `{}` is full (capacity {})",
                worker.name, worker.mailbox.capacity
            )));
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            return Err(JetServiceError::NotStarted(format!(
                "mailbox for `{}` is closed",
                worker.name
            )));
        }
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
    let draining = tree.draining.iter().any(|name| name == &endpoint.worker);
    let (message, should_stop) = {
        let worker = jet_services_find_worker_mut(tree, endpoint)?;
        if !worker.running && !draining {
        return Err(JetServiceError::NotStarted(format!(
            "worker `{}` is not running",
            worker.name
        )));
    }
        let message = match worker.mailbox.receiver.try_recv() {
        Ok(message) => message,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            return Err(JetServiceError::Ambiguous(format!(
                "mailbox for `{}` is empty",
                worker.name
            )));
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            return Err(JetServiceError::NotStarted(format!(
                "mailbox for `{}` is closed",
                worker.name
            )));
        }
    };
        let mirrored = worker.mailbox.messages.first().cloned().ok_or_else(|| {
        JetServiceError::Policy("mailbox channel and durable mirror diverged".to_string())
    })?;
        if mirrored != message {
        return Err(JetServiceError::Policy(
            "mailbox channel and durable mirror diverged".to_string(),
        ));
    }
        worker.mailbox.messages.remove(0);
        worker.mailbox.depth = worker.mailbox.messages.len() as i64;
        let should_stop = worker.mailbox.messages.is_empty();
        (message, should_stop)
    };
    if draining && should_stop {
        if let Ok(worker) = jet_services_find_worker_mut(tree, endpoint) {
            worker.running = false;
        }
        tree.draining.retain(|name| name != &endpoint.worker);
    }
    Ok(message)
}

fn jet_services_mailbox_depth(
    tree: &JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<i64, JetServiceError> {
    jet_services_validate_endpoint(tree, endpoint)?;
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
    jet_services_validate_endpoint(tree, endpoint)?;
    let restart = tree.restart.clone();
    let preserve_mailboxes = tree.delivery == JetServiceDelivery::DurableAtLeastOnce;
    let worker_name = endpoint.worker.clone();
    match restart {
        JetServiceRestart::OneForOne => {
            let worker = jet_services_find_worker_mut(tree, endpoint)?;
            jet_services_restart_worker(worker, preserve_mailboxes);
        }
        JetServiceRestart::OneForAll => {
            for worker in &mut tree.workers {
                jet_services_restart_worker(worker, preserve_mailboxes);
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
                jet_services_restart_worker(worker, preserve_mailboxes);
            }
        }
    }
    Ok(())
}

fn jet_services_restarts(
    tree: &JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<i64, JetServiceError> {
    jet_services_validate_endpoint(tree, endpoint)?;
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
    if idempotency_key.is_empty()
        || idempotency_key.len() > MAX_SERVICE_NAME
        || idempotency_key.chars().any(char::is_control)
    {
        return Err(JetServiceError::Policy(
            "durable send requires a non-empty idempotency key".to_string(),
        ));
    }
    if message.len() > MAX_SERVICE_MESSAGE || message.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "service message exceeds the 1 MiB limit".to_string(),
        ));
    }
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    jet_services_validate_endpoint(tree, endpoint)?;
    if let Some((_, previous_endpoint, previous_message)) = tree
        .idempotency_seen
        .iter()
        .find(|(key, _, _)| key == &idempotency_key)
    {
        if previous_endpoint == endpoint && previous_message == &message {
            return Ok(());
        }
        return Err(JetServiceError::Policy(format!(
            "idempotency key `{idempotency_key}` was already used for a different delivery"
        )));
    }
    if tree.idempotency_seen.len() >= MAX_SERVICE_IDEMPOTENCY {
        return Err(JetServiceError::Policy(
            "durable idempotency table is full".to_string(),
        ));
    }
    match jet_services_send(tree, endpoint, message.clone()) {
        Ok(()) => {
            tree.idempotency_seen
                .push((idempotency_key, endpoint.clone(), message));
            Ok(())
        }
        Err(JetServiceError::Full(m)) => {
            if tree.dead_letters.len() < MAX_SERVICE_DEAD_LETTERS {
                tree.dead_letters.push(format!("{idempotency_key}:{m}"));
            }
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

fn jet_services_state_record(prefix: &str, payload: &str) -> String {
    format!("{prefix}{}:{payload}", payload.len())
}

fn jet_services_read_state_record<'a>(
    prefix: &str,
    record: &'a str,
) -> Result<&'a str, JetServiceError> {
    let body = record.strip_prefix(prefix).ok_or_else(|| {
        JetServiceError::Policy("state record schema is incompatible".to_string())
    })?;
    let (length, payload) = body.split_once(':').ok_or_else(|| {
        JetServiceError::Policy("state record is truncated".to_string())
    })?;
    let expected = length.parse::<usize>().map_err(|_| {
        JetServiceError::Policy("state record length is invalid".to_string())
    })?;
    if expected != payload.len() {
        return Err(JetServiceError::Policy(
            "state record length does not match payload".to_string(),
        ));
    }
    Ok(payload)
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
    let record = jet_services_state_record("v1:", &payload);
    if record.len() > MAX_SERVICE_MESSAGE {
        return Err(JetServiceError::Policy(
            "snapshot record exceeds the state record limit".to_string(),
        ));
    }
    tree.snapshot = Some(record);
    Ok(())
}

fn jet_services_restore_snapshot(tree: &JetServiceTree) -> Result<String, JetServiceError> {
    match &tree.snapshot {
        Some(s) => Ok(jet_services_read_state_record("v1:", s)?.to_string()),
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
    let record = jet_services_state_record("v1:", &event);
    if record.len() > MAX_SERVICE_MESSAGE {
        return Err(JetServiceError::Policy(
            "event record exceeds the state record limit".to_string(),
        ));
    }
    if tree.event_log.len() >= MAX_SERVICE_STATE_RECORDS {
        return Err(JetServiceError::Policy(
            "event-log record limit exceeded".to_string(),
        ));
    }
    tree.event_log.push(record);
    Ok(())
}

fn jet_services_event_count(tree: &JetServiceTree) -> i64 {
    tree.event_log.len() as i64
}

fn jet_services_replay_events(tree: &JetServiceTree) -> String {
    let mut events = Vec::with_capacity(tree.event_log.len());
    for record in &tree.event_log {
        match jet_services_read_state_record("v1:", record) {
            Ok(event) => events.push(event),
            Err(_) => return "StateReplayError(invalid_record)".to_string(),
        }
    }
    events.join("|")
}

fn jet_services_workflow_start(
    tree: &mut JetServiceTree,
    id: String,
    version: i64,
) -> Result<i64, JetServiceError> {
    if id.trim().is_empty()
        || id.chars().any(char::is_control)
        || id.len() > MAX_SERVICE_NAME
    {
        return Err(JetServiceError::Policy(
            "workflow id must be non-empty and visible".to_string(),
        ));
    }
    if version < 1 {
        return Err(JetServiceError::Policy(
            "workflow version must be >= 1".to_string(),
        ));
    }
    if let Some(existing) = tree
        .workflows
        .iter()
        .find(|workflow| workflow.id == id && workflow.version == version)
    {
        return Ok(existing.run_id);
    }
    if tree
        .workflows
        .iter()
        .any(|workflow| workflow.id == id && workflow.version != version)
    {
        return Err(JetServiceError::Policy(format!(
            "workflow `{id}` already has a different active version"
        )));
    }
    let run_id = i64::try_from(tree.workflows.len())
        .ok()
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| JetServiceError::Policy("workflow run id exhausted".to_string()))?;
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
    if step.trim().is_empty() || step.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "workflow step must be non-empty and visible".to_string(),
        ));
    }
    let wf = tree
        .workflows
        .iter_mut()
        .find(|w| w.run_id == run_id)
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))?;
    if step.len() > MAX_SERVICE_MESSAGE || wf.steps.len() >= MAX_SERVICE_WORKFLOW_STEPS {
        return Err(JetServiceError::Policy(
            "workflow step history limit exceeded".to_string(),
        ));
    }
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
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    if name.trim().is_empty()
        || name.chars().any(char::is_control)
        || name.len() > MAX_SERVICE_NAME
    {
        return Err(JetServiceError::Policy(
            "directory name must be non-empty and visible".to_string(),
        ));
    }
    if endpoint.generation != tree.generation {
        return Err(JetServiceError::Policy(format!(
            "endpoint generation {} does not match tree generation {}",
            endpoint.generation, tree.generation
        )));
    }
    let worker = tree
        .workers
        .iter()
        .find(|worker| worker.endpoint == endpoint)
        .ok_or_else(|| {
            JetServiceError::Unknown(format!(
                "endpoint {}/{} is not a worker in this generation",
                endpoint.tree, endpoint.worker
            ))
        })?;
    if !worker.running {
        return Err(JetServiceError::NotStarted(format!(
            "worker `{}` is not running",
            worker.name
        )));
    }
    if tree.directory_key.is_empty() {
        tree.directory_key = jet_crypto_entropy_bytes(32).map_err(|_| {
            JetServiceError::Policy(
                "service directory cannot obtain an authentication key".to_string(),
            )
        })?;
    }
    if tree.directory.len() >= MAX_SERVICE_WORKERS
        && !tree.directory.iter().any(|(entry, _, _)| entry == &name)
    {
        return Err(JetServiceError::Policy(
            "service directory entry limit exceeded".to_string(),
        ));
    }
    let signature = jet_services_directory_signature(tree, &name, &endpoint);
    tree.directory.retain(|(n, _, _)| n != &name);
    tree.directory.push((name, endpoint, signature));
    Ok(())
}

fn jet_services_directory_resolve(
    tree: &JetServiceTree,
    name: &String,
) -> Result<JetServiceEndpoint, JetServiceError> {
    tree.directory
        .iter()
        .find(|(entry, endpoint, signature)| {
            entry == name
                && endpoint.generation == tree.generation
                && tree.workers.iter().any(|worker| {
                    worker.running && worker.endpoint == *endpoint
                })
                && signature == &jet_services_directory_signature(tree, entry, endpoint)
        })
        .map(|(_, ep, _)| ep.clone())
        .ok_or_else(|| JetServiceError::Unknown(format!("directory has no valid entry `{name}`")))
}

fn jet_services_directory_signature(
    tree: &JetServiceTree,
    name: &str,
    endpoint: &JetServiceEndpoint,
) -> String {
    jet_services_directory_signature_parts(&tree.name, &tree.directory_key, name, endpoint)
}

fn jet_services_directory_signature_parts(
    tree_name: &str,
    directory_key: &[u8],
    name: &str,
    endpoint: &JetServiceEndpoint,
) -> String {
    let mut input = Vec::new();
    let generation = endpoint.generation.to_string();
    for field in [
        "jet-service-directory-v1",
        tree_name,
        name,
        &endpoint.tree,
        &endpoint.worker,
        generation.as_str(),
    ] {
        input.extend_from_slice(&(field.len() as u64).to_be_bytes());
        input.extend_from_slice(field.as_bytes());
    }
    input.extend_from_slice(directory_key);
    jet_sha256_raw(&input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn jet_services_directory_generation(tree: &JetServiceTree) -> i64 {
    tree.generation
}

fn jet_services_drain_worker(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    let name = {
        let worker = jet_services_find_worker_mut(tree, endpoint)?;
        worker.name.clone()
    };
    if !tree.draining.iter().any(|n| n == &name) {
        tree.draining.push(name);
    }
    Ok(())
}

fn jet_services_handoff_generation(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    for worker in &tree.workers {
        if tree.draining.iter().any(|name| name == &worker.name)
            && !worker.mailbox.messages.is_empty()
        {
            return Err(JetServiceError::Policy(format!(
                "cannot hand off shard `{}` with {} queued message(s)",
                worker.name,
                worker.mailbox.messages.len()
            )));
        }
    }
    tree.previous_generation = tree.generation;
    tree.generation = tree
        .generation
        .checked_add(1)
        .ok_or_else(|| JetServiceError::Policy("service generation exhausted".to_string()))?;
    for worker in &mut tree.workers {
        if tree.draining.iter().any(|name| name == &worker.name) {
            worker.running = true;
        }
        worker.endpoint.generation = tree.generation;
        worker.mailbox.endpoint.generation = tree.generation;
    }
    let tree_name = tree.name.clone();
    let directory_key = tree.directory_key.clone();
    for (name, endpoint, signature) in &mut tree.directory {
        endpoint.generation = tree.generation;
        *signature = jet_services_directory_signature_parts(
            &tree_name,
            &directory_key,
            name,
            endpoint,
        );
    }
    // Durable delivery keys identify the logical operation, not the worker
    // incarnation.  Rebind their recorded endpoint with the generation so a
    // post-handoff retry remains idempotent and the state validator never
    // accepts a stale endpoint hidden inside durable state.
    for (_, endpoint, _) in &mut tree.idempotency_seen {
        endpoint.generation = tree.generation;
    }
    tree.draining.clear();
    Ok(tree.generation)
}

fn jet_services_rollback_generation(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    if tree.previous_generation <= 0 {
        return Err(JetServiceError::Policy(
            "no previous generation to roll back to".to_string(),
        ));
    }
    tree.generation = tree.previous_generation;
    tree.previous_generation = 0;
    for worker in &mut tree.workers {
        worker.endpoint.generation = tree.generation;
        worker.mailbox.endpoint.generation = tree.generation;
    }
    let tree_name = tree.name.clone();
    let directory_key = tree.directory_key.clone();
    for (name, endpoint, signature) in &mut tree.directory {
        endpoint.generation = tree.generation;
        *signature = jet_services_directory_signature_parts(
            &tree_name,
            &directory_key,
            name,
            endpoint,
        );
    }
    for (_, endpoint, _) in &mut tree.idempotency_seen {
        endpoint.generation = tree.generation;
    }
    Ok(tree.generation)
}

fn jet_services_chaos_fail(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    tree.chaos_fails = tree
        .chaos_fails
        .checked_add(1)
        .ok_or_else(|| JetServiceError::Policy("chaos failure counter exhausted".to_string()))?;
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

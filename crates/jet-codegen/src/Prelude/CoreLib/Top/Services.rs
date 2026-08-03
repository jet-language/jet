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
const MAX_SERVICE_STATE_SCHEMA: usize = 256;
const MAX_SERVICE_STATE_STORE: usize = 4096;
const MAX_SERVICE_STATE_BYTES: usize = 128 * 1024 * 1024;
const MAX_SERVICE_WORKFLOW_STEPS: usize = 100_000;
const MAX_SERVICE_ACTIVITY_ATTEMPTS: i64 = 1_000;
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetServiceMigration {
    Reversible,
    DualWrite,
    ForwardOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetServiceStateStore {
    path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetServiceStateAuthority {
    store: String,
    schema: String,
    version: i64,
    migration: String,
    adapter: JetServiceStateAdapter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetServiceUpgradeReceipt {
    from_generation: i64,
    to_generation: i64,
    migration: String,
    rollback_store: String,
    rollback_available: bool,
    pinned_shards: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    authority: String,
    generation: i64,
    delivery: JetServiceDelivery,
    restart: JetServiceRestart,
    workers: Vec<JetServiceWorker>,
    groups: Vec<JetServiceGroup>,
    started: bool,
    state_adapter: JetServiceStateAdapter,
    state_authority: Option<JetServiceStateAuthority>,
    snapshot: Option<String>,
    event_log: Vec<String>,
    dead_letters: Vec<String>,
    idempotency_seen: Vec<(String, JetServiceEndpoint, String)>,
    directory: Vec<(String, JetServiceEndpoint, String)>,
    directory_key: Vec<u8>,
    draining: Vec<String>,
    partitioned: Vec<String>,
    workflows: Vec<JetServiceWorkflow>,
    task_group: std::sync::Arc<JetTaskGroupRuntime<String>>,
    chaos_fails: i64,
    previous_generation: i64,
    last_upgrade: Option<JetServiceUpgradeReceipt>,
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

impl JetShow for JetServiceMigration {
    fn jet_show(&self) -> String {
        jet_services_migration_name(self).to_string()
    }
}

impl JetShow for JetServiceStateAuthority {
    fn jet_show(&self) -> String {
        format!(
            "ServiceStateAuthority(store={}, schema={}, version={}, migration={}, adapter={})",
            self.store,
            self.schema,
            self.version,
            self.migration,
            jet_services_state_adapter_name(&self.adapter),
        )
    }
}

impl JetShow for JetServiceStateStore {
    fn jet_show(&self) -> String {
        format!("ServiceStateStore({})", self.path)
    }
}

impl JetShow for JetServiceUpgradeReceipt {
    fn jet_show(&self) -> String {
        format!(
            "ServiceUpgradeReceipt(from={}, to={}, migration={}, rollback_available={}, pinned={})",
            self.from_generation,
            self.to_generation,
            self.migration,
            self.rollback_available,
            self.pinned_shards.join(","),
        )
    }
}

impl JetDisplay for JetServiceUpgradeReceipt {
    fn jet_display(&self) -> String {
        self.jet_show()
    }
}

impl JetDebug for JetServiceUpgradeReceipt {
    fn jet_debug(&self) -> String {
        self.jet_show()
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

impl JetShow for JetServiceRuntime {
    fn jet_show(&self) -> String {
        format!("ServiceRuntime(store={}, retention_ms={})", self.store, self.retention_ms)
    }
}

impl JetShow for JetServiceReceipt {
    fn jet_show(&self) -> String {
        match self {
            JetServiceReceipt::Accepted(id) => format!("Accepted({id})"),
            JetServiceReceipt::Duplicate(id) => format!("Duplicate({id})"),
            JetServiceReceipt::Retained { id, until } => format!("Retained({id}, {until})"),
            JetServiceReceipt::DeadLettered(id) => format!("DeadLettered({id})"),
            JetServiceReceipt::Rejected(reason) => format!("Rejected({reason})"),
            JetServiceReceipt::Unavailable(reason) => format!("Unavailable({reason})"),
        }
    }
}

impl JetShow for JetServiceError {
    fn jet_show(&self) -> String {
        match self {
            JetServiceError::Full(m)
            | JetServiceError::Ambiguous(m)
            | JetServiceError::Unknown(m)
            | JetServiceError::NotStarted(m)
            | JetServiceError::Policy(m)
            | JetServiceError::Unavailable(m)
            | JetServiceError::Partitioned(m)
            | JetServiceError::Revoked(m)
            | JetServiceError::Stale(m)
            | JetServiceError::Expired(m) => m.clone(),
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
        authority: String::new(),
        name,
        generation: 1,
        delivery: JetServiceDelivery::AtMostOnce,
        restart: JetServiceRestart::OneForOne,
        workers: Vec::new(),
        groups: Vec::new(),
        started: false,
        state_adapter: JetServiceStateAdapter::Empty,
        state_authority: None,
        snapshot: None,
        event_log: Vec::new(),
        dead_letters: Vec::new(),
        idempotency_seen: Vec::new(),
        directory: Vec::new(),
        directory_key: Vec::new(),
        draining: Vec::new(),
        partitioned: Vec::new(),
        workflows: Vec::new(),
        task_group: std::sync::Arc::new(JetTaskGroupRuntime::new()),
        chaos_fails: 0,
        previous_generation: 0,
        last_upgrade: None,
    }
}

pub fn jet_services_state_store(path: String) -> Result<JetServiceStateStore, JetServiceError> {
    service_authority_validate_text(
        &path,
        "service state store",
        MAX_SERVICE_STATE_STORE,
        false,
    )?;
    jet_services_state_validate_path(std::path::Path::new(&path))?;
    Ok(JetServiceStateStore { path })
}

fn jet_services_state_authority(
    store: &JetServiceStateStore,
    schema: String,
    version: i64,
) -> Result<JetServiceStateAuthority, JetServiceError> {
    jet_services_state_authority_with_migration(
        store.path.clone(),
        schema,
        version,
        JetServiceMigration::Reversible,
    )
}

fn jet_services_state_authority_with_migration(
    store: String,
    schema: String,
    version: i64,
    migration: JetServiceMigration,
) -> Result<JetServiceStateAuthority, JetServiceError> {
    service_authority_validate_text(
        &store,
        "service state store",
        MAX_SERVICE_STATE_STORE,
        false,
    )?;
    service_authority_validate_text(
        &schema,
        "service state schema",
        MAX_SERVICE_STATE_SCHEMA,
        false,
    )?;
    if version < 1 {
        return Err(JetServiceError::Policy(
            "service state schema version must be positive".to_string(),
        ));
    }
    Ok(JetServiceStateAuthority {
        store,
        schema,
        version,
        migration: jet_services_migration_name(&migration).to_string(),
        adapter: JetServiceStateAdapter::Empty,
    })
}

fn jet_services_migration_name(migration: &JetServiceMigration) -> &'static str {
    match migration {
        JetServiceMigration::Reversible => "reversible",
        JetServiceMigration::DualWrite => "dual_write",
        JetServiceMigration::ForwardOnly => "forward_only",
    }
}

fn jet_services_migration_reversible() -> JetServiceMigration {
    JetServiceMigration::Reversible
}

fn jet_services_migration_dual_write() -> JetServiceMigration {
    JetServiceMigration::DualWrite
}

fn jet_services_migration_forward_only() -> JetServiceMigration {
    JetServiceMigration::ForwardOnly
}

fn jet_services_attach_state_authority(
    authority: &JetServiceStateAuthority,
    adapter: JetServiceStateAdapter,
) -> Result<JetServiceStateAuthority, JetServiceError> {
    let migration = match authority.migration.as_str() {
        "reversible" => JetServiceMigration::Reversible,
        "dual_write" => JetServiceMigration::DualWrite,
        "forward_only" => JetServiceMigration::ForwardOnly,
        _ => {
            return Err(JetServiceError::Policy(
                "service state authority has an incompatible migration policy".to_string(),
            ))
        }
    };
    if authority.adapter != JetServiceStateAdapter::Empty && authority.adapter != adapter {
        return Err(JetServiceError::Policy(
            "service state authority has an incompatible adapter".to_string(),
        ));
    }
    let checked = jet_services_state_authority_with_migration(
        authority.store.clone(),
        authority.schema.clone(),
        authority.version,
        migration,
    )?;
    Ok(JetServiceStateAuthority { adapter, ..checked })
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
    if tree.authority.is_empty() {
        tree.authority = service_authority_provider_issue()?;
    }
    let endpoint = service_authority_endpoint_unchecked(
        tree.name.clone(),
        name.clone(),
        tree.generation,
        tree.authority.clone(),
    )?;
    let mailbox = jet_services_new_mailbox(endpoint.clone(), capacity, Vec::new())?;
    // Build the local mailbox before publishing the endpoint.  A failed
    // channel allocation must not leave a ghost authority in the registry.
    service_authority_register(&endpoint, false)?;
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
    if workers
        .iter()
        .enumerate()
        .any(|(index, worker)| workers[..index].iter().any(|seen| seen == worker))
    {
        return Err(JetServiceError::Policy(format!(
            "group `{name}` lists a worker more than once"
        )));
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
    let durable_records = match (&tree.state_adapter, &tree.state_authority) {
        (JetServiceStateAdapter::Empty, None) => Vec::new(),
        (JetServiceStateAdapter::Snapshot, Some(authority)) => {
            jet_services_state_store_load(authority, &JetServiceStateAdapter::Snapshot)?
        }
        (JetServiceStateAdapter::EventLog, Some(authority)) => {
            jet_services_state_store_load(authority, &JetServiceStateAdapter::EventLog)?
        }
        (JetServiceStateAdapter::Empty, Some(_))
        | (JetServiceStateAdapter::Snapshot, None)
        | (JetServiceStateAdapter::EventLog, None) => {
            return Err(JetServiceError::Policy(
                "durable service state needs its injected authority".to_string(),
            ));
        }
    };
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
            if durable_records.len() > 1 {
                return Err(jet_services_state_error(
                    "snapshot store contains more than one record",
                ));
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
        }
    }
    match &tree.state_adapter {
        JetServiceStateAdapter::Snapshot => {
            tree.snapshot = durable_records.into_iter().next();
        }
        JetServiceStateAdapter::EventLog => {
            tree.event_log = durable_records;
        }
        JetServiceStateAdapter::Empty => {}
    }
    if tree.state_authority.is_some() {
        let durable_workflows = jet_services_workflow_store_load(tree)?;
        if !tree.workflows.is_empty() && tree.workflows != durable_workflows {
            return Err(jet_services_state_error(
                "in-memory workflow history does not match the durable workflow log",
            ));
        }
        tree.workflows = durable_workflows;
    }
    let group = std::sync::Arc::new(JetTaskGroupRuntime::new());
    let mut activated = Vec::with_capacity(tree.workers.len());
    for worker in &mut tree.workers {
        worker.running = !tree.partitioned.iter().any(|name| name == &worker.name);
        group.register(worker.name.clone());
        let result = match jet_services_authority_update(&worker.endpoint, worker.running) {
            Ok(()) => Ok(()),
            Err(JetServiceError::Partitioned(_)) | Err(JetServiceError::Unavailable(_)) => {
                service_authority_register(&worker.endpoint, worker.running)
            }
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            let mut rollback_error = None;
            for endpoint in &activated {
                if let Err(error) = jet_services_authority_update(endpoint, false) {
                    rollback_error = Some(error);
                    break;
                }
            }
            for worker in &mut tree.workers {
                worker.running = false;
            }
            return Err(match rollback_error {
                Some(rollback_error) => JetServiceError::Policy(format!(
                    "{}; service authority activation rollback failed: {}",
                    error.jet_show(),
                    rollback_error.jet_show()
                )),
                None => error,
            });
        }
        activated.push(worker.endpoint.clone());
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
        jet_services_authority_update(&worker.endpoint, false)?;
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
        tree.partitioned.clear();
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
    jet_services_authority_validate(endpoint)?;
    if endpoint.authority != tree.authority || endpoint.tree != tree.name {
        return Err(JetServiceError::Revoked(format!(
            "service endpoint {}/{} is not issued by this authority",
            endpoint.tree, endpoint.worker
        )));
    }
    if endpoint.generation != tree.generation {
        return Err(JetServiceError::Stale(format!(
            "service endpoint {}/{}@g{} is stale (current generation {})",
            endpoint.tree, endpoint.worker, endpoint.generation, tree.generation
        )));
    }
    if tree.partitioned.iter().any(|name| name == &endpoint.worker) {
        return Err(JetServiceError::Partitioned(format!(
            "service endpoint {}/{} is partitioned",
            endpoint.tree, endpoint.worker
        )));
    }
    Ok(())
}

fn jet_services_validate_tree(tree: &JetServiceTree) -> Result<(), JetServiceError> {
    if tree.name.trim().is_empty()
        || tree.name.chars().any(char::is_control)
        || tree.name.len() > MAX_SERVICE_NAME
        || (tree.authority.trim().is_empty() && (tree.started || !tree.workers.is_empty()))
        || tree.authority.len() > MAX_SERVICE_NAME
        || tree.authority.chars().any(char::is_control)
        || tree.generation < 1
        || tree.previous_generation < 0
        || tree.workers.len() > MAX_SERVICE_WORKERS
        || tree.groups.len() > MAX_SERVICE_WORKERS
        || tree.idempotency_seen.len() > MAX_SERVICE_IDEMPOTENCY
        || tree.dead_letters.len() > MAX_SERVICE_DEAD_LETTERS
        || tree.event_log.len() > MAX_SERVICE_STATE_RECORDS
        || tree.partitioned.len() > MAX_SERVICE_WORKERS
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
                && worker.endpoint.authority == tree.authority
                && worker.endpoint.generation == generation
        })
    };
    let mut worker_names = Vec::with_capacity(tree.workers.len());
    for worker in &tree.workers {
        if worker.name.trim().is_empty()
            || worker.name.chars().any(char::is_control)
            || worker.name.len() > MAX_SERVICE_NAME
            || worker.endpoint.tree != tree.name
            || worker.endpoint.worker != worker.name
            || worker.endpoint.authority != tree.authority
            || worker.endpoint.generation != tree.generation
            || worker.restarts < 0
            || (tree.partitioned.iter().any(|name| name == &worker.name) && worker.running)
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
        if worker_names.iter().any(|seen| seen == &worker.name) {
            return Err(JetServiceError::Policy(
                "service worker names must be unique".to_string(),
            ));
        }
        worker_names.push(worker.name.clone());
    }
    if tree.partitioned.iter().any(|name| {
        name.trim().is_empty()
            || name.chars().any(char::is_control)
            || name.len() > MAX_SERVICE_NAME
            || !worker_exists(name, tree.generation)
    }) || tree.partitioned.iter().enumerate().any(|(index, name)| {
        tree.partitioned[..index].iter().any(|seen| seen == name)
    }) {
        return Err(JetServiceError::Policy(
            "service partition state is invalid".to_string(),
        ));
    }
    let mut group_names = Vec::with_capacity(tree.groups.len());
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
            || group
                .workers
                .iter()
                .enumerate()
                .any(|(index, name)| group.workers[..index].iter().any(|seen| seen == name))
            || group_names.iter().any(|seen| seen == &group.name)
        {
            return Err(JetServiceError::Policy(
                "service group state is invalid".to_string(),
            ));
        }
        group_names.push(group.name.clone());
    }
    match (&tree.state_adapter, &tree.state_authority) {
        (JetServiceStateAdapter::Empty, None) => {
            if tree.snapshot.is_some() || !tree.event_log.is_empty() {
                return Err(JetServiceError::Policy(
                    "Empty state adapter cannot contain durable records".to_string(),
                ));
            }
        }
        (JetServiceStateAdapter::Snapshot, Some(authority)) => {
            let checked = jet_services_attach_state_authority(
                authority,
                JetServiceStateAdapter::Snapshot,
            )?;
            let prefix = format!("v{}:", checked.version);
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
                jet_services_read_state_record(&prefix, snapshot)?;
            }
        }
        (JetServiceStateAdapter::EventLog, Some(authority)) => {
            let checked = jet_services_attach_state_authority(
                authority,
                JetServiceStateAdapter::EventLog,
            )?;
            let prefix = format!("v{}:", checked.version);
            if tree.snapshot.is_some() {
                return Err(JetServiceError::Policy(
                    "EventLog state adapter cannot contain a snapshot".to_string(),
                ));
            }
            for event in &tree.event_log {
                if event.len() > MAX_SERVICE_MESSAGE || event.chars().any(char::is_control) {
                    return Err(JetServiceError::Policy(
                        "event record exceeds the state record limit".to_string(),
                    ));
                }
                jet_services_read_state_record(&prefix, event)?;
            }
        }
        (JetServiceStateAdapter::Empty, Some(_))
        | (JetServiceStateAdapter::Snapshot, None)
        | (JetServiceStateAdapter::EventLog, None) => {
            return Err(JetServiceError::Policy(
                "service state adapter and authority do not match".to_string(),
            ));
        }
    }
    if !tree.workflows.is_empty() && tree.state_authority.is_none() {
        return Err(JetServiceError::Policy(
            "workflow history requires the injected service state authority".to_string(),
        ));
    }
    let mut idempotency_keys = Vec::new();
    for (key, endpoint, message) in &tree.idempotency_seen {
        if key.is_empty()
            || key.len() > MAX_SERVICE_NAME
            || key.chars().any(char::is_control)
            || message.len() > MAX_SERVICE_MESSAGE
            || message.chars().any(char::is_control)
            || endpoint.tree != tree.name
            || endpoint.authority != tree.authority
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
            || endpoint.authority != tree.authority
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
    jet_services_authority_validate(endpoint)?;
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

fn jet_services_rebuild_mailbox(worker: &mut JetServiceWorker) -> Result<(), JetServiceError> {
    worker.mailbox = jet_services_new_mailbox(
        worker.mailbox.endpoint.clone(),
        worker.mailbox.capacity,
        worker.mailbox.messages.clone(),
    )?;
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
    jet_services_authority_validate(endpoint)?;
    let draining = tree.draining.iter().any(|name| name == &endpoint.worker);
    if !draining {
        let available = tree
            .workers
            .iter()
            .find(|worker| worker.endpoint == *endpoint)
            .map(|worker| {
                if worker.running {
                    worker.mailbox.capacity.saturating_sub(worker.mailbox.depth)
                } else {
                    0
                }
            })
            .ok_or_else(|| {
                JetServiceError::Unknown(format!(
                    "endpoint {}/{} is not in this tree",
                    endpoint.tree, endpoint.worker
                ))
            })?;
        if available > 0 {
            let pending = jet_services_authority_take_pending(endpoint, available)?;
            if !pending.is_empty() {
                let worker = jet_services_find_worker_mut(tree, endpoint)?;
                let mut remaining = pending.clone();
                for (id, message, store) in pending {
                    if let Err(error) = worker.mailbox.sender.try_send(message.clone()) {
                        let delivery_error = match error {
                            std::sync::mpsc::TrySendError::Full(_) => JetServiceError::Policy(
                                "service authority delivery exceeded mailbox capacity".to_string(),
                            ),
                            std::sync::mpsc::TrySendError::Disconnected(_) => {
                                JetServiceError::NotStarted(
                                    "service authority worker mailbox is closed".to_string(),
                                )
                            }
                        };
                        let rollback = jet_services_authority_requeue_pending(endpoint, remaining.clone());
                        if let Err(rollback_error) = rollback {
                            let mailbox = jet_services_rebuild_mailbox(&mut *worker);
                            return Err(match mailbox {
                                Ok(()) => JetServiceError::Policy(format!(
                                    "{}; authority rollback failed: {}; unmarked receipts remain recoverable from the authority log",
                                    delivery_error.jet_show(),
                                    rollback_error.jet_show()
                                )),
                                Err(mailbox_error) => JetServiceError::Policy(format!(
                                    "{}; authority rollback failed: {}; mailbox rollback failed: {}",
                                    delivery_error.jet_show(),
                                    rollback_error.jet_show(),
                                    mailbox_error.jet_show()
                                )),
                            });
                        }
                        jet_services_rebuild_mailbox(&mut *worker).map_err(|rollback_error| {
                            JetServiceError::Policy(format!(
                                "{}; mailbox rollback failed: {}",
                                delivery_error.jet_show(),
                                rollback_error.jet_show()
                            ))
                        })?;
                        return Err(delivery_error);
                    }
                    worker.mailbox.messages.push(message.clone());
                    if let Err(error) = jet_services_authority_mark_delivered(&store, &id) {
                        // The mailbox now owns this message. Restore the
                        // whole uncommitted suffix before removing it locally.
                        let rollback =
                            jet_services_authority_requeue_pending(endpoint, remaining.clone());
                        if let Err(rollback_error) = rollback {
                            let Some(last) = worker.mailbox.messages.pop() else {
                                return Err(JetServiceError::Policy(format!(
                                    "{}; authority rollback failed: {}; mailbox lost the partially delivered message",
                                    error.jet_show(),
                                    rollback_error.jet_show()
                                )));
                            };
                            if last != message {
                                worker.mailbox.messages.push(last);
                                return Err(JetServiceError::Policy(format!(
                                    "{}; authority rollback failed: {}; mailbox order changed during rollback",
                                    error.jet_show(),
                                    rollback_error.jet_show()
                                )));
                            }
                            let mailbox = jet_services_rebuild_mailbox(&mut *worker);
                            return Err(match mailbox {
                                Ok(()) => JetServiceError::Policy(format!(
                                    "{}; authority rollback failed: {}; unmarked receipts remain recoverable from the authority log",
                                    error.jet_show(),
                                    rollback_error.jet_show()
                                )),
                                Err(mailbox_error) => JetServiceError::Policy(format!(
                                    "{}; authority rollback failed: {}; mailbox rollback failed: {}",
                                    error.jet_show(),
                                    rollback_error.jet_show(),
                                    mailbox_error.jet_show()
                                )),
                            });
                        }
                        let Some(last) = worker.mailbox.messages.pop() else {
                            return Err(JetServiceError::Policy(
                                "service mailbox lost the partially delivered message during rollback"
                                    .to_string(),
                            ));
                        };
                        if last != message {
                            worker.mailbox.messages.push(last);
                            return Err(JetServiceError::Policy(
                                "service mailbox order changed during partial delivery rollback"
                                    .to_string(),
                            ));
                        }
                        jet_services_rebuild_mailbox(&mut *worker).map_err(|rollback_error| {
                            JetServiceError::Policy(format!(
                                "{}; mailbox rollback failed: {}",
                                error.jet_show(),
                                rollback_error.jet_show()
                            ))
                        })?;
                        return Err(error);
                    }
                    remaining.remove(0);
                }
                worker.mailbox.depth = worker.mailbox.messages.len() as i64;
            }
        }
    }
    let (message, should_stop) = {
        let worker = jet_services_find_worker_mut(tree, endpoint)?;
        if !worker.running && (!draining || worker.mailbox.messages.is_empty()) {
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
        let worker = jet_services_find_worker_mut(tree, endpoint)?;
        let worker_endpoint = worker.endpoint.clone();
        jet_services_authority_update(&worker_endpoint, false)?;
        worker.running = false;
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
    // A partition is an authority decision, not a transient worker flag.
    // Restart policy may revive a crashed worker, but it must not silently
    // rejoin a partitioned side of the service graph.
    for worker in &mut tree.workers {
        if tree.partitioned.iter().any(|name| name == &worker.name) {
            worker.running = false;
        }
    }
    for worker in &tree.workers {
        jet_services_authority_update(&worker.endpoint, worker.running)?;
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

fn jet_services_state_authority_show(authority: &JetServiceStateAuthority) -> String {
    authority.jet_show()
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
    let authority = tree.state_authority.as_ref().ok_or_else(|| {
        JetServiceError::Policy(
            "durable service delivery needs its injected authority".to_string(),
        )
    })?;
    let runtime = JetServiceRuntime {
        store: authority.store.clone(),
        retention_ms: 0,
    };
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
    match jet_services_runtime_send(&runtime, endpoint, &message, &idempotency_key)? {
        JetServiceReceipt::Accepted(id) => {
            if let Err(error) = jet_services_send(tree, endpoint, message.clone()) {
                if let JetServiceError::Full(_) = error {
                    let _ = jet_services_runtime_dead_letter(&runtime, &id);
                    if tree.dead_letters.len() < MAX_SERVICE_DEAD_LETTERS {
                        tree.dead_letters.push(id);
                    }
                }
                return Err(error);
            }
            if tree.idempotency_seen.len() < MAX_SERVICE_IDEMPOTENCY
                && !tree
                    .idempotency_seen
                    .iter()
                    .any(|(key, _, _)| key == &idempotency_key)
            {
                tree.idempotency_seen
                    .push((idempotency_key, endpoint.clone(), message));
            }
            Ok(())
        }
        JetServiceReceipt::Duplicate(_) => Ok(()),
        JetServiceReceipt::Retained { .. } => Ok(()),
        JetServiceReceipt::DeadLettered(id) => {
            if tree.dead_letters.len() < MAX_SERVICE_DEAD_LETTERS {
                tree.dead_letters.push(id.clone());
            }
            Err(JetServiceError::Unavailable(format!(
                "durable service delivery dead-lettered: {id}"
            )))
        }
        JetServiceReceipt::Rejected(reason) => Err(JetServiceError::Policy(reason)),
        JetServiceReceipt::Unavailable(reason) => Err(JetServiceError::Unavailable(reason)),
    }
}

fn jet_services_dead_letter_count(tree: &JetServiceTree) -> i64 {
    tree.dead_letters.len() as i64
}

fn jet_services_drain_dead_letters(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
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
    if tree.snapshot.is_some() || !tree.event_log.is_empty() {
        return Err(JetServiceError::Policy(
            "cannot discard state records while selecting Empty state adapter".to_string(),
        ));
    }
    tree.state_adapter = JetServiceStateAdapter::Empty;
    tree.state_authority = None;
    Ok(())
}

fn jet_services_set_state_snapshot(
    tree: &mut JetServiceTree,
    store: JetServiceStateStore,
    schema: String,
    version: i64,
) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change state adapter after start".to_string(),
        ));
    }
    if tree.snapshot.is_some() || !tree.event_log.is_empty() {
        return Err(JetServiceError::Policy(
            "cannot replace a state adapter after it has records".to_string(),
        ));
    }
    let authority = jet_services_state_authority(&store, schema, version)?;
    tree.state_adapter = JetServiceStateAdapter::Snapshot;
    tree.state_authority = Some(jet_services_attach_state_authority(
        &authority,
        JetServiceStateAdapter::Snapshot,
    )?);
    Ok(())
}

fn jet_services_set_state_event_log(
    tree: &mut JetServiceTree,
    store: JetServiceStateStore,
    schema: String,
    version: i64,
) -> Result<(), JetServiceError> {
    if tree.started {
        return Err(JetServiceError::Policy(
            "cannot change state adapter after start".to_string(),
        ));
    }
    if tree.snapshot.is_some() || !tree.event_log.is_empty() {
        return Err(JetServiceError::Policy(
            "cannot replace a state adapter after it has records".to_string(),
        ));
    }
    let authority = jet_services_state_authority(&store, schema, version)?;
    tree.state_adapter = JetServiceStateAdapter::EventLog;
    tree.state_authority = Some(jet_services_attach_state_authority(
        &authority,
        JetServiceStateAdapter::EventLog,
    )?);
    Ok(())
}

fn jet_services_state_record(prefix: &str, payload: &str) -> String {
    format!("{prefix}{}:{payload}", payload.len())
}

fn jet_services_state_adapter_name(adapter: &JetServiceStateAdapter) -> &'static str {
    match adapter {
        JetServiceStateAdapter::Empty => "empty",
        JetServiceStateAdapter::Snapshot => "snapshot",
        JetServiceStateAdapter::EventLog => "event-log",
    }
}

fn jet_services_state_frame(payload: &str) -> Vec<u8> {
    let mut frame = format!("{}:", payload.len()).into_bytes();
    frame.extend_from_slice(payload.as_bytes());
    frame
}

fn jet_services_state_header(
    authority: &JetServiceStateAuthority,
    adapter: &JetServiceStateAdapter,
) -> Vec<u8> {
    let mut header = b"JET-SERVICE-STATE/1\nadapter:".to_vec();
    header.extend_from_slice(jet_services_state_adapter_name(adapter).as_bytes());
    header.extend_from_slice(b"\nschema:");
    header.extend_from_slice(&jet_services_state_frame(&authority.schema));
    header.extend_from_slice(b"\nversion:");
    header.extend_from_slice(authority.version.to_string().as_bytes());
    header.extend_from_slice(b"\nmigration:");
    header.extend_from_slice(&jet_services_state_frame(&authority.migration));
    header.extend_from_slice(b"\n");
    header
}

fn jet_services_state_error(message: impl Into<String>) -> JetServiceError {
    JetServiceError::Policy(format!(
        "{}; repair or replace the state store before retrying",
        message.into()
    ))
}

fn jet_services_state_read_line(
    bytes: &[u8],
    offset: &mut usize,
    label: &str,
) -> Result<String, JetServiceError> {
    let rest = bytes
        .get(*offset..)
        .ok_or_else(|| jet_services_state_error(format!("{label} is truncated")))?;
    let end = rest
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| jet_services_state_error(format!("{label} is truncated")))?;
    let line = std::str::from_utf8(&rest[..end])
        .map_err(|_| jet_services_state_error(format!("{label} is not valid UTF-8")))?;
    *offset = offset.saturating_add(end + 1);
    Ok(line.to_string())
}

fn jet_services_state_read_frame(
    bytes: &[u8],
    offset: &mut usize,
    label: &str,
) -> Result<String, JetServiceError> {
    let start = *offset;
    let rest = bytes
        .get(start..)
        .ok_or_else(|| jet_services_state_error(format!("{label} is truncated")))?;
    let colon = rest
        .iter()
        .position(|byte| *byte == b':')
        .ok_or_else(|| jet_services_state_error(format!("{label} length is missing")))?;
    let length = std::str::from_utf8(&rest[..colon])
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or_else(|| jet_services_state_error(format!("{label} length is invalid")))?;
    if length > MAX_SERVICE_MESSAGE {
        return Err(jet_services_state_error(format!(
            "{label} exceeds the state record limit"
        )));
    }
    let payload_start = start
        .checked_add(colon + 1)
        .ok_or_else(|| jet_services_state_error(format!("{label} offset overflow")))?;
    let payload_end = payload_start
        .checked_add(length)
        .ok_or_else(|| jet_services_state_error(format!("{label} length overflows the store")))?;
    let payload = bytes
        .get(payload_start..payload_end)
        .ok_or_else(|| jet_services_state_error(format!("{label} is truncated")))?;
    let value = std::str::from_utf8(payload)
        .map_err(|_| jet_services_state_error(format!("{label} is not valid UTF-8")))?;
    *offset = payload_end;
    Ok(value.to_string())
}

fn jet_services_state_encode(
    authority: &JetServiceStateAuthority,
    adapter: &JetServiceStateAdapter,
    records: &[String],
) -> Result<Vec<u8>, JetServiceError> {
    if records.len() > MAX_SERVICE_STATE_RECORDS
        || matches!(adapter, JetServiceStateAdapter::Snapshot) && records.len() > 1
    {
        return Err(JetServiceError::Policy(
            "service state record limit exceeded".to_string(),
        ));
    }
    let prefix = format!("v{}:", authority.version);
    let mut bytes = jet_services_state_header(authority, adapter);
    for record in records {
        jet_services_read_state_record(&prefix, record)?;
        if record.len() > MAX_SERVICE_MESSAGE || record.chars().any(char::is_control) {
            return Err(JetServiceError::Policy(
                "service state record contains invalid bytes".to_string(),
            ));
        }
        bytes.extend_from_slice(&jet_services_state_frame(record));
        if bytes.len() > MAX_SERVICE_STATE_BYTES {
            return Err(JetServiceError::Policy(
                "service state store exceeds its byte limit".to_string(),
            ));
        }
    }
    Ok(bytes)
}

fn jet_services_state_decode(
    authority: &JetServiceStateAuthority,
    adapter: &JetServiceStateAdapter,
    bytes: &[u8],
) -> Result<Vec<String>, JetServiceError> {
    if bytes.len() > MAX_SERVICE_STATE_BYTES {
        return Err(jet_services_state_error(
            "service state store exceeds its byte limit",
        ));
    }
    let mut offset = 0;
    if jet_services_state_read_line(bytes, &mut offset, "state magic")?
        != "JET-SERVICE-STATE/1"
    {
        return Err(jet_services_state_error("service state magic is invalid"));
    }
    let expected_adapter = format!("adapter:{}", jet_services_state_adapter_name(adapter));
    if jet_services_state_read_line(bytes, &mut offset, "state adapter")? != expected_adapter {
        return Err(jet_services_state_error(
            "service state adapter does not match the declared adapter",
        ));
    }
    let schema_line = jet_services_state_read_line(bytes, &mut offset, "state schema")?;
    let schema_prefix = "schema:";
    let schema_meta = schema_line.strip_prefix(schema_prefix).ok_or_else(|| {
        jet_services_state_error("service state schema header is invalid")
    })?;
    let (schema_len_text, schema) = schema_meta
        .split_once(':')
        .ok_or_else(|| jet_services_state_error("service state schema length is invalid"))?;
    let schema_len = schema_len_text
        .parse::<usize>()
        .map_err(|_| jet_services_state_error("service state schema length is invalid"))?;
    if schema_len != schema.len()
        || schema_len != authority.schema.len()
        || schema_len > MAX_SERVICE_STATE_SCHEMA
    {
        return Err(jet_services_state_error(
            "service state schema length does not match the authority",
        ));
    }
    if schema != authority.schema {
        return Err(jet_services_state_error(
            "service state schema does not match the authority",
        ));
    }
    let version_line = jet_services_state_read_line(bytes, &mut offset, "state version")?;
    let version = version_line
        .strip_prefix("version:")
        .and_then(|text| text.parse::<i64>().ok())
        .ok_or_else(|| jet_services_state_error("service state version is invalid"))?;
    if version != authority.version {
        return Err(jet_services_state_error(format!(
            "service state version {version} does not match authority version {}",
            authority.version
        )));
    }
    let migration_line = jet_services_state_read_line(bytes, &mut offset, "state migration")?;
    let migration_meta = migration_line.strip_prefix("migration:").ok_or_else(|| {
        jet_services_state_error("state migration header is invalid")
    })?;
    let (migration_len_text, migration) = migration_meta
        .split_once(':')
        .ok_or_else(|| jet_services_state_error("state migration length is invalid"))?;
    let migration_len = migration_len_text
        .parse::<usize>()
        .map_err(|_| jet_services_state_error("state migration length is invalid"))?;
    if migration_len != migration.len() || migration_len > 32 {
        return Err(jet_services_state_error(
            "state migration length does not match the authority",
        ));
    }
    if migration != authority.migration {
        return Err(jet_services_state_error(
            "state migration policy does not match the authority",
        ));
    }
    let prefix = format!("v{}:", authority.version);
    let mut records = Vec::new();
    while offset < bytes.len() {
        if records.len() >= MAX_SERVICE_STATE_RECORDS {
            return Err(jet_services_state_error(
                "service state record limit exceeded",
            ));
        }
        let record = jet_services_state_read_frame(bytes, &mut offset, "state record")?;
        jet_services_read_state_record(&prefix, &record)?;
        records.push(record);
    }
    if matches!(adapter, JetServiceStateAdapter::Snapshot) && records.len() > 1 {
        return Err(jet_services_state_error(
            "snapshot store contains more than one record",
        ));
    }
    Ok(records)
}

fn jet_services_state_store_read(
    authority: &JetServiceStateAuthority,
    adapter: &JetServiceStateAdapter,
) -> Result<Vec<String>, JetServiceError> {
    let path = std::path::Path::new(&authority.store);
    jet_services_state_validate_path(path)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let metadata = std::fs::metadata(path).map_err(|error| {
        jet_services_state_error(format!("cannot inspect service state store: {error}"))
    })?;
    if metadata.len() > MAX_SERVICE_STATE_BYTES as u64 {
        return Err(jet_services_state_error(
            "service state store exceeds its byte limit",
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|error| {
        jet_services_state_error(format!("cannot open service state store: {error}"))
    })?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|error| {
        jet_services_state_error(format!("cannot read service state store: {error}"))
    })?;
    jet_services_state_decode(authority, adapter, &bytes)
}

fn jet_services_state_store_load(
    authority: &JetServiceStateAuthority,
    adapter: &JetServiceStateAdapter,
) -> Result<Vec<String>, JetServiceError> {
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| jet_services_state_error("service state lock is poisoned"))?;
    jet_services_state_store_read(authority, adapter)
}

fn jet_services_state_store_write(
    authority: &JetServiceStateAuthority,
    adapter: &JetServiceStateAdapter,
    records: &[String],
) -> Result<(), JetServiceError> {
    let bytes = jet_services_state_encode(authority, adapter, records)?;
    let path = std::path::Path::new(&authority.store);
    jet_services_state_validate_path(path)?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| {
            jet_services_state_error(format!("cannot create service state directory: {error}"))
        })?;
    }
    // Check again after creating missing directories.  This closes the common
    // symlink-parent mistake without pretending to provide a cross-process
    // filesystem lock.
    jet_services_state_validate_path(path)?;
    let temporary = std::path::PathBuf::from(format!(
        "{}.jet-state-{}-{}-{}",
        authority.store,
        std::process::id(),
        service_authority_now(),
        service_state_temp_sequence(),
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)
            .map_err(|error| {
                jet_services_state_error(format!("cannot create temporary state store: {error}"))
            })?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|error| {
            jet_services_state_error(format!("cannot write service state store: {error}"))
        })?;
        std::io::Write::flush(&mut file).map_err(|error| {
            jet_services_state_error(format!("cannot flush service state store: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            jet_services_state_error(format!("cannot sync service state store: {error}"))
        })?;
        std::fs::rename(&temporary, path).map_err(|error| {
            jet_services_state_error(format!("cannot publish service state store: {error}"))
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn jet_services_state_validate_path(path: &std::path::Path) -> Result<(), JetServiceError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(jet_services_state_error(
                "service state store must not be a symlink",
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(jet_services_state_error(
                "service state store is not a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(jet_services_state_error(format!(
                "cannot inspect service state store: {error}"
            )));
        }
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut current = parent.to_path_buf();
    loop {
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(jet_services_state_error(format!(
                    "service state parent is a symlink: {}",
                    current.display()
                )))
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(jet_services_state_error(format!(
                    "service state parent is not a directory: {}",
                    current.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(jet_services_state_error(format!(
                    "cannot inspect service state parent: {error}"
                )));
            }
        }
        let Some(next) = current.parent() else {
            break;
        };
        if next == current {
            break;
        }
        current = next.to_path_buf();
    }
    Ok(())
}

static SERVICE_STATE_TEMP_SEQUENCE: std::sync::OnceLock<std::sync::atomic::AtomicU64> =
    std::sync::OnceLock::new();

fn service_state_temp_sequence() -> u64 {
    SERVICE_STATE_TEMP_SEQUENCE
        .get_or_init(|| std::sync::atomic::AtomicU64::new(0))
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn jet_services_state_store_append(
    authority: &JetServiceStateAuthority,
    record: &str,
) -> Result<(), JetServiceError> {
    let adapter = JetServiceStateAdapter::EventLog;
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| jet_services_state_error("service state lock is poisoned"))?;
    let records = jet_services_state_store_read(authority, &adapter)?;
    if records.len() >= MAX_SERVICE_STATE_RECORDS {
        return Err(JetServiceError::Policy(
            "event-log record limit exceeded".to_string(),
        ));
    }
    let path = std::path::Path::new(&authority.store);
    if !path.exists() {
        return jet_services_state_store_write(authority, &adapter, &[record.to_string()]);
    }
    let frame = jet_services_state_frame(record);
    let current_size = path.metadata().map_err(|error| {
        jet_services_state_error(format!("cannot inspect service state store before append: {error}"))
    })?.len();
    if current_size
        .checked_add(frame.len() as u64)
        .is_none_or(|size| size > MAX_SERVICE_STATE_BYTES as u64)
    {
        return Err(JetServiceError::Policy(
            "service state store exceeds its byte limit".to_string(),
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| jet_services_state_error(format!("cannot append state store: {error}")))?;
    std::io::Write::write_all(&mut file, &frame)
        .map_err(|error| jet_services_state_error(format!("cannot append state event: {error}")))?;
    std::io::Write::flush(&mut file)
        .map_err(|error| jet_services_state_error(format!("cannot flush state event: {error}")))?;
    file.sync_all()
        .map_err(|error| jet_services_state_error(format!("cannot sync state event: {error}")))
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
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    if tree.state_adapter != JetServiceStateAdapter::Snapshot {
        return Err(JetServiceError::Policy(
            "commit_snapshot requires Snapshot state adapter".to_string(),
        ));
    }
    let authority = tree.state_authority.as_ref().ok_or_else(|| {
        JetServiceError::Policy("Snapshot state has no injected authority".to_string())
    })?;
    if payload.len() > MAX_SERVICE_MESSAGE || payload.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "snapshot record exceeds the state record limit".to_string(),
        ));
    }
    let record = jet_services_state_record(&format!("v{}:", authority.version), &payload);
    let _guard = service_authority_lock()
        .lock()
        .map_err(|_| jet_services_state_error("service state lock is poisoned"))?;
    jet_services_state_store_write(authority, &JetServiceStateAdapter::Snapshot, &[record.clone()])?;
    tree.snapshot = Some(record);
    Ok(())
}

fn jet_services_restore_snapshot(tree: &JetServiceTree) -> Result<String, JetServiceError> {
    let authority = tree.state_authority.as_ref().ok_or_else(|| {
        JetServiceError::Policy("Snapshot state has no injected authority".to_string())
    })?;
    match jet_services_state_store_load(authority, &JetServiceStateAdapter::Snapshot)?
        .into_iter()
        .next()
    {
        Some(s) => Ok(jet_services_read_state_record(
            &format!("v{}:", authority.version),
            &s,
        )?
        .to_string()),
        None => Err(JetServiceError::Policy(
            "no snapshot committed".to_string(),
        )),
    }
}

fn jet_services_append_event(
    tree: &mut JetServiceTree,
    event: String,
) -> Result<(), JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    if tree.state_adapter != JetServiceStateAdapter::EventLog {
        return Err(JetServiceError::Policy(
            "append_event requires EventLog state adapter".to_string(),
        ));
    }
    let authority = tree.state_authority.as_ref().ok_or_else(|| {
        JetServiceError::Policy("EventLog state has no injected authority".to_string())
    })?;
    if event.len() > MAX_SERVICE_MESSAGE || event.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "event record exceeds the state record limit".to_string(),
        ));
    }
    if tree.event_log.len() >= MAX_SERVICE_STATE_RECORDS {
        return Err(JetServiceError::Policy(
            "event-log record limit exceeded".to_string(),
        ));
    }
    let record = jet_services_state_record(&format!("v{}:", authority.version), &event);
    jet_services_state_store_append(authority, &record)?;
    tree.event_log.push(record);
    Ok(())
}

fn jet_services_event_count(tree: &JetServiceTree) -> i64 {
    tree.event_log.len() as i64
}

fn jet_services_replay_events(tree: &JetServiceTree) -> String {
    let prefix = tree
        .state_authority
        .as_ref()
        .map(|authority| format!("v{}:", authority.version))
        .unwrap_or_else(|| "v1:".to_string());
    let mut events = Vec::with_capacity(tree.event_log.len());
    for record in &tree.event_log {
        match jet_services_read_state_record(&prefix, record) {
            Ok(event) => events.push(event),
            Err(_) => return "StateReplayError(invalid_record)".to_string(),
        }
    }
    events.join("|")
}

fn jet_services_workflow_authority(
    tree: &JetServiceTree,
) -> Result<JetServiceStateAuthority, JetServiceError> {
    let authority = tree.state_authority.as_ref().ok_or_else(|| {
        JetServiceError::Policy(
            "durable workflow history needs the injected service state authority".to_string(),
        )
    })?;
    let store = format!("{}.workflows", authority.store);
    service_authority_validate_text(
        &store,
        "service workflow store",
        MAX_SERVICE_STATE_STORE,
        false,
    )?;
    Ok(JetServiceStateAuthority {
        store,
        schema: authority.schema.clone(),
        version: authority.version,
        migration: authority.migration.clone(),
        adapter: JetServiceStateAdapter::EventLog,
    })
}

fn jet_services_workflow_field<'a>(
    input: &'a str,
    label: &str,
) -> Result<&'a str, JetServiceError> {
    let (length_text, value) = input.split_once(':').ok_or_else(|| {
        jet_services_state_error(format!("workflow {label} length is missing"))
    })?;
    let length = length_text.parse::<usize>().map_err(|_| {
        jet_services_state_error(format!("workflow {label} length is invalid"))
    })?;
    if length != value.len() || value.chars().any(char::is_control) {
        return Err(jet_services_state_error(format!(
            "workflow {label} length or contents are invalid"
        )));
    }
    Ok(value)
}

fn jet_services_workflow_frame(value: &str) -> String {
    format!("{}:{value}", value.len())
}

fn jet_services_workflow_take_field<'a>(
    input: &'a str,
    label: &str,
) -> Result<(&'a str, &'a str), JetServiceError> {
    let (length_text, value) = input.split_once(':').ok_or_else(|| {
        jet_services_state_error(format!("workflow {label} length is missing"))
    })?;
    let length = length_text.parse::<usize>().map_err(|_| {
        jet_services_state_error(format!("workflow {label} length is invalid"))
    })?;
    let value = value.get(..length).ok_or_else(|| {
        jet_services_state_error(format!("workflow {label} length is not a character boundary"))
    })?;
    let rest = input
        .split_once(':')
        .map(|(_, tail)| tail)
        .and_then(|tail| tail.get(length..))
        .ok_or_else(|| jet_services_state_error(format!("workflow {label} length is invalid")))?;
    if value.chars().any(char::is_control) {
        return Err(jet_services_state_error(format!(
            "workflow {label} contains control characters"
        )));
    }
    Ok((value, rest))
}

fn jet_services_workflow_token(
    value: &str,
    label: &str,
    max: usize,
) -> Result<(), JetServiceError> {
    if value.trim().is_empty()
        || value.len() > max
        || value.chars().any(char::is_control)
        || value.contains(':')
    {
        return Err(JetServiceError::Policy(format!(
            "workflow {label} must be non-empty, visible, bounded, and contain no `:`"
        )));
    }
    Ok(())
}

fn jet_services_workflow_activity_step(
    activity: &str,
    key: &str,
    attempt: i64,
    max_attempts: i64,
) -> String {
    format!("activity:{activity}:{key}:{attempt}:{max_attempts}")
}

fn jet_services_workflow_activity_parts(
    step: &str,
) -> Option<(&str, &str, i64, i64)> {
    let mut parts = step.split(':');
    if parts.next()? != "activity" {
        return None;
    }
    let activity = parts.next()?;
    let key = parts.next()?;
    let attempt = parts.next()?.parse::<i64>().ok()?;
    let max_attempts = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((activity, key, attempt, max_attempts))
}

fn jet_services_workflow_activity_index(
    workflow: &JetServiceWorkflow,
    key: &str,
) -> Option<(usize, String, i64, i64)> {
    workflow
        .steps
        .iter()
        .enumerate()
        .find_map(|(index, step)| {
            let (activity, existing_key, attempt, max_attempts) =
                jet_services_workflow_activity_parts(step)?;
            (existing_key == key).then_some((index, activity.to_string(), attempt, max_attempts))
        })
}

fn jet_services_workflow_replay(
    authority: &JetServiceStateAuthority,
) -> Result<Vec<JetServiceWorkflow>, JetServiceError> {
    let records = jet_services_state_store_load(authority, &JetServiceStateAdapter::EventLog)?;
    let prefix = format!("v{}:", authority.version);
    let mut workflows = Vec::new();
    for record in records {
        let payload = jet_services_read_state_record(&prefix, &record)?;
        if let Some(fields) = payload.strip_prefix("workflow-start:") {
            let (run_id_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow start run id is missing")
            })?;
            let run_id = run_id_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow start run id is invalid")
            })?;
            let (version_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow start version is missing")
            })?;
            let version = version_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow start version is invalid")
            })?;
            let id = jet_services_workflow_field(fields, "id")?.to_string();
            if run_id <= 0 || version < 1 || id.trim().is_empty() || id.len() > MAX_SERVICE_NAME {
                return Err(jet_services_state_error(
                    "workflow start contains invalid identity",
                ));
            }
            if workflows.iter().any(|workflow: &JetServiceWorkflow| {
                workflow.run_id == run_id || workflow.id == id
            }) {
                return Err(jet_services_state_error(
                    "workflow log contains a duplicate start record",
                ));
            }
            workflows.push(JetServiceWorkflow {
                id,
                run_id,
                version,
                steps: Vec::new(),
                history: vec![format!("start@v{version}")],
            });
        } else if let Some(fields) = payload.strip_prefix("workflow-step:") {
            let (run_id_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow step run id is missing")
            })?;
            let run_id = run_id_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow step run id is invalid")
            })?;
            let step = jet_services_workflow_field(fields, "step")?.to_string();
            if step.trim().is_empty() || step.len() > MAX_SERVICE_MESSAGE {
                return Err(jet_services_state_error(
                    "workflow step contains invalid contents",
                ));
            }
            let workflow = workflows
                .iter_mut()
                .find(|workflow| workflow.run_id == run_id)
                .ok_or_else(|| jet_services_state_error("workflow step has no start record"))?;
            if workflow.steps.len() >= MAX_SERVICE_WORKFLOW_STEPS {
                return Err(jet_services_state_error(
                    "workflow step history limit exceeded",
                ));
            }
            workflow.steps.push(step.clone());
            workflow.history.push(format!("step:{step}"));
        } else if let Some(fields) = payload.strip_prefix("workflow-activity:") {
            let (run_id_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow activity run id is missing")
            })?;
            let run_id = run_id_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow activity run id is invalid")
            })?;
            let (attempt_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow activity attempt is missing")
            })?;
            let attempt = attempt_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow activity attempt is invalid")
            })?;
            let (max_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow activity retry limit is missing")
            })?;
            let max_attempts = max_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow activity retry limit is invalid")
            })?;
            let (activity, rest) = jet_services_workflow_take_field(fields, "activity")?;
            let (key, rest) = jet_services_workflow_take_field(rest, "idempotency key")?;
            if jet_services_workflow_token(activity, "activity", MAX_SERVICE_NAME).is_err()
                || jet_services_workflow_token(key, "idempotency key", MAX_SERVICE_NAME).is_err()
                || run_id < 1
                || attempt != 1
                || max_attempts < 1
                || max_attempts > MAX_SERVICE_ACTIVITY_ATTEMPTS
                || !rest.is_empty()
            {
                return Err(jet_services_state_error(
                    "workflow activity start contains invalid fields",
                ));
            }
            let workflow = workflows
                .iter_mut()
                .find(|workflow| workflow.run_id == run_id)
                .ok_or_else(|| jet_services_state_error("workflow activity has no start record"))?;
            if jet_services_workflow_activity_index(workflow, key).is_some() {
                return Err(jet_services_state_error(
                    "workflow log contains a duplicate activity key",
                ));
            }
            workflow.steps.push(jet_services_workflow_activity_step(
                activity,
                key,
                attempt,
                max_attempts,
            ));
            workflow
                .history
                .push(format!("activity:{activity}:{key}@{attempt}/{max_attempts}"));
        } else if let Some(fields) = payload.strip_prefix("workflow-activity-retry:") {
            let (run_id_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow activity retry run id is missing")
            })?;
            let run_id = run_id_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow activity retry run id is invalid")
            })?;
            let (attempt_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow activity retry attempt is missing")
            })?;
            let attempt = attempt_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow activity retry attempt is invalid")
            })?;
            let (key, rest) = jet_services_workflow_take_field(fields, "idempotency key")?;
            if run_id < 1
                || attempt < 2
                || attempt > MAX_SERVICE_ACTIVITY_ATTEMPTS
                || !rest.is_empty()
                || jet_services_workflow_token(key, "idempotency key", MAX_SERVICE_NAME).is_err()
            {
                return Err(jet_services_state_error(
                    "workflow activity retry contains invalid fields",
                ));
            }
            let workflow = workflows
                .iter_mut()
                .find(|workflow| workflow.run_id == run_id)
                .ok_or_else(|| jet_services_state_error("workflow retry has no start record"))?;
            let (index, activity, current_attempt, max_attempts) =
                jet_services_workflow_activity_index(workflow, key)
                    .ok_or_else(|| jet_services_state_error("workflow retry has no activity"))?;
            if attempt != current_attempt + 1 || attempt > max_attempts {
                return Err(jet_services_state_error(
                    "workflow activity retry is out of order or exceeds its limit",
                ));
            }
            workflow.steps[index] = jet_services_workflow_activity_step(
                &activity,
                key,
                attempt,
                max_attempts,
            );
            workflow
                .history
                .push(format!("activity-retry:{key}@{attempt}/{max_attempts}"));
        } else if let Some(fields) = payload.strip_prefix("workflow-activity-done:") {
            let (run_id_text, fields) = fields.split_once(':').ok_or_else(|| {
                jet_services_state_error("workflow activity completion run id is missing")
            })?;
            let run_id = run_id_text.parse::<i64>().map_err(|_| {
                jet_services_state_error("workflow activity completion run id is invalid")
            })?;
            let (key, rest) = jet_services_workflow_take_field(fields, "idempotency key")?;
            let (outcome, rest) = jet_services_workflow_take_field(rest, "activity outcome")?;
            if run_id < 1
                || outcome.len() > MAX_SERVICE_MESSAGE
                || !rest.is_empty()
                || jet_services_workflow_token(key, "idempotency key", MAX_SERVICE_NAME).is_err()
            {
                return Err(jet_services_state_error(
                    "workflow activity completion contains invalid fields",
                ));
            }
            let workflow = workflows
                .iter_mut()
                .find(|workflow| workflow.run_id == run_id)
                .ok_or_else(|| jet_services_state_error("workflow completion has no start record"))?;
            if jet_services_workflow_activity_index(workflow, key).is_none() {
                return Err(jet_services_state_error(
                    "workflow completion has no activity record",
                ));
            }
            let marker = format!("activity-done:{key}");
            if workflow.history.iter().any(|entry| entry == &marker) {
                return Err(jet_services_state_error(
                    "workflow log contains a duplicate activity completion",
                ));
            }
            workflow.history.push(marker);
            workflow.history.push(format!("activity-result:{outcome}"));
        } else {
            return Err(jet_services_state_error(
                "workflow log contains an unknown record",
            ));
        }
    }
    if workflows.len() > MAX_SERVICE_WORKFLOW_STEPS {
        return Err(jet_services_state_error(
            "workflow run limit exceeded",
        ));
    }
    Ok(workflows)
}

fn jet_services_workflow_store_load(
    tree: &JetServiceTree,
) -> Result<Vec<JetServiceWorkflow>, JetServiceError> {
    let authority = jet_services_workflow_authority(tree)?;
    jet_services_workflow_replay(&authority)
}

fn jet_services_workflow_start(
    tree: &mut JetServiceTree,
    id: String,
    version: i64,
) -> Result<i64, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
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
    let run_id = tree
        .workflows
        .iter()
        .map(|workflow| workflow.run_id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| JetServiceError::Policy("workflow run id exhausted".to_string()))?;
    let authority = jet_services_workflow_authority(tree)?;
    let record = jet_services_state_record(
        &format!("v{}:", authority.version),
        &format!(
            "workflow-start:{run_id}:{version}{}",
            jet_services_workflow_frame(&id)
        ),
    );
    jet_services_state_store_append(&authority, &record)?;
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
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    if step.trim().is_empty() || step.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "workflow step must be non-empty and visible".to_string(),
        ));
    }
    let nondeterministic_markers = [
        "time.",
        "random",
        "rand.",
        "io.",
        "channel",
        "spawn",
        "service.connect",
    ];
    if nondeterministic_markers.iter().any(|marker| step.contains(marker)) {
        return Err(JetServiceError::Policy(
            "workflow step reaches a non-deterministic effect; use a recorded activity or timer"
                .to_string(),
        ));
    }
    let index = tree
        .workflows
        .iter()
        .position(|w| w.run_id == run_id)
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))?;
    if step.len() > MAX_SERVICE_MESSAGE
        || tree.workflows[index].steps.len() >= MAX_SERVICE_WORKFLOW_STEPS
    {
        return Err(JetServiceError::Policy(
            "workflow step history limit exceeded".to_string(),
        ));
    }
    let authority = jet_services_workflow_authority(tree)?;
    let record = jet_services_state_record(
        &format!("v{}:", authority.version),
        &format!(
            "workflow-step:{run_id}{}",
            jet_services_workflow_frame(&step)
        ),
    );
    jet_services_state_store_append(&authority, &record)?;
    let wf = &mut tree.workflows[index];
    wf.steps.push(step.clone());
    wf.history.push(format!("step:{step}"));
    Ok(())
}

fn jet_services_workflow_activity(
    tree: &mut JetServiceTree,
    run_id: i64,
    activity: String,
    key: String,
    max_attempts: i64,
) -> Result<String, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    jet_services_workflow_token(&activity, "activity", MAX_SERVICE_NAME)?;
    jet_services_workflow_token(&key, "idempotency key", MAX_SERVICE_NAME)?;
    if max_attempts < 1 || max_attempts > MAX_SERVICE_ACTIVITY_ATTEMPTS {
        return Err(JetServiceError::Policy(
            "workflow activity retry limit is outside the bounded range".to_string(),
        ));
    }
    let workflow_index = tree
        .workflows
        .iter()
        .position(|workflow| workflow.run_id == run_id)
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))?;
    if jet_services_workflow_activity_index(&tree.workflows[workflow_index], &key).is_some() {
        return Ok(format!("ActivityDuplicate({key})"));
    }
    let authority = jet_services_workflow_authority(tree)?;
    let record = jet_services_state_record(
        &format!("v{}:", authority.version),
        &format!(
            "workflow-activity:{run_id}:1:{max_attempts}{}{}",
            jet_services_workflow_frame(&activity),
            jet_services_workflow_frame(&key),
        ),
    );
    jet_services_state_store_append(&authority, &record)?;
    let workflow = &mut tree.workflows[workflow_index];
    workflow.steps.push(jet_services_workflow_activity_step(
        &activity,
        &key,
        1,
        max_attempts,
    ));
    workflow
        .history
        .push(format!("activity:{activity}:{key}@1/{max_attempts}"));
    Ok(format!("ActivityScheduled({key})"))
}

fn jet_services_workflow_activity_retry(
    tree: &mut JetServiceTree,
    run_id: i64,
    key: String,
) -> Result<String, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    jet_services_workflow_token(&key, "idempotency key", MAX_SERVICE_NAME)?;
    let workflow_index = tree
        .workflows
        .iter()
        .position(|workflow| workflow.run_id == run_id)
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))?;
    let (step_index, activity, attempt, max_attempts) =
        jet_services_workflow_activity_index(&tree.workflows[workflow_index], &key)
            .ok_or_else(|| JetServiceError::Unknown(format!("activity key `{key}` not found")))?;
    if attempt >= max_attempts {
        return Err(JetServiceError::Policy(
            "workflow activity retry limit exhausted".to_string(),
        ));
    }
    let next_attempt = attempt + 1;
    let authority = jet_services_workflow_authority(tree)?;
    let record = jet_services_state_record(
        &format!("v{}:", authority.version),
        &format!(
            "workflow-activity-retry:{run_id}:{next_attempt}{}",
            jet_services_workflow_frame(&key)
        ),
    );
    jet_services_state_store_append(&authority, &record)?;
    let workflow = &mut tree.workflows[workflow_index];
    workflow.steps[step_index] = jet_services_workflow_activity_step(
        &activity,
        &key,
        next_attempt,
        max_attempts,
    );
    workflow
        .history
        .push(format!("activity-retry:{key}@{next_attempt}/{max_attempts}"));
    Ok(format!("ActivityRetry({key}, attempt={next_attempt})"))
}

fn jet_services_workflow_activity_complete(
    tree: &mut JetServiceTree,
    run_id: i64,
    key: String,
    outcome: String,
) -> Result<(), JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    jet_services_workflow_token(&key, "idempotency key", MAX_SERVICE_NAME)?;
    if outcome.len() > MAX_SERVICE_MESSAGE || outcome.chars().any(char::is_control) {
        return Err(JetServiceError::Policy(
            "workflow activity outcome is too long or contains control characters".to_string(),
        ));
    }
    let workflow_index = tree
        .workflows
        .iter()
        .position(|workflow| workflow.run_id == run_id)
        .ok_or_else(|| JetServiceError::Unknown(format!("workflow run {run_id} not found")))?;
    if jet_services_workflow_activity_index(&tree.workflows[workflow_index], &key).is_none() {
        return Err(JetServiceError::Unknown(format!("activity key `{key}` not found")));
    }
    let marker = format!("activity-done:{key}");
    if tree.workflows[workflow_index]
        .history
        .iter()
        .any(|entry| entry == &marker)
    {
        return Ok(());
    }
    let authority = jet_services_workflow_authority(tree)?;
    let record = jet_services_state_record(
        &format!("v{}:", authority.version),
        &format!(
            "workflow-activity-done:{run_id}{}{}",
            jet_services_workflow_frame(&key),
            jet_services_workflow_frame(&outcome),
        ),
    );
    jet_services_state_store_append(&authority, &record)?;
    let workflow = &mut tree.workflows[workflow_index];
    workflow.history.push(marker);
    workflow.history.push(format!("activity-result:{outcome}"));
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
    let Some((entry, endpoint, signature)) = tree
        .directory
        .iter()
        .find(|(entry, _, _)| entry == name)
    else {
        return Err(JetServiceError::Unknown(format!(
            "directory has no entry `{name}`"
        )));
    };
    if endpoint.generation != tree.generation {
        return Err(JetServiceError::Stale(format!(
            "directory entry `{name}` is from generation {}",
            endpoint.generation
        )));
    }
    if signature != &jet_services_directory_signature(tree, entry, endpoint) {
        return Err(JetServiceError::Revoked(format!(
            "directory entry `{name}` has an invalid signature"
        )));
    }
    if tree.partitioned.iter().any(|worker| worker == &endpoint.worker) {
        return Err(JetServiceError::Partitioned(format!(
            "directory entry `{name}` is partitioned"
        )));
    }
    if !tree
        .workers
        .iter()
        .any(|worker| worker.running && worker.endpoint == *endpoint)
    {
        return Err(JetServiceError::Expired(format!(
            "directory entry `{name}` no longer names a running worker"
        )));
    }
    Ok(endpoint.clone())
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
        if let Ok(worker) = jet_services_find_worker_mut(tree, endpoint) {
            if worker.mailbox.messages.is_empty() {
                worker.running = false;
                jet_services_authority_update(&worker.endpoint, false)?;
                tree.draining.push(name);
                return Ok(());
            }
        }
        tree.draining.push(name);
    }
    Ok(())
}

fn jet_services_partition_worker(
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
        worker.running = false;
        worker.name.clone()
    };
    if !tree.partitioned.iter().any(|existing| existing == &name) {
        if tree.partitioned.len() >= MAX_SERVICE_WORKERS {
            return Err(JetServiceError::Policy(
                "service partition limit exceeded".to_string(),
            ));
        }
        tree.partitioned.push(name);
    }
    let worker = tree
        .workers
        .iter()
        .find(|worker| worker.name == endpoint.worker)
        .ok_or_else(|| JetServiceError::Unknown("partitioned worker disappeared".to_string()))?;
    jet_services_authority_update(&worker.endpoint, false)
}

fn jet_services_reconcile_worker(
    tree: &mut JetServiceTree,
    endpoint: &JetServiceEndpoint,
) -> Result<(), JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    jet_services_validate_endpoint(tree, endpoint).or_else(|error| match error {
        JetServiceError::Partitioned(_) => Ok(()),
        other => Err(other),
    })?;
    let Some(index) = tree
        .partitioned
        .iter()
        .position(|name| name == &endpoint.worker)
    else {
        return Err(JetServiceError::Unknown(format!(
            "worker `{}` is not partitioned",
            endpoint.worker
        )));
    };
    tree.partitioned.remove(index);
    let worker = tree
        .workers
        .iter_mut()
        .find(|worker| worker.endpoint == *endpoint)
        .ok_or_else(|| JetServiceError::Unknown("reconciled worker disappeared".to_string()))?;
    worker.running = true;
    jet_services_authority_update(&worker.endpoint, true)
}

fn jet_services_prepare_rollback(
    tree: &JetServiceTree,
) -> Result<(String, bool, String), JetServiceError> {
    let Some(authority) = tree.state_authority.as_ref() else {
        return Ok((String::new(), false, "none".to_string()));
    };
    if authority.migration == "forward_only" {
        return Ok((String::new(), false, authority.migration.clone()));
    }
    let rollback_store = format!("{}.rollback-g{}", authority.store, tree.generation);
    service_authority_validate_text(
        &rollback_store,
        "service rollback store",
        MAX_SERVICE_STATE_STORE,
        false,
    )?;
    let records = jet_services_state_store_load(authority, &tree.state_adapter)?;
    let rollback_authority = JetServiceStateAuthority {
        store: rollback_store.clone(),
        ..authority.clone()
    };
    jet_services_state_store_write(&rollback_authority, &tree.state_adapter, &records)?;
    Ok((rollback_store, true, authority.migration.clone()))
}

fn jet_services_restore_rollback(
    tree: &mut JetServiceTree,
    receipt: &JetServiceUpgradeReceipt,
) -> Result<(), JetServiceError> {
    if !receipt.rollback_available {
        return Ok(());
    }
    let authority = tree.state_authority.as_ref().ok_or_else(|| {
        JetServiceError::Policy("upgrade receipt requires a state authority".to_string())
    })?;
    let rollback_authority = JetServiceStateAuthority {
        store: receipt.rollback_store.clone(),
        ..authority.clone()
    };
    let records = jet_services_state_store_load(&rollback_authority, &tree.state_adapter)?;
    jet_services_state_store_write(authority, &tree.state_adapter, &records)?;
    match tree.state_adapter {
        JetServiceStateAdapter::Snapshot => tree.snapshot = records.first().cloned(),
        JetServiceStateAdapter::EventLog => tree.event_log = records,
        JetServiceStateAdapter::Empty => {}
    }
    std::fs::remove_file(&receipt.rollback_store).map_err(|error| {
        jet_services_state_error(format!("cannot retire service rollback store: {error}"))
    })
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
    let (rollback_store, rollback_available, migration) = jet_services_prepare_rollback(tree)?;
    let from_generation = tree.generation;
    let to_generation = tree
        .generation
        .checked_add(1)
        .ok_or_else(|| JetServiceError::Policy("service generation exhausted".to_string()))?;
    tree.previous_generation = from_generation;
    tree.generation = to_generation;
    for worker in &mut tree.workers {
        if tree.draining.iter().any(|name| name == &worker.name)
            && !tree.partitioned.iter().any(|name| name == &worker.name)
        {
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
    for worker in &tree.workers {
        jet_services_authority_update(&worker.endpoint, worker.running)?;
    }
    tree.last_upgrade = Some(JetServiceUpgradeReceipt {
        from_generation,
        to_generation,
        migration,
        rollback_store,
        rollback_available,
        pinned_shards: Vec::new(),
    });
    tree.draining.clear();
    Ok(tree.generation)
}

fn jet_services_upgrade_receipt(
    tree: &JetServiceTree,
) -> Result<JetServiceUpgradeReceipt, JetServiceError> {
    tree.last_upgrade.clone().ok_or_else(|| {
        JetServiceError::Policy("service tree has no completed generation handoff".to_string())
    })
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
    let receipt = tree.last_upgrade.clone().ok_or_else(|| {
        JetServiceError::Policy("service tree has no rollback receipt".to_string())
    })?;
    if receipt.to_generation != tree.generation
        || receipt.from_generation != tree.previous_generation
    {
        return Err(JetServiceError::Stale(
            "service rollback receipt does not match the active generation".to_string(),
        ));
    }
    if receipt.migration == "forward_only" {
        return Err(JetServiceError::Policy(
            "forward-only state migration cannot roll back; publish a forward recovery generation"
                .to_string(),
        ));
    }
    jet_services_restore_rollback(tree, &receipt)?;
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
    for worker in &tree.workers {
        jet_services_authority_update(&worker.endpoint, worker.running)?;
    }
    tree.last_upgrade = None;
    Ok(tree.generation)
}

fn jet_services_chaos_fail(tree: &mut JetServiceTree) -> Result<i64, JetServiceError> {
    if !tree.started {
        return Err(JetServiceError::NotStarted(
            "service tree is not started".to_string(),
        ));
    }
    tree.chaos_fails = tree
        .chaos_fails
        .checked_add(1)
        .ok_or_else(|| JetServiceError::Policy("chaos failure counter exhausted".to_string()))?;
    Ok(tree.chaos_fails)
}

fn jet_services_observe(tree: &JetServiceTree) -> String {
    format!(
        "Observe(workers={}, started={}, generation={}, dead_letters={}, events={}, chaos={}, draining={}, partitions={}, rollback={})",
        tree.workers.len(),
        tree.started,
        tree.generation,
        tree.dead_letters.len(),
        tree.event_log.len(),
        tree.chaos_fails,
        tree.draining.len(),
        tree.partitioned.len()
        ,tree.last_upgrade.as_ref().is_some_and(|receipt| receipt.rollback_available)
    )
}

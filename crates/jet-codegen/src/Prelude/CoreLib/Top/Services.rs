// ── D-SERVICE1=D (#444): sema-known structured service tree over taskgroups ─
// Beginner topology: named tree → workers with typed mailboxes → OneForOne
// restart policy. Delivery is at-most-once with Full under capacity (D-SERVICE-
// DELIVERY1). Engines marshal into these Prelude symbols only (I9).

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetServiceRestart {
    OneForOne,
    OneForAll,
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

#[derive(Clone, Debug)]
struct JetServiceTree {
    name: String,
    generation: i64,
    delivery: JetServiceDelivery,
    restart: JetServiceRestart,
    workers: Vec<JetServiceWorker>,
    groups: Vec<JetServiceGroup>,
    started: bool,
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
    }
}

fn jet_services_restart_one_for_one() -> JetServiceRestart {
    JetServiceRestart::OneForOne
}

fn jet_services_restart_one_for_all() -> JetServiceRestart {
    JetServiceRestart::OneForAll
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
    }
    let _ = worker_name;
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

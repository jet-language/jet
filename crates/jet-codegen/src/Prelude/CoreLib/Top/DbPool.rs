// D-DBPOOL1: one bounded connection pool state machine for every execution tier.
// The backend supplies driver construction, health, reset, and close hooks; pool
// admission, lease ownership, deadlines, and draining stay in this Prelude.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetDbPoolLifecycle {
    Open,
    Draining,
    Drained,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDbPoolReceipt {
    pub lifecycle: JetDbPoolLifecycle,
    pub max: i64,
    pub available: i64,
    pub leased: i64,
    pub opening: i64,
    pub ready: bool,
    pub acquires: i64,
    pub releases: i64,
    pub timeouts: i64,
    pub open_failures: i64,
    pub unhealthy: i64,
    pub replacements: i64,
    pub replacement_failures: i64,
}

impl JetDbPoolReceipt {
    pub fn render(&self) -> String {
        let lifecycle = match self.lifecycle {
            JetDbPoolLifecycle::Open => "open",
            JetDbPoolLifecycle::Draining => "draining",
            JetDbPoolLifecycle::Drained => "drained",
        };
        format!(
            "db.pool lifecycle={} max={} available={} leased={} opening={} ready={} acquires={} releases={} timeouts={} open_failures={} unhealthy={} replacements={} replacement_failures={}",
            lifecycle,
            self.max,
            self.available,
            self.leased,
            self.opening,
            if self.ready { 1 } else { 0 },
            self.acquires,
            self.releases,
            self.timeouts,
            self.open_failures,
            self.unhealthy,
            self.replacements,
            self.replacement_failures,
        )
    }
}

impl JetShow for JetDbPoolReceipt {
    fn jet_show(&self) -> String {
        self.render()
    }
}

impl JetDebug for JetDbPoolReceipt {
    fn jet_debug(&self) -> String {
        self.render()
    }
}


pub struct JetDbPoolHooks<D> {
    pub open: Box<dyn Fn(&str) -> Result<D, jet_std::DBError> + Send + Sync>,
    pub health: Box<dyn Fn(&mut D) -> bool + Send + Sync>,
    pub reset: Box<dyn Fn(&mut D) -> bool + Send + Sync>,
    pub close: Box<dyn Fn(D) + Send + Sync>,
}

impl<D> JetDbPoolHooks<D> {
    pub fn new<O, H, R, C>(open: O, health: H, reset: R, close: C) -> Self
    where
        O: Fn(&str) -> Result<D, jet_std::DBError> + Send + Sync + 'static,
        H: Fn(&mut D) -> bool + Send + Sync + 'static,
        R: Fn(&mut D) -> bool + Send + Sync + 'static,
        C: Fn(D) + Send + Sync + 'static,
    {
        Self {
            open: Box::new(open),
            health: Box::new(health),
            reset: Box::new(reset),
            close: Box::new(close),
        }
    }
}

struct JetDbPoolState<D> {
    lifecycle: JetDbPoolLifecycle,
    max: usize,
    available: Vec<D>,
    leased: usize,
    opening: usize,
    waiters: std::collections::VecDeque<u64>,
    next_ticket: u64,
    acquires: u64,
    releases: u64,
    timeouts: u64,
    open_failures: u64,
    unhealthy: u64,
    replacements: u64,
    replacement_failures: u64,
}

impl<D> JetDbPoolState<D> {
    fn new(max: usize) -> Self {
        Self {
            lifecycle: JetDbPoolLifecycle::Open,
            max,
            available: Vec::new(),
            leased: 0,
            opening: 0,
            waiters: std::collections::VecDeque::new(),
            next_ticket: 0,
            acquires: 0,
            releases: 0,
            timeouts: 0,
            open_failures: 0,
            unhealthy: 0,
            replacements: 0,
            replacement_failures: 0,
        }
    }

    fn reserved(&self) -> usize {
        self.available
            .len()
            .saturating_add(self.leased)
            .saturating_add(self.opening)
    }

    fn enqueue_waiter(&mut self) -> u64 {
        let ticket = self.next_ticket;
        self.next_ticket = self.next_ticket.wrapping_add(1);
        self.waiters.push_back(ticket);
        ticket
    }

    fn waiter_is_turn(&self, ticket: u64) -> bool {
        self.waiters.front().copied() == Some(ticket)
    }

    fn claim_waiter(&mut self, ticket: u64) {
        debug_assert!(self.waiter_is_turn(ticket));
        let _ = self.waiters.pop_front();
    }

    fn remove_waiter(&mut self, ticket: u64) {
        if let Some(index) = self.waiters.iter().position(|queued| *queued == ticket) {
            let _ = self.waiters.remove(index);
        }
    }

    fn mark_drained(&mut self) {
        if self.lifecycle == JetDbPoolLifecycle::Draining
            && self.available.is_empty()
            && self.leased == 0
            && self.opening == 0
        {
            self.lifecycle = JetDbPoolLifecycle::Drained;
        }
    }

    fn receipt(&self) -> JetDbPoolReceipt {
        fn count(value: usize) -> i64 {
            i64::try_from(value).unwrap_or(i64::MAX)
        }
        fn events(value: u64) -> i64 {
            i64::try_from(value).unwrap_or(i64::MAX)
        }
        JetDbPoolReceipt {
            lifecycle: self.lifecycle,
            max: count(self.max),
            available: count(self.available.len()),
            leased: count(self.leased),
            opening: count(self.opening),
            ready: self.lifecycle == JetDbPoolLifecycle::Open
                && self.available.len().saturating_add(self.leased) > 0,
            acquires: events(self.acquires),
            releases: events(self.releases),
            timeouts: events(self.timeouts),
            open_failures: events(self.open_failures),
            unhealthy: events(self.unhealthy),
            replacements: events(self.replacements),
            replacement_failures: events(self.replacement_failures),
        }
    }
}

struct JetDbPoolInner<D: Send + 'static> {
    url: String,
    hooks: JetDbPoolHooks<D>,
    state: std::sync::Mutex<JetDbPoolState<D>>,
    wake: std::sync::Condvar,
}

pub struct JetDbPool<D: Send + 'static> {
    inner: std::sync::Arc<JetDbPoolInner<D>>,
}

impl<D: Send + 'static> Clone for JetDbPool<D> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct JetDbLease<D: Send + 'static> {
    pool: std::sync::Arc<JetDbPoolInner<D>>,
    driver: Option<D>,
    healthy: bool,
}

impl<D: Send + 'static> JetDbLease<D> {
    pub fn driver_mut(&mut self) -> &mut D {
        self.driver
            .as_mut()
            .expect("database lease already released")
    }

    pub fn mark_unhealthy(&mut self) {
        self.healthy = false;
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy
    }

    pub fn with_driver<R, F>(&mut self, operation: F) -> Result<R, jet_std::DBError>
    where
        F: FnOnce(&mut D) -> Result<R, jet_std::DBError>,
    {
        let result = {
            let driver = self.driver_mut();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(driver)))
        };
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(error),
            Err(_) => {
                self.mark_unhealthy();
                Err(jet_db_pool_error("database driver operation panicked"))
            }
        }
    }

    pub fn release(self) {
        drop(self);
    }
}

impl<D: Send + 'static> Drop for JetDbLease<D> {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            self.pool.release(driver, self.healthy);
        }
    }
}

impl<D: Send + 'static> JetDbPoolInner<D> {
    fn lock_state(&self) -> std::sync::MutexGuard<'_, JetDbPoolState<D>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn open_driver(&self) -> Result<D, jet_std::DBError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.hooks.open)(&self.url)))
            .unwrap_or_else(|_| {
                Err(jet_db_pool_error(
                    "database pool backend open operation panicked",
                ))
            })
    }

    fn health_driver(&self, driver: &mut D) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.hooks.health)(driver)))
            .unwrap_or(false)
    }

    fn reset_driver(&self, driver: &mut D) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.hooks.reset)(driver)))
            .unwrap_or(false)
    }

    fn close_driver(&self, driver: D) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.hooks.close)(driver)));
    }

    fn state_error(lifecycle: JetDbPoolLifecycle) -> jet_std::DBError {
        match lifecycle {
            JetDbPoolLifecycle::Open => jet_db_pool_error("database pool is unavailable"),
            JetDbPoolLifecycle::Draining => jet_db_pool_error("database pool is draining"),
            JetDbPoolLifecycle::Drained => jet_db_pool_error("database pool is drained"),
        }
    }

    fn acquire(
        self: &std::sync::Arc<Self>,
        deadline_ms: Option<i64>,
    ) -> Result<JetDbLease<D>, jet_std::DBError> {
        let deadline = jet_db_pool_effective_deadline(deadline_ms);
        let mut ticket = None;
        let mut state = self.lock_state();
        loop {
            if state.lifecycle != JetDbPoolLifecycle::Open {
                if let Some(ticket) = ticket.take() {
                    state.remove_waiter(ticket);
                    self.wake.notify_all();
                }
                return Err(Self::state_error(state.lifecycle));
            }

            let can_claim = ticket
                .map(|ticket| state.waiter_is_turn(ticket))
                .unwrap_or(state.waiters.is_empty());
            if can_claim {
                if let Some(mut driver) = state.available.pop() {
                    if let Some(ticket) = ticket.take() {
                        state.claim_waiter(ticket);
                    }
                    state.leased = state.leased.saturating_add(1);
                    drop(state);
                    let healthy = self.health_driver(&mut driver);
                    let expired = jet_db_pool_expired(deadline);
                    if healthy && !expired {
                        state = self.lock_state();
                        let lifecycle = state.lifecycle;
                        if lifecycle == JetDbPoolLifecycle::Open {
                            state.acquires = state.acquires.saturating_add(1);
                            drop(state);
                            return Ok(JetDbLease {
                                pool: self.clone(),
                                driver: Some(driver),
                                healthy: true,
                            });
                        }
                        state.leased = state.leased.saturating_sub(1);
                        state.mark_drained();
                        self.wake.notify_all();
                        drop(state);
                        self.close_driver(driver);
                        return Err(Self::state_error(lifecycle));
                    }
                    self.close_driver(driver);
                    state = self.lock_state();
                    state.leased = state.leased.saturating_sub(1);
                    if !healthy {
                        state.unhealthy = state.unhealthy.saturating_add(1);
                    }
                    state.mark_drained();
                    let lifecycle = state.lifecycle;
                    if expired {
                        state.timeouts = state.timeouts.saturating_add(1);
                    }
                    self.wake.notify_all();
                    drop(state);
                    if lifecycle != JetDbPoolLifecycle::Open {
                        return Err(Self::state_error(lifecycle));
                    }
                    if expired {
                        return Err(jet_db_pool_error(
                            "database pool acquisition deadline exceeded",
                        ));
                    }
                    state = self.lock_state();
                    continue;
                }

                if state.reserved() < state.max {
                    if let Some(ticket) = ticket.take() {
                        state.claim_waiter(ticket);
                    }
                    state.opening = state.opening.saturating_add(1);
                    drop(state);
                    let opened = self.open_driver();
                    let mut driver = match opened {
                        Ok(driver) => driver,
                        Err(error) => {
                            let expired = jet_db_pool_expired(deadline);
                            state = self.lock_state();
                            state.opening = state.opening.saturating_sub(1);
                            state.open_failures = state.open_failures.saturating_add(1);
                            if expired {
                                state.timeouts = state.timeouts.saturating_add(1);
                            }
                            state.mark_drained();
                            let lifecycle = state.lifecycle;
                            self.wake.notify_all();
                            drop(state);
                            if lifecycle != JetDbPoolLifecycle::Open {
                                return Err(Self::state_error(lifecycle));
                            }
                            if expired {
                                return Err(jet_db_pool_error(
                                    "database pool acquisition deadline exceeded",
                                ));
                            }
                            return Err(error);
                        }
                    };
                    let healthy = self.health_driver(&mut driver);
                    let expired = jet_db_pool_expired(deadline);
                    state = self.lock_state();
                    state.opening = state.opening.saturating_sub(1);
                    let lifecycle = state.lifecycle;
                    if lifecycle != JetDbPoolLifecycle::Open || !healthy || expired {
                        if !healthy {
                            state.unhealthy = state.unhealthy.saturating_add(1);
                        }
                        if expired {
                            state.timeouts = state.timeouts.saturating_add(1);
                        }
                        state.mark_drained();
                        self.wake.notify_all();
                        drop(state);
                        self.close_driver(driver);
                        if lifecycle != JetDbPoolLifecycle::Open {
                            return Err(Self::state_error(lifecycle));
                        }
                        if expired {
                            return Err(jet_db_pool_error(
                                "database pool acquisition deadline exceeded",
                            ));
                        }
                        return Err(jet_db_pool_error(
                            "database pool opened an unhealthy connection",
                        ));
                    }
                    state.leased = state.leased.saturating_add(1);
                    state.acquires = state.acquires.saturating_add(1);
                    drop(state);
                    return Ok(JetDbLease {
                        pool: self.clone(),
                        driver: Some(driver),
                        healthy: true,
                    });
                }
            }

            if ticket.is_none() {
                ticket = Some(state.enqueue_waiter());
            }
            match jet_db_pool_remaining(deadline) {
                Some(duration) if duration.is_zero() => {
                    if let Some(ticket) = ticket.take() {
                        state.remove_waiter(ticket);
                    }
                    state.timeouts = state.timeouts.saturating_add(1);
                    self.wake.notify_all();
                    return Err(jet_db_pool_error(
                        "database pool acquisition deadline exceeded",
                    ));
                }
                Some(duration) => {
                    state = match self.wake.wait_timeout(state, duration) {
                        Ok((guard, _)) => guard,
                        Err(poisoned) => poisoned.into_inner().0,
                    };
                }
                None => {
                    state = match self.wake.wait(state) {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            }
        }
    }

    fn ready(self: &std::sync::Arc<Self>) -> Result<bool, jet_std::DBError> {
        loop {
            let state = self.lock_state();
            if state.lifecycle != JetDbPoolLifecycle::Open {
                return Ok(false);
            }
            if state.leased > 0 {
                return Ok(true);
            }
            if state.available.is_empty() && state.opening > 0 {
                let state = match self.wake.wait(state) {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                drop(state);
                continue;
            }
            drop(state);
            let lease = self.acquire(None)?;
            drop(lease);
            return Ok(true);
        }
    }

    fn drain(
        self: &std::sync::Arc<Self>,
        deadline_ms: Option<i64>,
    ) -> Result<JetDbPoolReceipt, jet_std::DBError> {
        let deadline = jet_db_pool_effective_deadline(deadline_ms);
        {
            let mut state = self.lock_state();
            if state.lifecycle == JetDbPoolLifecycle::Open {
                state.lifecycle = JetDbPoolLifecycle::Draining;
                self.wake.notify_all();
            }
        }

        loop {
            let mut close_now = Vec::new();
            let mut state = self.lock_state();
            if !state.available.is_empty() {
                close_now.append(&mut state.available);
            }
            state.mark_drained();
            if state.lifecycle == JetDbPoolLifecycle::Drained {
                let receipt = state.receipt();
                self.wake.notify_all();
                drop(state);
                for driver in close_now {
                    self.close_driver(driver);
                }
                return Ok(receipt);
            }
            match jet_db_pool_remaining(deadline) {
                Some(duration) if duration.is_zero() => {
                    state.timeouts = state.timeouts.saturating_add(1);
                    self.wake.notify_all();
                    drop(state);
                    for driver in close_now {
                        self.close_driver(driver);
                    }
                    return Err(jet_db_pool_error(
                        "database pool drain deadline exceeded",
                    ));
                }
                Some(duration) => {
                    if close_now.is_empty() {
                        state = match self.wake.wait_timeout(state, duration) {
                            Ok((guard, _)) => guard,
                            Err(poisoned) => poisoned.into_inner().0,
                        };
                    } else {
                        drop(state);
                        for driver in close_now {
                            self.close_driver(driver);
                        }
                        self.wake.notify_all();
                        continue;
                    }
                }
                None => {
                    if close_now.is_empty() {
                        state = match self.wake.wait(state) {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                    } else {
                        drop(state);
                        for driver in close_now {
                            self.close_driver(driver);
                        }
                        self.wake.notify_all();
                        continue;
                    }
                }
            }
            drop(state);
        }
    }

    fn release(&self, mut driver: D, mut healthy: bool) {
        if healthy {
            healthy = self.reset_driver(&mut driver);
        }

        let mut replace = false;
        {
            let mut state = self.lock_state();
            state.leased = state.leased.saturating_sub(1);
            state.releases = state.releases.saturating_add(1);
            if state.lifecycle == JetDbPoolLifecycle::Open && healthy {
                state.available.push(driver);
                self.wake.notify_all();
                return;
            }
            if !healthy {
                state.unhealthy = state.unhealthy.saturating_add(1);
                replace = state.lifecycle == JetDbPoolLifecycle::Open;
            }
            state.mark_drained();
            self.wake.notify_all();
        }

        self.close_driver(driver);
        if replace {
            self.replace_one();
        }
    }

    fn replace_one(&self) {
        {
            let mut state = self.lock_state();
            if state.lifecycle != JetDbPoolLifecycle::Open || state.reserved() >= state.max {
                return;
            }
            state.opening = state.opening.saturating_add(1);
        }

        let opened = self.open_driver();
        let mut driver = match opened {
            Ok(driver) => driver,
            Err(_) => {
                let mut state = self.lock_state();
                state.opening = state.opening.saturating_sub(1);
                if state.lifecycle == JetDbPoolLifecycle::Open {
                    state.replacement_failures = state.replacement_failures.saturating_add(1);
                }

                state.mark_drained();
                self.wake.notify_all();
                return;
            }
        };
        let healthy = self.health_driver(&mut driver);
        let mut driver = Some(driver);
        let mut state = self.lock_state();
        state.opening = state.opening.saturating_sub(1);
        if !healthy {
            state.unhealthy = state.unhealthy.saturating_add(1);
            if state.lifecycle == JetDbPoolLifecycle::Open {
                state.replacement_failures = state.replacement_failures.saturating_add(1);
            }
        } else if state.lifecycle == JetDbPoolLifecycle::Open && state.reserved() < state.max {
            if let Some(driver) = driver.take() {
                state.available.push(driver);
            }
            state.replacements = state.replacements.saturating_add(1);
        }
        state.mark_drained();
        self.wake.notify_all();
        drop(state);
        if let Some(driver) = driver {
            self.close_driver(driver);
        }
    }
}
impl<D: Send + 'static> Drop for JetDbPoolInner<D> {
    fn drop(&mut self) {
        let available = {
            let state = self
                .state
                .get_mut()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut state.available)
        };
        for driver in available {
            self.close_driver(driver);
        }
    }
}

impl<D: Send + 'static> JetDbPool<D> {
    pub fn new(
        url: String,
        max: i64,
        hooks: JetDbPoolHooks<D>,
    ) -> Result<Self, jet_std::DBError> {
        if max <= 0 {
            return Err(jet_db_pool_error(
                "database pool max must be greater than zero",
            ));
        }
        let max = usize::try_from(max).map_err(|_| {
            jet_db_pool_error("database pool max is outside the supported range")
        })?;
        Ok(Self {
            inner: std::sync::Arc::new(JetDbPoolInner {
                url,
                hooks,
                state: std::sync::Mutex::new(JetDbPoolState::new(max)),
                wake: std::sync::Condvar::new(),
            }),
        })
    }

    pub fn acquire(&self, deadline_ms: Option<i64>) -> Result<JetDbLease<D>, jet_std::DBError> {
        self.inner.acquire(deadline_ms)
    }

    pub fn ready(&self) -> Result<bool, jet_std::DBError> {
        self.inner.ready()
    }

    pub fn drain(
        &self,
        deadline_ms: Option<i64>,
    ) -> Result<JetDbPoolReceipt, jet_std::DBError> {
        self.inner.drain(deadline_ms)
    }

    pub fn receipt(&self) -> JetDbPoolReceipt {
        self.inner.lock_state().receipt()
    }
}

fn jet_db_pool_error(message: &str) -> jet_std::DBError {
    jet_std::DBError {
        message: message.to_string(),
    }
}


const JET_DB_POOL_DEFAULT_TIMEOUT_MS: i64 = 30_000;
fn jet_db_pool_effective_deadline(deadline: Option<i64>) -> Option<i64> {
    match (deadline, jet_ctx_deadline_ms()) {
        (Some(explicit), Some(context)) => Some(explicit.min(context)),
        (Some(explicit), None) => Some(explicit),
        (None, Some(context)) => Some(context),
        (None, None) => Some(
            jet_std_time_now().saturating_add(JET_DB_POOL_DEFAULT_TIMEOUT_MS),
        ),
    }
}

fn jet_db_pool_expired(deadline: Option<i64>) -> bool {
    deadline.is_some_and(|deadline| deadline <= jet_std_time_now())
}

fn jet_db_pool_remaining(deadline: Option<i64>) -> Option<std::time::Duration> {
    deadline.map(|deadline| {
        let remaining = deadline.saturating_sub(jet_std_time_now());
        if remaining <= 0 {
            std::time::Duration::from_millis(0)
        } else {
            std::time::Duration::from_millis(remaining as u64)
        }
    })
}

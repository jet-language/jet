fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// Tier-neutral live publication lifecycle. Payload ownership and execution
/// remain in each adapter; generation, cancellation, and publication rules do
/// not.
#[derive(Clone, Debug)]
pub struct JetLiveLifecycle {
    pub generation: u64,
    pub active: bool,
    pub dirty: bool,
    pub error: String,
    pub fresh_at_ms: u64,
    pub invalidation_cause: String,
    pub refreshing: bool,
    pub cancelled: bool,
}

impl JetLiveLifecycle {
    pub fn active(now_ms: u64) -> Self {
        Self {
            generation: 1,
            active: true,
            dirty: false,
            error: String::new(),
            fresh_at_ms: now_ms,
            invalidation_cause: String::new(),
            refreshing: false,
            cancelled: false,
        }
    }

    pub fn active_now() -> Self {
        Self::active(now_ms())
    }

    pub fn error(error: &str) -> Self {
        Self {
            generation: 0,
            active: false,
            dirty: false,
            error: error.to_string(),
            fresh_at_ms: 0,
            invalidation_cause: String::new(),
            refreshing: false,
            cancelled: false,
        }
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.active && !self.cancelled && self.generation == generation
    }

    pub fn invalidate(&mut self, cause: &str) -> bool {
        if !self.active {
            return false;
        }
        self.generation = self.generation.saturating_add(1);
        self.dirty = true;
        self.cancelled = false;
        self.error.clear();
        self.invalidation_cause = cause.to_string();
        true
    }

    pub fn begin_refresh(&mut self, generation: u64) -> bool {
        if !self.is_current(generation) || self.refreshing {
            return false;
        }
        self.refreshing = true;
        true
    }

    pub fn publish(&mut self, generation: u64, fresh_at_ms: u64) -> bool {
        let current = self.is_current(generation);
        self.refreshing = false;
        if !current {
            return false;
        }
        self.dirty = false;
        self.error.clear();
        self.fresh_at_ms = fresh_at_ms;
        true
    }

    pub fn fail(&mut self, generation: u64, error: String) -> bool {
        let current = self.is_current(generation);
        self.refreshing = false;
        if !current {
            return false;
        }
        self.error = error;
        self.dirty = true;
        true
    }

    pub fn cancel(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.generation = self.generation.saturating_add(1);
        self.dirty = true;
        self.refreshing = false;
        self.cancelled = true;
        self.invalidation_cause = "cancelled".to_string();
        true
    }

    pub fn close(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.refreshing = false;
        self.cancelled = true;
        true
    }

    pub fn freshness_ms(&self) -> u64 {
        now_ms().saturating_sub(self.fresh_at_ms)
    }
}

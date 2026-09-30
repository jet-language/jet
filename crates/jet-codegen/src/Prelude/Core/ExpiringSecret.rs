// The one secret-lifetime
// wrapper. T is sema-restricted to existing move-only, zeroizing secret types.
struct JetExpiringSecret<T> {
    value: std::sync::Mutex<Option<T>>,
    deadline_ms: i64,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}
impl<T> JetExpiringSecret<T> {
    fn new<F>(value: T, ttl_ms: i64, clock: F) -> Self
    where
        F: Fn() -> i64 + Send + Sync + 'static,
    {
        let deadline_ms = clock().saturating_add(ttl_ms);
        Self {
            value: std::sync::Mutex::new(Some(value)),
            deadline_ms,
            clock: Box::new(clock),
        }
    }

    fn with<F, R>(&self, callback: F) -> Result<R, JetExpired>
    where
        F: FnOnce(&T) -> R,
    {
        let mut value = self.value.lock().unwrap_or_else(|error| error.into_inner());
        if (self.clock)() > self.deadline_ms {
            value.take();
        }
        match value.as_ref() {
            Some(value) => Ok(callback(value)),
            None => Err(JetExpired),
        }
    }
}

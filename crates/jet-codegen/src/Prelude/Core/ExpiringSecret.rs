// D-TTLVAL1=A / D-TTL-CLOCK2=A / D-TTL-ZEROIZE1=A: the one secret-lifetime
// wrapper. T is sema-restricted to existing move-only, zeroizing secret types.
struct JetExpiringSecret<T> {
    value: Option<T>,
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
            value: Some(value),
            deadline_ms,
            clock: Box::new(clock),
        }
    }

    fn with<F, R>(&mut self, callback: F) -> Result<R, JetExpired>
    where
        F: FnOnce(&T) -> R,
    {
        if self.value.is_none() || (self.clock)() > self.deadline_ms {
            self.value.take();
            return Err(JetExpired);
        }
        Ok(callback(self.value.as_ref().expect("checked above")))
    }
}
impl<T> Drop for JetExpiringSecret<T> {
    fn drop(&mut self) {
        self.value.take();
    }
}

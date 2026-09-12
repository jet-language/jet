// Checked Shared host adapters. JetShared owns the lock protocol.
#[inline]
pub fn jet_shared_get<T: Clone + 'static>(shared: &jet_std::JetShared<T>) -> T {
    shared.get()
}

#[inline]
pub fn jet_shared_set<T: 'static>(shared: &jet_std::JetShared<T>, value: T) {
    shared.set(value);
}

#[inline]
pub fn jet_shared_replace<T: 'static>(shared: &jet_std::JetShared<T>, value: T) -> T {
    shared.replace(value)
}

#[inline]
pub fn jet_shared_read<T: 'static, F, R>(shared: &jet_std::JetShared<T>, callback: F) -> R
where
    F: FnOnce(&T) -> R,
{
    shared.read(callback)
}

#[inline]
pub fn jet_shared_edit<T: 'static, F, R>(shared: &jet_std::JetShared<T>, callback: F) -> R
where
    F: FnOnce(&mut T) -> R,
{
    shared.edit(callback)
}

#[inline]
pub fn jet_shared_read_txn<T: 'static, F, R>(
    shared: &jet_std::JetShared<T>,
    stm: &mut crate::JetSharedTransaction,
    callback: F,
) -> R
where
    F: FnOnce(&T) -> R,
{
    shared.read_txn(stm, callback)
}

#[inline]
pub fn jet_shared_edit_txn<T: Clone + 'static, F>(
    shared: &jet_std::JetShared<T>,
    stm: &mut crate::JetSharedTransaction,
    callback: F,
)
where
    F: FnOnce(&mut T) + 'static,
{
    shared.edit_txn(stm, callback)
}

#[inline]
pub fn jet_shared_capture<T: Clone + 'static>(
    shared: &jet_std::JetShared<T>,
) -> jet_std::JetSharedSnapshot<T, T> {
    shared.capture()
}

#[inline]
pub fn jet_shared_capture_with<T: 'static, U, F>(
    shared: &jet_std::JetShared<T>,
    project: F,
) -> jet_std::JetSharedSnapshot<T, U>
where
    F: FnOnce(&T) -> U,
{
    shared.capture_with(project)
}

#[inline]
pub fn jet_shared_capture_txn_plain<T: Clone + 'static>(
    shared: &jet_std::JetShared<T>,
    stm: &mut crate::JetSharedTransaction,
) -> jet_std::JetSharedSnapshot<T, T> {
    shared.capture_txn(stm, Clone::clone)
}

#[inline]
pub fn jet_shared_capture_txn<T: 'static, U, F>(
    shared: &jet_std::JetShared<T>,
    stm: &mut crate::JetSharedTransaction,
    project: F,
) -> jet_std::JetSharedSnapshot<T, U>
where
    F: FnOnce(&T) -> U,
{
    shared.capture_txn(stm, project)
}

#[inline]
pub fn jet_shared_try_replace<T: 'static, U>(
    shared: &jet_std::JetShared<T>,
    snapshot: jet_std::JetSharedSnapshot<T, U>,
    value: T,
) -> Result<bool, jet_std::JetSharedRevisionError> {
    shared.try_replace(snapshot, value)
}

#[inline]
pub fn jet_shared_snapshot_value<T: 'static, U: Clone>(
    snapshot: &jet_std::JetSharedSnapshot<T, U>,
) -> U {
    snapshot.value()
}

#[inline]
pub fn jet_shared_guard_read<T: 'static>(
    shared: &jet_std::JetShared<T>,
) -> jet_std::JetSharedGuard<T> {
    shared.guard_read()
}

#[inline]
pub fn jet_shared_guard_edit<T: 'static>(
    shared: &jet_std::JetShared<T>,
) -> jet_std::JetSharedGuard<T> {
    shared.guard_edit()
}

#[inline]
pub fn jet_shared_downgrade<T: 'static>(
    shared: &jet_std::JetShared<T>,
) -> jet_std::JetSharedWeak<T> {
    shared.downgrade()
}

#[inline]
pub fn jet_shared_strong_count<T: 'static>(shared: &jet_std::JetShared<T>) -> i64 {
    shared.strong_count()
}

#[inline]
pub fn jet_shared_weak_upgrade<T: 'static>(
    weak: &jet_std::JetSharedWeak<T>,
) -> crate::JetOutcome<jet_std::JetShared<T>, crate::JetAbsent> {
    weak.upgrade()
}

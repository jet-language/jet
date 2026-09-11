// D-CONC-SHARE1=A / D-CONC-STM1=A: the browser adapter for Shared<T>.
// JavaScript runs one event-loop turn at a time, so there is no OS lock to
// acquire. Stable ids still give the transaction commit the same participant
// order as the native SharedProtocol, and the body is never retried.
let jet_shared_next_id = 0;
const jet_shared_snapshots = new WeakMap();
const jet_shared_max_revision = 18446744073709551615n;

function jet_shared_revision_error(kind) {
  const error = new Error(`SharedRevisionError.${kind}`);
  error.name = "SharedRevisionError";
  error.code = kind;
  return error;
}

function jet_shared_next_revision(shared) {
  const next = shared.revision + 1n;
  if (next > jet_shared_max_revision) {
    throw jet_shared_revision_error("GenerationExhausted");
  }
  return next;
}

function jet_shared_snapshot_state(snapshot) {
  const state = snapshot != null && typeof snapshot === "object"
    ? jet_shared_snapshots.get(snapshot)
    : undefined;
  if (!state) throw jet_shared_revision_error("WrongOwner");
  return state;
}

function jet_shared_snapshot_new(shared, revision, value) {
  const snapshot = Object.freeze({ kind: "shared-snapshot" });
  jet_shared_snapshots.set(snapshot, {
    shared,
    revision,
    value,
    valid: true,
    consumed: false
  });
  return snapshot;
}

function jet_shared_stm_state(stm) {
  if (!stm || typeof stm !== "object" || !(stm.parts instanceof Map)) {
    throw new Error("invalid Web STM handle");
  }
  if (jet_web_stm_stack[jet_web_stm_stack.length - 1] !== stm) {
    throw new Error("Shared operation outside #Transact");
  }
  return stm;
}

function jet_shared_stm_touch(shared, stm) {
  const checked = jet_shared_stm_state(stm);
  let part = checked.parts.get(shared);
  if (!part) {
    part = { shared, value: undefined, writes: false, snapshots: [] };
    checked.parts.set(shared, part);
  }
  return checked;
}

function jet_shared_stm_part(transaction, shared) {
  return transaction.parts.get(shared);
}

function jet_shared_stm_value(transaction, shared) {
  for (let cursor = transaction; cursor; cursor = cursor.parent) {
    const part = jet_shared_stm_part(cursor, shared);
    if (part && part.writes) return part.value;
  }
  return shared.value;
}

function jet_shared_stm_has_writes(transaction, shared) {
  for (let cursor = transaction; cursor; cursor = cursor.parent) {
    const part = jet_shared_stm_part(cursor, shared);
    if (part && part.writes) return true;
  }
  return false;
}

function jet_shared_stm_invalidate_snapshots(part) {
  for (const snapshot of part.snapshots) {
    jet_shared_snapshot_state(snapshot).valid = false;
  }
}

function jet_shared_stm_stage_for_write(shared) {
  const transaction = jet_web_stm_stack[jet_web_stm_stack.length - 1];
  if (!transaction) return undefined;
  const checked = jet_shared_state(shared);
  jet_shared_stm_state(transaction);
  const part = jet_shared_stm_touch(checked, transaction).parts.get(checked);
  jet_shared_stm_invalidate_snapshots(part);
  if (!part.writes) {
    part.value = jet_web_clone(jet_shared_stm_value(transaction, checked), checked.copy_key);
    part.writes = true;
  }
  return { transaction, part };
}

function jet_web_clone(value, copy_key) {
  return jet_copy_value(value, copy_key, __jet_copy_type_facts);
}

function jet_shared_wrap(shared, value, seen = new WeakMap()) {
  if (value == null || typeof value !== "object") return value;
  const existing = seen.get(value);
  if (existing) return existing;
  const wrapped = new Proxy(value, {
    get(target, property, receiver) {
      return Reflect.get(target, property, receiver);
    },
    set(target, property, replacement, receiver) {
      const next = jet_shared_next_revision(shared);
      const wrappedReplacement = jet_shared_wrap(shared, replacement, seen);
      const changed = Reflect.set(target, property, wrappedReplacement, receiver);
      if (changed) shared.revision = next;
      return changed;
    }
  });
  seen.set(value, wrapped);
  return wrapped;
}

function jet_shared_capture_value(shared, project) {
  const revision = shared.revision;
  const value = project === undefined
    ? jet_web_clone(shared.value, shared.copy_key)
    : jet_web_clone(project(shared.value), shared.copy_key);
  return jet_shared_snapshot_new(shared, revision, value);
}

function jet_shared_snapshot_value(snapshot) {
  const state = jet_shared_snapshot_state(snapshot);
  return jet_web_clone(state.value, state.shared.copy_key);
}

function jet_shared_capture(shared) {
  return jet_shared_capture_value(jet_shared_state(shared), undefined);
}

function jet_shared_capture_with(shared, project) {
  const checked = jet_shared_state(shared);
  if (typeof project !== "function") throw new Error("invalid Web Shared capture callback");
  return jet_shared_capture_value(checked, project);
}

function jet_shared_capture_txn(shared, stm, project) {
  const checked = jet_shared_state(shared);
  const transaction = jet_shared_stm_touch(checked, stm);
  const part = transaction.parts.get(checked);
  const local = jet_shared_stm_value(transaction, checked);
  const revision = jet_shared_stm_has_writes(transaction, checked)
    ? jet_shared_next_revision(checked)
    : checked.revision;
  const value = project === undefined
    ? jet_web_clone(local, checked.copy_key)
    : jet_web_clone(project(local), checked.copy_key);
  const snapshot = jet_shared_snapshot_new(checked, revision, value);
  part.snapshots.push(snapshot);
  transaction.rollback_hooks.push(() => {
    jet_shared_snapshot_state(snapshot).valid = false;
  });
  return snapshot;
}

function jet_shared_try_replace(shared, snapshot, value) {
  const checked = jet_shared_state(shared);
  const state = jet_shared_snapshot_state(snapshot);
  if (state.shared !== checked) throw jet_shared_revision_error("WrongOwner");
  if (!state.valid || state.consumed || state.revision !== checked.revision) return false;
  const next = jet_shared_next_revision(checked);
  state.consumed = true;
  state.valid = false;
  checked.value = jet_shared_wrap(checked, value);
  checked.revision = next;
  return true;
}

function jet_shared_new(value, copy_key) {
  if (!__jet_copy_type_facts.has(copy_key)) {
    throw new TypeError("checked Web Shared value has no payload type fact");
  }
  const shared = { kind: "shared", id: jet_shared_next_id++, revision: 0n, value: undefined };
  Object.defineProperty(shared, "copy_key", {
    value: copy_key,
    enumerable: false,
  });
  shared.value = jet_shared_wrap(shared, value);
  return shared;
}
function jet_shared_get(shared) {
  const checked = jet_shared_state(shared);
  const transaction = jet_web_stm_stack[jet_web_stm_stack.length - 1];
  return jet_web_clone(
    transaction ? jet_shared_stm_value(transaction, checked) : checked.value,
    checked.copy_key,
  );
}

function jet_shared_set(shared, value) {
  const checked = jet_shared_state(shared);
  const staged = jet_shared_stm_stage_for_write(checked);
  if (staged) {
    staged.part.value = jet_web_clone(value, checked.copy_key);
    return;
  }
  const next = jet_shared_next_revision(checked);
  checked.value = jet_shared_wrap(checked, value);
  checked.revision = next;
}

function jet_shared_replace(shared, value) {
  const checked = jet_shared_state(shared);
  const staged = jet_shared_stm_stage_for_write(checked);
  if (staged) {
    const old = jet_web_clone(staged.part.value, checked.copy_key);
    staged.part.value = jet_web_clone(value, checked.copy_key);
    return old;
  }
  const old = jet_web_clone(checked.value, checked.copy_key);
  const next = jet_shared_next_revision(checked);
  checked.value = jet_shared_wrap(checked, value);
  checked.revision = next;
  return old;
}

function jet_shared_state(shared) {
  if (
    shared == null ||
    typeof shared !== "object" ||
    shared.kind !== "shared" ||
    !("value" in shared) ||
    typeof shared.copy_key !== "string"
  ) {
    throw new Error("invalid Web Shared value");
  }
  return shared;
}
function jet_shared_guard_read(shared) {
  const checked = jet_shared_state(shared);
  return { kind: "shared_guard", shared: checked, path: [], editable: false, active: true };
}

function jet_shared_guard_edit(shared) {
  const checked = jet_shared_state(shared);
  return { kind: "shared_guard", shared: checked, path: [], editable: true, active: true };
}

function jet_shared_read(shared, callback) {
  const checked = jet_shared_state(shared);
  if (typeof callback !== "function") throw new Error("invalid Web Shared read callback");
  const transaction = jet_web_stm_stack[jet_web_stm_stack.length - 1];
  return callback(transaction
    ? jet_shared_stm_value(transaction, checked)
    : checked.value);
}

function jet_shared_edit(shared, callback) {
  const checked = jet_shared_state(shared);
  if (typeof callback !== "function") throw new Error("invalid Web Shared edit callback");
  const staged = jet_shared_stm_stage_for_write(checked);
  if (staged) {
    const result = callback(staged.part.value);
    if (result !== undefined) staged.part.value = result;
    return result;
  }
  const next = jet_shared_next_revision(checked);
  const value = jet_web_clone(checked.value, checked.copy_key);
  const result = callback(value);
  checked.value = jet_shared_wrap(checked, value);
  checked.revision = next;
  return result;
}
function jet_shared_guard_assert(guard, editable) {
  if (!guard || guard.kind !== "shared_guard" || !guard.active) {
    throw new Error("Shared guard is no longer active");
  }
  if (editable && !guard.editable) {
    throw new Error("Shared guard is not editable");
  }
}

function jet_shared_guard_map(guard, field, editable) {
  jet_shared_guard_assert(guard, editable);
  const mapped = {
    kind: "shared_guard",
    shared: guard.shared,
    path: [...guard.path, field],
    editable,
    active: true
  };
  guard.active = false;
  return mapped;
}

function jet_shared_guard_split(guard, first, second, editable) {
  jet_shared_guard_assert(guard, editable);
  const firstGuard = {
    kind: "shared_guard",
    shared: guard.shared,
    path: [...guard.path, first],
    editable,
    active: true
  };
  const secondGuard = {
    kind: "shared_guard",
    shared: guard.shared,
    path: [...guard.path, second],
    editable,
    active: true
  };
  guard.active = false;
  return [firstGuard, secondGuard];
}

function jet_cell_guard_assert(guard) {
  if (!guard || guard.kind !== "cell_guard" || !guard.active) {
    throw new Error("Cell guard is no longer active");
  }
}

function jet_cell_guard_map(guard, field) {
  jet_cell_guard_assert(guard);
  const mapped = {
    kind: "cell_guard",
    cell: guard.cell,
    path: [...guard.path, field],
    editable: guard.editable,
    active: true
  };
  guard.active = false;
  return mapped;
}

function jet_cell_guard_split(guard, first, second) {
  jet_cell_guard_assert(guard);
  const firstGuard = {
    kind: "cell_guard",
    cell: guard.cell,
    path: [...guard.path, first],
    editable: guard.editable,
    active: true
  };
  const secondGuard = {
    kind: "cell_guard",
    cell: guard.cell,
    path: [...guard.path, second],
    editable: guard.editable,
    active: true
  };
  guard.active = false;
  return [firstGuard, secondGuard];
}

const jet_web_stm_stack = [];

function jet_stm_begin() {
  const transaction = {
    parts: new Map(),
    rollback_hooks: [],
    parent: jet_web_stm_stack[jet_web_stm_stack.length - 1] || null
  };
  jet_web_stm_stack.push(transaction);
  return transaction;
}

function jet_shared_read_txn(shared, stm, callback) {
  const checked = jet_shared_state(shared);
  const transaction = jet_shared_stm_touch(checked, stm);
  if (typeof callback !== "function") throw new Error("invalid Web Shared transaction callback");
  return callback(jet_shared_stm_value(transaction, checked));
}

function jet_shared_edit_txn(shared, stm, callback) {
  const checked = jet_shared_state(shared);
  const transaction = jet_shared_stm_touch(checked, stm);
  if (typeof callback !== "function") throw new Error("invalid Web Shared transaction callback");
  const part = transaction.parts.get(checked);
  jet_shared_stm_invalidate_snapshots(part);
  if (!part.writes) {
    part.value = jet_web_clone(jet_shared_stm_value(transaction, checked), checked.copy_key);
    part.writes = true;
  }
  const result = callback(part.value);
  if (result !== undefined) part.value = result;
}

function jet_shared_strong_count(shared) {
  jet_shared_state(shared);
  return 1n;
}

function jet_shared_downgrade(shared) {
  const checked = jet_shared_state(shared);
  if (typeof WeakRef !== "function") throw new Error("Web runtime lacks WeakRef");
  return { kind: "shared_weak", ref: new WeakRef(checked) };
}

function jet_shared_weak_upgrade(weak) {
  if (weak == null || typeof weak !== "object" || weak.kind !== "shared_weak" || !(weak.ref instanceof WeakRef)) {
    throw new Error("invalid Web Shared.Weak value");
  }
  const shared = weak.ref.deref();
  return shared === undefined
    ? jet_option_none()
    : jet_option_some(shared);
}

function jet_stm_commit(transaction) {
  if (jet_web_stm_stack[jet_web_stm_stack.length - 1] !== transaction) return;
  jet_web_stm_stack.pop();
  if (transaction.parent) {
    for (const part of transaction.parts.values()) {
      let parentPart = transaction.parent.parts.get(part.shared);
      if (!parentPart) {
        parentPart = { shared: part.shared, value: undefined, writes: false, snapshots: [] };
        transaction.parent.parts.set(part.shared, parentPart);
      }
      if (part.writes) {
        jet_shared_stm_invalidate_snapshots(parentPart);
        parentPart.value = part.value;
        parentPart.writes = true;
      }
      parentPart.snapshots.push(...part.snapshots);
    }
    transaction.parent.rollback_hooks.push(...transaction.rollback_hooks);
    return;
  }
  const parts = [...transaction.parts.values()]
    .sort((left, right) => left.shared.id - right.shared.id);
  for (const part of parts) {
    if (part.writes) {
      const next = jet_shared_next_revision(part.shared);
      part.shared.value = jet_shared_wrap(part.shared, part.value);
      part.shared.revision = next;
    }
  }
}

function jet_stm_abort(transaction) {
  if (jet_web_stm_stack[jet_web_stm_stack.length - 1] === transaction) {
    for (const callback of transaction.rollback_hooks.slice().reverse()) callback();
    jet_web_stm_stack.pop();
  }
}

function jet_stm_abort_if_active(transaction) {
  jet_stm_abort(transaction);
}

function jet_transaction() {
  return {
    committed: false,
    rolled_back: false,
    commits: [],
    rollbacks: [],
    async commit() {
      if (this.committed || this.rolled_back) return;
      this.committed = true;
      for (const callback of this.commits.slice().reverse()) await callback();
    },
    async rollback() {
      if (this.committed || this.rolled_back) return;
      this.rolled_back = true;
      for (const callback of this.rollbacks.slice().reverse()) await callback();
    },
    on_commit(callback) { this.commits.push(callback); },
    on_rollback(callback) { this.rollbacks.push(callback); }
  };
}

function jet_transaction_on_commit(transaction, callback) {
  transaction.on_commit(callback);
  return { tag: "TransactionGuard", values: [] };
}

function jet_transaction_on_rollback(transaction, callback) {
  transaction.on_rollback(callback);
  return { tag: "TransactionGuard", values: [] };
}

// D-CONC-SHARE1=A / D-CONC-STM1=A: the browser adapter for Shared<T>.
// JavaScript runs one event-loop turn at a time, so there is no OS lock to
// acquire. Stable ids still give the transaction commit the same participant
// order as the native SharedProtocol, and the body is never retried.
let jet_shared_next_id = 0;
function jet_web_clone(value) {
  if (typeof structuredClone === "function") {
    try { return structuredClone(value); } catch (_) {}
  }
  if (Array.isArray(value)) return value.map(jet_web_clone);
  if (value && typeof value === "object") {
    const copy = {};
    for (const [key, item] of Object.entries(value)) copy[key] = jet_web_clone(item);
    return copy;
  }
  return value;
}

function jet_shared_new(value) {
  return { kind: "shared", id: jet_shared_next_id++, value };
}

function jet_shared_read(shared, callback) {
  return callback(shared.value);
}

function jet_shared_edit(shared, callback) {
  const next = jet_web_clone(shared.value);
  const result = callback(next);
  shared.value = next;
  return result;
}

const jet_web_stm_stack = [];

function jet_stm_begin() {
  const transaction = { parts: new Map() };
  jet_web_stm_stack.push(transaction);
  return transaction;
}

function jet_shared_read_txn(shared, callback) {
  const transaction = jet_web_stm_stack[jet_web_stm_stack.length - 1];
  if (!transaction) throw new Error("Shared read outside #Transact");
  if (!transaction.parts.has(shared)) {
    transaction.parts.set(shared, { shared, value: undefined, callbacks: [], writes: false });
  }
  return callback(shared.value);
}

function jet_shared_edit_txn(shared, callback) {
  const transaction = jet_web_stm_stack[jet_web_stm_stack.length - 1];
  if (!transaction) throw new Error("Shared edit outside #Transact");
  let part = transaction.parts.get(shared);
  if (!part) {
    part = { shared, value: jet_web_clone(shared.value), callbacks: [], writes: true };
    transaction.parts.set(shared, part);
  } else if (!part.writes) {
    part.value = jet_web_clone(shared.value);
    part.writes = true;
  }
  part.callbacks.push(callback);
}

function jet_stm_commit(transaction) {
  if (jet_web_stm_stack[jet_web_stm_stack.length - 1] !== transaction) return;
  const parts = [...transaction.parts.values()].sort((left, right) => left.shared.id - right.shared.id);
  for (const part of parts) {
    let value = part.value;
    for (const callback of part.callbacks) {
      const result = callback(value);
      if (result !== undefined) value = result;
    }
    if (part.writes) part.shared.value = value;
  }
  jet_web_stm_stack.pop();
}

function jet_stm_abort(transaction) {
  if (jet_web_stm_stack[jet_web_stm_stack.length - 1] === transaction) {
    jet_web_stm_stack.pop();
  }
}

function jet_stm_abort_if_active(transaction) {
  jet_stm_abort(transaction);
}

function jet_shared_strong_count(_) { return 1n; }
function jet_shared_downgrade(shared) { return shared; }

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

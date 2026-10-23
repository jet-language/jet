// D-HOST-MIR1: checked Pool/Cell host kernels. Route rows select these
// symbols before JavaScript runs; this file does not inspect source names or
// perform receiver/member dispatch.
function jet_pool_state(pool) {
  if (
    pool == null ||
    typeof pool !== "object" ||
    pool.kind !== "pool" ||
    !Array.isArray(pool.slots) ||
    !Array.isArray(pool.free)
  ) {
    throw new Error("invalid Web Pool");
  }
  return pool;
}

function jet_pool_new() {
  return { kind: "pool", slots: [], free: [] };
}

function jet_pool_add(pool, value) {
  const checked = jet_pool_state(pool);
  let index;
  let generation;
  if (checked.free.length !== 0) {
    index = checked.free.pop();
    const slot = checked.slots[index];
    if (slot == null || slot.occupied) throw new Error("invalid Web Pool free slot");
    generation = slot.generation;
    checked.slots[index] = { occupied: true, generation, value };
  } else {
    index = checked.slots.length;
    generation = 0;
    checked.slots.push({ occupied: true, generation, value });
  }
  return { index, generation };
}

function jet_pool_remove(pool, id) {
  const checked = jet_pool_state(pool);
  if (id == null || typeof id !== "object") {
    throw new Error("invalid Web Pool id");
  }
  const index = Number(id.index);
  const generation = Number(id.generation);
  const slot = Number.isInteger(index) ? checked.slots[index] : undefined;
  if (slot == null || !slot.occupied || slot.generation !== generation) {
    return jet_option_none();
  }
  const value = slot.value;
  checked.slots[index] = {
    occupied: false,
    generation: (generation + 1) >>> 0,
    value: undefined,
  };
  checked.free.push(index);
  return jet_option_some(value);
}

function jet_pool_ids(pool) {
  const checked = jet_pool_state(pool);
  return checked.slots.reduce((ids, slot, index) => {
    if (slot.occupied) ids.push({ index, generation: slot.generation });
    return ids;
  }, []);
}

function jet_pool_get(pool, id) {
  const checked = jet_pool_state(pool);
  if (id == null || typeof id !== "object") throw new Error("invalid Web Pool id");
  const index = Number(id.index);
  const slot = Number.isInteger(index) ? checked.slots[index] : undefined;
  if (slot == null || !slot.occupied || slot.generation !== Number(id.generation)) {
    throw new Error("stale Web Pool id");
  }
  return slot.value;
}

function jet_pool_get_mut(pool, id) {
  return jet_pool_get(pool, id);
}

function jet_pool_set(pool, id, value) {
  const checked = jet_pool_state(pool);
  if (id == null || typeof id !== "object") throw new Error("invalid Web Pool id");
  const index = Number(id.index);
  const slot = Number.isInteger(index) ? checked.slots[index] : undefined;
  if (slot == null || !slot.occupied || slot.generation !== Number(id.generation)) {
    throw new Error("stale Web Pool id");
  }
  slot.value = value;
}

function jet_cell_state(cell) {
  if (cell == null || typeof cell !== "object" || cell.kind !== "cell" || !("value" in cell)) {
    throw new Error("invalid Web Cell");
  }
  return cell;
}

function jet_cell_new(value) {
  return { kind: "cell", value };
}

function jet_cell_get(cell) {
  return jet_web_clone(jet_cell_state(cell).value);
}

function jet_cell_set(cell, value) {
  jet_cell_state(cell).value = value;
}

function jet_cell_replace(cell, value) {
  const checked = jet_cell_state(cell);
  const old = checked.value;
  checked.value = value;
  return old;
}

function jet_cell_read(cell, callback) {
  const checked = jet_cell_state(cell);
  if (typeof callback !== "function") throw new Error("invalid Web Cell read callback");
  return callback(checked.value);
}

function jet_cell_edit(cell, callback) {
  const checked = jet_cell_state(cell);
  if (typeof callback !== "function") throw new Error("invalid Web Cell edit callback");
  const next = jet_web_clone(checked.value);
  const result = callback(next);
  checked.value = next;
  return result;
}

function jet_cell_get_or_set(cell, initializer) {
  const checked = jet_cell_state(cell);
  if (typeof initializer !== "function") throw new Error("invalid Web Cell initializer");
  const current = checked.value;
  if (current != null && current.tag === "Ok" && Array.isArray(current.values) && current.values.length === 1) {
    return current.values[0];
  }
  if (
    current == null ||
    current.tag !== "Err" ||
    !Array.isArray(current.values) ||
    current.values.length === 0 ||
    !jet_is_absent(current.values[0])
  ) {
    throw new Error("Web Cell get_or_set requires an optional carrier");
  }
  const value = initializer();
  checked.value = jet_option_some(value);
  return value;
}

function jet_cell_guard_target(guard) {
  jet_cell_guard_assert(guard);
  let target = jet_cell_state(guard.cell).value;
  for (const field of guard.path) {
    if (target == null || !(field in Object(target))) throw new Error("invalid Web Cell guard path");
    target = target[field];
  }
  return target;
}

function jet_cell_guard_read(cell) {
  const checked = jet_cell_state(cell);
  return { kind: "cell_guard", cell: checked, path: [], editable: false, active: true };
}

function jet_cell_guard_edit(cell) {
  const checked = jet_cell_state(cell);
  return { kind: "cell_guard", cell: checked, path: [], editable: true, active: true };
}

function jet_cell_read_guard_get(guard) {
  return jet_web_clone(jet_cell_guard_target(guard));
}

function jet_cell_read_guard_read(guard, callback) {
  const target = jet_cell_guard_target(guard);
  if (typeof callback !== "function") throw new Error("invalid Web Cell read-guard callback");
  return callback(target);
}

function jet_cell_edit_guard_get(guard) {
  return jet_web_clone(jet_cell_guard_target(guard));
}

function jet_cell_edit_guard_set(guard, value) {
  jet_cell_guard_assert(guard);
  if (!guard.editable) throw new Error("Web Cell guard is not editable");
  let target = jet_cell_state(guard.cell).value;
  for (let index = 0; index + 1 < guard.path.length; index += 1) {
    const field = guard.path[index];
    if (target == null || !(field in Object(target))) throw new Error("invalid Web Cell guard path");
    target = target[field];
  }
  if (guard.path.length === 0) jet_cell_state(guard.cell).value = value;
  else {
    const field = guard.path[guard.path.length - 1];
    if (target == null || !(field in Object(target))) throw new Error("invalid Web Cell guard path");
    target[field] = value;
  }
}

function jet_cell_edit_guard_read(guard, callback) {
  const target = jet_cell_guard_target(guard);
  if (typeof callback !== "function") throw new Error("invalid Web Cell edit-guard read callback");
  return callback(target);
}

function jet_cell_edit_guard_edit(guard, callback) {
  const target = jet_cell_guard_target(guard);
  if (!guard.editable || typeof callback !== "function") {
    throw new Error("invalid Web Cell edit-guard callback");
  }
  return callback(target);
}

// Existing index routes carry the historical source-context arguments after
// the checked receiver and id. The Web adapter has already validated those
// route rows; the value operation is the first two arguments.
globalThis["jet_std::jet_pool_get"] = jet_pool_get;
globalThis["jet_std::jet_pool_get_mut"] = jet_pool_get_mut;
globalThis["jet_std::jet_pool_set"] = jet_pool_set;

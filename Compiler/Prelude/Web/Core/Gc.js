// D-OPTGC1: the Web representation of the checked automatic-root boundary.
// MIR supplies the root, callback, edge values, and stable edit site. This
// module owns only the runtime container mechanics; it does not rediscover
// collection policy from names or source text.
function jet_gc_root(root) {
  if (root == null || typeof root !== "object" || !("value" in root)) {
    throw new Error("invalid Web GC root");
  }
  return root;
}

function jet_gc_apply(root, edges, edit) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  checked.edges = edges.slice();
  return edit(checked.value);
}
function jet_gc_read(root) {
  return jet_gc_root(root).value;
}

function jet_gc_edit_clear(root, edit) {
  return jet_gc_apply(root, [], edit);
}

function jet_gc_edit_pop(root, edit) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(checked.edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  checked.edges.pop();
  return edit(checked.value);
}

function jet_gc_edit_remove_index(root, edit, index) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(checked.edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  checked.edges.splice(Number(index), 1);
  return edit(checked.value);
}

function jet_gc_edit_insert_index(root, edit, index, edges) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(checked.edges) || !Array.isArray(edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  checked.edges.splice(Number(index), 0, ...edges);
  return edit(checked.value);
}

function jet_gc_edit_prepend(root, edges, edit) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(checked.edges) || !Array.isArray(edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  checked.edges = edges.concat(checked.edges);
  return edit(checked.value);
}

function jet_gc_edit_additive(root, edges, edit) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(checked.edges) || !Array.isArray(edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  checked.edges = checked.edges.concat(edges);
  return edit(checked.value);
}

function jet_gc_edit_plain(root, edit) {
  const checked = jet_gc_root(root);
  return edit(checked.value);
}

function jet_gc_edit_edge_slot(root, edges, edit, site) {
  const checked = jet_gc_root(root);
  if (!Array.isArray(checked.edges) || !Array.isArray(edges) || typeof edit !== "function") {
    throw new Error("invalid Web GC edit operands");
  }
  if (checked.edgeSlots == null || typeof checked.edgeSlots !== "object") {
    checked.edgeSlots = Object.create(null);
  }
  const key = String(site);
  if (edges.length === 0) delete checked.edgeSlots[key];
  else checked.edgeSlots[key] = edges.slice();
  checked.edges = Object.values(checked.edgeSlots).flat();
  return edit(checked.value);
}

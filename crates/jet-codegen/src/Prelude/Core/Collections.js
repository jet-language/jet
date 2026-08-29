// D-CORE-EAGER1=A / D-LOOPMAP1=B: web collection adapters use the same eager
// collection contract as the native Prelude. The web tier supplies only the
// array marshalling; these functions own the adapter call shape.
function jet_list_map(xs, f) {
  if (xs && xs.__jet_iter) return xs.map(f);
  return xs.map((value) => f(value));
}

function jet_list_filter(xs, f) {
  if (xs && xs.__jet_iter) return xs.filter(f);
  return xs.filter((value) => f(value));
}

// Fallible collection callbacks keep the callback's Result carrier at the
// adapter boundary. A successful callback contributes its payload; the first
// Err is returned unchanged.
function jet_list_try_map(xs, f) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  const out = [];
  for (const value of values) {
    const result = f(value);
    if (result && result.tag === "Err") return result;
    out.push(result && result.tag === "Ok" ? result.values[0] : result);
  }
  return { tag: "Ok", values: [out] };
}

function jet_list_try_filter(xs, f) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  const out = [];
  for (const value of values) {
    const result = f(value);
    if (result && result.tag === "Err") return result;
    const keep = result && result.tag === "Ok" ? result.values[0] : result;
    if (keep) out.push(value);
  }
  return { tag: "Ok", values: [out] };
}

function jet_sequence_argument_message(method, value) {
  if ((method === "take" || method === "skip") && value < 0)
    return "sequence count must be nonnegative";
  if (method === "step_by" && value <= 0)
    return "step_by requires a positive step";
  if (method === "chunks" && value <= 0)
    return "chunks requires a positive size";
  if (method === "windows" && value <= 0)
    return "windows requires a positive size";
  return null;
}

// The JS tier keeps the same explicit deferred boundary as native tiers.
// Each view is one-shot: a second materialization is a use-after-drive.
function jet_iter_lazy(pull) {
  let used = false;
  const view = {
    __jet_iter: true,
    _pull() {
      if (used) {
        jet_runtime_stop(
          "E3001",
          "<core.collections>",
          0,
          "lazy collection view was already consumed",
        );
      }
      used = true;
      return pull();
    },
    map(f) {
      return jet_iter_lazy(() => this._pull().map((value) => f(value)));
    },
    filter(f) {
      return jet_iter_lazy(() => this._pull().filter((value) => f(value)));
    },
    skip(n) {
      const value = Number(n);
      const message = jet_sequence_argument_message("skip", value);
      if (message)
        jet_runtime_stop("E3001", "<core.collections>", 0, message);
      return jet_iter_lazy(() => this._pull().slice(value));
    },
    to_list() {
      return this._pull();
    },
  };
  return view;
}

function jet_iter_from_vec(xs) {
  return jet_iter_lazy(() => xs.slice());
}

function jet_iter_to_list(view) {
  return view.to_list();
}

function jet_iter_skip(view, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("skip", value);
  if (message)
    jet_runtime_stop("E3001", "<core.collections>", 0, message);
  if (view && view.__jet_iter) return view.skip(n);
  return jet_iter_from_vec(view).skip(value);
}

function jet_iter_first(view) {
  if (view && view.__jet_iter) {
    const values = view._pull();
    return values.length === 0
      ? { tag: "None", values: [] }
      : { tag: "Some", values: [values[0]] };
  }
  return view.length === 0
    ? { tag: "None", values: [] }
    : { tag: "Some", values: [view[0]] };
}

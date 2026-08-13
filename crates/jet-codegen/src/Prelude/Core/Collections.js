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

// The JS tier keeps the same explicit deferred boundary as native tiers.
// Each view is one-shot: a second materialization is a use-after-drive.
function jet_iter_lazy(pull) {
  let used = false;
  const view = {
    __jet_iter: true,
    _pull() {
      if (used) throw new Error("lazy collection view was already consumed");
      used = true;
      return pull();
    },
    map(f) {
      return jet_iter_lazy(() => this._pull().map((value) => f(value)));
    },
    filter(f) {
      return jet_iter_lazy(() => this._pull().filter((value) => f(value)));
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

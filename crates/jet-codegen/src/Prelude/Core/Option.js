// D-HOLE1 / I9: Web's JS adapter for the shared OptionLift2 Prelude.
// The generated Web program calls this symbol for presence, lazy callable
// creation, invocation, and result construction.
function jet_option_lift2(a, b, make_f) {
  if (a == null || a.tag !== "Some" || b == null || b.tag !== "Some") {
    return { tag: "None", values: [] };
  }
  const f = make_f();
  return { tag: "Some", values: [f(a.values[0], b.values[0])] };
}

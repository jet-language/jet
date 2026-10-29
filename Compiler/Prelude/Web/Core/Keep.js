// D-BENCH-KEEP1=A: the web sink makes the value observable through the host
// object, then returns it. The dynamic property write blocks loop elision.
const JET_KEEP_SINK = Symbol("jet.keep");

function jet_keep(value) {
  globalThis[JET_KEEP_SINK] = value;
  return value;
}

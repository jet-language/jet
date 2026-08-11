// D-FLOAT-ORIGIN / I9: Web's JS adapter for the shared tracked-float origin.
function jet_float_origin(value, origin) {
  void value;
  return origin ?? "untracked";
}

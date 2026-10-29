// D-FIXARR1 / I9: Web's JS adapter for the shared checked fixed-list index.
function jet_fixed_list_index(base, index, file, line) {
  const position = Number(index);
  const len = base.length;
  if (!Number.isSafeInteger(position) || position < 0 || position >= len) {
    jet_runtime_stop(
      "E3010",
      file,
      line,
      jet_list_bounds_message(len, index),
    );
  }
  return base[position];
}

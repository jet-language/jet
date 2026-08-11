// D-FIXARR1 / I9: Web's JS adapter for the shared checked fixed-list index.
function jet_fixed_list_index(base, index, file, line) {
  const position = Number(index);
  const len = base.length;
  if (!Number.isSafeInteger(position) || position < 0 || position >= len) {
    throw new Error(
      `${file}:${line}: the list has ${len} items, so position ${index} doesn't exist`,
    );
  }
  return base[position];
}

// D-TYPE2-SPELL1: the JS adapter calls the same inline-range rule as native.
function jet_inline_range_from_int(value, lo, hi) {
  const n = BigInt(value);
  const low = BigInt(lo);
  const high = BigInt(hi);
  return n >= low && n <= high
    ? { tag: "Ok", values: [n] }
    : { tag: "Err", values: [`value is outside Int(${lo}..${hi})`] };
}

// JavaScript carries every Float as Number, whose default string form erases
// the decimal point from whole values. Jet Float display preserves that type
// fact, matching Prelude/Core/Values.rs on native and Wasm tiers.
function jet_float_display(value) {
  const number = Number(value);
  if (Number.isNaN(number)) return "NaN";
  if (number === Infinity) return "inf";
  if (number === -Infinity) return "-inf";
  if (Object.is(number, -0)) return "-0.0";
  const text = String(number);
  return Number.isInteger(number) && !/[eE]/.test(text) ? `${text}.0` : text;
}

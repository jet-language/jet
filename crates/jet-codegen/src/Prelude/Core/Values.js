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

// D-DISPLAY-SHAPE / I9: print uses the Prelude's JetShow collection rule.
// Keep it separate from Display: JavaScript Numbers are already the web
// adapter's marshalled values, so String(Number) preserves the web surface's
// measurement and fixed-list spelling.
function jet_show(value) {
  if (Array.isArray(value)) {
    return `[${value.map((item) => jet_show(item)).join(", ")}]`;
  }
  return String(value);
}

// D-FMT-PLAIN1=A / I9: decimal formatting is one Prelude rail on the JS
// adapter too. BigInt stays exact; Number carries the Float path.
function jet_fmt_decimal(value, precision) {
  const number = Number(value);
  if (Number.isNaN(number)) return "NaN";
  if (number === Infinity) return "inf";
  if (number === -Infinity) return "-inf";
  return jet_fmt_fixed_even(number, Math.max(0, Number(precision)));
}

// Rust's fixed formatter uses nearest-even at an exact decimal tie. Rebuild
// the finite Number as an integer over a power of two, scale by 10^places,
// and round the rational quotient so the Web rail does not inherit toFixed's
// different half-away behavior.
function jet_fmt_fixed_even(number, places) {
  const bits = new ArrayBuffer(8);
  const view = new DataView(bits);
  const negative = number < 0 || Object.is(number, -0);
  view.setFloat64(0, Math.abs(number), false);
  const high = view.getUint32(0, false);
  const low = view.getUint32(4, false);
  const exponent = (high >>> 20) & 0x7ff;
  const fraction = (BigInt(high & 0xfffff) << 32n) | BigInt(low);
  let significand = fraction;
  let binaryExponent = -1074;
  if (exponent !== 0) {
    significand |= 1n << 52n;
    binaryExponent = exponent - 1023 - 52;
  }
  let numerator = significand;
  let denominator = 1n;
  if (binaryExponent >= 0) numerator <<= BigInt(binaryExponent);
  else denominator <<= BigInt(-binaryExponent);
  numerator *= 10n ** BigInt(places);
  let whole = numerator / denominator;
  const remainder = numerator % denominator;
  const twice = remainder * 2n;
  if (twice > denominator || (twice === denominator && (whole & 1n) === 1n)) {
    whole += 1n;
  }
  let digits = whole.toString();
  if (places === 0) return `${negative ? "-" : ""}${digits}`;
  if (digits.length <= places) {
    digits = `${"0".repeat(places + 1 - digits.length)}${digits}`;
  }
  const split = digits.length - places;
  return `${negative ? "-" : ""}${digits.slice(0, split)}.${digits.slice(split)}`;
}

function jet_fmt_grouped(value, precision) {
  return jet_group_decimal(jet_fmt_decimal(value, precision));
}

function jet_fmt_decimal_int(value, precision) {
  const raw = BigInt(value).toString();
  const negative = raw.startsWith("-");
  const digits = (negative ? raw.slice(1) : raw).replace(/^0+(?=\d)/, "");
  const places = Math.max(0, Number(precision));
  const sign = negative && digits !== "0" ? "-" : "";
  return places === 0 ? `${sign}${digits}` : `${sign}${digits}.${"0".repeat(places)}`;
}

function jet_fmt_grouped_int(value, precision) {
  return jet_group_decimal(jet_fmt_decimal_int(value, precision));
}

function jet_group_decimal(value) {
  const sign = value.startsWith("-") ? "-" : "";
  const rest = sign === "" ? value : value.slice(1);
  const dot = rest.indexOf(".");
  const whole = dot < 0 ? rest : rest.slice(0, dot);
  const fraction = dot < 0 ? "" : rest.slice(dot);
  return `${sign}${whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",")}${fraction}`;
}

// D-DISPLAYDBG1 / I9: Web's value formatter mirrors the embedded Prelude
// Display and Debug rails. The JS backend supplies only the carrier
// marshalling; it never falls back to JavaScript's object or array spelling.
function jet_display(value) {
  if (Array.isArray(value)) {
    return `[${value.map((item) => jet_display(item)).join(", ")}]`;
  }
  if (value == null) return "null";
  if (value.tag === "Some") return jet_display(value.values[0]);
  if (value.tag === "None") return "null";
  if (value.tag === "Ok") return `Ok(${jet_display(value.values[0])})`;
  if (value.tag === "Err") return `Err(${jet_display(value.values[0])})`;
  if (typeof value === "number") return jet_float_display(value);
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "object" && typeof value.tag === "string") {
    const payload = Array.isArray(value.values) ? value.values : [];
    return payload.length === 0
      ? value.tag
      : `${value.tag}(${payload.map((item) => jet_display(item)).join(", ")})`;
  }
  return String(value);
}

function jet_debug(value) {
  if (Array.isArray(value)) {
    return `[${value.map((item) => jet_debug(item)).join(", ")}]`;
  }
  if (value == null) return "null";
  if (value.tag === "Some") return `Val(${jet_debug(value.values[0])})`;
  if (value.tag === "None") return "None";
  if (value.tag === "Ok") return `Ok(${jet_debug(value.values[0])})`;
  if (value.tag === "Err") return `Err(${jet_debug(value.values[0])})`;
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number") return jet_float_display(value);
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "object" && typeof value.tag === "string") {
    const payload = Array.isArray(value.values) ? value.values : [];
    return payload.length === 0
      ? value.tag
      : `${value.tag}(${payload.map((item) => jet_debug(item)).join(", ")})`;
  }
  return String(value);
}

// D-FMT-PRETTY1=A: the JS copy of the shared Prelude's canonical Debug layout.
// It scans quoted text before treating braces as structure, so a string value
// cannot change the shape of its own pretty output.
function jet_fmt_pretty(value) {
  return jet_pretty_fragment(String(value), 0);
}

function jet_pretty_fragment(value, indent) {
  value = value.trim();
  const structure = jet_first_structure(value);
  if (structure == null) return value;
  const [openAt, open, close] = structure;
  const end = jet_matching_close(value, openAt, open, close);
  if (end == null || value.slice(end + 1).trim() !== "") return value;
  const prefix = value.slice(0, openAt).trimEnd();
  const body = value.slice(openAt + 1, end);
  if (body.trim() === "") return `${prefix} ${open}${close}`.trimStart();
  if (open === "[" && (body.trim() === ":" || body.trim().toLowerCase() === "redacted")) {
    return value;
  }
  let out = prefix.length === 0 ? "" : `${prefix} `;
  out += open;
  for (const item of jet_split_top_level(body)) {
    out += `\n${" ".repeat(indent + 2)}${jet_pretty_fragment(item, indent + 2)}`;
  }
  return `${out}\n${" ".repeat(indent)}${close}`;
}

function jet_close_for(open) {
  if (open === "{") return "}";
  if (open === "[") return "]";
  if (open === "(") return ")";
  return null;
}

function jet_first_structure(value) {
  let quoted = false;
  let escaped = false;
  for (let index = 0; index < value.length; index += 1) {
    const ch = value[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') quoted = false;
      continue;
    }
    if (ch === '"') quoted = true;
    else {
      const close = jet_close_for(ch);
      if (close != null) return [index, ch, close];
    }
  }
  return null;
}

function jet_matching_close(value, openAt, open, close) {
  const stack = [close];
  let quoted = false;
  let escaped = false;
  for (let index = openAt + 1; index < value.length; index += 1) {
    const ch = value[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') quoted = false;
      continue;
    }
    if (ch === '"') {
      quoted = true;
    } else {
      const nested = jet_close_for(ch);
      if (nested != null) stack.push(nested);
      else if (stack[stack.length - 1] === ch) {
        stack.pop();
        if (stack.length === 0) return index;
      }
    }
  }
  return null;
}

function jet_split_top_level(value) {
  const items = [];
  let start = 0;
  const stack = [];
  let quoted = false;
  let escaped = false;
  for (let index = 0; index < value.length; index += 1) {
    const ch = value[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === '"') quoted = false;
      continue;
    }
    if (ch === '"') quoted = true;
    else {
      const nested = jet_close_for(ch);
      if (nested != null) stack.push(nested);
      else if (stack[stack.length - 1] === ch) stack.pop();
      else if (ch === "," && stack.length === 0) {
        if (value.slice(start, index).trim() !== "") items.push(value.slice(start, index).trim());
        start = index + 1;
      }
    }
  }
  if (value.slice(start).trim() !== "") items.push(value.slice(start).trim());
  return items;
}

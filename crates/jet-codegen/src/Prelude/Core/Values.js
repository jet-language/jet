// Shared exact Int bounds. Time and codec Prelude helpers use the same
// signed i64 carrier on the Web rail.
const JET_I64_MIN = -(1n << 63n);
const JET_I64_MAX = (1n << 63n) - 1n;
const JET_ABSENT = Symbol.for("jet.absent");
const JET_CLEAN_OUTCOMES = new WeakSet();
const JET_COPY_KEYS = new WeakMap();
const JET_WRITE_ARG = Symbol("jet.write_arg");

function jet_absent() {
  return JET_ABSENT;
}

function jet_is_absent(value) {
  return value === JET_ABSENT;
}

function jet_outcome_ok(value) {
  return { tag: "Ok", values: [value] };
}

function jet_outcome_err(error) {
  return { tag: "Err", values: [error] };
}

function jet_option_some(value) {
  const outcome = jet_outcome_ok(value);
  JET_CLEAN_OUTCOMES.add(outcome);
  return outcome;
}

function jet_option_none() {
  return jet_option_err(jet_absent());
}

function jet_option_err(error) {
  const outcome = jet_outcome_err(error);
  JET_CLEAN_OUTCOMES.add(outcome);
  return outcome;
}

function jet_is_clean_outcome(value) {
  return value != null && typeof value === "object" && JET_CLEAN_OUTCOMES.has(value);
}

function jet_clean_option(value) {
  if (jet_is_absent(value)) return jet_option_none();
  if (
    value == null ||
    typeof value !== "object" ||
    (value.tag !== "Ok" && value.tag !== "Err") ||
    !Array.isArray(value.values) ||
    value.values.length !== 1
  ) {
    throw new TypeError("invalid Web optional carrier");
  }
  JET_CLEAN_OUTCOMES.add(value);
  return value;
}

function jet_copy_remember(value, key) {
  if (value != null && (typeof value === "object" || typeof value === "function")) {
    JET_COPY_KEYS.set(value, key);
  }
  return value;
}

function jet_copy_type_metadata(copy, value) {
  if (value != null && Object.hasOwn(value, "__jet_type")) {
    Object.defineProperty(copy, "__jet_type", {
      value: value.__jet_type,
      enumerable: false,
    });
  }
  return copy;
}

function jet_copy_data(value, facts, seen) {
  if (value === null || typeof value !== "object") return value;
  const existing = seen.get(value);
  if (existing) return existing;
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) {
    const copy = [];
    seen.set(value, copy);
    for (const item of value) copy.push(jet_copy_data(item, facts, seen));
    return copy;
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    throw new TypeError("checked Web copy encountered an opaque data value");
  }
  const copy = {};
  seen.set(value, copy);
  for (const key of Object.keys(value)) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (!descriptor || !Object.hasOwn(descriptor, "value")) {
      throw new TypeError("checked Web copy encountered an accessor");
    }
    copy[key] = jet_copy_data(descriptor.value, facts, seen);
  }
  return copy;
}

function jet_copy_value(value, key, facts, seen) {
  if (value === null || value === undefined || jet_is_absent(value)) return value;
  if (typeof value !== "object" && typeof value !== "function") return value;
  const typeKey = key ?? JET_COPY_KEYS.get(value);
  if (typeKey == null) throw new TypeError("checked Web copy has no type fact");
  const schema = facts?.get(typeKey);
  if (!schema) throw new TypeError("checked Web copy has no schema");
  if (["unit", "integer", "float", "boolean", "text", "callable", "opaque", "shared"].includes(schema.kind)) {
    return jet_copy_remember(value, typeKey);
  }
  if (schema.kind === "absent") {
    if (!jet_is_absent(value)) throw new TypeError("checked Web copy expected JetAbsent");
    return value;
  }
  if (schema.kind === "transparent") {
    return jet_copy_remember(jet_copy_value(value, schema.inner, facts, seen), typeKey);
  }
  if (schema.kind === "data") {
    return jet_copy_remember(jet_copy_data(value, facts, seen ?? new Map()), typeKey);
  }
  const traversal = seen ?? new Map();
  const existing = traversal.get(value);
  if (existing) return existing;
  if (schema.kind === "bytes") {
    if (!(value instanceof Uint8Array)) throw new TypeError("checked Web copy expected bytes");
    return jet_copy_remember(value.slice(), typeKey);
  }
  if (schema.kind === "list") {
    if (!Array.isArray(value)) throw new TypeError("checked Web copy expected a list");
    const copy = [];
    traversal.set(value, copy);
    for (const item of value) copy.push(jet_copy_value(item, schema.item, facts, traversal));
    return jet_copy_remember(copy, typeKey);
  }
  if (schema.kind === "map") {
    if (!(value instanceof Map)) throw new TypeError("checked Web copy expected a map");
    const copy = new Map();
    traversal.set(value, copy);
    for (const [itemKey, item] of value) {
      copy.set(
        jet_copy_value(itemKey, schema.key, facts, traversal),
        jet_copy_value(item, schema.value, facts, traversal),
      );
    }
    return jet_copy_remember(copy, typeKey);
  }
  if (schema.kind === "record") {
    if (Array.isArray(value)) throw new TypeError("checked Web copy expected a record");
    const copy = jet_copy_type_metadata({}, value);
    traversal.set(value, copy);
    for (const [field, fieldKey] of schema.fields) {
      if (!Object.hasOwn(value, field)) throw new TypeError("checked Web copy record field is missing");
      copy[field] = jet_copy_value(value[field], fieldKey, facts, traversal);
    }
    return jet_copy_remember(copy, typeKey);
  }
  if (schema.kind === "enum") {
    if (Array.isArray(value) || !Array.isArray(value.values)) {
      throw new TypeError("checked Web copy expected an enum carrier");
    }
    const variant = schema.variants.find(([tag]) => tag === value.tag);
    if (!variant || variant[1].length !== value.values.length) {
      throw new TypeError("checked Web copy enum variant is invalid");
    }
    const copy = jet_copy_type_metadata({ tag: value.tag, values: [] }, value);
    traversal.set(value, copy);
    copy.values = variant[1].map((fieldKey, index) => fieldKey == null
      ? value.values[index]
      : jet_copy_value(value.values[index], fieldKey, facts, traversal));
    jet_copy_remember(copy, typeKey);
    if (schema.clean) JET_CLEAN_OUTCOMES.add(copy);
    return copy;
  }
  throw new TypeError("checked Web copy has an unsupported type fact");
}

function jet_copy_value_register(value, key, facts) {
  if (!facts?.has(key)) throw new TypeError("checked Web copy has no schema");
  return jet_copy_remember(value, key);
}

function jet_web_write_arg(cell) {
  if (cell == null || typeof cell !== "object") {
    throw new TypeError("checked Web write argument has no place cell");
  }
  return Object.freeze({ [JET_WRITE_ARG]: cell });
}

function jet_web_call_arg_cell(value) {
  return value != null && (typeof value === "object" || typeof value === "function")
    ? value[JET_WRITE_ARG]
    : undefined;
}

function jet_web_call_arg_value(value) {
  const cell = jet_web_call_arg_cell(value);
  return cell === undefined ? value : cell.value;
}

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
// Keep it separate from Display. MIR Web supplies checked aggregate facts in
// `__jet_print_type_facts`; values without those facts keep their carrier
// spelling instead of gaining a runtime-derived Printable implementation.
function jet_show(value) {
  if (Array.isArray(value)) {
    return `[${value.map((item) => jet_show(item)).join(", ")}]`;
  }
  if (jet_is_absent(value)) return "null";
  if (value == null) return "null";
  if (typeof value === "number") return jet_float_display(value);
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "string") return value;
  if (value instanceof Map) {
    return `[${Array.from(value.entries())
      .map(([key, item]) => `${jet_show(key)}: ${jet_show(item)}`)
      .join(", ")}]`;
  }
  if (typeof value === "object") {
    const facts = typeof __jet_print_type_facts === "undefined"
      ? null
      : __jet_print_type_facts;
    const fact = facts?.[value.__jet_type];
    if (fact != null) {
      if (!fact.auto_printable) {
        throw new Error(`non-printable Jet value of type ${fact.name}`);
      }
      if (fact.kind === "struct") {
        const fields = fact.fields
          .filter((field) => !field.computed)
          .map((field) => `${field.name}: ${jet_show(value[field.key])}`);
        return fields.length === 0
          ? `${fact.name} {}`
          : `${fact.name} { ${fields.join(", ")} }`;
      }
      if (fact.kind === "enum") {
        const variant = fact.variants.find((candidate) => candidate.tag === value.tag);
        if (variant == null) {
          throw new Error(`unknown variant for printable type ${fact.name}`);
        }
        if (variant.kind === "unit") return variant.name;
        if (variant.kind === "single") {
          return `${variant.name}(${jet_show(value.values?.[0])})`;
        }
        const fields = variant.fields
          .map((field, index) => field.computed
            ? null
            : `${field.name}: ${jet_show(value.values?.[index])}`)
          .filter((field) => field != null);
        return fields.length === 0
          ? `${variant.name} {}`
          : `${variant.name} { ${fields.join(", ")} }`;
      }
    }
  }
  if (value.tag === "Ok") {
    return jet_is_clean_outcome(value) ? jet_show(value.values[0]) : `Ok(${jet_show(value.values[0])})`;
  }
  if (value.tag === "Err") {
    return jet_is_clean_outcome(value) && jet_is_absent(value.values[0])
      ? "null"
      : `Err(${jet_show(value.values[0])})`;
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

function jet_fmt_quantity(value) {
  const number = Number(value);
  if (Number.isNaN(number)) return "NaN";
  if (number === Infinity) return "inf";
  if (number === -Infinity) return "-inf";
  return String(number).replace(/\.0$/, "");
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
  if (jet_is_absent(value)) return "null";
  if (value == null) return "null";
  if (value.tag === "Ok") {
    return jet_is_clean_outcome(value) ? jet_display(value.values[0]) : `Ok(${jet_display(value.values[0])})`;
  }
  if (value.tag === "Err") {
    return jet_is_clean_outcome(value) && jet_is_absent(value.values[0])
      ? "null"
      : `Err(${jet_display(value.values[0])})`;
  }
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
  if (jet_is_absent(value)) return "None";
  if (value == null) return "null";
  if (value.tag === "Ok") {
    return jet_is_clean_outcome(value) ? `Val(${jet_debug(value.values[0])})` : `Ok(${jet_debug(value.values[0])})`;
  }
  if (value.tag === "Err") {
    return jet_is_clean_outcome(value) && jet_is_absent(value.values[0])
      ? "None"
      : `Err(${jet_debug(value.values[0])})`;
  }
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number") return jet_float_display(value);
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "object") {
    const facts = typeof __jet_print_type_facts === "undefined"
      ? null
      : __jet_print_type_facts;
    const fact = facts?.[value.__jet_type];
    if (fact != null && fact.auto_printable) {
      if (fact.kind === "struct") {
        const fields = fact.fields
          .filter((field) => !field.computed)
          .map((field) => `${field.name}: ${field.redacted ? "[redacted]" : jet_debug(value[field.key])}`);
        return fields.length === 0
          ? `${fact.name} {}`
          : `${fact.name} { ${fields.join(", ")} }`;
      }
      if (fact.kind === "enum") {
        const variant = fact.variants.find((candidate) => candidate.tag === value.tag);
        if (variant == null) {
          throw new Error(`unknown variant for printable type ${fact.name}`);
        }
        if (variant.kind === "unit") return variant.name;
        if (variant.kind === "single") {
          return `${variant.name}(${jet_debug(value.values?.[0])})`;
        }
        const fields = variant.fields
          .map((field, index) => field.computed
            ? null
            : `${field.name}: ${field.redacted ? "[redacted]" : jet_debug(value.values?.[index])}`)
          .filter((field) => field != null);
        return fields.length === 0
          ? `${variant.name} {}`
          : `${variant.name} { ${fields.join(", ")} }`;
      }
    }
  }
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

// D-CARRIER1: checked effect-carrier projections. The field ID is consumed
// by MIR and selects one of these canonical kernels before JavaScript runs.
function jet_carrier_report(outcome) {
  if (outcome == null || typeof outcome !== "object") {
    throw new Error("invalid Web carrier outcome");
  }
  if (outcome.tag === "Ok") return null;
  if (outcome.tag !== "Err" || !Array.isArray(outcome.values) || outcome.values.length !== 1) {
    throw new Error("invalid Web carrier report");
  }
  const report = outcome.values[0];
  if (report == null || typeof report !== "object") {
    throw new Error("invalid Web carrier report value");
  }
  return report;
}

function jet_partial(outcome) {
  const report = jet_carrier_report(outcome);
  if (report == null) return jet_option_none();
  if (!("partial" in report)) throw new Error("carrier report has no partial value");
  return jet_option_some(report.partial);
}

function jet_notes(outcome) {
  const report = jet_carrier_report(outcome);
  if (report == null) return [];
  if (!Array.isArray(report.notes)) throw new Error("carrier report has no notes value");
  return report.notes.slice();
}
// Canonical typed DataTree decoding and accessors for the Web adapter. MIR
// supplies a checked shape descriptor; conversion and FieldError policy stay
// with the other value carriers in this Prelude module.
function jet_codec_decode_ok(value) {
  return { ok: true, value };
}

function jet_codec_decode_error(path, reason) {
  return { ok: false, errors: [{ path: String(path), reason: String(reason) }] };
}

function jet_codec_decode_under(prefix, result) {
  if (result.ok) return result;
  const base = String(prefix);
  return {
    ok: false,
    errors: result.errors.map((error) => ({
      path: base + String(error.path || ""),
      reason: String(error.reason),
    })),
  };
}

function jet_codec_decode_result(result) {
  return result.ok
    ? { tag: "Ok", values: [result.value] }
    : { tag: "Err", values: [result.errors] };
}

function jet_codec_tree_payload(tree) {
  return tree && Array.isArray(tree.values) ? tree.values[0] : undefined;
}

function jet_datatree_kind(tree) {
  switch (tree?.tag) {
    case "Null": return "null";
    case "Bool": return "Bool";
    case "Int": return "Int";
    case "Float": return "Float";
    case "Number": return "number";
    case "TypedText":
    case "Text": return "text";
    case "Bytes": return "Bytes";
    case "Array": return "a list";
    case "Object": return "an object";
    default: return "value";
  }
}

function jet_datatree_array_items(tree) {
  const payload = jet_codec_tree_payload(tree);
  return Array.isArray(payload) ? payload : null;
}

function jet_datatree_object_entries(tree) {
  const payload = jet_codec_tree_payload(tree);
  if (payload instanceof Map) return Array.from(payload.entries());
  if (Array.isArray(payload)) {
    return payload
      .filter((entry) => Array.isArray(entry) && entry.length >= 2)
      .map((entry) => [String(entry[0]), entry[1]]);
  }
  if (payload != null && typeof payload === "object") {
    return Object.keys(payload).map((key) => [key, payload[key]]);
  }
  return null;
}

function jet_datatree_render(tree) {
  switch (tree?.tag) {
    case "Null": return "null";
    case "Bool": return jet_codec_tree_payload(tree) ? "true" : "false";
    case "Int":
      try { return BigInt(jet_codec_tree_payload(tree)).toString(); }
      catch (_) { return String(jet_codec_tree_payload(tree)); }
    case "Float": return jet_float_display(jet_codec_tree_payload(tree));
    case "Number": return String(jet_codec_tree_payload(tree));
    case "TypedText":
    case "Text": return JSON.stringify(String(jet_codec_tree_payload(tree)));
    case "Bytes": {
      const bytes = jet_codec_tree_payload(tree);
      return Array.isArray(bytes) ? `[${bytes.join(",")}]` : "[]";
    }
    case "Array": {
      const items = jet_datatree_array_items(tree);
      return items == null ? "[]" : `[${items.map((item) => jet_datatree_render(item)).join(",")}]`;
    }
    case "Object": {
      const entries = jet_datatree_object_entries(tree);
      return entries == null
        ? "{}"
        : `{${entries.map(([key, value]) => `${JSON.stringify(key)}:${jet_datatree_render(value)}`).join(",")}}`;
    }
    default: return String(tree?.tag ?? "value");
  }
}

function jet_datatree_field(tree, name) {
  const key = String(name);
  const entries = jet_datatree_object_entries(tree);
  if (tree?.tag === "Object" && entries != null) {
    const found = entries.find((entry) => entry[0] === key);
    if (found) return { tag: "Ok", values: [found[1]] };
    return {
      tag: "Err",
      values: [[{ path: key, reason: `field \`${key}\` not found` }]],
    };
  }
  return {
    tag: "Err",
    values: [[{ path: key, reason: `expected object, got ${jet_datatree_render(tree)}` }]],
  };
}

function jet_datatree_at(tree, index) {
  const items = jet_datatree_array_items(tree);
  const raw = String(index);
  if (tree?.tag === "Array" && items != null) {
    let position;
    try {
      const value = BigInt(index);
      position = value < 0n ? items.length - Number(-value) : Number(value);
    } catch (_) {
      position = -1;
    }
    if (Number.isSafeInteger(position) && position >= 0 && position < items.length) {
      return { tag: "Ok", values: [items[position]] };
    }
    return {
      tag: "Err",
      values: [[{ path: `[${raw}]`, reason: `index ${raw} out of bounds (len ${items.length})` }]],
    };
  }
  return {
    tag: "Err",
    values: [[{ path: `[${raw}]`, reason: `expected array, got ${jet_datatree_render(tree)}` }]],
  };
}

function jet_codec_json_exact_integer(text) {
  const raw = String(text).trim();
  const match = /^([+-]?)(\d+)(?:\.(\d*))?(?:[eE]([+-]?\d+))?$/.exec(raw);
  if (!match) return null;
  try {
    const fraction = match[3] || "";
    let value = BigInt(`${match[2]}${fraction}` || "0");
    const scale = BigInt(fraction.length) - BigInt(match[4] || "0");
    if (scale > 0n) {
      const divisor = 10n ** scale;
      if (value % divisor !== 0n) return null;
      value /= divisor;
    } else if (scale < 0n) {
      value *= 10n ** (-scale);
    }
    return match[1] === "-" ? -value : value;
  } catch (_) {
    return null;
  }
}

function jet_codec_i64(value) {
  try {
    const number = BigInt(value);
    return number >= JET_I64_MIN && number <= JET_I64_MAX ? number : null;
  } catch (_) {
    return null;
  }
}

function jet_datatree_int(tree) {
  let value = null;
  if (tree?.tag === "Int") value = jet_codec_i64(jet_codec_tree_payload(tree));
  else if (tree?.tag === "Number") value = jet_codec_i64(jet_codec_json_exact_integer(jet_codec_tree_payload(tree)));
  if (value != null) return { tag: "Ok", values: [value] };
  return {
    tag: "Err",
    values: [[{ path: "", reason: `expected int, got ${jet_datatree_render(tree)}` }]],
  };
}

function jet_datatree_text(tree) {
  if (tree?.tag === "Text" || tree?.tag === "TypedText") {
    return { tag: "Ok", values: [String(jet_codec_tree_payload(tree))] };
  }
  return {
    tag: "Err",
    values: [[{ path: "", reason: `expected text, got ${jet_datatree_render(tree)}` }]],
  };
}

function jet_datatree_bool(tree) {
  if (tree?.tag === "Bool") return { tag: "Ok", values: [Boolean(jet_codec_tree_payload(tree))] };
  return {
    tag: "Err",
    values: [[{ path: "", reason: `expected bool, got ${jet_datatree_render(tree)}` }]],
  };
}

function jet_datatree_float(tree) {
  let value = null;
  if (tree?.tag === "Float") {
    const number = Number(jet_codec_tree_payload(tree));
    if (Number.isFinite(number)) value = number;
  } else if (tree?.tag === "Int") {
    const number = Number(jet_codec_tree_payload(tree));
    if (Number.isFinite(number)) value = number;
  } else if (tree?.tag === "Number") {
    const number = Number(String(jet_codec_tree_payload(tree)).trim());
    if (Number.isFinite(number)) value = number;
  }
  if (value != null) return { tag: "Ok", values: [value] };
  const reason = tree?.tag === "Int" && !Number.isFinite(Number(jet_codec_tree_payload(tree)))
    ? "expected float, got out-of-range Int"
    : `expected float, got ${jet_datatree_render(tree)}`;
  return { tag: "Err", values: [[{ path: "", reason }]] };
}

function jet_datatree_to_text(tree) {
  switch (tree?.tag) {
    case "Text":
    case "TypedText":
    case "Number":
      return jet_option_some(String(jet_codec_tree_payload(tree)));
    case "Int":
      try { return jet_option_some(BigInt(jet_codec_tree_payload(tree)).toString()); }
      catch (_) { return jet_option_none(); }
    case "Float":
      return jet_option_some(jet_float_display(jet_codec_tree_payload(tree)));
    case "Bool":
      return jet_option_some(jet_codec_tree_payload(tree) ? "true" : "false");
    default: return jet_option_none();
  }
}

function jet_datatree_equal_unordered(left, right) {
  if (left?.tag !== right?.tag) return false;
  switch (left?.tag) {
    case "Null": return true;
    case "Bool": return Boolean(jet_codec_tree_payload(left)) === Boolean(jet_codec_tree_payload(right));
    case "Int":
      try { return BigInt(jet_codec_tree_payload(left)) === BigInt(jet_codec_tree_payload(right)); }
      catch (_) { return false; }
    case "Float": {
      const l = Number(jet_codec_tree_payload(left));
      const r = Number(jet_codec_tree_payload(right));
      return !Number.isNaN(l) && !Number.isNaN(r) && l === r;
    }
    case "Number":
    case "TypedText":
    case "Text":
      return jet_codec_tree_payload(left) === jet_codec_tree_payload(right);
    case "Bytes": {
      const l = jet_codec_tree_payload(left);
      const r = jet_codec_tree_payload(right);
      return Array.isArray(l) && Array.isArray(r) && l.length === r.length && l.every((v, i) => v === r[i]);
    }
    case "Array": {
      const l = jet_datatree_array_items(left);
      const r = jet_datatree_array_items(right);
      return l != null && r != null && l.length === r.length
        && l.every((v, i) => jet_datatree_equal_unordered(v, r[i]));
    }
    case "Object": {
      const l = jet_datatree_object_entries(left);
      const r = jet_datatree_object_entries(right);
      if (l == null || r == null || l.length !== r.length) return false;
      const used = new Array(r.length).fill(false);
      return l.every(([key, value]) => {
        const index = r.findIndex((entry, i) => !used[i] && entry[0] === key
          && jet_datatree_equal_unordered(value, entry[1]));
        if (index < 0) return false;
        used[index] = true;
        return true;
      });
    }
    default: return false;
  }
}

function jet_codec_typed_string(tree) {
  if (tree?.tag === "Text" || tree?.tag === "TypedText") {
    return jet_codec_decode_ok(String(jet_codec_tree_payload(tree)));
  }
  if (tree?.tag === "Number") {
    return jet_codec_decode_error("", `expected Text, found number ${String(jet_codec_tree_payload(tree))}`);
  }
  if (tree?.tag === "Int") {
    try { return jet_codec_decode_ok(BigInt(jet_codec_tree_payload(tree)).toString()); }
    catch (_) { return jet_codec_decode_error("", "expected Text, found Int"); }
  }
  if (tree?.tag === "Float") return jet_codec_decode_ok(jet_float_display(jet_codec_tree_payload(tree)));
  if (tree?.tag === "Bool") return jet_codec_decode_ok(jet_codec_tree_payload(tree) ? "true" : "false");
  return jet_codec_decode_error("", `expected Text, found ${jet_datatree_kind(tree)}`);
}

function jet_codec_typed_int(tree) {
  if (tree?.tag === "Int") {
    try { return jet_codec_decode_ok(BigInt(jet_codec_tree_payload(tree))); }
    catch (_) { return jet_codec_decode_error("", "expected Int, found Int"); }
  }
  if (tree?.tag === "Float") {
    const value = Number(jet_codec_tree_payload(tree));
    if (Number.isFinite(value) && Number.isInteger(value)
      && value >= Number(JET_I64_MIN) && value < Number(JET_I64_MAX)) {
      return jet_codec_decode_ok(BigInt(value));
    }
    return jet_codec_decode_error("", "expected Int, found out-of-range Float");
  }
  if (tree?.tag === "Number") {
    const raw = String(jet_codec_tree_payload(tree));
    const value = jet_codec_json_exact_integer(raw);
    return value == null
      ? jet_codec_decode_error("", `expected Int, found number ${raw}`)
      : jet_codec_decode_ok(value);
  }
  if (tree?.tag === "TypedText") {
    const raw = String(jet_codec_tree_payload(tree));
    const value = /^[+-]?\d+$/.test(raw.trim()) ? BigInt(raw.trim()) : null;
    return value == null
      ? jet_codec_decode_error("", `expected Int, found text ${JSON.stringify(raw)}`)
      : jet_codec_decode_ok(value);
  }
  if (tree?.tag === "Text") {
    const raw = String(jet_codec_tree_payload(tree)).trim();
    const value = /^[+-]?\d+$/.test(raw) ? BigInt(raw) : null;
    return value == null
      ? jet_codec_decode_error("", `expected Int, found text ${JSON.stringify(raw)}`)
      : jet_codec_decode_ok(value);
  }
  return jet_codec_decode_error("", `expected Int, found ${jet_datatree_kind(tree)}`);
}

function jet_codec_typed_float(tree) {
  if (tree?.tag === "Float") {
    const value = Number(jet_codec_tree_payload(tree));
    return Number.isFinite(value)
      ? jet_codec_decode_ok(value)
      : jet_codec_decode_error("", "expected Float, found out-of-range Float");
  }
  if (tree?.tag === "Int") {
    const value = Number(jet_codec_tree_payload(tree));
    return Number.isFinite(value)
      ? jet_codec_decode_ok(value)
      : jet_codec_decode_error("", "expected Float, found out-of-range Int");
  }
  if (tree?.tag === "Number" || tree?.tag === "Text") {
    const raw = String(jet_codec_tree_payload(tree)).trim();
    const value = Number(raw);
    return Number.isFinite(value)
      ? jet_codec_decode_ok(value)
      : jet_codec_decode_error("", `expected Float, found text ${JSON.stringify(raw)}`);
  }
  if (tree?.tag === "TypedText") {
    return jet_codec_decode_error("", `expected Float, found text ${JSON.stringify(jet_codec_tree_payload(tree))}`);
  }
  return jet_codec_decode_error("", `expected Float, found ${jet_datatree_kind(tree)}`);
}

function jet_codec_typed_bool(tree) {
  if (tree?.tag === "Bool") return jet_codec_decode_ok(Boolean(jet_codec_tree_payload(tree)));
  if (tree?.tag === "Text") {
    const raw = String(jet_codec_tree_payload(tree)).trim();
    if (raw === "true") return jet_codec_decode_ok(true);
    if (raw === "false") return jet_codec_decode_ok(false);
    return jet_codec_decode_error("", `expected Bool, found text ${JSON.stringify(raw)}`);
  }
  if (tree?.tag === "TypedText") {
    return jet_codec_decode_error("", `expected Bool, found text ${JSON.stringify(jet_codec_tree_payload(tree))}`);
  }
  return jet_codec_decode_error("", `expected Bool, found ${jet_datatree_kind(tree)}`);
}

function jet_codec_typed_char(tree) {
  const result = jet_codec_typed_string(tree);
  if (!result.ok) return result;
  const chars = Array.from(result.value);
  return chars.length === 1
    ? jet_codec_decode_ok(chars[0])
    : jet_codec_decode_error("", `expected a single Char, found ${JSON.stringify(result.value)}`);
}

function jet_codec_typed_intn(tree, descriptor) {
  const result = jet_codec_typed_int(tree);
  if (!result.ok) return result;
  const value = result.value;
  if (value < JET_I64_MIN || value > JET_I64_MAX) {
    return jet_codec_decode_error("", `expected ${descriptor.name}, found out-of-range Int`);
  }
  const bits = BigInt(descriptor.bits);
  const min = descriptor.signed ? -(1n << (bits - 1n)) : 0n;
  const max = descriptor.signed ? (1n << (bits - 1n)) - 1n : (1n << bits) - 1n;
  return value >= min && value <= max
    ? jet_codec_decode_ok(value)
    : jet_codec_decode_error("", `expected ${descriptor.name}, found ${descriptor.name === "U8" ? "Int" : "out-of-range Int"}`);
}

function jet_codec_typed_float32(tree) {
  const result = tree?.tag === "Float" ? jet_codec_decode_ok(Number(jet_codec_tree_payload(tree)))
    : jet_codec_typed_float(tree);
  if (!result.ok) return result;
  const value = result.value;
  return Number.isFinite(value) && Math.abs(value) <= 3.4028234663852886e38
    ? jet_codec_decode_ok(Math.fround(value))
    : jet_codec_decode_error("", "expected Float32, found out-of-range Float");
}

function jet_codec_typed_decimal(tree) {
  if (tree?.tag === "TypedText") {
    return jet_codec_decode_error("", `expected Decimal, found text ${JSON.stringify(jet_codec_tree_payload(tree))}`);
  }
  if (tree?.tag === "Number" || tree?.tag === "Text") {
    const raw = String(jet_codec_tree_payload(tree));
    const trimmed = raw.trim();
    if (trimmed.length === 0 || !/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(trimmed)) {
      return jet_codec_decode_error("", `expected Decimal: invalid Decimal string \`${raw}\``);
    }
    return jet_codec_decode_ok(jet_decimal_from_str(raw));
  }
  if (tree?.tag === "Int") {
    const raw = BigInt(jet_codec_tree_payload(tree)).toString();
    return jet_codec_decode_ok(jet_decimal_from_str(raw));
  }
  return jet_codec_decode_error("", `expected Decimal, found ${jet_datatree_kind(tree)}`);
}

function jet_codec_typed_temporal(tree, descriptor) {
  const text = jet_codec_typed_string(tree);
  if (!text.ok) return text;
  try {
    switch (descriptor.kind) {
      case "date":
      case "local_date": return jet_codec_decode_ok(jet_time_date_parse(text.value));
      case "local_time": return jet_codec_decode_ok(jet_time_parse_time(text.value));
      case "datetime": return jet_codec_decode_ok(jet_time_parse_rfc3339(text.value));
      default: return jet_codec_decode_error("", `unsupported temporal codec ${descriptor.kind}`);
    }
  } catch (error) {
    const name = descriptor.kind === "local_time" ? "LocalTime"
      : descriptor.kind === "datetime" ? "DateTime" : "Date";
    return jet_codec_decode_error("", `expected ${name}: ${String(error?.message ?? error)}`);
  }
}

function jet_codec_encode_int(value) {
  const integer = BigInt(value);
  return integer >= JET_I64_MIN && integer <= JET_I64_MAX
    ? { tag: "Int", values: [integer] }
    : { tag: "Number", values: [integer.toString()] };
}

function jet_codec_encode_typed(value, descriptor) {
  switch (descriptor?.kind) {
    case "int":
    case "intn":
      return jet_codec_encode_int(value);
    case "float":
    case "float32":
      return { tag: "Float", values: [Number(value)] };
    case "bool":
      return { tag: "Bool", values: [Boolean(value)] };
    case "string":
    case "char":
      return { tag: "Text", values: [String(value)] };
    case "range":
      return jet_codec_encode_typed(value, descriptor.base);
    case "datatree":
      return value;
    case "date":
    case "local_date":
      return { tag: "Text", values: [jet_time_date_string(value)] };
    case "local_time":
      return { tag: "Text", values: [jet_time_time_string(value)] };
    case "datetime":
      return { tag: "Text", values: [jet_time_date_time_format_rfc3339(value)] };
    case "duration":
      return jet_codec_encode_int(jet_time_clamp_i64(value));
    case "decimal":
      return { tag: "Text", values: [jet_decimal_to_string(value)] };
    case "option":
      if (value?.tag === "Err") return { tag: "Null", values: [] };
      if (value?.tag === "Ok") {
        return jet_codec_encode_typed(value.values?.[0], descriptor.inner);
      }
      throw new Error("typed codec encoder received a non-option value");
    case "list":
    case "fixed_list": {
      if (!Array.isArray(value)) throw new Error("typed codec encoder received a non-list value");
      if (descriptor.kind === "fixed_list" && BigInt(value.length) !== BigInt(descriptor.length)) {
        throw new Error(`expected a fixed list of length ${descriptor.length}, found ${value.length}`);
      }
      if (descriptor.element?.byte) {
        const bytes = value.map((item) => {
          const byte = BigInt(item);
          if (byte < 0n || byte > 255n) throw new Error("typed codec [U8] contains an out-of-range byte");
          return Number(byte);
        });
        return { tag: "Bytes", values: [bytes] };
      }
      return {
        tag: "Array",
        values: [value.map((item) => jet_codec_encode_typed(item, descriptor.element))],
      };
    }
    case "map": {
      const entries = value instanceof Map
        ? Array.from(value.entries())
        : value && typeof value === "object" ? Object.entries(value) : null;
      if (entries == null) throw new Error("typed codec encoder received a non-map value");
      return {
        tag: "Object",
        values: [entries.map(([key, item]) => [
          String(key),
          jet_codec_encode_typed(item, descriptor.value),
        ])],
      };
    }
    default:
      throw new Error("unsupported typed codec target");
  }
}

function jet_codec_typed_value(tree, descriptor) {
  switch (descriptor?.kind) {
    case "int": return jet_codec_typed_int(tree);
    case "float": return jet_codec_typed_float(tree);
    case "bool": return jet_codec_typed_bool(tree);
    case "string": return jet_codec_typed_string(tree);
    case "char": return jet_codec_typed_char(tree);
    case "float32": return jet_codec_typed_float32(tree);
    case "intn": return jet_codec_typed_intn(tree, descriptor);
    case "range": {
      const result = jet_codec_typed_value(tree, descriptor.base);
      if (!result.ok) return result;
      const value = BigInt(result.value);
      return value >= BigInt(descriptor.lo) && value <= BigInt(descriptor.hi)
        ? result
        : jet_codec_decode_error("", `value is outside Int(${descriptor.lo}..${descriptor.hi})`);
    }
    case "datatree": return jet_codec_decode_ok(tree);
    case "date":
    case "local_date":
    case "local_time":
    case "datetime": return jet_codec_typed_temporal(tree, descriptor);
    case "duration": {
      const result = jet_codec_typed_int(tree);
      if (!result.ok) return result;
      return result.value >= JET_I64_MIN && result.value <= JET_I64_MAX
        ? jet_codec_decode_ok(jet_time_clamp_i64(result.value))
        : jet_codec_decode_error("", "expected Duration, found out-of-range Int");
    }
    case "decimal": return jet_codec_typed_decimal(tree);
    case "option":
      return tree?.tag === "Null"
        ? jet_codec_decode_ok(jet_option_none())
        : (() => {
          const result = jet_codec_typed_value(tree, descriptor.inner);
          return result.ok
            ? jet_codec_decode_ok(jet_option_some(result.value))
            : result;
        })();
    case "list":
    case "fixed_list": {
      let items;
      if (descriptor.element?.byte && tree?.tag === "Bytes") items = jet_codec_tree_payload(tree);
      else if (tree?.tag === "Array") items = jet_datatree_array_items(tree);
      if (!Array.isArray(items)) {
        return jet_codec_decode_error("", `expected a list, found ${jet_datatree_kind(tree)}`);
      }
      if (descriptor.kind === "fixed_list" && BigInt(items.length) !== BigInt(descriptor.length)) {
        return jet_codec_decode_error("", `expected a fixed list of length ${descriptor.length}, found ${items.length}`);
      }
      const values = [];
      const errors = [];
      items.forEach((item, index) => {
        const result = descriptor.element?.byte && tree?.tag === "Bytes"
          ? jet_codec_decode_ok(BigInt(item))
          : jet_codec_typed_value(item, descriptor.element);
        if (result.ok) values.push(result.value);
        else errors.push(...jet_codec_decode_under(`[${index}]`, result).errors);
      });
      return errors.length === 0 ? jet_codec_decode_ok(values) : { ok: false, errors };
    }
    case "map": {
      if (descriptor.key?.kind !== "string") {
        return jet_codec_decode_error("", "comptime maps require String keys");
      }
      const entries = jet_datatree_object_entries(tree);
      if (tree?.tag !== "Object" || entries == null) {
        return jet_codec_decode_error("", `expected an object, found ${jet_datatree_kind(tree)}`);
      }
      const values = new Map();
      const errors = [];
      entries.forEach(([key, item]) => {
        const result = jet_codec_typed_value(item, descriptor.value);
        if (result.ok) values.set(String(key), result.value);
        else errors.push(...jet_codec_decode_under(String(key), result).errors);
      });
      return errors.length === 0 ? jet_codec_decode_ok(values) : { ok: false, errors };
    }
    case "user": {
      const outcome = descriptor.decode(tree);
      if (outcome?.tag === "Ok") return jet_codec_decode_ok(outcome.values?.[0]);
      if (outcome?.tag === "Err") {
        const errors = outcome.values?.[0];
        return Array.isArray(errors)
          ? { ok: false, errors }
          : jet_codec_decode_error("", String(errors ?? "decode failed"));
      }
      return jet_codec_decode_error("", "invalid Decode result");
    }
    default: return jet_codec_decode_error("", "unsupported typed codec target");
  }
}

function jet_codec_decode_typed(tree, descriptor) {
  return jet_codec_decode_result(jet_codec_typed_value(tree, descriptor));
}

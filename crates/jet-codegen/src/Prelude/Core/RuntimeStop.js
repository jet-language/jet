// D-FAIL-ERRWIRE1=D: one versioned error wire and one native JS adapter.

function jet_web_error_wire(error) {
  const cause = error?.cause && error.cause.tag
    ? error.cause.tag === "Some" ? error.cause.values?.[0] : null
    : error?.cause;
  return {
    schema: "jet.err/v1",
    message: String(error?.message ?? error),
    code: typeof error?.code === "string" ? error.code : null,
    cause: cause && typeof cause === "object" ? jet_web_error_wire(cause) : null,
  };
}

function jet_web_base_frame(error) {
  let frame = error.code
    ? `Error [${error.code}]: ${error.message}`
    : `Error: ${error.message}`;
  const appendCause = (nested, depth) => {
    if (!nested) return;
    frame += `\n${"  ".repeat(depth)}cause: ${nested.message}`;
    appendCause(nested.cause, depth + 1);
  };
  appendCause(error.cause, 1);
  return frame;
}

function jet_web_error_frame(error, journey) {
  const base = jet_web_base_frame(error);
  return journey ? `${journey}\n${base}` : base;
}

export class JetError extends Error {
  constructor(error, metadata = {}) {
    const wire = jet_web_error_wire(error);
    const cause = wire.cause ? new JetError(wire.cause) : null;
    super(wire.message, cause ? { cause } : undefined);
    this.name = "JetError";
    this.code = wire.code;
    this.cause = cause;
    this.journey = metadata.journey ?? "";
    this.frame = metadata.frame ?? jet_web_error_frame(wire, this.journey);
    this._wire = wire;
  }

  toJSON() {
    return this._wire;
  }
}

class JetWebPropagation extends Error {
  constructor(wire, journey, frame) {
    super(wire.message);
    this.name = "JetWebPropagation";
    this.wire = wire;
    this.journey = journey;
    this.frame = frame;
  }
}

function jet_web_result_value(value) {
  if (value && value.tag === "Err") return "error" in value ? value.error : value.values?.[0] ?? {};
  if (value && value.tag === "Ok") return "value" in value ? value.value : value.values?.[0];
  return value;
}

function jet_web_edge_error(error, metadata = {}) {
  return new JetError(error, metadata);
}

export function jet_web_edge_result(value, metadata = {}) {
  if (value instanceof JetWebPropagation) {
    throw jet_web_edge_error(value.wire, {
      journey: value.journey,
      frame: value.frame,
    });
  }
  if (value && value.tag === "Err") {
    const carrier = jet_web_result_value(value);
    throw jet_web_edge_error(carrier?.wire ?? carrier, {
      journey: metadata.journey ?? carrier?.journey,
      frame: metadata.frame ?? carrier?.frame,
    });
  }
  return jet_web_result_value(value);
}

// A `?` carries a typed propagation until the enclosing fallible function
// returns its Err carrier. The final edge turns that carrier into the native
// Web error object, so nested `?` sites keep one journey.
function jet_web_try(value, file, line, fnName, note = null) {
  if (value && value.tag === "Ok") return "value" in value ? value.value : value.values?.[0];
  if (value && value.tag === "Err") {
    const carrier = jet_web_result_value(value);
    const wire = jet_web_error_wire(carrier?.wire ?? carrier);
    const noteText = typeof note === "function" ? String(note() ?? "") : "";
    const current = `error propagated from: ${fnName} (${file}:${line}) via ?${noteText ? `: ${noteText}` : ""}`;
    const priorJourney = carrier?.wire ? String(carrier.journey ?? "") : "";
    const journey = priorJourney ? `${priorJourney}\n${current}` : current;
    throw new JetWebPropagation(wire, journey, jet_web_error_frame(wire, journey));
  }
  return value;
}

function jet_list_bounds_message(len, index) {
  return `the list has ${len} items, so position ${index} doesn't exist`;
}

function jet_missing_map_key_message(key) {
  return `the map has no entry for key ${JSON.stringify(String(key))}`;
}

function jet_list_get(base, index, file, line) {
  const position = Number(index);
  if (!Number.isSafeInteger(position) || position < 0 || position >= base.length) {
    jet_runtime_stop("E3010", file, line, jet_list_bounds_message(base.length, index));
  }
  return base[position];
}

function jet_map_get(base, key, file, line) {
  if (!base.has(key)) {
    jet_runtime_stop("E3001", file, line, jet_missing_map_key_message(key));
  }
  return base.get(key);
}

function jet_runtime_stop_report(code, file, line, fn_name, source_line, col, caret_len, message, locals) {
  let what = message;
  let why = "the program reached a registered runtime stop";
  let fix = "handle the condition at the reported source location";
  if (code === "E3001") {
    what = `panic: ${message}`;
    why = "the program reached a panic stop and cannot continue";
    fix = "check the source location and handle the failing condition";
  } else if (code === "E3005") {
    why = "a runtime contract condition evaluated false";
    fix = "satisfy the contract or update it";
  } else if (code === "E3010") {
    why = "the operation has no valid result for these operands";
    fix = "check the operands before the operation, or use a checked operation";
  } else if (code === "E3011") {
    why = "a #Todo hole was reached at runtime";
    fix = "implement this code before running it";
  } else if (code === "E3012") {
    why = "the call stack exceeded Jet's safe runtime limit";
    fix = "end the recursion or make progress toward a base case";
  }

  let rendered = `Stop [${code}]: ${what}\n`;
  if (file) {
    rendered += `  --> ${file}:${line}${fn_name ? ` in ${fn_name}` : ""}\n`;
  }
  if (source_line) {
    const margin = String(line).length;
    const pad = " ".repeat(margin);
    rendered += `   ${pad}|\n`;
    rendered += `${line} | ${source_line}\n`;
    rendered += `   ${pad}| ${" ".repeat(Math.max(0, col - 1))}${"^".repeat(Math.max(1, caret_len))}\n`;
  }
  if (locals) rendered += `locals: ${locals}\n`;
  rendered += ` Why: ${why}\n Fix: ${fix}\n`;
  return rendered;
}

function jet_runtime_stop(code, file, line, message, fn_name = "", source_line = "", locals = "") {
  const frame = jet_runtime_stop_report(code, file, line, fn_name, source_line, 1, 1, message, locals);
  throw jet_web_edge_error({ schema: "jet.err/v1", message, code, cause: null }, { frame });
}

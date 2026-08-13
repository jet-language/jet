// D-FAIL-EDGE1=A / I9: the JS adapter's one report and edge door.
// The adapter supplies source facts; this function owns report shape and copy.

export class JetWebRuntimeError extends Error {
  constructor(report) {
    super(report.message);
    this.name = "JetWebRuntimeError";
    this.code = report.code;
    this.message = report.message;
    this.cause = report.cause ?? null;
    this.journey = report.journey ?? "";
    this.frame = report.frame;
    this.report = report;
  }
}

class JetWebPropagation extends Error {
  constructor(report) {
    super(report.message);
    this.name = "JetWebPropagation";
    this.report = report;
  }
}

function jet_web_result_value(value) {
  if (value && value.tag === "Err") return value.values?.[0] ?? {};
  if (value && value.tag === "Ok") return value.values?.[0];
  return value;
}

function jet_web_base_frame(report) {
  let frame = report.code
    ? `Error [${report.code}]: ${report.message}`
    : `Error: ${report.message}`;
  const appendCause = (nested, depth) => {
    if (!nested) return;
    frame += `\n${"  ".repeat(depth)}cause: ${nested.message}`;
    appendCause(nested.cause, depth + 1);
  };
  appendCause(report.cause, 1);
  return frame;
}

function jet_web_report_frame(report) {
  return report.journey
    ? `${report.journey}\n${jet_web_base_frame(report)}`
    : jet_web_base_frame(report);
}

function jet_web_error_report(value) {
  const error = jet_web_result_value(value) ?? {};
  const code = typeof error.code === "string" && error.code.length > 0
    ? error.code
    : "";
  const message = String(error.message ?? error);
  const causeValue = jet_web_result_value(error.cause);
  const cause = causeValue && typeof causeValue === "object"
    ? jet_web_error_report(causeValue)
    : null;
  const report = {
    code,
    message,
    cause,
    journey: String(error.journey ?? ""),
  };
  return { ...report, frame: jet_web_report_frame(report) };
}

export function jet_web_edge_result(value) {
  if (value instanceof JetWebPropagation) {
    throw new JetWebRuntimeError(value.report);
  }
  if (value && value.tag === "Err") {
    throw new JetWebRuntimeError(jet_web_error_report(value));
  }
  return jet_web_result_value(value);
}

// A `?` carries a typed propagation until the enclosing fallible function
// returns its Err carrier. The final edge turns that carrier into the native
// Web error object, so nested `?` sites keep one journey.
function jet_web_try(value, file, line, fnName, note = null) {
  if (value && value.tag === "Ok") return value.values?.[0];
  if (value && value.tag === "Err") {
    const report = jet_web_error_report(value);
    const noteText = typeof note === "function" ? String(note() ?? "") : "";
    const current = `error propagated from: ${fnName} (${file}:${line}) via ?${noteText ? `: ${noteText}` : ""}`;
    const journey = report.journey ? `${report.journey}\n${current}` : current;
    const next = { ...report, journey };
    throw new JetWebPropagation({ ...next, frame: jet_web_report_frame(next) });
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
  throw new JetWebRuntimeError({ code, message, cause: null, journey: "", frame });
}

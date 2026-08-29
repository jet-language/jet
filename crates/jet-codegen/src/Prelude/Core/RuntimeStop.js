// D-FAIL-ERRWIRE1=D: one versioned error wire and one native JS adapter.

function jet_web_error_wire(error) {
  const cause = error?.cause && error.cause.tag
    ? error.cause.tag === "Some" ? error.cause.values?.[0] : null
    : error?.cause;
  const contextFrames = Array.isArray(error?.context_frames)
    ? error.context_frames.map((frame) => ({
        text: String(frame.text ?? ""),
        file: String(frame.file ?? ""),
        line: Number(frame.line ?? 0),
      }))
    : [];
  const sourceJourney = Array.isArray(error?.source_journey)
    ? error.source_journey.map((frame) => ({
        fn_name: String(frame.fn_name ?? frame.fnName ?? ""),
        file: String(frame.file ?? ""),
        line: Number(frame.line ?? 0),
        note: String(frame.note ?? ""),
        hops: Number(frame.hops ?? 1),
      }))
    : [];
  const conversionHistory = Array.isArray(error?.conversion_history)
    ? error.conversion_history.map((conversion) => ({
        source: String(conversion.source ?? ""),
        target: String(conversion.target ?? ""),
      }))
    : [];
  const wire = {
    schema: "jet.err/v1",
    message: String(error?.message ?? error),
    code: typeof error?.code === "string" ? error.code : null,
    cause: cause && typeof cause === "object" ? jet_web_error_wire(cause) : null,
  };
  if (typeof error?.typed_identity === "string") wire.typed_identity = error.typed_identity;
  if (contextFrames.length) wire.context_frames = contextFrames;
  if (sourceJourney.length) wire.source_journey = sourceJourney;
  if (conversionHistory.length) wire.conversion_history = conversionHistory;
  return wire;
}

function jet_web_base_frame(error) {
  let frame = error.code
    ? `Error [${error.code}]: ${error.message}`
    : `Error: ${error.message}`;
  const appendIdentity = (value) => {
    if (typeof value?.typed_identity === "string") frame += ` (type: ${value.typed_identity})`;
  };
  appendIdentity(error);
  const appendDetails = (value, depth) => {
    if (value.cause) {
      frame += `\n${"  ".repeat(depth + 1)}cause: ${value.cause.message}`;
      appendIdentity(value.cause);
      appendDetails(value.cause, depth + 1);
    }
    for (const context of value.context_frames ?? []) {
      frame += `\n${"  ".repeat(depth + 1)}context (${context.file}:${context.line}): ${context.text}`;
    }
    for (const conversion of value.conversion_history ?? []) {
      frame += `\n${"  ".repeat(depth + 1)}conversion: ${conversion.source} -> ${conversion.target}`;
    }
  };
  appendDetails(error, 0);
  return frame;
}

// D-FAIL-CTX1 / E3002: the JS projection of Foundation's trail block
// (`jet_journey_trail` in crates/jet-foundation/src/Outcome.rs). The JS tier
// runs no Rust, so this grammar is duplicated by construction; the cross-tier
// parity assertion in tests/web_build.rs is what keeps the two identical.
function jet_web_journey_trail(hops) {
  if (!hops.length) return "";
  const total = hops.reduce((sum, hop) => sum + hop.hops, 0);
  let trail = ` Trail [E3002] (${total} hop${total === 1 ? "" : "s"} via ?, origin first):\n`;
  hops.forEach((hop, index) => {
    trail += `  ${index + 1}. ${hop.fn_name ?? hop.fnName} (${hop.file}:${hop.line})`;
    if (hop.hops > 1) trail += ` ×${hop.hops}`;
    if (hop.note) trail += ` — ${hop.note}`;
    trail += "\n";
  });
  return trail;
}

// The one order, mirroring Foundation's `jet_journey_compose`: the root failure
// leads, its trail follows.
function jet_web_error_frame(error, journey) {
  const base = jet_web_base_frame(error);
  return journey ? `${base}\n${journey}` : base;
}

export class JetError extends Error {
  constructor(error, metadata = {}) {
    const wire = jet_web_error_wire(error);
    const cause = wire.cause ? new JetError(wire.cause) : null;
    super(wire.message, cause ? { cause } : undefined);
    this.name = "JetError";
    this.code = wire.code;
    this.cause = cause;
    this.typedIdentity = wire.typed_identity ?? null;
    this.contextFrames = wire.context_frames ?? [];
    this.sourceJourney = wire.source_journey ?? [];
    this.conversionHistory = wire.conversion_history ?? [];
    this.journey = metadata.journey ?? "";
    this.frame = metadata.frame ?? jet_web_error_frame(wire, this.journey);
    this._wire = wire;
  }

  toJSON() {
    return this._wire;
  }
}

class JetWebPropagation extends Error {
  constructor(wire, journey, frame, hops) {
    super(wire.message);
    this.name = "JetWebPropagation";
    this.wire = wire;
    this.journey = journey;
    this.frame = frame;
    this.hops = hops;
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

// D-FAIL-CONV: these adapters preserve the Foundation carrier fields while
// invoking the lowered conversion body. They do not reconstruct meaning from
// rendered error text.
function jet_web_default_error(value) {
  return {
    schema: "jet.err/v1",
    message: String(value),
    code: null,
    cause: null,
  };
}

function jet_web_error_from_conversion(value, source, target, original = null) {
  const converted = jet_web_result_value(value);
  const wire = jet_web_error_wire(converted?.wire ?? converted);
  const originalWire = original?.wire ? jet_web_error_wire(original.wire) : null;
  const nextWire = {
    ...wire,
    typed_identity: source,
    conversion_history: [
      ...(Array.isArray(wire.conversion_history) ? wire.conversion_history : []),
      { source, target },
    ],
  };
  if (originalWire?.source_journey && !nextWire.source_journey) {
    nextWire.source_journey = originalWire.source_journey;
  }
  if (originalWire?.context_frames && !nextWire.context_frames) {
    nextWire.context_frames = originalWire.context_frames;
  }
  return nextWire;
}

// A `?` carries a typed propagation until the enclosing fallible function
// returns its Err carrier. The final edge turns that carrier into the native
// Web error object, so nested `?` sites keep one journey. The thunk is
// important: a nested `?` can throw before its caller's adapter gets control.
function jet_web_try(valueOrThunk, file, line, fnName, note = null, convert = null, addContext = false) {
  const appendHop = (carrier, priorHops = null) => {
    const wire = jet_web_error_wire(carrier?.wire ?? carrier);
    const hops = priorHops
      ? priorHops.slice()
      : (Array.isArray(wire.source_journey) ? wire.source_journey.slice() : []);
    const last = hops[hops.length - 1];
    const noteText = typeof note === "function" ? String(note() ?? "") : "";
    if (last && (last.fn_name ?? last.fnName) === fnName && last.file === file && last.line === line) {
      // Same site again: count the repeat instead of printing the line twice.
      hops[hops.length - 1] = { ...last, hops: last.hops + 1 };
    } else {
      hops.push({
        fn_name: fnName,
        file,
        line,
        note: noteText,
        hops: 1,
      });
    }
    const journey = jet_web_journey_trail(hops);
    const nextWire = { ...wire, source_journey: hops };
    if (addContext && typeof note === "function") {
      nextWire.context_frames = [
        ...(Array.isArray(wire.context_frames) ? wire.context_frames : []),
        { text: noteText, file, line },
      ];
    }
    throw new JetWebPropagation(nextWire, journey, jet_web_error_frame(nextWire, journey), hops);
  };
  const appendCaught = (error) => {
    if (!(error instanceof JetWebPropagation)) throw error;
    appendHop({ wire: error.wire }, error.hops);
  };
  const handle = (value) => {
    if (value && value.tag === "Ok") return "value" in value ? value.value : value.values?.[0];
    if (value && value.tag === "Err") {
      let carrier = jet_web_result_value(value);
      if (typeof convert === "function") {
        const original = carrier;
        carrier = jet_web_result_value(convert(carrier, original));
      }
      appendHop(carrier);
    }
    return value;
  };
  let value;
  try {
    value = typeof valueOrThunk === "function" ? valueOrThunk() : valueOrThunk;
  } catch (error) {
    appendCaught(error);
  }
  if (value && typeof value.then === "function") {
    return value.then(handle, appendCaught);
  }
  return handle(value);
}

function jet_list_bounds_message(len, index) {
  return `The list has ${len} items, so position ${index} doesn't exist`;
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

class JetHostError extends Error {
  constructor(code, frame) {
    super(frame);
    this.name = "JetHostError";
    this.code = code;
    this.status = 101;
    this.exitCode = 101;
    this.frame = frame;
  }
}

function jet_web_runtime_context(file, line, fn_name, source_line, col, caret_len, locals) {
  const active = JET_RUNTIME_STACK.length === 0
    ? null
    : JET_RUNTIME_STACK[JET_RUNTIME_STACK.length - 1];
  return {
    file: file || active?.file || "",
    line: line || active?.line || 0,
    fn_name: fn_name || active?.fn_name || "",
    source_line: source_line || active?.source_line || "",
    col: col || active?.col || 1,
    caret_len: caret_len || active?.caret_len || 1,
    locals: locals || active?.locals || "",
  };
}

function jet_web_runtime_context_frame(context, rich_context) {
  let frame = "";
  if (context.file) {
    frame += `  --> ${context.file}:${context.line}${rich_context && context.fn_name ? ` in ${context.fn_name}` : ""}\n`;
  }
  if (rich_context && context.source_line) {
    const margin = String(context.line).length;
    const pad = " ".repeat(margin);
    frame += `   ${pad}|\n`;
    frame += `${context.line} | ${context.source_line}\n`;
    frame += `   ${pad}| ${" ".repeat(Math.max(0, context.col - 1))}${"^".repeat(Math.max(1, context.caret_len))}\n`;
  }
  if (rich_context && context.locals) frame += `locals: ${context.locals}\n`;
  return frame;
}

function jet_runtime_stop_report(code, file, line, fn_name, source_line, col, caret_len, message, locals) {
  const known = Object.prototype.hasOwnProperty.call(JET_RUNTIME_STOP_METADATA, code);
  const projected = known ? JET_RUNTIME_STOP_METADATA[code] : JET_RUNTIME_STOP_DEFAULT;
  const context = jet_web_runtime_context(file, line, fn_name, source_line, col, caret_len, locals);
  const substitute = (template, marker, value) => template.split(marker).join(String(value));
  let rendered = projected.rendered;
  const todo_parts = String(message).split(" — expected ");
  const todo_type = todo_parts.pop();
  const todo_prefix = todo_parts.join(" — expected ");
  rendered = substitute(rendered, "__JET_RUNTIME_STOP_CODE__", code);
  rendered = substitute(rendered, "__JET_RUNTIME_MESSAGE__", message);
  rendered = substitute(rendered, "__JET_RUNTIME_TODO_PREFIX__", todo_prefix);
  rendered = substitute(rendered, "__JET_RUNTIME_TODO_TYPE__", todo_type);
  rendered = substitute(rendered, "__JET_RUNTIME_FILE__", context.file);
  rendered = substitute(rendered, "__JET_RUNTIME_LINE__", context.line);
  rendered = substitute(rendered, "__JET_RUNTIME_FUNCTION__", context.fn_name);
  rendered = substitute(rendered, "__JET_RUNTIME_CONTEXT__", jet_web_runtime_context_frame(context, projected.rich_context));
  return rendered;
}

function jet_runtime_stop(code, file, line, message, fn_name = "", source_line = "", col = 1, caret_len = 1, locals = "") {
  const frame = jet_runtime_stop_report(code, file, line, fn_name, source_line, col, caret_len, message, locals);
  if (!Object.prototype.hasOwnProperty.call(JET_RUNTIME_STOP_METADATA, code)) {
    throw new JetHostError(code, frame);
  }
  throw jet_web_edge_error({ schema: "jet.err/v1", message, code, cause: null }, { frame });
}

function jet_web_wasm_host_error(outcome, metadata, status = 101) {
  const code = outcome?.error?.code || outcome?.code || "__unknown_runtime_stop__";
  const frame = outcome?.report || metadata?.frame || "Internal error: Web host failure\n";
  const error = new JetHostError(code, frame);
  error.status = status;
  error.exitCode = status;
  return error;
}

function jet_stack_overflow_message(fn_name) {
  return JET_STACK_OVERFLOW_MESSAGE.split("__JET_RUNTIME_FUNCTION__").join(String(fn_name));
}

function jet_stack_enter(file, line, fn_name, source_line, col = 1, caret_len = 1, locals = "") {
  if (JET_RUNTIME_STACK.length >= JET_RUNTIME_STACK_LIMIT) {
    jet_runtime_stop("E3012", file, line, jet_stack_overflow_message(fn_name), fn_name, source_line, col, caret_len, locals);
  }
  const frame = Object.freeze({file, line, fn_name, source_line, col, caret_len, locals});
  JET_RUNTIME_STACK.push(frame);
  return frame;
}

function jet_stack_leave(frame) {
  const index = JET_RUNTIME_STACK.lastIndexOf(frame);
  if (index >= 0) JET_RUNTIME_STACK.splice(index, 1);
}

function jet_todo_stop(file, line, expected_type) {
  jet_runtime_stop("E3011", file, line, `#Todo at ${file}:${line} — expected ${expected_type}`);
}

function jet_contract_check(condition) {
  return condition;
}

function jet_contract_fail(file, line, clause_kw, message) {
  jet_runtime_stop("E3005", file, line, `#${clause_kw} contract failed: ${message}`);
}

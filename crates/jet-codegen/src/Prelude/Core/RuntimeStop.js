// D-FAIL-ERRWIRE1=D: Rust owns the error carrier, journey, and report edge.
// This file only marshals UTF-8 bytes through the compiled Prelude ABI and
// adapts the resulting carrier to JavaScript's native Error protocol.

const JET_ERROR_WASM_MAX_U32 = 0xffffffff;
const JET_ERROR_WASM_ENCODER = new TextEncoder();
const JET_ERROR_WASM_DECODER = new TextDecoder("utf-8", { fatal: true });
const JET_ERROR_WASM_MESSAGE_SLOT = 0;
const JET_ERROR_WASM_ERROR_SLOT = 1;
const JET_ERROR_WASM_FILE_SLOT = 2;
const JET_ERROR_WASM_FUNCTION_SLOT = 3;
const JET_ERROR_WASM_NOTE_SLOT = 4;
const JET_ERROR_WASM_ORIGINAL_SLOT = 5;
const JET_ERROR_WASM_SOURCE_SLOT = 6;
const JET_ERROR_WASM_TARGET_SLOT = 7;

function jet_error_wasm_exports() {
  let wasm;
  try {
    wasm = __jetPreludeWasm;
  } catch (_error) {
    throw new Error("compiled Prelude error Wasm exports are unavailable");
  }
  if (!wasm || !wasm.memory || !wasm.memory.buffer
      || typeof wasm.jet_error_wasm_input_alloc !== "function"
      || typeof wasm.jet_error_wasm_input_free !== "function"
      || typeof wasm.jet_error_wasm_result_clear !== "function"
      || typeof wasm.jet_error_wasm_output_ptr !== "function"
      || typeof wasm.jet_error_wasm_output_len !== "function"
      || typeof wasm.jet_error_wasm_error_ptr !== "function"
      || typeof wasm.jet_error_wasm_error_len !== "function") {
    throw new Error("compiled Prelude error Wasm ABI is unavailable");
  }
  return wasm;
}

function jet_error_wasm_operation(wasm, name) {
  const operation = wasm[name];
  if (typeof operation !== "function") {
    throw new Error(`compiled Prelude error Wasm export ${name} is unavailable`);
  }
  return operation;
}

function jet_error_wasm_number(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0 || number > JET_ERROR_WASM_MAX_U32) {
    throw new Error(`compiled Prelude error Wasm ${label} is invalid`);
  }
  return number;
}

function jet_error_wasm_range(wasm, pointer, length, label) {
  const offset = jet_error_wasm_number(pointer, `${label} pointer`);
  const size = jet_error_wasm_number(length, `${label} length`);
  if (size !== 0 && offset === 0) {
    throw new Error(`compiled Prelude error Wasm ${label} pointer is null`);
  }
  const capacity = wasm.memory.buffer.byteLength;
  if (offset > capacity || size > capacity - offset) {
    throw new Error(`compiled Prelude error Wasm ${label} range is outside memory`);
  }
  return { offset, size };
}

function jet_error_wasm_encode(value, label) {
  if (typeof value !== "string") throw new TypeError(`${label} must be text`);
  const bytes = JET_ERROR_WASM_ENCODER.encode(value);
  if (bytes.length > JET_ERROR_WASM_MAX_U32) {
    throw new Error(`compiled Prelude error Wasm ${label} exceeds the u32 ABI length`);
  }
  return bytes;
}

function jet_error_wasm_release(wasm, handle, label) {
  const released = jet_error_wasm_number(
    wasm.jet_error_wasm_input_free(handle.slot, handle.pointer),
    `${label} release`,
  );
  if (released !== 1) throw new Error(`compiled Prelude error Wasm ${label} release failed`);
}

function jet_error_wasm_with_inputs(entries, invoke) {
  const wasm = jet_error_wasm_exports();
  const handles = [];
  wasm.jet_error_wasm_result_clear();
  try {
    for (const entry of entries) {
      const bytes = jet_error_wasm_encode(entry.value, entry.label);
      const pointer = jet_error_wasm_number(
        wasm.jet_error_wasm_input_alloc(entry.slot, bytes.length),
        `${entry.label} allocation`,
      );
      if (bytes.length !== 0 && pointer === 0) {
        throw new Error(`compiled Prelude error Wasm ${entry.label} allocation failed`);
      }
      const handle = { slot: entry.slot, pointer, length: bytes.length, label: entry.label };
      handles.push(handle);
      if (bytes.length === 0) {
        if (pointer !== 0) {
          throw new Error(`compiled Prelude error Wasm ${entry.label} empty allocation is invalid`);
        }
      } else {
        const range = jet_error_wasm_range(wasm, pointer, bytes.length, `${entry.label} input`);
        new Uint8Array(wasm.memory.buffer, range.offset, range.size).set(bytes);
      }
    }
    return invoke(wasm, handles);
  } finally {
    let cleanupError = null;
    for (let index = handles.length - 1; index >= 0; index -= 1) {
      try {
        jet_error_wasm_release(wasm, handles[index], handles[index].label);
      } catch (error) {
        cleanupError ??= error;
      }
    }
    wasm.jet_error_wasm_result_clear();
    if (cleanupError) throw cleanupError;
  }
}

function jet_error_wasm_output(wasm) {
  const length = jet_error_wasm_number(
    wasm.jet_error_wasm_output_len(),
    "output length",
  );
  if (length === 0) return "";
  const pointer = wasm.jet_error_wasm_output_ptr();
  const range = jet_error_wasm_range(wasm, pointer, length, "output");
  const bytes = new Uint8Array(wasm.memory.buffer, range.offset, range.size).slice();
  return JET_ERROR_WASM_DECODER.decode(bytes);
}

function jet_error_wasm_error(wasm) {
  const length = jet_error_wasm_number(
    wasm.jet_error_wasm_error_len(),
    "diagnostic length",
  );
  if (length === 0) return "";
  const pointer = wasm.jet_error_wasm_error_ptr();
  const range = jet_error_wasm_range(wasm, pointer, length, "diagnostic");
  const bytes = new Uint8Array(wasm.memory.buffer, range.offset, range.size).slice();
  return JET_ERROR_WASM_DECODER.decode(bytes);
}

function jet_error_wasm_status(wasm, statusValue, operation) {
  const status = Number(statusValue);
  if (status === 1) return;
  const detail = jet_error_wasm_error(wasm);
  if (status !== -1) {
    throw new Error(`compiled Prelude error Wasm ${operation} returned an invalid status`);
  }
  throw new Error(detail || `compiled Prelude error Wasm ${operation} failed`);
}

function jet_error_wasm_json(wasm, statusValue, operation) {
  jet_error_wasm_status(wasm, statusValue, operation);
  const text = jet_error_wasm_output(wasm);
  if (!text) throw new Error(`compiled Prelude error Wasm ${operation} returned no result`);
  try {
    return globalThis.__jetWebJsonParse(text);
  } catch (_error) {
    throw new Error(`compiled Prelude error Wasm ${operation} returned invalid JSON`);
  }
}

function jet_web_option_value(value) {
  if (!value || typeof value !== "object") return value;
  if (value.tag === "Ok") {
    return "value" in value ? value.value : value.values?.[0];
  }
  if (value.tag === "Err") return null;
  return value;
}

function jet_web_clone_json(value) {
  if (value === null || typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "bigint") return value;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new Error("error details contain a non-finite number");
    return value;
  }
  if (Array.isArray(value)) return value.map(jet_web_clone_json);
  if (typeof value === "object") {
    const copy = {};
    for (const [key, entry] of Object.entries(value)) copy[key] = jet_web_clone_json(entry);
    return copy;
  }
  throw new Error("error details contain an unsupported JSON value");
}

// JSON has no BigInt primitive, but converting an exact Jet Int to a quoted
// string changes its wire type. Native JSON.rawJSON keeps the decimal lexeme
// numeric while JSON.stringify retains all normal JSON semantics.
function jet_web_json_stringify(value) {
  return JSON.stringify(
    value,
    (_key, item) => typeof item === "bigint" ? JSON.rawJSON(item.toString()) : item,
  );
}

function jet_web_error_details(value) {
  const details = jet_web_option_value(value);
  if (!details || typeof details !== "object") return null;
  const fields = {};
  if (Array.isArray(details.fields)) {
    for (const field of details.fields) {
      const name = String(field?.name ?? "");
      const raw = field?.value;
      if (typeof raw === "string") {
        try {
          fields[name] = globalThis.__jetWebJsonParse(raw);
        } catch (_error) {
          throw new Error(`error details field ${name} is not canonical JSON`);
        }
      } else {
        fields[name] = jet_web_clone_json(raw);
      }
    }
  } else if (details.fields && typeof details.fields === "object") {
    for (const [name, raw] of Object.entries(details.fields)) {
      fields[name] = jet_web_clone_json(raw);
    }
  } else {
    throw new Error("error details fields are not an object or list");
  }
  const sourceSpan = jet_web_option_value(details.source_span);
  const source_span = sourceSpan && typeof sourceSpan === "object"
    ? {
        start: Number(sourceSpan.start ?? 0),
        end: Number(sourceSpan.end ?? 0),
      }
    : null;
  return {
    variant: String(details.variant ?? ""),
    fields,
    source_span,
  };
}

function jet_web_error_wire(error) {
  const source = error && typeof error === "object" && error._wire ? error._wire : error;
  const metadata = source && typeof source === "object"
    ? source.__jet_memo_error_metadata ?? source.__jet_error_metadata
    : null;
  const cause = jet_web_option_value(source?.cause);
  const code = jet_web_option_value(source?.code);
  const contextFrames = source?.context_frames ?? source?.contextFrames
    ?? metadata?.context_frames ?? metadata?.contextFrames;
  const sourceJourney = source?.source_journey ?? source?.sourceJourney
    ?? metadata?.source_journey ?? metadata?.sourceJourney;
  const conversionHistory = source?.conversion_history ?? source?.conversionHistory
    ?? metadata?.conversion_history ?? metadata?.conversionHistory;
  const typedIdentity = jet_web_option_value(
    source?.typed_identity ?? source?.typedIdentity
      ?? metadata?.typed_identity ?? metadata?.typedIdentity,
  );
  const details = source?.details ?? metadata?.details;
  const wire = {
    schema: "jet.err/v1",
    message: String(source?.message ?? source),
    code: typeof code === "string" ? code : null,
    cause: cause && typeof cause === "object" ? jet_web_error_wire(cause) : null,
  };
  if (typeof typedIdentity === "string") wire.typed_identity = typedIdentity;
  if (Array.isArray(contextFrames) && contextFrames.length) {
    wire.context_frames = contextFrames.map((frame) => ({
      text: String(frame?.text ?? ""),
      file: String(frame?.file ?? ""),
      line: Number(frame?.line ?? 0),
    }));
  }
  if (Array.isArray(sourceJourney) && sourceJourney.length) {
    wire.source_journey = sourceJourney.map((frame) => ({
      fn_name: String(frame?.fn_name ?? frame?.fnName ?? ""),
      file: String(frame?.file ?? ""),
      line: Number(frame?.line ?? 0),
      note: String(frame?.note ?? ""),
      hops: Number(frame?.hops ?? 1),
    }));
  }
  if (Array.isArray(conversionHistory) && conversionHistory.length) {
    wire.conversion_history = conversionHistory.map((conversion) => ({
      source: String(conversion?.source ?? ""),
      target: String(conversion?.target ?? ""),
    }));
  }
  const normalizedDetails = jet_web_error_details(details);
  if (normalizedDetails) wire.details = normalizedDetails;
  return wire;
}

function jet_web_carrier_from_wire(wire) {
  const carrier = {
    message: wire.message,
    code: wire.code === null
      ? jet_option_none()
      : jet_option_some(wire.code),
    cause: wire.cause === null
      ? jet_option_none()
      : jet_option_some(jet_web_carrier_from_wire(wire.cause)),
  };
  const metadata = {};
  if (typeof wire.typed_identity === "string") metadata.typed_identity =
    jet_option_some(wire.typed_identity);
  if (Array.isArray(wire.context_frames)) metadata.context_frames = wire.context_frames;
  if (Array.isArray(wire.source_journey)) metadata.source_journey = wire.source_journey;
  if (Array.isArray(wire.conversion_history)) {
    metadata.conversion_history = wire.conversion_history;
  }
  if (wire.details) metadata.details = jet_option_some(wire.details);
  if (Object.keys(metadata).length) {
    Object.defineProperty(carrier, "__jet_memo_error_metadata", {
      value: metadata,
      enumerable: false,
      writable: false,
    });
  }
  return carrier;
}

function jet_web_error_terminal(error) {
  const wire = jet_web_error_wire(error);
  const encoded = jet_web_json_stringify(wire);
  return jet_error_wasm_with_inputs(
    [{ slot: JET_ERROR_WASM_ERROR_SLOT, value: encoded, label: "error" }],
    (wasm, handles) => {
      const operation = jet_error_wasm_operation(wasm, "jet_error_wasm_entry_error_exit");
      const result = jet_error_wasm_json(
        wasm,
        operation(handles[0].pointer, handles[0].length),
        "entry error exit",
      );
      if (!result || result.tag !== "Err" || !result.error
          || typeof result.report !== "string" || typeof result.journey !== "string") {
        throw new Error("compiled Prelude error Wasm entry error exit returned an invalid result");
      }
      return result;
    },
  );
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
    this.details = wire.details ?? null;
    this.journey = metadata.journey ?? "";
    this.frame = metadata.frame ?? "";
    this._wire = wire;
  }

  toJSON() {
    return this._wire;
  }
}

class JetWebPropagation extends Error {
  constructor(wire) {
    const normalized = jet_web_error_wire(wire);
    super(normalized.message);
    this.name = "JetWebPropagation";
    this.wire = normalized;
  }
}

function jet_web_result_value(value) {
  if (value && value.tag === "Err") return "error" in value ? value.error : value.values?.[0] ?? {};
  if (value && value.tag === "Ok") return "value" in value ? value.value : value.values?.[0];
  return value;
}

function jet_web_report_error(error, metadata = {}) {
  const result = jet_web_error_terminal(error);
  return new JetError(result.error, {
    ...metadata,
    journey: metadata.journey ?? result.journey,
    frame: metadata.frame ?? result.report,
  });
}

function jet_web_edge_error(error, metadata = {}) {
  if (Object.prototype.hasOwnProperty.call(metadata, "frame")) {
    return new JetError(error, metadata);
  }
  return jet_web_report_error(error, metadata);
}

export function jet_web_edge_result(value, metadata = {}) {
  if (value instanceof JetWebPropagation) {
    throw jet_web_report_error(value.wire, metadata);
  }
  if (value && value.tag === "Err") {
    const carrier = jet_web_result_value(value);
    if (Object.prototype.hasOwnProperty.call(metadata, "frame")) {
      throw jet_web_edge_error(carrier?.wire ?? carrier, metadata);
    }
    throw jet_web_report_error(carrier?.wire ?? carrier, metadata);
  }
  return jet_web_result_value(value);
}

function jet_journey_reset() {
  jet_error_wasm_with_inputs([], (wasm) => {
    const operation = jet_error_wasm_operation(wasm, "jet_error_wasm_journey_reset");
    jet_error_wasm_status(wasm, operation(), "journey reset");
  });
}

function jet_journey_frame_text(file, line, fnName, note) {
  jet_error_wasm_with_inputs(
    [
      { slot: JET_ERROR_WASM_FILE_SLOT, value: String(file), label: "source file" },
      { slot: JET_ERROR_WASM_FUNCTION_SLOT, value: String(fnName), label: "function" },
      { slot: JET_ERROR_WASM_NOTE_SLOT, value: String(note), label: "journey note" },
    ],
    (wasm, handles) => {
      const operation = jet_error_wasm_operation(wasm, "jet_error_wasm_journey_frame_text");
      jet_error_wasm_status(
        wasm,
        operation(
          handles[0].pointer,
          handles[0].length,
          jet_error_wasm_number(line, "source line"),
          handles[1].pointer,
          handles[1].length,
          handles[2].pointer,
          handles[2].length,
        ),
        "journey frame",
      );
    },
  );
}

function jet_err_from_message(message) {
  return jet_error_wasm_with_inputs(
    [{ slot: JET_ERROR_WASM_MESSAGE_SLOT, value: String(message), label: "message" }],
    (wasm, handles) => {
      const operation = jet_error_wasm_operation(wasm, "jet_error_wasm_err_from_message");
      const wire = jet_error_wasm_json(
        wasm,
        operation(handles[0].pointer, handles[0].length),
        "error from message",
      );
      return jet_web_carrier_from_wire(wire);
    },
  );
}

function jet_err_with_context_frame(error, file, line, fnName, note) {
  const encoded = jet_web_json_stringify(jet_web_error_wire(error));
  return jet_error_wasm_with_inputs(
    [
      { slot: JET_ERROR_WASM_ERROR_SLOT, value: encoded, label: "error" },
      { slot: JET_ERROR_WASM_FILE_SLOT, value: String(file), label: "source file" },
      { slot: JET_ERROR_WASM_FUNCTION_SLOT, value: String(fnName), label: "function" },
      { slot: JET_ERROR_WASM_NOTE_SLOT, value: String(note), label: "context note" },
    ],
    (wasm, handles) => {
      const operation = jet_error_wasm_operation(wasm, "jet_error_wasm_with_context_frame");
      const wire = jet_error_wasm_json(
        wasm,
        operation(
          handles[0].pointer,
          handles[0].length,
          handles[1].pointer,
          handles[1].length,
          jet_error_wasm_number(line, "source line"),
          handles[2].pointer,
          handles[2].length,
          handles[3].pointer,
          handles[3].length,
        ),
        "error context frame",
      );
      return jet_web_carrier_from_wire(wire);
    },
  );
}

function jet_web_default_error(value) {
  return jet_err_from_message(String(value));
}

function jet_web_error_from_conversion(value, source, target, original = null) {
  const converted = jet_web_result_value(value);
  const convertedWire = jet_web_error_wire(converted?.wire ?? converted);
  const originalWire = original == null
    ? ""
    : jet_web_json_stringify(jet_web_error_wire(original?.wire ?? original));
  return jet_error_wasm_with_inputs(
    [
      {
        slot: JET_ERROR_WASM_ERROR_SLOT,
        value: jet_web_json_stringify(convertedWire),
        label: "converted error",
      },
      { slot: JET_ERROR_WASM_ORIGINAL_SLOT, value: originalWire, label: "original error" },
      { slot: JET_ERROR_WASM_SOURCE_SLOT, value: String(source), label: "conversion source" },
      { slot: JET_ERROR_WASM_TARGET_SLOT, value: String(target), label: "conversion target" },
    ],
    (wasm, handles) => {
      const operation = jet_error_wasm_operation(wasm, "jet_error_wasm_from_conversion");
      const wire = jet_error_wasm_json(
        wasm,
        operation(
          handles[0].pointer,
          handles[0].length,
          handles[1].pointer,
          handles[1].length,
          handles[2].pointer,
          handles[2].length,
          handles[3].pointer,
          handles[3].length,
        ),
        "error conversion",
      );
      return jet_web_carrier_from_wire(wire);
    },
  );
}

function jet_entry_error_exit_jet(error) {
  const result = jet_web_error_terminal(error?.wire ?? error);
  throw new JetError(result.error, { journey: result.journey, frame: result.report });
}

// A `?` carries a typed propagation until the enclosing fallible function
// returns its Err carrier. The final edge turns that carrier into the native
// Web error object. Rust owns the one journey and report policy.
function jet_web_try(valueOrThunk, file, line, fnName, note = null, convert = null, addContext = false) {
  const appendHop = (carrier) => {
    const noteText = typeof note === "function" ? String(note() ?? "") : "";
    let next;
    if (addContext) {
      next = jet_err_with_context_frame(carrier?.wire ?? carrier, file, line, fnName, noteText);
    } else {
      jet_journey_frame_text(file, line, fnName, noteText);
      next = carrier?.wire ?? carrier;
    }
    throw new JetWebPropagation(next);
  };
  const appendCaught = (error) => {
    if (!(error instanceof JetWebPropagation)) throw error;
    appendHop(error.wire);
  };
  const handle = (value) => {
    if (value && value.tag === "Ok") {
      jet_journey_reset();
      return "value" in value ? value.value : value.values?.[0];
    }
    if (value && value.tag === "Err") {
      let carrier = jet_web_result_value(value);
      if (typeof convert === "function") {
        carrier = jet_web_result_value(convert(carrier, carrier));
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

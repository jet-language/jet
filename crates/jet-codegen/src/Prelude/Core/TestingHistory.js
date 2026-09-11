// D-TEST-HISTORY1=A: Web marshals history callbacks through the canonical
// Foundation runner embedded in app.wasm. JavaScript owns transport only;
// generation, replay, comparison, and shrinking stay in Rust.

let __jetTestingHistoryWasm = null;
const __jetTestingHistoryCallbacks = new Map();
let __jetTestingHistoryNextCallback = 1;
const __jetTestingHistoryEncoder = new TextEncoder();
const __jetTestingHistoryDecoder = new TextDecoder();
const __jetTestingHistoryCallableMetadata = new WeakMap();

// Metadata retains exactly the environment retained by invocation. Weak keys
// let returned, copied and discarded closures keep their ordinary lifetime.
function jet_testing_history_web_callable(callback, identity, captures) {
  __jetTestingHistoryCallableMetadata.set(callback, { identity, captures });
  return callback;
}

function jet_testing_history_web_visit(value, active, encode) {
  if (active.has(value)) throw new TypeError("history capture contains a cycle");
  active.add(value);
  try {
    return encode();
  } finally {
    active.delete(value);
  }
}

function jet_testing_history_web_callable_provenance(callback, active = new Set()) {
  const metadata = __jetTestingHistoryCallableMetadata.get(callback);
  if (!metadata || !metadata.identity) {
    throw new TypeError("history callback has no checked runtime provenance");
  }
  return jet_testing_history_web_visit(callback, active, () => ({
    function_identity: metadata.identity,
    captures: metadata.captures(active),
  }));
}

function jet_testing_history_web_float_bits(value) {
  const bytes = new DataView(new ArrayBuffer(8));
  bytes.setFloat64(0, value, false);
  return bytes.getBigUint64(0, false).toString(16).padStart(16, "0");
}

// The outer type and each nested value remain ordinary lossless DataTree JSON.
// Rust canonicalizes these bytes and hashes them; JS never makes fingerprints.
function jet_testing_history_web_capture(value, key, types, active) {
  const schema = types.get(key);
  if (!schema) throw new TypeError("history capture has no checked type");
  const encode = (value, key) => jet_testing_history_web_capture(value, key, types, active);
  const invalid = () => { throw new TypeError("history capture is opaque or has an invalid typed value"); };
  let encoded;
  switch (schema.kind) {
    case "unit":
      if (value !== undefined && value !== null) return invalid();
      encoded = null;
      break;
    case "integer":
      if (typeof value !== "bigint" && !(typeof value === "number" && Number.isSafeInteger(value))) return invalid();
      encoded = BigInt(value);
      break;
    case "float":
      if (typeof value !== "number") return invalid();
      encoded = jet_testing_history_web_float_bits(value);
      break;
    case "boolean":
      if (typeof value !== "boolean") return invalid();
      encoded = value;
      break;
    case "text":
      if (typeof value !== "string") return invalid();
      encoded = value;
      break;
    case "absent":
      if (!jet_is_absent(value)) return invalid();
      encoded = null;
      break;
    case "bytes":
      if (!(value instanceof Uint8Array)) return invalid();
      encoded = Array.from(value);
      break;
    case "callable":
      encoded = jet_testing_history_web_callable_provenance(value, active);
      break;
    case "transparent":
      encoded = encode(value, schema.inner);
      break;
    case "shared":
      encoded = jet_testing_history_web_visit(value, active, () => encode(jet_shared_get(value), schema.inner));
      break;
    case "list":
      if (!Array.isArray(value)) return invalid();
      encoded = jet_testing_history_web_visit(value, active, () => value.map((item) => encode(item, schema.item)));
      break;
    case "map":
      if (!(value instanceof Map)) return invalid();
      encoded = jet_testing_history_web_visit(value, active, () => Array.from(value, ([key, item]) => (
        [encode(key, schema.key), encode(item, schema.value)]
      )));
      break;
    case "record":
      if (!value || typeof value !== "object" || Array.isArray(value)) return invalid();
      encoded = jet_testing_history_web_visit(value, active, () => schema.fields.map(([name, type]) => {
        if (!Object.hasOwn(value, name)) return invalid();
        return [name, encode(value[name], type)];
      }));
      break;
    case "enum": {
      if (!value || typeof value !== "object" || !Array.isArray(value.values)) return invalid();
      const variant = schema.variants.find(([tag]) => tag === value.tag);
      if (!variant || variant[1].length !== value.values.length) return invalid();
      encoded = jet_testing_history_web_visit(value, active, () => ({
        tag: value.tag,
        values: variant[1].map((type, index) => encode(value.values[index], type)),
      }));
      break;
    }
    case "data":
      encoded = jet_testing_history_web_capture_data(value, active);
      break;
    default:
      return invalid();
  }
  return { type: schema.type, value: encoded };
}

function jet_testing_history_web_capture_data(value, active) {
  if (value === null) return { type: "null", value: null };
  switch (typeof value) {
    case "string": case "boolean": case "bigint":
      return { type: typeof value, value };
    case "number":
      return { type: "number", value: jet_testing_history_web_float_bits(value) };
    case "object":
      return jet_testing_history_web_visit(value, active, () => {
        if (value instanceof Uint8Array) return { type: "bytes", value: Array.from(value) };
        if (Array.isArray(value)) {
          return { type: "array", value: value.map((item) => jet_testing_history_web_capture_data(item, active)) };
        }
        if (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null) {
          throw new TypeError("history DataTree capture contains an opaque object");
        }
        return {
          type: "object",
          value: Object.keys(value).sort().map((key) => {
            const field = Object.getOwnPropertyDescriptor(value, key);
            if (!Object.hasOwn(field, "value")) throw new TypeError("history DataTree capture contains an accessor");
            return [key, jet_testing_history_web_capture_data(field.value, active)];
          }),
        };
      });
    default:
      throw new TypeError("history DataTree capture contains an unsupported value");
  }
}

function jet_testing_history_web_bind_wasm(wasm) {
  const required = [
    "jet_testing_history_web_rng_alloc",
    "jet_testing_history_web_rng_release",
    "jet_testing_history_web_rng_state",
    "jet_testing_history_web_rng_next_u64",
    "jet_testing_history_web_rng_below",
  ];
  if (!wasm || !wasm.memory
      || required.some((name) => typeof wasm[name] !== "function")) {
    throw new Error("testing history WebWasm binding is invalid");
  }
  __jetTestingHistoryWasm = wasm;
  if (typeof wasm.jet_testing_history_web_register_strategies === "function") {
    wasm.jet_testing_history_web_register_strategies();
  }
}

function jet_testing_history_web_require_wasm() {
  if (!__jetTestingHistoryWasm) throw new Error("testing history WebWasm is not bound");
  return __jetTestingHistoryWasm;
}

function jet_testing_history_web_register_callback(callback) {
  if (typeof callback !== "function") throw new TypeError("history callback must be callable");
  const id = __jetTestingHistoryNextCallback;
  __jetTestingHistoryNextCallback += 1;
  __jetTestingHistoryCallbacks.set(id, callback);
  return id;
}

function jet_testing_history_web_callback_payload(callback, pointer, length) {
  const wasm = jet_testing_history_web_require_wasm();
  const fn = __jetTestingHistoryCallbacks.get(Number(callback));
  if (!fn) return { ok: false, reason: "history callback is not registered" };
  try {
    const bytes = new Uint8Array(wasm.memory.buffer, Number(pointer), Number(length));
    const input = globalThis.__jetWebJsonParse(__jetTestingHistoryDecoder.decode(bytes));
    const value = fn(input);
    if (value && typeof value.then === "function") {
      return { ok: false, reason: "history callbacks must be synchronous" };
    }
    return { ok: true, value };
  } catch (error) {
    const reason = error && typeof error.message === "string" ? error.message : String(error);
    return { ok: false, reason };
  }
}

function jet_testing_history_web_rng(handle) {
  const wasm = jet_testing_history_web_require_wasm();
  const opaque = BigInt(handle);
  return {
    next_u64() {
      return wasm.jet_testing_history_web_rng_next_u64(opaque);
    },
    below(bound) {
      return wasm.jet_testing_history_web_rng_below(opaque, BigInt(bound));
    },
    coin() {
      return this.below(2n) === 0n;
    },
  };
}

function jet_testing_history_rng_next_u64(rng) {
  return rng.next_u64();
}

function jet_testing_history_rng_below(rng, bound) {
  return rng.below(bound);
}

// Rust invokes this import while it runs the Foundation history campaign. The
// returned packed pointer/length identifies a temporary JSON response allocated
// by the same WASM module; no history policy is implemented in this callback.
function jet_testing_history_web_imports() {
  return {
    jet_testing_history_web_callback(callback, pointer, length) {
      try {
        const wasm = jet_testing_history_web_require_wasm();
        const payload = jet_testing_history_web_callback_payload(callback, pointer, length);
        const encoded = __jetTestingHistoryEncoder.encode(jet_web_json_stringify(payload));
        const target = wasm.jet_testing_history_web_output_alloc(encoded.length);
        if (!target) return 0n;
        new Uint8Array(wasm.memory.buffer, target, encoded.length).set(encoded);
        return (BigInt(target) << 32n) | BigInt(encoded.length);
      } catch (_) {
        return 0n;
      }
    },
  };
}

function jet_testing_histories(seed, cases, strategy, model, actual, observe, commandType) {
  const wasm = jet_testing_history_web_require_wasm();
  strategy = jet_web_option_value(strategy);
  const callbacksToCheck = [model, actual, observe];
  if (callbacksToCheck.some((callback) => typeof callback !== "function")) {
    throw new TypeError("testing histories callbacks must be callable");
  }
  if (strategy !== null && strategy !== undefined
      && (typeof strategy !== "object"
        || typeof strategy.generate !== "function"
        || typeof strategy.rebuild !== "function"
        || typeof strategy.valid !== "function"
        || !strategy.bounds
        || !Array.isArray(strategy.distributions))) {
    throw new TypeError("testing histories strategy has invalid fields");
  }
  const descriptor = commandType && typeof commandType === "object"
    ? commandType
    : { command_type: String(commandType), provenance: null };
  const selected = [["model", model], ["actual", actual], ["observe", observe]];
  if (strategy) {
    selected.push(["generate", strategy.generate], ["rebuild", strategy.rebuild], ["valid", strategy.valid]);
  }
  const callbackProvenance = selected.map(([role, callback]) => ({
    role,
    ...jet_testing_history_web_callable_provenance(callback),
  }));
  if (!strategy) {
    callbackProvenance.push({ role: "strategy", function_identity: `derived:${descriptor.command_type}`, captures: [] });
  }
  const callbacks = [];
  const register = (callback) => {
    const id = jet_testing_history_web_register_callback(callback);
    callbacks.push(id);
    return id;
  };
  let pointer = 0;
  try {
    callbacksToCheck.forEach(register);
    // Freeze callback selection for this invocation. Captured state stays live.
    const generate = selected[3]?.[1];
    const rebuild = selected[4]?.[1];
    const valid = selected[5]?.[1];
    const strategyCallbacks = strategy
      ? {
          generate: register((input) => {
            const rng = jet_testing_history_web_rng(input.rng_handle);
            return generate(rng, input.case_index, input.max_steps) ?? null;
          }),
          rebuild: register((input) => rebuild(input)),
          valid: register((input) => Boolean(valid(input))),
        }
      : null;
    const wire = jet_web_json_stringify({
      seed,
      cases,
      command_type: String(descriptor.command_type),
      provenance: descriptor.provenance ?? null,
      callback_provenance: callbackProvenance,
      strategy: strategy
        ? {
            ...strategyCallbacks,
            bounds: strategy.bounds,
            distributions: strategy.distributions,
          }
        : null,
      model: BigInt(callbacks[0]),
      actual: BigInt(callbacks[1]),
      observe: BigInt(callbacks[2]),
    });
    const encoded = __jetTestingHistoryEncoder.encode(wire);
    pointer = wasm.jet_testing_history_web_input_alloc(encoded.length);
    if (!pointer) throw new Error("core.testing histories WebWasm input allocation failed");
    new Uint8Array(wasm.memory.buffer, pointer, encoded.length).set(encoded);
    const status = Number(wasm.jet_testing_history_web_call(pointer, encoded.length));
    const outputPointer = Number(wasm.jet_testing_history_web_output_ptr());
    const outputLength = Number(wasm.jet_testing_history_web_output_len());
    if (status !== 0 || !outputPointer || !outputLength) {
      throw new Error("core.testing histories WebWasm returned no result");
    }
    const output = new Uint8Array(wasm.memory.buffer, outputPointer, outputLength);
    return globalThis.__jetWebJsonParse(__jetTestingHistoryDecoder.decode(output));
  } finally {
    for (const callback of callbacks) __jetTestingHistoryCallbacks.delete(callback);
    if (pointer) wasm.jet_testing_history_web_input_free(pointer);
    wasm.jet_testing_history_web_output_clear();
  }
}

function jet_testing_status(comparison) {
  return comparison?.status ?? "unavailable";
}

function jet_testing_assert_equal(comparison) {
  return comparison?.status === "matched"
    && comparison?.universal_proof === false
    && comparison?.first_difference < 0
    && Array.isArray(comparison?.inputs)
    && comparison.inputs.length > 0;
}

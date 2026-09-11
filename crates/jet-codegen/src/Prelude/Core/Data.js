// D-DATA-WEB1=A: Web only marshaling for the canonical typed DataFlow kernel.
// This module owns bytes, handles, and callback transport only. Decoding,
// limits, query execution, snapshot identity, and lifetime state remain in the
// Rust DataFlow carrier.

let __jetDataWebWasm = null;
const __jetDataWebEncoder = new TextEncoder();
const __jetDataWebCallbacks = new Map();
let __jetDataWebNextCallback = 1;

function jet_data_web_bind_wasm(wasm) {
  __jetDataWebWasm = wasm;
}

function jet_data_web_require_wasm() {
  if (!__jetDataWebWasm || !__jetDataWebWasm.memory) {
    throw new Error("core.data WebWasm bridge is unavailable");
  }
  return __jetDataWebWasm;
}

function jet_data_web_register_callback(callback) {
  if (typeof callback !== "function") throw new TypeError("data query callback must be callable");
  const id = __jetDataWebNextCallback++;
  __jetDataWebCallbacks.set(id, callback);
  return id;
}

function jet_data_web_decode_callback_value(id, pointer, length) {
  const wasm = jet_data_web_require_wasm();
  const bytes = new Uint8Array(wasm.memory.buffer, Number(pointer), Number(length));
  return jetWebJsonParse(new TextDecoder().decode(bytes));
}

function jet_data_web_imports() {
  return {
    jet_data_web_callback_bool(callback, pointer, length) {
      const fn = __jetDataWebCallbacks.get(Number(callback));
      if (!fn) return 0;
      try {
        return fn(jet_data_web_decode_callback_value(callback, pointer, length)) ? 1 : 0;
      } catch (_) {
        return 0;
      }
    },
    jet_data_web_callback_key(callback, pointer, length) {
      const fn = __jetDataWebCallbacks.get(Number(callback));
      if (!fn) return 0n;
      try {
        const key = String(fn(jet_data_web_decode_callback_value(callback, pointer, length)));
        const encoded = __jetDataWebEncoder.encode(key);
        const wasm = jet_data_web_require_wasm();
        const target = wasm.jet_data_web_input_alloc(3, encoded.length);
        if (!target) return 0n;
        new Uint8Array(wasm.memory.buffer, target, encoded.length).set(encoded);
        return (BigInt(target) << 32n) | BigInt(encoded.length);
      } catch (_) {
        return 0n;
      }
    },
    jet_data_web_callback_value(callback, pointer, length) {
      const fn = __jetDataWebCallbacks.get(Number(callback));
      if (!fn) return 0n;
      try {
        const value = fn(jet_data_web_decode_callback_value(callback, pointer, length));
        const encoded = __jetDataWebEncoder.encode(jet_web_json_stringify(value === undefined ? null : value));
        const wasm = jet_data_web_require_wasm();
        const target = wasm.jet_data_web_input_alloc(3, encoded.length);
        if (!target) return 0n;
        new Uint8Array(wasm.memory.buffer, target, encoded.length).set(encoded);
        return (BigInt(target) << 32n) | BigInt(encoded.length);
      } catch (_) {
        return 0n;
      }
    },
  };
}

function jet_data_web_invoke(operation, args, typeArgs = []) {
  const wasm = jet_data_web_require_wasm();
  const encoded = __jetDataWebEncoder.encode(jet_web_json_stringify({
    op: operation,
    args,
    type_args: typeArgs,
  }));
  const pointer = wasm.jet_data_web_input_alloc(0, encoded.length);
  if (!pointer) throw new Error("core.data WebWasm input allocation failed");
  try {
    new Uint8Array(wasm.memory.buffer, pointer, encoded.length).set(encoded);
    const status = Number(wasm.jet_data_web_call(pointer, encoded.length));
    const outputPointer = Number(wasm.jet_data_web_output_ptr());
    const outputLength = Number(wasm.jet_data_web_output_len());
    const output = outputPointer && outputLength
      ? jetWebJsonParse(new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, outputPointer, outputLength)))
      : null;
    wasm.jet_data_web_output_clear();
    if (status !== 0) {
      const detail = output?.error;
      throw new Error(detail?.reason || detail?.message || `core.data ${operation} failed`);
    }
    return output;
  } finally {
    wasm.jet_data_web_input_free(0, pointer);
  }
}

function jet_data_web_type_args(typeArgs) {
  return Array.isArray(typeArgs) ? typeArgs : [];
}

function jet_data_loader_load(locator, limits, typeArgs = []) {
  return jet_data_web_invoke("load", [locator, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_load_default(locator, typeArgs = []) {
  return jet_data_web_invoke("load_default", [locator], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_file(path, format, limits, typeArgs = []) {
  return jet_data_web_invoke("file", [path, format, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_file_member(path, member, format, limits, typeArgs = []) {
  return jet_data_web_invoke("file_member", [path, member, format, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_url(url, format, authority, limits, typeArgs = []) {
  return jet_data_web_invoke("url", [url, format, authority, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_database(query, parameters, authority, limits, typeArgs = []) {
  return jet_data_web_invoke("database", [query, parameters, authority, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_value(value, limits, typeArgs = []) {
  return jet_data_web_invoke("value", [value, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_snapshot(loader, typeArgs = []) {
  return jet_data_web_invoke("snapshot", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_bind(loader, payload, typeArgs = []) {
  const bytes = payload instanceof Uint8Array ? Array.from(payload, Number) : Array.from(payload || [], Number);
  return jet_data_web_invoke("bind", [loader, bytes], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_bind_text(loader, payload, typeArgs = []) {
  return jet_data_web_invoke("bind_text", [loader, String(payload)], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_cancel(loader, typeArgs = []) {
  return jet_data_web_invoke("loader_cancel", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_offline(loader, enabled, typeArgs = []) {
  return jet_data_web_invoke("offline", [loader, enabled], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_invalidate(loader, cause, typeArgs = []) {
  return jet_data_web_invoke("invalidate", [loader, cause], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_needs_refresh(loader, typeArgs = []) {
  return jet_data_web_invoke("needs_refresh", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_ready(loader, typeArgs = []) {
  return jet_data_web_invoke("ready", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_status(loader, typeArgs = []) {
  return jet_data_web_invoke("loader_status", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_source_identity(loader, typeArgs = []) {
  return jet_data_web_invoke("source_identity", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_authority_of(loader, typeArgs = []) {
  return jet_data_web_invoke("authority_of", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_snapshot_reusable(previous, current, typeArgs = []) {
  return jet_data_web_invoke("snapshot_reusable", [previous, current], jet_data_web_type_args(typeArgs));
}
function jet_data_loader_stream(loader, typeArgs = []) {
  return jet_data_web_invoke("stream", [loader], jet_data_web_type_args(typeArgs));
}
function jet_data_stream_next(stream, typeArgs = []) {
  return jet_data_web_invoke("stream_next", [stream], jet_data_web_type_args(typeArgs));
}
function jet_data_stream_collect(stream, typeArgs = []) {
  return jet_data_web_invoke("stream_collect", [stream], jet_data_web_type_args(typeArgs));
}
function jet_data_stream_cancel(stream, typeArgs = []) {
  return jet_data_web_invoke("stream_cancel", [stream], jet_data_web_type_args(typeArgs));
}

function jet_data_query(rows, typeArgs = []) {
  return jet_data_web_invoke("query", [rows], jet_data_web_type_args(typeArgs));
}
function jet_data_query_filter(query, predicate, typeArgs = []) {
  const callback = jet_data_web_register_callback(predicate);
  return jet_data_web_invoke("filter", [query, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_sort_by(query, key, typeArgs = []) {
  const callback = jet_data_web_register_callback(key);
  return jet_data_web_invoke("sort_by", [query, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_map(query, mapper, typeArgs = []) {
  const callback = jet_data_web_register_callback(mapper);
  return jet_data_web_invoke("map", [query, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_min(query, key, typeArgs = []) {
  const callback = jet_data_web_register_callback(key);
  return jet_data_web_invoke("min", [query, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_max(query, key, typeArgs = []) {
  const callback = jet_data_web_register_callback(key);
  return jet_data_web_invoke("max", [query, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_inner_join(left, right, leftKey, rightKey, typeArgs = []) {
  const leftCallback = jet_data_web_register_callback(leftKey);
  const rightCallback = jet_data_web_register_callback(rightKey);
  return jet_data_web_invoke(
    "inner_join",
    [left, right, leftCallback, rightCallback],
    jet_data_web_type_args(typeArgs),
  );
}
function jet_data_query_left_join(left, right, leftKey, rightKey, typeArgs = []) {
  const leftCallback = jet_data_web_register_callback(leftKey);
  const rightCallback = jet_data_web_register_callback(rightKey);
  return jet_data_web_invoke(
    "left_join",
    [left, right, leftCallback, rightCallback],
    jet_data_web_type_args(typeArgs),
  );
}
function jet_data_query_group_by(query, key, typeArgs = []) {
  const callback = jet_data_web_register_callback(key);
  return jet_data_web_invoke("group_by", [query, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_watch(query, typeArgs = []) {
  return jet_data_web_invoke("watch", [query], jet_data_web_type_args(typeArgs));
}
function jet_data_watch_get(watch, typeArgs = []) {
  return jet_data_web_invoke("watch_get", [watch], jet_data_web_type_args(typeArgs));
}
function jet_data_watch_status(watch, typeArgs = []) {
  return jet_data_web_invoke("watch_status", [watch], jet_data_web_type_args(typeArgs));
}
function jet_data_watch_cancel(watch, typeArgs = []) {
  return jet_data_web_invoke("watch_cancel", [watch], jet_data_web_type_args(typeArgs));
}
function jet_data_group_count_query(grouped, typeArgs = []) {
  return jet_data_web_invoke("group_count", [grouped], jet_data_web_type_args(typeArgs));
}
function jet_data_group_sum_query(grouped, value, typeArgs = []) {
  const callback = jet_data_web_register_callback(value);
  return jet_data_web_invoke("group_sum", [grouped, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_group_mean_query(grouped, value, typeArgs = []) {
  const callback = jet_data_web_register_callback(value);
  return jet_data_web_invoke("group_mean", [grouped, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_collect(value, typeArgs = []) {
  return jet_data_web_invoke("collect", [value], jet_data_web_type_args(typeArgs));
}
function jet_data_query_plan(value, typeArgs = []) {
  return jet_data_web_invoke("plan", [value], jet_data_web_type_args(typeArgs));
}
function jet_data_count(value, typeArgs = []) {
  return jet_data_web_invoke("count", [value], jet_data_web_type_args(typeArgs));
}
function jet_data_status(typeArgs = []) {
  return jet_data_web_invoke("status", [], jet_data_web_type_args(typeArgs));
}
function jet_data_require_bridge(value, typeArgs = []) {
  return jet_data_web_invoke("require_bridge", [value], jet_data_web_type_args(typeArgs));
}

function jet_data_left_join(left, right, leftKey, rightKey, typeArgs = []) {
  const leftCallback = jet_data_web_register_callback(leftKey);
  const rightCallback = jet_data_web_register_callback(rightKey);
  return jet_data_web_invoke(
    "left_join",
    [left, right, leftCallback, rightCallback],
    jet_data_web_type_args(typeArgs),
  );
}
function jet_data_inner_join(left, right, leftKey, rightKey, typeArgs = []) {
  const leftCallback = jet_data_web_register_callback(leftKey);
  const rightCallback = jet_data_web_register_callback(rightKey);
  return jet_data_web_invoke(
    "inner_join",
    [left, right, leftCallback, rightCallback],
    jet_data_web_type_args(typeArgs),
  );
}
function jet_data_track(value, key, typeArgs = []) {
  const callback = jet_data_web_register_callback(key);
  return jet_data_web_invoke("track", [value, callback], jet_data_web_type_args(typeArgs));
}
function jet_data_query_tracked(tracked, typeArgs = []) {
  return jet_data_web_invoke("tracked_query", [tracked], jet_data_web_type_args(typeArgs));
}
function jet_data_track_insert(tracked, row, typeArgs = []) {
  return jet_data_web_invoke("track_insert", [tracked, row], jet_data_web_type_args(typeArgs));
}
function jet_data_track_replace(tracked, key, row, typeArgs = []) {
  return jet_data_web_invoke("track_replace", [tracked, key, row], jet_data_web_type_args(typeArgs));
}
function jet_data_track_remove(tracked, key, typeArgs = []) {
  return jet_data_web_invoke("track_remove", [tracked, key], jet_data_web_type_args(typeArgs));
}
function jet_data_csv_reader(input, limits, typeArgs = []) {
  return jet_data_web_invoke("csv_reader", [input, limits], jet_data_web_type_args(typeArgs));
}
function jet_data_json_reader(input, limits, typeArgs = []) {
  return jet_data_web_invoke("json_reader", [input, limits], jet_data_web_type_args(typeArgs));
}

function jet_data_web_release(value) {
  if (!value || typeof value !== "object") return;
  const handle = Number(value.handle);
  if (!Number.isSafeInteger(handle) || handle < 0) return;
  jet_data_web_invoke("release", [value], []);
}

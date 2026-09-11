// Browser transport for OnnxRuntimeProvider::browser. Package semantics and
// errors stay in Rust. Runtime bytes are supplied by the embedding, never fetched.
// Usage: const host = createOnnxRuntimeWebHost(approvedArchiveBlob);
//        const {instance} = await WebAssembly.instantiate(appBytes, host.imports);
//        host.bind(instance.exports);

const utf8 = new TextDecoder("utf-8", { fatal: true });
const encoder = new TextEncoder();

export class PacketReader {
  constructor(bytes) { this.bytes = bytes; this.offset = 0; }
  take(n) {
    if (!Number.isSafeInteger(n) || n < 0 || n > this.bytes.length - this.offset) throw new Error("truncated model host packet");
    const value = this.bytes.subarray(this.offset, this.offset + n); this.offset += n; return value;
  }
  u32() { const b = this.take(4); return new DataView(b.buffer, b.byteOffset, 4).getUint32(0, true); }
  u64() { const b = this.take(8); return new DataView(b.buffer, b.byteOffset, 8).getBigUint64(0, true); }
  blob() { return this.take(this.u32()); }
  text() { return utf8.decode(this.blob()); }
  end() { if (this.offset !== this.bytes.length) throw new Error("trailing model host packet bytes"); }
}

export class PacketWriter {
  constructor() { this.parts = []; this.length = 0; }
  add(bytes) { this.parts.push(bytes); this.length += bytes.length; }
  u32(value) { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, value, true); this.add(b); }
  u64(value) { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0, BigInt.asUintN(64, BigInt(value)), true); this.add(b); }
  blob(bytes) { this.u32(bytes.length); this.add(bytes); }
  text(value) { this.blob(encoder.encode(value)); }
  finish() {
    const result = new Uint8Array(this.length); let offset = 0;
    for (const part of this.parts) { result.set(part, offset); offset += part.length; }
    return result;
  }
}

export function failure(error) {
  const out = new PacketWriter();
  const kind = error?.kind ?? (error instanceof RangeError ? 3 : 1);
  out.u32(kind);
  if (kind === 1) { out.u32(error?.ortCode ?? 1); out.text(String(error?.message ?? error)); }
  else if (kind === 4) {
    out.u64(error?.bytes ?? 0);
    const metadata = Array.isArray(error?.metadata) ? error.metadata : undefined;
    out.u32(metadata ? metadata.length : 0xffff_ffff);
    if (metadata) {
      for (const endpoint of metadata) {
        out.text(endpoint.name); out.u32(endpoint.dtype); out.u32(endpoint.shape.length);
        for (const dimension of endpoint.shape) out.u64(dimension);
      }
    }
  } else out.text(String(error?.message ?? error));
  return out.finish();
}

function provenance(message) { return Object.assign(new Error(message), { kind: 2 }); }

// Parse only regular members from the hash-admitted tar stream. Nothing is
// extracted to the filesystem, and no archive name ever becomes a URL.
function members(tar, wanted) {
  const result = new Map(); let offset = 0; let local = {}; let global = {}; let longName;
  const cstr = b => utf8.decode(b.subarray(0, b.indexOf(0) < 0 ? b.length : b.indexOf(0)));
  const octal = b => {
    const text = cstr(b).trim();
    if (!/^[0-7]+$/.test(text)) throw provenance("invalid pinned tar integer");
    const value = Number.parseInt(text, 8);
    if (!Number.isSafeInteger(value)) throw provenance("pinned tar length overflow");
    return value;
  };
  const pax = bytes => {
    const attrs = {}; let at = 0;
    while (at < bytes.length) {
      const space = bytes.indexOf(32, at);
      if (space < 0) throw provenance("invalid pinned tar PAX record");
      const count = Number(utf8.decode(bytes.subarray(at, space)));
      if (!Number.isSafeInteger(count) || count <= space - at + 1 || at + count > bytes.length) throw provenance("invalid pinned tar PAX length");
      const record = utf8.decode(bytes.subarray(space + 1, at + count - 1));
      const equal = record.indexOf("=");
      if (equal < 1 || bytes[at + count - 1] !== 10) throw provenance("invalid pinned tar PAX value");
      attrs[record.slice(0, equal)] = record.slice(equal + 1); at += count;
    }
    return attrs;
  };
  while (offset + 512 <= tar.length) {
    const h = tar.subarray(offset, offset + 512); offset += 512;
    if (h.every(b => b === 0)) break;
    let sum = 0;
    for (let i = 0; i < 512; i++) sum += i >= 148 && i < 156 ? 32 : h[i];
    if (sum !== octal(h.subarray(148, 156))) throw provenance("pinned tar checksum mismatch");
    const length = octal(h.subarray(124, 136));
    if (length > tar.length - offset) throw provenance("truncated pinned tar member");
    const data = tar.subarray(offset, offset + length); offset += Math.ceil(length / 512) * 512;
    const type = h[156];
    if (type === 120) { local = pax(data); continue; }
    if (type === 103) { global = { ...global, ...pax(data) }; continue; }
    if (type === 76) { longName = cstr(data); continue; }
    const prefix = cstr(h.subarray(345, 500));
    const name = local.path ?? global.path ?? longName ?? (prefix ? prefix + "/" : "") + cstr(h.subarray(0, 100));
    local = {}; longName = undefined;
    if (!wanted.has(name)) continue;
    if (type !== 0 && type !== 48) throw provenance("pinned runtime member is not a regular file");
    if (result.has(name)) throw provenance("duplicate pinned runtime member");
    result.set(name, data);
  }
  for (const name of wanted) if (!result.has(name)) throw provenance(`pinned runtime member is absent: ${name}`);
  return result;
}

async function admit(archive, pin) {
  if (!archive) throw provenance("approved ONNX Runtime Web archive bytes are absent");
  const bytes = await archive.arrayBuffer();
  const digest = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), b => b.toString(16).padStart(2, "0")).join("");
  if (digest !== pin.hash) throw provenance(`runtime archive hash is ${digest}, expected ${pin.hash}`);
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
  const tar = new Uint8Array(await new Response(stream).arrayBuffer());
  const files = members(tar, new Set(["package/package.json", pin.module, pin.wasm]));
  const manifest = JSON.parse(utf8.decode(files.get("package/package.json")));
  if (manifest.name !== "onnxruntime-web" || manifest.version !== pin.version || manifest.license !== pin.license) throw provenance("runtime archive package/version/license differs from the provider pin");
  return {
    module: new Blob([files.get(pin.module)], { type: "text/javascript" }),
    wasm: new Blob([files.get(pin.wasm)], { type: "application/wasm" }),
  };
}

export function createOnnxRuntimeWebHost(archiveBytes) {
  // Blob captures mutable caller buffers before any asynchronous hash check.
  const archive = archiveBytes == null ? null : archiveBytes instanceof Blob ? archiveBytes : new Blob([archiveBytes]);
  const sessions = new Map(); const jobs = new Map();
  let nextTransportJob = 0x80000000;
  let exports; let admitted; let admittedKey; let disposed = false;
  const kill = session => {
    session.worker?.terminate(); session.worker = undefined;
    session.reject?.(Object.assign(new Error("model operation cancelled"), { ortCode: 12 }));
    session.reject = undefined;
  };
  const release = id => {
    const session = sessions.get(id);
    if (!session) return;
    kill(session); sessions.delete(id);
  };
  const complete = (job, bytes) => {
    const pending = jobs.get(job);
    if (!pending) return;
    pending.signal?.removeEventListener("abort", pending.abort);
    if (pending.resolve) {
      // Direct transport callers receive the Rust packet unchanged. The
      // packet remains the provider's wire format; JavaScript only schedules
      // the worker operation and never interprets model policy.
      pending.done = true;
      pending.resolve(bytes);
      if (pending.polled) jobs.delete(job);
      return;
    }
    jobs.delete(job);
    const ptr = exports.jet_onnx_web_reply_alloc(job, bytes.length) >>> 0;
    if (ptr) new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes);
    exports.jet_onnx_web_reply_finish(job);
  };
  const rpc = (session, operation, data) => new Promise((resolve, reject) => {
    const worker = session.worker;
    session.reject = reject;
    worker.onmessage = event => {
      if (session.worker !== worker) return;
      session.reject = undefined; resolve(event.data);
    };
    worker.onerror = event => {
      event.preventDefault();
      if (session.worker !== worker) return;
      kill(session); reject(new Error(event.message || "ONNX worker failed"));
    };
    worker.onmessageerror = () => {
      if (session.worker !== worker) return;
      kill(session); reject(new Error("ONNX worker message could not be decoded"));
    };
    worker.postMessage({ operation, ...data });
  });
  const initialize = async session => {
    session.worker = new Worker(new URL("./OnnxRuntimeWebWorker.js", import.meta.url), { type: "module", name: "jet-onnx-cpu" });
    return rpc(session, "open", { runtime: session.runtime, graph: session.graph });
  };
  const execute = async (job, sessionId, operation, bytes) => {
    try {
      if (disposed) throw new Error("ONNX browser host has been disposed");
      let session;
      if (operation === 0) {
        const p = new PacketReader(bytes);
        const pin = { version: p.text(), license: p.text(), hash: p.text(), module: p.text(), wasm: p.text() };
        const record = p.text(); const graph = new Blob([p.blob()]); p.end();
        const key = JSON.stringify(pin);
        if (admittedKey !== undefined && key !== admittedKey) throw provenance("one browser host cannot change runtime pins");
        if (!admitted) { admittedKey = key; admitted = admit(archive, pin); }
        const runtime = await admitted;
        if (!jobs.has(job)) return;
        if (sessions.has(sessionId)) throw new Error("duplicate model session handle");
        session = { runtime, graph, record };
        sessions.set(sessionId, session);
        const result = await initialize(session);
        if (result[0] !== 0) release(sessionId);
        complete(job, result);
      } else if (operation === 1) {
        session = sessions.get(sessionId);
        if (!session) throw new Error("model session is closed");
        // A cancelled worker is destroyed, never reused while ORT is running.
        // A subsequent call reopens the exact same checked graph/runtime.
        if (!session.worker) {
          const opened = await initialize(session);
          if (!jobs.has(job)) return;
          if (opened[0] !== 0) { kill(session); complete(job, opened); return; }
        }
        complete(job, await rpc(session, "run", { packet: bytes }));
      } else throw new Error("unknown model host operation");
    } catch (error) {
      if (operation === 0) release(sessionId);
      complete(job, failure(error));
    }
  };
  const transport = {
    start(session, operation, bytes, options = {}) {
      if (!(bytes instanceof Uint8Array)) bytes = new Uint8Array(bytes);
      const job = nextTransportJob = (0x80000000 | ((nextTransportJob + 1) & 0x7fff_ffff)) >>> 0;
      let resolve;
      let reject;
      const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
      const signal = options?.signal;
      const abort = () => transport.cancel(job);
      jobs.set(job, { session, operation, resolve, reject, promise, signal, abort, polled: false, done: false });
      if (signal?.aborted) {
        abort();
        return job;
      }
      signal?.addEventListener("abort", abort, { once: true });
      try {
        void execute(job, session, operation, bytes);
      } catch (error) {
        queueMicrotask(() => {
          const pending = jobs.get(job);
          if (!pending) return;
          jobs.delete(job);
          pending.signal?.removeEventListener("abort", pending.abort);
          pending.reject(error);
        });
      }
      return job;
    },
    poll(job) {
      const pending = jobs.get(job);
      if (!pending?.promise) return Promise.reject(new Error("unknown model transport job"));
      pending.polled = true;
      if (pending.done) jobs.delete(job);
      return pending.promise;
    },
    // A resumed Rust future observes completion through the same Promise.
    // Keeping resume separate makes the start/poll/resume protocol explicit
    // without moving packet interpretation or model policy into JavaScript.
    resume(job) {
      return this.poll(job);
    },
    request(session, operation, bytes, options = {}) {
      return this.poll(this.start(session, operation, bytes, options));
    },
    cancel(job) {
      const pending = jobs.get(job);
      if (!pending) return;
      jobs.delete(job);
      pending.signal?.removeEventListener("abort", pending.abort);
      const session = sessions.get(pending.session);
      if (pending.operation === 0) release(pending.session);
      else if (session) kill(session);
      pending.reject?.(Object.assign(new Error("model operation cancelled"), { ortCode: 12 }));
    },
  };
  return {
    imports: { jet_onnx_web: {
      start(job, session, operation, ptr, len) {
        // Imports must not throw through Rust. Malformed packets become typed
        // model replies on the next microtask, just like runtime failures.
        jobs.set(job, { session, operation });
        try {
          const bytes = new Uint8Array(exports.memory.buffer, ptr >>> 0, len >>> 0).slice();
          void execute(job, session, operation, bytes);
        } catch (error) { queueMicrotask(() => complete(job, failure(error))); }
      },
      cancel(job) {
        const pending = jobs.get(job); if (!pending) return;
        jobs.delete(job);
        const session = sessions.get(pending.session);
        if (pending.operation === 0) release(pending.session);
        else if (session) kill(session);
      },
      release,
      now() { return performance.now() * 1e6; },
    } },
    transport,
    bind(instanceExports) { exports = instanceExports; },
    dispose() {
      disposed = true;
      for (const [job, pending] of jobs) {
        if (pending.reject) pending.reject(Object.assign(new Error("ONNX browser host disposed"), { ortCode: 12 }));
        else complete(job, failure(new Error("ONNX browser host disposed")));
      }
      for (const id of sessions.keys()) release(id);
    },
  };
}

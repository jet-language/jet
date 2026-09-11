// D-DOMGEN1=A (c123 M2): first-party DOM/runtime shim for generated web JS.
// Generated `app.js` imports these primitives; the loader wires WASM exports.

export function print(...values) {
  const texts = values.map((value) => String(value));
  if (typeof console !== "undefined" && console.log) {
    for (const text of texts) {
      console.log(text);
    }
  }
  return texts.length ? texts[texts.length - 1] : "";
}

// Host failures stay distinct from Jet program reports at the edge.
class JetHostWasmError extends Error {
  constructor(message, cause) {
    super(message);
    this.name = "JetHostWasmError";
    if (cause !== undefined) this.cause = cause;
  }
}

class JetHostAbiError extends Error {
  constructor(message, cause) {
    super(message);
    this.name = "JetHostAbiError";
    if (cause !== undefined) this.cause = cause;
  }
}
// Native JSON.parse supplies the original numeric token through the reviver
// context. Keep safely representable integers as Number; only an integer that
// rounds outside the safe range becomes an exact BigInt. Decimal, exponent,
// and negative-zero tokens retain native Number semantics.
const JET_WEB_JSON_INTEGER = /^-?(?:0|[1-9]\d*)$/;

function jet_web_json_parse(text) {
  return JSON.parse(text, (_key, value, context) => {
    if (typeof value !== "number") return value;
    const source = context?.source;
    if (typeof source !== "string") {
      throw new TypeError("canonical Web JSON parsing requires numeric-token context");
    }
    if (!JET_WEB_JSON_INTEGER.test(source) || Number.isSafeInteger(value)) return value;
    return BigInt(source);
  });
}

if (typeof globalThis !== "undefined") {
  globalThis.__jetWebJsonParse = jet_web_json_parse;
}

// Target identity is part of the executable artifact, not optional metadata.
// The generated JS calls this before invoking any Wasm export and treats a
// missing identity exactly like a mismatched one.
export function targetBindingError(expected, actual, cause) {
  const expectedArtifact = String(expected?.artifactIdentity ?? "<missing>");
  const actualArtifact = String(actual?.artifactIdentity ?? "<missing>");
  const expectedDossier = String(expected?.dossierIdentity ?? "<missing>");
  const actualDossier = String(actual?.dossierIdentity ?? "<missing>");
  return new JetHostWasmError(
    `WebAssembly target binding mismatch (artifact expected ${expectedArtifact}, received ${actualArtifact}; dossier expected ${expectedDossier}, received ${actualDossier})`,
    cause,
  );
}

// D-FAIL-EDGE1=A: every browser-visible failure keeps the report frame from
// the edge object. The overlay uses textContent so report punctuation and
// source lines are not interpreted as markup.
export function showRuntimeError(error) {
  const frame = String(error?.frame ?? error?.report?.frame ?? error?.message ?? error);
  if (error && typeof error === "object") {
    if (error.__jetOverlayShown) return frame;
    error.__jetOverlayShown = true;
  }
  if (typeof document === "undefined" || !document.body) {
    if (typeof console !== "undefined" && console.error) console.error(frame);
    return frame;
  }
  let overlay = document.getElementById("jet-runtime-overlay");
  if (!overlay) {
    overlay = document.createElement("div");
    overlay.id = "jet-runtime-overlay";
    overlay.style.position = "fixed";
    overlay.style.inset = "16px";
    overlay.style.zIndex = "2147483647";
    overlay.style.padding = "20px";
    overlay.style.overflow = "auto";
    overlay.style.background = "#171923";
    overlay.style.color = "#f7f7fb";
    overlay.style.border = "1px solid #e05252";
    overlay.style.borderRadius = "10px";
    overlay.style.boxShadow = "0 12px 48px rgba(0,0,0,.45)";
    overlay.style.font = "14px ui-monospace, SFMono-Regular, monospace";
    document.body.appendChild(overlay);
  }
  overlay.textContent = frame;
  overlay.hidden = false;
  return frame;
}

const jetErrorPageSensitive = /authorization|proxy-authorization|set-cookie|cookie|password|passwd|secret|token|api[_-]?key|private[_-]?key|credential|session|jwt|bearer|access[_-]?key|refresh[_-]?token|client[_-]?secret|signature|unpublished/i;

function jetErrorPageText(value, limit = 2048, fallback = "") {
  const text = String(value ?? "").replace(/[\u0000-\u001f\u007f]/g, " ").trim();
  if (!text || jetErrorPageSensitive.test(text)) return fallback;
  return text.slice(0, limit);
}

function jetErrorPagePath(value) {
  return String(value ?? "").replace(/[\u0000-\u001f\u007f]/g, " ").trim().slice(0, 512);
}

function jetErrorPageStatusText(status) {
  const names = {
    400: "Bad Request",
    401: "Unauthorized",
    403: "Forbidden",
    404: "Not Found",
    405: "Method Not Allowed",
    408: "Request Timeout",
    409: "Conflict",
    413: "Payload Too Large",
    415: "Unsupported Media Type",
    422: "Unprocessable Content",
    429: "Too Many Requests",
    500: "Internal Server Error",
    501: "Not Implemented",
    502: "Bad Gateway",
    503: "Service Unavailable",
    504: "Gateway Timeout",
  };
  return names[status] ?? (status >= 400 && status < 500 ? "Client Error" : "Server Error");
}

function jetErrorPageLink(value) {
  const link = String(value ?? "").trim().slice(0, 512);
  if (
    !link ||
    jetErrorPageSensitive.test(link) ||
    !(/^\//.test(link) || /^https?:\/\//i.test(link) || /^jet:\/\//i.test(link))
  ) {
    return null;
  }
  return link;
}

function jetErrorPageNode(tag, text, className) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  node.textContent = text;
  return node;
}

// D-FAIL-EDGE1=A: the browser consumes the same `jet.err/v1` projection as
// other hosts. Every dynamic field uses textContent; links are scheme-limited
// before they become attributes, so report content cannot become markup/script.
export function showErrorPage(error) {
  const page = error && typeof error === "object" && error.schema === "jet.err/v1" ? error : {};
  const status = Number.isInteger(Number(page.status)) ? Number(page.status) : 500;
  const statusText = `${status} ${jetErrorPageStatusText(status)}`;
  const requestId = jetErrorPageText(page.request_id, 128, "request-unknown");
  const failureId = jetErrorPageText(page.failure_id, 128, "failure-unknown");
  const message = jetErrorPageText(page.message, 2048, jetErrorPageStatusText(status));
  if (typeof document === "undefined" || !document.body) {
    if (typeof console !== "undefined" && console.error) console.error(message);
    return message;
  }

  let pageNode = document.getElementById("jet-error-page");
  if (!pageNode) {
    pageNode = document.createElement("section");
    pageNode.id = "jet-error-page";
    pageNode.setAttribute("role", "alert");
    pageNode.style.position = "fixed";
    pageNode.style.inset = "16px";
    pageNode.style.zIndex = "2147483647";
    pageNode.style.padding = "20px";
    pageNode.style.overflow = "auto";
    pageNode.style.background = "#101418";
    pageNode.style.color = "#e8edf2";
    pageNode.style.border = "1px solid #e05252";
    pageNode.style.borderRadius = "10px";
    pageNode.style.boxShadow = "0 12px 48px rgba(0,0,0,.45)";
    pageNode.style.font = "16px/1.5 system-ui,sans-serif";
    document.body.appendChild(pageNode);
  }
  while (pageNode.firstChild) pageNode.removeChild(pageNode.firstChild);

  pageNode.appendChild(jetErrorPageNode("div", "Jet service failure", "jet-error-page-kind"));
  pageNode.appendChild(jetErrorPageNode("h1", statusText));
  const details = document.createElement("dl");
  details.appendChild(jetErrorPageNode("dt", "Request ID"));
  details.appendChild(jetErrorPageNode("dd", requestId));
  details.appendChild(jetErrorPageNode("dt", "Failure ID"));
  details.appendChild(jetErrorPageNode("dd", failureId));
  pageNode.appendChild(details);
  pageNode.appendChild(jetErrorPageNode("h2", "Message"));
  pageNode.appendChild(jetErrorPageNode("p", message));

  const code = jetErrorPageText(page.code, 128);
  if (code) {
    const codeLine = document.createElement("p");
    codeLine.appendChild(jetErrorPageNode("strong", "Code: "));
    codeLine.appendChild(jetErrorPageNode("code", code));
    pageNode.appendChild(codeLine);
  }

  const source = page.source_frame && typeof page.source_frame === "object" ? page.source_frame : null;
  if (source) {
    const sourceLine = document.createElement("p");
    const sourceCode = document.createElement("code");
    const fnName = jetErrorPageText(source.fn_name ?? source.function, 512);
    const file = jetErrorPagePath(source.file);
    sourceCode.textContent = `${fnName ? `${fnName} (` : ""}${file}:${Number(source.line) || 0}${fnName ? ")" : ""}`;
    sourceLine.appendChild(sourceCode);
    const note = jetErrorPageText(source.note, 2048);
    if (note) {
      sourceLine.appendChild(document.createTextNode(` — ${note}`));
    }
    pageNode.appendChild(jetErrorPageNode("h2", "Source frame"));
    pageNode.appendChild(sourceLine);
  }

  const context = Array.isArray(page.context) ? page.context.slice(0, 16) : [];
  if (context.length) {
    const list = document.createElement("ul");
    for (const frame of context) {
      if (!frame || typeof frame !== "object") continue;
      const item = document.createElement("li");
      const location = jetErrorPageNode("code", `${jetErrorPagePath(frame.file)}:${Number(frame.line) || 0}`);
      item.appendChild(location);
      item.appendChild(document.createTextNode(` ${jetErrorPageText(frame.text, 2048, "[redacted]")}`));
      list.appendChild(item);
    }
    if (list.childNodes.length) {
      pageNode.appendChild(jetErrorPageNode("h2", "Context"));
      pageNode.appendChild(list);
    }
  }

  const links = Array.isArray(page.correlation_links) ? page.correlation_links.slice(0, 8) : [];
  const safeLinks = links.map(jetErrorPageLink).filter(Boolean);
  if (safeLinks.length) {
    const list = document.createElement("ul");
    for (const link of safeLinks) {
      const item = document.createElement("li");
      const anchor = document.createElement("a");
      anchor.setAttribute("href", link);
      anchor.textContent = link;
      item.appendChild(anchor);
      list.appendChild(item);
    }
    pageNode.appendChild(jetErrorPageNode("h2", "Correlation"));
    pageNode.appendChild(list);
  }
  pageNode.hidden = false;
  return pageNode;
}


export function raiseRuntimeError(error) {
  showRuntimeError(error);
  throw error;
}

// D-FAIL-EDGE1=A: Wasm exposes one host-readable JSON value. The host copies
// it before clearing the slot; no JS runtime target guess chooses the edge.
export function takeWasmError(wasm) {
  const length = Number(wasm?.jet_wasm_error_len?.() ?? 0);
  if (!length) return null;
  const status = Number(wasm?.jet_wasm_error_status?.() ?? 0);
  const ptr = Number(wasm.jet_wasm_error_ptr?.() ?? 0);
  const bytes = new Uint8Array(wasm.memory.buffer, ptr, length).slice();
  const frame = new TextDecoder().decode(bytes);
  wasm.jet_wasm_error_clear?.();
  try {
    return globalThis.__jetWebJsonParse(frame);
  } catch (_) {
    return {
      tag: "Host",
      status,
      error: {
        code: "__malformed_wasm_error__",
        message: "Wasm host returned malformed error JSON",
      },
      report: frame,
    };
  }
}

// D-WEB-SWAP1=A: the page is the only producer of live DOM state. The host
// receives typed facts and a compiler-published module registry; it never
// manufactures an empty web-v1 module or copies arbitrary page objects.
const jetWebModules = new Map();
const jetWebState = new Map();

function jetWebText(value, fallback = "") {
  const text = String(value ?? fallback);
  return text.length > 2048 ? text.slice(0, 2048) : text;
}

export function registerWebModule(identity, interfaceFingerprint, bodyFingerprint) {
  const key = jetWebText(identity);
  if (!key) return;
  jetWebModules.set(key, {
    identity: key,
    interface_fingerprint: jetWebText(interfaceFingerprint),
    body_fingerprint: jetWebText(bodyFingerprint),
  });
}

export function registerWebState(kind, key, typeFingerprint, value, identity = "") {
  const stateKind = jetWebText(kind);
  const stateKey = jetWebText(key);
  const stateType = jetWebText(typeFingerprint);
  if (!stateKind || !stateKey || !stateType) return;
  jetWebState.set(`${stateKind}\0${stateKey}`, {
    kind: stateKind,
    key: stateKey,
    type_fingerprint: stateType,
    identity: jetWebText(identity),
    value,
  });
}
// D-DX-LIVE1: persist slots cross the browser boundary as raw facts. The host
// applies the shared Observe policy after decoding; this projection does not
// assign disposition/reason or redact the rendered payload.
export function publishLiveValueUpdate(key, typeIdentity, rendered) {
  const valueKey = jetWebText(key);
  const identity = jetWebText(typeIdentity);
  if (!valueKey || !identity) return;
  jetWebState.set(`persist_live_value\0${valueKey}`, {
    kind: "persist_live_value",
    key: valueKey,
    type_identity: identity,
    rendered_value: rendered === null || rendered === undefined
      ? null
      : jetWebText(rendered),
  });
}


function jetWebDomFact(kind, key, typeFingerprint, value, identity = "") {
  return {
    kind,
    key: jetWebText(key),
    type_fingerprint: jetWebText(typeFingerprint),
    ...(identity ? { identity: jetWebText(identity) } : {}),
    value,
  };
}

export function captureWebSwapSnapshot() {
  const stateByKey = new Map(
    [...jetWebState.values()].map((fact) => [`${fact.kind}\0${fact.key}`, { ...fact }]),
  );
  const setState = (fact) => {
    const stateKey = `${fact.kind}\0${fact.key}`;
    stateByKey.set(stateKey, fact);
    jetWebState.set(stateKey, fact);
  };
  if (typeof document !== "undefined") {
    for (const element of document.querySelectorAll("input,textarea,select")) {
      const key = element.id || element.name;
      if (!key) continue;
      setState(jetWebDomFact(
        "form_draft",
        key,
        `dom:${element.tagName.toLowerCase()}:value`,
        { kind: "text", value: jetWebText(element.value) },
      ));
    }
    const active = document.activeElement;
    const focusKey = active?.dataset?.jetKey || active?.id || active?.name;
    if (focusKey) {
      setState(jetWebDomFact(
        "focus",
        focusKey,
        `dom:${active.tagName.toLowerCase()}:focus`,
        { kind: "text", value: "focused" },
      ));
    }
  }
  if (typeof window !== "undefined") {
    setState(jetWebDomFact(
      "scroll",
      "window",
      "dom:window:scroll",
      { kind: "integer", value: Math.trunc(Number(window.scrollY || 0)) },
    ));
  }
  return {
    protocol: "jet.devtools.v1",
    kind: "web.dom-snapshot",
    modules: [...jetWebModules.values()],
    state: [...stateByKey.values()],
  };
}

function jetWebFindElement(key) {
  if (typeof document === "undefined") return null;
  const text = String(key);
  const byId = document.getElementById(text);
  if (byId) return byId;
  for (const element of document.querySelectorAll("[data-jet-key],[name]")) {
    if (element.dataset?.jetKey === text || element.name === text) return element;
  }
  return null;
}

export function applyWebSwapReceipt(swap) {
  const receipt = swap?.last_receipt;
  if (!receipt) return;
  for (const fact of receipt.preserved ?? []) {
    if (fact.kind === "form_draft") {
      const element = jetWebFindElement(fact.key);
      if (element && "value" in element) {
        const saved = jetWebState.get(`form_draft\0${fact.key}`);
        if (saved?.value?.kind === "text") element.value = saved.value.value;
      }
    } else if (fact.kind === "focus") {
      jetWebFindElement(fact.key)?.focus?.();
    } else if (fact.kind === "scroll" && fact.key === "window" && typeof window !== "undefined") {
      const saved = jetWebState.get("scroll\0window");
      if (saved?.value?.kind === "integer") window.scrollTo(0, saved.value.value);
    }
  }
  globalThis.dispatchEvent?.(new CustomEvent("jet:web-swap-applied", { detail: receipt }));
}

if (typeof globalThis !== "undefined") {
  globalThis.__jetWebSwapSnapshot = captureWebSwapSnapshot;
  globalThis.addEventListener?.("jet:web-swap", (event) => applyWebSwapReceipt(event.detail));
}

// Whole-number Jet values are BigInt on the JS tier (#1485). DOM layout math
// and CSS pixel lengths still need ordinary numbers (safe for UI sizes).
function jetNum(value) {
  return typeof value === "bigint" ? Number(value) : value;
}

// D-DOMGEN1=A (Phase 7 extension): when a real `document` is available (the
// generated app is running in a browser, not under `node`'s golden/roundtrip
// tests), the DOM backend also mounts real elements under a fixed-id
// container so `jet build --target web` output is an actually-viewable page,
// not just an in-memory command log. Under `node` (no `document`), this is a
// no-op and behavior is byte-identical to before — the existing snapshot
// tests never touch a browser.
function jetDomContainer() {
  if (typeof document === "undefined") return null;
  let el = document.getElementById("jet-app");
  if (!el) {
    el = document.createElement("div");
    el.id = "jet-app";
    el.style.position = "relative";
    document.body.appendChild(el);
  }
  return el;
}

// D-DX-WEBPENDING1 / cards #2427/#2478: generated route facts are the only
// source for boundary attachment.  The DOM host projects their nested
// `render_facts` without deriving a mode or copying a second route table.
function jetWebRouteFacts(route) {
  if (!route || typeof route !== "object") {
    throw new TypeError("web route facts must be an object");
  }
  for (const field of ["path", "handler", "provenance"]) {
    if (typeof route[field] !== "string" || route[field].length === 0) {
      throw new TypeError(`web route ${field} must be a non-empty string`);
    }
  }
  const facts = route.render_facts;
  if (!facts || typeof facts !== "object") {
    throw new TypeError("web route is missing nested render_facts");
  }
  for (const [name, boundary] of [
    ["pending_boundary_id", facts.pending_boundary_id],
    ["error_boundary_id", facts.error_boundary_id],
  ]) {
    if (
      boundary !== null
      && boundary !== undefined
      && (!Number.isSafeInteger(Number(boundary)) || Number(boundary) < 0)
    ) {
      throw new TypeError(`web route ${name} must be a non-negative safe integer or null`);
    }
  }
  const island = facts.island;
  if (island !== null && island !== undefined && typeof island !== "object") {
    throw new TypeError("web route island facts must be an object or null");
  }
  const trigger = island?.hydration_trigger ?? null;
  if (
    trigger !== null
    && !["immediate", "client-load", "client-idle", "client-visible", "manual"].includes(String(trigger))
  ) {
    throw new TypeError(`web route hydration trigger ${String(trigger)} is not recognized`);
  }
  return facts;
}

function jetWebRouteBoundaryId(route, status = "pending") {
  const facts = jetWebRouteFacts(route);
  const boundary = status === "error"
    ? facts.error_boundary_id ?? facts.pending_boundary_id
    : facts.pending_boundary_id;
  return boundary === null || boundary === undefined ? null : String(Number(boundary));
}

function jetWebRouteContainer(container) {
  if (container && typeof container.appendChild === "function") return container;
  if (typeof container === "string") return query(container);
  return jetDomContainer();
}

export function projectWebRoute(route, navigationState = null) {
  const facts = jetWebRouteFacts(route);
  const status = String(navigationState?.status ?? "ready").toLowerCase();
  if (!["pending", "ready", "error"].includes(status)) {
    throw new TypeError(`web route navigation status ${status} is not renderable`);
  }
  const boundaryId = jetWebRouteBoundaryId(route, status);
  const content = navigationState?.data;
  return {
    handler: route.handler,
    provenance: route.provenance,
    render_facts: facts,
    boundary: boundaryId === null
      ? null
      : {
        boundary_id: boundaryId,
        state: status,
        hydration_trigger: facts.island?.hydration_trigger ?? "immediate",
        content: typeof content === "string" ? content : String(content ?? ""),
      },
  };
}

export function attachWebRouteBoundary(container, route, navigationState = null) {
  const projection = projectWebRoute(route, navigationState);
  if (!projection.boundary) return projection;
  const root = jetWebRouteContainer(container);
  if (!root) return projection;
  let boundary = [...(root.children ?? [])].find(
    (child) => child.dataset?.jetBoundaryId === projection.boundary.boundary_id,
  );
  if (!boundary) {
    boundary = document.createElement("section");
    boundary.dataset.jetBoundaryId = projection.boundary.boundary_id;
    root.appendChild(boundary);
  }
  boundary.dataset.jetBoundaryState = projection.boundary.state;
  boundary.dataset.jetHydrationTrigger = projection.boundary.hydration_trigger;
  boundary.setAttribute("aria-busy", projection.boundary.state === "pending" ? "true" : "false");
  boundary.textContent = projection.boundary.content;
  return projection;
}

function jetWebRouteStreamId(route, streamId) {
  const boundaryId = jetWebRouteBoundaryId(route);
  const checked = jetWebStreamId(streamId);
  if (boundaryId !== null && checked.boundary_id !== boundaryId) {
    throw new TypeError("web route stream boundary does not match render_facts");
  }
  return checked;
}

export function resumeWebRouteHydration(route, streamId, startedAtNs) {
  const facts = jetWebRouteFacts(route);
  const checked = jetWebRouteStreamId(route, streamId);
  const trigger = facts.island?.hydration_trigger ?? "immediate";
  return webStreamHydrationStart(checked, trigger, startedAtNs);
}

export function completeWebRouteHydration(route, streamId, completedAtNs) {
  const checked = jetWebRouteStreamId(route, streamId);
  return webStreamHydrationComplete(checked, completedAtNs);
}
export async function resumeWebRouteStream(container, route) {
  const boundaryId = jetWebRouteBoundaryId(route);
  if (boundaryId === null) {
    throw new TypeError("web route has no pending boundary to resume");
  }
  const body = await webStreamReceipt(boundaryId);
  const receipt = body?.receipt;
  if (!receipt || typeof receipt !== "object") {
    throw new TypeError("web stream receipt is missing from host response");
  }
  const state = receipt.state === "failed" ? "error" : String(receipt.state);
  return attachWebRouteBoundary(container, route, {
    status: state,
    data: receipt?.state === "pending"
      ? receipt?.stream_content ?? ""
      : receipt?.content ?? "",
  });
}

if (typeof globalThis !== "undefined") {
  globalThis.__jetProjectWebRoute = projectWebRoute;
  globalThis.__jetAttachWebRouteBoundary = attachWebRouteBoundary;
  globalThis.__jetResumeWebRouteStream = resumeWebRouteStream;
  globalThis.addEventListener?.("jet:web-route", (event) => {
    const detail = event.detail ?? {};
    attachWebRouteBoundary(detail.container, detail.route, detail.navigation);
  });
}

// D-DX-ROUTER1 / D-WEBAPP1: the browser consumes the sema-owned App graph and
// the native server's HTML/data response.  It never matches a route, decodes a
// typed parameter, or owns a loader cache in JavaScript.  Navigation is a
// transport adapter around the one checked server graph; the graph remains the
// authority for precedence, codecs, loaders, and boundaries.
function jetWebAppBuilder() {
  return {
    __jetWebAppBuilder: true,
    mode: "csr",
    hydration: "dev-overlay",
    policy: {
      security: [],
      assets: [],
      split: [],
      cache: [],
      a11y: [],
      adapters: [],
    },
    route_source: null,
  };
}

function jetWebAppBuilderCheck(app, operation) {
  if (!app || app.__jetWebAppBuilder !== true) {
    throw new TypeError(`${operation} requires a web App builder`);
  }
  return app;
}


function jetWebAppCallable(value, operation) {
  if (typeof value !== "function") {
    throw new TypeError(`${operation} requires a checked handler callback`);
  }
  return value;
}

/** JS-side opaque value used while `jet_main` evaluates the checked App chain. */
export function jetApp() {
  return jetWebAppBuilder();
}

function jetWebAppRegisterRoute(app, path, handler, binding, kind) {
  const checked = jetWebAppBuilderCheck(app, `App.${kind}`);
  if (typeof path !== "string" || path.length === 0) {
    throw new TypeError(`App.${kind} requires a non-empty route path`);
  }
  jetWebAppCallable(handler, `App.${kind}`);
  if (binding !== undefined && binding !== null && typeof binding !== "string") {
    throw new TypeError(`App.${kind} requires a checked binding descriptor`);
  }
  return checked;
}

export function jetAppRoute(app, path, handler, binding) {
  return jetWebAppRegisterRoute(app, path, handler, binding, "route");
}
export function jetAppPage(app, path, handler, binding) {
  return jetWebAppRegisterRoute(app, path, handler, binding, "page");
}


function jetWebAppLoader(app, pathHandler, loader, binding, _preload) {
  const checked = jetWebAppBuilderCheck(app, "App.loader");
  if (pathHandler !== undefined && pathHandler !== null) {
    jetWebAppCallable(pathHandler, "App.loader");
  }
  jetWebAppCallable(loader, "App.loader");
  if (binding !== undefined && binding !== null && typeof binding !== "string") {
    throw new TypeError("App.loader requires a checked binding descriptor");
  }
  return checked;
}

export function jetAppLoader(app, pathHandler, loader, binding) {
  return jetWebAppLoader(app, pathHandler, loader, binding, false);
}
export function jetAppLoaderPreload(app, pathHandler, loader, preload, binding) {
  return jetWebAppLoader(app, pathHandler, loader, binding, preload);
}

function jetWebAppBoundary(app, name, handler) {
  const checked = jetWebAppBuilderCheck(app, `App.${name}`);
  jetWebAppCallable(handler, `App.${name}`);
  return checked;
}
export function jetAppPending(app, handler) {
  return jetWebAppBoundary(app, "pending", handler);
}
export function jetAppNotFound(app, handler) {
  return jetWebAppBoundary(app, "not_found", handler);
}
export function jetAppError(app, handler) {
  return jetWebAppBoundary(app, "error", handler);
}

function jetWebAppServerBoundary(app, name, action, handler) {
  const checked = jetWebAppBuilderCheck(app, `App.${name}`);
  if (typeof action !== "string" || action.length === 0) {
    throw new TypeError(`App.${name} requires a non-empty action name`);
  }
  jetWebAppCallable(handler, `App.${name}`);
  return checked;
}
export function jetAppAction(app, action, handler) {
  return jetWebAppServerBoundary(app, "action", action, handler);
}
export function jetAppForm(app, action, handler) {
  return jetWebAppServerBoundary(app, "form", action, handler);
}
export function jetAppData(app, action, handler) {
  return jetWebAppServerBoundary(app, "data", action, handler);
}

function jetWebAppMount(app, prefix, handler, _effects = null, _security = null) {
  const checked = jetWebAppBuilderCheck(app, "App.mount");
  if (typeof prefix !== "string" || prefix.length === 0) {
    throw new TypeError("App.mount requires a non-empty prefix");
  }
  jetWebAppCallable(handler, "App.mount");
  return checked;
}
export function jetAppMount(app, prefix, handler) {
  return jetWebAppMount(app, prefix, handler);
}
export function jetAppMountWithEffect(app, prefix, handler, effects) {
  return jetWebAppMount(app, prefix, handler, effects);
}
export function jetAppMountWithPolicy(app, prefix, handler, effects, security) {
  return jetWebAppMount(app, prefix, handler, effects, security);
}

function jetWebAppPolicy(app, field, value) {
  const checked = jetWebAppBuilderCheck(app, `App.${field}`);
  checked.policy[field].push(String(value));
  return checked;
}
export function jetAppRoutes(app, source) {
  const checked = jetWebAppBuilderCheck(app, "App.routes");
  checked.route_source = String(source);
  return checked;
}
export function jetAppSecurity(app, value) {
  return jetWebAppPolicy(app, "security", value);
}
export function jetAppAssets(app, value) {
  return jetWebAppPolicy(app, "assets", value);
}
export function jetAppSplit(app, value) {
  return jetWebAppPolicy(app, "split", value);
}
export function jetAppCodeSplit(app, value) {
  return jetAppPolicy(app, "split", value);
}
export function jetAppCache(app, value) {
  return jetWebAppPolicy(app, "cache", value);
}
export function jetAppA11y(app, value) {
  return jetWebAppPolicy(app, "a11y", value);
}
export function jetAppAdapter(app, value) {
  return jetWebAppPolicy(app, "adapters", value);
}

function jetWebAppMode(app, mode) {
  const checked = jetWebAppBuilderCheck(app, `App.${mode}`);
  checked.mode = mode;
  return checked;
}
export function jetAppCsr(app) {
  return jetWebAppMode(app, "csr");
}
export function jetAppSsr(app) {
  return jetWebAppMode(app, "ssr");
}
export function jetAppSsg(app) {
  return jetWebAppMode(app, "ssg");
}
export function jetAppStream(app) {
  return jetWebAppMode(app, "stream");
}
export function jetAppStreaming(app) {
  return jetWebAppMode(app, "stream");
}
export function jetAppHydrationDev(app) {
  const checked = jetWebAppBuilderCheck(app, "App.hydration_dev");
  checked.hydration = "dev-overlay";
  return checked;
}
export function jetAppHydrationRelease(app) {
  const checked = jetWebAppBuilderCheck(app, "App.hydration_release");
  checked.hydration = "release-keep-server";
  return checked;
}

export function jetAppFactsJson(app) {
  const checked = jetWebAppBuilderCheck(app, "App.facts_json");
  const graph = globalThis.__jetAppGraph;
  const routes = Array.isArray(graph?.routes) ? graph.routes : [];
  const actions = Array.isArray(graph?.actions) ? graph.actions : [];
  const queries = Array.isArray(graph?.queries) ? graph.queries : [];
  return jetWebJsonStringify({
    hydration: checked.hydration,
    shared_tir: true,
    routes,
    actions,
    queries,
    security: checked.policy.security,
    assets: checked.policy.assets,
    split: checked.policy.split,
    cache: checked.policy.cache,
    a11y: checked.policy.a11y,
    adapters: checked.policy.adapters,
  });
}

// Serving is a native-host concern.  Keeping these links total lets the same
// checked App expression be emitted for Web without pretending that a browser
// can open a listening socket.
export function jetAppServe(app) {
  return jetWebAppBuilderCheck(app, "App.serve");
}
export function jetAppServeWithPort(app, _port) {
  return jetWebAppBuilderCheck(app, "App.serve");
}
export function jetAppServeOn(app, _host) {
  return jetWebAppBuilderCheck(app, "App.serve_on");
}

function jetWebNavigationEvent(type, detail) {
  if (typeof globalThis === "undefined" || typeof globalThis.dispatchEvent !== "function") return;
  const event = typeof globalThis.CustomEvent === "function"
    ? new globalThis.CustomEvent(type, { detail })
    : { type, detail };
  globalThis.dispatchEvent(event);
}

function jetWebNavigationStatus(status, fallback = "idle") {
  const normalized = String(status ?? fallback).toLowerCase();
  return ["idle", "preloading", "pending", "ready", "error", "aborted"].includes(normalized)
    ? normalized
    : fallback;
}

function jetWebRouteDataScript(doc) {
  if (!doc) return null;
  const script = doc.getElementById?.("jet-route-data")
    ?? doc.querySelector?.('script[type="application/json"]#jet-route-data')
    ?? null;
  if (!script) return null;
  const tagName = String(script.tagName ?? "").toLowerCase();
  const type = String(script.type ?? script.getAttribute?.("type") ?? "").toLowerCase();
  if (tagName && tagName !== "script") {
    throw new TypeError("jet-route-data must be an inert application/json script");
  }
  if (type && type !== "application/json") {
    throw new TypeError("jet-route-data must be an inert application/json script");
  }
  return script;
}

/**
 * Read the server's route payload using textContent.  The raw wire is retained
 * beside the parsed value so hosts never have to stringify and re-marshal
 * numeric data merely to inspect or hand it to a canonical decoder.
 */
export function readWebRouteData(doc = globalThis.document) {
  const script = jetWebRouteDataScript(doc);
  if (!script) return null;
  const route = String(script.dataset?.route ?? script.getAttribute?.("data-route") ?? "");
  if (route.length === 0) throw new TypeError("jet-route-data is missing data-route");
  const url = String(script.dataset?.url ?? script.getAttribute?.("data-url") ?? "");
  const status = jetWebNavigationStatus(
    script.dataset?.status ?? script.getAttribute?.("data-status") ?? "ready",
    "ready",
  );
  const data_wire = String(script.textContent ?? "");
  let data = null;
  if (data_wire.trim() !== "") {
    try {
      data = globalThis.__jetWebJsonParse(data_wire);
    } catch (cause) {
      const error = new TypeError("server route data is not valid JSON");
      error.cause = cause;
      throw error;
    }
  }
  return { route, url, status, data, data_wire };
}

function jetWebGraphInput(input) {
  const graph = input && Object.prototype.hasOwnProperty.call(input, "graph")
    ? input.graph
    : input ?? null;
  if (graph === null) {
    return { graph: null, routes: [], actions: [] };
  }
  if (typeof graph !== "object" || !Array.isArray(graph.routes)) {
    throw new TypeError("web bootstrap requires the checked App graph");
  }
  const routes = graph.routes;
  for (const route of routes) {
    jetWebRouteFacts(route);
    if (typeof route.path !== "string" || route.path.length === 0) {
      throw new TypeError("checked web graph route path must be non-empty");
    }
  }
  const actions = Array.isArray(graph.actions) ? graph.actions : [];
  return { graph, routes, actions };
}

function jetWebGraphRoute(routes, path) {
  if (!path) return null;
  return routes.find((route) => route.path === String(path)) ?? null;
}

function jetWebBoundaryState(route, navigation) {
  if (!route) return {};
  const facts = route.render_facts;
  const pending = facts.pending_boundary_id;
  const error = facts.error_boundary_id;
  const active = navigation.status === "error"
    ? error ?? pending
    : pending;
  return {
    pending_boundary_id: pending == null ? null : String(Number(pending)),
    error_boundary_id: error == null ? null : String(Number(error)),
    active_boundary_id: active == null ? null : String(Number(active)),
    island_identity: facts.island?.identity ?? null,
    hydration_trigger: facts.island?.hydration_trigger ?? null,
    route: route.path,
    status: navigation.status,
  };
}

function jetWebNavigationSnapshot(controller, status, url, payload, route, error = "") {
  const current = route
    ? {
      route: route.path,
      url: payload?.url || url,
      params: Object.create(null),
      search: Object.create(null),
    }
    : controller.state.current;
  return {
    status,
    current,
    data: payload?.data ?? null,
    data_wire: payload?.data_wire ?? "",
    error: String(error || ""),
    ...jetWebBoundaryState(route, { status }),
  };
}

function jetWebResponseDocument(html) {
  if (typeof DOMParser !== "undefined") {
    return new DOMParser().parseFromString(String(html), "text/html");
  }
  throw new TypeError("web navigation requires the browser DOMParser");
}

function jetWebReplaceBody(doc, parsed) {
  if (!doc?.body || !parsed?.body) return;
  const nodes = [...(parsed.body.childNodes ?? [])].map((node) => doc.adoptNode?.(node) ?? node);
  if (typeof doc.body.replaceChildren === "function") {
    doc.body.replaceChildren(...nodes);
  } else {
    while (doc.body.firstChild) doc.body.removeChild(doc.body.firstChild);
    for (const node of nodes) doc.body.appendChild(node);
  }
  const title = parsed.querySelector?.("title")?.textContent;
  if (typeof title === "string") doc.title = title;
}

function jetWebSameOriginUrl(url) {
  if (typeof URL === "undefined") throw new TypeError("web navigation requires URL support");
  const base = typeof location !== "undefined" && location.href
    ? location.href
    : "http://jet.invalid/";
  const target = new URL(String(url), base);
  if (typeof location !== "undefined" && location.origin && target.origin !== location.origin) {
    throw new TypeError("web navigation endpoint must be same-origin");
  }
  if (target.hash) target.hash = "";
  return target;
}

function jetWebNavigationRoot(options, doc) {
  if (options?.container && typeof options.container.setAttribute === "function") {
    return options.container;
  }
  return doc?.getElementById?.("jet-app") ?? null;
}

function jetWebNavigationMark(root, state) {
  if (!root?.dataset) return;
  root.dataset.jetRoute = state.current?.route ?? "";
  root.dataset.jetUrl = state.current?.url ?? "";
  root.dataset.jetNavigationStatus = state.status;
  root.dataset.jetLoaderData = state.data_wire;
  root.setAttribute?.("aria-busy", state.status === "pending" ? "true" : "false");
}


/**
 * Browser transport/controller over the native App graph.  Every state/data
 * consumer below remains the existing WebQuery/WebForm/WebTable/WebVirtual/
 * WebStore projection; this object only schedules server documents.
 */
export function createWebApp(input, options = {}) {
  if (input && typeof input.navigate === "function" && input.graph !== undefined) {
    return input;
  }
  const facts = jetWebGraphInput(input);
  const doc = options.document ?? globalThis.document ?? null;
  const controller = {
    graph: facts.graph,
    routes: facts.routes,
    actions: facts.actions,
    document: doc,
    options: { ...options },
    state: {
      status: "idle",
      current: null,
      data: null,
      data_wire: "",
      error: "",
      pending_boundary_id: null,
      error_boundary_id: null,
      island_identity: null,
      hydration_trigger: null,
    },
    listeners: new Set(),
    active: null,
    sequence: 0,
    preloads: new Map(),
    preloadControllers: new Map(),
    navigationCleanup: null,

    route(path) {
      return jetWebGraphRoute(this.routes, path);
    },
    subscribe(handler) {
      if (typeof handler !== "function") throw new TypeError("web navigation subscription requires a callback");
      this.listeners.add(handler);
      handler(this.state);
      return () => this.listeners.delete(handler);
    },
    publish(next) {
      this.state = next;
      jetWebNavigationMark(jetWebNavigationRoot(this.options, this.document), next);
      for (const listener of this.listeners) listener(next);
      jetWebNavigationEvent("jet:web-navigation", { app: this, navigation: next });
      return next;
    },
    seed(doc = this.document) {
      this.document = doc ?? this.document;
      const payload = readWebRouteData(this.document);
      if (!payload) return this.state;
      const route = jetWebGraphRoute(this.routes, payload.route);
      if (!route) throw new TypeError(`server route data names unknown checked route ${payload.route}`);
      return this.publish(
        jetWebNavigationSnapshot(this, payload.status, payload.url || this.document?.defaultView?.location?.href || globalThis.location?.href || payload.route, payload, route),
      );
    },
    async fetchDocument(target, signal) {
      const fetcher = this.options.fetch ?? globalThis.fetch;
      if (typeof fetcher !== "function") throw new TypeError("web navigation requires fetch");
      const response = await fetcher(target.href, {
        method: "GET",
        mode: "same-origin",
        credentials: "same-origin",
        cache: "no-store",
        headers: {
          accept: "text/html, application/xhtml+xml",
          "x-jet-navigation": "1",
        },
        signal,
      });
      const html = await response.text();
      const parsed = jetWebResponseDocument(html);
      const payload = readWebRouteData(parsed);
      return { response, parsed, payload };
    },
    async preload(url) {
      const target = jetWebSameOriginUrl(url);
      const key = target.href;
      const existing = this.preloads.get(key);
      if (existing) return existing;
      const controller = new AbortController();
      this.preloadControllers.set(key, controller);
      const promise = (async () => {
        const previous = this.state;
        this.publish({ ...previous, status: "preloading", error: "" });
        try {
          const result = await this.fetchDocument(target, controller.signal);
          const responseOk = result.response.ok !== false && Number(result.response.status) < 400;
          if (!responseOk) {
            throw new Error(`web preload returned HTTP ${result.response.status}`);
          }
          const route = jetWebGraphRoute(this.routes, result.payload?.route);
          if (result.payload && !route) {
            throw new TypeError(`server preload names unknown checked route ${result.payload.route}`);
          }
          return {
            url: result.payload?.url || target.href,
            route,
            response_status: Number(result.response.status),
            payload: result.payload,
            document: result.parsed,
          };
        } catch (cause) {
          if (controller.signal.aborted && this.state.status === "preloading") {
            this.publish({ ...this.state, status: "aborted", error: "navigation aborted" });
          }
          throw cause;
        } finally {
          if (this.preloadControllers.get(key) === controller) {
            this.preloadControllers.delete(key);
          }
          if (this.state.status === "preloading") this.publish({ ...this.state, status: previous.status });
        }
      })();
      this.preloads.set(key, promise);
      try {
        return await promise;
      } finally {
        this.preloads.delete(key);
      }
    },
    async navigate(url, navigationOptions = {}) {
      const target = jetWebSameOriginUrl(url);
      const token = ++this.sequence;
      if (this.active) {
        this.active.superseded = true;
        this.active.controller.abort("superseded");
      }
      const active = {
        token,
        controller: new AbortController(),
        superseded: false,
        externalAbortCleanup: null,
      };
      const externalSignal = navigationOptions?.signal;
      if (externalSignal && typeof externalSignal.addEventListener === "function") {
        const onAbort = () => active.controller.abort(externalSignal.reason ?? "aborted");
        externalSignal.addEventListener("abort", onAbort, { once: true });
        active.externalAbortCleanup = () => externalSignal.removeEventListener?.("abort", onAbort);
        if (externalSignal.aborted) onAbort();
      }
      this.active = active;
      const routeHint = this.state.current?.route
        ? jetWebGraphRoute(this.routes, this.state.current.route)
        : null;
      this.publish(jetWebNavigationSnapshot(this, "pending", target.href, null, routeHint));
      try {
        const result = await this.fetchDocument(target, active.controller.signal);
        if (this.active !== active || active.superseded || token !== this.sequence) {
          return this.state;
        }
        const route = jetWebGraphRoute(this.routes, result.payload?.route);
        if (result.payload && !route) {
          throw new TypeError(`server navigation names unknown checked route ${result.payload.route}`);
        }
        const responseOk = result.response.ok !== false && Number(result.response.status) < 400;
        const responseStatus = jetWebNavigationStatus(
          result.payload?.status ?? (responseOk ? "ready" : "error"),
          responseOk ? "ready" : "error",
        );
        const nextStatus = responseOk ? responseStatus : "error";
        const next = jetWebNavigationSnapshot(
          this,
          nextStatus,
          target.href,
          result.payload,
          route,
          responseOk ? "" : `navigation returned HTTP ${result.response.status}`,
        );
        jetWebReplaceBody(this.document, result.parsed);
        if (typeof history !== "undefined" && navigationOptions.history !== "none") {
          const method = navigationOptions.replace === true ? "replaceState" : "pushState";
          history[method]?.({}, "", target.href);
        }
        return this.publish(next);
      } catch (cause) {
        if (this.active !== active || active.superseded || token !== this.sequence) {
          return this.state;
        }
        if (active.controller.signal.aborted) {
          const aborted = jetWebNavigationSnapshot(this, "aborted", this.state.current?.url ?? target.href, null, routeHint);
          return this.publish(aborted);
        }
        const error = cause instanceof Error ? cause : new Error(String(cause));
        const failed = jetWebNavigationSnapshot(
          this,
          "error",
          this.state.current?.url ?? target.href,
          null,
          routeHint,
          error.message,
        );
        this.publish(failed);
        throw error;
      } finally {
        active.externalAbortCleanup?.();
        active.externalAbortCleanup = null;
        if (this.active === active) this.active = null;
      }
    },
    load(url, navigationOptions = {}) {
      return this.navigate(url, navigationOptions);
    },
    abort() {
      const active = this.active;
      const preloadControllers = [...this.preloadControllers.values()];
      if (!active && preloadControllers.length === 0) return false;
      if (active) {
        active.superseded = true;
        active.controller.abort("cancelled");
        this.active = null;
      }
      for (const controller of preloadControllers) controller.abort("cancelled");
      this.publish(jetWebNavigationSnapshot(
        this,
        "aborted",
        this.state.current?.url ?? "",
        null,
        this.route(this.state.current?.route),
      ));
      return true;
    },
    link(url) {
      const target = jetWebSameOriginUrl(url);
      return target.pathname + target.search;
    },
    action(nameOrAction, input, callOptions = {}) {
      const action = typeof nameOrAction === "string"
        ? this.actions.find((candidate) => candidate.name === nameOrAction)
        : nameOrAction;
      if (!action || typeof action !== "object") {
        throw new TypeError(`checked web action ${String(nameOrAction)} is not present`);
      }
      return webServerFunctionCall(
        {
          ...action,
          endpoint: action.endpoint,
          method: action.method,
          name: action.name,
          csrf: action.csrf,
        },
        input,
        callOptions,
      );
    },
    bindRoute(container, route, navigation = this.state) {
      const state = navigation?.status === "idle"
        ? { ...navigation, status: "ready" }
        : navigation;
      return projectWebLoader(container, route, state);
    },
    bindQuery(container, source) {
      projectWebQuery(container, source);
      return subscribeWebQuery(source, (next) => projectWebQuery(container, next));
    },
    bindForm(container, source) {
      projectWebForm(container, source);
      return subscribeWebForm(source, (next) => projectWebForm(container, next));
    },
    bindTable(container, source) {
      projectWebTable(container, source);
      return source?.subscribe
        ? source.subscribe((next) => projectWebTable(container, next))
        : null;
    },
    bindVirtual(container, source, rows) {
      projectWebVirtual(container, source, rows);
      return source?.subscribe
        ? source.subscribe((next) => projectWebVirtual(container, next))
        : null;
    },
    bindStore(container, source) {
      projectWebStore(container, source);
      return subscribeWebStore(source, (next) => projectWebStore(container, next));
    },
    hydrate(route, streamId, startedAtNs) {
      return resumeWebRouteHydration(route, streamId, startedAtNs);
    },
    installNavigation() {
      const doc = this.document;
      if (!doc?.addEventListener || this.navigationCleanup) return this;
      const onClick = (event) => {
        if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
        const anchor = event.target?.closest?.("a[href]");
        if (!anchor || anchor.target || anchor.hasAttribute?.("download")) return;
        const routeHint = anchor.dataset?.jetRoute;
        if (routeHint && !this.route(routeHint)) return;
        const href = anchor.href || anchor.getAttribute?.("href");
        let target;
        try {
          target = jetWebSameOriginUrl(href);
        } catch (_) {
          return;
        }
        event.preventDefault();
        this.navigate(target.href).catch((error) => {
          jetWebNavigationEvent("jet:web-navigation-error", { app: this, error });
        });
      };
      const onPopState = () => {
        this.navigate(globalThis.location?.href ?? "/", { history: "none" }).catch(() => {});
      };
      doc.addEventListener("click", onClick);
      globalThis.addEventListener?.("popstate", onPopState);
      this.navigationCleanup = () => {
        doc.removeEventListener("click", onClick);
        globalThis.removeEventListener?.("popstate", onPopState);
        this.navigationCleanup = null;
      };
      return this;
    },
    destroy() {
      this.navigationCleanup?.();
      this.abort();
      this.listeners.clear();
      if (globalThis.__jetWebApp === this) delete globalThis.__jetWebApp;
    },
  };
  return controller;
}

export function bootstrapWebApp(appOrFacts, options = {}) {
  const app = appOrFacts && typeof appOrFacts.navigate === "function"
    ? appOrFacts
    : createWebApp(appOrFacts, options);
  if (options.document) app.document = options.document;
  app.options = { ...app.options, ...options };
  app.seed(app.document ?? globalThis.document);
  if (options.navigation !== false) app.installNavigation();
  globalThis.__jetWebApp = app;
  return app;
}

export function jetWebAppAction(app, name, input, options = {}) {
  if (!app || typeof app.action !== "function") throw new TypeError("web action requires a bootstrapped App");
  return app.action(name, input, options);
}

if (typeof globalThis !== "undefined") {
  globalThis.__jetReadWebRouteData = readWebRouteData;
  globalThis.__jetCreateWebApp = createWebApp;
  globalThis.__jetBootstrapWebApp = bootstrapWebApp;
}

// D-DX-SUITE1=C: browser adapters consume the canonical Web* state/facts
// emitted by the checked Prelude.  They do not sort, cache, validate, retry,
// or decode application values; those policies stay in the shared App/Observe
// kernels.  Native form submission remains the default.

function jetWebJsonStringify(value) {
  return JSON.stringify(
    value,
    (_key, item) => typeof item === "bigint" ? JSON.rawJSON(item.toString()) : item,
  );
}
function jetWebProjectionValue(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  try {
    return jetWebJsonStringify(value);
  } catch (_) {
    return String(value);
  }
}

function jetWebProjectionFacts(value, label) {
  const facts = typeof value === "function" ? value() : value;
  if (!facts || typeof facts !== "object") {
    throw new TypeError(`${label} projection requires canonical state facts`);
  }
  return facts;
}

function jetWebProjectionContainer(container) {
  if (container && typeof container.appendChild === "function") return container;
  if (typeof container === "string") return query(container);
  return jetDomContainer();
}

function jetWebProjectionRows(root, rows, prefix) {
  if (!root || !Array.isArray(rows) || typeof document === "undefined") return;
  root.replaceChildren();
  const fragment = document.createDocumentFragment();
  rows.forEach((row, index) => {
    const element = document.createElement("div");
    const key = row && typeof row === "object"
      ? row.key ?? row.id ?? index
      : index;
    element.dataset.jetRowKey = String(key);
    element.dataset.jetProjection = prefix;
    element.textContent = jetWebProjectionValue(
      row && typeof row === "object" && "value" in row ? row.value : row,
    );
    fragment.appendChild(element);
  });
  root.appendChild(fragment);
}

/** Project canonical WebQuery state without creating a browser cache. */
export function projectWebQuery(container, queryState) {
  const facts = jetWebProjectionFacts(queryState, "web query");
  const root = jetWebProjectionContainer(container);
  const state = facts.state && typeof facts.state === "object" ? facts.state : facts;
  const status = String(state.status ?? "").toLowerCase();
  if (root) {
    root.dataset.jetQueryKey = String(facts.key ?? facts.identity ?? "");
    root.dataset.jetQueryStatus = status;
    root.dataset.jetQueryGeneration = String(state.generation ?? "");
    root.setAttribute("aria-busy", ["pending", "fetching"].includes(status) ? "true" : "false");
    root.textContent = jetWebProjectionValue(state.value ?? state.error ?? "");
  }
  return facts;
}

/** Project canonical WebForm state into an existing native form. */
export function projectWebForm(container, formState) {
  const facts = jetWebProjectionFacts(formState, "web form");
  const root = jetWebProjectionContainer(container);
  const state = facts.state && typeof facts.state === "object" ? facts.state : facts;
  if (!root) return facts;

  const status = String(state.status ?? "").toLowerCase();
  const lifecycle = state.lifecycle && typeof state.lifecycle === "object"
    ? state.lifecycle
    : facts.lifecycle && typeof facts.lifecycle === "object" ? facts.lifecycle : null;
  const lifecycleStatus = String(
    lifecycle?.status ?? facts.lifecycle_status ?? "",
  ).toLowerCase();
  root.dataset.jetFormStatus = status;
  if (lifecycleStatus) root.dataset.jetFormLifecycle = lifecycleStatus;
  root.dataset.jetFormAction = String(state.action ?? facts.action ?? "");
  root.setAttribute(
    "aria-busy",
    ["validating", "submitting", "pending"].includes(status)
      || ["pending", "submitting"].includes(lifecycleStatus) ? "true" : "false",
  );

  const fieldEntries = state.fields instanceof Map
    ? [...state.fields.entries()]
    : Object.entries(state.fields ?? {});
  const controls = root.elements ? [...root.elements] : [];
  const fieldRoots = root.querySelectorAll?.("[data-field]") ?? [];
  const fieldErrors = root.querySelectorAll?.("[data-field-error], .form-error") ?? [];
  const findFieldRoot = (name) => [...fieldRoots].find(
    (candidate) => candidate.dataset?.field === String(name),
  );
  const findFieldError = (name, fieldRoot) => {
    const existing = fieldRoot?.querySelector?.("[data-field-error], .error");
    if (existing) return existing;
    return [...fieldErrors].find(
      (candidate) => candidate.dataset?.fieldError === String(name),
    );
  };

  for (const [fieldName, rawField] of fieldEntries) {
    const field = rawField && typeof rawField === "object" ? rawField : {};
    const name = String(field.name ?? fieldName);
    const wireName = String(field.wire_name ?? field.wireName ?? name);
    const value = jetWebProjectionValue(field.value ?? "");
    const errors = Array.isArray(field.errors)
      ? field.errors.map((error) => String(error))
      : field.error ? [String(field.error)] : [];
    const invalid = errors.length > 0;
    for (const control of controls) {
      if (!control || ![name, wireName].includes(String(control.name))) continue;
      if (String(control.type).toLowerCase() === "checkbox") {
        control.checked = value === "true" || value === "1" || value === control.value;
      } else if (control.value !== value) {
        control.value = value;
      }
      control.setAttribute?.("aria-invalid", invalid ? "true" : "false");
      control.setAttribute?.("aria-busy", field.validating ? "true" : "false");
    }
    const fieldRoot = findFieldRoot(name);
    let errorNode = findFieldError(name, fieldRoot);
    if (invalid) {
      if (!errorNode && fieldRoot && typeof document !== "undefined") {
        errorNode = document.createElement("span");
        errorNode.dataset.fieldError = name;
        errorNode.className = "error";
        errorNode.setAttribute("role", "alert");
        fieldRoot.appendChild(errorNode);
      }
      if (errorNode) errorNode.textContent = errors.join("; ");
    } else if (errorNode?.dataset?.fieldError === name) {
      errorNode.remove();
    } else if (errorNode) {
      errorNode.textContent = "";
    }
  }

  const formError = root.querySelector?.("[data-form-error], .form-error");
  if (formError) formError.textContent = String(state.error ?? "");
  return facts;
}

function subscribeWebProjection(source, handler, label) {
  if (typeof handler !== "function") {
    throw new TypeError(`${label} subscription requires a callback`);
  }
  if (source && typeof source.subscribe === "function") {
    return source.subscribe(handler);
  }
  if (source && typeof source.subscribe_state === "function") {
    return source.subscribe_state(handler);
  }
  const signal = source?.state_signal ?? source?.signal;
  if (signal && typeof signal.subscribe === "function") {
    return signal.subscribe(handler);
  }
  throw new TypeError(`${label} subscription requires canonical subscribe()`);
}

/** Subscribe through the canonical query state; this adapter owns no cache. */
export function subscribeWebQuery(query, handler) {
  return subscribeWebProjection(query, handler, "web query");
}

/** Subscribe through the canonical form state; this adapter owns no state. */
export function subscribeWebForm(form, handler) {
  return subscribeWebProjection(form, handler, "web form");
}

function jetWebFormInput(form) {
  const input = Object.create(null);
  for (const [name, value] of new FormData(form).entries()) {
    const item = typeof value === "string" ? value : value.name;
    if (!(name in input)) {
      input[name] = item;
    } else if (Array.isArray(input[name])) {
      input[name].push(item);
    } else {
      input[name] = [input[name], item];
    }
  }
  return input;
}

function jetWebFormServerErrorPayload(error) {
  const detail = error?.detail && typeof error.detail === "object"
    ? error.detail
    : null;
  const direct = detail?.field_errors || detail?.fieldErrors || detail?.form_errors
    || detail?.formErrors ? detail : null;
  if (direct) return direct;
  const message = typeof detail?.message === "string"
    ? detail.message
    : typeof error?.message === "string" ? error.message : "";
  if (!message.trim().startsWith("{")) return null;
  try {
    const parsed = (globalThis.__jetWebJsonParse ?? JSON.parse)(message);
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch (_) {
    return null;
  }
}

/** Paint the typed action's field-addressed error contract onto native DOM. */
function jetWebFormApplyServerErrors(form, error) {
  const payload = jetWebFormServerErrorPayload(error);
  if (!payload) return null;
  const fieldErrors = payload.field_errors ?? payload.fieldErrors ?? {};
  const fieldRoots = form.querySelectorAll?.("[data-field]") ?? [];
  const controls = form.elements ? [...form.elements] : [];
  const findRoot = (name) => [...fieldRoots].find(
    (candidate) => candidate.dataset?.field === String(name),
  );
  for (const [name, rawMessages] of Object.entries(fieldErrors)) {
    const messages = Array.isArray(rawMessages)
      ? rawMessages.map((message) => String(message))
      : [String(rawMessages)];
    const fieldRoot = findRoot(name);
    for (const control of controls) {
      if (!control || ![String(name), control.dataset?.field].includes(String(control.name))) {
        continue;
      }
      control.setAttribute?.("aria-invalid", "true");
      control.setAttribute?.("aria-describedby", `${control.id || name}-error`);
    }
    let errorNode = fieldRoot?.querySelector?.("[data-field-error], .error");
    if (!errorNode && fieldRoot && typeof document !== "undefined") {
      errorNode = document.createElement("span");
      errorNode.dataset.fieldError = String(name);
      errorNode.className = "error";
      errorNode.setAttribute("role", "alert");
      fieldRoot.appendChild(errorNode);
    }
    if (errorNode) {
      errorNode.dataset.fieldError = String(name);
      errorNode.textContent = messages.join("; ");
    }
  }
  const formMessages = payload.form_errors ?? payload.formErrors ?? [];
  const messages = Array.isArray(formMessages)
    ? formMessages.map((message) => String(message))
    : [String(formMessages)];
  if (messages.length > 0 && messages.some((message) => message.length > 0)) {
    let formError = form.querySelector?.("[data-form-error], .form-error");
    if (!formError && typeof document !== "undefined") {
      formError = document.createElement("div");
      formError.dataset.formError = "true";
      formError.className = "form-error";
      formError.setAttribute("role", "alert");
      form.appendChild(formError);
    }
    if (formError) formError.textContent = messages.join("; ");
  }
  form.dataset && (form.dataset.jetFormStatus = "invalid");
  return payload;
}

/**
 * Configure one native form from the checked server-function boundary.
 * Enhancement is opt-in; without it the browser performs the ordinary native
 * POST and the server owns decoding/validation.
 */
export function attachWebForm(boundary, form, options = {}) {
  const config = webServerFunctionForm(boundary, form, options);
  if (options.enhance !== true) return config;
  if (typeof form.addEventListener !== "function") {
    throw jetWebServerFnError("invalid_definition", "enhanced web form requires an event-capable form element", "request-unknown");
  }
  form.__jetWebSubmitHandler?.();
  let active = false;
  const listener = (event) => {
    event.preventDefault();
    if (active) return;
    active = true;
    form.setAttribute("aria-busy", "true");
    if (form.dataset) form.dataset.jetPending = "true";
    Promise.resolve()
      .then(() => {
        const input = typeof options.encode === "function"
          ? options.encode(form, event)
          : jetWebFormInput(form);
        return webServerFunctionCall(boundary, input, options);
      })
      .then(async (result) => {
        const keys = Array.isArray(result?.revalidate) ? result.revalidate : config.revalidate;
        if (typeof options.revalidate === "function") {
          await options.revalidate(keys.slice(), result);
        }
        options.onSuccess?.(result);
        form.dispatchEvent?.(new CustomEvent("jet:web-form-result", { detail: result }));
      })
      .catch((error) => {
        jetWebFormApplyServerErrors(form, error);
        options.onError?.(error);
        form.dispatchEvent?.(new CustomEvent("jet:web-form-error", { detail: error }));
      })
      .finally(() => {
        active = false;
        form.setAttribute("aria-busy", "false");
        if (form.dataset) form.dataset.jetPending = "false";
      });
  };
  form.addEventListener("submit", listener);
  form.__jetWebSubmitHandler = () => form.removeEventListener("submit", listener);
  return { ...config, enhanced: true };
}

/** A loader result is already resolved by the shared Router; this only paints it. */
export function projectWebLoader(container, route, navigationState = null) {
  return attachWebRouteBoundary(container, route, navigationState);
}

/** Project canonical table rows; sorting/filtering/pagination stay in WebTable. */
export function projectWebTable(container, tableState) {
  const facts = jetWebProjectionFacts(tableState, "web table");
  const root = jetWebProjectionContainer(container);
  const state = facts.state && typeof facts.state === "object" ? facts.state : facts;
  const rows = state.rows ?? state.page?.rows;
  if (root) {
    root.dataset.jetTableKey = String(facts.key ?? facts.identity ?? "");
    root.dataset.jetTableStatus = String(state.status ?? "");
    if (Array.isArray(rows)) jetWebProjectionRows(root, rows, "table");
  }
  return facts;
}

/** Project the bounded visible window supplied by the canonical Virtual plan. */
export function projectWebVirtual(container, viewport, visibleRows = undefined) {
  const facts = jetWebProjectionFacts(viewport, "web virtual");
  const root = jetWebProjectionContainer(container);
  const rows = visibleRows ?? facts.rows ?? facts.visible_rows;
  if (root) {
    root.dataset.jetVirtualStart = String(facts.start ?? "");
    root.dataset.jetVirtualEnd = String(facts.end ?? "");
    if (Array.isArray(rows)) jetWebProjectionRows(root, rows, "virtual");
  }
  return facts;
}

/** Project a store snapshot; history/transactions remain Prelude-owned. */
export function projectWebStore(container, storeState) {
  const facts = jetWebProjectionFacts(storeState, "web store");
  const root = jetWebProjectionContainer(container);
  const state = facts.state && typeof facts.state === "object" ? facts.state : facts;
  if (root) {
    root.dataset.jetStoreKey = String(facts.key ?? facts.identity ?? "");
    root.dataset.jetStoreGeneration = String(state.generation ?? "");
    root.textContent = jetWebProjectionValue(state.value ?? state);
  }
  return facts;
}

/** Subscribe through the canonical store; this adapter owns no second signal. */
export function subscribeWebStore(store, handler) {
  if (!store || typeof store.subscribe !== "function") {
    throw new TypeError("web store subscription requires canonical subscribe()");
  }
  if (typeof handler !== "function") {
    throw new TypeError("web store subscription requires a callback");
  }
  return store.subscribe(handler);
}

if (typeof globalThis !== "undefined") {
  globalThis.__jetProjectWebQuery = projectWebQuery;
  globalThis.__jetAttachWebForm = attachWebForm;
  globalThis.__jetProjectWebLoader = projectWebLoader;
  globalThis.__jetProjectWebTable = projectWebTable;
  globalThis.__jetProjectWebVirtual = projectWebVirtual;
  globalThis.__jetProjectWebStore = projectWebStore;
  globalThis.__jetSubscribeWebStore = subscribeWebStore;
  globalThis.__jetProjectWebForm = projectWebForm;
  globalThis.__jetSubscribeWebQuery = subscribeWebQuery;
  globalThis.__jetSubscribeWebForm = subscribeWebForm;
}

// D-UISHOWCASE1 (c134 Phase 8): stable per-node DOM identity without adding
// any argument to the Jet-level `ui.null_backend()` call (I7/I8 — no new
// language surface for what's purely a codegen bookkeeping concern). Every
// exported top-level `#JS` function's generated body is wrapped in
// `enterRenderScope(name)` / `exitRenderScope()` (see Web.rs's `emit_js_fn`).
// `enterRenderScope` only resets the scope name + counter when call depth is
// 0 — i.e. only for the OUTERMOST exported call, not for a shared helper
// (also exported, since every `#JS` function is) invoked *from* that outer
// call. `createBackend()` then stamps each new backend with
// `"{scope}#{ordinal}"`, and `paint()` looks its box up in a scope-keyed
// registry instead of caching one element per backend OBJECT. That's what
// makes both real-world shapes work with the same mechanism:
//   - 196_ui_web_click.jet's `render(n)`, called repeatedly by a click
//     handler: each call resets to the same first key ("render#0") ->
//     the one box is found and updated in place, never duplicated.
//   - 197_ui_showcase.jet's `initApp()`, called once, painting several
//     distinct cards via a shared `paint_stat_card` helper: each nested
//     `createBackend()` call increments the *same* "initApp" scope's
//     counter ("initApp#0", "initApp#1", …) -> each card gets its own box.
let jetDomScopeName = "__top__";
let jetDomScopeCounter = 0;
let jetDomScopeDepth = 0;
const jetDomBoxRegistry = new Map();
let jetDomTouchedBackends = new Set();
// D-UI-EVT-DISP1=E: O(1) click slots keyed by stable node identity
// (D-UI-NODE-ID1=C: author key if present, else render path).
const jetUiClickSlots = new Map();

export function jetUiDispatch(identity) {
  const handler = jetUiClickSlots.get(String(identity));
  if (typeof handler === "function") handler();
}

export function jetUiBindClick(identity, handler) {
  if (typeof handler === "function") {
    jetUiClickSlots.set(String(identity), handler);
  } else {
    jetUiClickSlots.delete(String(identity));
  }
}

export function jetUiUnbindClick(identity) {
  jetUiClickSlots.delete(String(identity));
}

export function perfNow() {
  return globalThis.__jetPerfNow?.() ?? 0;
}

export function perfRecord(symbol, eventClass, started) {
  globalThis.__jetPerfRecord?.(String(symbol), String(eventClass), started);
}

export function enterRenderScope(name) {
  if (jetDomScopeDepth === 0) {
    jetDomScopeName = name;
    jetDomScopeCounter = 0;
    jetDomTouchedBackends = new Set();
  }
  jetDomScopeDepth++;
}

export function exitRenderScope() {
  jetDomScopeDepth = Math.max(0, jetDomScopeDepth - 1);
  if (jetDomScopeDepth === 0) {
    const prefix = `${jetDomScopeName}#`;
    for (const [key, record] of jetDomBoxRegistry) {
      const backendKey = key.split("/")[0];
      if (key.startsWith(prefix) && !jetDomTouchedBackends.has(backendKey)) {
        record.element?.remove?.();
        jetDomBoxRegistry.delete(key);
        jetUiClickSlots.delete(key);
      }
    }
  }
}

export function createBackend() {
  const boxKey = `${jetDomScopeName}#${jetDomScopeCounter++}`;
  jetDomTouchedBackends.add(boxKey);
  return {
    kind: "dom",
    commands: [],
    root: jetDomContainer(),
    boxKey,
    focusNodes: [],
    focusedIndex: -1,
  };
}

export function measure(node, constraint) {
  const projected = jetUiNodeProjection(node);
  const minWidth = constraint?.minWidth ?? constraint?.min_width ?? 0;
  const minHeight = constraint?.minHeight ?? constraint?.min_height ?? 0;
  const maxWidth = constraint?.maxWidth ?? constraint?.max_width ?? DEFAULT_MOUNT_COLS;
  const maxHeight = constraint?.maxHeight ?? constraint?.max_height ?? DEFAULT_MOUNT_ROWS;
  const naturalWidth = projected.kind === "box"
    ? projected.children.reduce((width, child) => Math.max(width, jetNum(child.width)), 0)
    : jetNum(projected.width);
  const naturalHeight = projected.kind === "box"
    ? projected.children.reduce((height, child) => height + jetNum(child.height), 0)
    : jetNum(projected.height);
  const width = Math.min(Math.max(naturalWidth, jetNum(minWidth)), jetNum(maxWidth));
  const height = Math.min(Math.max(naturalHeight, jetNum(minHeight)), jetNum(maxHeight));
  return { width, height };
}

export function layout(backend, node, frame) {
  backend.frame = frame;
  backend.node = node;
  return frame;
}

// Card #1658: shared default mount viewport (classic 80x24 terminal), named
// once so no host hand-types the literal. Matches DEFAULT_MOUNT_COLS/ROWS in
// crates/jet-codegen/src/Prelude/Ui.rs.
export const DEFAULT_MOUNT_COLS = 80;
export const DEFAULT_MOUNT_ROWS = 24;

/** D-UI-MOUNT1=A: measure → layout → paint (optional constraint; default 80×24). */
export function mount(backend, node, constraint) {
  const bounds = constraint ?? { minWidth: 0, minHeight: 0, maxWidth: DEFAULT_MOUNT_COLS, maxHeight: DEFAULT_MOUNT_ROWS };
  backend.commands = [];
  const size = measure(node, bounds);
  layout(backend, node, { x: 0, y: 0, width: size.width, height: size.height });
  paint(backend, node);
}

// D-STYLESHAPE1=A wiring: pick readable text color (WCAG-style relative
// luminance threshold) so a dark fill gets light text and vice versa,
// instead of hardcoding one text color regardless of the node's fill.
function jetReadableTextColor(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex || "");
  if (!m) return "#111";
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.6 ? "#111" : "#fff";
}

function jetUiVariant(value) {
  return value?.tag ?? (value == null ? "" : String(value));
}

function jetUiOptionValue(value) {
  if (value == null || value.tag === "None") return undefined;
  return value.tag === "Some" ? value.values?.[0] : value;
}
function jetUiImeEnabled(mode) {
  // A role-only textbox has no explicit mode and keeps the native default.
  return jetUiVariant(mode) !== "Disabled";
}

function jetUiNodeProjection(node) {
  const kindNames = {
    Custom: "custom",
    Text: "text",
    Box: "box",
    Button: "button",
    TextInput: "textInput",
  };
  const roleNames = {
    Button: "button",
    TextInput: "textbox",
    Label: "label",
    Container: "group",
  };
  const accessibility = jetUiOptionValue(node?.accessibility);
  const ime = jetUiOptionValue(node?.ime);
  const shortcut = jetUiOptionValue(node?.shortcut);
  const color = jetUiOptionValue(node?.color);
  const key = jetUiOptionValue(node?.key);
  const role = jetUiOptionValue(node?.role);
  return {
    ...node,
    kind: kindNames[jetUiVariant(node?.kind)] ?? String(node?.kind ?? "custom"),
    role: roleNames[jetUiVariant(role)] ?? (role == null ? null : String(role)),
    color: color == null ? undefined : String(color),
    key: key == null || String(key) === "" ? null : String(key),
    accessibility,
    ime,
    shortcut,
    onClick: node?.onClick ?? node?.on_click ?? null,
    onDrop: node?.onDrop ?? node?.on_drop ?? null,
    children: Array.from(node?.children ?? []),
  };
}

function jetUiAccessibilityProjection(element, accessibility, fallbackLabel) {
  const name = jetUiOptionValue(accessibility?.name);
  const description = jetUiOptionValue(accessibility?.description);
  const label = name == null || String(name) === "" ? fallbackLabel : String(name);
  if (label) element.setAttribute?.("aria-label", label);
  else element.removeAttribute?.("aria-label");
  if (description == null || String(description) === "") {
    element.removeAttribute?.("aria-description");
  } else {
    element.setAttribute?.("aria-description", String(description));
  }
  for (const state of accessibility?.states ?? []) {
    const tag = jetUiVariant(state);
    const value = state?.values?.[0];
    if (tag === "Disabled") element.setAttribute?.("aria-disabled", "true");
    if (tag === "Busy") element.setAttribute?.("aria-busy", "true");
    if (tag === "Expanded") element.setAttribute?.("aria-expanded", String(Boolean(value)));
    if (tag === "Checked") element.setAttribute?.("aria-checked", String(Boolean(value)));
    if (tag === "Selected") element.setAttribute?.("aria-selected", String(Boolean(value)));
    if (tag === "Required") element.setAttribute?.("aria-required", "true");
    if (tag === "Value") element.setAttribute?.("aria-valuetext", String(value ?? ""));
  }
}

function jetUiShortcutText(shortcut) {
  if (!shortcut) return "";
  const bits = Number(shortcut.modifiers?.bits ?? 0) & 0x1f;
  const names = [];
  if (bits & 16) names.push("Command");
  if (bits & 8) names.push("Meta");
  if (bits & 1) names.push("Control");
  if (bits & 2) names.push("Alt");
  if (bits & 4) names.push("Shift");
  return [...names, String(shortcut.key ?? "")].filter(Boolean).join("+");
}

function jetUiDropOperation(event) {
  const effect = String(event?.dataTransfer?.dropEffect ?? "copy").toLowerCase();
  return effect === "move" ? { tag: "Move", values: [] } : effect === "link" ? { tag: "Link", values: [] } : { tag: "Copy", values: [] };
}

function jetUiDropItems(event) {
  const transfer = event?.dataTransfer;
  if (!transfer) return [];
  const values = [];
  const uriText = transfer.getData?.("text/uri-list") ?? "";
  for (const uri of String(uriText).split(/\r?\n/).map((item) => item.trim()).filter(Boolean)) {
    if (!uri.startsWith("#")) values.push({ tag: "Uri", values: [uri] });
  }
  const text = transfer.getData?.("text/plain") ?? "";
  if (text) values.push({ tag: "Text", values: [String(text)] });
  // A browser File has no native path. Do not turn its display name into a
  // fabricated JetUiGrantedPath; package adapters may provide URI items.
  return values;
}

function jetUiWireDrop(element, identity, handler) {
  element._jetDropHandler = typeof handler === "function" ? handler : null;
  if (element._jetDropWired || typeof element.addEventListener !== "function") return;
  element._jetDropWired = true;
  const queue = globalThis.jet_ui_web_drag_queue;
  const emit = (phase, event) => {
    event.preventDefault?.();
    const items = jetUiDropItems(event);
    queue?.({
      target: { value: identity },
      phase: { tag: phase, values: [] },
      operation: jetUiDropOperation(event),
      items,
    });
    if (phase === "Drop") element._jetDropHandler?.(items);
  };
  element.addEventListener("dragenter", (event) => emit("Enter", event));
  element.addEventListener("dragover", (event) => emit("Over", event));
  element.addEventListener("dragleave", (event) => emit("Leave", event));
  element.addEventListener("drop", (event) => emit("Drop", event));
}

function jetUiWireIme(element, identity, enabled = true) {
  element._jetImeEnabled = Boolean(enabled);
  if (element._jetImeWired || typeof element.addEventListener !== "function") return;
  element._jetImeWired = true;
  const queue = globalThis.jet_ui_web_ime_queue;
  const selection = () => ({
    start: Number(element.selectionStart ?? 0),
    end: Number(element.selectionEnd ?? element.selectionStart ?? 0),
  });
  const emit = (phase, text) => {
    if (!element._jetImeEnabled) return;
    queue?.({
      target: { value: identity },
      phase: { tag: phase, values: [] },
      composition: {
        text: String(text ?? element.value ?? ""),
        selection: selection(),
        marked: { tag: "None", values: [] },
      },
    });
  };
  element.addEventListener("compositionstart", (event) => emit("Start", event.data));
  element.addEventListener("compositionupdate", (event) => emit("Update", event.data));
  element.addEventListener("compositionend", (event) => emit("Commit", event.data));
  element.addEventListener("input", () => emit("Commit", element.value));
}

export function paint(backend, node) {
  const perfStarted = perfNow();
  // Reactive reruns reuse the captured backend instead of calling
  // `createBackend()` again. Mark that stable key live for this render scope
  // before scope cleanup prunes untouched backends.
  jetDomTouchedBackends.add(backend.boxKey);
  const frame = backend.frame ?? { x: 0, y: 0, width: node.width, height: node.height };
  const live = new Set();
  const activeElement = typeof document !== "undefined"
    ? document.activeElement ?? null
    : null;
  const activeKey = activeElement?.dataset?.jetKey ?? null;
  const backendPrefix = `${backend.boxKey}/`;
  const activeInBackend = activeKey !== null
    && (activeKey === backend.boxKey || activeKey.startsWith(backendPrefix));
  const externalActive = activeElement !== null
    && activeElement !== document.body
    && activeKey === null;
  const focusedKey = (activeInBackend ? activeKey : null)
    ?? backend.focusNodes[backend.focusedIndex]?.dataset?.jetKey
    ?? null;
  backend.focusNodes = [];
  const render = (rawCurrent, currentFrame, path) => {
    const current = jetUiNodeProjection(rawCurrent);
    if (current.kind === "box") {
      let y = currentFrame.y;
      current.children.forEach((child, index) => {
        render(child, { x: currentFrame.x, y, width: currentFrame.width, height: child.height }, `${path}/${index}`);
        y += child.height;
      });
      return;
    }
    // D-UI-NODE-ID1=C: author key overrides render path.
    const identity = current.key != null && String(current.key) !== ""
      ? `key:${current.key}`
      : path;
    const fillColor = current.color ?? "#000000";
    if (current.kind !== "text") {
      backend.commands.push(`fill({x:${currentFrame.x},y:${currentFrame.y},w:${currentFrame.width},h:${currentFrame.height}}, ${fillColor})`);
    }
    backend.commands.push(`text({x:${currentFrame.x},y:${currentFrame.y},w:${currentFrame.width},h:${currentFrame.height}}, ${current.label})`);
    live.add(identity);
    if (!backend.root) return;
    const tag = current.kind === "button" ? "button" : current.kind === "textInput" ? "input" : current.kind === "text" ? "span" : "div";
    let record = jetDomBoxRegistry.get(identity);
    if (record && record.tag !== tag) {
      record.element?.remove?.();
      jetDomBoxRegistry.delete(identity);
      jetUiClickSlots.delete(identity);
      record = null;
    }
    let box = record?.element;
    if (!box) {
      box = document.createElement(tag);
      box.dataset.jetNode = "1";
      box.dataset.jetKey = identity;
      box.style.position = "absolute";
      box.style.boxSizing = "border-box";
      box.style.border = "1px solid rgba(0,0,0,0.15)";
      box.style.borderRadius = "6px";
      box.style.font = "14px system-ui, sans-serif";
      box.style.display = "flex";
      box.style.alignItems = "center";
      box.style.justifyContent = "center";
      backend.root.appendChild(box);
      jetDomBoxRegistry.set(identity, { element: box, tag });
    }
    box.dataset.jetKey = identity;
    box.style.left = `${currentFrame.x}px`;
    box.style.top = `${currentFrame.y}px`;
    box.style.width = `${currentFrame.width}px`;
    box.style.height = `${currentFrame.height}px`;
    box.style.background = current.kind === "text" ? "transparent" : fillColor;
    box.style.color = jetReadableTextColor(fillColor);
    if (tag === "input") box.value = current.label;
    else box.textContent = current.label;
    if (current.role) box.setAttribute?.("role", current.role);
    else box.removeAttribute?.("role");
    for (const attribute of ["aria-label", "aria-description", "aria-disabled", "aria-busy", "aria-expanded", "aria-checked", "aria-selected", "aria-required", "aria-valuetext"]) {
      box.removeAttribute?.(attribute);
    }
    jetUiAccessibilityProjection(box, current.accessibility, current.role ? current.label : "");
    const shortcutText = jetUiShortcutText(current.shortcut);
    if (shortcutText) {
      box.setAttribute?.("aria-keyshortcuts", shortcutText);
      box.dataset.jetShortcut = shortcutText;
    } else {
      box.removeAttribute?.("aria-keyshortcuts");
      delete box.dataset.jetShortcut;
    }
    if (current.role === "button" || current.role === "textbox") {
      backend.focusNodes.push(box);
    }
    if (tag === "input") jetUiWireIme(box, identity, jetUiImeEnabled(current.ime));
    jetUiWireDrop(box, identity, current.onDrop);
    // D-UI-EVT-DISP1=E / D-UI-EVT-SET1=D: portable click only. Rebind the slot
    // each paint so remounts keep one listener (no stacking).
    if (typeof current.onClick === "function") {
      jetUiClickSlots.set(identity, current.onClick);
      if (!box._jetClickWired) {
        box._jetClickWired = true;
        box.addEventListener?.("click", () => {
          jetUiDispatch(box.dataset?.jetKey ?? identity);
        });
      }
    } else {
      jetUiClickSlots.delete(identity);
    }
  };
  render(node, frame, backend.boxKey);
  if (backend.root) {
    const prefix = `${backend.boxKey}/`;
    for (const [key, record] of jetDomBoxRegistry) {
      // Path-keyed nodes belong to this backend. Key-keyed nodes stay until
      // the render scope exits (they can move across siblings in one frame).
      if ((key === backend.boxKey || key.startsWith(prefix)) && !live.has(key)) {
        record.element?.remove?.();
        jetDomBoxRegistry.delete(key);
        jetUiClickSlots.delete(key);
      }
    }
  }
  const preservedIndex = focusedKey === null
    ? -1
    : backend.focusNodes.findIndex((element) => element.dataset?.jetKey === focusedKey);
  backend.focusedIndex = preservedIndex >= 0
    ? preservedIndex
    : backend.focusNodes.length ? 0 : -1;
  if (backend.focusedIndex >= 0 && (activeInBackend || (!externalActive && activeKey === null))) {
    backend.focusNodes[backend.focusedIndex].focus?.();
  }
  perfRecord(jetDomScopeName, "dom", perfStarted);
  return backend;
}

export function commands(backend) {
  return backend.commands.slice();
}

export function onEvent(backend, event) {
  if (event.kind === "key" && event.code === "") {
    return "Ignored";
  }
  if (event.kind === "resize" && (event.width <= 0 || event.height <= 0)) {
    return "Ignored";
  }
  if (event.kind === "key" && event.code === "Tab" && backend.focusNodes.length) {
    backend.focusedIndex = (backend.focusedIndex + 1) % backend.focusNodes.length;
    backend.focusNodes[backend.focusedIndex].focus?.();
    return "Handled";
  }
  return "Handled";
}

export function setFocusGroup(backend, nodes) {
  const labels = nodes.filter((node) => node.role === "button" || node.role === "textbox").map((node) => node.label);
  backend.focusNodes = Array.from(backend.root?.children ?? []).filter((element) => labels.includes(element.getAttribute?.("aria-label") ?? element.textContent));
  backend.focusedIndex = backend.focusNodes.length ? 0 : -1;
  backend.focusNodes[0]?.focus?.();
}

export function focusedLabel(backend) {
  if (backend.focusedIndex < 0) return "";
  const element = backend.focusNodes[backend.focusedIndex];
  return String(element?.getAttribute?.("aria-label") ?? element?.textContent ?? "");
}

function query(selector) {
  if (typeof document === "undefined" || !document.querySelector) return null;
  return document.querySelector(String(selector));
}

function normalizeEvent(ev) {
  return {
    kind: String(ev?.type ?? ""),
    key: String(ev?.key ?? ""),
    code: String(ev?.code ?? ""),
    value: ev?.target && "value" in ev.target ? String(ev.target.value ?? "") : "",
    checked: !!(ev?.target && ev.target.checked),
  };
}

export function on(selector, eventName, handler, symbol) {
  const el = query(selector);
  if (!el || !el.addEventListener) return "Missing";
  const handlerSymbol = String(symbol);
  el.addEventListener(String(eventName), (ev) => {
    const started = perfNow();
    try {
      const result = handler(normalizeEvent(ev));
      if (result && typeof result.finally === "function") {
        return result.finally(() => perfRecord(handlerSymbol, "event", started));
      }
      perfRecord(handlerSymbol, "event", started);
      return result;
    } catch (error) {
      perfRecord(handlerSymbol, "event", started);
      throw error;
    }
  });
  return "Bound";
}

export function value(selector) {
  const el = query(selector);
  if (!el) return "";
  if ("value" in el) return String(el.value ?? "");
  return String(el.textContent ?? "");
}

const fallbackStorage = { local: new Map(), session: new Map() };

function storage(kind) {
  const name = kind === "session" ? "sessionStorage" : "localStorage";
  const candidate = globalThis?.[name];
  if (
    candidate &&
    typeof candidate.getItem === "function" &&
    typeof candidate.setItem === "function" &&
    typeof candidate.removeItem === "function"
  ) {
    return candidate;
  }
  const map = fallbackStorage[kind === "session" ? "session" : "local"];
  return {
    getItem(key) {
      key = String(key);
      return map.has(key) ? map.get(key) : null;
    },
    setItem(key, value) {
      map.set(String(key), String(value));
    },
    removeItem(key) {
      map.delete(String(key));
    },
    clear() {
      map.clear();
    },
  };
}

export function storageGet(kind, key) {
  const value = storage(kind).getItem(String(key));
  return value == null ? null : String(value);
}

export function storageSet(kind, key, value) {
  storage(kind).setItem(String(key), String(value));
  return null;
}

export function storageRemove(kind, key) {
  storage(kind).removeItem(String(key));
  return null;
}

export function storageClear(kind) {
  storage(kind).clear();
  return null;
}

export function makeNode(label, width, height, color) {
  return {
    kind: "custom",
    label: String(label),
    width: jetNum(width),
    height: jetNum(height),
    color: color != null ? String(color) : undefined,
    role: color != null ? "label" : null,
    children: [],
  };
}

export function makeNodeRole(label, width, height, role) {
  return { kind: role === "button" ? "button" : role === "textbox" ? "textInput" : "custom", label: String(label), width: jetNum(width), height: jetNum(height), role, children: [] };
}

export function makeText(text) {
  const label = String(text);
  return { kind: "text", label, width: Array.from(label).length, height: 1, role: "label", children: [] };
}

export function makeButton(text, onClick, key) {
  const label = String(text);
  return {
    kind: "button",
    label,
    width: Array.from(label).length + 4,
    height: 36,
    role: "button",
    children: [],
    onClick: typeof onClick === "function" ? onClick : null,
    key: key != null && String(key) !== "" ? String(key) : null,
  };
}

export function makeBox(children) {
  const list = Array.from(children ?? []);
  return {
    kind: "box",
    label: "",
    width: list.reduce((width, child) => Math.max(width, child.width), 0),
    height: list.reduce((height, child) => height + child.height, 0),
    role: "group",
    children: list,
  };
}

export function makeConstraint(minW, minH, maxW, maxH) {
  return { minWidth: minW, minHeight: minH, maxWidth: maxW, maxHeight: maxH };
}

export function makeRect(x, y, width, height) {
  return { x, y, width, height };
}

export function makeKeyEvent(code) {
  return { kind: "key", code: String(code) };
}

export function makeResizeEvent(width, height) {
  return { kind: "resize", width, height };
}

export function ariaRoleButton() { return "button"; }
export function ariaRoleTextInput() { return "textbox"; }
export function ariaRoleLabel() { return "label"; }
export function ariaRoleContainer() { return "group"; }

// D-RENDERTGT2=A carried into JS (Phase 7 web/DOM backend): a minimal
// reactive runtime mirroring the Rust `JetSignal`/`jet_reactive_effect`
// prelude (crates/jet-codegen/src/Prelude/CoreLib.rs). An "observer" is the
// closure currently (re)running; reading a signal while an observer is
// active subscribes it; `set` re-runs every subscriber synchronously.
const jetReactiveObservers = [];
const jetReactiveRootEffects = new Set();
let jetReactiveNextObserver = 1;

function jetReactiveActiveObserver() {
  return jetReactiveObservers.length > 0
    ? jetReactiveObservers[jetReactiveObservers.length - 1]
    : null;
}

/** D-RENDERTGT2=A: `reactive.signal(initial)` → a `{ get, set }` cell. */
export function makeSignal(initial) {
  const cell = { value: initial, subs: new Map() };
  return {
    get() {
      const obs = jetReactiveActiveObserver();
      if (obs && !cell.subs.has(obs.id)) {
        cell.subs.set(obs.id, new WeakRef(obs));
        obs.dependencies.add(cell);
      }
      return cell.value;
    },
    set(value) {
      cell.value = value;
      for (const [id, weak] of Array.from(cell.subs)) {
        const sub = weak.deref();
        if (sub) sub.run();
        else cell.subs.delete(id);
      }
    },
  };
}

/**
 * D-RENDERTGT2=A: `ui.reactive_render(() => { ... })` — run `body` now, and
 * again whenever a signal it read changes. Mirrors `jet_reactive_effect`.
 */
export function makeEffect(body) {
  const observer = {
    id: jetReactiveNextObserver++,
    active: true,
    running: false,
    body,
    dependencies: new Set(),
    run() {
      if (!this.active || this.running) return;
      this.running = true;
      for (const cell of this.dependencies) cell.subs.delete(this.id);
      this.dependencies.clear();
      jetReactiveObservers.push(this);
      try {
        this.body();
      } finally {
        jetReactiveObservers.pop();
        this.running = false;
      }
    },
  };
  observer.run();
  return {
    unsubscribe() {
      if (!observer.active) return;
      observer.active = false;
      for (const cell of observer.dependencies) cell.subs.delete(observer.id);
      observer.dependencies.clear();
      observer.body = null;
    },
    isActive() {
      return observer.active;
    },
  };
}

/** Runtime-owned rendering effect; public `reactive.effect` returns its handle. */
export function reactiveRender(body) {
  const scopeName = jetDomScopeName;
  // Re-enter the caller's DOM scope on every synchronous rerun so backend
  // keys restart at the same root and paint reconciles the existing nodes.
  jetReactiveRootEffects.add(makeEffect(() => {
    enterRenderScope(scopeName);
    try {
      body();
    } finally {
      exitRenderScope();
    }
  }));
}

export async function instantiateWasm(wasmPath, imports = {}) {
  if (typeof WebAssembly === "undefined") {
    throw new JetHostWasmError("WebAssembly is not available in this runtime");
  }
  const source = await loadBytes(wasmPath);
  const memory = { current: null };
  const wasmImports = {
    jet_web_print(ptr, len) {
      if (!memory.current) throw new JetHostWasmError("Wasm print called before memory was ready");
      const bytes = new Uint8Array(memory.current.buffer, Number(ptr), Number(len));
      if (typeof process !== "undefined" && process.versions?.node && process.stdout?.write) {
        process.stdout.write(bytes.slice());
      } else {
        print(new TextDecoder().decode(bytes));
      }
    },
    ...imports,
  };
  const { instance } = await WebAssembly.instantiate(source, { env: wasmImports });
  memory.current = instance.exports.memory ?? null;
  return instance;
}

const JET_ABI_U32_MAX = 0xffffffff;

function abiU32(value, label) {
  if (!Number.isInteger(value) || value < 0 || value > JET_ABI_U32_MAX) {
    throw new RangeError(`${label} exceeds u32`);
  }
  return value >>> 0;
}

function abiWasmU32(value, label) {
  if (!Number.isInteger(value) || value < -0x80000000 || value > JET_ABI_U32_MAX) {
    throw new RangeError(`${label} is not a Wasm i32/u32`);
  }
  return value >>> 0;
}

function abiAddU32(total, amount, label) {
  amount = abiU32(amount, label);
  if (total > JET_ABI_U32_MAX - amount) {
    throw new RangeError(`${label} exceeds u32`);
  }
  return total + amount;
}

/** D-JSBIND1=A: marshal ABI-safe values at the JS/WASM boundary.
 *  String params: TextEncoder → jet_abi_string_alloc → packed u64 (ptr<<32)|len.
 *  Int params: decimal UTF-8 ownership transfer on the String rail.
 *  [Int] params: count/length/decimal UTF-8 blob → jet_abi_list_int_alloc.
 *  fixed-width integer lists use the separate BigInt64Array `list-i64` rail.
 *  [String] params: contiguous LE [count][len][utf8]… → jet_abi_list_string_alloc.
 *  [String:Int] params: contiguous LE [count][key-len][utf8][value-len][decimal]… . */
export function marshalAbi(value, kind, wasm) {
  if (kind === "string") {
    const encoded = new TextEncoder().encode(String(value ?? ""));
    if (!ArrayBuffer.isView(encoded) || encoded.BYTES_PER_ELEMENT !== 1) {
      throw new TypeError("string ABI expects UTF-8 bytes");
    }
    if (encoded.length === 0) return 0n;
    abiU32(encoded.length, "string ABI byte length");
    const ptr = abiWasmU32(
      wasm.jet_abi_string_alloc(encoded.length),
      "string ABI allocation pointer",
    );
    try {
      new Uint8Array(wasm.memory.buffer, ptr, encoded.length).set(encoded);
      return (BigInt(ptr) << 32n) | BigInt(encoded.length);
    } catch (error) {
      wasm.jet_abi_string_free(ptr, encoded.length);
      throw error;
    }
  }
  if (kind === "int") {
    if (typeof value !== "bigint") {
      throw new TypeError("int ABI expects a BigInt");
    }
    return marshalAbi(value.toString(), "string", wasm);
  }
  if (kind === "list-int") {
    const arr = Array.isArray(value) ? value : [];
    if (arr.length === 0) return 0n;
    const enc = new TextEncoder();
    const parts = arr.map((raw) => {
      if (typeof raw !== "bigint") throw new TypeError("list-int ABI expects BigInt values");
      return enc.encode(raw.toString());
    });
    let byteLen = 4;
    for (const part of parts) byteLen = abiAddU32(byteLen, 4 + part.length, "list-int ABI blob length");
    const ptr = abiWasmU32(
      wasm.jet_abi_list_int_alloc(byteLen),
      "list-int ABI allocation pointer",
    );
    try {
      const bytes = new Uint8Array(wasm.memory.buffer, ptr, byteLen);
      const view = new DataView(wasm.memory.buffer, ptr, byteLen);
      view.setUint32(0, arr.length, true);
      let o = 4;
      for (const part of parts) {
        view.setUint32(o, part.length, true);
        o += 4;
        bytes.set(part, o);
        o += part.length;
      }
      return (BigInt(ptr) << 32n) | BigInt(byteLen);
    } catch (error) {
      wasm.jet_abi_list_int_free(ptr, byteLen);
      throw error;
    }
  }
  if (kind === "list-i64") {
    const arr = Array.isArray(value) ? value : [];
    if (arr.length === 0) return 0n;
    const ptr = wasm.jet_abi_list_i64_alloc(arr.length);
    try {
      const view = new BigInt64Array(wasm.memory.buffer, ptr, arr.length);
      for (let i = 0; i < arr.length; i++) {
        view[i] = BigInt(arr[i]);
      }
      return (BigInt(ptr) << 32n) | BigInt(arr.length);
    } catch (error) {
      wasm.jet_abi_list_i64_free(ptr, arr.length);
      throw error;
    }
  }
  if (kind === "list-string") {
    const arr = Array.isArray(value) ? value : [];
    if (arr.length === 0) return 0n;
    const enc = new TextEncoder();
    const parts = arr.map((s) => enc.encode(String(s ?? "")));
    let byteLen = 4;
    for (const p of parts) byteLen += 4 + p.length;
    const ptr = wasm.jet_abi_list_string_alloc(byteLen);
    try {
      const bytes = new Uint8Array(wasm.memory.buffer, ptr, byteLen);
      const view = new DataView(wasm.memory.buffer, ptr, byteLen);
      view.setUint32(0, arr.length, true);
      let o = 4;
      for (const p of parts) {
        view.setUint32(o, p.length, true);
        o += 4;
        bytes.set(p, o);
        o += p.length;
      }
      return (BigInt(ptr) << 32n) | BigInt(byteLen);
    } catch (error) {
      wasm.jet_abi_list_string_free(ptr, byteLen);
      throw error;
    }
  }
  if (kind === "map-string-int") {
    if (!(value instanceof Map)) {
      throw new TypeError("map-string-int ABI expects a Map");
    }
    const claimedCount = abiU32(value.size, "map-string-int ABI entry count");
    const enc = new TextEncoder();
    const entries = [];
    let byteLen = 4;
    for (const [key, raw] of value) {
      if (typeof key !== "string") {
        throw new TypeError("map-string-int ABI expects String keys");
      }
      if (typeof raw !== "bigint") {
        throw new TypeError("map-string-int ABI expects BigInt values");
      }
      const int = raw;
      const valueBytes = enc.encode(int.toString());
      const bytes = enc.encode(key);
      entries.push([bytes, valueBytes]);
      abiU32(bytes.length, "map-string-int ABI key length");
      byteLen = abiAddU32(byteLen, 4, "map-string-int ABI blob length");
      byteLen = abiAddU32(byteLen, bytes.length, "map-string-int ABI blob length");
      byteLen = abiAddU32(byteLen, 4, "map-string-int ABI blob length");
      byteLen = abiAddU32(byteLen, valueBytes.length, "map-string-int ABI blob length");
    }
    const count = abiU32(entries.length, "map-string-int ABI entry count");
    if (count !== claimedCount) {
      throw new TypeError("map-string-int ABI Map size/iterator mismatch");
    }
    if (count === 0) return 0n;
    const ptr = abiWasmU32(
      wasm.jet_abi_map_string_int_alloc(byteLen),
      "map-string-int ABI allocation pointer",
    );
    try {
      const bytes = new Uint8Array(wasm.memory.buffer, ptr, byteLen);
      const view = new DataView(wasm.memory.buffer, ptr, byteLen);
      view.setUint32(0, count, true);
      let o = 4;
      for (const [key, valueBytes] of entries) {
        view.setUint32(o, key.length, true);
        o += 4;
        bytes.set(key, o);
        o += key.length;
        view.setUint32(o, valueBytes.length, true);
        o += 4;
        bytes.set(valueBytes, o);
        o += valueBytes.length;
      }
      return (BigInt(ptr) << 32n) | BigInt(byteLen);
    } catch (error) {
      wasm.jet_abi_map_string_int_free(ptr, byteLen);
      throw error;
    }
  }
  if (kind === "struct-point") {
    return { x: Number(value?.x ?? 0), y: Number(value?.y ?? 0) };
  }
  return value;
}

/** D-JSBIND1=A: read ABI-safe return values from WASM.
 *  String returns are packed u64 (ptr<<32)|len; ownership frees via jet_abi_string_free.
 *  Int returns are packed UTF-8 strings and become JS BigInt values.
 *  [Int] returns are decimal UTF-8 blobs; fixed-width lists use BigInt64Array.
 *  [String] returns are packed u64 (ptr<<32)|byte_len; frees via jet_abi_list_string_free.
 *  [String:Int] returns are packed decimal UTF-8 blobs; frees via jet_abi_map_string_int_free. */
export function unmarshalAbi(value, kind, wasm) {
  if (kind === "string") {
    const packed = BigInt.asUintN(64, typeof value === "bigint" ? value : BigInt(value));
    const ptr = Number((packed >> 32n) & 0xffffffffn) >>> 0;
    const len = Number(packed & 0xffffffffn) >>> 0;
    let bytes;
    try {
      bytes = new Uint8Array(wasm.memory.buffer, ptr, len).slice();
    } finally {
      wasm.jet_abi_string_free(ptr, len);
    }
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  }
  if (kind === "int") {
    return BigInt(unmarshalAbi(value, "string", wasm));
  }
  if (kind === "list-int") {
    const packed = BigInt.asUintN(64, typeof value === "bigint" ? value : BigInt(value));
    const ptr = Number((packed >> 32n) & 0xffffffffn) >>> 0;
    const len = Number(packed & 0xffffffffn) >>> 0;
    if (len === 0) {
      if (ptr !== 0) wasm.jet_abi_list_int_free(ptr, 0);
      return [];
    }
    let out;
    try {
      const bytes = new Uint8Array(wasm.memory.buffer, ptr, len).slice();
      if (bytes.length < 4) throw new JetHostAbiError("invalid list-int ABI header");
      const view = new DataView(bytes.buffer);
      const count = view.getUint32(0, true);
      const dec = new TextDecoder("utf-8", { fatal: true });
      out = [];
      let o = 4;
      for (let i = 0; i < count; i++) {
        if (o + 4 > bytes.length) throw new JetHostAbiError("invalid list-int ABI length");
        const valueLen = view.getUint32(o, true);
        o += 4;
        if (valueLen > bytes.length - o) throw new JetHostAbiError("invalid list-int ABI value");
        out.push(BigInt(dec.decode(bytes.subarray(o, o + valueLen))));
        o += valueLen;
      }
      if (o !== bytes.length) throw new JetHostAbiError("invalid list-int ABI trailing bytes");
    } finally {
      wasm.jet_abi_list_int_free(ptr, len);
    }
    return out;
  }
  if (kind === "list-i64") {
    const packed = BigInt.asUintN(64, typeof value === "bigint" ? value : BigInt(value));
    const ptr = Number((packed >> 32n) & 0xffffffffn) >>> 0;
    const len = Number(packed & 0xffffffffn) >>> 0;
    let out;
    try {
      const view = new BigInt64Array(wasm.memory.buffer, ptr, len);
      out = Array.from(view, (x) => Number(x));
    } finally {
      wasm.jet_abi_list_i64_free(ptr, len);
    }
    return out;
  }
  if (kind === "list-string") {
    const packed = typeof value === "bigint" ? value : BigInt(value);
    const ptr = Number(packed >> 32n);
    const byteLen = Number(packed & 0xffffffffn);
    if (byteLen === 0) {
      if (ptr !== 0) wasm.jet_abi_list_string_free(ptr, 0);
      return [];
    }
    const bytes = new Uint8Array(wasm.memory.buffer, ptr, byteLen).slice();
    wasm.jet_abi_list_string_free(ptr, byteLen);
    const view = new DataView(bytes.buffer);
    const count = view.getUint32(0, true);
    const out = [];
    let o = 4;
    const dec = new TextDecoder("utf-8", { fatal: true });
    for (let i = 0; i < count; i++) {
      const len = view.getUint32(o, true);
      o += 4;
      out.push(dec.decode(bytes.subarray(o, o + len)));
      o += len;
    }
    return out;
  }
  if (kind === "map-string-int") {
    const packed = BigInt.asUintN(64, typeof value === "bigint" ? value : BigInt(value));
    const ptr = Number((packed >> 32n) & 0xffffffffn) >>> 0;
    const byteLen = Number(packed & 0xffffffffn) >>> 0;
    if (byteLen === 0) {
      if (ptr !== 0) wasm.jet_abi_map_string_int_free(ptr, 0);
      return new Map();
    }
    let bytes;
    try {
      bytes = new Uint8Array(wasm.memory.buffer, ptr, byteLen).slice();
    } finally {
      wasm.jet_abi_map_string_int_free(ptr, byteLen);
    }
    if (byteLen < 4) throw new JetHostAbiError("invalid map-string-int ABI header");
    const view = new DataView(bytes.buffer);
    const count = view.getUint32(0, true);
    const out = new Map();
    const dec = new TextDecoder("utf-8", { fatal: true });
    let o = 4;
    for (let i = 0; i < count; i++) {
      if (o + 4 > byteLen) throw new JetHostAbiError("invalid map-string-int ABI key length");
      const len = view.getUint32(o, true);
      o += 4;
      if (len > byteLen - o || byteLen - o - len < 4) {
        throw new JetHostAbiError("invalid map-string-int ABI entry");
      }
      const key = dec.decode(bytes.subarray(o, o + len));
      o += len;
      const valueLen = view.getUint32(o, true);
      o += 4;
      if (valueLen > byteLen - o) throw new JetHostAbiError("invalid map-string-int ABI value");
      out.set(key, BigInt(dec.decode(bytes.subarray(o, o + valueLen))));
      o += valueLen;
    }
    if (o !== byteLen) throw new JetHostAbiError("invalid map-string-int ABI trailing bytes");
    return out;
  }
  if (typeof value === "bigint") {
    return Number(value);
  }
  return value;
}

async function loadBytes(path) {
  if (typeof process !== "undefined" && process.versions?.node) {
    const fs = await import("node:fs");
    const source = typeof path === "string" && path.startsWith("file:")
      ? new URL(path)
      : path;
    return fs.readFileSync(source);
  }
  const response = await fetch(path);
  if (!response.ok) {
    throw new JetHostWasmError(`failed to load wasm module: ${path}`);
  }
  return response.arrayBuffer();
}

// D-DX-WEBPENDING1 / card #2478: the browser bridge transports typed stream
// identities only. The lifecycle kernel remains in Rust; this layer performs
// request framing and returns the host's receipt/projection without retaining
// a second state machine in JavaScript.
const JET_WEB_STREAM_ENDPOINT = "/__jet_web/stream";

function jetWebStreamString(value, field) {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`web stream ${field} must be a non-empty string`);
  }
  return value;
}
function jetWebStreamText(value, field) {
  if (typeof value !== "string") {
    throw new TypeError(`web stream ${field} must be a string`);
  }
  return value;
}
function jetWebStreamBool(value, field) {
  if (typeof value !== "boolean") {
    throw new TypeError(`web stream ${field} must be a boolean`);
  }
  return value;
}

function jetWebStreamU64(value, field) {
  const number = typeof value === "bigint" ? Number(value) : Number(value);
  if (!Number.isSafeInteger(number) || number < 0) {
    throw new TypeError(`web stream ${field} must be a non-negative safe integer`);
  }
  return number;
}

function jetWebStreamIdentity(identity) {
  if (!identity || typeof identity !== "object") {
    throw new TypeError("web stream identity must be an object");
  }
  return {
    island_id: jetWebStreamString(identity.island_id, "identity.island_id"),
    source_id: jetWebStreamString(identity.source_id, "identity.source_id"),
    build_id: jetWebStreamString(identity.build_id, "identity.build_id"),
    revision: jetWebStreamString(identity.revision, "identity.revision"),
  };
}

function jetWebStreamId(streamId) {
  if (!streamId || typeof streamId !== "object") {
    throw new TypeError("web stream stream_id must be an object");
  }
  return {
    boundary_id: jetWebStreamString(streamId.boundary_id, "stream_id.boundary_id"),
    generation: jetWebStreamU64(streamId.generation, "stream_id.generation"),
    identity: jetWebStreamIdentity(streamId.identity),
  };
}

async function jetWebStreamRequest(action, payload) {
  if (typeof fetch !== "function") {
    throw new JetHostWasmError("web stream host bridge requires fetch");
  }
  const session = jetWebStreamString(globalThis.__jetDevSession, "session");
  const response = await fetch(
    `${JET_WEB_STREAM_ENDPOINT}?session=${encodeURIComponent(session)}`,
    {
      method: "POST",
      cache: "no-store",
      headers: { "content-type": "application/json" },
      body: jetWebJsonStringify({ ...payload, action: jetWebStreamString(action, "action") }),
    },
  );
  const text = await response.text();
  let body;
  try {
    body = globalThis.__jetWebJsonParse(text);
  } catch (error) {
    throw new JetHostWasmError("web stream host returned malformed JSON", error);
  }
  if (!response.ok || body?.ok !== true) {
    throw new JetHostWasmError(
      body?.error?.message || `web stream ${action} rejected`,
    );
  }
  return body;
}

export function webStreamRegister(boundaryId, identity) {
  return jetWebStreamRequest("register", {
    boundary_id: jetWebStreamString(boundaryId, "boundary_id"),
    identity: jetWebStreamIdentity(identity),
  });
}

export function webStreamBegin(boundaryId, identity, atNs) {
  return jetWebStreamRequest("begin", {
    boundary_id: jetWebStreamString(boundaryId, "boundary_id"),
    identity: jetWebStreamIdentity(identity),
    at_ns: jetWebStreamU64(atNs, "at_ns"),
  });
}

export function webStreamChunk(streamId, sequence, content, isFinal, atNs) {
  return jetWebStreamRequest("chunk", {
    stream_id: jetWebStreamId(streamId),
    sequence: jetWebStreamU64(sequence, "sequence"),
    content: jetWebStreamText(content, "content"),
    is_final: jetWebStreamBool(isFinal, "is_final"),
    at_ns: jetWebStreamU64(atNs, "at_ns"),
  });
}

export function webStreamCommit(streamId, atNs) {
  return jetWebStreamRequest("commit", {
    stream_id: jetWebStreamId(streamId),
    at_ns: jetWebStreamU64(atNs, "at_ns"),
  });
}

export function webStreamFail(streamId, error, atNs) {
  if (!error || typeof error !== "object") {
    throw new TypeError("web stream error must be an object");
  }
  return jetWebStreamRequest("fail", {
    stream_id: jetWebStreamId(streamId),
    error: { ...error },
    at_ns: jetWebStreamU64(atNs, "at_ns"),
  });
}


export function webStreamRollback(streamId, atNs) {
  return jetWebStreamRequest("rollback", {
    stream_id: jetWebStreamId(streamId),
    at_ns: jetWebStreamU64(atNs, "at_ns"),
  });
}

export function webStreamHydrationStart(streamId, trigger, startedAtNs) {
  const hydrationTrigger = jetWebStreamString(trigger, "trigger");
  if (!["immediate", "client-load", "client-idle", "client-visible", "manual"].includes(hydrationTrigger)) {
    throw new TypeError(`web stream trigger ${hydrationTrigger} is not recognized`);
  }
  return jetWebStreamRequest("hydration_start", {
    stream_id: jetWebStreamId(streamId),
    trigger: hydrationTrigger,
    started_at_ns: jetWebStreamU64(startedAtNs, "started_at_ns"),
  });
}

export function webStreamHydrationComplete(streamId, completedAtNs) {
  return jetWebStreamRequest("hydration_complete", {
    stream_id: jetWebStreamId(streamId),
    completed_at_ns: jetWebStreamU64(completedAtNs, "completed_at_ns"),
  });
}

export function webStreamReceipt(boundaryId) {
  return jetWebStreamRequest("receipt", {
    boundary_id: jetWebStreamString(boundaryId, "boundary_id"),
  });
}

export function webStreamProjection(boundaryId, target) {
  const projectionTarget = jetWebStreamString(target, "target");
  if (projectionTarget !== "server" && projectionTarget !== "client") {
    throw new TypeError(`web stream target ${projectionTarget} is not recognized`);
  }
  return jetWebStreamRequest("projection", {
    boundary_id: jetWebStreamString(boundaryId, "boundary_id"),
    target: projectionTarget,
  });
}
// D-DX-SERVERFN1=A: scripted calls and native forms share the App-owned
// action endpoint. This adapter does not keep a second RPC registry or copy
// handler state; it only frames JSON/form requests and exposes transport facts.
function jetWebServerFnBoundary(boundary, options = {}) {
  const input = typeof boundary === "string" ? { endpoint: boundary } : boundary;
  if (!input || typeof input !== "object") {
    throw jetWebServerFnError("invalid_definition", "server function boundary must be an object or endpoint", "request-unknown");
  }
  const endpoint = String(input.endpoint ?? "").trim();
  if (!endpoint.startsWith("/")) {
    throw jetWebServerFnError("csrf_rejected", "server function endpoint must be same-origin and start with /", "request-unknown");
  }
  const method = String(options.method ?? input.method ?? "POST").toUpperCase();
  if (!["GET", "POST", "PUT", "PATCH", "DELETE"].includes(method)) {
    throw jetWebServerFnError("invalid_definition", `server function method ${method} is not supported`, "request-unknown");
  }
  const maxAttempts = Number(options.maxAttempts ?? 1);
  const timeoutMs = options.timeoutMs == null ? null : Number(options.timeoutMs);
  if (!Number.isSafeInteger(maxAttempts) || maxAttempts < 1 || !Number.isFinite(timeoutMs ?? 1) || (timeoutMs != null && timeoutMs < 1)) {
    throw jetWebServerFnError("invalid_definition", "server function retry and timeout options must be positive finite numbers", "request-unknown");
  }
  return {
    ...input,
    endpoint,
    method,
    name: String(input.name ?? endpoint.slice("/actions/".length)),
    retry: options.retry ?? input.retry ?? "none",
    maxAttempts: Math.min(8, maxAttempts),
    timeoutMs,
    idempotencyKey: options.idempotencyKey == null ? null : String(options.idempotencyKey),
    csrfToken: options.csrfToken == null ? null : String(options.csrfToken),
    revalidate: Array.isArray(input.revalidate) ? input.revalidate.slice() : [],
  };
}

function jetWebServerFnError(code, message, requestId, extra = {}) {
  const error = new Error(String(message || "server function request failed"));
  error.name = "JetServerFunctionError";
  error.code = String(code || "transport_error");
  error.request_id = String(requestId || "request-unknown");
  Object.assign(error, extra);
  return error;
}

function jetWebServerFnRequestId() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `jet-sfn-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function jetWebServerFnSameOrigin(endpoint) {
  if (typeof location === "undefined" || !location.origin) return true;
  return new URL(endpoint, location.origin).origin === location.origin;
}

function jetWebServerFnRetryable(boundary, error, attempt) {
  if (attempt >= boundary.maxAttempts) return false;
  if (!["transport_error", "timed_out"].includes(error?.code)) return false;
  if (boundary.retry === "safe") return !["POST", "PUT", "PATCH", "DELETE"].includes(boundary.method);
  if (boundary.retry === "idempotent") {
    return Boolean(boundary.idempotencyKey) && boundary.idempotency === true;
  }
  return false;
}

function jetWebServerFnAbortSignal(signal, timeoutMs) {
  const controller = new AbortController();
  let timer = null;
  if (timeoutMs != null) timer = setTimeout(() => controller.abort("timeout"), timeoutMs);
  const abort = () => controller.abort(signal?.reason ?? "cancelled");
  if (signal) {
    if (signal.aborted) abort();
    else signal.addEventListener("abort", abort, { once: true });
  }
  return {
    signal: controller.signal,
    cleanup() {
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
    },
  };
}

async function jetWebServerFnResponse(response, requestId) {
  const text = await response.text();
  let body;
  try {
    body = text ? globalThis.__jetWebJsonParse(text) : {};
  } catch (cause) {
    throw jetWebServerFnError("invalid_output", "server function returned malformed JSON", requestId, { cause });
  }
  if (!response.ok) {
    const detail = body && typeof body === "object" ? body : {};
    throw jetWebServerFnError(
      detail.code ?? `http_${response.status}`,
      detail.message ?? "server function returned an error",
      detail.request_id ?? requestId,
      { status: response.status, detail },
    );
  }
  return { value: body, request_id: requestId, status: response.status };
}

export async function webServerFunctionCall(boundary, input, options = {}) {
  const config = jetWebServerFnBoundary(boundary, options);
  if (!jetWebServerFnSameOrigin(config.endpoint)) {
    throw jetWebServerFnError("csrf_rejected", "server function endpoint is not same-origin", "request-unknown");
  }
  if (typeof fetch !== "function") {
    throw jetWebServerFnError("transport_error", "server function host requires fetch", "request-unknown");
  }
  if (config.retry === "idempotent" && (!config.idempotencyKey || config.idempotency !== true)) {
    throw jetWebServerFnError(
      "invalid_definition",
      "idempotent retries require a declared idempotency key",
      "request-unknown",
    );
  }
  let body;
  try {
    body = jetWebJsonStringify(input ?? {});
  } catch (cause) {
    throw jetWebServerFnError(
      "invalid_input",
      "server function input is not JSON serializable",
      "request-unknown",
      { cause },
    );
  }
  let lastError = null;
  for (let attempt = 1; attempt <= config.maxAttempts; attempt += 1) {
    const requestId = jetWebServerFnRequestId();
    const abort = jetWebServerFnAbortSignal(options.signal, config.timeoutMs);
    const headers = {
      "content-type": "application/json",
      "x-request-id": requestId,
    };
    if (config.csrfToken) headers["x-csrf-token"] = config.csrfToken;
    if (config.idempotencyKey) headers["idempotency-key"] = config.idempotencyKey;
    let response;
    try {
      response = await fetch(config.endpoint, {
        method: config.method,
        mode: "same-origin",
        credentials: "same-origin",
        cache: "no-store",
        headers,
        body,
        signal: abort.signal,
      });
      const result = await jetWebServerFnResponse(response, requestId);
      abort.cleanup();
      return {
        ...result,
        attempts: attempt,
        endpoint: config.endpoint,
        revalidate: config.revalidate,
      };
    } catch (cause) {
      abort.cleanup();
      const timedOut = abort.signal.aborted && abort.signal.reason === "timeout";
      const cancelled = abort.signal.aborted && !timedOut;
      const error = cause?.name === "JetServerFunctionError"
        ? cause
        : jetWebServerFnError(
            cancelled ? "cancelled" : timedOut ? "uncertain_completion" : "transport_error",
            cancelled
              ? "server function call was cancelled"
              : timedOut
                ? "the server may have completed the call"
                : "server function transport failed",
            requestId,
            { cause, uncertain_completion: timedOut },
          );
      if (!jetWebServerFnRetryable(config, error, attempt) || error.code === "uncertain_completion") {
        throw error;
      }
      lastError = error;
    }
  }
  throw lastError || jetWebServerFnError("retry_exhausted", "server function retry policy exhausted", "request-unknown");
}

export function webServerFunctionForm(boundary, form, options = {}) {
  const config = jetWebServerFnBoundary(boundary, options);
  if (!form || typeof form.setAttribute !== "function") {
    throw jetWebServerFnError("invalid_definition", "server function form requires a form element", "request-unknown");
  }
  if (!["GET", "POST"].includes(config.method)) {
    throw jetWebServerFnError("invalid_definition", "native server function forms support only GET and POST", "request-unknown");
  }
  if (!jetWebServerFnSameOrigin(config.endpoint)) {
    throw jetWebServerFnError("csrf_rejected", "server function endpoint is not same-origin", "request-unknown");
  }
  form.setAttribute("action", config.endpoint);
  form.setAttribute("method", config.method);
  form.setAttribute("enctype", "application/x-www-form-urlencoded");
  form.dataset.jetServerFunction = config.name;
  form.dataset.jetPending = "false";
  if (config.csrfToken) {
    let token = form.querySelector('input[name="__jet_csrf"]');
    if (!token) {
      token = document.createElement("input");
      token.type = "hidden";
      token.name = "__jet_csrf";
      form.appendChild(token);
    }
    token.value = config.csrfToken;
  }
  return {
    action: config.endpoint,
    method: config.method,
    pending: false,
    revalidate: config.revalidate,
    no_script: true,
  };
}

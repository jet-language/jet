// Jet VS Code extension — LSP v0 client (M6 phase 4). Plain JS, no build step.
//
// Server discovery, in order:
//   1. jet.executablePath setting (supports ${workspaceFolder} and ~)
//   2. legacy jet.languageServerPath setting
//   3. <workspaceFolder>/target/debug/jet   (developing the compiler itself, trusted workspaces only)
//   4. `jet` on PATH                        (installed, or editor launched from dev shell)
// `jet self lsp` does not invoke rustc, so the plain cargo binary is enough.

const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

/** @type {LanguageClient | undefined} */
let client;
/** @type {vscode.WebviewPanel | undefined} */
let reasoningPanel;
/** @type {{
 *   panel: vscode.WebviewPanel,
 *   uri: string,
 *   selection: string,
 *   expanded: boolean,
 *   projection: object,
 *   documentVersion: number,
 *   stale: boolean,
 *   pinned: boolean
 * } | undefined} */
let reasoningPanelState;

function expandPathSetting(value, workspaceFolder) {
  return value
    .replace(/\$\{workspaceFolder\}/g, workspaceFolder)
    .replace(/\$\{workspaceRoot\}/g, workspaceFolder)
    .replace(/^~(?=$|\/|\\)/, process.env.HOME || "~");
}

function findServer(workspaceFolder) {
  // VS Code normally skips activation in an untrusted workspace, but keep
  // this boundary in the extension too: no workspace-controlled setting,
  // binary, or PATH command may become a process-launch identity.
  if (!vscode.workspace.isTrusted) {
    return undefined;
  }
  const settings = vscode.workspace.getConfiguration("jet");
  // Workspace settings and workspace binaries are both untrusted until the
  // editor has crossed its workspace-trust boundary.
  const explicit = vscode.workspace.isTrusted ? settings.get("executablePath", "") : "";
  const legacy = vscode.workspace.isTrusted ? settings.get("languageServerPath", "") : "";
  const configured = explicit || legacy;
  const configuredName = explicit ? "jet.executablePath" : "jet.languageServerPath";

  if (configured) {
    const expanded = expandPathSetting(configured, workspaceFolder || "");
    if (fs.existsSync(expanded)) {
      return expanded;
    }
    vscode.window.showWarningMessage(
      `${configuredName} is set to "${expanded}" but nothing exists there; falling back to auto-discovery.`
    );
  }

  if (vscode.workspace.isTrusted && workspaceFolder) {
    const debugBin = path.join(workspaceFolder, "target", "debug", "jet");
    if (fs.existsSync(debugBin)) {
      return debugBin;
    }
  }

  // Bare command: the OS resolves it from PATH when the server spawns.
  return "jet";
}

function canonicalProgram(program) {
  const absolute = path.resolve(program);
  return fs.existsSync(absolute) ? fs.realpathSync(absolute) : absolute;
}

function debugSourceForConfiguration(configuration) {
  if (configuration.request !== "attach") {
    return canonicalProgram(configuration.program);
  }
  if (!configuration.map) {
    throw new Error("Jet attach needs the existing `map` sidecar to identify its .jet source.");
  }
  let map;
  const mapPath = canonicalProgram(configuration.map);
  try {
    map = JSON.parse(fs.readFileSync(mapPath, "utf8"));
  } catch (error) {
    throw new Error(`Jet attach cannot read its verified map sidecar: ${error.message}`);
  }
  if (typeof map.jet_file !== "string" || !map.jet_file) {
    throw new Error("Jet attach map sidecar does not identify a Jet source file.");
  }
  // The sidecar records the source path used by the build. Relative paths are
  // relative to the sidecar, not to the editor process's current directory.
  const source = path.isAbsolute(map.jet_file)
    ? map.jet_file
    : path.resolve(path.dirname(mapPath), map.jet_file);
  return canonicalProgram(source);
}

function uriArgToPath(uriArg) {
  if (typeof uriArg === "string" && uriArg.startsWith("file:")) {
    return vscode.Uri.parse(uriArg).fsPath;
  }
  return vscode.window.activeTextEditor?.document.uri.fsPath;
}

function runJetInTerminal(serverPath, args) {
  if (!serverPath || !vscode.workspace.isTrusted) {
    vscode.window.showErrorMessage("Jet commands require a trusted workspace.");
    return;
  }
  const terminal = vscode.window.createTerminal({
    name: "Jet",
    shellPath: serverPath,
    shellArgs: args,
  });
  terminal.show();
}

function debugFile(file) {
  if (!debuggingIsAllowed()) {
    return;
  }
  const uri = vscode.Uri.file(canonicalProgram(file));
  const folder = vscode.workspace.getWorkspaceFolder(uri);
  vscode.debug.startDebugging(folder, {
    type: "jet",
    request: "launch",
    name: "Jet: Debug File",
    program: canonicalProgram(file),
  });
}

function htmlEscape(value) {
  return String(value).replace(/[&<>"']/g, (character) => {
    const entities = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return entities[character];
  });
}

function textValue(value) {
  return typeof value === "string" ? value : "";
}

function jsonForDisplay(value) {
  try {
    const json = JSON.stringify(value, null, 2);
    return json === undefined ? "unavailable" : json;
  } catch (_error) {
    return "unavailable";
  }
}

function reasoningStringList(value) {
  return Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
}

function reasoningSpan(record) {
  const span = record && typeof record === "object" ? record.source_span : undefined;
  if (
    !span ||
    !Number.isSafeInteger(span.start) ||
    !Number.isSafeInteger(span.end) ||
    span.start < 0 ||
    span.end < span.start
  ) {
    return undefined;
  }
  return { start: span.start, end: span.end };
}

function reasoningIdentity(record) {
  const identity = record && typeof record.identity === "object" ? record.identity : {};
  return {
    source: textValue(identity.source),
    build: textValue(identity.build),
    run: textValue(identity.run),
    target: textValue(identity.target),
  };
}

function displayFact(value, unavailable = "Unavailable in this projection") {
  const text = textValue(value);
  return text ? htmlEscape(text) : `<span class="empty">${htmlEscape(unavailable)}</span>`;
}

function displayStringList(values, empty = "None supplied by producer") {
  if (!values.length) {
    return `<p class="empty">${htmlEscape(empty)}</p>`;
  }
  return `<ul class="fact-list">${values.map((value) => `<li>${htmlEscape(value)}</li>`).join("")}</ul>`;
}

function displayObservation(observation) {
  if (!observation || typeof observation !== "object") {
    return `<p class="empty">Unavailable in this projection</p>`;
  }
  const rows = [
    ["Event", observation.event],
    ["Counterexample", observation.counterexample],
  ]
    .filter(([, value]) => typeof value === "string" && value.length > 0)
    .map(
      ([label, value]) =>
        `<div class="fact-row"><dt>${htmlEscape(label)}</dt><dd>${htmlEscape(value)}</dd></div>`
    )
    .join("");
  return rows || `<p class="empty">Unavailable in this projection</p>`;
}

function reasoningRecordCard(record, index) {
  const id = textValue(record.id);
  const claim = textValue(record.claim) || "Checked fact unavailable";
  const subject = textValue(record.subject);
  const disposition = textValue(record.disposition) || "Unavailable";
  const identity = reasoningIdentity(record);
  const span = reasoningSpan(record);
  const sourceHint = span ? `byte ${span.start}–${span.end}` : "Source span unavailable";
  const sourceAction = id
    ? `<button type="button" class="quiet-button" data-action="open-source" data-record-id="${htmlEscape(
        id
      )}">${span ? "Open source span" : "Open source file"}</button>`
    : "";
  const sourceSpan = span
    ? `byte ${span.start}–${span.end}`
    : `<span class="empty">Unavailable in this projection</span>`;
  const payload =
    record.payload === null || record.payload === undefined
      ? `<p class="empty">Unavailable in this projection</p>`
      : `<pre class="payload">${htmlEscape(jsonForDisplay(record.payload))}</pre>`;
  return `<details class="fact-card"${index === 0 ? " open" : ""} id="fact-${index}">
<summary>
  <span class="fact-index">${String(index + 1).padStart(2, "0")}</span>
  <span class="fact-title">${htmlEscape(claim)}</span>
  <span class="source-hint">${htmlEscape(sourceHint)}</span>
  <span class="badge">${htmlEscape(disposition)}</span>
</summary>
<div class="fact-body">
  <div class="fact-actions">${sourceAction}</div>
  <dl class="fact-grid">
    <div class="fact-row"><dt>Subject</dt><dd>${displayFact(subject)}</dd></div>
    <div class="fact-row"><dt>Source span</dt><dd>${sourceSpan}</dd></div>
    <div class="fact-row"><dt>Producer</dt><dd>${displayFact(record.producer)}</dd></div>
    <div class="fact-row"><dt>Method</dt><dd>${displayFact(record.method)}</dd></div>
    <div class="fact-row"><dt>Rule</dt><dd>${displayFact(record.rule)}</dd></div>
  </dl>
  <section class="fact-section" aria-labelledby="premises-${index}">
    <h3 id="premises-${index}">Premises</h3>
    ${displayStringList(reasoningStringList(record.premises))}
  </section>
  <section class="fact-section" aria-labelledby="assumptions-${index}">
    <h3 id="assumptions-${index}">Assumptions</h3>
    ${displayStringList(reasoningStringList(record.assumptions))}
  </section>
  <section class="fact-section" aria-labelledby="identity-${index}">
    <h3 id="identity-${index}">Source and run identity</h3>
    <dl class="fact-grid">
      <div class="fact-row"><dt>Source</dt><dd>${displayFact(identity.source)}</dd></div>
      <div class="fact-row"><dt>Build</dt><dd>${displayFact(identity.build)}</dd></div>
      <div class="fact-row"><dt>Run</dt><dd>${displayFact(identity.run)}</dd></div>
      <div class="fact-row"><dt>Target</dt><dd>${displayFact(identity.target)}</dd></div>
    </dl>
  </section>
  <section class="fact-section" aria-labelledby="observation-${index}">
    <h3 id="observation-${index}">Producer observation</h3>
    <dl class="fact-grid">${displayObservation(record.observation)}</dl>
  </section>
  <details class="payload-details">
    <summary>Producer payload</summary>
    ${payload}
  </details>
</div>
</details>`;
}

function reasoningLensCards(lenses, recordIndexes) {
  return lenses
    .filter((lens) => lens && typeof lens === "object" && typeof lens.name === "string")
    .map((lens) => {
      const ids = Array.isArray(lens.records)
        ? lens.records.filter((id) => typeof id === "string" && recordIndexes.has(id))
        : [];
      if (!ids.length) {
        return "";
      }
      const links = ids
        .map(
          (id) =>
            `<li><a href="#fact-${recordIndexes.get(id)}"><code>${htmlEscape(id)}</code></a></li>`
        )
        .join("");
      return `<section class="lens-card">
<h3>${htmlEscape(lens.name)}</h3>
<p class="lens-count">${ids.length} checked fact${ids.length === 1 ? "" : "s"}</p>
<ul class="fact-list">${links}</ul>
</section>`;
    })
    .filter(Boolean)
    .join("");
}

function reasoningUnavailableHtml(projection, state) {
  const nonce = crypto.randomBytes(16).toString("hex");
  const kind =
    projection && typeof projection.kind === "string" ? projection.kind : "unavailable";
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
<title>Jet Reasoning</title>
<style>
body { color: var(--vscode-foreground); background: var(--vscode-editor-background); font: var(--vscode-font-size) var(--vscode-font-family); line-height: 1.5; margin: 0 auto; max-width: 60rem; padding: 1.5rem; }
.empty-state { border: 1px solid var(--vscode-panel-border); padding: 1rem; }
.empty { color: var(--vscode-descriptionForeground); }
button { background: var(--vscode-button-background); border: 0; color: var(--vscode-button-foreground); cursor: pointer; padding: .45rem .7rem; }
button:hover { background: var(--vscode-button-hoverBackground); }
button:focus-visible, a:focus-visible, summary:focus-visible { outline: 2px solid var(--vscode-focusBorder); outline-offset: 2px; }
</style>
</head>
<body>
<h1>Jet reasoning</h1>
<section class="empty-state" role="status">
  <h2>Projection unavailable</h2>
  <p>The language server did not return a <code>jet.reasoning/v1</code> projection.</p>
  <p class="empty">Received kind: <code>${htmlEscape(kind)}</code></p>
  <button type="button" data-action="refresh">Refresh</button>
</section>
<script nonce="${nonce}">
const api = acquireVsCodeApi();
document.addEventListener('click', (event) => {
  const target = event.target instanceof Element ? event.target.closest('[data-action]') : null;
  if (target && target.getAttribute('data-action') === 'refresh') api.postMessage({ type: 'refresh' });
});
</script>
</body>
</html>`;
}

function reasoningHtml(projection, state = {}) {
  if (!projection || projection.kind !== "jet.reasoning/v1") {
    return reasoningUnavailableHtml(projection, state);
  }
  const records = Array.isArray(projection.records)
    ? projection.records.filter((record) => record && typeof record === "object")
    : [];
  const lenses = Array.isArray(projection.lenses) ? projection.lenses : [];
  const recordIndexes = new Map(
    records
      .map((record, index) => [textValue(record.id), index])
      .filter(([id]) => id.length > 0)
  );
  const recordCards = records.length
    ? records.map((record, index) => reasoningRecordCard(record, index)).join("")
    : `<p class="empty-state" role="status">No checked facts were returned for this selection.</p>`;
  const lensCards =
    reasoningLensCards(lenses, recordIndexes) ||
    `<p class="empty-state" role="status">No lens records were returned by the producer.</p>`;
  const limits = projection.limits && typeof projection.limits === "object" ? projection.limits : {};
  const truncated =
    limits.records_truncated === true
      ? `<p class="warning" role="status">The server limited this projection. Some checked facts are not shown.</p>`
      : "";
  const expandedLabel =
    limits.expanded === true
      ? "Expanded projection"
      : limits.expanded === false
        ? "Summary projection"
        : "Expansion state unavailable";
  const serialized = htmlEscape(jsonForDisplay(projection));
  const nonce = crypto.randomBytes(16).toString("hex");
  const source = textValue(projection.source);
  const selection = textValue(projection.selection);
  const stale = state.stale === true;
  const pinLabel = state.pinned === true ? "Unpin panel" : "Pin panel";
  const status = stale
    ? "Source changed since these facts were checked. Refresh to load current facts."
    : "Static checked facts. This view does not execute Jet code.";
  const sourceButton = state.uri
    ? `<button type="button" class="toolbar-button" data-action="open-source-file">Open source</button>`
    : "";
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
<title>Jet Reasoning</title>
<style>
:root { color-scheme: light dark; }
body { color: var(--vscode-foreground); background: var(--vscode-editor-background); font: var(--vscode-font-size) var(--vscode-font-family); line-height: 1.5; margin: 0 auto; max-width: 72rem; padding: 1.25rem 1.5rem 3rem; }
h1, h2, h3 { line-height: 1.2; }
h1 { font-size: 1.5rem; margin: 0; }
h2 { font-size: 1.1rem; margin: 1.6rem 0 .6rem; }
h3 { color: var(--vscode-descriptionForeground); font-size: .85rem; font-weight: 600; letter-spacing: .02em; margin: 0 0 .45rem; text-transform: uppercase; }
a { color: var(--vscode-textLink-foreground); }
code, pre { font-family: var(--vscode-editor-font-family); }
code { overflow-wrap: anywhere; }
pre { background: var(--vscode-textCodeBlock-background); margin: .5rem 0 0; overflow: auto; padding: .75rem; white-space: pre-wrap; overflow-wrap: anywhere; }
button { border: 0; cursor: pointer; font: inherit; }
button:focus-visible, a:focus-visible, summary:focus-visible { outline: 2px solid var(--vscode-focusBorder); outline-offset: 2px; }
.topbar { align-items: center; display: flex; flex-wrap: wrap; gap: .7rem; justify-content: space-between; }
.toolbar { display: flex; flex-wrap: wrap; gap: .45rem; }
.toolbar-button, .quiet-button { background: var(--vscode-button-background); color: var(--vscode-button-foreground); padding: .4rem .65rem; }
.toolbar-button:hover, .quiet-button:hover { background: var(--vscode-button-hoverBackground); }
.quiet-button { background: var(--vscode-textLink-foreground); color: var(--vscode-editor-background); font-size: .9em; }
.status { border-left: 3px solid var(--vscode-testing-iconPassed); margin: 1rem 0; padding: .55rem .75rem; }
.status.stale, .warning { border-left-color: var(--vscode-editorWarning-foreground); color: var(--vscode-editorWarning-foreground); }
.warning { border-left: 3px solid; margin: .8rem 0; padding: .55rem .75rem; }
.meta { background: var(--vscode-editorWidget-background); border: 1px solid var(--vscode-panel-border); display: grid; gap: .6rem 1.25rem; grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr)); margin: 1rem 0; padding: .85rem 1rem; }
.meta-item { min-width: 0; }
.meta-label { color: var(--vscode-descriptionForeground); display: block; font-size: .78em; letter-spacing: .04em; text-transform: uppercase; }
.meta-value { display: block; overflow-wrap: anywhere; }
.truth { display: flex; flex-wrap: wrap; gap: .45rem; margin: 1.25rem 0 .25rem; }
.tag, .badge { background: var(--vscode-textBlockQuote-background); border: 1px solid var(--vscode-textBlockQuote-border); color: var(--vscode-descriptionForeground); display: inline-block; font-size: .78em; padding: .15rem .45rem; }
.badge { background: var(--vscode-badge-background); border: 0; color: var(--vscode-badge-foreground); white-space: nowrap; }
.empty, .lens-count { color: var(--vscode-descriptionForeground); }
.empty-state { border: 1px solid var(--vscode-panel-border); color: var(--vscode-descriptionForeground); padding: 1rem; }
.lens-grid { display: grid; gap: .75rem; grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr)); }
.lens-card { background: var(--vscode-editorWidget-background); border: 1px solid var(--vscode-panel-border); padding: .75rem; }
.lens-card h3 { color: var(--vscode-foreground); text-transform: none; }
.lens-count { margin: 0 0 .45rem; }
.fact-list { list-style: none; margin: .4rem 0 0; padding: 0; }
.fact-list li { margin: .25rem 0; overflow-wrap: anywhere; }
.facts { margin-top: .75rem; }
.source-hint { color: var(--vscode-descriptionForeground); font-family: var(--vscode-editor-font-family); font-size: .78em; overflow-wrap: anywhere; }
.fact-card, .raw-details { border: 1px solid var(--vscode-panel-border); margin: .65rem 0; }
.fact-card[open], .raw-details[open] { border-color: var(--vscode-focusBorder); }
summary { align-items: center; cursor: pointer; display: flex; gap: .65rem; list-style-position: inside; padding: .7rem .8rem; }
summary::-webkit-details-marker { color: var(--vscode-descriptionForeground); }
.fact-index { color: var(--vscode-descriptionForeground); font-family: var(--vscode-editor-font-family); font-size: .85em; }
.fact-title { flex: 1; font-weight: 600; overflow-wrap: anywhere; }
.fact-body { border-top: 1px solid var(--vscode-panel-border); padding: .8rem; }
.fact-actions { display: flex; gap: .45rem; justify-content: flex-end; margin-bottom: .7rem; }
.fact-grid { display: grid; gap: .45rem .9rem; grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr)); margin: 0; }
.fact-row { min-width: 0; }
.fact-row dt { color: var(--vscode-descriptionForeground); font-size: .8em; margin-bottom: .1rem; }
.fact-row dd { margin: 0; overflow-wrap: anywhere; }
.fact-section { border-top: 1px solid var(--vscode-panel-border); margin-top: 1rem; padding-top: .85rem; }
.payload-details { background: var(--vscode-textCodeBlock-background); margin-top: 1rem; }
.payload-details summary { font-size: .9em; }
.payload { margin: 0 .8rem .8rem; }
.raw-details { margin-top: 1.5rem; }
.raw-details summary { font-weight: 600; }
.raw-actions { display: flex; justify-content: flex-end; padding: 0 .8rem .5rem; }
@media (max-width: 34rem) { body { padding: .9rem; } .fact-grid { grid-template-columns: 1fr; } }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { scroll-behavior: auto !important; transition: none !important; } }
</style>
</head>
<body>
<div class="topbar">
  <h1>Jet reasoning</h1>
  <div class="toolbar" aria-label="Reasoning actions">
    <button type="button" class="toolbar-button" data-action="refresh">Refresh</button>
    <button type="button" class="toolbar-button" data-action="toggle-pin" aria-pressed="${state.pinned === true}">${pinLabel}</button>
    ${sourceButton}
  </div>
</div>
<p class="status${stale ? " stale" : ""}" role="status" aria-live="polite">${htmlEscape(status)}</p>
<div class="truth" aria-label="Projection truth">
  <span class="tag">Static checked facts</span>
  <span class="tag">Execution not included</span>
  <span class="tag">${htmlEscape(expandedLabel)}</span>
</div>
<section class="meta" aria-label="Projection summary">
  <div class="meta-item"><span class="meta-label">Source</span><span class="meta-value"><code>${displayFact(
    source
  )}</code></span></div>
  <div class="meta-item"><span class="meta-label">Selection</span><span class="meta-value"><code>${displayFact(
    selection
  )}</code></span></div>
  <div class="meta-item"><span class="meta-label">Checked facts</span><span class="meta-value">${records.length}</span></div>
  <div class="meta-item"><span class="meta-label">Panel</span><span class="meta-value">${state.pinned === true ? "Pinned" : "Reusable"}</span></div>
</section>
${truncated}
<h2>Fact lenses</h2>
<section class="lens-grid" aria-label="Fact lenses">${lensCards}</section>
<h2>Checked facts</h2>
<section class="facts" aria-label="Checked facts">${recordCards}</section>
<details class="raw-details">
  <summary>Raw projection JSON</summary>
  <div class="raw-actions"><button type="button" class="toolbar-button" data-action="copy-raw">Copy raw JSON</button></div>
  <pre>${serialized}</pre>
</details>
<script nonce="${nonce}">
const api = acquireVsCodeApi();
document.addEventListener('click', (event) => {
  const target = event.target instanceof Element ? event.target.closest('[data-action]') : null;
  if (!target) return;
  const action = target.getAttribute('data-action');
  if (action === 'open-source') {
    api.postMessage({ type: 'open-source', recordId: target.getAttribute('data-record-id') || '' });
  } else if (action === 'open-source-file') {
    api.postMessage({ type: 'open-source-file' });
  } else if (action === 'refresh') {
    api.postMessage({ type: 'refresh' });
  } else if (action === 'toggle-pin') {
    api.postMessage({ type: 'toggle-pin' });
  } else if (action === 'copy-raw') {
    api.postMessage({ type: 'copy-raw' });
  }
});
</script>
</body>
</html>`;
}

function reasoningUriFromArgument(uriArg) {
  let uri;
  if (typeof uriArg === "string" && uriArg.length > 0) {
    try {
      uri = vscode.Uri.parse(uriArg);
    } catch (_error) {
      return undefined;
    }
  } else if (uriArg && typeof uriArg.scheme === "string" && typeof uriArg.fsPath === "string") {
    uri = uriArg;
  } else {
    const editor = vscode.window.activeTextEditor;
    if (editor) {
      uri = editor.document.uri;
    }
  }
  if (!uri || uri.scheme !== "file" || path.extname(uri.fsPath).toLowerCase() !== ".jet") {
    return undefined;
  }
  return uri;
}

function reasoningClientReady() {
  return Boolean(client && typeof client.isRunning === "function" && client.isRunning());
}

async function fetchReasoning(uri, selection, expanded) {
  if (!reasoningClientReady()) {
    throw new Error("Jet language server is not running.");
  }
  const beforeVersion = (await vscode.workspace.openTextDocument(uri)).version;
  const projection = await client.sendRequest("workspace/executeCommand", {
    command: "jet.reasoning",
    arguments: [uri.toString(), selection, expanded],
  });
  const after = await vscode.workspace.openTextDocument(uri);
  return {
    projection,
    documentVersion: beforeVersion,
    stale: after.version !== beforeVersion,
  };
}

function ensureReasoningPanel() {
  if (reasoningPanel) {
    return reasoningPanel;
  }
  const panel = vscode.window.createWebviewPanel(
    "jetReasoning",
    "Jet: Reasoning",
    vscode.ViewColumn.Beside,
    { enableScripts: true, retainContextWhenHidden: true }
  );
  reasoningPanel = panel;
  panel.onDidDispose(() => {
    if (reasoningPanel === panel) {
      reasoningPanel = undefined;
      reasoningPanelState = undefined;
    }
  });
  panel.webview.onDidReceiveMessage((message) => handleReasoningMessage(panel, message));
  return panel;
}

function renderReasoningPanel(state) {
  if (!state || !state.panel || state.panel !== reasoningPanel) {
    return;
  }
  state.panel.title = state.pinned ? "Jet: Reasoning (pinned)" : "Jet: Reasoning";
  state.panel.webview.html = reasoningHtml(state.projection, state);
}

async function refreshReasoning(panel) {
  const state = reasoningPanelState;
  if (!state || state.panel !== panel) {
    return;
  }
  try {
    const uri = vscode.Uri.parse(state.uri);
    const result = await fetchReasoning(uri, state.selection, state.expanded);
    state.projection = result.projection;
    state.documentVersion = result.documentVersion;
    state.stale = result.stale;
    renderReasoningPanel(state);
  } catch (error) {
    vscode.window.showErrorMessage(`Jet reasoning refresh failed: ${error.message}`);
  }
}

async function openReasoningSource(panel, recordId) {
  const state = reasoningPanelState;
  if (!state || state.panel !== panel) {
    return;
  }
  let uri;
  try {
    uri = vscode.Uri.parse(state.uri);
  } catch (_error) {
    vscode.window.showErrorMessage("Jet reasoning source navigation is unavailable.");
    return;
  }
  if (uri.scheme !== "file" || path.extname(uri.fsPath).toLowerCase() !== ".jet") {
    vscode.window.showErrorMessage("Jet reasoning source navigation is limited to Jet files.");
    return;
  }
  const records = Array.isArray(state.projection?.records) ? state.projection.records : [];
  const record =
    typeof recordId === "string" && recordId.length > 0
      ? records.find((candidate) => candidate && candidate.id === recordId)
      : undefined;
  if (recordId && !record) {
    vscode.window.showErrorMessage("Jet reasoning source span is no longer available.");
    return;
  }
  try {
    const document = await vscode.workspace.openTextDocument(uri);
    const options = {
      viewColumn: vscode.ViewColumn.One,
      preserveFocus: false,
      preview: true,
    };
    const span = record && reasoningSpan(record);
    if (span && (state.stale || document.version !== state.documentVersion)) {
      vscode.window.showWarningMessage(
        "Source changed since these facts were checked. Refresh before opening a source span."
      );
      return;
    }
    if (span) {
      options.selection = new vscode.Range(
        byteOffsetToPosition(document.getText(), span.start),
        byteOffsetToPosition(document.getText(), span.end)
      );
    }
    await vscode.window.showTextDocument(document, options);
  } catch (error) {
    vscode.window.showErrorMessage(`Jet reasoning source navigation failed: ${error.message}`);
  }
}

function byteOffsetToPosition(text, byteOffset) {
  const bytes = Buffer.from(text, "utf8");
  let boundary = Math.max(0, Math.min(byteOffset, bytes.length));
  while (boundary > 0 && boundary < bytes.length && (bytes[boundary] & 0xc0) === 0x80) {
    boundary -= 1;
  }
  const prefix = bytes.subarray(0, boundary).toString("utf8");
  const line = prefix.split("\n").length - 1;
  const lastNewline = prefix.lastIndexOf("\n");
  const lineText = prefix.slice(lastNewline + 1).replace(/\r$/, "");
  return new vscode.Position(line, lineText.length);
}

async function handleReasoningMessage(panel, message) {
  if (!message || panel !== reasoningPanel || panel !== reasoningPanelState?.panel) {
    return;
  }
  if (message.type === "refresh") {
    await refreshReasoning(panel);
  } else if (message.type === "toggle-pin") {
    reasoningPanelState.pinned = !reasoningPanelState.pinned;
    renderReasoningPanel(reasoningPanelState);
  } else if (message.type === "open-source-file") {
    await openReasoningSource(panel, "");
  } else if (message.type === "open-source") {
    await openReasoningSource(panel, message.recordId);
  } else if (message.type === "copy-raw") {
    try {
      await vscode.env.clipboard.writeText(jsonForDisplay(reasoningPanelState.projection));
      vscode.window.showInformationMessage("Jet reasoning JSON copied.");
    } catch (error) {
      vscode.window.showErrorMessage(`Jet reasoning copy failed: ${error.message}`);
    }
  }
}

async function showReasoning(uriArg, selectionArg, expandedArg) {
  if (!reasoningClientReady()) {
    vscode.window.showErrorMessage("Jet reasoning requires the language server to be running.");
    return;
  }
  const uri = reasoningUriFromArgument(uriArg);
  if (!uri) {
    vscode.window.showErrorMessage("Jet reasoning needs an open .jet file.");
    return;
  }
  const selection =
    typeof selectionArg === "string" && selectionArg.length > 0 ? selectionArg : "";
  const expanded =
    expandedArg === undefined ? true : expandedArg === true || expandedArg === "true";
  const uriString = uri.toString();
  const existingState = reasoningPanelState;
  if (
    existingState &&
    existingState.pinned &&
    (existingState.uri !== uriString ||
      existingState.selection !== selection ||
      existingState.expanded !== expanded)
  ) {
    existingState.panel.reveal(vscode.ViewColumn.Beside);
    vscode.window.showInformationMessage(
      "Jet reasoning is pinned. Unpin the panel to show another selection."
    );
    return existingState.projection;
  }
  try {
    const result = await fetchReasoning(uri, selection, expanded);
    const panel = ensureReasoningPanel();
    reasoningPanelState = {
      panel,
      uri: uriString,
      selection,
      expanded,
      projection: result.projection,
      documentVersion: result.documentVersion,
      stale: result.stale,
      pinned: existingState?.pinned === true,
    };
    renderReasoningPanel(reasoningPanelState);
    panel.reveal(vscode.ViewColumn.Beside);
    return result.projection;
  } catch (error) {
    vscode.window.showErrorMessage(`Jet reasoning failed: ${error.message}`);
  }
}

function activate(context) {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const serverPath = findServer(workspaceFolder);
  const debugProvider = {
    resolveDebugConfiguration(_folder, config) {
      if (!debuggingIsAllowed()) {
        return undefined;
      }
      const program = config.program || vscode.window.activeTextEditor?.document.uri.fsPath;
      if (!program) {
        vscode.window.showErrorMessage("Jet debugger needs an open .jet file.");
        return undefined;
      }
      return {
        ...config,
        type: "jet",
        request: config.request || "launch",
        name: config.name || "Jet: Debug File",
        program: canonicalProgram(program),
      };
    },
  };
  const debugFactory = {
    createDebugAdapterDescriptor(session) {
      if (!serverPath || !debuggingIsAllowed()) {
        throw new Error("Jet debugging requires a trusted workspace.");
      }
      const program = debugSourceForConfiguration(session.configuration);
      const cwd = session.workspaceFolder?.uri.fsPath || workspaceFolder;
      return new vscode.DebugAdapterExecutable(
        serverPath,
        ["debug", "--dap", program],
        cwd ? { cwd } : undefined
      );
    },
  };

  if (serverPath) {
    client = new LanguageClient(
      "jet",
      "Jet Language Server",
      {
        command: serverPath,
        args: ["self", "lsp"],
        options: { cwd: workspaceFolder },
        transport: TransportKind.stdio,
      },
      {
        documentSelector: [{ scheme: "file", language: "jet" }],
        middleware: {
          // ExecuteCommandFeature owns registration for every server-advertised command.
          executeCommand(command, args, next) {
            if (command === "jet.reasoning") {
              const [uriArg, selectionArg, expandedArg] = Array.isArray(args) ? args : [];
              return showReasoning(uriArg, selectionArg, expandedArg);
            }
            return next(command, args);
          },
        },
        synchronize: {
          fileEvents: vscode.workspace.createFileSystemWatcher("**/*.jet"),
        },
      }
    );
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("jet.restartServer", async () => {
      if (client) {
        await client.restart();
      }
    }),
    vscode.commands.registerCommand("jet.runFile", (uriArg) => {
      const file = uriArgToPath(uriArg);
      if (file) {
        runJetInTerminal(serverPath, ["run", file]);
      }
    }),
    vscode.commands.registerCommand("jet.testFile", (uriArg) => {
      const file = uriArgToPath(uriArg);
      if (file) {
        runJetInTerminal(serverPath, ["test", file]);
      }
    }),
    vscode.commands.registerCommand("jet.learn", () => {
      runJetInTerminal(serverPath, ["learn"]);
    }),
    vscode.commands.registerCommand("jet.learnOnce", () => {
      runJetInTerminal(serverPath, ["learn", "--watch=off"]);
    }),
    vscode.workspace.onDidChangeTextDocument((event) => {
      const state = reasoningPanelState;
      if (
        !state ||
        state.stale ||
        event.document.uri.toString() !== state.uri ||
        event.document.version === state.documentVersion
      ) {
        return;
      }
      state.stale = true;
      renderReasoningPanel(state);
    }),
    vscode.commands.registerCommand("jet.debugFile", (uriArg) => {
      const file = uriArgToPath(uriArg);
      if (file) {
        debugFile(file);
      }
    }),
    vscode.debug.registerDebugConfigurationProvider("jet", debugProvider),
    vscode.debug.registerDebugAdapterDescriptorFactory("jet", debugFactory)
  );

  if (client) {
    client.start().catch(() => {
      vscode.window.showErrorMessage(
        `Jet language server failed to start (tried: ${serverPath}). ` +
          `Build it with \`nix develop -c cargo build\` in the jet repo, ` +
          `or set jet.languageServerPath to a jet binary.`
      );
    });
  }
}

function debuggingIsAllowed() {
  if (vscode.workspace.isTrusted) {
    return true;
  }
  vscode.window.showErrorMessage("Jet debugging requires a trusted workspace.");
  return false;
}

function deactivate() {
  if (client) {
    return client.stop();
  }
}

module.exports = { activate, deactivate };

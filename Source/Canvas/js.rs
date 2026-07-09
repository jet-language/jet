pub fn canvas_js() -> String {
    r###"(function () {
  const canvas = document.getElementById("jet-canvas-view");
  const ctx = canvas.getContext("2d");
  const stage = document.getElementById("stage");
  const sourceView = document.getElementById("source-view");
  const sourceEditor = document.getElementById("source-editor");
  const viewToggle = document.getElementById("view-toggle");
  const lensButtons = Array.from(document.querySelectorAll("[data-view-mode]"));
  const detailToggleInputs = Array.from(document.querySelectorAll("[data-detail-toggle]"));
  const developerModeButton = document.getElementById("developer-mode");
  const contextMenu = document.getElementById("context-menu");
  const minimap = document.getElementById("minimap");
  const mini = minimap.getContext("2d");
  const details = document.getElementById("details");
  const jump = document.getElementById("jump");
  const graphBack = document.getElementById("graph-back");
  const graphForward = document.getElementById("graph-forward");
  const graphSelect = document.getElementById("graph-select");
  const graphStrip = document.getElementById("graph-strip");
  const graphCount = document.getElementById("graph-count");
  const wireStatus = document.getElementById("wire-status");
  const graphOverview = document.getElementById("graph-overview");
  const leftDrawer = document.getElementById("left-drawer");
  const rightDrawer = document.getElementById("right-drawer");
  const dockGraphs = document.getElementById("dock-graphs");
  const dockDetails = document.getElementById("dock-details");
  const projectRail = document.getElementById("project-rail");
  const projectMode = document.getElementById("project-mode");
  const packageSummary = document.getElementById("package-summary");
  const dependencySummary = document.getElementById("dependency-summary");
  const devSummary = document.getElementById("dev-summary");
  const diagnosticsSummary = document.getElementById("diagnostics-summary");
  const trustSummary = document.getElementById("trust-summary");
  const graphList = document.getElementById("graph-list");
  const canvasSearch = document.getElementById("canvas-search");
  const searchResults = document.getElementById("search-results");
  const zoomLabel = document.getElementById("zoom-label");
  const graphMeta = document.getElementById("graph-meta");
  const sourceId = document.getElementById("source-id");
  const revision = document.getElementById("revision");
  const scmState = document.getElementById("scm-state");
  const proofState = document.getElementById("proof-state");
  const proofRail = document.getElementById("proof-rail");
  const toast = document.getElementById("toast");
  const sourceDiff = document.getElementById("source-diff");
  const editSource = document.getElementById("edit-source");
  const applySourceEdit = document.getElementById("apply-source-edit");
  const cancelSourceEdit = document.getElementById("cancel-source-edit");
  const undoEdit = document.getElementById("undo-edit");
  const redoEdit = document.getElementById("redo-edit");
  const orgAlign = document.getElementById("org-align");
  const orgTidy = document.getElementById("org-tidy");
  const bookmarkAdd = document.getElementById("bookmark-add");
  const bookmarkJump = document.getElementById("bookmark-jump");
  const coreCatalog = document.getElementById("core-catalog");
  const favoriteAction = document.getElementById("favorite-action");
  const runCurrent = document.getElementById("run-current");
  const runHud = document.getElementById("run-hud");
  const firstRunTour = document.getElementById("first-run-tour");
  const tourDismiss = document.getElementById("tour-dismiss");
  const debugStep = document.getElementById("debug-step");
  const debugNext = document.getElementById("debug-next");
  const debugContinue = document.getElementById("debug-continue");
  const debugStop = document.getElementById("debug-stop");
  const debugBreak = document.getElementById("debug-break");
  const debugWatch = document.getElementById("debug-watch");
  let hit = [];
  let pinPoints = new Map();
  let pinHit = [];
  let pinEditorHit = [];
  let connectedPinIds = new Set();
  let latestDoc = null;
  let latestProject = null;
  let selectedSourceId = null;
  let debugOverlay = null;
  let debugState = { breakpoints: [], watches: [] };
  let searchState = { results: [], spans: [], active: -1, diff: null, impact: null };
  let scm = null;
  let proofDoc = null;
  let undoStack = [];
  let redoStack = [];
  let editorState = { bookmarks: [], favorites: [], actionUse: {}, rerouteKnots: [], tourDismissed: false };
  let wireStyle = "bezier";
  let runState = { running: false, last: "idle" };
  let selectedGraphId = null;
  let graphBackStack = [];
  let graphForwardStack = [];
  let selectedNodeId = null;
  let selectedNodeIds = new Set();
  let view = { x: 64, y: 42, zoom: 1 };
  let drag = null;
  let nodeOffsets = new Map();
  let autoNodeOffsets = new Map();
  let hoverPin = null;
  let pendingPin = null;
  let spaceDown = false;
  let viewMode = "graph";
  let sourceEditMode = false;
  let detailToggles = { types: false, diagnostics: true, effects: false, debug: false, package: true };
  let developerMode = false;
  const layoutScale = { x: 1.08, y: 1.08 };
  const palette = [
    { title: "Print", detail: "Call print(\"canvas\")", op: "insert_print" },
    { title: "Branch", detail: "Insert if/else rails", op: "insert_branch" },
    { title: "Switch", detail: "Insert dispatch rails", op: "insert_switch" },
    { title: "Loop", detail: "Insert loop rail", op: "insert_loop" },
    { title: "Fallible", detail: "Insert ? rail", op: "insert_fallible_rail" },
    { title: "Call", detail: "Insert call transaction", op: "insert_call" },
    { title: "Comment", detail: "Source comment projection", op: "comment" }
  ];
  let actionEntries = [];
  let coreCatalogLoaded = false;
  let coreCatalogLoading = null;
  let contextMenuState = null;
  const UI_FONT = '"Inter", "Segoe UI", Roboto, system-ui, sans-serif';
  const MONO_FONT = '"JetBrains Mono", ui-monospace, "SFMono-Regular", Consolas, monospace';
  const TYPE_COLOR_MAP = {
    exec: "#f2f4f8",
    control: "#f2f4f8",
    Bool: "#c0392b",
    Int: "#2ec4b6",
    I64: "#2ec4b6",
    U64: "#2ec4b6",
    Float: "#9acd32",
    F32: "#9acd32",
    F64: "#9acd32",
    String: "#c678dd",
    Char: "#e8a2c8",
    Map: "#e5a03c",
    Result: "#fb7185",
    Struct: "#5b8dd9",
    Enum: "#4f9e5a",
    Fn: "#a78bfa",
    Void: "#6b7280",
    unknown: "#8a8f98"
  };
  const NODE_ARCHETYPE_STYLES = {
    entry: { accent: "#b83647", header: "#b83647", header2: "#6f1e2b", label: "Entry", glyph: "ƒ", subtitle: "entry" },
    function_exec: { accent: "#4f83b6", header: "#315d84", header2: "#1f3e5b", label: "Function", glyph: "ƒ", subtitle: "" },
    function_pure: { accent: "#4f9e5a", header: "#357745", header2: "#234f31", label: "Pure function", glyph: "ƒ", subtitle: "" },
    control: { accent: "#f2f4f8", header: "#2c333d", header2: "#151a21", label: "Control", glyph: "◇", subtitle: "control" },
    value: { accent: "#8a8f98", header: "#252b34", header2: "#151a21", label: "Value", glyph: "•", subtitle: "value" }
  };

  function storedFlag(name) {
    try { return window.localStorage && window.localStorage.getItem(name) === "1"; }
    catch (_) { return false; }
  }

  function storeFlag(name, value) {
    try { if (window.localStorage) window.localStorage.setItem(name, value ? "1" : "0"); }
    catch (_) {}
  }

  function projectStateKey(doc) {
    return "jet.canvas.editor:" + ((doc && doc.source_id) || "source");
  }

  function loadEditorState(doc) {
    try {
      editorState = JSON.parse(localStorage.getItem(projectStateKey(doc)) || "null") || editorState;
    } catch (_) {
      editorState = { bookmarks: [], favorites: [], actionUse: {}, rerouteKnots: [], tourDismissed: false };
    }
    editorState.bookmarks ||= [];
    editorState.favorites ||= [];
    editorState.actionUse ||= {};
    editorState.rerouteKnots ||= [];
    editorState.tourDismissed = !!editorState.tourDismissed;
    if (firstRunTour) firstRunTour.classList.toggle("is-open", !editorState.tourDismissed);
  }

  function saveEditorState() {
    if (!latestDoc) return;
    try { localStorage.setItem(projectStateKey(latestDoc), JSON.stringify(editorState)); }
    catch (_) {}
  }

  function favoriteSet() {
    return new Set(editorState.favorites || []);
  }

  function markActionUsed(action) {
    const id = action.action_id || action.callee || action.title;
    editorState.actionUse[id] = (editorState.actionUse[id] || 0) + 1;
    saveEditorState();
  }

  function rankAction(action) {
    const id = action.action_id || action.callee || action.title;
    const favorite = favoriteSet().has(id) ? 100000 : 0;
    const used = editorState.actionUse[id] || 0;
    return favorite + used;
  }

  function currentGraphOrNull() {
    return latestDoc ? currentGraph(latestDoc) : null;
  }

  function selectedGraphNodes(graph) {
    if (!graph) return [];
    return graph.nodes.filter((node) => selectedNodeIds.has(node.node_id));
  }

  function setNodeViewPosition(node, x, y) {
    nodeOffsets.set(node.node_id, {
      x: x - node.layout.x * layoutScale.x,
      y: y - node.layout.y * layoutScale.y
    });
  }

  function alignSelectedNodes(axis) {
    const graph = currentGraphOrNull();
    const nodes = selectedGraphNodes(graph);
    if (nodes.length < 2) return showToast("Select nodes to align");
    const target = axis === "y"
      ? Math.min(...nodes.map((node) => nodeY(node)))
      : Math.min(...nodes.map((node) => nodeX(node)));
    for (const node of nodes) {
      const x = axis === "x" ? target : nodeX(node);
      const y = axis === "y" ? target : nodeY(node);
      setNodeViewPosition(node, x, y);
    }
    showToast(axis === "y" ? "Aligned top" : "Aligned left");
    drawGraph(latestDoc);
  }

  function tidyGraphLayout() {
    const graph = currentGraphOrNull();
    if (!graph) return;
    const positions = rankedGraphLayout(graph);
    for (const node of graph.nodes) {
      const pos = positions.get(node.node_id);
      if (pos) setNodeViewPosition(node, pos.x, pos.y);
    }
    autoNodeOffsets = new Map();
    wireStyle = "bezier";
    showToast("Graph tidied");
    drawGraph(latestDoc);
  }

  function addRerouteKnot() {
    const graph = currentGraphOrNull();
    if (!graph) return;
    const size = cssSize();
    editorState.rerouteKnots.push({
      graph_id: graph.graph_id,
      x: wx(size.width / 2),
      y: wy(size.height / 2)
    });
    saveEditorState();
    showToast("Reroute knot placed");
    drawGraph(latestDoc);
  }

  function bookmarkCurrentGraph() {
    const graph = currentGraphOrNull();
    if (!graph) return;
    const entry = { graph_id: graph.graph_id, node_id: selectedNodeId, title: graph.title, view };
    editorState.bookmarks = (editorState.bookmarks || []).filter((b) => b.graph_id !== graph.graph_id);
    editorState.bookmarks.unshift(entry);
    saveEditorState();
    showToast("Bookmark saved");
  }

  function jumpBookmark() {
    const mark = (editorState.bookmarks || [])[0];
    if (!mark) return showToast("No bookmark");
    view = mark.view || view;
    switchGraph(mark.graph_id, { nodeId: mark.node_id, fit: false, toast: "Bookmark opened" });
  }

  function toggleFavoriteAction() {
    const action = actionEntries[0] || graphActionItems()[0];
    if (!action) return showToast("No action to favorite");
    const id = action.action_id || action.callee || action.title;
    const set = favoriteSet();
    if (set.has(id)) set.delete(id);
    else set.add(id);
    editorState.favorites = Array.from(set);
    saveEditorState();
    showToast(set.has(id) ? "Favorite pinned" : "Favorite removed");
  }

  function runCurrentGraph() {
    const run = actionEntries.find((item) => item.action_id === "canvas.command:run")
      || actionEntries.find((item) => item.action_id === "canvas.command:dev")
      || null;
    if (!run) {
      runHud.textContent = "run authority loading";
      loadCanvasActions();
      return;
    }
    renderCommandAuthority(run);
  }

  function renderCommandAuthority(item) {
    const command = (item.command || []).join(" ");
    runState = { running: false, last: item.title + " authority ready" };
    runHud.textContent = item.available ? runState.last : (item.denied_reason || "command unavailable");
    runHud.classList.remove("is-running");
    window.__jetCanvasRunLoop = { graph_id: selectedGraphId, state: "authority_required", action_id: item.action_id, command: item.command || [] };
    details.innerHTML = `<h2>Command</h2><div class="signature-source"><code>${escapeHtml(command)}</code><span>${escapeHtml(item.writes || "none")} · ${item.requires_confirmation ? "confirmation required" : "read-only"}</span><button id="execute-command-authority">Run</button></div><div class="inline-row"><b>Authority</b><code>${escapeHtml((item.authority || []).join("\n"))}</code></div>`;
    const execute = document.getElementById("execute-command-authority");
    if (execute) execute.addEventListener("click", () => executeCommandAuthority(item));
    showToast(item.available ? item.title + " authority ready" : item.denied_reason || "Command unavailable");
    loadProofRail();
  }

  function executeCommandAuthority(item) {
    if (!latestDoc || !item || !item.available) return;
    const confirmed = !item.requires_confirmation || window.confirm(item.title + " writes " + (item.writes || "outputs") + ". Continue?");
    if (!confirmed) return;
    const body = { schema_version: 1, revision: latestDoc.revision, action_id: item.action_id, confirmed };
    runHud.textContent = item.title + " running";
    runHud.classList.add("is-running");
    fetch(commandUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((json) => ({ ok: r.ok, json })))
      .then((result) => {
        runHud.classList.remove("is-running");
        const doc = result.json || {};
        runHud.textContent = doc.success ? item.title + " passed" : item.title + " failed";
        details.innerHTML = `<h2>Receipt</h2><div class="signature-source"><code>${escapeHtml((doc.command || []).join(" "))}</code><span>${escapeHtml(doc.success ? "success" : "failed")} · ${escapeHtml(String(doc.exit_code ?? "?"))} · ${escapeHtml(String(doc.elapsed_ms || 0))}ms</span></div><div class="inline-row"><b>stdout</b><code>${escapeHtml(doc.stdout || "")}</code></div><div class="inline-row"><b>stderr</b><code>${escapeHtml(doc.stderr || "")}</code></div>`;
        loadProofRail();
      })
      .catch((e) => {
        runHud.classList.remove("is-running");
        runHud.textContent = item.title + " failed";
        showToast(String(e));
      });
  }

  function setDeveloperMode(on) {
    developerMode = !!on;
    document.body.classList.toggle("is-dev-mode", developerMode);
    developerModeButton.classList.toggle("is-active", developerMode);
    developerModeButton.textContent = developerMode ? "Developer on" : "Developer";
    developerModeButton.title = developerMode ? "Hide Canvas internals" : "Show Canvas internals";
    storeFlag("jet.canvas.developerMode", developerMode);
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  }

  function detailStateKey(doc) {
    return "jet.canvas.detail:" + ((doc && doc.source_id) || "source");
  }

  function loadDetailToggles(doc) {
    try {
      detailToggles = Object.assign(detailToggles, JSON.parse(localStorage.getItem(detailStateKey(doc)) || "null") || {});
    } catch (_) {}
    syncDetailToggles();
  }

  function saveDetailToggles() {
    if (!latestDoc) return;
    try { localStorage.setItem(detailStateKey(latestDoc), JSON.stringify(detailToggles)); }
    catch (_) {}
  }

  function syncDetailToggles() {
    for (const key of Object.keys(detailToggles)) document.body.classList.toggle("detail-" + key, !!detailToggles[key]);
    for (const input of detailToggleInputs) input.checked = !!detailToggles[input.getAttribute("data-detail-toggle")];
    window.__jetCanvasDetailToggles = Object.assign({}, detailToggles);
  }

  function showToast(text) {
    toast.textContent = text;
    window.clearTimeout(showToast.timer);
    showToast.timer = window.setTimeout(() => { toast.textContent = ""; }, 2200);
  }

  function setPendingPin(pin) {
    pendingPin = pin;
    window.__jetCanvasPendingPin = pin ? { pin_id: pin.pin_id, name: pin.name, type: pin.type, direction: pin.direction } : null;
    syncWireStatus(pin ? { title: "Source socket", detail: pinName(pin) + " : " + exactPinType(pin), color: colorForType(pin.type || "Value") } : null);
  }

  function compactCanvasMode() {
    return window.matchMedia && window.matchMedia("(max-width: 900px)").matches;
  }

  function setDrawer(which) {
    const compact = compactCanvasMode();
    if (!compact) {
      leftDrawer.classList.remove("is-drawer-open");
      rightDrawer.classList.remove("is-drawer-open");
      dockGraphs.classList.remove("is-active");
      dockDetails.classList.remove("is-active");
      return;
    }
    const leftOpen = which === "graphs";
    leftDrawer.classList.toggle("is-drawer-open", leftOpen);
    rightDrawer.classList.toggle("is-drawer-open", which === "details");
    dockGraphs.classList.toggle("is-active", which === "graphs");
    dockDetails.classList.toggle("is-active", which === "details");
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  }

  function fit() {
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(640, Math.floor(rect.width * dpr));
    canvas.height = Math.max(420, Math.floor(rect.height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function cssSize() {
    const rect = canvas.getBoundingClientRect();
    return { width: rect.width || 640, height: rect.height || 420 };
  }

  function sx(x) { return x * view.zoom + view.x; }
  function sy(y) { return y * view.zoom + view.y; }
  function wx(x) { return (x - view.x) / view.zoom; }
  function wy(y) { return (y - view.y) / view.zoom; }
  function nodeOffset(node) { return nodeOffsets.get(node.node_id) || { x: 0, y: 0 }; }
  function autoNodeOffset(node) { return autoNodeOffsets.get(node.node_id) || { x: 0, y: 0 }; }
  function rawNodeX(node) { const o = nodeOffset(node); return node.layout.x * layoutScale.x + o.x; }
  function rawNodeY(node) { const o = nodeOffset(node); return node.layout.y * layoutScale.y + o.y; }
  function nodeX(node) { const o = autoNodeOffset(node); return rawNodeX(node) + o.x; }
  function nodeY(node) { const o = autoNodeOffset(node); return rawNodeY(node) + o.y; }

  function colorForType(type) {
    const t = String(type || "unknown").trim();
    if (TYPE_COLOR_MAP[t]) return TYPE_COLOR_MAP[t];
    if (t === "IntN" || /^U?\d+$/.test(t) || /^I\d+$/.test(t)) return TYPE_COLOR_MAP.Int;
    if (t.startsWith("[")) return colorForType(t.slice(1, -1).trim() || "unknown");
    if (t.endsWith("?")) return colorForType(t.slice(0, -1).trim() || "unknown");
    if (/Result|Error|Fallible/.test(t)) return TYPE_COLOR_MAP.Result;
    if (/Map|Dict/.test(t)) return TYPE_COLOR_MAP.Map;
    if (/Enum|Variant/.test(t)) return TYPE_COLOR_MAP.Enum;
    if (/Fn|fn\(|=>/.test(t)) return TYPE_COLOR_MAP.Fn;
    if (/^[A-Z][A-Za-z0-9_]*(::[A-Z][A-Za-z0-9_]*)*$/.test(t)) return TYPE_COLOR_MAP.Struct;
    return TYPE_COLOR_MAP.unknown;
  }

  function wireColor(wire, from) {
    if (wire.wire_kind === "control") return "#f8fafc";
    if (wire.wire_kind === "fallible") return "#fb7185";
    if (wire.wire_kind === "effect") return "#c084fc";
    if (wire.wire_kind === "debug") return "#facc15";
    return from.color;
  }

  function pinRail(pin) {
    if (!pin) return "data";
    if ((pin.type || "") === "exec" || pin.name === "exec" || pin.capability === "control") return "control";
    if (pin.fallible) return "fallible";
    if (pin.effect_grant_need) return "effect";
    return "data";
  }

  function setViewMode(mode) {
    viewMode = mode === "code" || mode === "split" ? mode : "graph";
    if (viewMode === "graph") setSourceEditMode(false);
    stage.classList.toggle("is-code", viewMode === "code");
    stage.classList.toggle("is-split", viewMode === "split");
    viewToggle.textContent = viewMode === "graph" ? "Code" : "Graph";
    viewToggle.classList.toggle("is-active", viewMode !== "graph");
    for (const button of lensButtons) {
      button.classList.toggle("is-active", button.getAttribute("data-view-mode") === viewMode);
    }
    sourceView.textContent = latestDoc && latestDoc.source_text ? latestDoc.source_text : "";
    if (sourceEditor && !sourceEditMode) sourceEditor.value = latestDoc && latestDoc.source_text ? latestDoc.source_text : "";
    window.__jetCanvasLensMode = viewMode;
    if (viewMode !== "code" && latestDoc) drawGraph(latestDoc);
  }

  function setSourceEditMode(active) {
    sourceEditMode = !!active && !!latestDoc;
    stage.classList.toggle("is-source-edit", sourceEditMode);
    if (sourceEditMode) {
      if (viewMode === "graph") viewMode = "split";
      stage.classList.toggle("is-code", viewMode === "code");
      stage.classList.toggle("is-split", viewMode === "split");
      if (sourceEditor) {
        sourceEditor.value = latestDoc.source_text || "";
        sourceEditor.focus();
      }
    }
    if (editSource) editSource.classList.toggle("is-active", sourceEditMode);
    window.__jetCanvasSourceEditMode = sourceEditMode;
  }

  function applySourceEditBuffer() {
    if (!latestDoc || !sourceEditor) return;
    postTransaction({
      schema_version: 1,
      op: "replace_source",
      revision: latestDoc.revision,
      source: sourceEditor.value,
      source_edit: true
    });
  }

  function hexToRgba(hex, alpha) {
    const h = String(hex || "#2563eb").replace("#", "");
    const r = parseInt(h.slice(0, 2), 16) || 37;
    const g = parseInt(h.slice(2, 4), 16) || 99;
    const b = parseInt(h.slice(4, 6), 16) || 235;
    return "rgba(" + r + "," + g + "," + b + "," + (parseFloat(alpha) || 0.18) + ")";
  }

  function spansOverlap(a, b) {
    if (!a || !b) return false;
    return a.start <= b.end && b.start <= a.end;
  }

  function postQuery(body) {
    if (!latestDoc) return Promise.resolve(null);
    return fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(Object.assign({ schema_version: 1, revision: latestDoc.revision }, body))
    })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) {
          showToast(result.json.message || "Canvas query rejected");
          return null;
        }
        searchState.results = result.json.results || [];
        searchState.spans = searchState.results.map((r) => r.source_span).filter(Boolean);
        searchState.active = searchState.results.length ? 0 : -1;
        searchState.diff = result.json.diff || null;
        searchState.impact = result.json.impact || null;
        renderSearchResults();
        if (searchState.results[0]) selectQueryResult(searchState.results[0], false);
        if (latestDoc) drawGraph(latestDoc);
        return result.json;
      })
      .catch((e) => { showToast(String(e)); return null; });
  }

  function renderSearchResults() {
    const rows = (searchState.results || []).slice(0, 24).map((result, i) => {
      const active = i === searchState.active ? " is-active" : "";
      const label = escapeHtml(result.title || result.symbol || result.kind || "match");
      const where = `${result.kind || "match"} · line ${result.line || "?"}`;
      return `<button class="search-item${active}" data-search-hit="${i}">${label}<small>${escapeHtml(where)} ${escapeHtml(result.excerpt || "")}</small></button>`;
    }).join("");
    const diff = searchState.diff && searchState.diff.text ? `<div class="inline-row"><b>Preview diff</b><code>${escapeHtml(searchState.diff.text)}</code></div>` : "";
    const impact = searchState.impact && searchState.impact.found ? `<div class="pin-row"><b>Impact</b><br><span class="tag">${(searchState.impact.references || []).length} refs / ${(searchState.impact.call_sites || []).length} calls</span></div>` : "";
    searchResults.innerHTML = rows || diff || impact ? rows + diff + impact : "<div class=\"tag\">no matches</div>";
    searchResults.querySelectorAll("[data-search-hit]").forEach((button) => {
      button.addEventListener("click", () => {
        const index = Number(button.getAttribute("data-search-hit"));
        searchState.active = index;
        selectQueryResult(searchState.results[index], true);
        renderSearchResults();
        if (latestDoc) drawGraph(latestDoc);
      });
    });
  }

  function loadSourceControl() {
    return fetch(sourceControlUrl, { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        scm = doc;
        const dirtyCount = doc.dirty_files ? " · " + doc.dirty_files + " files" : "";
        scmState.textContent = doc.available ? (doc.dirty ? "git dirty" + dirtyCount : "git clean") : "no git";
        scmState.style.color = doc.dirty ? "#fde68a" : "#8fb2dc";
        return doc;
      })
      .catch(() => {
        scm = null;
        scmState.textContent = "git ?";
      });
  }

  function proofRequestUrl(sourceId) {
    if (!sourceId) return proofUrl;
    return proofUrl + (proofUrl.includes("?") ? "&" : "?") + "source_id=" + encodeURIComponent(sourceId);
  }

  function loadProofRail() {
    if (!proofRail) return Promise.resolve(null);
    return fetch(proofRequestUrl(selectedSourceId), { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        proofDoc = doc;
        const proof = doc.proof || {};
        const check = doc.check || {};
        const scmDoc = doc.source_control || {};
        const rows = [
          ["revision", doc.revision || "?"],
          ["check", check.state || "unknown"],
          ["git", scmDoc.available ? (scmDoc.dirty ? "dirty" : "clean") : "unavailable"],
          ["receipt", (doc.command_receipts && (doc.command_receipts.reason || doc.command_receipts.state)) || "missing"],
          ["proof", proof.state || "missing"]
        ];
        proofState.textContent = proof.state || "missing";
        proofState.style.color = proof.state === "current" ? "#8fb2dc" : "#f8c76a";
        proofRail.innerHTML = rows.map(([label, value]) => {
          const cls = label === "receipt" || value === "missing" ? " is-missing" : "";
          return `<div class="proof-row${cls}"><b>${escapeHtml(label)}</b><span>${escapeHtml(value)}</span></div>`;
        }).join("");
        return doc;
      })
      .catch(() => {
        proofDoc = null;
        proofState.textContent = "unknown";
        proofRail.innerHTML = "<div class=\"proof-row is-missing\"><b>proof</b><span>unavailable</span></div>";
      });
  }

  function showSourceDiff() {
    const render = (doc) => {
      if (!doc) return;
      const fileDiff = (doc.files || []).filter((file) => file.dirty).map((file) => file.diff || file.status || file.path).join("\n");
      const diff = doc.diff || fileDiff || (doc.dirty ? doc.status : "clean");
      searchState.results = [];
      searchState.spans = [];
      searchState.active = -1;
      searchState.impact = null;
      searchState.diff = { text: diff || "clean" };
      renderSearchResults();
      showToast(doc.dirty ? "Source diff loaded" : "Source clean");
    };
    if (scm) render(scm);
    else loadSourceControl().then(render);
  }

  function selectQueryResult(result, fitView) {
    if (!result) return;
    if (result.graph_id) selectedGraphId = result.graph_id;
    if (result.node_id) {
      selectedNodeId = result.node_id;
      selectedNodeIds = new Set([result.node_id]);
    }
    if (fitView && latestDoc) fitGraph();
    if (result.source_span) setSourceHash(result.source_span);
  }

  function runCanvasSearch() {
    const query = canvasSearch.value.trim();
    if (!query) {
      searchState = { results: [], spans: [], active: -1, diff: null, impact: null };
      renderSearchResults();
      if (latestDoc) drawGraph(latestDoc);
      return;
    }
    postQuery({ op: "find", query });
  }

  function sourceHashSpan() {
    const m = String(window.location.hash || "").match(/^#span-(\d+)-(\d+)$/);
    return m ? { start: Number(m[1]), end: Number(m[2]) } : null;
  }

  function setSourceHash(span) {
    if (!span) return;
    const next = "#span-" + span.start + "-" + span.end;
    if (window.location.hash === next) return;
    window.history.replaceState(null, "", next);
  }

  function applySourceHash() {
    const span = sourceHashSpan();
    if (!span || !latestDoc) return;
    postQuery({ op: "source_to_graph", start: span.start, end: span.end });
  }

  function roundRect(x, y, w, h, r) {
    const rr = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rr, y);
    ctx.lineTo(x + w - rr, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + rr);
    ctx.lineTo(x + w, y + h - rr);
    ctx.quadraticCurveTo(x + w, y + h, x + w - rr, y + h);
    ctx.lineTo(x + rr, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - rr);
    ctx.lineTo(x, y + rr);
    ctx.quadraticCurveTo(x, y, x + rr, y);
  }

  function drawPin(pin, x, y, dir, recordHit = true) {
    const color = colorForType(pin.type || "unknown");
    const connected = connectedPinIds.has(pin.pin_id);
    const r = isExecPin(pin) ? Math.max(6.5, 8 * view.zoom) : Math.max(5.2, 5.8 * view.zoom);
    if (connected) {
      ctx.save();
      ctx.beginPath();
      ctx.arc(x, y, Math.max(8, 10 * view.zoom), 0, Math.PI * 2);
      ctx.fillStyle = hexToRgba(color, .25);
      ctx.shadowColor = hexToRgba(color, .45);
      ctx.shadowBlur = 4 * view.zoom;
      ctx.fill();
      ctx.restore();
    }
    ctx.beginPath();
    if (isExecPin(pin)) {
      ctx.moveTo(x - r * .9, y - r);
      ctx.lineTo(x + r * 1.25, y);
      ctx.lineTo(x - r * .9, y + r);
      ctx.closePath();
    } else {
      ctx.arc(x, y, r, 0, Math.PI * 2);
    }
    ctx.fillStyle = connected ? color : "rgba(8,12,18,.98)";
    ctx.fill();
    if (hoverPin && hoverPin.pin_id === pin.pin_id) {
      ctx.beginPath();
      ctx.arc(x, y, 11 * view.zoom, 0, Math.PI * 2);
      ctx.strokeStyle = "#fef08a";
      ctx.lineWidth = Math.max(1.3, 1.6 * view.zoom);
      ctx.stroke();
    }
    ctx.lineWidth = Math.max(1.4, 1.8 * view.zoom);
    ctx.strokeStyle = color;
    ctx.stroke();
    if (recordHit) {
      const hitR = Math.max(12, 18 * view.zoom);
      pinPoints.set(pin.pin_id, { x, y, color, pin });
      pinHit.push({ x: x - hitR, y: y - hitR, w: hitR * 2, h: hitR * 2, cx: x, cy: y, pin });
    }
  }

  function pinName(pin) {
    if (!pin) return "";
    return pin.name || "";
  }

  function exactPinType(pin) {
    if (!pin) return "";
    return isExecPin(pin) ? "exec" : (pin.type || "Value");
  }

  function clipText(text, max) {
    const s = String(text || "");
    return s.length > max ? s.slice(0, Math.max(1, max - 1)) + "…" : s;
  }

  function drawPinLabel(pin, x, y, align) {
    const label = pinName(pin);
    ctx.font = `${Math.max(9, 11 * view.zoom)}px ${UI_FONT}`;
    ctx.textAlign = align;
    ctx.fillStyle = isExecPin(pin) ? "rgba(242,244,248,.86)" : "rgba(234,242,255,.70)";
    ctx.fillText(clipText(label, 24), x, y);
    ctx.textAlign = "left";
  }

  function drawPinHoverTooltip(pin) {
    const point = pin && pinPoints.get(pin.pin_id);
    if (!point) return;
    const text = pinName(pin) + " : " + exactPinType(pin) + " - " + typeExplanation(pin.type);
    ctx.font = `${Math.max(10, 12 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 9 * view.zoom;
    const badgeW = Math.min(220 * view.zoom, ctx.measureText(text).width + padX * 2);
    const badgeH = 25 * view.zoom;
    const bx = Math.max(8, Math.min(point.x + 14 * view.zoom, cssSize().width - badgeW - 8));
    const by = Math.max(8, point.y - badgeH - 13 * view.zoom);
    roundRect(bx, by, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.95)";
    ctx.fill();
    ctx.strokeStyle = point.color;
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = point.color;
    ctx.textAlign = "left";
    ctx.fillText(clipText(text, 28), bx + padX, by + 17 * view.zoom);
  }

  function typeExplanation(type) {
    if (type === "exec" || type === "control") return "control rail";
    if (type === "Bool") return "branch condition";
    if (type === "String") return "text value";
    if (type === "Int") return "whole number";
    if (String(type || "").includes("Task")) return "task handle";
    if (String(type || "").includes("Event") || String(type || "").includes("Hook")) return "event dispatcher";
    return "source-backed Jet value";
  }

  function drawSocketRow(pin, x, y, w, dir, recordHit) {
    const execInset = isExecPin(pin) ? 5 * view.zoom : 0;
    const px = dir === "input" ? x + execInset : x + w - execInset;
    const labelX = dir === "input" ? x + 22 * view.zoom : x + w - 22 * view.zoom;
    const labelAlign = dir === "input" ? "left" : "right";
    drawPin(pin, px, y, dir, recordHit);
    drawPinLabel(pin, labelX, y + 4 * view.zoom, labelAlign);
  }

  function editablePinKind(pin) {
    const t = String(pin && pin.type || "");
    if (t === "Bool") return "bool";
    if (["Int", "I64", "U64", "Float", "F32", "F64"].includes(t)) return "number";
    if (t === "String" || t === "Char") return "string";
    if (/Enum|Variant/.test(t)) return "enum";
    return "";
  }

  function inlineDefaultForPin(graph, pin) {
    if (!graph || !pin || pin.direction !== "input" || connectedPinIds.has(pin.pin_id)) return null;
    const expr = inlineForPin(graph, pin);
    if (!expr) return null;
    return expr;
  }

  function defaultEditorValue(expr) {
    return String((expr && expr.source) || "").trim();
  }

  function isLiteralDefault(kind, source) {
    const s = String(source || "").trim();
    if (kind === "bool") return s === "true" || s === "false";
    if (kind === "number") return /^-?\d+(\.\d+)?$/.test(s);
    if (kind === "string") return /^"([^"\\]|\\.)*"$/.test(s) || /^'([^'\\]|\\.)'$/.test(s);
    if (kind === "enum") return /^\.?[A-Za-z_][A-Za-z0-9_.]*$/.test(s);
    return false;
  }

  function drawInlineExprChip(pin, expr, x, y, recordHit) {
    const source = defaultEditorValue(expr);
    const color = colorForType(pin.type || "unknown");
    ctx.font = `${Math.max(8, 10 * view.zoom)}px ${MONO_FONT}`;
    const boxW = Math.min(150, Math.max(58, ctx.measureText(source || "expr").width / view.zoom + 18)) * view.zoom;
    const boxH = 20 * view.zoom;
    const bx = x;
    const by = y - boxH / 2;
    roundRect(bx, by, boxW, boxH, 4 * view.zoom);
    ctx.fillStyle = "rgba(7,12,19,.92)";
    ctx.fill();
    ctx.strokeStyle = hexToRgba(color, .78);
    ctx.lineWidth = Math.max(1, 1.2 * view.zoom);
    ctx.stroke();
    ctx.fillStyle = "#dbeafe";
    ctx.textAlign = "left";
    ctx.fillText(clipText(source || "expr", 20), bx + 8 * view.zoom, y + 3.5 * view.zoom);
    if (recordHit) pinEditorHit.push({ x: bx, y: by, w: boxW, h: boxH, pin, expr, kind: "inline_expr" });
    window.__jetCanvasInlineExprChips = true;
    return boxW + 8 * view.zoom;
  }

  function drawPinDefaultEditor(graph, pin, x, y, recordHit) {
    const kind = editablePinKind(pin);
    if (!kind || pin.direction !== "input" || connectedPinIds.has(pin.pin_id)) return 0;
    const expr = inlineDefaultForPin(graph, pin);
    const source = defaultEditorValue(expr);
    if (expr && !isLiteralDefault(kind, source)) return drawInlineExprChip(pin, expr, x, y, recordHit);
    const color = colorForType(pin.type || "unknown");
    const boxW = Math.min(96, kind === "bool" ? 24 : kind === "enum" ? 88 : 76) * view.zoom;
    const boxH = 20 * view.zoom;
    const bx = x;
    const by = y - boxH / 2;
    roundRect(bx, by, boxW, boxH, 4 * view.zoom);
    ctx.fillStyle = "rgba(7,12,19,.92)";
    ctx.fill();
    ctx.strokeStyle = expr ? hexToRgba(color, .62) : "rgba(107,114,128,.52)";
    ctx.lineWidth = Math.max(.8, view.zoom);
    ctx.stroke();
    if (kind === "bool") {
      ctx.strokeStyle = expr ? color : "rgba(138,143,152,.75)";
      ctx.lineWidth = Math.max(1, 1.4 * view.zoom);
      ctx.strokeRect(bx + 6 * view.zoom, by + 5 * view.zoom, 10 * view.zoom, 10 * view.zoom);
      if (source === "true") {
        ctx.beginPath();
        ctx.moveTo(bx + 8 * view.zoom, by + 10 * view.zoom);
        ctx.lineTo(bx + 11 * view.zoom, by + 13 * view.zoom);
        ctx.lineTo(bx + 16 * view.zoom, by + 7 * view.zoom);
        ctx.stroke();
      }
    } else {
      ctx.font = `${Math.max(8, 10 * view.zoom)}px ${kind === "string" ? UI_FONT : MONO_FONT}`;
      ctx.fillStyle = expr ? "rgba(238,247,255,.82)" : "rgba(138,143,152,.72)";
      ctx.textAlign = "left";
      ctx.fillText(clipText(source || "default", kind === "string" ? 12 : 10), bx + 7 * view.zoom, y + 3.5 * view.zoom);
      if (kind === "enum") {
        ctx.fillStyle = color;
        ctx.fillText("▾", bx + boxW - 14 * view.zoom, y + 3.5 * view.zoom);
      }
    }
    if (recordHit) pinEditorHit.push({ x: bx, y: by, w: boxW, h: boxH, pin, expr, kind });
    window.__jetCanvasPinDefaultEditors = true;
    return boxW + 8 * view.zoom;
  }

  function compatibleActionType(accepted, actual) {
    if (!accepted || !actual) return true;
    if (accepted === actual) return true;
    if (accepted === "Any" || accepted === "Value") return true;
    if (actual === "Any" || actual === "Value") return true;
    return numericType(accepted) && numericType(actual);
  }

  function actionReturnType(action) {
    if (action.ret) return action.ret;
    const signature = String(action.signature || "");
    const m = signature.match(/->\s*([A-Za-z0-9_\[\]?:.]+)/);
    return m ? m[1] : "";
  }

  function functionsForPin(pin) {
    if (!pin) return actionEntries;
    const targetType = pin.type || null;
    let entries = actionEntries.filter((entry) => {
      if (!targetType) return true;
      if (pin.direction === "output") {
        return (entry.pins || []).some((p) => p.direction === "input" && compatibleActionType(p.type, targetType));
      }
      return compatibleActionType(pin.type, actionReturnType(entry) || entry.ret || "Value");
    });
    if (entries.length === 0) entries = actionEntries;
    return entries;
  }

  function closeContextMenu() {
    contextMenu.classList.remove("is-open");
    contextMenu.innerHTML = "";
    contextMenuState = null;
  }

  function actionMatchesQuery(action, query) {
    if (!query) return true;
    const q = query.toLowerCase();
    return [action.title, action.detail, action.group, action.kind, action.signature, action.summary, action.module_path, action.callee, action.ret, action.type]
      .some((value) => String(value || "").toLowerCase().includes(q));
  }

  function paletteCategoryForAction(action) {
    const group = String(action.group || "").toLowerCase();
    if (group.includes("flow") || ["insert_branch", "insert_switch", "insert_loop", "insert_fallible_rail"].includes(action.op)) return "Flow";
    if (action.kind === "variable_get" || action.kind === "variable_set" || group.includes("variable") || group.includes("binding") || action.op === "promote_to_binding") return "Variables";
    if (action.kind === "canvas.core_catalog" || group.includes("core")) return "Core";
    if (action.kind === "project_function" || group.includes("project") || action.kind === "canvas.action") return "Project";
    return "Flow";
  }

  function paletteActionGlyph(action) {
    if (action.kind === "variable_get" || action.kind === "variable_set") return action.type || action.ret || "•";
    if (action.kind === "canvas.core_catalog" || action.kind === "project_function" || action.kind === "canvas.action") return action.pure ? "ƒ" : "ƒ";
    if (action.op === "insert_branch") return "◇";
    if (action.op === "insert_loop") return "↻";
    if (action.op === "insert_return" || String(action.title || "").toLowerCase().includes("return")) return "⏎";
    if (action.op === "insert_switch") return "⇉";
    if (paletteCategoryForAction(action) === "Flow") return "◇";
    return "•";
  }

  function paletteActionColor(action) {
    if (action.kind === "variable_get" || action.kind === "variable_set") return colorForType(action.type || action.ret || "unknown");
    if (action.kind === "canvas.core_catalog" || action.kind === "project_function" || action.kind === "canvas.action") return action.pure ? NODE_ARCHETYPE_STYLES.function_pure.accent : NODE_ARCHETYPE_STYLES.function_exec.accent;
    if (action.op === "insert_branch" || action.op === "insert_switch" || action.op === "insert_loop") return "#f2f4f8";
    return colorForType(action.ret || action.type || "unknown");
  }

  function paletteTypeSummary(action) {
    const signature = action.signature || action.detail || "";
    if (signature) return signature;
    if ((action.pins || []).length) return (action.pins || []).filter((p) => p.direction === "input").map((p) => p.type || "Value").join(", ") + " -> " + (action.ret || "Void");
    return action.detail || "";
  }

  function renderActionPalette() {
    if (!contextMenuState) return;
    const query = contextMenuState.query || "";
    const matches = contextMenuState.actions
      .filter((action) => actionMatchesQuery(action, query))
      .sort((a, b) => rankAction(b) - rankAction(a) || String(a.module_path || "").localeCompare(String(b.module_path || "")) || String(a.title).localeCompare(String(b.title)));
    const context = contextMenuState.pin ? `${contextMenuState.pin.name}: ${contextMenuState.pin.type}` : `All nodes · ${matches.length}/${contextMenuState.actions.length}`;
    const port = contextMenuState.pin ? pinPortHtml(contextMenuState.pin.type || "Value") : "";
    const favorites = favoriteSet();
    const rowForAction = (action) => {
      const id = action.action_id || action.callee || action.title;
      const fav = favorites.has(id);
      const color = paletteActionColor(action);
      return `<button class="action-result${fav ? " is-favorite" : ""}" data-menu-action="${escapeAttr(action.index)}" style="--action-color:${escapeAttr(color)}"><span class="action-glyph">${escapeHtml(paletteActionGlyph(action))}</span><span>${fav ? "★ " : ""}${escapeHtml(action.title)}<small style="color:${escapeAttr(color)}">${escapeHtml(paletteTypeSummary(action))}</small></span></button>`;
    };
    const categories = ["Flow", "Variables", "Project", "Core"].map((category) => {
      const limit = category === "Core" ? 1000 : category === "Project" ? 500 : category === "Variables" ? 200 : 64;
      const rows = matches.filter((action) => paletteCategoryForAction(action) === category).slice(0, limit);
      if (!rows.length && query) return "";
      let body = "<div class=\"action-empty\">No actions</div>";
      if (rows.length && category === "Core") {
        const modules = [];
        for (const action of rows) {
          const module = action.module_path || "core";
          let bucket = modules.find((item) => item.module === module);
          if (!bucket) {
            bucket = { module, rows: [] };
            modules.push(bucket);
          }
          bucket.rows.push(action);
        }
        body = modules.map((bucket) => `<h4>${escapeHtml(bucket.module)}</h4>${bucket.rows.map(rowForAction).join("")}`).join("");
      } else if (rows.length) {
        body = rows.map(rowForAction).join("");
      }
      return `<section class="action-category"><h3>${escapeHtml(category)}</h3>${body}</section>`;
    }).join("");
    const countTag = contextMenuState.pin ? `<span class="tag">${matches.length}/${contextMenuState.actions.length}</span>` : "";
    contextMenu.innerHTML = `<div class="action-palette-head"><div class="menu-title">${escapeHtml(contextMenuState.title)}</div><div class="action-context">${port}<span>${escapeHtml(context)}</span>${countTag}</div><input id="action-palette-search" placeholder="Search actions" value="${escapeAttr(query)}"></div><div class="action-results">${categories || "<div class=\"action-empty\">No matching actions</div>"}</div>`;
    const input = document.getElementById("action-palette-search");
    if (input) {
      input.addEventListener("input", () => {
        contextMenuState.query = input.value || "";
        renderActionPalette();
        const next = document.getElementById("action-palette-search");
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      });
      input.addEventListener("keydown", (ev) => {
        if (ev.key === "Escape") {
          ev.preventDefault();
          closeContextMenu();
        } else if (ev.key === "Enter") {
          const first = contextMenu.querySelector("[data-menu-action]");
          if (first) {
            ev.preventDefault();
            first.click();
          }
        }
      });
    }
    contextMenu.querySelectorAll("[data-menu-action]").forEach((button) => {
      button.addEventListener("click", () => {
        const action = contextMenuState && contextMenuState.actions.find((item) => String(item.index) === button.getAttribute("data-menu-action"));
        closeContextMenu();
        if (action) {
          markActionUsed(action);
          action.run();
        }
      });
    });
  }

  function openActionPalette(x, y, title, actions, opts = {}) {
    contextMenuState = {
      title,
      actions: (actions.length ? actions : [{ title: "No compatible actions", detail: "source-backed only", group: "empty", run: () => {} }]).map((action, index) => Object.assign({ index }, action)),
      pin: opts.pin || null,
      context: opts.context || "",
      query: opts.query || ""
    };
    renderActionPalette();
    contextMenu.style.left = Math.min(x, window.innerWidth - 430) + "px";
    contextMenu.style.top = Math.min(y, window.innerHeight - 430) + "px";
    contextMenu.classList.add("is-open");
    const input = document.getElementById("action-palette-search");
    if (input) input.focus();
  }

  function openContextMenu(x, y, title, actions) {
    openActionPalette(x, y, title, actions, { context: "node actions" });
  }

  function openPinMenu(pin, x, y) {
    const entries = functionsForPin(pin);
    const actions = entries.map((entry) => ({
      title: entry.title,
      detail: entry.detail,
      group: paletteCategoryForAction(entry),
      kind: entry.kind,
      module_path: entry.module_path,
      signature: entry.signature,
      summary: entry.summary,
      pure: entry.pure,
      pins: entry.pins,
      ret: entry.ret,
      action_id: entry.action_id,
      callee: entry.callee,
      insert_callee: entry.insert_callee,
      args: entry.args,
      run: () => runPalette(entry, pin)
    }));
    actions.push({
      title: "Use same value twice",
      detail: "Refused: E0204. Fix: clone first or end the active change before reading it again.",
      group: "refused",
      run: () => showToast("E0204: clone first or end the active change")
    });
    actions.unshift({
      title: "Create function accepting " + (pin.type || "Value"),
      detail: pin.name || "value",
      group: "function",
      run: () => {
        const base = String(pin.name || "value").replace(/[^A-Za-z0-9_]/g, "_").replace(/^[^A-Za-z_]+/, "") || "value";
        const name = window.prompt("Function name", "use_" + base);
        if (name) postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "value: " + (pin.type || "Int"), ret_type: "Void" });
      }
    });
    if (pin.direction === "input") {
      actions.unshift({
        title: "Promote pin to binding",
        detail: pin.type || "value",
        group: "refactor",
        run: () => {
          const name = window.prompt("Binding name", pin.name || "value");
          const graph = latestDoc ? currentGraph(latestDoc) : null;
          const expr = graph && (graph.inline_exprs || []).find((e) => e.source_span && pin.source_span && spansOverlap(e.source_span, pin.source_span));
          if (name && expr) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, name });
          else showToast("Select an inline expression to promote");
        }
      });
    }
    openActionPalette(x, y, "Compatible actions", actions, { pin, context: "pin actions" });
  }

  function richestGraph(doc) {
    if (!doc.graphs || doc.graphs.length === 0) return null;
    return doc.graphs.slice().sort((a, b) => b.nodes.length - a.nodes.length || a.title.localeCompare(b.title))[0];
  }

  function syncGraphPicker(doc) {
    const best = richestGraph(doc);
    if (!selectedGraphId && best) selectedGraphId = best.graph_id;
    graphSelect.innerHTML = "";
    for (const graph of doc.graphs || []) {
      const opt = document.createElement("option");
      opt.value = graph.graph_id;
      opt.textContent = graph.title + " (" + graph.nodes.length + ")";
      graphSelect.appendChild(opt);
    }
    if (selectedGraphId) graphSelect.value = selectedGraphId;
  }

  function syncGraphList(doc) {
    graphList.innerHTML = "";
    for (const graph of doc.graphs || []) {
      const button = document.createElement("button");
      button.className = "graph-item" + (graph.graph_id === selectedGraphId ? " is-active" : "");
      button.type = "button";
      button.setAttribute("data-sidebar-graph", graph.graph_id);
      button.innerHTML = "<span>" + escapeHtml(graph.title) + "</span><span class=\"count\">" + graph.nodes.length + "</span>";
      button.addEventListener("click", () => {
        switchGraph(graph.graph_id);
      });
      graphList.appendChild(button);
    }
  }

  function projectMiniCard(title, small, code) {
    return `<div class="project-card"><b>${escapeHtml(title || "")}</b><small>${escapeHtml(small || "")}</small><code>${escapeHtml(code || "")}</code></div>`;
  }

  function syncProjectPanel(panel, title, rows, empty) {
    if (!panel) return;
    panel.innerHTML = `<h3>${escapeHtml(title)}</h3>` + (rows.length ? rows.join("") : `<div class="tag">${escapeHtml(empty)}</div>`);
  }

  function collectProjectDiagnostics(project) {
    const rows = [];
    const push = (scope, diag) => rows.push(projectMiniCard(diag.code || "diagnostic", scope, diag.what || diag.message || diag.fix || "project diagnostic"));
    for (const diag of project.diagnostics || []) push("project", diag);
    if (project.workspace) for (const diag of project.workspace.diagnostics || []) push(project.workspace.path || "workspace.jet", diag);
    for (const pkg of project.packages || []) for (const diag of pkg.diagnostics || []) push(pkg.manifest || pkg.path || "pkg.jet", diag);
    for (const env of project.envs || []) for (const diag of env.diagnostics || []) push(env.path || "env.jet", diag);
    return rows;
  }

  function syncProjectPanels(project) {
    const packageRows = (project.packages || []).map((pkg) => {
      const targets = (pkg.targets || []).map((t) => `${t.package || pkg.name}:${t.target}`).join(", ") || pkg.target || "native";
      return projectMiniCard(pkg.name || pkg.path || "package", `${pkg.path || ""} · ${pkg.version || ""}`, targets + (pkg.effects_enabled ? " · effects" : ""));
    });
    const depRows = [];
    for (const pkg of project.packages || []) {
      for (const dep of pkg.deps || []) depRows.push(projectMiniCard(dep.name || "dependency", pkg.name || pkg.path || "package", dep.source || "source"));
    }
    const devRows = [];
    for (const env of project.envs || []) {
      const packages = (env.packages || []).join(", ") || "no packages";
      const secrets = (env.secrets || []).length ? `${env.secrets.length} secrets` : "no secrets";
      devRows.push(projectMiniCard(env.path || "env.jet", env.prompt || "dev", `${packages} · ${secrets}`));
    }
    for (const svc of project.services || []) {
      const ports = (svc.ports || []).join(", ") || "no ports";
      devRows.push(projectMiniCard(svc.name || "service", svc.enable === false ? "disabled" : "enabled", `${ports} · ${svc.ready || svc.init || "no command"}`));
    }
    const diagRows = collectProjectDiagnostics(project);
    const lockRows = (project.locks || []).map((lock) => projectMiniCard(lock.path || ".jet/lock", lock.kind || "lock", lock.revision || ""));
    const policy = project.state_policy || {};
    lockRows.unshift(projectMiniCard("Source truth", project.source_control && project.source_control.truth || "git-text", `${policy.semantic || "source"} semantics · ${(policy.local || []).join(", ") || "local viewport"}`));
    syncProjectPanel(packageSummary, "Packages", packageRows, "no packages");
    syncProjectPanel(dependencySummary, "Dependencies", depRows, "no dependencies");
    syncProjectPanel(devSummary, "Dev", devRows, "no env or services");
    syncProjectPanel(diagnosticsSummary, "Diagnostics", diagRows, "clean");
    syncProjectPanel(trustSummary, "Trust", lockRows, "source-only");
    window.__jetCanvasWorkspacePanels = {
      packages: packageRows.length,
      dependencies: depRows.length,
      dev: devRows.length,
      diagnostics: diagRows.length,
      trust: lockRows.length
    };
  }

  function syncProjectRail(project) {
    if (!projectRail || !project) return;
    latestProject = project;
    projectMode.textContent = project.mode || "file";
    const cards = [];
    cards.push(`<div class="project-card"><b>${escapeHtml(project.entry || "entry")}</b><small>${escapeHtml(project.project_root || "")}</small><span class="tag">${escapeHtml(project.mode || "single_file")}</span></div>`);
    for (const pkg of (project.packages || []).slice(0, 8)) {
      const deps = (pkg.deps || []).map((d) => d.name).join(", ") || "no deps";
      const targets = (pkg.targets || []).map((t) => t.target).join(", ") || (pkg.target || "native");
      cards.push(`<div class="project-card" data-project-package="${escapeAttr(pkg.path || "")}"><b>${escapeHtml(pkg.name || pkg.path || "package")}</b><small>${escapeHtml(pkg.path || "")}</small><code>${escapeHtml(targets)} · ${escapeHtml(deps)}</code></div>`);
    }
    const fileCount = (project.files || []).length;
    const dirty = project.source_control && project.source_control.truth ? project.source_control.truth : "git-text";
    cards.push(`<div class="project-card"><b>${fileCount} source-truth files</b><small>${escapeHtml(dirty)}</small><code>${escapeHtml((project.state_policy && project.state_policy.semantic) || "source")} semantics</code></div>`);
    for (const file of (project.files || []).filter((f) => f.kind === "source").slice(0, 12)) {
      const active = (selectedSourceId || project.entry) === file.path ? " is-active" : "";
      cards.push(`<button class="project-card${active}" type="button" data-project-file="${escapeAttr(file.path || "")}"><b>${escapeHtml(file.path || "source")}</b><small>${escapeHtml(file.kind || "source")}</small><code>${escapeHtml(file.revision || "")}</code></button>`);
    }
    projectRail.innerHTML = cards.join("");
    syncProjectPanels(project);
    window.__jetCanvasProjectRail = { mode: project.mode, packages: (project.packages || []).length, files: fileCount, panels: window.__jetCanvasWorkspacePanels };
  }

  if (projectRail) {
    projectRail.addEventListener("click", (event) => {
      const card = event.target.closest("[data-project-file]");
      if (!card) return;
      const sourceId = card.getAttribute("data-project-file");
      if (sourceId) loadGraph(sourceId);
    });
  }

  function loadProject() {
    const projectUrl = window.__JET_CANVAS_PROJECT__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/project");
    return fetch(projectUrl, { cache: "no-store" })
      .then((r) => r.json())
      .then((project) => {
        syncProjectRail(project);
        return project;
      })
      .catch(() => {
        if (projectRail) projectRail.innerHTML = "<div class=\"tag\">project unavailable</div>";
        return null;
      });
  }

  function syncWireStatus(state) {
    if (!wireStatus) return;
    let title = "Ready";
    let detail = "Drag from a socket or right-click the canvas";
    let color = "#7dd3fc";
    if (state) {
      title = state.title || title;
      detail = state.detail || detail;
      color = state.color || color;
    } else if (hoverPin) {
      title = "Socket";
      detail = pinName(hoverPin) + " : " + exactPinType(hoverPin);
      color = colorForType(hoverPin.type || "Value");
    }
    wireStatus.style.setProperty("--wire-color", color);
    wireStatus.innerHTML = `<span id="wire-status-dot"></span><b>${escapeHtml(title)}</b><span>${escapeHtml(detail)}</span>`;
  }

  function graphRailSummary(graph) {
    return graph && graph.rails && graph.rails.kinds && graph.rails.kinds.length ? graph.rails.kinds.join(", ") : "data";
  }

  function syncGraphOverview(graph, node) {
    if (!graphOverview || !graph) return;
    const fn = graph.function || {};
    const signature = fn.signature || graph.title || graph.graph_id;
    const execPins = (graph.pins || []).filter(isExecPin).length;
    const dataPins = (graph.pins || []).length - execPins;
    const selected = node ? nodeKindLabel(node, graph) + " / " + node.title : "none";
    graphOverview.innerHTML = `
      <div class="graph-overview-title"><b>${escapeHtml(graph.title || graph.graph_id)}</b><code>${escapeHtml(signature)}</code></div>
      <div class="graph-stats">
        <div class="graph-stat"><b>${graph.nodes.length}</b><span>nodes</span></div>
        <div class="graph-stat"><b>${graph.wires.length}</b><span>wires</span></div>
        <div class="graph-stat"><b>${dataPins}/${execPins}</b><span>data/exec</span></div>
      </div>
      <div class="tag">${escapeHtml(graphRailSummary(graph))} / ${escapeHtml(selected)}</div>
    `;
    window.__jetCanvasGraphOverview = {
      graph_id: graph.graph_id,
      title: graph.title || graph.graph_id,
      nodes: graph.nodes.length,
      wires: graph.wires.length,
      data_pins: dataPins,
      exec_pins: execPins,
      selected
    };
  }

  function variableActionsForGraph(graph) {
    if (!graph) return [];
    const vars = new Map();
    const remember = (name, type) => {
      const clean = String(name || "").replace(/^get\s+/, "");
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(clean)) return;
      if (["result", "value", "exec", "then", "else", "return"].includes(clean)) return;
      if (!vars.has(clean)) vars.set(clean, type || "Value");
    };
    for (const node of graph.nodes || []) {
      if (node.kind === "binding" || node.kind === "assign") {
        const out = (graph.pins || []).find((pin) => pin.node_id === node.node_id && pin.direction === "output" && !isExecPin(pin));
        remember(node.title, out && out.type);
      }
      if (node.kind === "variable_get") {
        const out = (graph.pins || []).find((pin) => pin.node_id === node.node_id && pin.direction === "output" && !isExecPin(pin));
        remember(node.title, out && out.type);
      }
      if (node.kind === "entry") {
        for (const pin of (graph.pins || []).filter((p) => p.node_id === node.node_id && p.direction === "output" && !isExecPin(p))) remember(pin.name, pin.type);
      }
    }
    const actions = [];
    for (const [name, type] of Array.from(vars.entries()).sort((a, b) => a[0].localeCompare(b[0]))) {
      actions.push({
        title: name,
        detail: name + " : " + type,
        group: "Variables",
        kind: "variable_get",
        type,
        ret: type,
        variable: name,
        signature: name + " : " + type,
        run: () => {
          const pin = contextMenuState && contextMenuState.pin;
          const graphNow = currentGraphOrNull();
          const expr = pin && pin.direction === "input" && inlineForPin(graphNow, pin);
          if (expr) postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: name });
          else showToast("Drag from an input expression to use " + name);
        }
      });
      actions.push({
        title: "set " + name,
        detail: name + " : " + type,
        group: "Variables",
        kind: "variable_set",
        type,
        ret: type,
        variable: name,
        signature: name + " = value",
        run: () => showToast("Variable set needs source-backed assign insertion")
      });
    }
    window.__jetCanvasVariablePalette = actions.length;
    return actions;
  }

  function nodeContextActions(graph, node) {
    const actions = [
      { title: "Jump source", detail: "span", group: "source", run: () => { const s = node.source_span || { start: 0, end: 0 }; setSourceHash(s); setViewMode("code"); } },
      { title: "Find references", detail: "semindex", group: "query", run: () => postQuery({ op: "references", symbol: node.title }) },
      { title: "Set breakpoint", detail: "local span", group: "debug", run: () => toggleBreakpoint(node) }
    ];
    if (graphForFunctionName(node.title)) actions.unshift({ title: "Open function graph", detail: "nested graph", group: "graph", run: () => openFunctionGraph(node.title) });
    return actions;
  }

  function syncGraphStrip(doc) {
    graphStrip.innerHTML = "";
    if (graphCount) graphCount.textContent = String((doc.graphs || []).length);
    for (const graph of doc.graphs || []) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "graph-tab" + (graph.graph_id === selectedGraphId ? " is-active" : "");
      button.setAttribute("data-graph-tab", graph.graph_id);
      button.title = "Open graph: " + graph.title;
      button.innerHTML = "<span class=\"graph-tab-kind\">fn</span><span class=\"graph-tab-title\">" + escapeHtml(graph.title) + "</span><span class=\"graph-tab-count\">" + graph.nodes.length + "</span>";
      button.addEventListener("click", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        switchGraph(graph.graph_id);
      });
      graphStrip.appendChild(button);
    }
    window.__jetCanvasGraphSwitcherReady = true;
    window.__jetCanvasGraphTabCount = graphStrip.children.length;
  }

  function graphActionItems() {
    const graph = currentGraphOrNull();
    const actions = [
      { title: "Fit graph", detail: "viewport", group: "view", run: fitGraph },
      { title: "New function", detail: "source", group: "function", run: () => {
        const name = window.prompt("Function name", "helper");
        if (name) postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "", ret_type: "Int" });
      } },
      { title: "Show source", detail: "toggle", group: "Flow", run: () => setViewMode("code") },
      { title: "Align top", detail: "local view state", group: "Flow", run: () => alignSelectedNodes("y") },
      { title: "Align left", detail: "local view state", group: "Flow", run: () => alignSelectedNodes("x") },
      { title: "Auto tidy", detail: "local view state", group: "Flow", run: tidyGraphLayout },
      { title: "Straighten wires", detail: "Bezier wires stay source-faithful", group: "Flow", run: () => { wireStyle = "bezier"; showToast("Wires use Blueprint curves"); drawGraph(latestDoc); } },
      { title: "Add reroute knot", detail: "local view state", group: "Flow", run: addRerouteKnot },
      { title: "Bookmark graph", detail: "local editor state", group: "navigation", run: bookmarkCurrentGraph },
      { title: "Run graph", detail: "debug overlay", group: "run", run: runCurrentGraph }
    ];
    actions.push(...variableActionsForGraph(graph));
    for (const item of palette.concat(actionEntries)) {
      actions.push({ title: item.title, detail: item.detail || "", group: item.group || (item.op === "preview_canvas_action" ? "Project" : "Flow"), kind: item.kind, module_path: item.module_path, signature: item.signature, summary: item.summary, pure: item.pure, pins: item.pins, ret: item.ret, type: item.type, op: item.op, action_id: item.action_id, callee: item.callee, insert_callee: item.insert_callee, args: item.args, run: () => runPalette(item) });
    }
    return actions;
  }

  function openGraphActionPalette(x, y, query) {
    openActionPalette(x, y, "Graph actions", graphActionItems(), { context: "All nodes", query: query || "" });
  }

  function openCoreCatalogPalette(query = "") {
    if (!coreCatalogLoaded && !coreCatalogLoading) {
      loadCoreCatalogActions("").then(() => openCoreCatalogPalette(query));
      return;
    }
    if (!coreCatalogLoaded && coreCatalogLoading) {
      coreCatalogLoading.then(() => openCoreCatalogPalette(query));
      return;
    }
    const actions = actionEntries
      .filter((item) => item.kind === "canvas.core_catalog")
      .map((item) => ({
        title: item.title,
        detail: item.detail,
        group: "Core",
        kind: item.kind,
        module_path: item.module_path,
        signature: item.signature,
        pure: item.pure,
        pins: item.pins,
        ret: item.ret,
        action_id: item.action_id,
        callee: item.callee,
        insert_callee: item.insert_callee,
        args: item.args,
        run: () => runPalette(item)
      }));
    openActionPalette(window.innerWidth / 2 - 210, 72, "Core catalog", actions, { context: "core.* modules and methods", query });
  }

  function currentGraph(doc) {
    return (doc.graphs || []).find((g) => g.graph_id === selectedGraphId) || richestGraph(doc);
  }

  function graphById(id) {
    return latestDoc && (latestDoc.graphs || []).find((graph) => graph.graph_id === id) || null;
  }

  function graphTitle(id) {
    const graph = graphById(id);
    return graph ? graph.title : "graph";
  }

  function updateGraphNav(graph) {
    graphBack.disabled = graphBackStack.length === 0;
    graphForward.disabled = graphForwardStack.length === 0;
    const trail = graphBackStack.slice(-2).map(graphTitle).concat(graph ? [graph.title] : []);
    jump.textContent = trail.join("  ›  ") + (graph ? " - " + graph.nodes.length + " nodes" + (selectedNodeIds.size > 1 ? " / " + selectedNodeIds.size + " selected" : "") : "");
  }

  function switchGraph(graphId, opts = {}) {
    if (!latestDoc || !graphId) return;
    const graph = graphById(graphId);
    if (!graph) return;
    const push = opts.push !== false && selectedGraphId && selectedGraphId !== graphId;
    if (push) {
      graphBackStack.push(selectedGraphId);
      graphForwardStack = [];
    }
    selectedGraphId = graphId;
    window.__jetCanvasSelectedGraphId = selectedGraphId;
    selectedNodeId = opts.nodeId || graph.entry_node;
    selectedNodeIds = new Set([selectedNodeId]);
    setViewMode("graph");
    if (opts.fit === false) drawGraph(latestDoc);
    else fitGraph();
    if (opts.toast) showToast(opts.toast);
  }

  function graphForFunctionName(name) {
    if (!latestDoc || !name) return null;
    return (latestDoc.graphs || []).find((graph) => graph.title === name) || null;
  }

  function openFunctionGraph(name) {
    const graph = graphForFunctionName(name);
    if (!graph) return false;
    switchGraph(graph.graph_id, { toast: "Opened " + graph.title });
    return true;
  }

  function debugStorageKey(doc) {
    return "jet.canvas.debug:" + (doc.source_id || "source");
  }

  function loadDebugState(doc) {
    const key = debugStorageKey(doc);
    if (debugState.key === key && debugState.revision === doc.revision) return;
    try {
      debugState = JSON.parse(localStorage.getItem(key) || "null") || { breakpoints: [], watches: [] };
    } catch (_) {
      debugState = { breakpoints: [], watches: [] };
    }
    debugState.key = key;
    debugState.revision = doc.revision;
    debugState.breakpoints = (debugState.breakpoints || []).filter((b) => b.revision === doc.revision);
    debugState.watches = debugState.watches || [];
  }

  function saveDebugState() {
    if (!debugState.key) return;
    localStorage.setItem(debugState.key, JSON.stringify({
      breakpoints: debugState.breakpoints || [],
      watches: debugState.watches || [],
      revision: debugState.revision
    }));
  }

  function spanAnchor(span) {
    if (!span) return "";
    return String(span.start) + ":" + String(span.end);
  }

  function nodeBreakpoint(node) {
    const anchor = spanAnchor(node && node.source_span);
    return (debugState.breakpoints || []).find((b) => b.anchor === anchor);
  }

  function toggleBreakpoint(node) {
    if (!latestDoc || !node || !node.source_span) return;
    loadDebugState(latestDoc);
    const anchor = spanAnchor(node.source_span);
    const before = debugState.breakpoints.length;
    debugState.breakpoints = debugState.breakpoints.filter((b) => b.anchor !== anchor);
    if (debugState.breakpoints.length === before) {
      debugState.breakpoints.push({ anchor, source_span: node.source_span, node_id: node.node_id, revision: latestDoc.revision });
      showToast("Breakpoint anchored to source span");
    } else {
      showToast("Breakpoint removed");
    }
    saveDebugState();
    drawGraph(latestDoc);
  }

  function addWatch(name) {
    if (!name) return;
    loadDebugState(latestDoc);
    if (!debugState.watches.includes(name)) debugState.watches.push(name);
    saveDebugState();
    showToast("Watch added: " + name);
  }

  function runDebug(commands) {
    if (!latestDoc) return;
    loadDebugState(latestDoc);
    const debugUrl = window.__JET_CANVAS_DEBUG__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/debug");
    const body = {
      schema_version: 1,
      revision: latestDoc.revision,
      commands,
      breakpoint_spans: (debugState.breakpoints || []).map((b) => b.anchor),
      watches: debugState.watches || []
    };
    fetch(debugUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) {
          debugOverlay = null;
          showToast((result.json.message || "Debug rejected").split("\n")[0]);
          return;
        }
        debugOverlay = result.json.overlay || null;
        if (debugOverlay && debugOverlay.active_graph_id) selectedGraphId = debugOverlay.active_graph_id;
        if (debugOverlay && debugOverlay.active_node_id) selectedNodeId = debugOverlay.active_node_id;
        showToast("Debug " + ((debugOverlay && debugOverlay.debug_overlay) || "updated"));
        drawGraph(latestDoc);
      })
      .catch((e) => showToast(String(e)));
  }

  function stopDebug() {
    debugOverlay = null;
    showToast("Debug overlay stopped");
    if (latestDoc) drawGraph(latestDoc);
  }

  function rankedGraphLayout(graph) {
    const nodes = graph.nodes || [];
    const byId = new Map(nodes.map((node) => [node.node_id, node]));
    const pinNode = new Map((graph.pins || []).map((pin) => [pin.pin_id, pin.node_id]));
    const rank = new Map(nodes.map((node) => [node.node_id, node.node_id === graph.entry_node ? 0 : Math.max(0, Math.round(rawNodeX(node) / 220))]));
    for (let pass = 0; pass < nodes.length + 3; pass++) {
      let changed = false;
      for (const wire of graph.wires || []) {
        const from = pinNode.get(wire.from_pin);
        const to = pinNode.get(wire.to_pin);
        if (!from || !to || from === to) continue;
        const next = Math.max(rank.get(to) || 0, (rank.get(from) || 0) + 1);
        if (next !== (rank.get(to) || 0)) {
          rank.set(to, next);
          changed = true;
        }
      }
      if (!changed) break;
    }
    const columns = new Map();
    for (const node of nodes) {
      const r = rank.get(node.node_id) || 0;
      if (!columns.has(r)) columns.set(r, []);
      columns.get(r).push(node);
    }
    const sortedRanks = Array.from(columns.keys()).sort((a, b) => a - b);
    const colWidth = new Map();
    for (const r of sortedRanks) {
      colWidth.set(r, Math.max(...columns.get(r).map((node) => nodeSize(graph, node).w), 150));
    }
    const colX = new Map();
    let x = 80;
    for (const r of sortedRanks) {
      colX.set(r, x);
      x += (colWidth.get(r) || 150) + 56;
    }
    const positions = new Map();
    for (const r of sortedRanks) {
      const col = columns.get(r).slice().sort((a, b) => {
        const family = (node) => node.node_id === graph.entry_node ? -2 : node.kind === "variable_get" || node.kind === "constant" ? -1 : node.kind === "return" ? 2 : node.kind === "branch" || node.archetype === "control" ? 1 : 0;
        return family(a) - family(b) || rawNodeY(a) - rawNodeY(b) || rawNodeX(a) - rawNodeX(b);
      });
      let y = 70;
      for (const node of col) {
        const size = nodeSize(graph, node);
        positions.set(node.node_id, { x: colX.get(r), y });
        y += size.h + 44;
      }
    }
    for (const wire of graph.wires || []) {
      const from = byId.get(pinNode.get(wire.from_pin));
      const to = byId.get(pinNode.get(wire.to_pin));
      if (!from || !to || from === to) continue;
      const fp = positions.get(from.node_id);
      const tp = positions.get(to.node_id);
      const fs = nodeSize(graph, from);
      if (fp && tp && tp.x < fp.x + fs.w + 40) tp.x = fp.x + fs.w + 40;
    }
    return positions;
  }

  function reflowGraph(graph) {
    autoNodeOffsets = new Map();
    if (!graph || !graph.nodes || graph.nodes.length === 0) return;
    const colGap = 40;
    const ranked = rankedGraphLayout(graph);
    for (const node of graph.nodes) {
      const pos = ranked.get(node.node_id);
      if (pos) autoNodeOffsets.set(node.node_id, { x: pos.x - rawNodeX(node), y: pos.y - rawNodeY(node) });
    }
    const placed = [];
    for (const node of graph.nodes.slice().sort((a, b) => nodeY(a) - nodeY(b) || nodeX(a) - nodeX(b))) {
      const size = nodeSize(graph, node);
      let offset = autoNodeOffset(node);
      let box = { x: rawNodeX(node) + offset.x, y: rawNodeY(node) + offset.y, w: size.w, h: size.h };
      let moved = true;
      while (moved) {
        moved = false;
        for (const other of placed) {
          const overlapX = box.x < other.x + other.w + colGap && box.x + box.w + colGap > other.x;
          const overlapY = box.y < other.y + other.h + 28 && box.y + box.h + 28 > other.y;
          if (overlapX && overlapY) {
            box.y = other.y + other.h + 36;
            moved = true;
          }
        }
      }
      autoNodeOffsets.set(node.node_id, { x: offset.x, y: box.y - rawNodeY(node) });
      placed.push(box);
    }
  }

  function graphBounds(graph) {
    if (!graph || graph.nodes.length === 0) return { minX: 0, minY: 0, maxX: 600, maxY: 360 };
    reflowGraph(graph);
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of graph.nodes) {
      const size = nodeSize(graph, n);
      minX = Math.min(minX, nodeX(n));
      minY = Math.min(minY, nodeY(n));
      maxX = Math.max(maxX, nodeX(n) + size.w);
      maxY = Math.max(maxY, nodeY(n) + size.h);
    }
    return { minX, minY, maxX, maxY };
  }

  function fitGraph() {
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    if (!graph) return;
    const b = graphBounds(graph);
    const size = cssSize();
    const compact = compactCanvasMode();
    const leftInset = 22;
    const topInset = compact ? (developerMode ? 154 : 52) : (developerMode ? 108 : 32);
    const bottomInset = 38;
    const zx = (size.width - leftInset - 28) / Math.max(1, b.maxX - b.minX);
    const zy = (size.height - topInset - bottomInset) / Math.max(1, b.maxY - b.minY);
    view.zoom = Math.max(.42, Math.min(1.05, Math.min(zx, zy)));
    view.x = leftInset - b.minX * view.zoom;
    view.y = topInset - b.minY * view.zoom;
    drawGraph(latestDoc);
  }

  function drawGrid(size) {
    ctx.fillStyle = "#101318";
    ctx.fillRect(0, 0, size.width, size.height);
    const major = 128 * view.zoom;
    const minor = 16 * view.zoom;
    ctx.lineWidth = 1;
    for (const step of [minor, major]) {
      if (step < 6) continue;
      ctx.strokeStyle = step === major ? "#20262f" : "#161a21";
      ctx.beginPath();
      let ox = view.x % step;
      let oy = view.y % step;
      for (let x = ox; x < size.width; x += step) { ctx.moveTo(x, 0); ctx.lineTo(x, size.height); }
      for (let y = oy; y < size.height; y += step) { ctx.moveTo(0, y); ctx.lineTo(size.width, y); }
      ctx.stroke();
    }
  }

  function drawTypeLegend(size) {
    if (!detailToggles.types || compactCanvasMode() || size.width < 760 || viewMode === "code") return;
    const items = [
      ["Exec", "exec"],
      ["Bool", "Bool"],
      ["Int", "Int"],
      ["Float", "Float"],
      ["String", "String"],
      ["Fallible", "Value?"]
    ];
    const x = Math.max(14, size.width - 430);
    const y = 58;
    const w = Math.min(416, size.width - x - 12);
    const h = 34;
    roundRect(x, y, w, h, 6);
    ctx.fillStyle = "rgba(8,17,29,.78)";
    ctx.fill();
    ctx.strokeStyle = "rgba(54,90,127,.72)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.font = "10px ui-monospace, Consolas, monospace";
    ctx.textAlign = "left";
    let cursor = x + 12;
    for (const [label, type] of items) {
      const color = colorForType(type);
      ctx.beginPath();
      if (type === "exec") {
        ctx.moveTo(cursor, y + 12);
        ctx.lineTo(cursor + 10, y + 17);
        ctx.lineTo(cursor, y + 22);
        ctx.closePath();
      } else {
        ctx.arc(cursor + 5, y + 17, 5, 0, Math.PI * 2);
      }
      ctx.fillStyle = color;
      ctx.fill();
      ctx.fillStyle = "#b9c9df";
      ctx.fillText(label, cursor + 15, y + 20);
      cursor += Math.min(72, 22 + ctx.measureText(label).width);
    }
    ctx.textAlign = "left";
  }

  function isExecPin(pin) {
    return pinRail(pin) === "control";
  }

  function pinsForNode(graph, node, direction, exec) {
    return (graph.pins || []).filter((pin) => pin.node_id === node.node_id && pin.direction === direction && isExecPin(pin) === exec);
  }

  function nodeStyle(node, graph) {
    const archetype = node.archetype || (node.kind === "entry" ? "entry" : node.kind === "variable_get" || node.kind === "constant" ? "value" : "function_exec");
    const style = Object.assign({ fill: "rgba(29,33,41,.96)" }, NODE_ARCHETYPE_STYLES[archetype] || NODE_ARCHETYPE_STYLES.value);
    if (node.kind === "constant") {
      const out = pinsForNode(graph, node, "output", false)[0];
      return Object.assign({}, style, { accent: colorForType(out && out.type || "unknown"), header: colorForType(out && out.type || "unknown"), label: "Literal", glyph: "•", subtitle: "literal" });
    }
    if (node.kind === "variable_get") {
      const out = pinsForNode(graph, node, "output", false)[0];
      return Object.assign({}, style, { accent: colorForType(out && out.type || "unknown"), header: colorForType(out && out.type || "unknown"), label: "Value", glyph: "•", subtitle: "value" });
    }
    if (node.kind === "binding" || node.kind === "assign") {
      const dataPin = pinsForNode(graph, node, "output", false)[0] || pinsForNode(graph, node, "input", false)[0] || {};
      const typeColor = colorForType(dataPin.type || "unknown");
      return Object.assign({}, NODE_ARCHETYPE_STYLES.control, { label: "Set variable", subtitle: "", glyph: "•", accent: typeColor, header: "#2c333d", header2: "#151a21" });
    }
    if (node.kind === "branch") return Object.assign({}, NODE_ARCHETYPE_STYLES.control, { glyph: "◇", subtitle: "branch" });
    if (node.kind === "dispatch") return Object.assign({}, NODE_ARCHETYPE_STYLES.control, { glyph: "⇉", subtitle: "switch" });
    if (node.kind === "loop") return Object.assign({}, NODE_ARCHETYPE_STYLES.control, { glyph: "↻", subtitle: "loop" });
    if (node.kind === "return") return Object.assign({}, NODE_ARCHETYPE_STYLES.control, { glyph: "⏎", subtitle: "return", accent: "#7dd3a6" });
    return style;
  }

  function nodeSubtitle(node) {
    return nodeStyle(node, currentGraphOrNull()).subtitle || "";
  }

  function nodeKindLabel(node, graph) {
    return nodeStyle(node, graph).label || node.kind || "Node";
  }

  function shouldDrawNodeBadge(node) {
    return developerMode && !!node && !!node.kind;
  }

  function isGetterCapsule(node) {
    return node && (node.kind === "variable_get" || node.kind === "constant");
  }

  function isOperatorNode(node) {
    return node && node.archetype === "function_pure" && /^[+\-*\/%=!<>&|^]+$/.test(node.title || "");
  }

  function simpleEmbeddedValue(expr) {
    const s = String((expr && expr.source) || "").trim();
    return /^[A-Za-z_][A-Za-z0-9_]*$/.test(s) || /^-?\d+(\.\d+)?$/.test(s) || /^"[^"]*"$/.test(s);
  }

  function nodeSize(graph, node) {
    const inputData = pinsForNode(graph, node, "input", false).length;
    const outputData = pinsForNode(graph, node, "output", false).length;
    const execIn = pinsForNode(graph, node, "input", true).length;
    const execOut = pinsForNode(graph, node, "output", true).length;
    const compact = isGetterCapsule(node) || isOperatorNode(node);
    if (node.archetype === "entry") return { w: Math.max(174, 118 + String(node.title || "").length * 8, 148 + Math.max(inputData, outputData) * 16), h: 66 + Math.max(inputData, outputData) * 24 };
    if (isOperatorNode(node)) return { w: 88, h: Math.max(58, 28 + Math.max(inputData, outputData) * 22) };
    const titleW = 92 + String(node.title || "").length * 7.5;
    const pinW = Math.max(inputData, outputData) ? 150 + Math.max(...(graph.pins || []).filter((p) => p.node_id === node.node_id).map((p) => String(p.name || "").length * 6 + String(p.type || "").length * 5), 0) : 0;
    const w = compact ? Math.max(150, titleW) : Math.max(150, node.kind === "branch" || node.kind === "dispatch" ? 296 : 232, titleW, pinW);
    const execRows = Math.max(execIn, execOut);
    const dataRows = Math.max(inputData, outputData);
    const h = compact ? 40 : Math.max(64, 42 + Math.max(0, execRows) * 24 + Math.max(0, dataRows) * 24 + 16);
    return { w, h };
  }

  function bezierPoint(from, to, t) {
    const controls = bezierControls(from, to);
    const c1 = controls.c1;
    const c2 = controls.c2;
    const mt = 1 - t;
    return {
      x: mt * mt * mt * from.x + 3 * mt * mt * t * c1.x + 3 * mt * t * t * c2.x + t * t * t * to.x,
      y: mt * mt * mt * from.y + 3 * mt * mt * t * c1.y + 3 * mt * t * t * c2.y + t * t * t * to.y
    };
  }

  function bezierControls(from, to) {
    const dx = Math.abs(to.x - from.x);
    const strength = Math.max(32 * view.zoom, Math.min(180 * view.zoom, dx * .48));
    return {
      c1: { x: from.x + strength, y: from.y },
      c2: { x: to.x - strength, y: to.y }
    };
  }

  function drawWireArrow(from, to, color, control) {
    const p = bezierPoint(from, to, .72);
    const q = bezierPoint(from, to, .66);
    const angle = Math.atan2(p.y - q.y, p.x - q.x);
    const len = (control ? 12 : 9) * view.zoom;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.rotate(angle);
    ctx.beginPath();
    ctx.moveTo(len, 0);
    ctx.lineTo(-len * .55, -len * .55);
    ctx.lineTo(-len * .2, 0);
    ctx.lineTo(-len * .55, len * .55);
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.shadowColor = hexToRgba(color, .55);
    ctx.shadowBlur = control ? 10 : 6;
    ctx.fill();
    ctx.restore();
    ctx.shadowBlur = 0;
  }

  function drawWire(wire, from, to, activeWire, selectedWire) {
    const control = wire.wire_kind === "control" || isExecPin(from.pin);
    const color = activeWire ? "#facc15" : wireColor(wire, from);
    const controls = bezierControls(from, to);
    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.bezierCurveTo(controls.c1.x, controls.c1.y, controls.c2.x, controls.c2.y, to.x, to.y);
    ctx.strokeStyle = "rgba(1,6,12,.86)";
    ctx.lineWidth = control ? Math.max(4.5, 5.4 * view.zoom) : Math.max(3.2, 4.2 * view.zoom);
    ctx.shadowBlur = 0;
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.bezierCurveTo(controls.c1.x, controls.c1.y, controls.c2.x, controls.c2.y, to.x, to.y);
    ctx.strokeStyle = color;
    ctx.lineWidth = activeWire ? Math.max(3.2, (control ? 3.2 : 2.5) * view.zoom + .7) : control ? Math.max(2.5, 2.5 * view.zoom) : Math.max(1.8, 1.8 * view.zoom);
    ctx.shadowColor = activeWire ? "rgba(250,204,21,.72)" : hexToRgba(color, control || selectedWire ? .62 : .34);
    ctx.shadowBlur = activeWire ? 18 : control ? 13 : 8;
    ctx.stroke();
    ctx.shadowBlur = 0;
    if (control || selectedWire || activeWire) drawWireArrow(from, to, color, control);
  }

  function drawRerouteKnots(graph) {
    const knots = (editorState.rerouteKnots || []).filter((k) => k.graph_id === graph.graph_id);
    for (const knot of knots) {
      const x = sx(knot.x), y = sy(knot.y);
      ctx.beginPath();
      ctx.arc(x, y, Math.max(5, 7 * view.zoom), 0, Math.PI * 2);
      ctx.fillStyle = "#0f172a";
      ctx.fill();
      ctx.strokeStyle = "#facc15";
      ctx.lineWidth = 2;
      ctx.stroke();
    }
  }

  function drawArchetypeHeader(node, style, x, y, w, headerH) {
    drawArchetypeHeader(node, style, x, y, w, headerH);
  }

  function drawNode(graph, node, inlineByNode, recordHit = true) {
    const size = nodeSize(graph, node);
    const w = size.w * view.zoom, h = size.h * view.zoom;
    const x = sx(nodeX(node)), y = sy(nodeY(node));
    if (view.zoom < .38) {
      roundRect(x, y, Math.max(72, w), Math.max(18, 24 * view.zoom), 4 * view.zoom);
      ctx.fillStyle = selectedNodeIds.has(node.node_id) ? hexToRgba(nodeStyle(node, graph).accent, .28) : "rgba(29,33,41,.88)";
      ctx.fill();
      ctx.strokeStyle = selectedNodeIds.has(node.node_id) ? "#f5a623" : hexToRgba(nodeStyle(node, graph).accent, .48);
      ctx.stroke();
      ctx.fillStyle = "#dbeafe";
      ctx.font = `10px ${UI_FONT}`;
      ctx.fillText(clipText(node.title, 18), x + 8, y + 15);
      if (recordHit) hit.push({ x, y, w: Math.max(72, w), h: Math.max(18, 24 * view.zoom), node });
      return;
    }
    const selected = selectedNodeIds.has(node.node_id);
    const active = debugOverlay && debugOverlay.active_node_id === node.node_id;
    const searchHit = (searchState.spans || []).some((span) => spansOverlap(node.source_span, span));
    const breakpoint = nodeBreakpoint(node);
    const style = nodeStyle(node, graph);
    const headerH = Math.min(26, size.h) * view.zoom;

    if (isGetterCapsule(node)) {
      const out = pinsForNode(graph, node, "output", false)[0] || {};
      const color = colorForType(out.type || "Value");
      ctx.shadowColor = selected ? hexToRgba("#f5a623", .38) : "rgba(0,0,0,.45)";
      ctx.shadowBlur = selected ? 18 : 12;
      ctx.shadowOffsetY = 4;
      roundRect(x, y, w, h, 20 * view.zoom);
      ctx.fillStyle = hexToRgba(color, .12);
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      ctx.strokeStyle = selected ? "#f5a623" : color;
      ctx.lineWidth = selected ? 1.8 : 1;
      ctx.stroke();
      ctx.fillStyle = "#f8fbff";
      ctx.font = `${Math.max(11, 13 * view.zoom)}px ${UI_FONT}`;
      ctx.textAlign = "left";
      ctx.fillText(clipText(node.title, 24), x + 16 * view.zoom, y + 25 * view.zoom);
      if (out.pin_id) drawPin(out, x + w, y + h / 2, "output", recordHit);
      if (detailToggles.types) {
        ctx.fillStyle = color;
        ctx.font = `${Math.max(8, 9.5 * view.zoom)}px ${MONO_FONT}`;
        ctx.fillText(clipText(out.type || "Value", 16), x + 16 * view.zoom, y + 36 * view.zoom);
      }
      if (recordHit) hit.push({ x, y, w, h, node });
      window.__jetCanvasGetterCapsules = true;
      return;
    }

    if (isOperatorNode(node)) {
      const inputs = pinsForNode(graph, node, "input", false);
      const outputs = pinsForNode(graph, node, "output", false);
      ctx.shadowColor = selected ? "rgba(245,166,35,.34)" : "rgba(0,0,0,.42)";
      ctx.shadowBlur = selected ? 18 : 10;
      ctx.shadowOffsetY = 4;
      roundRect(x, y, w, h, 8 * view.zoom);
      ctx.fillStyle = "rgba(29,33,41,.96)";
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      ctx.strokeStyle = selected ? "#f5a623" : hexToRgba(style.accent, .55);
      ctx.lineWidth = selected ? 1.8 : 1;
      ctx.stroke();
      ctx.fillStyle = "#f2f4f8";
      ctx.font = `${Math.max(19, 26 * view.zoom)}px ${UI_FONT}`;
      ctx.textAlign = "center";
      ctx.fillText(clipText(node.title, 2), x + w / 2, y + h / 2 + 9 * view.zoom);
      ctx.textAlign = "left";
      inputs.forEach((p, i) => drawSocketRow(p, x, y + (18 + i * 22) * view.zoom, w, "input", recordHit));
      outputs.forEach((p, i) => drawSocketRow(p, x, y + (18 + i * 22) * view.zoom, w, "output", recordHit));
      if (recordHit) hit.push({ x, y, w, h, node });
      return;
    }

    ctx.shadowColor = active ? "rgba(250,204,21,.50)" : selected ? "rgba(245,166,35,.34)" : searchHit ? "rgba(192,132,252,.35)" : "rgba(0,0,0,.45)";
    ctx.shadowBlur = active ? 28 : selected ? 22 : searchHit ? 18 : 12;
    ctx.shadowOffsetY = 4;
    roundRect(x, y, w, h, 8 * view.zoom);
    ctx.fillStyle = "rgba(29,33,41,.96)";
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.strokeStyle = active ? "#facc15" : selected ? "#f5a623" : searchHit ? "#c084fc" : "#0b0d11";
    ctx.lineWidth = active ? 2.2 : selected ? 1.5 : 1;
    ctx.stroke();
    if (node.kind === "binding" || node.kind === "assign") {
      ctx.fillStyle = style.accent;
      ctx.fillRect(x, y + 7 * view.zoom, Math.max(3, 3 * view.zoom), h - 14 * view.zoom);
      window.__jetCanvasBindingTypeAccent = true;
    }
    if (selected) {
      ctx.save();
      roundRect(x - 2 * view.zoom, y - 2 * view.zoom, w + 4 * view.zoom, h + 4 * view.zoom, 9 * view.zoom);
      ctx.strokeStyle = "rgba(245,166,35,.62)";
      ctx.shadowColor = "rgba(245,166,35,.32)";
      ctx.shadowBlur = 10 * view.zoom;
      ctx.lineWidth = 1.5 * view.zoom;
      ctx.stroke();
      ctx.restore();
    }

    roundRect(x, y, w, headerH, 8 * view.zoom);
    const headerGrad = ctx.createLinearGradient(x, y, x + w, y);
    headerGrad.addColorStop(0, style.header || style.accent);
    headerGrad.addColorStop(1, style.header2 || "#151a21");
    ctx.fillStyle = headerGrad;
    ctx.fill();
    ctx.fillStyle = "rgba(255,255,255,.86)";
    ctx.font = `${Math.max(10, 14 * view.zoom)}px ${UI_FONT}`;
    ctx.textAlign = "center";
    ctx.fillText(style.glyph || "ƒ", x + 16 * view.zoom, y + 18 * view.zoom);
    ctx.textAlign = "left";
    ctx.fillStyle = "#f8fbff";
    ctx.font = `600 ${Math.max(10, 13 * view.zoom)}px ${UI_FONT}`;
    ctx.fillText(clipText(node.title, 24), x + 30 * view.zoom, y + 17.5 * view.zoom);
    const subtitle = nodeSubtitle(node);
    if (subtitle && node.archetype !== "entry") {
      ctx.fillStyle = "rgba(255,255,255,.65)";
      ctx.font = `${Math.max(8, 10 * view.zoom)}px ${UI_FONT}`;
      ctx.fillText(clipText(subtitle, 28), x + 30 * view.zoom, y + 29.5 * view.zoom);
    }

    if (shouldDrawNodeBadge(node)) {
      const badgeText = String(node.kind || "").toUpperCase();
      ctx.font = `${Math.max(7.5, 9.2 * view.zoom)}px ${MONO_FONT}`;
      const badgeW = Math.min(118 * view.zoom, ctx.measureText(badgeText).width + 14 * view.zoom);
      const badgeY = y + headerH + 6 * view.zoom;
      roundRect(x + 14 * view.zoom, badgeY, badgeW, 20 * view.zoom, 4 * view.zoom);
      ctx.fillStyle = hexToRgba(style.accent, .14);
      ctx.fill();
      ctx.strokeStyle = hexToRgba(style.accent, .42);
      ctx.lineWidth = Math.max(.8, view.zoom);
      ctx.stroke();
      ctx.fillStyle = style.accent;
      ctx.textAlign = "center";
      ctx.fillText(clipText(badgeText, 16), x + 14 * view.zoom + badgeW / 2, badgeY + 13.5 * view.zoom);
      ctx.textAlign = "left";
    }

    if (breakpoint) {
      ctx.beginPath();
      ctx.arc(x + w - 17 * view.zoom, y + 17 * view.zoom, 6 * view.zoom, 0, Math.PI * 2);
      ctx.fillStyle = "#ef4444";
      ctx.fill();
    }

    const execIn = pinsForNode(graph, node, "input", true);
    const execOut = pinsForNode(graph, node, "output", true);
    const execTop = 42;
    execIn.forEach((p, i) => drawSocketRow(p, x, y + (execTop + i * 28) * view.zoom, w, "input", recordHit));
    execOut.forEach((p, i) => drawSocketRow(p, x, y + (execTop + i * 28) * view.zoom, w, "output", recordHit));

    const inputs = pinsForNode(graph, node, "input", false);
    const outputs = pinsForNode(graph, node, "output", false);
    const execRows = Math.max(execIn.length, execOut.length);
    const dataTop = Math.max(44, execTop + execRows * 24 + (execRows ? 10 : 0));
    inputs.forEach((p, i) => {
      const rowY = y + (dataTop + i * 24) * view.zoom;
      drawSocketRow(p, x, rowY, w, "input", recordHit);
      const editorX = x + Math.min(w - 108 * view.zoom, 74 * view.zoom);
      drawPinDefaultEditor(graph, p, editorX, rowY, recordHit);
    });
    outputs.forEach((p, i) => drawSocketRow(p, x, y + (dataTop + i * 24) * view.zoom, w, "output", recordHit));

    const inline = (selected || view.zoom >= .95 ? (inlineByNode.get(node.node_id) || []) : []).slice(0, 2);
    inline.forEach((expr, i) => {
      const cy = y + (dataTop + Math.max(inputs.length, outputs.length) * 24 + 12 + i * 22) * view.zoom;
      roundRect(x + 12 * view.zoom, cy - 13 * view.zoom, w - 24 * view.zoom, 18 * view.zoom, 5 * view.zoom);
      ctx.fillStyle = simpleEmbeddedValue(expr) ? "rgba(212,212,216,.16)" : "rgba(246,211,101,.11)";
      ctx.fill();
      ctx.strokeStyle = simpleEmbeddedValue(expr) ? "rgba(212,212,216,.38)" : "rgba(246,211,101,.24)";
      ctx.lineWidth = Math.max(.8, view.zoom);
      ctx.stroke();
      ctx.fillStyle = simpleEmbeddedValue(expr) ? "#e4e4e7" : "#f6d365";
      ctx.font = `${Math.max(9, 11 * view.zoom)}px ${MONO_FONT}`;
      ctx.fillText(clipText(expr.source, 34), x + 19 * view.zoom, cy);
      if (simpleEmbeddedValue(expr)) window.__jetCanvasEmbeddedVariables = true;
    });

    if (recordHit) hit.push({ x, y, w, h, node });
  }

  function visibleGraphNodes(graph) {
    const size = cssSize();
    const margin = 360;
    if (!graph || graph.nodes.length < 180) {
      window.__jetCanvasVirtualizationStats = { total: graph ? graph.nodes.length : 0, visible: graph ? graph.nodes.length : 0, lod: view.zoom < .38 };
      return graph ? graph.nodes : [];
    }
    const visible = graph.nodes.filter((node) => {
      const ns = nodeSize(graph, node);
      const x = sx(nodeX(node)), y = sy(nodeY(node));
      return x + ns.w * view.zoom > -margin && y + ns.h * view.zoom > -margin && x < size.width + margin && y < size.height + margin;
    });
    window.__jetCanvasVirtualizationStats = { total: graph.nodes.length, visible: visible.length, lod: view.zoom < .38 };
    return visible;
  }

  function drawGraph(doc) {
    latestDoc = doc;
    loadDebugState(doc);
    const graph = currentGraph(doc);
    if (!graph) return;
    selectedGraphId = graph.graph_id;
    window.__jetCanvasSelectedGraphId = selectedGraphId;
    if (!selectedNodeId || !graph.nodes.some((n) => n.node_id === selectedNodeId)) selectedNodeId = graph.entry_node;
    if (selectedNodeIds.size === 0 && selectedNodeId) selectedNodeIds.add(selectedNodeId);
    selectedNodeIds = new Set([...selectedNodeIds].filter((id) => graph.nodes.some((n) => n.node_id === id)));
    syncGraphPicker(doc);
    syncGraphList(doc);
    syncGraphStrip(doc);
    fit();
    const size = cssSize();
    drawGrid(size);
    hit = [];
    pinPoints = new Map();
    pinHit = [];
    pinEditorHit = [];
    graphSelect.value = selectedGraphId;
    const pins = new Map(graph.pins.map((p) => [p.pin_id, p]));
    connectedPinIds = new Set((graph.wires || []).flatMap((wire) => [wire.from_pin, wire.to_pin]));
    const nodes = new Map(graph.nodes.map((n) => [n.node_id, n]));
    const inlineByNode = new Map();
    for (const expr of graph.inline_exprs || []) {
      if (!inlineByNode.has(expr.node_id)) inlineByNode.set(expr.node_id, []);
      inlineByNode.get(expr.node_id).push(expr);
    }
    reflowGraph(graph);

    drawCommentRegions(graph);
    const visibleNodes = visibleGraphNodes(graph);
    const visibleIds = new Set(visibleNodes.map((node) => node.node_id));

    for (const node of visibleNodes) {
      drawNode(graph, node, inlineByNode);
    }

    for (const wire of graph.wires) {
      const from = pinPoints.get(wire.from_pin);
      const to = pinPoints.get(wire.to_pin);
      if (!from || !to) continue;
      if (!visibleIds.has(from.pin.node_id) && !visibleIds.has(to.pin.node_id)) continue;
      const activeWire = debugOverlay && debugOverlay.active_wire_id === wire.wire_id;
      const selectedWire = selectedNodeIds.has(from.pin.node_id) || selectedNodeIds.has(to.pin.node_id);
      drawWire(wire, from, to, activeWire, selectedWire);
      if (detailToggles.types && (activeWire || selectedWire || view.zoom >= 1.05)) drawWireTypeBadge(wire, from, to);
    }

    drawRerouteKnots(graph);

    for (const node of visibleNodes) {
      drawNode(graph, node, inlineByNode, false);
    }

    if (drag && drag.mode === "pin") {
      drawCompatibleDropTargets(graph, drag.pin);
      const from = pinPoints.get(drag.pin.pin_id);
      if (from) {
        const controls = bezierControls(from, { x: drag.mx, y: drag.my });
        const plan = connectionPlan(graph, drag.pin, hoverPin);
        syncWireStatus({ title: plan.ok ? "Wire preview" : "Wire refused", detail: plan.label, color: plan.color });
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.bezierCurveTo(controls.c1.x, controls.c1.y, controls.c2.x, controls.c2.y, drag.mx, drag.my);
        ctx.strokeStyle = plan.color;
        ctx.lineWidth = Math.max(2.5, 4 * view.zoom);
        ctx.setLineDash(plan.ok ? [12, 6] : [4, 6]);
        ctx.stroke();
        ctx.setLineDash([]);
        drawConnectionBadge(plan, drag.mx, drag.my);
      }
    }

    if (pendingPin && (!drag || drag.mode !== "pin")) {
      syncWireStatus({ title: "Destination needed", detail: pinName(pendingPin) + " : " + exactPinType(pendingPin), color: colorForType(pendingPin.type || "Value") });
      drawCompatibleDropTargets(graph, pendingPin);
      const from = pinPoints.get(pendingPin.pin_id);
      if (from) {
        ctx.beginPath();
        ctx.arc(from.x, from.y, Math.max(17, 22 * view.zoom), 0, Math.PI * 2);
        ctx.strokeStyle = "#7dd3fc";
        ctx.lineWidth = Math.max(1.5, 2.4 * view.zoom);
        ctx.setLineDash([8, 5]);
        ctx.stroke();
        ctx.setLineDash([]);
        drawConnectionBadge({ ok: true, label: "Select destination socket", color: "#7dd3fc" }, from.x + 36 * view.zoom, from.y - 12 * view.zoom);
      }
    }

    if (drag && drag.mode === "marquee") {
      const x = Math.min(drag.x, drag.mx), y = Math.min(drag.y, drag.my);
      const w = Math.abs(drag.mx - drag.x), h = Math.abs(drag.my - drag.y);
      ctx.setLineDash([6, 5]);
      ctx.strokeStyle = "#67e8f9";
      ctx.lineWidth = 1.5;
      ctx.strokeRect(x, y, w, h);
      ctx.fillStyle = "rgba(103,232,249,.08)";
      ctx.fillRect(x, y, w, h);
      ctx.setLineDash([]);
    }

    if (hoverPin && (!drag || drag.mode !== "pin")) drawPinHoverTooltip(hoverPin);
    drawMinimap(graph);
    drawTypeLegend(size);
    if (!pendingPin && (!drag || drag.mode !== "pin")) syncWireStatus(null);
    const selectedNode = nodes.get(selectedNodeId);
    updateDetails(graph, selectedNode, graph.pins.filter((p) => p.node_id === selectedNodeId), inlineByNode.get(selectedNodeId) || []);
    syncGraphOverview(graph, selectedNode);
    window.__jetCanvasNonblankPixels = graph.nodes.length > 0 ? 1 : 0;
    window.__jetCanvasPendingPin = pendingPin ? { pin_id: pendingPin.pin_id, name: pendingPin.name, type: pendingPin.type, direction: pendingPin.direction } : null;
    const hitMap = {
      graph_id: graph.graph_id,
      nodes: hit.map((h) => ({ node_id: h.node.node_id, title: h.node.title, kind: h.node.kind, x: h.x, y: h.y, w: h.w, h: h.h })),
      pins: pinHit.map((h) => ({ pin_id: h.pin.pin_id, node_id: h.pin.node_id, name: h.pin.name, type: h.pin.type, direction: h.pin.direction, x: h.x, y: h.y, w: h.w, h: h.h, cx: h.cx, cy: h.cy }))
    };
    window.__jetCanvasHitMap = hitMap;
    canvas.dataset.hitMap = JSON.stringify(hitMap);
    updateGraphNav(graph);
    const rails = (graph.rails && graph.rails.kinds ? graph.rails.kinds.join(", ") : "data");
    graphMeta.textContent = graph.nodes.length + " nodes / " + graph.wires.length + " wires / " + rails;
    zoomLabel.textContent = Math.round(view.zoom * 100) + "%";
    sourceId.textContent = doc.source_id || "source";
    revision.textContent = (doc.revision || "").slice(0, 18);
  }

  graphSelect.addEventListener("change", function () {
    switchGraph(graphSelect.value);
  });

  function drawMinimap(graph) {
    mini.clearRect(0, 0, minimap.width, minimap.height);
    mini.fillStyle = "#07101c";
    mini.fillRect(0, 0, minimap.width, minimap.height);
    const b = graphBounds(graph);
    const scale = Math.min((minimap.width - 20) / Math.max(1, b.maxX - b.minX), (minimap.height - 20) / Math.max(1, b.maxY - b.minY));
    for (const n of graph.nodes) {
      const size = nodeSize(graph, n);
      const style = nodeStyle(n, graph);
      mini.fillStyle = n.node_id === selectedNodeId ? "#f5a623" : style.accent;
      mini.fillRect(10 + (nodeX(n) - b.minX) * scale, 10 + (nodeY(n) - b.minY) * scale, Math.max(16, size.w * scale), Math.max(9, size.h * scale));
    }
  }

  function nodesInRegion(graph, region) {
    return (graph.nodes || []).filter((node) => spansOverlap(node.source_span, region.source_span));
  }

  function commentRegionBounds(graph, region) {
    const b = region.bounds || {};
    if (b.w > 0 && b.h > 0) return { x: b.x || 0, y: b.y || 0, w: b.w, h: b.h };
    const nodes = nodesInRegion(graph, region);
    if (nodes.length === 0) return { x: 120, y: 120, w: 360, h: 180 };
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const node of nodes) {
      const size = nodeSize(graph, node);
      minX = Math.min(minX, nodeX(node));
      minY = Math.min(minY, nodeY(node));
      maxX = Math.max(maxX, nodeX(node) + size.w);
      maxY = Math.max(maxY, nodeY(node) + size.h);
    }
    return { x: minX - 26, y: minY - 36, w: maxX - minX + 52, h: maxY - minY + 70 };
  }

  function drawCommentRegions(graph) {
    for (const region of (graph.regions || []).filter((r) => r.kind === "comment")) {
      const b = commentRegionBounds(graph, region);
      const x = sx(b.x), y = sy(b.y), w = b.w * view.zoom, h = b.h * view.zoom;
      roundRect(x, y, w, h, 7);
      ctx.fillStyle = hexToRgba(region.color, region.alpha);
      ctx.fill();
      ctx.strokeStyle = region.color || "#2563eb";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([8, 5]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = "#eaf3ff";
      ctx.font = `${Math.max(11, 14 * view.zoom)}px "Segoe UI", system-ui, sans-serif`;
      ctx.fillText(region.title || "Comment", x + 12 * view.zoom, y + 23 * view.zoom);
    }
  }

  function regionsForNode(graph, node) {
    if (!node) return [];
    return (graph.regions || []).filter((region) => region.kind === "comment" && spansOverlap(region.source_span, node.source_span));
  }

  function debugRows(items) {
    return (items || []).map((item) => `<div class="pin-row"><b>${escapeHtml(item.name || "frame")}</b><br><span class="tag">${escapeHtml(item.value || String(item))}</span></div>`).join("");
  }

  function signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride) {
    const retType = retOverride !== undefined ? retOverride : (document.getElementById("function-return-type") && document.getElementById("function-return-type").value.trim()) || "Void";
    const ret = retType && retType !== "Void" ? " -> " + retType : "";
    const rows = [...details.querySelectorAll("[data-fn-param]")];
    const params = rows.map((row) => {
      const name = (row.querySelector("[data-param-name]") || {}).value || "";
      const type = (row.querySelector("[data-param-type]") || {}).value || "Int";
      const fallback = (row.querySelector("[data-param-default]") || {}).value || "";
      const defaultExpr = fallback.trim() ? " = " + fallback.trim() : "";
      return name.trim() + ": " + type.trim() + defaultExpr;
    }).filter((p) => !p.startsWith(":"));
    const visibility = fnMeta && fnMeta.visibility === "public" ? "pub " : "";
    return visibility + "fn " + ((fnMeta && fnMeta.name) || nodeTitle || "function") + "(" + params.join(", ") + ")" + ret;
  }

  function applyFunctionPins(graph, fnMeta, nodeTitle, retOverride) {
    const signature = signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride);
    window.__jetCanvasLastSignature = signature;
    postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
    return signature;
  }

  function nextParamName(fnMeta) {
    const used = new Set((fnMeta && fnMeta.params || []).map((p) => p.name || ""));
    let i = 1;
    while (used.has("input" + i)) i += 1;
    return "input" + i;
  }

  function pinPortHtml(type) {
    const t = type || "Value";
    const cls = t === "exec" || t === "control" ? " is-exec" : String(t).endsWith("?") ? " is-fallible" : "";
    return `<span class="pin-port${cls}" style="color:${escapeAttr(colorForType(t))}"></span>`;
  }

  function typeChipHtml(type) {
    return `<span class="type-chip" style="color:${escapeAttr(colorForType(type))}">${escapeHtml(type || "Value")}</span>`;
  }

  function functionParamRow(p, i) {
    const pinType = p && p.type ? p.type : "Int";
    const pinName = p && p.name ? p.name : "value";
    const defaultSource = p && p.default_source ? p.default_source : "";
    const defaultLabel = defaultSource ? "default " + defaultSource : "required";
    return `<div class="pin-editor-row" data-fn-param="${escapeAttr(i)}"><div class="pin-editor-title">${pinPortHtml(pinType)}<b>${escapeHtml(pinName)}</b>${typeChipHtml(pinType)}</div><div class="lane-meta">${escapeHtml(defaultLabel)}</div><div class="pin-tools"><input data-param-name="${escapeAttr(i)}" aria-label="Input pin name" title="Input pin name" value="${escapeAttr(pinName)}"><input data-param-type="${escapeAttr(i)}" aria-label="Input pin type" title="Input pin type" value="${escapeAttr(pinType)}"><input data-param-default="${escapeAttr(i)}" aria-label="Default expression" title="Default expression" placeholder="default" value="${escapeAttr(defaultSource)}"><button data-param-remove="${escapeAttr(i)}" title="Remove input pin">-</button></div></div>`;
  }

  function functionReturnRow(retType) {
    const outType = retType || "Void";
    const color = colorForType(outType);
    return `<div class="pin-editor-row"><div class="pin-editor-title"><span id="function-return-port" class="pin-port" style="color:${escapeAttr(color)}"></span><b>return</b><span id="function-return-type-chip" class="type-chip" style="color:${escapeAttr(color)}">${escapeHtml(outType)}</span></div><div id="function-return-meta" class="lane-meta">${outType === "Void" ? "no output value" : "output pin"}</div><div class="pin-tools output-pin-tools"><input id="function-return-type" aria-label="Return type" title="Return type" value="${escapeAttr(outType)}"><button id="set-function-output">Set</button><button id="remove-function-output">Void</button></div></div>`;
  }

  function syncReturnEditorPreview(type) {
    const outType = type && type.trim() ? type.trim() : "Void";
    const color = colorForType(outType);
    const port = document.getElementById("function-return-port");
    const chip = document.getElementById("function-return-type-chip");
    const meta = document.getElementById("function-return-meta");
    if (port) port.style.color = color;
    if (chip) {
      chip.style.color = color;
      chip.textContent = outType;
    }
    if (meta) meta.textContent = outType === "Void" ? "no output value" : "output pin";
  }

  function pinCardHtml(p) {
    const type = p.type || "Value";
    const color = colorForType(type);
    const rail = pinRail(p);
    const flags = [p.direction, rail, p.fallible ? "fallible" : "", p.effect_grant_need ? "effect" : ""].filter(Boolean).join(" / ");
    return `<div class="pin-card" style="--pin-color:${escapeAttr(color)}">${pinPortHtml(isExecPin(p) ? "exec" : type)}<div class="pin-card-title"><b>${escapeHtml(p.name)}</b><small>${escapeHtml(flags)}<span class="type-detail"> - ${escapeHtml(type)}</span></small></div><button data-pin-menu="${escapeAttr(p.pin_id)}">Actions</button></div>`;
  }

  function updateDetails(graph, node, pins, inline) {
    if (!node) {
      details.innerHTML = "<div class=\"details-empty\"><b>No node selected</b><span class=\"tag\">Select, marquee, or use the graph tabs.</span></div>";
      return;
    }
    const style = nodeStyle(node, graph);
    details.style.setProperty("--node-accent", style.accent);
    const span = node.source_span || { start: 0, end: 0 };
    const pinRows = pins.map(pinCardHtml).join("");
    const inlineRows = inline.map((expr) => `<div class="inline-row"><b>${escapeHtml(expr.role)}</b><code>${escapeHtml(expr.source)}</code><div class="edit-grid"><input data-inline-id="${escapeHtml(expr.inline_expr_id)}" value="${escapeAttr(expr.source)}"><button data-inline-apply="${escapeHtml(expr.inline_expr_id)}">Apply expression</button><button data-inline-promote="${escapeHtml(expr.inline_expr_id)}">Promote to binding</button><button data-inline-convert="${escapeHtml(expr.inline_expr_id)}">Insert conversion</button><button data-inline-preview-extract="${escapeHtml(expr.inline_expr_id)}">Preview extract</button><button data-inline-extract="${escapeHtml(expr.inline_expr_id)}">Extract function</button></div></div>`).join("");
    const rename = node.kind === "binding" ? `<div class="edit-grid"><label>Rename binding<input id="rename-to" value="${escapeAttr(node.title)}"></label><button id="preview-rename">Preview rename</button><button id="rename-binding" class="primary">Rename</button></div>` : "";
    const calleeGraph = (node.kind === "call" || node.kind === "method") ? graphForFunctionName(node.title) : null;
    const calleeOpen = calleeGraph ? `<button id="open-callee-graph">Open ${escapeHtml(calleeGraph.title)} graph</button>` : "";
    const fnMeta = node.node_id === graph.entry_node ? graph.function : null;
    const fnReturnType = fnMeta ? (typeof fnMeta.returns === "string" ? fnMeta.returns : (fnMeta.returns && fnMeta.returns.type) || "Void") : "Void";
    const fnParams = fnMeta ? (fnMeta.params || []).map((p, i) => functionParamRow(p, i)).join("") : "";
    const fnReturnPanel = fnMeta ? functionReturnRow(fnReturnType) : "";
    const fnEvents = fnMeta ? (graph.event_views || []).map((event) => `<div class="pin-row"><b>${escapeHtml(event.title || event.function)}</b><br><span class="tag">${escapeHtml(event.semantics || "ordinary_jet_function")}</span></div>`).join("") : "";
    const fnPanel = fnMeta ? `<h2>Function</h2><div class="signature-board"><div class="signature-head"><div><span class="sig-eyebrow">function graph</span><b>${escapeHtml(fnMeta.visibility || "private")} ${escapeHtml(fnMeta.name || node.title)}</b><code>${escapeHtml(fnMeta.signature || "")}</code></div><button id="create-function" title="Create sibling function">New</button></div><div class="pin-lane"><div class="lane-head"><b>Inputs</b><span class="lane-meta">${(fnMeta.params || []).length} pins</span><button id="add-function-pin">+ Input</button></div><div class="pin-list" id="function-pin-list">${fnParams || "<div class=\"pin-empty\">No input pins</div>"}</div></div><div class="pin-lane"><div class="lane-head"><b>Output</b><span class="lane-meta">return type</span><button id="add-function-output">+ Output</button></div><div class="pin-list">${fnReturnPanel}</div></div><div class="signature-source"><span class="sig-eyebrow">source signature</span><code>${escapeHtml(fnMeta.signature || "")}</code><input id="function-signature" value="${escapeAttr(fnMeta.signature || "")}"><div class="rename-strip"><input id="function-rename-to" aria-label="Function name" title="Function name" value="${escapeAttr(fnMeta.name || node.title)}"><button id="rename-function">Rename</button></div></div><div class="signature-actions"><button id="edit-function-signature">Apply signature</button><button id="apply-function-pins" class="primary">Apply pins</button></div></div>${fnEvents ? `<h2>Callback views</h2><div class="pin-list">${fnEvents}</div>` : ""}` : "";
    const bpLabel = nodeBreakpoint(node) ? "Remove breakpoint" : "Set breakpoint";
    const locals = debugRows(debugOverlay && debugOverlay.locals);
    const watches = debugRows(debugOverlay && debugOverlay.watches);
    const stack = (debugOverlay && debugOverlay.call_stack || []).map((frame) => `<div class="pin-row"><span class="tag">${escapeHtml(frame)}</span></div>`).join("");
    const regionRows = regionsForNode(graph, node).map((region) => {
      const b = region.bounds || { x: 0, y: 0, w: 360, h: 180 };
      const bounds = [b.x || 0, b.y || 0, b.w || 360, b.h || 180].join(",");
      return `<div class="inline-row"><b>${escapeHtml(region.title || "Comment")}</b><code>${escapeHtml(region.region_id)}</code><div class="edit-grid"><input data-region-title="${escapeHtml(region.region_id)}" value="${escapeAttr(region.title || "Comment")}"><input data-region-color="${escapeHtml(region.region_id)}" value="${escapeAttr(region.color || "#2563eb")}"><input data-region-alpha="${escapeHtml(region.region_id)}" value="${escapeAttr(region.alpha || "0.18")}"><input data-region-bounds="${escapeHtml(region.region_id)}" value="${escapeAttr(bounds)}"><button data-region-apply="${escapeHtml(region.region_id)}">Apply comment</button><button data-region-delete="${escapeHtml(region.region_id)}">Delete comment</button></div></div>`;
    }).join("");
    const affords = (node.edit_affordances || []).slice(0, 4).map((a) => `<span class="details-chip">${escapeHtml(a)}</span>`).join("");
    details.innerHTML = `
      <div class="details-hero">
        <div class="details-titleline"><span class="node-glyph">${escapeHtml(style.glyph)}</span><div class="details-title"><p class="title">${escapeHtml(node.title)}</p><div class="kind">${escapeHtml(nodeKindLabel(node, graph))}</div></div></div>
        <div class="details-chips dev-only"><span class="details-chip">${escapeHtml(node.kind)}</span><span class="details-chip type-detail">${pins.length} pins</span>${affords}</div>
        <div class="quick-actions"><button id="source-jump">Jump source</button><button id="find-references">Find refs</button>${calleeOpen ? calleeOpen.replace("<button", "<button class=\"wide\"") : ""}</div>
        <dl class="dev-only">
          <dt>span</dt><dd>${span.start}..${span.end}</dd>
          <dt>node</dt><dd>${escapeHtml(node.node_id)}</dd>
        </dl>
      </div>
      ${rename}
      ${fnPanel}
      <div class="debug-detail">
        <h2>Debug</h2>
        <div class="edit-grid"><button id="debug-toggle-break">${bpLabel}</button><button id="debug-add-watch">Add watch</button></div>
        <div class="pin-list">${locals || watches || stack ? locals + watches + stack : "<div class=\"tag\">no live values</div>"}</div>
      </div>
      <div class="diagnostic-detail">
        <h2>Comments</h2><div class="inline-list">${regionRows || "<div class=\"tag\">none</div>"}</div>
      </div>
      <div class="type-detail">
        <h2>Pins</h2><div class="pin-list">${pinRows || "<div class=\"tag\">none</div>"}</div>
        <h2>Inline</h2><div class="inline-list">${inlineRows || "<div class=\"tag\">none</div>"}</div>
      </div>
    `;
    const renameButton = document.getElementById("rename-binding");
    if (renameButton) {
      renameButton.addEventListener("click", () => {
        const to = document.getElementById("rename-to").value.trim();
        postTransaction({ schema_version: 1, op: "rename_binding", revision: latestDoc.revision, from: node.title, to });
      });
    }
    const previewRename = document.getElementById("preview-rename");
    if (previewRename) {
      previewRename.addEventListener("click", () => {
        const to = document.getElementById("rename-to").value.trim();
        postQuery({ op: "preview_rename", symbol: node.title, to });
      });
    }
    const renameFunction = document.getElementById("rename-function");
    if (renameFunction && fnMeta) {
      renameFunction.addEventListener("click", () => {
        const to = document.getElementById("function-rename-to").value.trim();
        postTransaction({ schema_version: 1, op: "rename_function", revision: latestDoc.revision, from: fnMeta.name, to });
      });
    }
    const editFunctionSignature = document.getElementById("edit-function-signature");
    if (editFunctionSignature && fnMeta) {
      editFunctionSignature.addEventListener("click", () => {
        const signature = document.getElementById("function-signature").value.trim();
        postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
      });
    }
    const createFunction = document.getElementById("create-function");
    if (createFunction) {
      createFunction.addEventListener("click", () => {
        const name = window.prompt("Function name", "helper");
        if (!name) return;
        const params = window.prompt("Parameters", "value: Int") || "";
        const ret_type = window.prompt("Return type", "Int") || "Int";
        postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params, ret_type });
      });
    }
    const openCalleeGraph = document.getElementById("open-callee-graph");
    if (openCalleeGraph && calleeGraph) {
      openCalleeGraph.addEventListener("click", () => openFunctionGraph(calleeGraph.title));
    }
    const addFunctionPin = document.getElementById("add-function-pin");
    if (addFunctionPin && fnMeta) {
      addFunctionPin.addEventListener("click", () => {
        const list = document.getElementById("function-pin-list");
        const i = "new" + Date.now();
        const row = document.createElement("div");
        row.innerHTML = functionParamRow({ name: nextParamName(fnMeta), type: "Int", default_source: "" }, i);
        const editorRow = row.firstElementChild;
        if (editorRow) {
          const empty = list.querySelector(".pin-empty");
          if (empty) empty.remove();
          list.appendChild(editorRow);
          const remove = editorRow.querySelector("[data-param-remove]");
          if (remove) remove.addEventListener("click", () => {
            editorRow.remove();
            applyFunctionPins(graph, fnMeta, node.title);
          });
          applyFunctionPins(graph, fnMeta, node.title);
        }
      });
    }
    ["apply-function-pins", "set-function-output", "remove-function-output", "add-function-output"].forEach((id) => {
      const button = document.getElementById(id);
      if (button) button.addEventListener("click", handleFunctionPinButton);
    });
    details.querySelectorAll("[data-param-remove]").forEach((button) => {
      button.addEventListener("click", () => {
        const row = button.closest("[data-fn-param]");
        if (row) row.remove();
        if (fnMeta) applyFunctionPins(graph, fnMeta, node.title);
      });
    });
    details.querySelectorAll("[data-pin-menu]").forEach((button) => {
      button.addEventListener("click", (ev) => {
        const pin = pins.find((p) => p.pin_id === button.getAttribute("data-pin-menu"));
        if (pin) openPinMenu(pin, ev.clientX, ev.clientY);
      });
    });
    const sourceJump = document.getElementById("source-jump");
    if (sourceJump) {
      sourceJump.addEventListener("click", () => {
        setSourceHash(span);
        showToast("Source span copied to URL");
      });
    }
    const findReferences = document.getElementById("find-references");
    if (findReferences) {
      findReferences.addEventListener("click", () => postQuery({ op: "references", symbol: node.title }));
    }
    details.querySelectorAll("[data-inline-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-apply");
        const input = details.querySelector(`[data-inline-id="${cssEscape(id)}"]`);
        postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: id, new_expr: input.value });
      });
    });
    details.querySelectorAll("[data-inline-promote]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-promote");
        const name = window.prompt("Binding name", "value");
        if (name) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: id, name });
      });
    });
    details.querySelectorAll("[data-inline-convert]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-convert");
        const callee = window.prompt("Conversion function", "Float.from");
        if (callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: id, callee });
      });
    });
    details.querySelectorAll("[data-inline-preview-extract], [data-inline-extract]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-preview-extract") || button.getAttribute("data-inline-extract");
        const name = window.prompt("Helper function", "extracted");
        if (!name) return;
        const op = button.hasAttribute("data-inline-preview-extract") ? "preview_extract_inline_expr" : "extract_inline_expr";
        postTransaction({ schema_version: 1, op, revision: latestDoc.revision, inline_expr_id: id, function: name, ret_type: "Int" });
      });
    });
    const toggle = document.getElementById("debug-toggle-break");
    if (toggle) toggle.addEventListener("click", () => toggleBreakpoint(node));
    const watch = document.getElementById("debug-add-watch");
    if (watch) watch.addEventListener("click", () => {
      const name = window.prompt("Watch local", node.title);
      addWatch(name && name.trim());
    });
    details.querySelectorAll("[data-region-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-region-apply");
        postTransaction({
          schema_version: 1,
          op: "edit_comment_region",
          revision: latestDoc.revision,
          region_id: id,
          title: details.querySelector(`[data-region-title="${cssEscape(id)}"]`).value,
          color: details.querySelector(`[data-region-color="${cssEscape(id)}"]`).value,
          alpha: details.querySelector(`[data-region-alpha="${cssEscape(id)}"]`).value,
          bounds: details.querySelector(`[data-region-bounds="${cssEscape(id)}"]`).value
        });
      });
    });
    details.querySelectorAll("[data-region-delete]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-region-delete");
        postTransaction({ schema_version: 1, op: "delete_comment_region", revision: latestDoc.revision, region_id: id });
      });
    });
  }

  function selectNode(node, mode) {
    if (mode === "toggle") {
      if (selectedNodeIds.has(node.node_id)) selectedNodeIds.delete(node.node_id);
      else selectedNodeIds.add(node.node_id);
    } else if (mode === "add") {
      selectedNodeIds.add(node.node_id);
    } else {
      selectedNodeIds = new Set([node.node_id]);
    }
    selectedNodeId = node.node_id;
    const s = node.source_span || { start: 0, end: 0 };
    setSourceHash(s);
    if (latestDoc) drawGraph(latestDoc);
  }

  function hitNodeAt(x, y) {
    for (let i = hit.length - 1; i >= 0; i--) {
      const h = hit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function hitPinAt(x, y) {
    let best = null;
    let bestDistance = Infinity;
    for (let i = pinHit.length - 1; i >= 0; i--) {
      const h = pinHit[i];
      if (x < h.x || x > h.x + h.w || y < h.y || y > h.y + h.h) continue;
      const dx = x - (h.cx || h.x + h.w / 2);
      const dy = y - (h.cy || h.y + h.h / 2);
      const distance = dx * dx + dy * dy;
      if (distance < bestDistance) {
        best = h.pin;
        bestDistance = distance;
      }
    }
    return best;
  }

  function hitPinDefaultEditorAt(x, y) {
    for (let i = pinEditorHit.length - 1; i >= 0; i--) {
      const h = pinEditorHit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function applyPinDefaultEditor(hit) {
    if (!hit || !hit.pin) return false;
    if (!hit.expr) {
      showToast("Default value is read-only");
      return true;
    }
    let next = defaultEditorValue(hit.expr);
    if (hit.kind === "inline_expr") {
      next = window.prompt("Expression", next);
    } else if (hit.kind === "bool") {
      next = next === "true" ? "false" : "true";
    } else if (hit.kind === "number") {
      next = window.prompt("Default " + hit.pin.name, next || "0");
    } else if (hit.kind === "string") {
      const raw = window.prompt("Default " + hit.pin.name, next.replace(/^"|"$/g, ""));
      next = raw === null ? null : JSON.stringify(raw);
    } else if (hit.kind === "enum") {
      next = window.prompt("Default " + hit.pin.name, next || "." + hit.pin.name);
    }
    if (next === null || next === undefined || next === "") return true;
    postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: hit.expr.inline_expr_id, new_expr: next });
    return true;
  }

  function numericType(type) {
    return ["Int", "Float", "F32", "F64"].includes(type || "");
  }

  function compatiblePin(from, to) {
    if (!from || !to || from.pin_id === to.pin_id) return false;
    if (from.direction === to.direction) return false;
    const out = from.direction === "output" ? from : to;
    const input = from.direction === "input" ? from : to;
    if (out.type === input.type) return true;
    return numericType(out.type) && numericType(input.type);
  }

  function connectionPlan(graph, fromPin, toPin) {
    if (!fromPin) return { ok: false, label: "No pin", color: "#fb7185" };
    if (!toPin) return { ok: true, label: "Release for actions", color: "#7dd3fc" };
    if (fromPin.pin_id === toPin.pin_id) return { ok: false, label: "Same socket", color: "#fb7185" };
    if (fromPin.direction === toPin.direction) {
      const expected = fromPin.direction === "output" ? "Drop on an input socket" : "Drop on an output socket";
      return { ok: false, label: expected, color: "#fb7185" };
    }
    const out = fromPin.direction === "output" ? fromPin : toPin;
    const input = fromPin.direction === "input" ? fromPin : toPin;
    if (!compatiblePin(fromPin, toPin)) return { ok: false, label: "Type mismatch " + (out.type || "?") + " -> " + (input.type || "?"), color: "#fb7185" };
    if (!exactPinMatch(fromPin, toPin)) return { ok: true, label: "Insert visible conversion", color: "#f59e0b" };
    const wire = wireIntoPin(graph, input);
    const replacement = sourceExprForOutputPin(out);
    if (wire && replacement) return { ok: true, label: "Rewire source", color: "#a7f3d0" };
    return { ok: true, label: "Compatible preview", color: "#fde68a" };
  }

  function drawCompatibleDropTargets(graph, fromPin) {
    const seen = new Set();
    for (const hit of pinHit) {
      const pin = hit.pin;
      if (!compatiblePin(fromPin, pin) || seen.has(pin.pin_id)) continue;
      seen.add(pin.pin_id);
      const point = pinPoints.get(pin.pin_id);
      if (!point) continue;
      ctx.beginPath();
      ctx.arc(point.x, point.y, Math.max(13, 18 * view.zoom), 0, Math.PI * 2);
      ctx.strokeStyle = exactPinMatch(fromPin, pin) ? "#a7f3d0" : "#f59e0b";
      ctx.lineWidth = Math.max(1.4, 2.2 * view.zoom);
      ctx.shadowColor = exactPinMatch(fromPin, pin) ? "rgba(167,243,208,.52)" : "rgba(245,158,11,.44)";
      ctx.shadowBlur = 10;
      ctx.stroke();
      ctx.shadowBlur = 0;
    }
  }

  function drawConnectionBadge(plan, x, y) {
    const text = plan.label || "";
    ctx.font = `${Math.max(10, 12 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 9 * view.zoom;
    const badgeW = Math.min(230 * view.zoom, ctx.measureText(text).width + padX * 2);
    const badgeH = 26 * view.zoom;
    const bx = Math.min(x + 14 * view.zoom, cssSize().width - badgeW - 8);
    const by = Math.max(8, y - badgeH - 12 * view.zoom);
    roundRect(bx, by, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.94)";
    ctx.fill();
    ctx.strokeStyle = plan.color || "#7dd3fc";
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = plan.color || "#dbeafe";
    ctx.fillText(clipText(text, 28), bx + padX, by + 17 * view.zoom);
  }

  function drawWireTypeBadge(wire, from, to) {
    const label = wire.wire_kind === "control" ? "EXEC" : (from.pin && from.pin.type) || wire.wire_kind || "Value";
    const color = wireColor(wire, from);
    const mx = (from.x + to.x) / 2;
    const my = (from.y + to.y) / 2;
    ctx.font = `${Math.max(8, 10 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 7 * view.zoom;
    const badgeW = Math.min(112 * view.zoom, ctx.measureText(label).width + padX * 2);
    const badgeH = 19 * view.zoom;
    roundRect(mx - badgeW / 2, my - badgeH / 2, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.90)";
    ctx.fill();
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(.8, view.zoom);
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.fillText(clipText(label, 16), mx, my + 3.5 * view.zoom);
    ctx.textAlign = "left";
  }

  function exactPinMatch(from, to) {
    if (!from || !to) return false;
    const out = from.direction === "output" ? from : to;
    const input = from.direction === "input" ? from : to;
    return out.type === input.type;
  }

  function sourceExprForOutputPin(pin) {
    if (!pin || pin.direction !== "output") return null;
    const name = pin.name || "";
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !["value", "result", "ok", "target"].includes(name)) return name;
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph && (graph.nodes || []).find((n) => n.node_id === pin.node_id);
    if (node && /^[A-Za-z_][A-Za-z0-9_]*$/.test(node.title) && node.kind === "binding") return node.title;
    return null;
  }

  function wireIntoPin(graph, pin) {
    if (!graph || !pin) return null;
    return (graph.wires || []).find((w) => w.to_pin === pin.pin_id && w.source_span);
  }

  function inlineForPin(graph, pin) {
    if (!graph || !pin || !pin.source_span) return null;
    return (graph.inline_exprs || []).find((e) => e.source_span && spansOverlap(e.source_span, pin.source_span));
  }

  function selectMarquee() {
    if (!drag || drag.mode !== "marquee") return;
    const x0 = Math.min(drag.x, drag.mx), x1 = Math.max(drag.x, drag.mx);
    const y0 = Math.min(drag.y, drag.my), y1 = Math.max(drag.y, drag.my);
    const next = drag.additive ? new Set(selectedNodeIds) : new Set();
    for (const h of hit) {
      if (h.x < x1 && h.x + h.w > x0 && h.y < y1 && h.y + h.h > y0) next.add(h.node.node_id);
    }
    selectedNodeIds = next;
    selectedNodeId = [...selectedNodeIds][0] || selectedNodeId;
  }

  function completeConnection(fromPin, target, graph) {
    const plan = connectionPlan(graph, fromPin, target);
    window.__jetCanvasLastConnectionPlan = plan;
    if (compatiblePin(fromPin, target)) {
      const out = fromPin.direction === "output" ? fromPin : target;
      const input = fromPin.direction === "input" ? fromPin : target;
      const wire = wireIntoPin(graph, input);
      const replacement = sourceExprForOutputPin(out);
      if (exactPinMatch(fromPin, target) && wire && replacement) {
        postTransaction({ schema_version: 1, op: "move_link", revision: latestDoc.revision, wire_id: wire.wire_id, replacement });
      } else if (!exactPinMatch(fromPin, target)) {
        const expr = inlineForPin(graph, input);
        const callee = window.prompt("Visible conversion function", (input.type || "Value") + ".from");
        if (expr && callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, callee });
        else showToast("Conversion needs an inline source expression");
      } else {
        showToast(plan.label + ": no safe source anchor");
      }
      return true;
    }
    if (target) showToast("Wire refused: " + plan.label);
    return false;
  }

  canvas.addEventListener("click", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const pinEditor = hitPinDefaultEditorAt(x, y);
    if (pinEditor && applyPinDefaultEditor(pinEditor)) {
      ev.preventDefault();
      return;
    }
    if (hitPinAt(x, y)) {
      ev.preventDefault();
      return;
    }
    const found = hitNodeAt(x, y);
    if (found) selectNode(found.node, ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : "replace");
  });

  canvas.addEventListener("dblclick", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const found = hitNodeAt(ev.clientX - rect.left, ev.clientY - rect.top);
    if (found && openFunctionGraph(found.node.title)) ev.preventDefault();
  });

  canvas.addEventListener("mousedown", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    if (hitPinDefaultEditorAt(x, y)) return;
    const pin = hitPinAt(x, y);
    if (pin) {
      hoverPin = pin;
      drag = { mode: "pin", pin, x, y, mx: x, my: y };
      showToast(pin.name + ": " + pin.type);
      return;
    }
    setPendingPin(null);
    const found = hitNodeAt(x, y);
    if (found) {
      selectNode(found.node, ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : "replace");
      const starts = new Map();
      for (const id of selectedNodeIds) starts.set(id, nodeOffsets.get(id) || { x: 0, y: 0 });
      drag = { mode: "node", x, y, wx: wx(x), wy: wy(y), starts };
    } else if (ev.button === 1 || ev.altKey || spaceDown) {
      drag = { mode: "pan", x, y, ox: view.x, oy: view.y };
    } else {
      drag = { mode: "marquee", x, y, mx: x, my: y, additive: ev.shiftKey || ev.ctrlKey || ev.metaKey };
    }
  });

  window.addEventListener("mousemove", function (ev) {
    const rect = canvas.getBoundingClientRect();
    if (!drag) {
      const nextHover = hitPinAt(ev.clientX - rect.left, ev.clientY - rect.top);
      if ((nextHover && !hoverPin) || (!nextHover && hoverPin) || (nextHover && hoverPin && nextHover.pin_id !== hoverPin.pin_id)) {
        hoverPin = nextHover;
        if (latestDoc) drawGraph(latestDoc);
      }
      return;
    }
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    if (drag.mode === "pan") {
      view.x = drag.ox + (x - drag.x);
      view.y = drag.oy + (y - drag.y);
    } else if (drag.mode === "node") {
      const dx = wx(x) - drag.wx;
      const dy = wy(y) - drag.wy;
      for (const [id, start] of drag.starts.entries()) nodeOffsets.set(id, { x: start.x + dx, y: start.y + dy });
    } else if (drag.mode === "marquee") {
      drag.mx = x;
      drag.my = y;
      selectMarquee();
    } else if (drag.mode === "pin") {
      drag.mx = x;
      drag.my = y;
      hoverPin = hitPinAt(x, y);
    }
    if (latestDoc) drawGraph(latestDoc);
  });

  window.addEventListener("mouseup", function (ev) {
    if (drag && drag.mode === "node") showToast("Moved " + selectedNodeIds.size + " node" + (selectedNodeIds.size === 1 ? "" : "s") + " locally");
    if (drag && drag.mode === "pin") {
      const rect = canvas.getBoundingClientRect();
      const target = hitPinAt(ev.clientX - rect.left, ev.clientY - rect.top) || hoverPin;
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      const moved = Math.abs(drag.mx - drag.x) > 5 || Math.abs(drag.my - drag.y) > 5;
      if (!moved && pendingPin && target) {
        const done = completeConnection(pendingPin, target, graph);
        setPendingPin(done || pendingPin.pin_id === target.pin_id ? null : pendingPin);
      } else if (!moved && target && target.pin_id === drag.pin.pin_id) {
        if (pendingPin && pendingPin.pin_id === target.pin_id) {
          setPendingPin(null);
          showToast("Connection cancelled");
        } else {
          setPendingPin(drag.pin);
          showToast("Select destination socket");
        }
      } else if (target) {
        setPendingPin(null);
        completeConnection(drag.pin, target, graph);
      } else {
        setPendingPin(null);
        openPinMenu(drag.pin, ev.clientX, ev.clientY);
      }
    }
    drag = null;
    if (latestDoc) drawGraph(latestDoc);
  });

  canvas.addEventListener("contextmenu", function (ev) {
    ev.preventDefault();
    closeContextMenu();
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const pin = hitPinAt(x, y);
    if (pin) {
      openPinMenu(pin, ev.clientX, ev.clientY);
      return;
    }
    const found = hitNodeAt(x, y);
    if (found) {
      selectNode(found.node, "replace");
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      openContextMenu(ev.clientX, ev.clientY, found.node.title, nodeContextActions(graph, found.node));
    } else {
      openGraphActionPalette(ev.clientX, ev.clientY);
    }
  });

  canvas.addEventListener("wheel", function (ev) {
    ev.preventDefault();
    closeContextMenu();
    const rect = canvas.getBoundingClientRect();
    const mx = ev.clientX - rect.left;
    const my = ev.clientY - rect.top;
    const before = { x: wx(mx), y: wy(my) };
    const factor = ev.deltaY < 0 ? 1.09 : .92;
    view.zoom = Math.max(.35, Math.min(2.2, view.zoom * factor));
    view.x = mx - before.x * view.zoom;
    view.y = my - before.y * view.zoom;
    if (latestDoc) drawGraph(latestDoc);
  }, { passive: false });

  graphBack.addEventListener("click", () => {
    if (!graphBackStack.length || !selectedGraphId) return;
    const previous = graphBackStack.pop();
    graphForwardStack.push(selectedGraphId);
    switchGraph(previous, { push: false, toast: "Back to " + graphTitle(previous) });
  });
  graphForward.addEventListener("click", () => {
    if (!graphForwardStack.length || !selectedGraphId) return;
    const next = graphForwardStack.pop();
    graphBackStack.push(selectedGraphId);
    switchGraph(next, { push: false, toast: "Forward to " + graphTitle(next) });
  });
  dockGraphs.addEventListener("click", () => setDrawer(dockGraphs.classList.contains("is-active") ? null : "graphs"));
  dockDetails.addEventListener("click", () => setDrawer(dockDetails.classList.contains("is-active") ? null : "details"));
  document.getElementById("fit").addEventListener("click", fitGraph);
  document.getElementById("reload").addEventListener("click", loadGraph);
  sourceDiff.addEventListener("click", showSourceDiff);
  viewToggle.addEventListener("click", () => setViewMode(viewMode === "graph" ? "code" : "graph"));
  if (editSource) editSource.addEventListener("click", () => setSourceEditMode(true));
  if (applySourceEdit) applySourceEdit.addEventListener("click", applySourceEditBuffer);
  if (cancelSourceEdit) cancelSourceEdit.addEventListener("click", () => setSourceEditMode(false));
  for (const button of lensButtons) {
    button.addEventListener("click", () => setViewMode(button.getAttribute("data-view-mode") || "graph"));
  }
  for (const input of detailToggleInputs) {
    input.addEventListener("change", () => {
      const key = input.getAttribute("data-detail-toggle");
      detailToggles[key] = input.checked;
      syncDetailToggles();
      saveDetailToggles();
      if (latestDoc) drawGraph(latestDoc);
    });
  }
  developerModeButton.addEventListener("click", () => setDeveloperMode(!developerMode));
  undoEdit.addEventListener("click", undoTransaction);
  redoEdit.addEventListener("click", redoTransaction);
  orgAlign.addEventListener("click", () => alignSelectedNodes("y"));
  orgTidy.addEventListener("click", tidyGraphLayout);
  bookmarkAdd.addEventListener("click", bookmarkCurrentGraph);
  bookmarkJump.addEventListener("click", jumpBookmark);
  favoriteAction.addEventListener("click", toggleFavoriteAction);
  runCurrent.addEventListener("click", runCurrentGraph);
  tourDismiss.addEventListener("click", () => {
    editorState.tourDismissed = true;
    saveEditorState();
    firstRunTour.classList.remove("is-open");
  });
  debugStep.addEventListener("click", () => runDebug(["s"]));
  debugNext.addEventListener("click", () => runDebug(["n"]));
  debugContinue.addEventListener("click", () => runDebug(["c"]));
  debugStop.addEventListener("click", stopDebug);
  debugBreak.addEventListener("click", () => {
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph ? graph.nodes.find((n) => n.node_id === selectedNodeId) : null;
    if (node) toggleBreakpoint(node);
  });
  debugWatch.addEventListener("click", () => {
    const name = window.prompt("Watch local", "");
    addWatch(name && name.trim());
  });
  canvasSearch.addEventListener("input", runCanvasSearch);
  window.addEventListener("hashchange", applySourceHash);

  window.addEventListener("keydown", function (ev) {
    if (ev.key === " ") spaceDown = true;
    if (ev.key === "Escape") closeContextMenu();
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "`") {
      ev.preventDefault();
      setViewMode(viewMode === "graph" ? "code" : "graph");
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "k") {
      ev.preventDefault();
      const rect = canvas.getBoundingClientRect();
      openGraphActionPalette(rect.left + Math.min(rect.width - 20, 220), rect.top + 90);
      showToast("Context actions");
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "f") {
      ev.preventDefault();
      canvasSearch.focus();
      canvasSearch.select();
      showToast("Find in Canvas");
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "p") {
      ev.preventDefault();
      const rect = canvas.getBoundingClientRect();
      openGraphActionPalette(rect.left + Math.min(rect.width - 20, 220), rect.top + 90);
      showToast("Context actions");
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "a") {
      ev.preventDefault();
      alignSelectedNodes(ev.shiftKey ? "x" : "y");
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "t") {
      ev.preventDefault();
      tidyGraphLayout();
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "r") {
      ev.preventDefault();
      addRerouteKnot();
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "b") {
      ev.preventDefault();
      bookmarkCurrentGraph();
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "g") {
      ev.preventDefault();
      jumpBookmark();
      return;
    }
    if (ev.altKey && ev.key === "Enter") {
      ev.preventDefault();
      runCurrentGraph();
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "z") {
      ev.preventDefault();
      undoTransaction();
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && (ev.key === "y" || (ev.shiftKey && ev.key === "Z"))) {
      ev.preventDefault();
      redoTransaction();
      return;
    }
    if (ev.key === "/" && document.activeElement !== canvasSearch) {
      ev.preventDefault();
      canvasSearch.focus();
      return;
    }
    if (ev.key === "Escape") {
      if (compactCanvasMode() && (leftDrawer.classList.contains("is-drawer-open") || rightDrawer.classList.contains("is-drawer-open"))) {
        setDrawer(null);
        return;
      }
      selectedNodeIds = new Set();
      selectedNodeId = null;
      if (latestDoc) drawGraph(latestDoc);
      return;
    }
    if (ev.key === "f" && document.activeElement !== canvasSearch) {
      ev.preventDefault();
      fitGraph();
      return;
    }
    const arrows = { ArrowLeft: [-16, 0], ArrowRight: [16, 0], ArrowUp: [0, -16], ArrowDown: [0, 16] };
    if (arrows[ev.key] && selectedNodeIds.size > 0) {
      ev.preventDefault();
      const step = ev.shiftKey ? 4 : 1;
      const [dx, dy] = arrows[ev.key];
      for (const id of selectedNodeIds) {
        const old = nodeOffsets.get(id) || { x: 0, y: 0 };
        nodeOffsets.set(id, { x: old.x + dx * step, y: old.y + dy * step });
      }
      if (latestDoc) drawGraph(latestDoc);
    }
  });

  window.addEventListener("keyup", function (ev) {
    if (ev.key === " ") spaceDown = false;
  });

  document.addEventListener("click", function (ev) {
    if (!contextMenu.contains(ev.target)) closeContextMenu();
  });

  function handleFunctionPinButton(ev) {
    const button = ev.target && ev.target.closest && ev.target.closest("#apply-function-pins, #set-function-output, #remove-function-output, #add-function-output");
    if (!button || !latestDoc) return;
    const now = Date.now();
    const handled_at = Number(button.getAttribute("data-canvas-handled-at") || "0");
    if (ev.type === "click" && now - handled_at < 900) return;
    button.setAttribute("data-canvas-handled-at", String(now));
    const graph = currentGraph(latestDoc);
    if (!graph || !graph.function) return;
    ev.preventDefault();
    ev.stopPropagation();
    if (ev.stopImmediatePropagation) ev.stopImmediatePropagation();
    if (button.id === "remove-function-output") {
      const ret = document.getElementById("function-return-type");
      if (ret) ret.value = "Void";
    } else if (button.id === "add-function-output") {
      const ret = document.getElementById("function-return-type");
      if (ret && (!ret.value.trim() || ret.value.trim() === "Void")) {
        ret.value = "Int";
        syncReturnEditorPreview(ret.value);
        window.__jetCanvasLastSignature = signatureFromVisibleFunctionPins(graph.function, graph.title);
        showToast("Output pin ready");
        return;
      }
    }
    applyFunctionPins(graph, graph.function, graph.title, button.id === "remove-function-output" ? "Void" : undefined);
  }
  function defaultArgsForAction(item, pin) {
    const existing = (item.default_args || item.args || []).slice();
    if (!pin) return existing.length ? existing : ["\"canvas\""];
    const graph = currentGraph(latestDoc);
    if (pin.direction === "output") {
      const expr = sourceExprForOutputPin(pin);
      if (expr) {
        const inputs = (item.pins || []).filter((p) => p.direction === "input");
        if (!inputs.length || inputs.some((p) => compatibleActionType(p.type, pin.type))) return [expr].concat(existing.slice(1));
      }
    }
    return existing.length ? existing : ["1"];
  }

  function callExpressionForAction(item, pin) {
    const callee = item.insert_callee || item.callee || item.title;
    const args = defaultArgsForAction(item, pin);
    return callee + "(" + args.join(", ") + ")";
  }

  function runPalette(item, pinContext) {
    if (!latestDoc || !selectedGraphId) return;
    const pin = pinContext || (contextMenuState && contextMenuState.pin) || null;
    if (item.kind === "canvas.core_catalog") {
      postTransaction({ schema_version: 1, op: "insert_call", revision: latestDoc.revision, graph_id: selectedGraphId, callee: item.insert_callee || item.callee, args: defaultArgsForAction(item, pin) });
    } else if (item.kind === "project_function") {
      if (pin && pin.direction === "input") {
        const graph = currentGraph(latestDoc);
        const expr = inlineForPin(graph, pin);
        if (expr) return postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: callExpressionForAction(item, pin) });
      }
      postTransaction({ schema_version: 1, op: "insert_call", revision: latestDoc.revision, graph_id: selectedGraphId, callee: item.callee, args: defaultArgsForAction(item, pin) });
    } else if (item.op === "preview_canvas_action") {
      if (pin && pin.direction === "input") {
        const graph = currentGraph(latestDoc);
        const expr = inlineForPin(graph, pin);
        if (expr && item.callee) return postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: callExpressionForAction(item, pin) });
      }
      postTransaction({ schema_version: 1, op: "preview_canvas_action", revision: latestDoc.revision, graph_id: selectedGraphId, action_id: item.action_id, callee: item.callee, args: defaultArgsForAction(item, pin) });
    } else if (item.op === "command_authority") {
      renderCommandAuthority(item);
    } else if (item.op === "insert_print") {
      postTransaction({ schema_version: 1, op: "insert_call", revision: latestDoc.revision, graph_id: selectedGraphId, callee: "print", args: ["\"canvas\""] });
    } else if (item.op === "insert_call") {
      const callee = item.insert_callee || item.callee || window.prompt("Call function", "print");
      if (callee) postTransaction({ schema_version: 1, op: "insert_call", revision: latestDoc.revision, graph_id: selectedGraphId, callee, args: defaultArgsForAction(item.insert_callee || item.callee ? item : { args: ["\"canvas\""] }, pin) });
    } else if (["insert_branch", "insert_switch", "insert_loop", "insert_fallible_rail"].includes(item.op)) {
      postTransaction({ schema_version: 1, op: item.op, revision: latestDoc.revision, graph_id: selectedGraphId });
    } else if (item.op === "comment") {
      const graph = currentGraph(latestDoc);
      const node = graph && (graph.nodes.find((n) => n.node_id === selectedNodeId) || graph.nodes[0]);
      if (!node || !node.source_span) return showToast("Select a source node first");
      const b = { x: nodeX(node) - 28, y: nodeY(node) - 40, w: 246, h: 166 };
      const title = window.prompt("Comment title", "Comment");
      if (title) postTransaction({
        schema_version: 1,
        op: "create_comment_region",
        revision: latestDoc.revision,
        graph_id: selectedGraphId,
        start: node.source_span.start,
        end: node.source_span.end,
        title,
        color: "#2563eb",
        alpha: "0.18",
        bounds: [b.x, b.y, b.w, b.h].map((n) => Math.round(n)).join(",")
      });
    } else {
      showToast(item.title + " needs the next write transaction");
    }
  }

  function postTransaction(body) {
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    const beforeSource = latestDoc && latestDoc.source_text;
    window.__jetCanvasLastTx = body;
    fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        window.__jetCanvasLastTxResult = result.json;
        if (!result.ok) { showToast(result.json.message || "Edit rejected"); return; }
        if (result.json.protocol === "jet.canvas.action") {
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.impact = null;
          searchState.diff = { text: result.json.diff || "clean" };
          renderSearchResults();
          showToast("Canvas action preview validated");
          return;
        }
        if (result.json.changed && beforeSource && result.json.source_text && (body.op !== "replace_source" || body.source_edit)) {
          undoStack.push({ before: beforeSource, after: result.json.source_text });
          redoStack = [];
        }
        if (body.op === "replace_source" && body.source_edit) setSourceEditMode(false);
        showToast(result.json.changed ? "Source updated" : "No change");
        loadSourceControl();
        loadProject();
        loadGraph();
      })
      .catch((e) => showToast(String(e)));
  }

  function restoreSource(source, redoEntry, undoEntry) {
    if (!latestDoc || !source) return;
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ schema_version: 1, op: "replace_source", revision: latestDoc.revision, source }) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) { showToast(result.json.message || "Undo rejected"); return; }
        if (redoEntry) redoStack.push(redoEntry);
        if (undoEntry) undoStack.push(undoEntry);
        showToast("Source restored");
        loadSourceControl();
        loadGraph();
      })
      .catch((e) => showToast(String(e)));
  }

  function undoTransaction() {
    const entry = undoStack.pop();
    if (!entry) return showToast("Nothing to undo");
    restoreSource(entry.before, entry);
  }

  function redoTransaction() {
    const entry = redoStack.pop();
    if (!entry) return showToast("Nothing to redo");
    restoreSource(entry.after, null, entry);
  }

  function graphRequestUrl(sourceId) {
    if (!sourceId) return graphUrl;
    return graphUrl + (graphUrl.includes("?") ? "&" : "?") + "source_id=" + encodeURIComponent(sourceId);
  }

  function loadGraph(sourceId) {
    if (typeof sourceId === "string") selectedSourceId = sourceId || null;
    return fetch(graphRequestUrl(selectedSourceId), { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        latestDoc = doc;
        loadEditorState(doc);
        loadDetailToggles(doc);
        sourceView.textContent = doc.source_text || "";
        const firstLoad = selectedGraphId === null;
        drawGraph(doc);
        setViewMode(viewMode);
        loadProject();
        loadSourceControl();
        loadProofRail();
        loadCanvasActions();
        applySourceHash();
        if (firstLoad) fitGraph();
      })
      .catch((e) => { jump.textContent = "Canvas graph failed"; details.textContent = String(e); });
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" }[c]));
  }

  function escapeAttr(s) {
    return escapeHtml(s).replace(/`/g, "&#96;");
  }

  function loadCanvasActions() {
    if (!latestDoc) return;
    fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ schema_version: 1, revision: latestDoc.revision, op: "actions" })
    })
      .then((r) => r.json())
      .then((doc) => {
        if (!doc || !doc.actions) return;
        const projectFunctions = (doc.project_functions || []).map((fn) => ({
          title: fn.name || fn.callee,
          detail: (fn.module_path || "project") + " · " + (fn.signature || fn.callee || ""),
          kind: "project_function",
          group: "Project",
          op: fn.insert_op || "insert_call",
          callee: fn.callee || fn.name,
          insert_callee: fn.insert_callee || fn.callee || fn.name,
          module_path: fn.module_path || "project",
          action_id: "project:" + (fn.callee || fn.name),
          signature: fn.signature || "",
          pure: !!fn.pure,
          pins: fn.pins || [],
          ret: actionReturnType(fn) || fn.ret || "Void",
          args: fn.default_args || ["1"]
        }));
        const canvasActions = doc.actions.map((action) => ({
          title: action.title || action.callee,
          detail: action.kind === "canvas.core_catalog" ? ((action.module_path || "core") + " · " + (action.signature || action.callee || "") + " · read-only") : (action.command ? ((action.kind || "canvas.command") + " · " + (action.command || []).join(" ") + " · " + (action.writes || "none")) : ((action.kind || "canvas.action") + " · " + (action.engine || "checked-tir+jit") + " · " + (action.callee || "") + "(" + (action.pins || []).filter((p) => p.direction === "input").map((p) => p.type || "Value").join(", ") + ") -> " + (action.ret || "Void"))),
          kind: action.kind || "canvas.action",
          group: action.kind === "canvas.core_catalog" ? "Core" : action.kind === "canvas.command" ? "Flow" : "Project",
          op: action.insert_op || action.op || (action.kind === "canvas.action" || action.kind === "canvas.core_catalog" ? "insert_call" : "preview_canvas_action"),
          action_id: action.action_id,
          callee: action.callee,
          module_path: action.module_path || "",
          insert_callee: action.insert_callee || action.callee,
          signature: action.signature || "",
          summary: action.summary || "",
          command: action.command || [],
          authority: action.authority || [],
          writes: action.writes || "none",
          requires_confirmation: !!action.requires_confirmation,
          available: action.available !== false,
          denied_reason: action.denied_reason || "",
          pins: action.pins || [],
          pure: !!action.pure,
          ret: action.ret || actionReturnType(action) || "Void",
          args: action.default_args || ["\"canvas\""]
        }));
        actionEntries = projectFunctions.concat(canvasActions);
        if (canvasActions.some((action) => action.kind === "canvas.core_catalog")) coreCatalogLoaded = true;
      })
      .catch(() => {});
  }

  function mergeActionEntries(entries) {
    const seen = new Set(actionEntries.map((entry) => entry.action_id || entry.kind + ":" + entry.title + ":" + entry.module_path));
    for (const entry of entries) {
      const id = entry.action_id || entry.kind + ":" + entry.title + ":" + entry.module_path;
      if (seen.has(id)) continue;
      seen.add(id);
      actionEntries.push(entry);
    }
  }

  function loadCoreCatalogActions(query = "") {
    if (coreCatalogLoaded && !query) return Promise.resolve(actionEntries);
    if (coreCatalogLoading && !query) return coreCatalogLoading;
    const url = coreCatalogUrl + "?query=" + encodeURIComponent(query || "");
    coreCatalogLoading = fetch(url, { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        const entries = [];
        for (const module of doc.modules || []) {
          for (const member of module.members || []) {
            const callee = String(module.path || "core") + "." + member.name;
            entries.push({
              title: member.name + " · " + (module.path || "core"),
              detail: (module.path || "core") + " · " + (member.signature || member.name),
              group: "Core",
              kind: "canvas.core_catalog",
              op: "insert_call",
              action_id: "canvas.core_catalog:" + (module.path || "core") + ":" + member.name,
              module_path: module.path || "core",
              callee,
              insert_callee: callee,
              signature: member.signature || "",
              summary: member.summary || module.summary || "",
              pure: !!member.pure,
              pins: member.pins || [],
              ret: actionReturnType(member) || "Value",
              args: member.default_args || ["1"],
              default_args: member.default_args || ["1"]
            });
          }
        }
        mergeActionEntries(entries);
        if (!query) coreCatalogLoaded = true;
        window.__jetCanvasCoreCatalogPalette = entries.length;
        return actionEntries;
      })
      .catch(() => actionEntries)
      .finally(() => { if (!query) coreCatalogLoading = null; });
    return coreCatalogLoading;
  }

  function cssEscape(s) {
    if (window.CSS && CSS.escape) return CSS.escape(s);
    return String(s).replace(/["\\]/g, "\\$&");
  }

  window.__jetCanvasPinAuthoring = true;
  window.__jetCanvasDebugOverlay = true;
  window.__jetCanvasFrontendFamily = {
    family: "modified_hybrid",
    workbenchProjectViewer: true,
    codeSplitGraphLens: true,
    contextCommands: true,
    dragPinCompatibleMenu: true,
    hoverOnlyTypes: true,
    getterCapsules: true,
    embeddedVariables: true,
    graphiteDetailToggles: ["types", "diagnostics", "effects", "debug", "package"]
  };
  setDeveloperMode(storedFlag("jet.canvas.developerMode"));
  if (coreCatalog) coreCatalog.addEventListener("click", () => openCoreCatalogPalette(""));
  syncDetailToggles();
  setViewMode("graph");
  details.innerHTML = "<h2>Details</h2><p>Select a node.</p>";
  window.addEventListener("resize", function () {
    if (!compactCanvasMode()) setDrawer(null);
    if (latestDoc) window.requestAnimationFrame(fitGraph);
  });

  const base = window.__JET_CANVAS_BASE__ || "/canvas";
  const graphUrl = window.__JET_CANVAS_GRAPH__ || (base + "/graph");
  const queryUrl = window.__JET_CANVAS_QUERY__ || (base + "/query");
  const coreCatalogUrl = window.__JET_CANVAS_CORE_CATALOG__ || (base + "/core-catalog");
  const sourceControlUrl = window.__JET_CANVAS_SCM__ || (base + "/source-control");
  const proofUrl = window.__JET_CANVAS_PROOF__ || (base + "/proof");
  const commandUrl = window.__JET_CANVAS_COMMAND__ || (base + "/command");
  window.__jetCanvasProofRail = true;
  loadGraph();
})();
"###
    .to_string()
}

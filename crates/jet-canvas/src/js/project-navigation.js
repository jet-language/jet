
// Project panels, graph navigation, variables, source-backed actions, and debugger state.
  let keyboardHelpReturnFocus = null;

  function keyboardCheatSheetIsOpen() {
    const dialog = document.getElementById("keyboard-cheat-sheet");
    return !!dialog && (dialog.open || dialog.hasAttribute("open"));
  }

  function closeKeyboardCheatSheet() {
    const dialog = document.getElementById("keyboard-cheat-sheet");
    if (!dialog) return false;
    if (dialog.open && typeof dialog.close === "function") dialog.close();
    else dialog.removeAttribute("open");
    dialog.setAttribute("aria-hidden", "true");
    window.__jetCanvasKeyboardCheatSheet = { open: false };
    const returnFocus = keyboardHelpReturnFocus;
    keyboardHelpReturnFocus = null;
    if (returnFocus && document.contains(returnFocus) && typeof returnFocus.focus === "function") {
      window.requestAnimationFrame(() => returnFocus.focus({ preventScroll: true }));
    }
    return true;
  }

  function openKeyboardCheatSheet() {
    const dialog = document.getElementById("keyboard-cheat-sheet");
    if (!dialog) return false;
    if (keyboardCheatSheetIsOpen()) return true;
    keyboardHelpReturnFocus = document.activeElement && document.activeElement !== document.body
      ? document.activeElement
      : document.getElementById("jet-canvas-view");
    if (typeof dialog.showModal === "function") dialog.showModal();
    else dialog.setAttribute("open", "");
    dialog.setAttribute("aria-hidden", "false");
    window.__jetCanvasKeyboardCheatSheet = { open: true, shortcut: "?", focus: "keyboard-cheat-sheet-close" };
    window.requestAnimationFrame(() => {
      const close = document.getElementById("keyboard-cheat-sheet-close");
      if (close) close.focus({ preventScroll: true });
    });
    return true;
  }

  function createCanvasFunction() {
    if (!latestDoc) return false;
    const name = window.prompt("Function name", "helper");
    if (!name) return false;
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      showToast("Function name must be a Jet identifier", { isError: true });
      return false;
    }
    postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "", ret_type: "Int" });
    return true;
  }

  function callbackEventView(graph) {
    return (graph && graph.event_views || []).find((event) => event.kind === "callback_event" && event.function) || null;
  }

  function createCanvasCallback() {
    const trigger = document.getElementById("canvas-new-callback");
    if (!latestDoc) return false;
    const name = window.prompt("Callback name", "on_event");
    if (name === null) {
      window.__jetCanvasLastTx = null;
      window.__jetCanvasLastTxResult = { ok: false, changed: false, code: "client_cancelled", message: "Callback creation cancelled" };
      if (trigger) trigger.focus({ preventScroll: true });
      return false;
    }
    if (!/^on_[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      const message = "Callback name must start with on_ and be a Jet identifier";
      window.__jetCanvasLastTx = null;
      window.__jetCanvasLastTxResult = { ok: false, changed: false, code: "client_callback_gate", message };
      showToast(message, { isError: true });
      if (trigger) trigger.focus({ preventScroll: true });
      return false;
    }
    const request = postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "", ret_type: "Void" });
    if (request && typeof request.then === "function") {
      return request.then((result) => {
        if (result && result.ok && result.json && result.json.changed) openFunctionGraph(name);
        if (trigger) trigger.focus({ preventScroll: true });
        return result;
      });
    }
    return request;
  }

  function addCanvasVariable() {
    const graph = currentGraphOrNull();
    const expr = graph && selectedNodeId
      ? (graph.inline_exprs || []).find((candidate) => candidate.node_id === selectedNodeId)
      : null;
    if (!expr) {
      showToast("Select a value expression first; Add promotes it to a source-backed variable", { isError: true });
      return false;
    }
    const name = window.prompt("Binding name", "value");
    if (!name) return false;
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      showToast("Binding name must be a Jet identifier", { isError: true });
      return false;
    }
    postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, name });
    return true;
  }

  function ensureCanvasChrome() {
    const stage = document.getElementById("stage");
    if (stage && !document.getElementById("canvas-state")) {
      const state = document.createElement("section");
      state.id = "canvas-state";
      state.setAttribute("role", "status");
      state.setAttribute("aria-live", "polite");
      state.setAttribute("aria-atomic", "true");
      state.setAttribute("aria-labelledby", "canvas-state-title");
      state.setAttribute("aria-describedby", "canvas-state-detail");
      state.setAttribute("aria-hidden", "true");
      state.style.cssText = "position:absolute;z-index:28;left:50%;top:12px;transform:translateX(-50%);display:none;grid-template-columns:minmax(0,1fr) auto;gap:12px;align-items:center;width:min(520px,calc(100% - 24px));padding:10px 12px;border:1px solid #365a7f;border-radius:7px;background:rgba(7,16,28,.96);box-shadow:0 18px 52px rgba(0,0,0,.48);color:#c9dcf2;pointer-events:none";
      state.innerHTML = "<div><b id=\"canvas-state-title\" style=\"display:block;color:#f8fbff\"></b><span id=\"canvas-state-detail\" style=\"display:block;margin-top:3px;color:#9fb9d8;line-height:1.35\"></span></div><div id=\"canvas-state-actions\" style=\"display:flex;gap:6px;flex-wrap:wrap;justify-content:end;pointer-events:auto\"></div>";
      stage.appendChild(state);
    }
    const statusbar = document.getElementById("statusbar");
    if (statusbar && !document.getElementById("save-state")) {
      const saved = document.createElement("span");
      saved.id = "save-state";
      saved.textContent = "Source Saved";
      saved.style.color = "#8fb2dc";
      statusbar.appendChild(saved);
    }
    const tour = document.getElementById("first-run-tour");
    if (tour && !document.getElementById("tour-next")) {
      tour.innerHTML = "<div style=\"display:flex;align-items:center;justify-content:space-between;gap:8px\"><span id=\"tour-progress\" style=\"color:#8fb2dc;font:10px ui-monospace,monospace;letter-spacing:.1em;text-transform:uppercase\"></span><button id=\"tour-dismiss\" type=\"button\" style=\"min-height:26px;padding:0 8px\">Finish</button></div><b id=\"tour-title\"></b><span id=\"tour-detail\" style=\"line-height:1.45\"></span><div style=\"display:flex;gap:6px;justify-content:end;flex-wrap:wrap\"><button id=\"tour-back\" type=\"button\">Back</button><button id=\"tour-action\" class=\"primary\" type=\"button\"></button><button id=\"tour-next\" class=\"primary\" type=\"button\">Next</button></div>";
    }
    const more = document.querySelector("#more-tools-toggle + .toolbar-popover");
    if (more && !document.getElementById("tour-open")) {
      const open = document.createElement("button");
      open.id = "tour-open";
      open.type = "button";
      open.textContent = "Tour";
      more.appendChild(open);
    }
    const keyboardHelp = document.getElementById("keyboard-help");
    if (keyboardHelp && keyboardHelp.dataset.bound !== "true") {
      keyboardHelp.dataset.bound = "true";
      keyboardHelp.addEventListener("click", openKeyboardCheatSheet);
    }
    const keyboardClose = document.getElementById("keyboard-cheat-sheet-close");
    if (keyboardClose && keyboardClose.dataset.bound !== "true") {
      keyboardClose.dataset.bound = "true";
      keyboardClose.addEventListener("click", closeKeyboardCheatSheet);
    }
    const keyboardDialog = document.getElementById("keyboard-cheat-sheet");
    if (keyboardDialog && keyboardDialog.dataset.bound !== "true") {
      keyboardDialog.dataset.bound = "true";
      keyboardDialog.addEventListener("cancel", (event) => {
        event.preventDefault();
        closeKeyboardCheatSheet();
      });
    }
    const newFunction = document.getElementById("canvas-new-function");
    if (newFunction && newFunction.dataset.bound !== "true") {
      newFunction.dataset.bound = "true";
      newFunction.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        createCanvasFunction();
      });
    }
    const newCallback = document.getElementById("canvas-new-callback");
    if (newCallback && newCallback.dataset.bound !== "true") {
      newCallback.dataset.bound = "true";
      newCallback.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        createCanvasCallback();
      });
    }
    const addVariable = document.getElementById("canvas-add-variable");
    if (addVariable && addVariable.dataset.bound !== "true") {
      addVariable.dataset.bound = "true";
      addVariable.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        addCanvasVariable();
      });
    }
  }

  function setSaveState(label, kind = "saved") {
    const saved = document.getElementById("save-state");
    if (!saved) return;
    const displayLabel = {
      "source saved": "Source Saved",
      "source unchanged": "Source Unchanged",
      "source unavailable": "Source Unavailable",
      "local draft": "Local Draft",
      "project saved": "Project Saved",
      "project unchanged": "Project Unchanged",
      saving: "Saving",
      loading: "Loading"
    }[label] || label;
    saved.textContent = displayLabel;
    saved.style.color = kind === "error" ? "#fecaca" : kind === "draft" ? "#fde68a" : "#8fb2dc";
    saved.dataset.state = kind;
  }

  function setCanvasState(kind, title, detail, actions = []) {
    if (window.__jetCanvasNoopCanvasState) return;
    ensureCanvasChrome();
    const state = document.getElementById("canvas-state");
    const titleEl = document.getElementById("canvas-state-title");
    const detailEl = document.getElementById("canvas-state-detail");
    const actionsEl = document.getElementById("canvas-state-actions");
    if (!state || !titleEl || !detailEl || !actionsEl) return;
    const stateActions = actions.length ? actions : [
      { label: "Open Source", run: openSourceRecovery },
      { label: "Retry", primary: true, run: () => typeof loadGraph === "function" && loadGraph() }
    ];
    state.dataset.state = kind || "info";
    titleEl.textContent = title || "Canvas";
    detailEl.textContent = detail || "";
    actionsEl.innerHTML = "";
    state.setAttribute("aria-hidden", "false");
    const actionLabels = {
      "Open source": "Open Source",
      "Show source": "Show Source",
      "Try again": "Try Again",
      "Close preview": "Close Preview"
    };
    for (const action of stateActions) {
      const displayLabel = actionLabels[action.label] || action.label;
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = displayLabel;
      button.setAttribute("aria-label", displayLabel);
      button.dataset.canvasStateAction = displayLabel;
      button.style.cssText = "min-height:27px;padding:0 8px";
      if (action.primary) button.classList.add("primary");
      button.addEventListener("click", () => action.run && action.run());
      actionsEl.appendChild(button);
    }
    state.style.display = "grid";
    const snapshot = { kind: kind || "info", title: title || "Canvas", detail: detail || "", actions: stateActions.map((action) => actionLabels[action.label] || action.label) };
    const history = window.__jetCanvasCanvasStateHistory || [];
    history.push(snapshot);
    if (history.length > 32) history.shift();
    window.__jetCanvasCanvasStateHistory = history;
    window.__jetCanvasCanvasState = snapshot;
  }

  function clearCanvasState() {
    const state = document.getElementById("canvas-state");
    if (!state) return;
    state.style.display = "none";
    state.dataset.state = "";
    state.setAttribute("aria-hidden", "true");
    window.__jetCanvasCanvasState = null;
  }

  const TOUR_STEPS = [
    { title: "Read the graph", detail: "Files, functions, and variables live in the left rail. Select a node to inspect its source-backed details. The graph is a view of Jet source, not a second file.", target: "left-drawer" },
    { title: "Edit, Then Save", detail: "Canvas selects the example value in Inspector. Change it, then Apply; the checked, formatted result is written to Jet source immediately.", action: "Open Example Editor", target: "details" },
    { title: "Check and Fix", detail: "Check runs Jet diagnostics. Problems stay in the panel with what, why, and fix text. Rejected edits leave the last valid source intact.", action: "Check Source", target: "check-current" },
    { title: "Run and Inspect", detail: "Run opens a command card with its authority. Execute it there; the Run HUD and Details panel show the real receipt and output.", action: "Prepare Run", target: "run-current", capability: "runtime_output" },
    { title: "Undo and Keep Control", detail: "Undo restores exact validated source. Reload reprojects from disk. Canvas saves source edits; local layout and tour state stay separate.", action: "Undo Last Edit", target: "undo-edit" }
  ];
  let tourStep = 0;

  function availableTourSteps() {
    return TOUR_STEPS.filter((step) => !step.capability || canvasCapability(step.capability));
  }

  function clearTourTarget() {
    document.querySelectorAll(".canvas-tour-target").forEach((element) => {
      element.classList.remove("canvas-tour-target");
    });
  }

  function prepareTourEdit() {
    if (!latestDoc) return;
    const graph = (latestDoc.graphs || []).find((candidate) => candidate.title === "run") || currentGraph(latestDoc);
    const expr = graph && (graph.inline_exprs || []).find((candidate) => String(candidate.source || "").trim() === "4");
    const node = graph && expr && (graph.nodes || []).find((candidate) => candidate.node_id === expr.node_id);
    if (!graph || !expr || !node) {
      setViewMode("split");
      showToast("Open Inspector on the example value to edit it");
      return;
    }
    detailToggles.types = true;
    syncDetailToggles();
    if (selectedGraphId !== graph.graph_id) switchGraph(graph.graph_id, { nodeId: node.node_id });
    else {
      selectedVariableName = null;
      selectedNodeId = node.node_id;
      selectedNodeIds = new Set([node.node_id]);
      drawGraph(latestDoc);
    }
    setDrawer("details");
    window.requestAnimationFrame(() => {
      const field = Array.from(details.querySelectorAll("[data-inline-id]")).find((candidate) => candidate.getAttribute("data-inline-id") === expr.inline_expr_id);
      if (field) {
        field.focus();
        if (typeof field.select === "function") field.select();
      }
    });
  }

  function renderTour() {
    const tour = document.getElementById("first-run-tour");
    if (!tour) return;
    const steps = availableTourSteps();
    const dismissed = !!editorState.tourDismissed;
    tourStep = Math.max(0, Math.min(steps.length - 1, Number(editorState.tourStep) || 0));
    const step = steps[tourStep];
    clearTourTarget();
    const progress = document.getElementById("tour-progress");
    const title = document.getElementById("tour-title");
    const detail = document.getElementById("tour-detail");
    const action = document.getElementById("tour-action");
    const back = document.getElementById("tour-back");
    const next = document.getElementById("tour-next");
    if (!step || !progress || !title || !detail || !action || !back || !next) return;
    progress.textContent = `Step ${tourStep + 1} of ${steps.length}`;
    title.textContent = step.title;
    detail.textContent = step.detail;
    action.textContent = step.action || "";
    action.style.display = step.action ? "inline-block" : "none";
    back.disabled = tourStep === 0;
    next.textContent = tourStep === steps.length - 1 ? "Done" : "Next";
    next.disabled = tourStep === steps.length - 1;
    tour.dataset.tourStep = String(tourStep);
    tour.setAttribute("aria-hidden", dismissed ? "true" : "false");
    tour.classList.toggle("is-open", !dismissed);
    if (!dismissed && step.target) {
      const target = document.getElementById(step.target);
      if (target) target.classList.add("canvas-tour-target");
    }
    window.__jetCanvasTourState = { step: tourStep, total: steps.length, title: step.title, target: step.target, dismissed };
  }

  function finishTour() {
    editorState.tourDismissed = true;
    saveEditorState();
    renderTour();
  }

  function startTour() {
    editorState.tourDismissed = false;
    editorState.tourStep = 0;
    tourStep = 0;
    saveEditorState();
    renderTour();
  }

  function nextTourStep() {
    const steps = availableTourSteps();
    if (tourStep >= steps.length - 1) return;
    tourStep += 1;
    editorState.tourStep = tourStep;
    saveEditorState();
    renderTour();
  }

  function previousTourStep() {
    if (tourStep <= 0) return;
    tourStep -= 1;
    editorState.tourStep = tourStep;
    saveEditorState();
    renderTour();
  }

  function runTourAction() {
    const step = availableTourSteps()[tourStep];
    if (!step) return;
    if (step.action === "Open Example Editor") prepareTourEdit();
    else if (step.action === "Check Source") showCheckAuthority();
    else if (step.action === "Prepare Run") runCurrentGraph();
    else if (step.action === "Undo Last Edit") {
      const restore = undoTransaction();
      if (restore && typeof restore.then === "function") restore.then(renderTour);
    }
    renderTour();
  }

  ensureCanvasChrome();
  window.addEventListener("offline", () => {
    setCanvasState("offline", "Offline", "Canvas cannot write while disconnected. Jet source stays visible; reconnect, then retry.", [
      { label: "Show Source", run: openSourceRecovery },
      { label: "Retry", primary: true, run: loadGraph }
    ]);
    setSaveState("Source Unchanged", "error");
  });
  window.addEventListener("online", () => {
    const state = window.__jetCanvasCanvasState;
    if (state && state.kind === "offline") loadGraph();
  });

  function richestGraph(doc) {
    if (!doc.graphs || doc.graphs.length === 0) return null;
    return doc.graphs.slice().sort((a, b) => b.nodes.length - a.nodes.length || a.title.localeCompare(b.title))[0];
  }

  let graphPickerKey = null;
  let graphListKey = null;
  let graphStripKey = null;
  let variablesListKey = null;
  let traitsPanelKey = null;
  let libraryPanelKey = null;
  let librarySearchQuery = "";
  let eventsPanelKey = null;

  function graphNavigationKey(doc) {
    return `${doc && doc.source_id || ""}:${doc && doc.revision || ""}:${selectedGraphId || ""}`;
  }

  function syncGraphPicker(doc) {
    const best = richestGraph(doc);
    if (!selectedGraphId && best) selectedGraphId = best.graph_id;
    const key = graphNavigationKey(doc);
    if (graphPickerKey === key) return;
    graphPickerKey = key;
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
    const key = graphNavigationKey(doc);
    if (graphListKey === key) return;
    graphListKey = key;
    graphList.setAttribute("role", "group");
    graphList.innerHTML = "";
    for (const graph of doc.graphs || []) {
      const button = document.createElement("button");
      const callback = callbackEventView(graph);
      button.className = "graph-item" + (graph.graph_id === selectedGraphId ? " is-active" : "");
      button.type = "button";
      button.setAttribute("role", "treeitem");
      button.setAttribute("aria-level", "2");
      button.setAttribute("aria-selected", graph.graph_id === selectedGraphId ? "true" : "false");
      button.setAttribute("data-canvas-tree-item", "function");
      button.setAttribute("data-sidebar-graph", graph.graph_id);
      if (callback) {
        button.setAttribute("data-callback-handler", callback.function);
        button.title = "Open Callback Handler: " + callback.function;
        button.innerHTML = "<span>" + escapeHtml(graph.title) + " <span class=\"tag\">Handler</span></span><span class=\"count\">" + graph.nodes.length + "</span>";
      } else {
        button.innerHTML = "<span>" + escapeHtml(graph.title) + "</span><span class=\"count\">" + graph.nodes.length + "</span>";
      }
      button.addEventListener("click", () => {
        switchGraph(graph.graph_id);
      });
      graphList.appendChild(button);
    }
  }

  function graphVariables(graph) {
    if (!graph) return [];
    const vars = new Map();
    const addVar = (name, type, source, editable, defaultSource, nodeId, meta) => {
      if (!name) return;
      const prev = vars.get(name) || {};
      const keepsDefinition = source === "input" || source === "local";
      vars.set(name, {
        name,
        type: type || prev.type || "Value",
        source: source || prev.source || "local",
        editable: editable || prev.editable || false,
        defaultSource: keepsDefinition && defaultSource ? defaultSource : prev.defaultSource || "",
        nodeId: prev.nodeId || nodeId || "",
        meta: meta || prev.meta || null
      });
    };
    for (const param of (graph.function && graph.function.params) || []) {
      addVar(param.name, param.type, "input", true, param.default_source || "", graph.entry_node);
    }
    const inlineByNode = new Map();
    for (const expr of graph.inline_exprs || []) {
      if (!inlineByNode.has(expr.node_id)) inlineByNode.set(expr.node_id, []);
      inlineByNode.get(expr.node_id).push(expr);
    }
    for (const node of graph.nodes || []) {
      const dataOut = (graph.pins || []).find((pin) => pin.node_id === node.node_id && pin.direction === "output" && !isExecPin(pin));
      if (node.kind === "binding" || node.kind === "assign" || node.kind === "variable_get") {
        const init = (inlineByNode.get(node.node_id) || []).find((expr) => expr.role === "init" || expr.role === "value");
        addVar(node.title, (dataOut && dataOut.type) || "Value", node.kind === "variable_get" ? "read" : "local", node.kind === "binding", init ? init.source : "", node.node_id, node.meta || null);
      }
    }
    return Array.from(vars.values()).sort((a, b) => (a.source === "input" ? 0 : 1) - (b.source === "input" ? 0 : 1) || a.name.localeCompare(b.name));
  }

  function enumVariantsForType(type) {
    const facts = latestDoc && latestDoc.facts;
    const variants = facts && facts.enum_variants && facts.enum_variants[type];
    return Array.isArray(variants) ? variants : [];
  }

  function patternVariantsForType(type) {
    const facts = latestDoc && latestDoc.facts;
    const variants = facts && facts.pattern_variants && facts.pattern_variants[type];
    return Array.isArray(variants) ? variants : [];
  }

  function patternArmHead(pattern) {
    let text = String(pattern || "").trim();
    if (text.startsWith("==")) text = text.slice(2).trim();
    text = text.replace(/\s*->\s*$/, "").trim();
    const match = text.match(/^\.?([A-Z][A-Za-z0-9_.]*)(?:\s*\((.*)\))?$/);
    if (!match) return null;
    const args = match[2];
    let arity = 0;
    if (args !== undefined && args.trim()) {
      let depth = 0;
      arity = 1;
      for (const char of args) {
        if ("([{".includes(char)) depth++;
        else if (")]}".includes(char)) depth--;
        else if (char === "," && depth === 0) arity++;
      }
    }
    return { name: match[1], arity };
  }

  function patternArmEditPlan(node, pattern) {
    const type = node && node.meta && node.meta.pattern_type;
    const variants = patternVariantsForType(type);
    if (!type || !variants.length) return null;
    const head = patternArmHead(pattern);
    if (!head) return null;
    const variant = variants.find((candidate) => candidate && candidate.name === head.name);
    if (!variant) {
      return { label: `Pattern arm ${head.name} is not a ${type} variant` };
    }
    if (Number.isInteger(variant.arity) && variant.arity !== head.arity) {
      return { label: `Pattern arm ${head.name} needs ${variant.arity} payload value${variant.arity === 1 ? "" : "s"}` };
    }
    return null;
  }

  function refusePatternArmEdit(plan) {
    const refusal = {
      ok: false,
      changed: false,
      reason: plan.label,
      message: plan.label,
      code: "client_pattern_gate"
    };
    window.__jetCanvasLastTx = null;
    window.__jetCanvasLastTxResult = refusal;
    showToast("Edit refused: " + plan.label, { isError: true });
    return false;
  }

  function isScalarType(type) {
    return ["Bool", "Int", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "Float", "F32", "F64", "Decimal", "String", "Char"].includes(String(type || ""));
  }

  function isReferenceExpression(source) {
    const text = String(source || "").trim();
    return /^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/.test(text)
      && !["true", "false", "Absent"].includes(text);
  }

  function detailEditorKind(type, source, graph) {
    if (isReferenceExpression(source) && graphVariables(graph).some((candidate) => candidate.type === type && candidate.name === String(source).trim())) return "reference";
    if (enumVariantsForType(type).length) return "enum";
    if (isScalarType(type) && !isReferenceExpression(source)) return "scalar";
    if (isReferenceExpression(source)) return "reference";
    return "expression";
  }

  function syncVariablesList(graph) {
    if (!variablesList) return;
    const key = `${graph && graph.graph_id || ""}:${latestDoc && latestDoc.revision || ""}:${selectedVariableName || ""}`;
    if (variablesListKey === key) return;
    variablesListKey = key;
    const vars = graphVariables(graph);
    if (variableCount) variableCount.textContent = String(vars.length);
    variablesList.setAttribute("role", "group");
    variablesList.innerHTML = vars.map((v) => {
      const color = colorForType(v.type);
      const active = selectedVariableName === v.name ? " is-active" : "";
      return `<button class="variable-item${active}" type="button" role="treeitem" aria-level="3" aria-selected="${selectedVariableName === v.name ? "true" : "false"}" data-canvas-tree-item="variable" data-variable-name="${escapeAttr(v.name)}"><span class="variable-dot" style="color:${escapeAttr(color)}"></span><span class="variable-name">${escapeHtml(v.name)}</span>${typeChipHtml(v.type)}</button>`;
    }).join("") || "<div class=\"tag\">no variables</div>";
    variablesList.querySelectorAll("[data-variable-name]").forEach((button) => {
      button.addEventListener("click", () => selectVariable(button.getAttribute("data-variable-name")));
    });
    window.__jetCanvasVariablesSidebar = vars.length;
  }

  function canvasInterfaceFacts(doc) {
    const blueprint = doc && doc.facts && doc.facts.blueprint;
    return blueprint && Array.isArray(blueprint.interfaces) ? blueprint.interfaces : [];
  }

  function traitScope(fact) {
    return Array.isArray(fact && fact.scope) ? fact.scope.join(".") : String(fact && fact.scope || "");
  }

  function traitQualifiedName(fact) {
    const scope = traitScope(fact);
    return scope ? scope + "." + fact.trait : String(fact && fact.trait || "trait");
  }

  function traitMethodRequired(method) {
    return method && method.required !== false && !method.default;
  }

  function libraryEntryTitle(action) {
    const title = String(action && action.title || action && action.callee || "member");
    const separator = title.indexOf(" · ");
    return separator >= 0 ? title.slice(0, separator) : title;
  }

  function libraryModulePath(action) {
    return String(action && action.module_path || (action && action.kind === "canvas.core_catalog" ? "core" : "project"));
  }

  function libraryEntriesFor(doc) {
    if (!doc || actionEntriesRevision !== doc.revision) return [];
    return actionEntries.filter((action) => action && (
      action.kind === "canvas.core_catalog"
      || action.kind === "canvas.action"
    ));
  }

  function libraryPackageRows(project) {
    const rows = [];
    const seen = new Set();
    for (const pkg of project && project.packages || []) {
      const key = String(pkg.name || pkg.path || "package");
      if (seen.has(key)) continue;
      seen.add(key);
      rows.push({
        name: key,
        detail: [pkg.path, pkg.version].filter(Boolean).join(" · ") || "package facts",
        members: Array.isArray(pkg.members) ? pkg.members.length : 0,
        dependencies: Array.isArray(pkg.deps) ? pkg.deps.length : 0
      });
    }
    return rows;
  }

  function syncLibraryPanel(doc) {
    const canvasPanel = document.getElementById("canvas-panel");
    if (!canvasPanel || !doc) return;
    const key = `${doc.source_id || ""}:${doc.revision || ""}:${actionEntriesRevision || ""}:${actionEntries.length}:${librarySearchQuery}`;
    if (libraryPanelKey === key) return;
    libraryPanelKey = key;
    let panel = canvasPanel.querySelector("[data-canvas-library]");
    if (!panel) {
      panel = document.createElement("div");
      panel.setAttribute("data-canvas-library", "true");
      canvasPanel.appendChild(panel);
    }
    clearDom(panel);

    const section = document.createElement("section");
    section.className = "project-section library-panel";
    const head = appendText(section, "div", "lane-head", "");
    appendText(head, "h3", "", "Library");
    const entries = libraryEntriesFor(doc);
    const query = librarySearchQuery.trim();
    const matches = query ? entries.filter((action) => actionMatchesQuery(action, query)) : entries;
    const count = appendText(head, "span", "lane-meta", entries.length ? `${matches.length}/${entries.length}` : "loading");
    count.setAttribute("data-library-count", "true");

    const search = document.createElement("input");
    search.type = "search";
    search.className = "search library-search";
    search.placeholder = "Search modules and members";
    search.value = librarySearchQuery;
    search.setAttribute("data-library-search", "true");
    search.setAttribute("aria-label", "Search library modules and members");
    search.addEventListener("input", () => {
      const cursor = search.selectionStart;
      librarySearchQuery = search.value;
      libraryPanelKey = null;
      syncLibraryPanel(latestDoc);
      const next = panel.querySelector("[data-library-search]");
      if (next) {
        next.focus();
        if (Number.isFinite(cursor)) next.setSelectionRange(cursor, cursor);
      }
    });
    section.appendChild(search);

    const description = appendText(
      section,
      "div",
      "tag",
      entries.length
        ? "Checked Core modules and ordinary Jet project functions. Select an entry to insert source."
        : (actionEntriesRevision === doc.revision ? "No source-backed library entries are available." : "Refreshing the library from the current checked source…")
    );
    description.setAttribute("data-library-status", "true");

    const list = document.createElement("div");
    list.className = "library-list";
    section.appendChild(list);
    const groups = new Map();
    for (const action of matches) {
      const modulePath = libraryModulePath(action);
      if (!groups.has(modulePath)) groups.set(modulePath, []);
      groups.get(modulePath).push(action);
    }
    const modulePaths = Array.from(groups.keys()).sort((left, right) => left.localeCompare(right));
    modulePaths.forEach((modulePath, moduleIndex) => {
      const module = document.createElement("details");
      module.className = "library-module";
      module.setAttribute("data-library-module", modulePath);
      module.open = !!query || moduleIndex === 0;
      const summary = document.createElement("summary");
      appendText(summary, "span", "library-module-name", modulePath);
      appendText(summary, "span", "count", String(groups.get(modulePath).length));
      module.appendChild(summary);
      const body = document.createElement("div");
      body.className = "library-module-body";
      const moduleSummary = groups.get(modulePath).find((action) => action.summary);
      if (moduleSummary) appendText(body, "p", "library-module-summary", moduleSummary.summary);
      for (const action of groups.get(modulePath).sort((left, right) => libraryEntryTitle(left).localeCompare(libraryEntryTitle(right)))) {
        const availability = actionAvailability(action, currentGraphOrNull());
        const row = document.createElement("div");
        row.className = "library-entry-row";
        row.setAttribute("data-library-entry", action.action_id || action.callee || libraryEntryTitle(action));
        const button = document.createElement("button");
        button.type = "button";
        button.className = "library-entry";
        button.setAttribute("data-library-action", action.action_id || action.callee || libraryEntryTitle(action));
        button.setAttribute("data-library-module", modulePath);
        button.disabled = !availability.available;
        button.title = availability.available
          ? (action.stageable ? (action.stage_reason || "Stage until a compatible input is connected.") : "Insert checked Jet source")
          : availability.reason;
        appendText(button, "span", "library-entry-name", libraryEntryTitle(action));
        appendText(button, "code", "library-entry-signature", action.signature || action.insert_callee || action.callee || "member");
        button.addEventListener("click", () => runLibraryAction(action));
        row.appendChild(button);
        const meta = appendText(row, "div", "library-entry-meta", "");
        appendText(meta, "span", "library-entry-state", action.stageable ? "staged until wired" : availability.available ? "checked source" : "unavailable");
        const inputPins = (action.pins || []).filter((pin) => pin.direction === "input").map((pin) => `${pin.name || "arg"}: ${pin.type || "Value"}`).join(", ");
        if (inputPins) appendText(meta, "small", "library-entry-pins", inputPins);
        appendText(meta, "small", "library-entry-source", action.source || (action.kind === "canvas.core_catalog" ? "docs/reference/core-library.md" : modulePath));
        if (!availability.available) appendText(meta, "small", "library-entry-reason", availability.reason);
        body.appendChild(row);
      }
      module.appendChild(body);
      list.appendChild(module);
    });
    if (!modulePaths.length) appendText(list, "div", "tag", query ? "No library entries match this search." : "Library entries will appear after the checked action query completes.");
    section.appendChild(list);

    const packages = libraryPackageRows(latestProject);
    if (packages.length) {
      const packageSection = document.createElement("section");
      packageSection.className = "library-packages";
      const packageHead = appendText(packageSection, "div", "lane-head", "");
      appendText(packageHead, "h4", "", "Packages");
      appendText(packageHead, "span", "lane-meta", String(packages.length));
      for (const pkg of packages) {
        const packageRow = appendText(packageSection, "div", "library-package", "");
        appendText(packageRow, "b", "", pkg.name);
        appendText(packageRow, "small", "", `${pkg.detail} · ${pkg.members} modules · ${pkg.dependencies} deps`);
      }
      section.appendChild(packageSection);
    }
    panel.appendChild(section);

    const moduleState = modulePaths.map((modulePath) => ({
      path: modulePath,
      entries: groups.get(modulePath).map((action) => ({
        action_id: action.action_id || "",
        title: libraryEntryTitle(action),
        signature: action.signature || action.insert_callee || action.callee || "",
        source: action.source || "",
        source_span: action.source_span || null,
        pins: action.pins || [],
        available: actionAvailability(action, currentGraphOrNull()).available,
        stageable: !!action.stageable
      }))
    }));
    window.__jetCanvasLibraryPanel = {
      rendered: true,
      source: "canvas.query.actions",
      revision: doc.revision || "",
      query,
      moduleCount: modulePaths.length,
      actionCount: matches.length,
      totalActionCount: entries.length,
      availableCount: matches.filter((action) => actionAvailability(action, currentGraphOrNull()).available).length,
      stagedCount: matches.filter((action) => action.stageable).length,
      unavailableCount: matches.filter((action) => !actionAvailability(action, currentGraphOrNull()).available).length,
      modules: moduleState,
      packages: packages.length
    };
  }

  function syncTraitsPanel(doc) {
    const canvasPanel = document.getElementById("canvas-panel");
    if (!canvasPanel) return;
    const key = `${doc && doc.source_id || ""}:${doc && doc.revision || ""}`;
    if (traitsPanelKey === key) return;
    traitsPanelKey = key;
    let panel = canvasPanel.querySelector("[data-canvas-traits]");
    if (!panel) {
      panel = document.createElement("div");
      panel.setAttribute("data-canvas-traits", "true");
      canvasPanel.appendChild(panel);
    }

    const facts = canvasInterfaceFacts(doc);
    const traits = facts.filter((fact) => fact && fact.kind === "trait_interface");
    const implementations = facts.filter((fact) => fact && fact.kind === "trait_impl");
    const jumps = [];
    const jumpButton = (span, label) => {
      if (!span || !Number.isFinite(span.start)) return "";
      const index = jumps.push({ span, label }) - 1;
        return `<button type="button" data-trait-jump="${index}">${escapeHtml(label || "Open Source")}</button>`;
    };
    const implsFor = (trait) => implementations.filter((impl) => {
      return impl.trait === trait.trait && traitScope(impl) === traitScope(trait);
    });
    const stateTraits = [];
    let requiredMethodCount = 0;
    let implementedMethodCount = 0;
    const traitRows = traits.map((trait, traitIndex) => {
      const methods = Array.isArray(trait.methods) ? trait.methods : [];
      const associatedTypes = Array.isArray(trait.associated_types) ? trait.associated_types : [];
      const requiredMethods = methods.filter(traitMethodRequired);
      requiredMethodCount += requiredMethods.length;
      const impls = implsFor(trait);
      const implRows = impls.map((impl) => {
        const methodNames = new Set((impl.methods || []).map((name) => String(name)));
        const implemented = requiredMethods.filter((method) => methodNames.has(method.name));
        implementedMethodCount += implemented.length;
        const methodStatus = requiredMethods.map((method) => {
          const present = methodNames.has(method.name);
          return `<div class="pin-row"><div class="lane-head"><b>${escapeHtml(method.name)}</b><span class="tag">${present ? "Implemented" : "Required"}</span></div><code>${escapeHtml(method.signature || "")}</code></div>`;
        }).join("");
        const assocStatus = associatedTypes.map((name) => {
          const present = (impl.associated_types || []).includes(name);
          return `<div class="pin-row"><div class="lane-head"><b>type ${escapeHtml(name)}</b><span class="tag">${present ? "Implemented" : "Required"}</span></div></div>`;
        }).join("");
        return `<div class="signature-board" data-trait-implementation="${escapeAttr(String(impl.type || ""))}"><div class="signature-head"><div><span class="sig-eyebrow">Implementation</span><b>${escapeHtml(impl.type || "Type")}</b><code>${escapeHtml(traitQualifiedName(trait))}</code></div>${jumpButton(impl.source_span, "Open Source")}</div><div class="lane-meta">${implemented.length}/${requiredMethods.length} required methods</div><div class="pin-list">${methodStatus || "<div class=\"pin-empty\">No required methods</div>"}${assocStatus}</div></div>`;
      }).join("");
      const methodRows = methods.map((method) => {
        const label = traitMethodRequired(method) ? "Required" : "Default";
        return `<div class="pin-row"><div class="lane-head"><b>${escapeHtml(method.name || "method")}</b><span class="tag">${label}</span></div><code>${escapeHtml(method.signature || "")}</code>${jumpButton(method.source_span, "Open Source")}</div>`;
      }).join("");
      const assocRows = associatedTypes.length
        ? `<div class="tag">Associated Types: ${escapeHtml(associatedTypes.join(", "))}</div>`
        : "";
      const create = associatedTypes.length
        ? `<div class="tag">Add associated type choices in source before creating an implementation.</div>`
        : `<div class="edit-grid"><label><span>Type Name</span><input data-trait-type="${traitIndex}" placeholder="${escapeAttr(traitScope(trait) ? "Type in " + traitScope(trait) : "Type name")}"></label><button class="primary" type="button" data-trait-create="${traitIndex}">Add Implementation</button></div>`;
      stateTraits.push({
        name: trait.trait,
        scope: traitScope(trait),
        requiredMethods: requiredMethods.map((method) => method.name),
        methods: methods.map((method) => method.name),
        implementations: impls.map((impl) => ({ type: impl.type, methods: (impl.methods || []).slice() }))
      });
      return `<div class="signature-board" data-trait="${escapeAttr(traitQualifiedName(trait))}"><div class="signature-head"><div><span class="sig-eyebrow">Trait</span><b>${escapeHtml(trait.trait || "Trait")}</b><code>${escapeHtml(traitScope(trait) || "file scope")}</code></div>${jumpButton(trait.source_span, "Open Source")}</div><div class="lane-head"><b>Methods</b><span class="lane-meta">${requiredMethods.length} required · ${methods.length - requiredMethods.length} default</span></div><div class="pin-list">${methodRows || "<div class=\"pin-empty\">No methods</div>"}</div>${assocRows}${implRows || "<div class=\"pin-empty\">No implementation</div>"}${create}</div>`;
    }).join("");

    panel.innerHTML = `<section class="project-section"><div class="lane-head"><h3>Traits</h3><span class="lane-meta">${traits.length}</span></div>${traitRows || "<div class=\"tag\">no traits</div>"}</section>`;
    panel.querySelectorAll("[data-trait-jump]").forEach((button) => {
      button.addEventListener("click", () => {
        const target = jumps[Number(button.getAttribute("data-trait-jump"))];
        if (!target) return;
        setSourceHash(target.span);
        setViewMode("code");
        showToast(target.label + " selected");
      });
    });
    panel.querySelectorAll("[data-trait-create]").forEach((button) => {
      button.addEventListener("click", () => {
        const index = button.getAttribute("data-trait-create");
        const trait = traits[Number(index)];
        const input = panel.querySelector(`[data-trait-type="${cssEscape(index)}"]`);
        const typeName = input ? input.value.trim() : "";
        if (!trait || !/^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/.test(typeName)) {
          showToast("Type name must be a Jet name", { isError: true });
          return;
        }
        postTransaction({
          schema_version: 1,
          op: "create_trait_impl",
          revision: latestDoc.revision,
          type_name: typeName,
          trait_name: traitQualifiedName(trait)
        });
      });
    });
    window.__jetCanvasTraitsPanel = {
      rendered: true,
      traitCount: traits.length,
      implementationCount: implementations.length,
      requiredMethodCount,
      implementedMethodCount,
      traits: stateTraits,
      revision: doc && doc.revision || ""
    };
  }

  function eventFactLabel(fact) {
    return String(fact && fact.kind || "event fact")
      .replace(/_/g, " ")
      .replace(/\b\w/g, (letter) => letter.toUpperCase());
  }

  function syncEventsPanel(doc) {
    const canvasPanel = document.getElementById("canvas-panel");
    if (!canvasPanel) return;
    const key = `${doc && doc.source_id || ""}:${doc && doc.revision || ""}`;
    if (eventsPanelKey === key) return;
    eventsPanelKey = key;
    let panel = canvasPanel.querySelector("[data-canvas-events]");
    if (!panel) {
      panel = document.createElement("div");
      panel.setAttribute("data-canvas-events", "true");
      canvasPanel.appendChild(panel);
    }
    const blueprint = doc && doc.facts && doc.facts.blueprint;
    const dispatchers = Array.isArray(blueprint && blueprint.event_dispatchers)
      ? blueprint.event_dispatchers.filter((fact) => fact && fact.kind)
      : [];
    const jumps = [];
    const jumpButton = (span, label) => {
      if (!span || !Number.isFinite(span.start)) return "";
      const index = jumps.push({ span, label }) - 1;
      return `<button type="button" data-event-jump="${index}">${escapeHtml(label || "Open Source")}</button>`;
    };
    const stateEvents = dispatchers.map((fact) => ({
      kind: fact.kind,
      source: fact.source || "",
      receiver: fact.receiver || "",
      receiverType: fact.receiver_type || "",
      scope: fact.scope || "",
      sourceSpan: fact.source_span || null,
      factSource: fact.fact_source || ""
    }));
    const rows = dispatchers.map((fact) => {
      const type = fact.receiver_type || (fact.kind.endsWith("create") ? "new event value" : "checked event call");
      const scope = fact.scope ? `scope ${fact.scope}` : "scope from source";
      return `<div class="signature-board" data-event-kind="${escapeAttr(fact.kind)}"><div class="signature-head"><div><span class="sig-eyebrow">${escapeHtml(eventFactLabel(fact))}</span><b>${escapeHtml(fact.receiver || fact.kind)}</b><code>${escapeHtml(type)}</code></div>${jumpButton(fact.source_span, "Open Source")}</div><div class="lane-meta">${escapeHtml(scope)} · source-backed · ${escapeHtml(fact.lifetime || "EventScope-owned")}</div><code>${escapeHtml(fact.source || "")}</code></div>`;
    }).join("");
    panel.innerHTML = `<section class="project-section"><div class="lane-head"><h3>Events</h3><span class="lane-meta">${dispatchers.length}</span></div><div class="edit-grid"><button type="button" data-event-actions>Open Event Actions</button></div>${rows || "<div class=\"tag\">no core.event calls</div>"}</section>`;
    panel.querySelectorAll("[data-event-jump]").forEach((button) => {
      button.addEventListener("click", () => {
        const target = jumps[Number(button.getAttribute("data-event-jump"))];
        if (!target) return;
        setSourceHash(target.span);
        setViewMode("code");
        showToast(target.label + " selected");
      });
    });
    panel.querySelectorAll("[data-event-actions]").forEach((button) => {
      button.addEventListener("click", () => {
        openGraphActionPalette(window.innerWidth / 2 - 210, 72, "core.event");
      });
    });
    window.__jetCanvasEventsPanel = {
      rendered: true,
      dispatcherCount: dispatchers.length,
      eventCount: new Set(dispatchers.map((fact) => fact.receiver || fact.kind)).size,
      events: stateEvents,
      revision: doc && doc.revision || ""
    };
  }

  function projectMiniCard(title, small, code, apply_op, className) {
    return {
      label: String(title || ""),
      value: String(small || ""),
      detail: String(code || ""),
      editable: false,
      apply_op: apply_op || null,
      layout: "card",
      className: className || "project-card"
    };
  }

  function syncProjectPanel(panel, title, rows, empty) {
    if (!panel) return;
    clearDom(panel);
    appendText(panel, "h3", "", title);
    if (rows.length) renderFieldDescriptors(panel, rows, { fieldsClass: "project-list" });
    else appendText(panel, "div", "tag", empty);
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
    const capabilities = project.capabilities || {};
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
      const run = (svc.run || []).join(" ") || "catalog/default";
      devRows.push(projectMiniCard(svc.name || "service", svc.enable === false ? "disabled" : "enabled", `${ports} · ${run}`));
    }
    const diagRows = collectProjectDiagnostics(project);
    const lockRows = (project.locks || []).map((lock) => projectMiniCard(lock.path || ".jet/lock", lock.kind || "lock", lock.revision || ""));
    const policy = project.state_policy || {};
    lockRows.unshift(projectMiniCard("Source files", project.source_control && project.source_control.truth || "git text", `${policy.semantic || "source"} model · ${(policy.local || []).join(", ") || "local viewport"}`));
    syncProjectPanel(packageSummary, "Packages", packageRows, "no packages");
    syncProjectPanel(dependencySummary, "Dependencies", depRows, "no dependencies");
    const hasDevCapability = capabilities.service === true || (project.envs || []).length > 0;
    if (devSummary) devSummary.hidden = !hasDevCapability;
    if (hasDevCapability) syncProjectPanel(devSummary, "Dev", devRows, "no env or services");
    syncProjectPanel(diagnosticsSummary, "Diagnostics", diagRows, "clean");
    syncProjectPanel(trustSummary, "Source Internals", lockRows, "source files only");
    if (typeof syncCanvasCapabilities === "function") syncCanvasCapabilities(project);
    if (statusSummary) {
      const packageName = (project.packages || [])[0] && ((project.packages || [])[0].name || (project.packages || [])[0].path);
      const sourceFiles = (project.files || []).filter((f) => f.kind === "source").length;
      const diagCount = diagRows.length;
      clearDom(statusSummary);
      renderFieldDescriptors(statusSummary, [
        { label: packageName || project.mode || "Single file", value: sourceFiles + " source file" + (sourceFiles === 1 ? "" : "s"), detail: "", editable: false, layout: "card", className: "status-card" },
        { label: diagCount === 0 ? "Clean" : diagCount + " issue" + (diagCount === 1 ? "" : "s"), value: (project.mode || "file") + " mode", detail: "", editable: false, layout: "card", className: "status-card" }
      ], { fieldsClass: "status-list" });
      if (statusCount) statusCount.textContent = diagCount === 0 ? "Clean" : String(diagCount);
    }
    window.__jetCanvasWorkspacePanels = {
      packages: packageRows.length,
      dependencies: depRows.length,
      dev: devRows.length,
      diagnostics: diagRows.length,
      trust: lockRows.length,
      capabilities
    };
  }

  function syncProjectRail(project) {
    if (!projectRail || !project) return;
    latestProject = project;
    libraryPanelKey = null;
    if (latestDoc) syncLibraryPanel(latestDoc);
    const sourceFiles = (project.files || []).filter((f) => f.kind === "source");
    projectMode.textContent = `${project.mode || "file"} · ${sourceFiles.length} source files`;
    const cards = [];
    const fileCount = (project.files || []).length;
    for (const file of sourceFiles) {
      const active = (selectedSourceId || project.entry) === file.path;
      cards.push(projectMiniCard(
        file.path || "source",
        active ? "open" : "click to open",
        file.revision || "",
        { id: "project-file:" + (file.path || "source"), mode: "action", run: () => loadGraph(file.path) },
        "project-card" + (active ? " is-active" : "")
      ));
      cards[cards.length - 1].buttonAttributes = { "data-project-file": file.path || "" };
    }
    if (!cards.length) cards.push(projectMiniCard(project.entry || "source", "open", "", null, "project-card is-active"));
    clearDom(projectRail);
    renderFieldDescriptors(projectRail, cards, { fieldsClass: "project-list" });
    projectRail.setAttribute("role", "group");
    projectRail.querySelectorAll("[data-project-file]").forEach((button) => {
      button.setAttribute("role", "treeitem");
      button.setAttribute("aria-level", "1");
      button.setAttribute("aria-selected", button.getAttribute("data-project-file") === (selectedSourceId || project.entry) ? "true" : "false");
      button.setAttribute("data-canvas-tree-item", "file");
    });
    syncProjectPanels(project);
    if (typeof syncCanvasOutputs === "function") syncCanvasOutputs(project);
    if (typeof syncCanvasServers === "function") syncCanvasServers(project, canvasSession);
    if (typeof syncCanvasSession === "function" && canvasSession) syncCanvasSession(canvasSession);
    window.__jetCanvasProjectRail = { mode: project.mode, packages: (project.packages || []).length, files: fileCount, panels: window.__jetCanvasWorkspacePanels };
  }

  function syncComponentTree(doc, graph) {
    if (!componentTree || !doc) return;
    componentTree.dataset.sourceId = doc.source_id || "";
    componentTree.dataset.revision = doc.revision || "";
    componentTree.dataset.graphId = graph && graph.graph_id || "";
    componentTree.setAttribute("aria-label", `My Canvas · ${graph && graph.title || "source"}`);
    const items = Array.from(componentTree.querySelectorAll("[data-canvas-tree-item]"));
    for (const item of items) {
      item.setAttribute("data-canvas-source-id", doc.source_id || "");
      item.setAttribute("data-canvas-revision", doc.revision || "");
      item.setAttribute("data-canvas-source-backed", "true");
    }
    window.__jetCanvasComponentTree = {
      rendered: true,
      sourceId: doc.source_id || "",
      revision: doc.revision || "",
      graphId: graph && graph.graph_id || "",
      graphTitle: graph && graph.title || "",
      files: componentTree.querySelectorAll('[data-canvas-tree-item="file"]').length,
      functions: componentTree.querySelectorAll('[data-canvas-tree-item="function"]').length,
      variables: componentTree.querySelectorAll('[data-canvas-tree-item="variable"]').length,
      itemCount: items.length,
      sourceBacked: true
    };
  }

  let projectLoadGeneration = 0;
  function loadProject() {
    const loadToken = ++projectLoadGeneration;
    const requestedSourceId = currentCanvasSourceId();
    const projectUrl = window.__JET_CANVAS_PROJECT__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/project");
    return fetch(projectUrl, { cache: "no-store" })
      .then((r) => {
        if (!r.ok) throw new Error("project request failed (" + r.status + ")");
        return r.json().then(canvasPayload);
      })
      .then((project) => {
        if (loadToken !== projectLoadGeneration || currentCanvasSourceId() !== requestedSourceId) return latestProject;
        syncProjectRail(project);
        if (latestDoc) syncComponentTree(latestDoc, currentGraphOrNull());
        return project;
      })
      .catch(() => {
        if (loadToken !== projectLoadGeneration) return latestProject;
        if (latestProject) {
          if (projectMode) projectMode.textContent = "project unavailable · showing last project";
          return latestProject;
        }
        if (projectRail) {
          clearDom(projectRail);
          appendText(projectRail, "div", "tag", "project unavailable");
        }
        return null;
      });
  }

  function syncWireStatus(state) {
    if (!wireStatus) return;
    let title = "Ready";
    let detail = "Hover a node or pin for details";
    let color = "#7dd3fc";
    if (state) {
      title = state.title || title;
      detail = state.detail || detail;
      color = state.color || color;
    } else if (hoverPin) {
      title = "Pin";
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
        node_descriptor_id: "variable_get",
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
        title: "Set " + name,
        detail: name + " : " + type,
        group: "Variables",
        kind: "variable_set",
        node_descriptor_id: "assignment",
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

  function spanStart(span) {
    return span && Number.isFinite(span.start) ? span.start : 0;
  }

  function spanEnd(span) {
    return span && Number.isFinite(span.end) ? span.end : spanStart(span);
  }

  function defaultElementForType(type) {
    const t = String(type || "");
    if (t === "String" || t === "Path" || t === "Url") return "\"canvas\"";
    if (t === "Bool") return "true";
    if (["Float", "F32", "F64", "Decimal"].includes(t)) return "1.0";
    if (t.endsWith("?")) return "Absent";
    return "1";
  }

  function firstDataInputForNode(graph, node) {
    return (graph && graph.pins || []).find((p) => p.node_id === node.node_id && p.direction === "input" && !isExecPin(p));
  }

  function addPatternArm(node) {
    if (!latestDoc || !node) return;
    const pattern = window.prompt("Pattern arm", "== Variant(x)");
    if (pattern === null || pattern.trim() === "") return;
    const plan = patternArmEditPlan(node, pattern);
    if (plan) return refusePatternArmEdit(plan);
    postTransaction({
      schema_version: 1,
      op: "add_pattern_arm",
      revision: latestDoc.revision,
      graph_id: selectedGraphId,
      node_start: spanStart(node.source_span),
      node_end: spanEnd(node.source_span),
      pattern
    });
  }

  function editPatternArm(pin) {
    if (!latestDoc || !pin) return;
    const pattern = window.prompt("Pattern arm", pin.pattern_source || "== Variant(x)");
    if (pattern === null || pattern.trim() === "") return;
    const graph = currentGraphOrNull();
    const node = graph && (graph.nodes || []).find((candidate) => candidate.node_id === pin.node_id);
    const plan = patternArmEditPlan(node, pattern);
    if (plan) return refusePatternArmEdit(plan);
    const span = pin.pattern_source_span || pin.source_span || {};
    postTransaction({
      schema_version: 1,
      op: "edit_pattern_arm",
      revision: latestDoc.revision,
      graph_id: selectedGraphId,
      pattern_start: spanStart(span),
      pattern_end: spanEnd(span),
      pattern
    });
  }

  function removePatternArm(pin) {
    if (!latestDoc || !pin) return;
    const span = pin.pattern_source_span || pin.source_span || {};
    postTransaction({
      schema_version: 1,
      op: "remove_pattern_arm",
      revision: latestDoc.revision,
      graph_id: selectedGraphId,
      pattern_start: spanStart(span),
      pattern_end: spanEnd(span)
    });
  }

  function toggleSwitchState(node) {
    if (!latestDoc || !node || !node.source_span) return;
    postTransaction({
      schema_version: 1,
      op: "toggle_switch_state",
      revision: latestDoc.revision,
      graph_id: selectedGraphId,
      node_start: spanStart(node.source_span),
      node_end: spanEnd(node.source_span)
    });
  }

  function appendMultiInput(node) {
    if (!latestDoc || !node) return;
    const graph = currentGraphOrNull();
    const input = firstDataInputForNode(graph, node);
    const element = window.prompt("Element", defaultElementForType(input && input.type));
    if (element === null || element.trim() === "") return;
    postTransaction({
      schema_version: 1,
      op: "append_multi_input",
      revision: latestDoc.revision,
      node_start: spanStart(node.source_span),
      node_end: spanEnd(node.source_span),
      element
    });
  }

  function removeMultiInputElement(pin) {
    if (!latestDoc || !pin) return;
    const graph = currentGraphOrNull();
    const node = graph && (graph.nodes || []).find((n) => n.node_id === pin.node_id);
    if (!node) return showToast("Element source moved");
    postTransaction({
      schema_version: 1,
      op: "remove_multi_input_element",
      revision: latestDoc.revision,
      node_start: spanStart(node.source_span),
      node_end: spanEnd(node.source_span),
      element_start: spanStart(pin.source_span),
      element_end: spanEnd(pin.source_span)
    });
  }

  function collapseSelectedNodes() {
    const graph = currentGraphOrNull();
    if (!graph) return;
    const selected = selectedGraphNodes(graphWithViewState(graph)).filter((node) => node.source_span);
    if (!selected.length) return showToast("Select source nodes to collapse");
    if (selected.some((node) => node.collapsed_region_id)) {
      return showToast("Selection is already collapsed");
    }
    const title = window.prompt("Collapsed region title", "Collapsed");
    if (!title) return;
    postTransaction({
      schema_version: 1,
      op: "create_collapsed_region",
      revision: latestDoc.revision,
      graph_id: graph.graph_id,
      start: Math.min(...selected.map((node) => node.source_span.start)),
      end: Math.max(...selected.map((node) => node.source_span.end)),
      title
    });
  }

  function expandSelectedCollapse() {
    const graph = currentGraphOrNull();
    const selected = graph && selectedGraphNodes(graphWithViewState(graph));
    const collapsed = selected && selected.find((node) => node.collapsed_region_id);
    if (!collapsed) return showToast("Select one collapsed region to expand");
    postTransaction({
      schema_version: 1,
      op: "expand_collapsed_region",
      revision: latestDoc.revision,
      region_id: collapsed.collapsed_region_id
    });
  }

  function nodeContextActions(graph, node) {
    const actions = [
      { title: "Copy", detail: "selection", group: "Edit", run: copySelection },
      { title: "Paste as Staged", detail: "local selection", group: "Edit", run: pasteAsStaged },
      { title: "Duplicate", detail: "selection", group: "Edit", run: duplicateSelection },
      { title: "Add Comment", detail: "around selection", group: "Comment", run: addCommentAroundSelection },
      { title: "Jump Source", detail: "span", group: "Source", run: () => { const s = node.source_span || { start: 0, end: 0 }; setSourceHash(s); setViewMode("code"); } },
      { title: "Find References", detail: "search index", group: "Query", run: () => postQuery({ op: "references", symbol: node.title }) },
      { title: nodeBreakpoint(node) ? "Remove Breakpoint" : "Set Breakpoint", detail: "local span", group: "Debug", run: () => toggleBreakpoint(node) }
    ];
    if ((node.edit_affordances || []).includes("add_pattern_arm")) actions.unshift({ title: "Add Pattern Arm", detail: "source transaction", group: "Patterns", run: () => addPatternArm(node) });
    if ((node.edit_affordances || []).includes("append_multi_input")) actions.unshift({ title: "Append Input", detail: "source transaction", group: "Pins", run: () => appendMultiInput(node) });
    const stateBadge = ((node.badges || []).find((badge) => badge === "#Off" || badge === "#DebugOnly")) || "";
    if (!node.staged && node.source_span && node.kind !== "entry") {
      actions.unshift({
        title: stateBadge ? "Turn on" : "Turn off",
        detail: stateBadge ? stateBadge + " source transaction" : "#Off source transaction",
        group: "State",
        run: () => toggleSwitchState(node)
      });
    }
    if (node.staged) actions.unshift({ title: "Delete Staged Node", detail: "local view", group: "Edit", run: () => { removeStagedNode(node.node_id); drawGraph(latestDoc); } });
    if (graphForFunctionName(node.title)) actions.unshift({ title: "Open Function", detail: "function", group: "Graph", run: () => openFunctionGraph(node.title) });
    return actions;
  }

  function syncGraphStrip(doc) {
    const key = graphNavigationKey(doc);
    if (graphStripKey === key) return;
    graphStripKey = key;
    graphStrip.innerHTML = "";
    if (graphCount) graphCount.textContent = String((doc.graphs || []).length);
    for (const graph of doc.graphs || []) {
      const button = document.createElement("button");
      const callback = callbackEventView(graph);
      button.type = "button";
      button.className = "graph-tab" + (graph.graph_id === selectedGraphId ? " is-active" : "");
      button.setAttribute("data-graph-tab", graph.graph_id);
      button.title = callback ? "Open Callback Handler: " + callback.function : "Open Graph: " + graph.title;
      if (callback) button.setAttribute("data-callback-handler", callback.function);
      button.innerHTML = "<span class=\"graph-tab-kind\">" + (callback ? "Callback" : "Fn") + "</span><span class=\"graph-tab-title\">" + escapeHtml(graph.title) + "</span><span class=\"graph-tab-count\">" + graph.nodes.length + "</span>";
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

  function graphActionItems(options = {}) {
    const sourceTransactionOnly = options.sourceTransactionOnly === true;
    const graph = currentGraphOrNull();
    const actions = [
      { title: "Fit Graph", detail: "viewport", group: "View", run: fitGraph },
      { title: "New Function", detail: "source", group: "Function", run: createCanvasFunction },
      { title: "New Callback", detail: "source handler", group: "Function", run: createCanvasCallback },
      { title: "Show Source", detail: "toggle", group: "Execution", run: () => setViewMode("code") },
      { title: "Paste", detail: "selection", group: "Execution", run: pasteSelection },
      { title: "Paste as Staged", detail: "local selection", group: "Execution", run: pasteAsStaged },
      { title: "Duplicate", detail: "selection", group: "Execution", run: duplicateSelection },
      { title: "Add Comment", detail: "local view", group: "Comment", run: addCommentAroundSelection },
      { title: "Align Top", detail: "local view", group: "Execution", run: () => alignSelectedNodes("y") },
      { title: "Align Left", detail: "local view", group: "Execution", run: () => alignSelectedNodes("x") },
      { title: "Auto Tidy", detail: "local view", group: "Execution", run: tidyGraphLayout },
      { title: "Straighten Wires", detail: "Blueprint curves", group: "Execution", run: () => { wireStyle = "bezier"; showToast("Wires use Blueprint curves"); drawGraph(latestDoc); } },
      { title: "Add Reroute Knot", detail: "local view", group: "Execution", run: addRerouteKnot },
      { title: "Bookmark Graph", detail: "local editor state", group: "Navigation", run: bookmarkCurrentGraph }
    ];
    if (canvasCapability("runtime_output")) {
      actions.push({ title: "Run Graph", detail: "debug overlay", group: "Run", run: runCurrentGraph });
    }
    actions.push(...variableActionsForGraph(graph));
    actions.push(...traitMethodActions(latestDoc));
    if (!sourceTransactionOnly) actions.push(...eventDispatcherActions(latestDoc));
    for (const item of palette.concat(actionEntries)) {
      if (!canvasCapability("runtime_output") && item.action_id === "canvas.command:run") continue;
      const run = sourceTransactionOnly && item.kind === "canvas.core_catalog"
        ? () => runLibraryAction(item)
        : () => runPalette(item);
      actions.push({ title: item.title, detail: item.detail || "", group: item.group || (item.op === "preview_canvas_action" ? "Project" : "Execution"), kind: item.kind, node_descriptor_id: item.node_descriptor_id, module_path: item.module_path, signature: item.signature, summary: item.summary, pure: item.pure, pins: item.pins, ret: item.ret, type: item.type, op: item.op, action_id: item.action_id, callee: item.callee, insert_callee: item.insert_callee, args: item.args, available: item.available, stageable: item.stageable, stage_reason_code: item.stage_reason_code, stage_reason: item.stage_reason, receiver_type: item.receiver_type, denied_reason: item.denied_reason, unavailable_reason_code: item.unavailable_reason_code, run });
    }
    return actions;
  }

  function openGraphActionPalette(x, y, query, graphPoint, options = {}) {
    if (latestDoc && actionEntries.length && actionEntriesRevision === latestDoc.revision) {
      openActionPalette(x, y, "Canvas actions", graphActionItems(options), { context: "All nodes", query: query || "", graphPoint: graphPoint || graphPointFromClient(x, y) });
      return;
    }
    loadCanvasActions({ skipRedraw: true }).then(() => {
      openActionPalette(x, y, "Canvas actions", graphActionItems(options), { context: "All nodes", query: query || "", graphPoint: graphPoint || graphPointFromClient(x, y) });
    });
  }

  function renderCoreCatalogPalette(query = "") {
    const actions = actionEntries
      .filter((item) => item.kind === "canvas.core_catalog")
      .map((item) => ({
        title: item.title,
        detail: item.detail,
        group: "Core",
        kind: item.kind,
        node_descriptor_id: item.node_descriptor_id,
        module_path: item.module_path,
        signature: item.signature,
        pure: item.pure,
        pins: item.pins,
        ret: item.ret,
        action_id: item.action_id,
        callee: item.callee,
        insert_callee: item.insert_callee,
        args: item.args,
        available: item.available,
        stageable: item.stageable,
        stage_reason_code: item.stage_reason_code,
        stage_reason: item.stage_reason,
        receiver_type: item.receiver_type,
        denied_reason: item.denied_reason,
        unavailable_reason_code: item.unavailable_reason_code,
        run: () => runPalette(item)
      }));
    openActionPalette(window.innerWidth / 2 - 210, 72, "Core catalog", actions, { context: "core.* modules and methods", query });
  }

  function openCoreCatalogPalette(query = "") {
    if (query) {
      loadCoreCatalogActions(query).then(() => renderCoreCatalogPalette(query));
      contextMenuState = null;
      return;
    }
    if (!coreCatalogLoaded && !coreCatalogLoading) {
      loadCoreCatalogActions("").then(() => openCoreCatalogPalette(query));
      return;
    }
    if (!coreCatalogLoaded && coreCatalogLoading) {
      coreCatalogLoading.then(() => openCoreCatalogPalette(query));
      return;
    }
    renderCoreCatalogPalette(query);
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
    selectedVariableName = null;
    hoverPin = null;
    hoverNode = null;
    hoverDiagnostic = null;
    selectedNodeId = opts.nodeId || graph.entry_node;
    selectedNodeIds = new Set([selectedNodeId]);
    selectionExplicitlyCleared = false;
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
    const previous = debugState;
    try {
      const stored = JSON.parse(localStorage.getItem(key) || "null");
      debugState = stored && typeof stored === "object" ? stored : { breakpoints: [], watches: [] };
    } catch (_) {
      debugState = { breakpoints: [], watches: [] };
    }
    debugState.key = key;
    debugState.revision = doc.revision;
    const storedBreakpoints = Array.isArray(debugState.breakpoints) ? debugState.breakpoints : [];
    const storedWatches = Array.isArray(debugState.watches) ? debugState.watches : [];
    debugState.breakpoints = storedBreakpoints.filter((b) => b && typeof b === "object").slice(0, 128);
    debugState.staleBreakpoints = debugState.breakpoints.filter((b) => b.revision !== doc.revision);
    debugState.watches = storedWatches.filter((watch) => typeof watch === "string" && watch.trim()).slice(0, 32);
    const discarded = storedBreakpoints.length > debugState.breakpoints.length || storedWatches.length > debugState.watches.length;
    if (discarded && (!previous || previous.revision !== doc.revision)) {
      showToast("Debug state was bounded; source was kept", { isError: true });
    }
    if (debugState.staleBreakpoints.length && (!previous || previous.revision !== doc.revision)) {
      showToast(debugState.staleBreakpoints.length + " breakpoint" + (debugState.staleBreakpoints.length === 1 ? " is" : "s are") + " stale; source was kept", { isError: true });
    }
    syncDebugActive();
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
    return (debugState.breakpoints || []).find((b) => b.revision === (latestDoc && latestDoc.revision) && b.anchor === anchor);
  }

  function toggleBreakpoint(node) {
    if (!latestDoc || !node || !node.source_span) return;
    loadDebugState(latestDoc);
    const anchor = spanAnchor(node.source_span);
    const current = debugState.breakpoints.some((b) => b.revision === latestDoc.revision && b.anchor === anchor);
    if (!current && debugState.breakpoints.length >= 128) {
      showToast("Breakpoint limit reached; source was kept", { isError: true });
      return;
    }
    debugState.breakpoints = debugState.breakpoints.filter((b) => !(b.revision === latestDoc.revision && b.anchor === anchor));
    if (!current) {
      debugState.breakpoints.push({ anchor, source_span: node.source_span, node_id: node.node_id, revision: latestDoc.revision });
      showToast("Breakpoint anchored to source span");
    } else {
      showToast("Breakpoint removed");
    }
    saveDebugState();
    drawGraph(latestDoc);
  }

  function addWatch(name) {
    if (!latestDoc || !name) return;
    loadDebugState(latestDoc);
    if (!debugState.watches.includes(name)) {
      if (debugState.watches.length >= 32) {
        showToast("Watch limit reached; source was kept", { isError: true });
        return;
      }
      debugState.watches.push(name);
    }
    saveDebugState();
    showToast("Watch added: " + name);
  }

  function debugSessionSnapshot(id = debugSessionId, info = debugSessionInfo, doc = latestDoc) {
    if (!id || !doc) return null;
    return {
      id,
      revision: doc.revision,
      sourceId: currentCanvasSourceId(),
      tier: info && info.tier
    };
  }

  function debugResponseSnapshot(json) {
    const session = json && json.session;
    if (!session || session.state !== "running" || !session.id || !session.revision) return null;
    return {
      id: session.id,
      revision: session.revision,
      sourceId: session.source_id || json.source_id,
      tier: session.tier
    };
  }

  function restoreCanvasDebugger() {
    const persisted = canvasSession && canvasSession.debugger;
    if (!latestDoc || !persisted || persisted.state !== "active" || !persisted.session_id) return;
    if (persisted.source_id && persisted.source_id !== currentCanvasSourceId()) return;
    if (persisted.revision && persisted.revision !== latestDoc.revision) return;
    if (debugSessionId) return;
    debugSessionId = persisted.session_id;
    debugSessionInfo = {
      id: persisted.session_id,
      state: "running",
      tier: persisted.tier || "jet-dev-interpreter",
      source_id: persisted.source_id || currentCanvasSourceId(),
      revision: persisted.revision || latestDoc.revision
    };
    debugConnectionState = "connecting";
    syncDebugSessionPicker();
    runDebug([]);
  }

  function releaseDebugSession(snapshot, report = false) {
    if (!snapshot || !snapshot.id || !snapshot.revision) return Promise.resolve();
    const debugUrl = window.__JET_CANVAS_DEBUG__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/debug");
    const body = { schema_version: 1, revision: snapshot.revision, session_id: snapshot.id, stop: true };
    if (snapshot.sourceId) body.source_id = snapshot.sourceId;
    if (snapshot.tier) body.tier = snapshot.tier;
    return fetch(debugUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((response) => response.json().catch(() => ({})).then((json) => ({ ok: response.ok, json: canvasPayload(json) })))
      .then((result) => {
        if (report && !result.ok) showToast((result.json && result.json.message) || "Debug stop rejected", { isError: true });
      })
      .catch(() => {
        if (report) showToast("Debug session disconnected; source was kept", { isError: true });
      });
  }

  function clearDebugClientState(state = "idle") {
    debugSessionId = null;
    debugSessionInfo = null;
    debugOverlay = null;
    debugConnectionState = state;
    syncDebugSessionPicker();
    syncDebugActive();
  }

  function syncDebugLiveness() {
    if (!debugLiveness) return;
    const labels = {
      idle: "Runtime idle",
      connecting: "Runtime connecting",
      live: "Runtime live",
      finished: "Runtime finished",
      stale: "Source changed · anchors stale",
      disconnected: "Runtime disconnected",
      failed: "Runtime unavailable"
    };
    const staleCount = debugState && Array.isArray(debugState.staleBreakpoints) ? debugState.staleBreakpoints.length : 0;
    const suffix = staleCount && debugConnectionState !== "live" && debugConnectionState !== "connecting"
      ? " · " + staleCount + " stale anchor" + (staleCount === 1 ? "" : "s")
      : "";
    debugLiveness.textContent = (labels[debugConnectionState] || labels.idle) + suffix;
    debugLiveness.dataset.state = debugConnectionState;
  }

  function syncDebugSessionPicker() {
    if (debugSession) {
      debugSession.innerHTML = "";
      const option = document.createElement("option");
      option.value = debugSessionId || "none";
      option.textContent = debugSessionInfo && debugSessionInfo.state === "running"
        ? "Canvas · " + (debugSessionInfo.tier || "live session")
        : "No live session";
      debugSession.appendChild(option);
      debugSession.disabled = !debugSessionId;
    }
    syncDebugLiveness();
  }

  function runDebug(commands) {
    if (!latestDoc || !canvasCapability("runtime_output")) return;
    loadDebugState(latestDoc);
    debugConnectionState = "connecting";
    syncDebugSessionPicker();
    const requestGeneration = ++debugRequestGeneration;
    const requestedSession = debugSessionSnapshot();
    const requestedRevision = latestDoc.revision;
    const requestedSourceId = currentCanvasSourceId();
    const debugUrl = window.__JET_CANVAS_DEBUG__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/debug");
    const body = {
      schema_version: 1,
      revision: requestedRevision,
      commands,
      breakpoint_spans: (debugState.breakpoints || [])
        .filter((b) => b.revision === requestedRevision)
        .map((b) => b.anchor),
      watches: debugState.watches || []
    };
    if (requestedSourceId) body.source_id = requestedSourceId;
    if (requestedSession) body.session_id = requestedSession.id;
    if (debugSessionInfo && debugSessionInfo.tier) body.tier = debugSessionInfo.tier;
    fetch(debugUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: canvasPayload(j) })))
      .then((result) => {
        if (requestGeneration !== debugRequestGeneration) {
          releaseDebugSession(debugResponseSnapshot(result.json));
          return;
        }
        const responseSourceId = result.json && (result.json.source_id || (result.json.session && result.json.session.source_id));
        const responseRevision = result.json && (result.json.revision || (result.json.session && result.json.session.revision));
        if (latestDoc && (
          latestDoc.revision !== requestedRevision
          || currentCanvasSourceId() !== requestedSourceId
          || (result.ok && (
            responseRevision !== requestedRevision
            || (responseSourceId && responseSourceId !== currentCanvasSourceId())
          ))
        )) {
          releaseDebugSession(debugResponseSnapshot(result.json) || requestedSession);
          clearDebugClientState("stale");
          showToast("Debug result is stale; current source was kept", { isError: true });
          drawGraph(latestDoc);
          return;
        }
        if (!result.ok) {
          const kind = result.json && result.json.kind;
          const keepSession = requestedSession && ["bad_request", "schema", "unsupported", "limit"].includes(kind);
          if (!keepSession) {
            releaseDebugSession(requestedSession);
            clearDebugClientState(kind === "conflict" ? "stale" : "failed");
          }
          showToast((result.json.message || "Debug rejected").split("\n")[0], { isError: true });
          if (!keepSession) drawGraph(latestDoc);
          return;
        }
        debugSessionInfo = result.json.session || null;
        debugSessionId = debugSessionInfo && debugSessionInfo.state === "running" ? debugSessionInfo.id : null;
        debugOverlay = result.json.overlay || null;
        debugConnectionState = debugOverlay && debugOverlay.runtime_state === "live"
          ? "live"
          : debugOverlay && debugOverlay.runtime_state === "finished"
            ? "finished"
            : "failed";
        syncDebugSessionPicker();
        syncDebugActive();
        if (debugOverlay && debugOverlay.active_graph_id) selectedGraphId = debugOverlay.active_graph_id;
        if (debugOverlay && debugOverlay.active_node_id) selectedNodeId = debugOverlay.active_node_id;
        const limits = debugOverlay && debugOverlay.limits;
        const limited = limits && Object.keys(limits).some((key) => limits[key] === true);
        showToast(limited
          ? "Debug values truncated; source is current"
          : "Debug " + ((debugOverlay && debugOverlay.debug_overlay) || "updated"));
        drawGraph(latestDoc);
      })
      .catch(() => {
        if (requestGeneration !== debugRequestGeneration) return;
        releaseDebugSession(requestedSession);
        clearDebugClientState("disconnected");
        showToast("Debug session disconnected; source was kept", { isError: true });
        drawGraph(latestDoc);
      });
  }

  function stopDebug() {
    const doc = latestDoc;
    const session = debugSessionSnapshot();
    debugRequestGeneration++;
    clearDebugClientState("idle");
    if (session && doc) releaseDebugSession(session, true);
    showToast("Debug overlay stopped");
    if (latestDoc) drawGraph(latestDoc);
  }

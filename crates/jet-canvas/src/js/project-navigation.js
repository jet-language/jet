
// Project panels, graph navigation, variables, source-backed actions, and debugger state.
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

  function graphVariables(graph) {
    if (!graph) return [];
    const vars = new Map();
    const addVar = (name, type, source, editable, defaultSource, nodeId) => {
      if (!name) return;
      const prev = vars.get(name) || {};
      vars.set(name, {
        name,
        type: type || prev.type || "Value",
        source: source || prev.source || "local",
        editable: editable || prev.editable || false,
        defaultSource: defaultSource !== undefined ? defaultSource : prev.defaultSource || "",
        nodeId: nodeId || prev.nodeId || ""
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
        addVar(node.title, (dataOut && dataOut.type) || "Value", node.kind === "variable_get" ? "read" : "local", node.kind === "binding", init ? init.source : "", node.node_id);
      }
    }
    return Array.from(vars.values()).sort((a, b) => (a.source === "input" ? 0 : 1) - (b.source === "input" ? 0 : 1) || a.name.localeCompare(b.name));
  }

  function syncVariablesList(graph) {
    if (!variablesList) return;
    const vars = graphVariables(graph);
    if (variableCount) variableCount.textContent = String(vars.length);
    variablesList.innerHTML = vars.map((v) => {
      const color = colorForType(v.type);
      const active = selectedVariableName === v.name ? " is-active" : "";
      return `<button class="variable-item${active}" type="button" data-variable-name="${escapeAttr(v.name)}"><span class="variable-dot" style="color:${escapeAttr(color)}"></span><span class="variable-name">${escapeHtml(v.name)}</span>${typeChipHtml(v.type)}</button>`;
    }).join("") || "<div class=\"tag\">no variables</div>";
    variablesList.querySelectorAll("[data-variable-name]").forEach((button) => {
      button.addEventListener("click", () => selectVariable(button.getAttribute("data-variable-name")));
    });
    window.__jetCanvasVariablesSidebar = vars.length;
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
    lockRows.unshift(projectMiniCard("Source files", project.source_control && project.source_control.truth || "git text", `${policy.semantic || "source"} model · ${(policy.local || []).join(", ") || "local viewport"}`));
    syncProjectPanel(packageSummary, "Packages", packageRows, "no packages");
    syncProjectPanel(dependencySummary, "Dependencies", depRows, "no dependencies");
    syncProjectPanel(devSummary, "Dev", devRows, "no env or services");
    syncProjectPanel(diagnosticsSummary, "Diagnostics", diagRows, "clean");
    syncProjectPanel(trustSummary, "Source internals", lockRows, "source files only");
    if (statusSummary) {
      const packageName = (project.packages || [])[0] && ((project.packages || [])[0].name || (project.packages || [])[0].path);
      const sourceFiles = (project.files || []).filter((f) => f.kind === "source").length;
      const diagCount = diagRows.length;
      statusSummary.innerHTML = [
        `<div class="status-card"><b>${escapeHtml(packageName || project.mode || "Single file")}</b><small>${escapeHtml(sourceFiles + " source file" + (sourceFiles === 1 ? "" : "s"))}</small></div>`,
        `<div class="status-card"><b>${diagCount === 0 ? "Clean" : diagCount + " issue" + (diagCount === 1 ? "" : "s")}</b><small>${escapeHtml((project.mode || "file") + " mode")}</small></div>`
      ].join("");
      if (statusCount) statusCount.textContent = diagCount === 0 ? "clean" : String(diagCount);
    }
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
    const fileCount = (project.files || []).length;
    for (const file of (project.files || []).filter((f) => f.kind === "source").slice(0, 12)) {
      const active = (selectedSourceId || project.entry) === file.path ? " is-active" : "";
      cards.push(`<button class="project-card${active}" type="button" data-project-file="${escapeAttr(file.path || "")}"><b>${escapeHtml(file.path || "source")}</b><small>${escapeHtml(active ? "open" : "click to open")}</small><code class="dev-only">${escapeHtml(file.revision || "")}</code></button>`);
    }
    if (!cards.length) cards.push(`<button class="project-card is-active" type="button"><b>${escapeHtml(project.entry || "source")}</b><small>open</small></button>`);
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
    let detail = "Drag from a pin or right-click the canvas";
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

  function nodeContextActions(graph, node) {
    const actions = [
      { title: "Copy", detail: "selection", group: "edit", run: copySelection },
      { title: "Duplicate", detail: "selection", group: "edit", run: duplicateSelection },
      { title: "Add comment", detail: "around selection", group: "Comment", run: addCommentAroundSelection },
      { title: "Jump source", detail: "span", group: "source", run: () => { const s = node.source_span || { start: 0, end: 0 }; setSourceHash(s); setViewMode("code"); } },
      { title: "Find references", detail: "search index", group: "query", run: () => postQuery({ op: "references", symbol: node.title }) },
      { title: "Set breakpoint", detail: "local span", group: "debug", run: () => toggleBreakpoint(node) }
    ];
    if ((node.edit_affordances || []).includes("add_pattern_arm")) actions.unshift({ title: "Add pattern arm", detail: "source transaction", group: "Patterns", run: () => addPatternArm(node) });
    if ((node.edit_affordances || []).includes("append_multi_input")) actions.unshift({ title: "Append input", detail: "source transaction", group: "Pins", run: () => appendMultiInput(node) });
    if (node.staged) actions.unshift({ title: "Delete staged node", detail: "local view", group: "edit", run: () => { removeStagedNode(node.node_id); drawGraph(latestDoc); } });
    if (graphForFunctionName(node.title)) actions.unshift({ title: "Open function", detail: "function", group: "graph", run: () => openFunctionGraph(node.title) });
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
      { title: "Show source", detail: "toggle", group: "Execution", run: () => setViewMode("code") },
      { title: "Paste", detail: "selection", group: "Execution", run: pasteSelection },
      { title: "Duplicate", detail: "selection", group: "Execution", run: duplicateSelection },
      { title: "Add comment", detail: "local view", group: "Comment", run: addCommentAroundSelection },
      { title: "Align top", detail: "local view", group: "Execution", run: () => alignSelectedNodes("y") },
      { title: "Align left", detail: "local view", group: "Execution", run: () => alignSelectedNodes("x") },
      { title: "Auto tidy", detail: "local view", group: "Execution", run: tidyGraphLayout },
      { title: "Straighten wires", detail: "Blueprint curves", group: "Execution", run: () => { wireStyle = "bezier"; showToast("Wires use Blueprint curves"); drawGraph(latestDoc); } },
      { title: "Add reroute knot", detail: "local view", group: "Execution", run: addRerouteKnot },
      { title: "Bookmark graph", detail: "local editor state", group: "navigation", run: bookmarkCurrentGraph },
      { title: "Run graph", detail: "debug overlay", group: "run", run: runCurrentGraph }
    ];
    actions.push(...variableActionsForGraph(graph));
    for (const item of palette.concat(actionEntries)) {
      actions.push({ title: item.title, detail: item.detail || "", group: item.group || (item.op === "preview_canvas_action" ? "Project" : "Execution"), kind: item.kind, module_path: item.module_path, signature: item.signature, summary: item.summary, pure: item.pure, pins: item.pins, ret: item.ret, type: item.type, op: item.op, action_id: item.action_id, callee: item.callee, insert_callee: item.insert_callee, args: item.args, available: item.available, denied_reason: item.denied_reason, unavailable_reason_code: item.unavailable_reason_code, run: () => runPalette(item) });
    }
    return actions;
  }

  function openGraphActionPalette(x, y, query, graphPoint) {
    openActionPalette(x, y, "Canvas actions", graphActionItems(), { context: "All nodes", query: query || "", graphPoint: graphPoint || graphPointFromClient(x, y) });
  }

  function renderCoreCatalogPalette(query = "") {
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
        available: item.available,
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
          syncDebugActive();
          showToast((result.json.message || "Debug rejected").split("\n")[0]);
          return;
        }
        debugOverlay = result.json.overlay || null;
        syncDebugActive();
        if (debugOverlay && debugOverlay.active_graph_id) selectedGraphId = debugOverlay.active_graph_id;
        if (debugOverlay && debugOverlay.active_node_id) selectedNodeId = debugOverlay.active_node_id;
        showToast("Debug " + ((debugOverlay && debugOverlay.debug_overlay) || "updated"));
        drawGraph(latestDoc);
      })
      .catch((e) => showToast(String(e)));
  }

  function stopDebug() {
    debugOverlay = null;
    syncDebugActive();
    showToast("Debug overlay stopped");
    if (latestDoc) drawGraph(latestDoc);
  }

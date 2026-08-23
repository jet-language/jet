
// Project panels, graph navigation, variables, source-backed actions, and debugger state.
  function ensureCanvasChrome() {
    const stage = document.getElementById("stage");
    if (stage && !document.getElementById("canvas-state")) {
      const state = document.createElement("section");
      state.id = "canvas-state";
      state.setAttribute("role", "status");
      state.setAttribute("aria-live", "polite");
      state.style.cssText = "position:absolute;z-index:28;left:50%;top:12px;transform:translateX(-50%);display:none;grid-template-columns:minmax(0,1fr) auto;gap:12px;align-items:center;width:min(520px,calc(100% - 24px));padding:10px 12px;border:1px solid #365a7f;border-radius:7px;background:rgba(7,16,28,.96);box-shadow:0 18px 52px rgba(0,0,0,.48);color:#c9dcf2";
      state.innerHTML = "<div><b id=\"canvas-state-title\" style=\"display:block;color:#f8fbff\"></b><span id=\"canvas-state-detail\" style=\"display:block;margin-top:3px;color:#9fb9d8;line-height:1.35\"></span></div><div id=\"canvas-state-actions\" style=\"display:flex;gap:6px;flex-wrap:wrap;justify-content:end\"></div>";
      stage.appendChild(state);
    }
    const statusbar = document.getElementById("statusbar");
    if (statusbar && !document.getElementById("save-state")) {
      const saved = document.createElement("span");
      saved.id = "save-state";
      saved.textContent = "source saved";
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
  }

  function setSaveState(label, kind = "saved") {
    const saved = document.getElementById("save-state");
    if (!saved) return;
    saved.textContent = label;
    saved.style.color = kind === "error" ? "#fecaca" : kind === "draft" ? "#fde68a" : "#8fb2dc";
    saved.dataset.state = kind;
  }

  function setCanvasState(kind, title, detail, actions = []) {
    ensureCanvasChrome();
    const state = document.getElementById("canvas-state");
    const titleEl = document.getElementById("canvas-state-title");
    const detailEl = document.getElementById("canvas-state-detail");
    const actionsEl = document.getElementById("canvas-state-actions");
    if (!state || !titleEl || !detailEl || !actionsEl) return;
    state.dataset.state = kind || "info";
    titleEl.textContent = title || "Canvas";
    detailEl.textContent = detail || "";
    actionsEl.innerHTML = "";
    for (const action of actions) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = action.label;
      button.style.cssText = "min-height:27px;padding:0 8px";
      if (action.primary) button.classList.add("primary");
      button.addEventListener("click", () => action.run && action.run());
      actionsEl.appendChild(button);
    }
    state.style.display = "grid";
    window.__jetCanvasCanvasState = { kind: kind || "info", title: title || "Canvas", detail: detail || "", actions: actions.map((action) => action.label) };
  }

  function clearCanvasState() {
    const state = document.getElementById("canvas-state");
    if (!state) return;
    state.style.display = "none";
    state.dataset.state = "";
    window.__jetCanvasCanvasState = null;
  }

  const TOUR_STEPS = [
    { title: "Read the graph", detail: "Files, functions, and variables live in the left rail. Select a node to inspect its source-backed details. The graph is a view of Jet source, not a second file.", target: "left-drawer" },
    { title: "Edit, then save", detail: "Use the palette or an inline value. Accepted edits are checked, formatted, and written to Jet source immediately. Code and Split keep the source in reach.", action: "Open source", target: "jet-canvas-view" },
    { title: "Check and fix", detail: "Check runs Jet diagnostics. Problems stay in the panel with what, why, and fix text. Rejected edits leave the last valid source intact.", action: "Check source", target: "check-current" },
    { title: "Run and inspect", detail: "Run opens a command card with its authority. Execute it there; the Run HUD and Details panel show the real receipt and output.", action: "Prepare run", target: "run-current" },
    { title: "Undo and keep control", detail: "Undo restores exact validated source. Reload reprojects from disk. Canvas saves source edits; local layout and tour state stay separate.", action: "Finish tour", target: "undo-edit" }
  ];
  let tourStep = 0;

  function renderTour() {
    const tour = document.getElementById("first-run-tour");
    if (!tour) return;
    const dismissed = !!editorState.tourDismissed;
    tourStep = Math.max(0, Math.min(TOUR_STEPS.length - 1, Number(editorState.tourStep) || 0));
    const step = TOUR_STEPS[tourStep];
    const progress = document.getElementById("tour-progress");
    const title = document.getElementById("tour-title");
    const detail = document.getElementById("tour-detail");
    const action = document.getElementById("tour-action");
    const back = document.getElementById("tour-back");
    const next = document.getElementById("tour-next");
    if (!step || !progress || !title || !detail || !action || !back || !next) return;
    progress.textContent = `Step ${tourStep + 1} of ${TOUR_STEPS.length}`;
    title.textContent = step.title;
    detail.textContent = step.detail;
    action.textContent = step.action || "";
    action.style.display = step.action ? "inline-block" : "none";
    back.disabled = tourStep === 0;
    next.textContent = tourStep === TOUR_STEPS.length - 1 ? "Done" : "Next";
    next.disabled = tourStep === TOUR_STEPS.length - 1;
    tour.dataset.tourStep = String(tourStep);
    tour.setAttribute("aria-hidden", dismissed ? "true" : "false");
    tour.classList.toggle("is-open", !dismissed);
    window.__jetCanvasTourState = { step: tourStep, total: TOUR_STEPS.length, title: step.title, target: step.target, dismissed };
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
    if (tourStep >= TOUR_STEPS.length - 1) return;
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
    const step = TOUR_STEPS[tourStep];
    if (!step) return;
    if (step.action === "Open source") setViewMode("split");
    else if (step.action === "Check source") showCheckAuthority();
    else if (step.action === "Prepare run") runCurrentGraph();
    else if (step.action === "Finish tour") finishTour();
    renderTour();
  }

  ensureCanvasChrome();
  window.addEventListener("offline", () => {
    setCanvasState("offline", "Offline", "Canvas cannot write while disconnected. Jet source stays visible; reconnect, then retry.", [
      { label: "Show source", run: openSourceRecovery },
      { label: "Retry", primary: true, run: loadGraph }
    ]);
    setSaveState("source unchanged", "error");
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
    const addVar = (name, type, source, editable, defaultSource, nodeId, meta) => {
      if (!name) return;
      const prev = vars.get(name) || {};
      vars.set(name, {
        name,
        type: type || prev.type || "Value",
        source: source || prev.source || "local",
        editable: editable || prev.editable || false,
        defaultSource: defaultSource !== undefined ? defaultSource : prev.defaultSource || "",
        nodeId: nodeId || prev.nodeId || "",
        meta: meta !== undefined ? meta : prev.meta || null
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
      return `<button type="button" data-trait-jump="${index}">${escapeHtml(label || "Open source")}</button>`;
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
        return `<div class="signature-board" data-trait-implementation="${escapeAttr(String(impl.type || ""))}"><div class="signature-head"><div><span class="sig-eyebrow">Implementation</span><b>${escapeHtml(impl.type || "Type")}</b><code>${escapeHtml(traitQualifiedName(trait))}</code></div>${jumpButton(impl.source_span, "Open source")}</div><div class="lane-meta">${implemented.length}/${requiredMethods.length} required methods</div><div class="pin-list">${methodStatus || "<div class=\"pin-empty\">No required methods</div>"}${assocStatus}</div></div>`;
      }).join("");
      const methodRows = methods.map((method) => {
        const label = traitMethodRequired(method) ? "Required" : "Default";
        return `<div class="pin-row"><div class="lane-head"><b>${escapeHtml(method.name || "method")}</b><span class="tag">${label}</span></div><code>${escapeHtml(method.signature || "")}</code>${jumpButton(method.source_span, "Open source")}</div>`;
      }).join("");
      const assocRows = associatedTypes.length
        ? `<div class="tag">Associated types: ${escapeHtml(associatedTypes.join(", "))}</div>`
        : "";
      const create = associatedTypes.length
        ? `<div class="tag">Add associated type choices in source before creating an implementation.</div>`
        : `<div class="edit-grid"><label><span>Type name</span><input data-trait-type="${traitIndex}" placeholder="${escapeAttr(traitScope(trait) ? "Type in " + traitScope(trait) : "Type name")}"></label><button class="primary" type="button" data-trait-create="${traitIndex}">Add implementation</button></div>`;
      stateTraits.push({
        name: trait.trait,
        scope: traitScope(trait),
        requiredMethods: requiredMethods.map((method) => method.name),
        methods: methods.map((method) => method.name),
        implementations: impls.map((impl) => ({ type: impl.type, methods: (impl.methods || []).slice() }))
      });
      return `<div class="signature-board" data-trait="${escapeAttr(traitQualifiedName(trait))}"><div class="signature-head"><div><span class="sig-eyebrow">Trait</span><b>${escapeHtml(trait.trait || "Trait")}</b><code>${escapeHtml(traitScope(trait) || "file scope")}</code></div>${jumpButton(trait.source_span, "Open source")}</div><div class="lane-head"><b>Methods</b><span class="lane-meta">${requiredMethods.length} required · ${methods.length - requiredMethods.length} default</span></div><div class="pin-list">${methodRows || "<div class=\"pin-empty\">No methods</div>"}</div>${assocRows}${implRows || "<div class=\"pin-empty\">No implementation</div>"}${create}</div>`;
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
      const run = (svc.run || []).join(" ") || "catalog/default";
      devRows.push(projectMiniCard(svc.name || "service", svc.enable === false ? "disabled" : "enabled", `${ports} · ${run}`));
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
    const sourceFiles = (project.files || []).filter((f) => f.kind === "source");
    projectMode.textContent = `${project.mode || "file"} · ${sourceFiles.length} source files`;
    const cards = [];
    const fileCount = (project.files || []).length;
    for (const file of sourceFiles) {
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
        title: "set " + name,
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
      { title: "Copy", detail: "selection", group: "edit", run: copySelection },
      { title: "Duplicate", detail: "selection", group: "edit", run: duplicateSelection },
      { title: "Add comment", detail: "around selection", group: "Comment", run: addCommentAroundSelection },
      { title: "Jump source", detail: "span", group: "source", run: () => { const s = node.source_span || { start: 0, end: 0 }; setSourceHash(s); setViewMode("code"); } },
      { title: "Find references", detail: "search index", group: "query", run: () => postQuery({ op: "references", symbol: node.title }) },
      { title: "Set breakpoint", detail: "local span", group: "debug", run: () => toggleBreakpoint(node) }
    ];
    if ((node.edit_affordances || []).includes("add_pattern_arm")) actions.unshift({ title: "Add pattern arm", detail: "source transaction", group: "Patterns", run: () => addPatternArm(node) });
    if ((node.edit_affordances || []).includes("append_multi_input")) actions.unshift({ title: "Append input", detail: "source transaction", group: "Pins", run: () => appendMultiInput(node) });
    const stateBadge = ((node.badges || []).find((badge) => badge === "#Off" || badge === "#DebugOnly")) || "";
    if (!node.staged && node.source_span && node.kind !== "entry") {
      actions.unshift({
        title: stateBadge ? "Turn on" : "Turn off",
        detail: stateBadge ? stateBadge + " source transaction" : "#Off source transaction",
        group: "State",
        run: () => toggleSwitchState(node)
      });
    }
    if (node.staged) actions.unshift({ title: "Delete staged node", detail: "local view", group: "edit", run: () => { removeStagedNode(node.node_id); drawGraph(latestDoc); } });
    if (graphForFunctionName(node.title)) actions.unshift({ title: "Open function", detail: "function", group: "graph", run: () => openFunctionGraph(node.title) });
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
    actions.push(...traitMethodActions(latestDoc));
    for (const item of palette.concat(actionEntries)) {
      actions.push({ title: item.title, detail: item.detail || "", group: item.group || (item.op === "preview_canvas_action" ? "Project" : "Execution"), kind: item.kind, node_descriptor_id: item.node_descriptor_id, module_path: item.module_path, signature: item.signature, summary: item.summary, pure: item.pure, pins: item.pins, ret: item.ret, type: item.type, op: item.op, action_id: item.action_id, callee: item.callee, insert_callee: item.insert_callee, args: item.args, available: item.available, stageable: item.stageable, stage_reason_code: item.stage_reason_code, stage_reason: item.stage_reason, receiver_type: item.receiver_type, denied_reason: item.denied_reason, unavailable_reason_code: item.unavailable_reason_code, run: () => runPalette(item) });
    }
    return actions;
  }

  function openGraphActionPalette(x, y, query, graphPoint) {
    if (latestDoc && actionEntries.length && actionEntriesRevision === latestDoc.revision) {
      openActionPalette(x, y, "Canvas actions", graphActionItems(), { context: "All nodes", query: query || "", graphPoint: graphPoint || graphPointFromClient(x, y) });
      return;
    }
    loadCanvasActions().then(() => {
      openActionPalette(x, y, "Canvas actions", graphActionItems(), { context: "All nodes", query: query || "", graphPoint: graphPoint || graphPointFromClient(x, y) });
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
    const requestedRevision = latestDoc.revision;
    const requestedSourceId = selectedSourceId || latestDoc.source_id || null;
    const debugUrl = window.__JET_CANVAS_DEBUG__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/debug");
    const body = {
      schema_version: 1,
      revision: requestedRevision,
      commands,
      breakpoint_spans: (debugState.breakpoints || []).map((b) => b.anchor),
      watches: debugState.watches || []
    };
    if (requestedSourceId) body.source_id = requestedSourceId;
    fetch(debugUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (latestDoc && (latestDoc.revision !== requestedRevision || (selectedSourceId || latestDoc.source_id || null) !== requestedSourceId)) {
          showToast("Debug result is stale; current source was kept", { isError: true });
          return;
        }
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

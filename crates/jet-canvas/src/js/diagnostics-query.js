
// Toasts, diagnostics, drawers, source control, proof data, search, and viewport state.
  let sourceControlLoadToken = 0;

  function showToast(text, opts = {}) {
    const message = String(text || "");
    const isError = !!opts.isError || /^Error \[[A-Z0-9]+\]:/.test(message) || message.includes("\n Why:") || message.includes("\n Fix:");
    if (opts.showDetails) {
      clearDom(toast);
      appendText(toast, "span", "", message);
      appendButton(toast, "", "Show details", "", { "data-show-problems": "true" });
    } else {
      toast.textContent = message;
    }
    toast.title = message;
    toast.classList.toggle("is-error", isError);
    toast.setAttribute("role", isError ? "alert" : "status");
    window.clearTimeout(showToast.timer);
    showToast.timer = window.setTimeout(() => {
      toast.textContent = "";
      toast.title = "";
      toast.classList.remove("is-error");
    }, isError ? 10000 : 2200);
  }

  toast.addEventListener("click", (ev) => {
    if (ev.target && ev.target.hasAttribute("data-show-problems")) {
      ev.stopPropagation();
      openProblemsPanel();
      return;
    }
    window.clearTimeout(showToast.timer);
    toast.textContent = "";
    toast.title = "";
    toast.classList.remove("is-error");
  });

  function openProblemsPanel() {
    if (!problemsPanel) return;
    const detailsEl = problemsPanel.querySelector("details");
    if (detailsEl) detailsEl.open = true;
    setDrawer("details");
    problemsPanel.scrollIntoView({ block: "nearest" });
  }

  function openSourceRecovery() {
    const base = window.__JET_CANVAS_BASE__ || "/canvas";
    const source = latestDoc && latestDoc.source_id;
    const query = source ? "?source_id=" + encodeURIComponent(source) : "";
    const currentSource = latestDoc && typeof latestDoc.source_text === "string"
      ? latestDoc.source_text
      : null;
    const draft = latestDoc && readSourceDraft(latestDoc);
    const editorSource = sourceEditMode && sourceEditor ? sourceEditor.value : null;
    const pendingSource = draft !== null && draft !== currentSource
      ? draft
      : editorSource !== null && editorSource !== currentSource ? editorSource : null;
    const sourceRequest = pendingSource !== null
      ? Promise.resolve(pendingSource)
      : fetch(base + "/source" + query, { cache: "no-store" }).then((response) => {
        if (!response.ok) throw new Error("source request failed (" + response.status + ")");
        return response.text();
      });
    return sourceRequest
      .catch((error) => {
        if (currentSource !== null) return currentSource;
        throw error;
      })
      .then((text) => {
        setViewMode("split");
        setSourceEditMode(true);
        if (sourceEditor) {
          sourceEditor.value = text;
          saveSourceDraft(text);
        }
        sourceView.textContent = text;
        setCanvasState("recovery", "Source is available", "Edit the Jet source, then check it before applying a change.", [
          { label: "Close", run: clearCanvasState }
        ]);
        return text;
      })
      .catch((error) => {
        setCanvasState("error", "Source unavailable", "Canvas could not read Jet source. Retry when the server is reachable.", [
          { label: "Retry", primary: true, run: openSourceRecovery }
        ]);
        setSaveState("source unavailable", "error");
        showToast(String(error), { isError: true });
        return null;
      });
  }

  function diagnosticFullText(entry) {
    return entry.rendered || `Error [${entry.code || "diagnostic"}]: ${entry.what || entry.message || ""}\n Why: ${entry.why || ""}\n Fix: ${entry.fix || ""}`;
  }

  function diagnosticKey(diag, i, rev) {
    const span = diag.source_span || {};
    return [rev || "revision", diag.code || "diagnostic", span.start ?? "?", span.end ?? "?", i].join(":");
  }

  function normalizeDiagnostic(diag, i, payload, source) {
    const diagnosticRevision = payload.checked_revision || payload.diagnostic_revision || payload.revision || (latestDoc && latestDoc.revision) || "unknown";
    const baseRevision = payload.revision || (latestDoc && latestDoc.revision) || diagnosticRevision;
    const span = diag.source_span || (diag.span ? {
      start: diag.span.start,
      end: diag.span.end,
      line: diag.line,
      column: diag.col
    } : null);
    const severity = diag.severity === "warning" || String(diag.code || "").startsWith("L") ? "warning" : "error";
    return {
      id: diagnosticKey(diag, i, diagnosticRevision),
      source,
      baseRevision,
      diagnosticRevision,
      severity,
      code: diag.code || "diagnostic",
      what: diag.what || diag.message || "Jet diagnostic",
      why: diag.why || "",
      fix: diag.fix || "",
      rendered: diag.rendered || `Error [${diag.code || "diagnostic"}]: ${diag.what || "Jet diagnostic"}\n Why: ${diag.why || ""}\n Fix: ${diag.fix || ""}`,
      source_span: span,
      line: span && span.line,
      column: span && span.column
    };
  }

  function activeDiagnostics() {
    const revision = latestDoc && latestDoc.revision;
    return diagnosticsState.entries.filter((entry) =>
      !diagnosticsState.dismissed.has(entry.id)
      && (!revision || !entry.baseRevision || entry.baseRevision === revision)
    );
  }

  function acceptDiagnosticsPayload(payload, source) {
    if (!payload) return false;
    const currentSourceId = currentCanvasSourceId();
    const payloadSourceId = payload.source_id || null;
    const currentRevision = latestDoc && latestDoc.revision;
    const payloadRevision = payload.revision || payload.base_revision || null;
    if (
      (currentRevision && payloadRevision && currentRevision !== payloadRevision)
      || (currentSourceId && payloadSourceId && currentSourceId !== payloadSourceId)
    ) {
      setCanvasState("stale", "Diagnostic result is stale", "Canvas kept the current source and previous diagnostics. Check the current source again.", [
        { label: "Open source", run: openSourceRecovery },
        { label: "Retry", primary: true, run: () => checkCurrentSource() }
      ]);
      setSaveState("source unchanged", "error");
      showToast("Diagnostic result is stale; current source was kept", { isError: true });
      return true;
    }
    const raw = Array.isArray(payload.diagnostics) ? payload.diagnostics : [];
    const isCheck = payload.protocol === "jet.canvas.command_receipt" && payload.action_id === "canvas.command:check";
    if (!raw.length && !isCheck) return false;
    const entries = raw.map((diag, i) => normalizeDiagnostic(diag, i, payload, source || "Canvas"));
    diagnosticsState.baseRevision = payload.revision || (latestDoc && latestDoc.revision) || null;
    diagnosticsState.diagnosticRevision = payload.checked_revision || payload.diagnostic_revision || payload.revision || null;
    diagnosticsState.entries = entries;
    diagnosticsState.dismissed = new Set();
    renderProblemsPanel();
    if (entries.length) {
      const first = entries[0];
      setCanvasState("invalid", "Source needs a fix", "Canvas kept the last valid source. Fix the diagnostic in Code view, then check again.", [
        { label: "Open source", run: openSourceRecovery },
        { label: "Dismiss", run: clearCanvasState }
      ]);
      setSaveState("source unchanged", "error");
      showToast(`${entries.length} ${entries.length === 1 ? "problem" : "problems"}: ${first.code} ${first.what}`, { isError: true, showDetails: true });
    } else {
      if (window.__jetCanvasCanvasState && window.__jetCanvasCanvasState.kind === "invalid") clearCanvasState();
      setSaveState("source saved");
      showToast("Check passed");
    }
    if (latestDoc) drawGraph(latestDoc);
    return entries.length > 0;
  }

  function clearDiagnosticsForRevision(revision) {
    if (!diagnosticsState.entries.length) {
      if (window.__jetCanvasCanvasState && ["invalid", "error"].includes(window.__jetCanvasCanvasState.kind)) clearCanvasState();
      setSaveState("source saved");
      return;
    }
    diagnosticsState.entries = [];
    diagnosticsState.dismissed = new Set();
    diagnosticsState.baseRevision = revision || null;
    diagnosticsState.diagnosticRevision = revision || null;
    renderProblemsPanel();
    if (window.__jetCanvasCanvasState && ["invalid", "error"].includes(window.__jetCanvasCanvasState.kind)) clearCanvasState();
    setSaveState("source saved");
    if (latestDoc) drawGraph(latestDoc);
  }

  function clearStaleDiagnostics(doc) {
    if (!doc || !doc.revision || !diagnosticsState.entries.length) return;
    if (diagnosticsState.baseRevision && diagnosticsState.baseRevision !== doc.revision) {
      diagnosticsState.entries = [];
      diagnosticsState.dismissed = new Set();
      diagnosticsState.baseRevision = doc.revision;
      diagnosticsState.diagnosticRevision = doc.revision;
      renderProblemsPanel();
    }
  }

  function nodeDiagnostics(node) {
    if (!node || !node.source_span) return [];
    const graph = latestDoc && currentGraph(latestDoc);
    const relatedSpans = [node.source_span].concat(
      (graph && graph.inline_exprs || [])
        .filter((expr) => expr.node_id === node.node_id && expr.source_span)
        .map((expr) => expr.source_span)
    );
    const diagnostics = activeDiagnostics();
    const direct = diagnostics.filter((diag) =>
      diag.source_span && relatedSpans.some((span) => spansOverlap(span, diag.source_span))
    );
    if (!graph || !graph.function || !graph.function.source_span) return direct;

    // A failed source edit is checked against the candidate text, while the
    // visible graph still describes the last valid revision. Formatting can
    // move an error span away from the stale node/inline spans. Keep the
    // diagnostic on the nearest node in this function so the canvas still
    // exposes the error without pretending that the invalid candidate became
    // the source of truth.
    const fallback = diagnostics.filter((diag) => {
      if (!diag.source_span || direct.includes(diag) || !spansOverlap(graph.function.source_span, diag.source_span)) return false;
      let nearest = null;
      for (const candidate of graph.nodes || []) {
        const spans = [candidate.source_span].concat(
          (graph.inline_exprs || [])
            .filter((expr) => expr.node_id === candidate.node_id && expr.source_span)
            .map((expr) => expr.source_span)
        ).filter(Boolean);
        for (const span of spans) {
          const offset = diag.source_span.start;
          const distance = offset < span.start ? span.start - offset : offset > span.end ? offset - span.end : 0;
          if (!nearest || distance < nearest.distance || (distance === nearest.distance && span.start < nearest.start)) {
            nearest = { node_id: candidate.node_id, distance, start: span.start };
          }
        }
      }
      return nearest && nearest.node_id === node.node_id;
    });
    return direct.concat(fallback);
  }

  function worstDiagnosticSeverity(entries) {
    return entries.some((entry) => entry.severity === "error") ? "error" : "warning";
  }

  function diagnosticDetailDescriptors(entries) {
    return entries.map((entry, index) => {
      const loc = entry.line ? `line ${entry.line}, column ${entry.column || 1}` : (entry.source || "source");
      return {
        key: entry.id || String(index),
        label: entry.code,
        value: entry.what,
        detail: loc + " · " + (entry.fix || ""),
        fullText: diagnosticFullText(entry),
        severity: entry.severity,
        layout: "diagnostic",
        rowAttributes: { "data-problem-index": index },
        valueAttributes: { "data-problem-jump": index },
        editable: false,
        apply_op: {
          id: "diagnostic-jump:" + (entry.id || index),
          mode: "action",
          run: () => jumpToDiagnostic(entry)
        },
        actions: [{
          label: "Dismiss",
          attributes: { "data-problem-dismiss": index },
          run: () => {
            diagnosticsState.dismissed.add(entry.id);
            renderProblemsPanel();
            if (latestDoc) drawGraph(latestDoc);
          }
        }]
      };
    });
  }

  function renderProblemsPanel() {
    const entries = activeDiagnostics();
    if (problemsCount) problemsCount.textContent = entries.length ? String(entries.length) : "0";
    if (!problemsList) return;
    clearDom(problemsList);
    if (!entries.length) {
      appendText(problemsList, "div", "problem-empty", "No problems");
      window.__jetCanvasProblems = [];
      return;
    }
    renderFieldDescriptors(problemsList, diagnosticDetailDescriptors(entries), { fieldsClass: "problem-list" });
    window.__jetCanvasProblems = entries.map((entry) => ({ code: entry.code, what: entry.what, severity: entry.severity, source_span: entry.source_span, rendered: diagnosticFullText(entry) }));
  }

  function jumpToDiagnostic(entry) {
    if (!entry) return;
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph && entry.source_span ? (graph.nodes || []).find((n) => n.source_span && spansOverlap(n.source_span, entry.source_span)) : null;
    if (node) {
      selectedGraphId = graph.graph_id;
      selectedNodeId = node.node_id;
      selectedNodeIds = new Set([node.node_id]);
      centerNodeInView(graph, node);
      setViewMode("graph");
      if (latestDoc) drawGraph(latestDoc);
    } else if (entry.source_span) {
      setSourceHash(entry.source_span);
      setViewMode("code");
    }
  }

  function centerNodeInView(graph, node) {
    const rect = canvas.getBoundingClientRect();
    const size = nodeSize(graph, node);
    view.x = rect.width / 2 - (nodeX(node) + size.w / 2) * view.zoom;
    view.y = rect.height / 2 - (nodeY(node) + size.h / 2) * view.zoom;
  }

  function setPendingPin(pin) {
    pendingPin = pin;
    pendingPinContext = pin ? {
      graphId: selectedGraphId,
      revision: latestDoc && latestDoc.revision,
      sourceId: currentCanvasSourceId()
    } : null;
    window.__jetCanvasPendingPin = pin ? { pin_id: pin.pin_id, name: pin.name, type: pin.type, direction: pin.direction } : null;
    syncWireStatus(pin ? { title: "Source pin", detail: pinName(pin) + " : " + exactPinType(pin), color: colorForType(pin.type || "Value") } : null);
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
    if (latestDoc && !document.getElementById("execute-command-authority")) window.requestAnimationFrame(fitGraph);
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
    if (/Result|Error/.test(t)) return TYPE_COLOR_MAP.Result;
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
    if ((pin.type || "") === "exec" || pin.name === "exec" || pin.ability === "control") return "control";
    if (pin.fallible) return "fallible";
    if (pin.effect_grant_need) return "effect";
    return "data";
  }

  function setViewMode(mode) {
    viewMode = mode === "code" || mode === "split" || mode === "review" ? mode : "graph";
    if (viewMode === "graph" || viewMode === "review") setSourceEditMode(false);
    stage.classList.toggle("is-code", viewMode === "code");
    stage.classList.toggle("is-split", viewMode === "split");
    stage.classList.toggle("is-review", viewMode === "review");
    viewToggle.textContent = viewMode === "graph" ? "Code" : "Graph";
    viewToggle.classList.toggle("is-active", viewMode !== "graph");
    for (const button of lensButtons) {
      button.classList.toggle("is-active", button.getAttribute("data-view-mode") === viewMode);
    }
    sourceView.textContent = latestDoc && latestDoc.source_text ? latestDoc.source_text : "";
    if (sourceEditor && !sourceEditMode) {
      const draft = latestDoc ? readSourceDraft(latestDoc) : null;
      sourceEditor.value = draft !== null ? draft : (latestDoc && latestDoc.source_text ? latestDoc.source_text : "");
    }
    window.__jetCanvasLensMode = viewMode;
    if (viewMode === "review") loadReview();
    else if (viewMode !== "code" && latestDoc) drawGraph(latestDoc);
  }

  function setSourceEditMode(active) {
    sourceEditMode = !!active && !!sourceEditor;
    stage.classList.toggle("is-source-edit", sourceEditMode);
    if (sourceEditMode) {
      if (viewMode === "graph") viewMode = "split";
      stage.classList.toggle("is-code", viewMode === "code");
      stage.classList.toggle("is-split", viewMode === "split");
      if (sourceEditor) {
        const draft = latestDoc ? readSourceDraft(latestDoc) : null;
        if (latestDoc) sourceEditor.value = draft !== null ? draft : (latestDoc.source_text || "");
        setSaveState(draft !== null && latestDoc && draft !== latestDoc.source_text ? "local draft" : "source saved", draft !== null && latestDoc && draft !== latestDoc.source_text ? "draft" : "saved");
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

  function currentCanvasSourceId() {
    return selectedSourceId || (latestDoc && latestDoc.source_id) || null;
  }

  function syncSearchSpans() {
    const sourceId = currentCanvasSourceId();
    searchState.spans = (searchState.results || [])
      .filter((result) => !result.source_id || result.source_id === sourceId)
      .map((result) => result.source_span)
      .filter(Boolean);
  }

  function postQuery(body) {
    if (!latestDoc) return Promise.resolve(null);
    const querySourceId = currentCanvasSourceId();
    const queryRevision = latestDoc.revision;
    const queryProjectRevision = latestProject && latestProject.project_revision;
    const request = { schema_version: 1, revision: queryRevision };
    if (querySourceId) request.source_id = querySourceId;
    if (queryProjectRevision) request.project_revision = queryProjectRevision;
    return fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(Object.assign(request, body))
    })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) {
          searchState.stale = true;
          searchState.renamePlan = null;
          renderSearchResults();
          const stale = result.json && ["conflict", "stale"].includes(result.json.kind);
          const message = result.json && result.json.message || "Canvas query rejected";
          setCanvasState(
            stale ? "stale" : "error",
            stale ? "Search results are stale" : "Search unavailable",
            stale
              ? "The project changed. Canvas kept the current source and previous results; reload, then search again."
              : "Canvas kept the current source and previous results. Retry when the query service is available.",
            [
              { label: "Show source", run: openSourceRecovery },
              { label: "Retry", primary: true, run: () => postQuery(body) }
            ]
          );
          setSaveState("source unchanged", "error");
          showToast(message, { isError: true });
          return null;
        }
        if (latestDoc && (
          latestDoc.revision !== queryRevision
          || currentCanvasSourceId() !== querySourceId
          || (queryProjectRevision && (!latestProject || latestProject.project_revision !== queryProjectRevision))
          || (queryProjectRevision && result.json.project_revision !== queryProjectRevision)
        )) {
          searchState.stale = true;
          searchState.renamePlan = null;
          renderSearchResults();
          // A background query can finish after a source edit has already
          // moved the debugger into its own stale state. Keep that primary
          // source/debug surface visible; the search rail still marks its
          // result stale and the toast names the discarded response.
          if (window.__jetCanvasDebugState?.state !== "stale") {
            setCanvasState("stale", "Search results are stale", "The source or project changed while Canvas searched. Previous results stay visible; search again for the current revision.", [
              { label: "Show source", run: openSourceRecovery },
              { label: "Retry", primary: true, run: () => postQuery(body) }
            ]);
            setSaveState("source unchanged", "error");
          }
          showToast("Canvas query result is stale; reload the current source", { isError: true });
          return null;
        }
        searchState.results = result.json.results || [];
        searchState.stale = false;
        syncSearchSpans();
        searchState.active = searchState.results.length ? 0 : -1;
        searchState.diff = result.json.diff || null;
        searchState.renamePlan = result.json.op === "preview_rename"
          && result.json.diff
          && Array.isArray(result.json.diff.files)
          ? result.json.diff
          : null;
        searchState.impact = result.json.impact || null;
        searchState.truncated = result.json.truncated === true;
        searchState.resultLimit = result.json.result_limit || 0;
        if (window.__jetCanvasCanvasState && ["stale", "offline", "error"].includes(window.__jetCanvasCanvasState.kind)) {
          clearCanvasState();
        }
        renderSearchResults();
        if (searchState.results[0]) selectQueryResult(searchState.results[0], false);
        if (latestDoc) drawGraph(latestDoc);
        return result.json;
      })
      .catch((e) => {
        searchState.stale = true;
        renderSearchResults();
        const offline = navigator.onLine === false;
        setCanvasState(
          offline ? "offline" : "error",
          offline ? "Search is offline" : "Search unavailable",
          "Canvas kept the current source and previous results. Reconnect or retry when the query service is available.",
          [
            { label: "Show source", run: openSourceRecovery },
            { label: "Retry", primary: true, run: () => postQuery(body) }
          ]
        );
        setSaveState("source unchanged", "error");
        showToast(String(e), { isError: true });
        return null;
      });
  }

  function renderSearchResults() {
    const allResults = searchState.results || [];
    const visibleResults = allResults.slice(0, 24);
    const rows = visibleResults.map((result, i) => {
      const active = i === searchState.active ? " is-active" : "";
      const label = escapeHtml(result.title || result.symbol || result.kind || "match");
      const source = result.source_id ? `${result.source_id} · ` : "";
      const where = `${source}${result.kind || "match"} · line ${result.line || "?"}`;
      return `<button class="search-item${active}" data-search-hit="${i}">${label}<small>${escapeHtml(where)} ${escapeHtml(result.excerpt || "")}</small></button>`;
    }).join("");
    const limit = searchState.truncated || allResults.length > visibleResults.length
      ? `<div class="tag">Showing first ${visibleResults.length} of ${searchState.resultLimit || allResults.length} results; narrow search</div>`
      : "";
    const stale = searchState.stale
      ? `<div class="tag">Search results are stale; reload the current source and search again.</div>`
      : "";
    const diff = searchState.diff && searchState.diff.text ? `<div class="inline-row"><b>Preview diff</b><code>${escapeHtml(searchState.diff.text)}</code></div>` : "";
    const renameFiles = searchState.renamePlan && Array.isArray(searchState.renamePlan.files)
      ? `<div class="tag">Rename preview covers ${searchState.renamePlan.files.length} source file${searchState.renamePlan.files.length === 1 ? "" : "s"}; Apply commits them as one project transaction.</div>`
      : "";
    const impact = searchState.impact && searchState.impact.found ? `<div class="pin-row"><b>Impact</b><br><span class="tag">${(searchState.impact.references || []).length} refs / ${(searchState.impact.call_sites || []).length} calls</span></div>` : "";
    searchResults.innerHTML = stale + (rows || diff || renameFiles || impact || limit ? limit + rows + renameFiles + diff + impact : "<div class=\"tag\">no matches</div>");
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

  function projectPathKey(path) {
    return String(path || "").replace(/\\/g, "/").replace(/^\.\/+/, "");
  }

  function sourceControlFile(doc, sourceId) {
    const wanted = projectPathKey(sourceId || (latestDoc && latestDoc.source_id));
    if (!wanted) return null;
    return (doc && doc.files || []).find((file) => projectPathKey(file.path) === wanted) || null;
  }

  function sourceControlRevision(doc, sourceId) {
    const file = sourceControlFile(doc, sourceId);
    return file ? file.revision : (sourceId ? null : (doc && doc.revision));
  }

  function loadSourceControl() {
    const loadToken = ++sourceControlLoadToken;
    const requestedSourceId = currentCanvasSourceId();
    const requestedRevision = latestDoc && latestDoc.revision;
    const requestedProjectRevision = latestProject && latestProject.project_revision;
    return fetch(sourceControlUrl, { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        if (loadToken !== sourceControlLoadToken) return null;
        const sourceRevision = sourceControlRevision(doc, requestedSourceId);
        const stale = currentCanvasSourceId() !== requestedSourceId
          || (latestProject && latestProject.project_revision !== requestedProjectRevision)
          || (requestedRevision && sourceRevision !== requestedRevision);
        if (stale) {
          scm = null;
          scmState.textContent = "git stale";
          scmState.style.color = "#f8c76a";
          if (latestDoc && !(latestDoc.graphs && latestDoc.graphs.length)
            && sourceIsTeachingEmpty(latestDoc.source_text)) {
            setTeachingEmptyCanvasState();
            return null;
          }
          setCanvasState("stale", "Source control is stale", "The source changed while Canvas read Git state. The current source stays visible; reload source control.", [
            { label: "Open source", run: openSourceRecovery },
            { label: "Retry", primary: true, run: loadSourceControl }
          ]);
          return null;
        }
        scm = doc;
        const dirtyCount = doc.dirty_files ? " · " + doc.dirty_files + " files" : "";
        scmState.textContent = doc.available ? (doc.dirty ? "git dirty" + dirtyCount : "git clean") : "no git";
        scmState.style.color = doc.dirty ? "#fde68a" : "#8fb2dc";
        if (typeof syncReviewFromSourceControl === "function") syncReviewFromSourceControl(doc);
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
    const requestedSourceId = currentCanvasSourceId();
    const requestedRevision = latestDoc && latestDoc.revision;
    return fetch(proofRequestUrl(requestedSourceId), { cache: "no-store" })
      .then((r) => r.json())
      .then((doc) => {
        if (currentCanvasSourceId() !== requestedSourceId || (latestDoc && latestDoc.revision !== requestedRevision) || (doc.revision && doc.revision !== requestedRevision)) {
          proofState.textContent = "stale — reload";
          proofState.style.color = "#f8c76a";
          return null;
        }
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
      const file = sourceControlFile(doc, currentCanvasSourceId());
      const fileDiff = file && file.dirty ? (file.diff || file.status || file.path) : "";
      const diff = file
        ? (fileDiff || "clean")
        : (doc.diff || (doc.dirty ? doc.status : "clean"));
      searchState.results = [];
      searchState.spans = [];
      searchState.active = -1;
      searchState.impact = null;
      searchState.stale = false;
      searchState.diff = { text: diff || "clean" };
      renderSearchResults();
      showToast(file ? (file.dirty ? "Source diff loaded" : "Source clean") : (doc.dirty ? "Source diff loaded" : "Source clean"));
    };
    const currentRevision = latestDoc && latestDoc.revision;
    if (scm && (!currentRevision || sourceControlRevision(scm, currentCanvasSourceId()) === currentRevision)) {
      render(scm);
    } else {
      scm = null;
      loadSourceControl().then(render);
    }
  }

  function selectQueryResult(result, fitView) {
    if (!result) return;
    if (searchState.stale) {
      showToast("Search result is stale; search again for the current revision", { isError: true });
      return;
    }
    const sourceId = currentCanvasSourceId();
    if (result.source_id && result.source_id !== sourceId) {
      const results = searchState.results;
      const active = searchState.active;
      const truncated = searchState.truncated;
      const resultLimit = searchState.resultLimit;
      return loadGraph(result.source_id).then(() => {
        if (!latestDoc || latestDoc.revision !== result.revision) {
          showToast("Search result is stale; reload the project");
          return;
        }
        searchState.results = results;
        searchState.active = active;
        searchState.truncated = truncated;
        searchState.resultLimit = resultLimit;
        syncSearchSpans();
        selectQueryResult(result, fitView);
        renderSearchResults();
        drawGraph(latestDoc);
      });
    }
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
      searchState = { results: [], spans: [], active: -1, diff: null, impact: null, renamePlan: null, truncated: false, resultLimit: 0, stale: false };
      renderSearchResults();
      if (latestDoc) drawGraph(latestDoc);
      return;
    }
    postQuery({ op: "project_search", query });
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

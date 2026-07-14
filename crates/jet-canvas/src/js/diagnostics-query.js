
// Toasts, diagnostics, drawers, source control, proof data, search, and viewport state.
  function showToast(text, opts = {}) {
    const message = String(text || "");
    const isError = !!opts.isError || /^Error \[[A-Z0-9]+\]:/.test(message) || message.includes("\n Why:") || message.includes("\n Fix:");
    if (opts.showDetails) {
      toast.innerHTML = `<span>${escapeHtml(message)}</span><button type="button" data-show-problems>Show details</button>`;
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
    const span = diag.source_span || null;
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
      rendered: diag.rendered || "",
      source_span: span,
      line: span && span.line,
      column: span && span.column
    };
  }

  function activeDiagnostics() {
    return diagnosticsState.entries.filter((entry) => !diagnosticsState.dismissed.has(entry.id));
  }

  function acceptDiagnosticsPayload(payload, source) {
    if (!payload) return false;
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
      showToast(`${entries.length} ${entries.length === 1 ? "problem" : "problems"}: ${first.code} ${first.what}`, { isError: true, showDetails: true });
    } else {
      showToast("Check passed");
    }
    if (latestDoc) drawGraph(latestDoc);
    return entries.length > 0;
  }

  function clearDiagnosticsForRevision(revision) {
    if (!diagnosticsState.entries.length) return;
    diagnosticsState.entries = [];
    diagnosticsState.dismissed = new Set();
    diagnosticsState.baseRevision = revision || null;
    diagnosticsState.diagnosticRevision = revision || null;
    renderProblemsPanel();
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
    return activeDiagnostics().filter((diag) => diag.source_span && spansOverlap(node.source_span, diag.source_span));
  }

  function worstDiagnosticSeverity(entries) {
    return entries.some((entry) => entry.severity === "error") ? "error" : "warning";
  }

  function renderProblemsPanel() {
    const entries = activeDiagnostics();
    if (problemsCount) problemsCount.textContent = entries.length ? String(entries.length) : "0";
    if (!problemsList) return;
    if (!entries.length) {
      problemsList.innerHTML = "<div class=\"problem-empty\">No problems</div>";
      window.__jetCanvasProblems = [];
      return;
    }
    problemsList.innerHTML = entries.map((entry, i) => {
      const loc = entry.line ? `line ${entry.line}, column ${entry.column || 1}` : (entry.source || "source");
      return `<div class="problem-row" data-problem-index="${i}" data-severity="${escapeAttr(entry.severity)}"><b>${escapeHtml(entry.code)}</b><button type="button" data-problem-jump="${i}">${escapeHtml(entry.what)}</button><button type="button" data-problem-dismiss="${i}">Dismiss</button><small>${escapeHtml(loc)} · ${escapeHtml(entry.fix || "")}</small><pre class="problem-detail">${escapeHtml(diagnosticFullText(entry))}</pre></div>`;
    }).join("");
    problemsList.querySelectorAll("[data-problem-jump]").forEach((button) => {
      button.addEventListener("click", () => jumpToDiagnostic(entries[Number(button.getAttribute("data-problem-jump"))]));
    });
    problemsList.querySelectorAll("[data-problem-dismiss]").forEach((button) => {
      button.addEventListener("click", () => {
        const entry = entries[Number(button.getAttribute("data-problem-dismiss"))];
        if (entry) diagnosticsState.dismissed.add(entry.id);
        renderProblemsPanel();
        if (latestDoc) drawGraph(latestDoc);
      });
    });
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

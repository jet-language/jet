
// Function-pin editing, transactions, graph loading, and action catalog loading.
  let canvasActionsLoading = null;
  let canvasActionsLoadingRevision = null;
  let actionEntriesRevision = null;
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
    const inputs = (item.pins || []).filter((p) => p.direction === "input");
    if (!inputs.length) return existing;
    const graph = currentGraph(latestDoc);
    if (pin.direction === "output") {
      const expr = sourceExprForOutputPin(pin);
      if (expr) {
        if (inputs.some((p) => compatibleActionType(p.type, pin.type))) return [expr].concat(existing.slice(1));
      }
    }
    return existing.length ? existing : ["1"];
  }

  function wiredArgsForAction(item, pin) {
    const args = defaultArgsForAction(item, pin).slice();
    if (!pin || pin.direction !== "output" || isExecPin(pin)) return args;
    const expr = sourceExprForOutputPin(pin);
    if (!expr) return args;
    const inputs = (item.pins || []).filter((p) => p.direction === "input");
    const index = inputs.findIndex((p) => compatibleActionType(p.type, pin.type));
    if (index >= 0) {
      while (args.length <= index) args.push(default_arg_for_pin_js(inputs[args.length]));
      args[index] = expr;
    }
    return args;
  }

  function default_arg_for_pin_js(pin) {
    const t = String(pin && pin.type || "Int");
    if (t === "String" || t === "Char") return "\"\"";
    if (t === "Bool") return "false";
    if (t === "Float" || t === "F32" || t === "F64") return "0.0";
    return "1";
  }

  function callExpressionForAction(item, pin) {
    const callee = item.insert_callee || item.callee || item.title;
    const args = wiredArgsForAction(item, pin);
    return callee + "(" + args.join(", ") + ")";
  }

  function wireTargetForAction(item, pin) {
    if (!pin) return null;
    if (isExecPin(pin)) return { pin: pin.direction === "output" ? "exec" : "then", expr: null };
    if (pin.direction === "output") {
      const input = (item.pins || []).filter((p) => p.direction === "input").find((p) => compatibleActionType(p.type, pin.type));
      return { pin: input && input.name || "arg1", expr: sourceExprForOutputPin(pin) };
    }
    const ret = actionReturnType(item) || item.ret || "Value";
    if (compatibleActionType(pin.type, ret)) return { pin: "result", expr: null };
    return null;
  }

  function transactionForPaletteInsert(item, pin, graphPoint) {
    const descriptor = nodeDescriptorForAction(item);
    if (!descriptor || descriptor.transaction !== "insert_call") return null;
    const baseCallee = item.insert_callee || item.callee || (item.op === "insert_print" ? "print" : null);
    if (!baseCallee) return null;
    const target = wireTargetForAction(item, pin);
    const graph = currentGraph(latestDoc);
    const args = wiredArgsForAction(item, pin);
    const receiverIndex = (item.pins || []).filter((p) => p.direction === "input").findIndex((p) => p.name === "receiver");
    const receiverExpr = item.receiver_type && receiverIndex === 0 && target && target.pin === "receiver" && target.expr;
    if (item.receiver_type && !receiverExpr) return null;
    const methodName = String(baseCallee).split(".").pop();
    const callee = receiverExpr ? receiverExpr + "." + methodName : baseCallee;
    const body = { schema_version: 1, op: descriptor.transaction, revision: latestDoc.revision, graph_id: selectedGraphId, callee, args: receiverExpr ? args.slice(1) : args };
    const ret = actionReturnType(item) || item.ret || "Void";
    if ((!pin || isExecPin(pin)) && ret && ret !== "Void") body.bind = "canvas_value";
    if (pin && target) {
      body.wire_origin_pin_id = pin.pin_id;
      body.wire_target_pin = target.pin;
      if (target.expr) body.wire_expr = target.expr;
      if (pin.direction === "input") {
        const inline = inlineForPin(graph, pin);
        if (inline) body.wire_inline_expr_id = inline.inline_expr_id;
      }
    }
    if (graphPoint) {
      pendingInsertPlacement = { graph_id: selectedGraphId, title: callee, x: graphPoint.x, y: graphPoint.y };
    }
    return body;
  }

  function runPalette(item, pinContext) {
    if (!latestDoc || !selectedGraphId) return;
    const availability = actionAvailability(item);
    if (!availability.available) return showToast(availability.reason);
    const pin = pinContext || (contextMenuState && contextMenuState.pin) || null;
    const graphPoint = contextMenuState && contextMenuState.graphPoint || null;
    const descriptor = nodeDescriptorForAction(item);
    if ((item.stageable || !pin) && actionInsertsNode(item) && item.op !== "preview_canvas_action") {
      if (item.op === "insert_call" && !item.insert_callee && !item.callee) {
        const callee = window.prompt("Call function", "print");
        if (!callee) return;
        return createStagedNodeFromAction(Object.assign({}, item, { title: callee, callee, insert_callee: callee, args: ["\"canvas\""] }), graphPoint || viewportCenterGraphPoint());
      }
      return createStagedNodeFromAction(item, graphPoint || viewportCenterGraphPoint());
    }
    if (item.kind === "canvas.core_catalog") {
      postTransaction(transactionForPaletteInsert(item, pin, graphPoint));
    } else if (item.kind === "project_function") {
      postTransaction(transactionForPaletteInsert(item, pin, graphPoint));
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
      postTransaction(transactionForPaletteInsert(Object.assign({}, item, { callee: "print" }), pin, graphPoint));
    } else if (item.op === "insert_call") {
      const callee = item.insert_callee || item.callee || window.prompt("Call function", "print");
      if (callee) postTransaction(transactionForPaletteInsert(Object.assign({}, item.insert_callee || item.callee ? item : { args: ["\"canvas\""] }, { callee }), pin, graphPoint));
    } else if (descriptor && descriptor.transaction && descriptor.transaction !== "insert_call" && descriptor.transaction !== "edit_inline_expr") {
      postTransaction({ schema_version: 1, op: descriptor.transaction, revision: latestDoc.revision, graph_id: selectedGraphId });
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
    if (!body) return showToast("Action needs a source transaction");
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    const beforeSource = latestDoc && latestDoc.source_text;
    setSaveState("saving", "draft");
    window.__jetCanvasLastTx = body;
    window.__jetCanvasLastTxResult = null;
    return fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        if (!result.ok) {
          window.__jetCanvasLastTxResult = result.json;
          if (!acceptDiagnosticsPayload(result.json, "Transaction")) {
            setCanvasState("error", "Edit not saved", "Jet source stayed unchanged. Review the request, then retry or open Code.", [
              { label: "Open source", run: openSourceRecovery },
              { label: "Retry", primary: true, run: () => postTransaction(body) }
            ]);
            setSaveState("source unchanged", "error");
            showToast(result.json.message || "Edit rejected", { isError: true });
          }
          return;
        }
        if (result.json.protocol === "jet.canvas.action") {
          window.__jetCanvasLastTxResult = result.json;
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.impact = null;
          searchState.diff = { text: result.json.diff || "clean" };
          renderSearchResults();
          showToast("Canvas action preview validated");
          return;
        }
        if (result.json.changed && beforeSource && result.json.source_text) {
          recordUndoEntry(body, beforeSource, result.json.source_text);
          searchState.diff = { text: "source changed by " + transactionUndoLabel(body) };
          renderSearchResults();
        }
        if (latestDoc && result.json.revision) {
          latestDoc = Object.assign({}, latestDoc, {
            revision: result.json.revision,
            source_text: result.json.source_text || latestDoc.source_text
          });
        }
        if (body.op === "replace_source" && body.source_edit) setSourceEditMode(false);
        if (result.json.changed) clearSourceDraft();
        clearDiagnosticsForRevision(result.json.revision);
        showToast(result.json.changed ? "Source updated" : "No change");
        return loadGraph().then(() => {
          window.__jetCanvasLastTxResult = result.json;
        });
      })
      .catch((e) => {
        setCanvasState(navigator.onLine === false ? "offline" : "error", navigator.onLine === false ? "Offline" : "Edit failed", "Jet source was not changed. Keep the source visible, then retry when the connection is ready.", [
          { label: "Open source", run: openSourceRecovery },
          { label: "Retry", primary: true, run: () => postTransaction(body) }
        ]);
        setSaveState("source unchanged", "error");
        showToast(String(e), { isError: true });
      });
  }

  function restoreSource(source, redoEntry, undoEntry, action) {
    if (!latestDoc || !source) return;
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    window.__jetCanvasLastTx = { schema_version: 1, op: "replace_source", revision: latestDoc.revision, source, undo_restore: action || "restore" };
    window.__jetCanvasLastTxResult = null;
    return fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ schema_version: 1, op: "replace_source", revision: latestDoc.revision, source, undo_restore: action || "restore" }) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: j })))
      .then((result) => {
        window.__jetCanvasLastTxResult = result.json;
        if (!result.ok) {
          if (!acceptDiagnosticsPayload(result.json, "Undo")) showToast(result.json.message || "Undo rejected");
          return;
        }
        if (latestDoc && result.json.revision) {
          latestDoc = Object.assign({}, latestDoc, {
            revision: result.json.revision,
            source_text: result.json.source_text || latestDoc.source_text
          });
        }
        clearSourceDraft();
        if (redoEntry) pushHistory(redoStack, redoEntry);
        if (undoEntry) pushHistory(undoStack, undoEntry);
        clearDiagnosticsForRevision(result.json.revision);
        showToast((action || "Restore") + ": " + ((redoEntry || undoEntry || {}).label || "source"));
        loadSourceControl();
        return loadGraph().then(() => {
          window.__jetCanvasLastTxResult = result.json;
        });
      })
      .catch((e) => {
        setCanvasState(navigator.onLine === false ? "offline" : "error", navigator.onLine === false ? "Offline" : "Restore failed", "Jet source was not changed. Retry when the connection is ready.", [
          { label: "Open source", run: openSourceRecovery },
          { label: "Retry", primary: true, run: () => restoreSource(source, redoEntry, undoEntry, action) }
        ]);
        setSaveState("source unchanged", "error");
        showToast(String(e), { isError: true });
      });
  }

  function undoTransaction() {
    const entry = undoStack.pop();
    if (!entry) return showToast("Nothing to undo");
    return restoreSource(entry.before, entry, null, "Undo");
  }

  function redoTransaction() {
    const entry = redoStack.pop();
    if (!entry) return showToast("Nothing to redo");
    return restoreSource(entry.after, null, entry, "Redo");
  }

  function graphRequestUrl(sourceId) {
    if (!sourceId) return graphUrl;
    return graphUrl + (graphUrl.includes("?") ? "&" : "?") + "source_id=" + encodeURIComponent(sourceId);
  }

  function loadGraph(sourceId) {
    const loadToken = (window.__jetCanvasGraphLoadGeneration || 0) + 1;
    window.__jetCanvasGraphLoadGeneration = loadToken;
    const requestedSourceId = typeof sourceId === "string" ? (sourceId || null) : selectedSourceId;
    const previousSourceId = selectedSourceId;
    setCanvasState("loading", "Opening Canvas", "Reading Jet source and rebuilding the source-backed graph…", latestDoc ? [
      { label: "Show source", run: openSourceRecovery },
      { label: "Retry", primary: true, run: () => loadGraph(sourceId) }
    ] : [
      { label: "Retry", primary: true, run: () => loadGraph(sourceId) }
    ]);
    setSaveState("loading", "draft");
    return fetch(graphRequestUrl(requestedSourceId), { cache: "no-store" })
      .then((r) => r.json().then((doc) => ({ ok: r.ok, doc })))
      .then((result) => {
        if (loadToken !== window.__jetCanvasGraphLoadGeneration) return;
        if (!result.ok) {
          const hasDiagnostics = acceptDiagnosticsPayload(result.doc, "Graph");
          jump.textContent = "Canvas graph has problems";
          details.textContent = result.doc && result.doc.message || "Graph check failed";
          if (!hasDiagnostics) setCanvasState("error", "Canvas could not open", "The last source remains available. Fix the request or retry the graph projection.", [
            { label: "Open source", run: openSourceRecovery },
            { label: "Retry", primary: true, run: () => loadGraph(sourceId) }
          ]);
          setSaveState("source unchanged", "error");
          return;
        }
        const doc = result.doc;
        const sourceChanged = previousSourceId !== requestedSourceId;
        if (sourceChanged) {
          selectedSourceId = requestedSourceId;
          selectedVariableName = null;
          graphBackStack = [];
          graphForwardStack = [];
          selectedGraphId = null;
          selectedNodeId = null;
          selectedNodeIds = new Set();
          debugOverlay = null;
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.diff = null;
          searchState.impact = null;
          searchState.truncated = false;
          searchState.resultLimit = 0;
          if (typeof renderSearchResults === "function") renderSearchResults();
        }
        latestDoc = doc;
        if (sourceChanged) clearDiagnosticsForRevision(doc.revision);
        else clearStaleDiagnostics(doc);
        loadEditorState(doc);
        loadDetailToggles(doc);
        applyPendingInsertPlacement(doc);
        sourceView.textContent = doc.source_text || "";
        const firstLoad = selectedGraphId === null;
        drawGraph(doc);
        if (doc.graphs && doc.graphs.length) {
          clearCanvasState();
        } else {
          setCanvasState("empty", "No functions yet", "Add fn run() in Jet source. Canvas will project the source here; no graph file is created.", [
            { label: "Open source", run: openSourceRecovery },
          { label: "Reload", primary: true, run: () => loadGraph(requestedSourceId) }
          ]);
        }
        const draft = readSourceDraft(doc);
        setSaveState(draft !== null && draft !== doc.source_text ? "local draft" : "source saved", draft !== null && draft !== doc.source_text ? "draft" : "saved");
        setViewMode(viewMode);
        loadProject();
        loadSourceControl();
        loadProofRail();
        loadCanvasActions();
        applySourceHash();
        if (firstLoad) fitGraph();
      })
      .catch((e) => {
        const offline = navigator.onLine === false;
        jump.textContent = offline ? "Canvas is offline" : "Canvas graph failed";
        details.textContent = String(e);
        setCanvasState(offline ? "offline" : "error", offline ? "Offline" : "Canvas could not load", offline ? "Jet source stays visible. Reconnect, then retry the graph." : "Jet source stays visible. Retry the graph projection when the server is ready.", [
          { label: "Show source", run: openSourceRecovery },
          { label: "Retry", primary: true, run: () => loadGraph(requestedSourceId) }
        ]);
        setSaveState("source unchanged", "error");
      });
  }

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" }[c]));
  }

  function escapeAttr(s) {
    return escapeHtml(s).replace(/`/g, "&#96;");
  }

  function loadCanvasActions() {
    if (!latestDoc) return Promise.resolve(actionEntries);
    const loadRevision = latestDoc.revision;
    const loadSourceId = latestDoc.source_id || selectedSourceId;
    if (canvasActionsLoading) {
      if (canvasActionsLoadingRevision === loadRevision) return canvasActionsLoading;
      return canvasActionsLoading.then(() => loadCanvasActions());
    }
    canvasActionsLoadingRevision = loadRevision;
    canvasActionsLoading = fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(Object.assign({ schema_version: 1, revision: loadRevision, op: "actions" }, loadSourceId ? { source_id: loadSourceId } : {}))
    })
      .then((r) => r.json())
      .then((doc) => {
        if (!latestDoc || latestDoc.revision !== loadRevision || (latestDoc.source_id || selectedSourceId) !== loadSourceId) return actionEntries;
        if (!doc || !doc.actions) return;
        const projectFunctions = (doc.project_functions || []).map((fn) => withNodeDescriptor({
          title: fn.name || fn.callee,
          detail: (fn.module_path || "project") + " · " + (fn.signature || fn.callee || ""),
          kind: "project_function",
          node_descriptor_id: fn.node_descriptor_id,
          group: "Project",
          op: fn.insert_op || "insert_call",
          callee: fn.callee || fn.name,
          insert_callee: fn.insert_callee || fn.callee || fn.name,
          rank: Number(fn.rank || 0),
          rank_terms: fn.rank_terms || [],
          source_span: fn.source_span || null,
          module_path: fn.module_path || "project",
          action_id: "project:" + (fn.callee || fn.name),
          signature: fn.signature || "",
          pure: !!fn.pure,
          pins: fn.pins || [],
          ret: actionReturnType(fn) || fn.ret || "Void",
          available: fn.available !== false,
          stageable: !!fn.stageable,
          stage_reason_code: fn.stage_reason_code || "",
          stage_reason: fn.stage_reason || "",
          receiver_type: fn.receiver_type || "",
          denied_reason: fn.denied_reason || "",
          unavailable_reason_code: fn.unavailable_reason_code || "",
          args: fn.default_args || []
        }));
        const canvasActions = doc.actions.map((action) => withNodeDescriptor({
          title: action.title || action.callee,
          detail: action.kind === "canvas.core_catalog" ? ((action.module_path || "core") + " · " + (action.signature || action.callee || "") + " · read-only") : (action.command ? ((action.kind || "canvas.command") + " · " + (action.command || []).join(" ") + " · " + (action.writes || "none")) : ((action.kind || "canvas.action") + " · " + (action.engine || "checked-tir+jit") + " · " + (action.callee || "") + "(" + (action.pins || []).filter((p) => p.direction === "input").map((p) => p.type || "Value").join(", ") + ") -> " + (action.ret || "Void"))),
          kind: action.kind || "canvas.action",
          node_descriptor_id: action.node_descriptor_id,
          group: action.kind === "canvas.core_catalog" ? "Core" : action.kind === "canvas.command" ? "Commands" : action.kind === "canvas.structural" ? "Execution" : "Project",
          op: action.insert_op || action.op || (action.kind === "canvas.action" || action.kind === "canvas.core_catalog" ? "insert_call" : "preview_canvas_action"),
          action_id: action.action_id,
          callee: action.callee,
          module_path: action.module_path || "",
          insert_callee: action.insert_callee || action.callee,
          rank: Number(action.rank || 0),
          rank_terms: action.rank_terms || [],
          source_span: action.source_span || null,
          signature: action.signature || "",
          summary: action.summary || "",
          command: action.command || [],
          authority: action.authority || [],
          writes: action.writes || "none",
          requires_confirmation: !!action.requires_confirmation,
          available: action.available !== false,
          stageable: !!action.stageable,
          stage_reason_code: action.stage_reason_code || "",
          stage_reason: action.stage_reason || "",
          receiver_type: action.receiver_type || "",
          denied_reason: action.denied_reason || "",
          unavailable_reason_code: action.unavailable_reason_code || "",
          pins: action.pins || [],
          pure: !!action.pure,
          ret: action.ret || actionReturnType(action) || "Void",
          args: action.default_args || ["\"canvas\""]
        }));
        actionEntries = projectFunctions.concat(canvasActions);
        actionEntriesRevision = loadRevision;
        if (canvasActions.some((action) => action.kind === "canvas.core_catalog")) coreCatalogLoaded = true;
        if (latestDoc && latestDoc.revision === loadRevision) drawGraph(latestDoc);
        return actionEntries;
      })
      .catch(() => actionEntries)
      .finally(() => {
        canvasActionsLoading = null;
        canvasActionsLoadingRevision = null;
      });
    return canvasActionsLoading;
  }

  function mergeActionEntries(entries) {
    const seen = new Set(actionEntries.map((entry) => entry.action_id || entry.kind + ":" + entry.title + ":" + entry.module_path));
    for (const entry of entries) {
      const id = entry.action_id || entry.kind + ":" + entry.title + ":" + entry.module_path;
      if (seen.has(id)) continue;
      seen.add(id);
      actionEntries.push(withNodeDescriptor(entry));
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
            entries.push(withNodeDescriptor({
              title: member.name + " · " + (module.path || "core"),
              detail: (module.path || "core") + " · " + (member.signature || member.name),
              group: "Core",
              kind: "canvas.core_catalog",
              node_descriptor_id: member.node_descriptor_id,
              op: "insert_call",
              action_id: "canvas.core_catalog:" + (module.path || "core") + ":" + member.name,
              module_path: module.path || "core",
              callee,
              insert_callee: callee,
              rank: Number(member.rank || 0),
              rank_terms: member.rank_terms || [],
              signature: member.signature || "",
              summary: member.summary || module.summary || "",
              available: member.available !== false,
              stageable: !!member.stageable,
              stage_reason_code: member.stage_reason_code || "",
              stage_reason: member.stage_reason || "",
              receiver_type: member.receiver_type || "",
              denied_reason: member.denied_reason || "",
              unavailable_reason_code: member.unavailable_reason_code || "",
              pure: !!member.pure,
              pins: member.pins || [],
              ret: actionReturnType(member) || "Value",
              args: member.default_args || ["1"],
              default_args: member.default_args || ["1"]
            }));
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

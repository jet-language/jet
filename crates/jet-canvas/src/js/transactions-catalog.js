
// Function-pin editing, transactions, graph loading, and action catalog loading.
  let canvasActionsLoading = null;
  let canvasActionsLoadingRevision = null;
  let canvasActionsSkipRedraw = false;
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
    const inputs = (item.pins || []).filter((p) => p.direction === "input");
    if (!pin) return inputs.length ? (existing.length ? existing : ["\"canvas\""]) : existing;
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
    const callee = insertCalleeForAction(item);
    if (!callee) return null;
    const args = wiredArgsForAction(item, pin);
    return callee + "(" + args.join(", ") + ")";
  }

  function insertCalleeForAction(item) {
    const callee = item && item.insert_callee;
    return typeof callee === "string" && callee.trim() ? callee : null;
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
    const baseCallee = insertCalleeForAction(item);
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
    if (item.kind === "canvas.core_catalog" && typeof item.module_path === "string" && item.module_path.trim()) {
      body.module_path = item.module_path;
    }
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

  function transactionForStructuralInsert(item, pin) {
    const descriptor = nodeDescriptorForAction(item);
    if (!descriptor || !descriptor.transaction || descriptor.transaction === "insert_call") return null;
    const body = {
      schema_version: 1,
      op: descriptor.transaction,
      revision: latestDoc.revision,
      graph_id: selectedGraphId,
      wire_target_pin: "exec"
    };
    if (pin && pin.pin_id) body.wire_origin_pin_id = pin.pin_id;
    return body;
  }

  function runPalette(item, pinContext) {
    if (!latestDoc || !selectedGraphId) return;
    const availability = actionAvailability(item);
    if (!availability.available) return showToast(availability.reason);
    const pin = pinContext || (contextMenuState && contextMenuState.pin) || null;
    const graphPoint = contextMenuState && contextMenuState.graphPoint || null;
    const descriptor = nodeDescriptorForAction(item);
    if (descriptor && descriptor.transaction === "insert_call" && !insertCalleeForAction(item)) {
      showToast("Checked action has no source callee; source was not changed", { isError: true });
      return;
    }
    if ((item.stageable || !pin) && actionInsertsNode(item) && item.op !== "preview_canvas_action") {
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
        if (expr && insertCalleeForAction(item)) {
          const newExpr = callExpressionForAction(item, pin);
          if (newExpr) return postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: newExpr });
        }
      }
      const callee = insertCalleeForAction(item);
      if (!callee) return showToast("Checked action has no source callee; source was not changed", { isError: true });
      postTransaction({ schema_version: 1, op: "preview_canvas_action", revision: latestDoc.revision, graph_id: selectedGraphId, action_id: item.action_id, callee, args: defaultArgsForAction(item, pin) });
    } else if (item.op === "command_authority") {
      renderCommandAuthority(item);
    } else if (item.op === "insert_print") {
      postTransaction(transactionForPaletteInsert(Object.assign({}, item, { callee: "print", insert_callee: "print" }), pin, graphPoint));
    } else if (item.op === "insert_call") {
      postTransaction(transactionForPaletteInsert(item, pin, graphPoint));
    } else if (descriptor && descriptor.transaction && descriptor.transaction !== "insert_call" && descriptor.transaction !== "edit_inline_expr") {
      postTransaction(transactionForStructuralInsert(item, pin));
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

  function runLibraryAction(item) {
    if (!latestDoc || !selectedGraphId) return Promise.resolve({ ok: false, changed: false, code: "no_graph" });
    const availability = actionAvailability(item, currentGraphOrNull());
    if (!availability.available) {
      setCanvasState("permission", "Library entry unavailable", `${availability.reason} Source was not changed.`, [
        { label: "Open source", run: openSourceRecovery },
        { label: "Close", primary: true, run: clearCanvasState }
      ]);
      return Promise.resolve({ ok: false, changed: false, code: availability.code, message: availability.reason });
    }
    if (item.stageable) return runPalette(item);
    const body = transactionForPaletteInsert(item, null, viewportCenterGraphPoint());
    if (!body) {
      setCanvasState("error", "Library entry needs a compatible source action", "The checked action was kept visible. Source was not changed.", [
        { label: "Open source", run: openSourceRecovery },
        { label: "Close", primary: true, run: clearCanvasState }
      ]);
      return Promise.resolve({ ok: false, changed: false, code: "missing_insert_descriptor" });
    }
    return postTransaction(body);
  }

  function postTransaction(body) {
    if (!body) return showToast("Action needs a source transaction");
    const inlinePlan = typeof inlineEditPlan === "function" ? inlineEditPlan(body) : null;
    if (inlinePlan && !inlinePlan.ok) {
      const refusal = {
        ok: false,
        changed: false,
        reason: inlinePlan.label,
        message: inlinePlan.label,
        code: "client_type_gate"
      };
      window.__jetCanvasLastTx = null;
      window.__jetCanvasLastTxResult = refusal;
      showToast("Edit refused: " + inlinePlan.label, { isError: true });
      return Promise.resolve(refusal);
    }
    const projectRename = ["rename_binding", "rename_function"].includes(body.op)
      && latestProject
      && latestProject.project_revision;
    const beforeSource = latestDoc && latestDoc.source_text;
    const request = Object.assign({}, body);
    if (typeof canvasClientId === "function") request.client_id = canvasClientId();
    const sourceId = currentCanvasSourceId();
    if (!request.source_id && sourceId) request.source_id = sourceId;
    let txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    if (projectRename) {
      const plannedFiles = searchState.renamePlan && searchState.renamePlan.files;
      const files = Array.isArray(plannedFiles) && plannedFiles.length
        ? plannedFiles
        : (latestProject.files || []).filter((file) => file.kind === "source");
      request.project_revision = latestProject.project_revision;
      request.files = files.map((file) => ({ path: file.path, revision: file.revision }));
      txUrl = (window.__JET_CANVAS_BASE__ || "/canvas") + "/project/transaction";
    }
    setSaveState("saving", "draft");
    window.__jetCanvasLastTx = request;
    window.__jetCanvasLastTxResult = null;
    return fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: canvasPayload(j) })))
      .then((result) => {
        if (!result.ok) {
          window.__jetCanvasLastTxResult = result.json;
          const conflict = result.json && result.json.kind === "conflict";
          if (conflict) {
            const message = String(result.json.message || "Source changed while this Canvas edit was prepared");
            const recovery = "Selection is stale; copy the current selection again. " + message + ". Canvas kept the current source; reload before retrying.";
            setCanvasState("stale", "Edit not applied", recovery, [
              { label: "Show source", run: openSourceRecovery },
              { label: "Reload", primary: true, run: () => loadGraph() }
            ]);
            setSaveState("source unchanged", "error");
            if (body.op === "replace_source" && body.source_edit === "paste_clone") {
              pasteRenameChips = [];
              pasteRenameChipsExpanded = false;
            }
            showToast(recovery, { isError: true });
          } else if (!acceptDiagnosticsPayload(result.json, "Transaction")) {
            setCanvasState("error", "Edit not saved", "Jet source stayed unchanged. Review the request, then retry or open Code.", [
              { label: "Open source", run: openSourceRecovery },
              { label: "Retry", primary: true, run: () => postTransaction(body) }
            ]);
            setSaveState("source unchanged", "error");
            if (body.op === "replace_source" && body.source_edit === "paste_clone") {
              pasteRenameChips = [];
              pasteRenameChipsExpanded = false;
            }
            showToast(result.json.message || "Edit rejected", { isError: true });
          }
          return { ok: false, json: result.json };
        }
        if (result.json.protocol === "jet.canvas.action") {
          window.__jetCanvasLastTxResult = result.json;
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.impact = null;
          searchState.diff = { text: result.json.diff || "clean" };
          searchState.stale = false;
          renderSearchResults();
          showToast("Canvas action preview validated");
          return { ok: true, json: result.json };
        }
        if (result.json.protocol === "jet.canvas.project.edit") {
          window.__jetCanvasLastTxResult = result.json;
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.impact = null;
          searchState.renamePlan = null;
          searchState.stale = true;
          searchState.diff = result.json.diff ? { text: result.json.diff } : null;
          renderSearchResults();
          if (result.json.changed) clearSourceDraft();
          setSaveState(result.json.changed ? "project saved" : "project unchanged");
          showToast(result.json.changed ? "Project updated" : "No project change");
          return loadProject()
            .then(() => loadGraph(sourceId))
            .then(() => {
              window.__jetCanvasLastTxResult = result.json;
              return { ok: true, json: result.json };
            });
        }
        if (result.json.changed && typeof beforeSource === "string" && typeof result.json.source_text === "string") {
          recordUndoEntry(body, beforeSource, result.json.source_text);
          searchState.stale = true;
          searchState.diff = { text: "source changed by " + transactionUndoLabel(body) };
          renderSearchResults();
        }
        if (!(body.op === "replace_source" && body.source_edit === "paste_clone")) {
          pasteRenameChips = [];
          pasteRenameChipsExpanded = false;
        }
        if (latestDoc && result.json.revision) {
          if (result.json.revision !== latestDoc.revision) {
            releaseDebugSession(debugSessionSnapshot());
            debugRequestGeneration++;
            clearDebugClientState("stale");
          }
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
          return { ok: true, json: result.json };
        });
      })
      .catch((e) => {
        setCanvasState(navigator.onLine === false ? "offline" : "error", navigator.onLine === false ? "Offline" : "Edit failed", "Jet source was not changed. Keep the source visible, then retry when the connection is ready.", [
          { label: "Open source", run: openSourceRecovery },
          { label: "Retry", primary: true, run: () => postTransaction(body) }
        ]);
        setSaveState("source unchanged", "error");
        showToast(String(e), { isError: true });
        return { ok: false, error: e };
      });
  }

  function restoreSource(source, redoEntry, undoEntry, action) {
    if (!latestDoc || typeof source !== "string") return;
    const txUrl = window.__JET_CANVAS_TX__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/transaction");
    const request = { schema_version: 1, op: "replace_source", revision: latestDoc.revision, source, undo_restore: action || "restore" };
    if (typeof canvasClientId === "function") request.client_id = canvasClientId();
    const sourceId = currentCanvasSourceId();
    if (sourceId) request.source_id = sourceId;
    window.__jetCanvasLastTx = request;
    window.__jetCanvasLastTxResult = null;
    return fetch(txUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: canvasPayload(j) })))
      .then((result) => {
        window.__jetCanvasLastTxResult = result.json;
        if (!result.ok) {
          if (redoEntry) pushHistory(undoStack, redoEntry);
          if (undoEntry) pushHistory(redoStack, undoEntry);
          persistHistory();
          if (!acceptDiagnosticsPayload(result.json, action || "Restore")) {
            const message = String(result.json && result.json.message || ((action || "Restore") + " rejected"));
            const stale = result.json && result.json.kind === "conflict";
            setCanvasState(stale ? "stale" : "error", (action || "Restore") + " not applied", message + ". Canvas kept the current source and undo history. Reload before retry.", [
              { label: "Open source", run: openSourceRecovery },
              { label: "Reload", primary: true, run: () => loadGraph() }
            ]);
            setSaveState("source unchanged", "error");
            showToast(message, { isError: true });
            if (latestDoc) drawGraph(latestDoc);
          }
          return { ok: false, json: result.json };
        }
        if (latestDoc && result.json.revision) {
          if (result.json.revision !== latestDoc.revision) {
            releaseDebugSession(debugSessionSnapshot());
            debugRequestGeneration++;
            clearDebugClientState("stale");
          }
          latestDoc = Object.assign({}, latestDoc, {
            revision: result.json.revision,
            source_text: result.json.source_text || latestDoc.source_text
          });
        }
        clearSourceDraft();
        searchState.stale = true;
        renderSearchResults();
        pasteRenameChips = [];
        pasteRenameChipsExpanded = false;
        if (redoEntry) pushHistory(redoStack, redoEntry);
        if (undoEntry) pushHistory(undoStack, undoEntry);
        persistHistory();
        clearDiagnosticsForRevision(result.json.revision);
        showToast((action || "Restore") + ": " + ((redoEntry || undoEntry || {}).label || "source"));
        loadSourceControl();
        return loadGraph().then(() => {
          window.__jetCanvasLastTxResult = result.json;
        });
      })
      .catch((e) => {
        if (redoEntry) pushHistory(undoStack, redoEntry);
        if (undoEntry) pushHistory(redoStack, undoEntry);
        persistHistory();
        const failure = { ok: false, kind: "io", message: String(e) };
        window.__jetCanvasLastTxResult = failure;
        setCanvasState(navigator.onLine === false ? "offline" : "error", navigator.onLine === false ? "Offline" : "Restore failed", "Jet source was not changed. Undo history was preserved. Retry when the connection is ready.", [
          { label: "Open source", run: openSourceRecovery },
          { label: "Reload", primary: true, run: () => loadGraph() }
        ]);
        setSaveState("source unchanged", "error");
        showToast(String(e), { isError: true });
        if (latestDoc) drawGraph(latestDoc);
        return failure;
      });
  }

  function undoTransaction() {
    if (historyRequest) return historyRequest;
    const entry = undoStack.pop();
    if (!entry) return showToast("Nothing to undo");
    historyRequest = Promise.resolve(restoreSource(entry.before, entry, null, "Undo"))
      .finally(() => { historyRequest = null; });
    return historyRequest;
  }

  function redoTransaction() {
    if (historyRequest) return historyRequest;
    const entry = redoStack.pop();
    if (!entry) return showToast("Nothing to redo");
    historyRequest = Promise.resolve(restoreSource(entry.after, null, entry, "Redo"))
      .finally(() => { historyRequest = null; });
    return historyRequest;
  }

  function graphRequestUrl(sourceId) {
    if (!sourceId) return graphUrl;
    return graphUrl + (graphUrl.includes("?") ? "&" : "?") + "source_id=" + encodeURIComponent(sourceId);
  }

  function canvasSourceUrl(sourceId) {
    const base = window.__JET_CANVAS_BASE__ || "/canvas";
    const query = sourceId ? "?source_id=" + encodeURIComponent(sourceId) : "";
    return base + "/source" + query;
  }

  function sourceIsTeachingEmpty(source) {
    const withoutComments = String(source || "")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");
    return withoutComments.trim() === "";
  }

  function emptyCanvasGraphPayload(payload, source) {
    const message = String(payload && payload.message || "");
    return payload && payload.kind === "diagnostic"
      && message.includes("E0101")
      && message.includes("no `run` function")
      && sourceIsTeachingEmpty(source);
  }

  function currentSourceForEmptyGraph(sourceId) {
    return fetch(canvasSourceUrl(sourceId), { cache: "no-store" })
      .then((response) => response.ok ? response.text() : null)
      .catch(() => null);
  }

  function setTeachingEmptyCanvasState() {
    setCanvasState("empty", "No functions yet", "Add fn run() in Jet source. Canvas will project the source here; no graph file is created.", [
      { label: "Open source", run: openSourceRecovery },
      { label: "Reload", primary: true, run: () => loadGraph(currentCanvasSourceId()) }
    ]);
  }

  function preserveTeachingEmptyStateAfterSourceControl(sourceId, revision) {
    Promise.resolve(loadSourceControl()).then(() => {
      const doc = latestDoc;
      if (!doc || doc.source_id !== sourceId || doc.revision !== revision
        || (doc.graphs && doc.graphs.length) || !sourceIsTeachingEmpty(doc.source_text)) return;
      setTeachingEmptyCanvasState();
    });
  }

  function showEmptyCanvasGraph(payload, sourceId, sourceText) {
    const base = latestDoc || {
      protocol: "jet.canvas.graph",
      schema_version: 1,
      source_id: sourceId || "",
      revision: payload && payload.revision || "",
      node_descriptors: []
    };
    const emptySourceId = sourceId || base.source_id || "";
    drawGraph(Object.assign({}, base, {
      source_id: emptySourceId,
      revision: payload && payload.revision || base.revision || "",
      source_text: sourceText,
      graphs: []
    }));
    setTeachingEmptyCanvasState();
    setSaveState("source saved");
    setViewMode(viewMode);
    preserveTeachingEmptyStateAfterSourceControl(emptySourceId, payload && payload.revision || base.revision || "");
    return true;
  }

  function loadGraph(sourceId) {
    const loadToken = (window.__jetCanvasGraphLoadGeneration || 0) + 1;
    window.__jetCanvasGraphLoadGeneration = loadToken;
    const requestedSourceId = typeof sourceId === "string" ? (sourceId || null) : selectedSourceId;
    const previousSourceId = selectedSourceId;
    const previousRevision = latestDoc && latestDoc.revision;
    const previousDebugSession = debugSessionSnapshot();
    setCanvasState("loading", "Opening Canvas", "Reading Jet source and rebuilding the source-backed graph…", [
      { label: "Show source", run: openSourceRecovery },
      { label: "Retry", primary: true, run: () => loadGraph(sourceId) }
    ]);
    setSaveState("loading", "draft");
    return fetch(graphRequestUrl(requestedSourceId), { cache: "no-store" })
      .then((r) => r.text().then((body) => {
        let doc;
        try {
          doc = canvasPayload(JSON.parse(body));
        } catch (_) {
          doc = { protocol: "jet.canvas.query", ok: false, kind: "diagnostic", message: body };
        }
        return { ok: r.ok, doc };
      }))
      .then(async (result) => {
        if (loadToken !== window.__jetCanvasGraphLoadGeneration) return;
        const message = String(result.doc && result.doc.message || "");
        if (result.doc && result.doc.kind === "diagnostic"
          && message.includes("E0101")
          && message.includes("no `run` function")) {
          const source = await currentSourceForEmptyGraph(requestedSourceId);
          if (loadToken !== window.__jetCanvasGraphLoadGeneration) return;
          if (source !== null && emptyCanvasGraphPayload(result.doc, source)) {
            showEmptyCanvasGraph(result.doc, requestedSourceId, source);
            return;
          }
        }
        if (!result.ok) {
          const hasDiagnostics = acceptDiagnosticsPayload(result.doc, "Graph");
          jump.textContent = "Canvas graph has problems";
          const retained = canvasSession && (canvasSession.last_good_revision || canvasSession.last_good_program);
          if (retained) {
            window.__jetCanvasLastGoodGraph = {
              revision: canvasSession.last_good_revision || null,
              program: canvasSession.last_good_program || null,
              retained: true
            };
          }
          details.textContent = (result.doc && result.doc.message || "Graph check failed")
            + (retained ? " · last-good graph retained" : "");
          if (!hasDiagnostics) setCanvasState("error", "Canvas could not open", "The last source remains available. Fix the request or retry the graph projection.", [
            { label: "Open source", run: openSourceRecovery },
            { label: "Retry", primary: true, run: () => loadGraph(sourceId) }
          ]);
          setSaveState("source unchanged", "error");
          return;
        }
        const doc = result.doc;
        const firstLoad = !latestDoc;
        const sourceChanged = previousSourceId !== requestedSourceId;
        const revisionChanged = !!previousRevision && previousRevision !== doc.revision;
        if (sourceChanged || revisionChanged) releaseDebugSession(previousDebugSession);
        if (sourceChanged) {
          selectedSourceId = requestedSourceId;
          selectedVariableName = null;
          graphBackStack = [];
          graphForwardStack = [];
          selectedGraphId = null;
          selectedNodeId = null;
          selectedNodeIds = new Set();
          selectionExplicitlyCleared = false;
          debugRequestGeneration++;
          clearDebugClientState("stale");
          searchState.results = [];
          searchState.spans = [];
          searchState.active = -1;
          searchState.diff = null;
          searchState.impact = null;
          searchState.renamePlan = null;
          searchState.truncated = false;
          searchState.resultLimit = 0;
          searchState.stale = false;
          if (typeof renderSearchResults === "function") renderSearchResults();
        } else if (revisionChanged) {
          debugRequestGeneration++;
          clearDebugClientState("stale");
          searchState.stale = true;
          if (typeof renderSearchResults === "function") renderSearchResults();
        }
        latestDoc = doc;
        if (sourceChanged) clearDiagnosticsForRevision(doc.revision);
        else clearStaleDiagnostics(doc);
        loadEditorState(doc);
        loadHistory(doc);
        loadDetailToggles(doc);
        applyPendingInsertPlacement(doc);
        sourceView.textContent = doc.source_text || "";
        drawGraph(doc);
        if (doc.graphs && doc.graphs.length) {
          clearCanvasState();
        } else {
          setTeachingEmptyCanvasState();
        }
        const draft = readSourceDraft(doc);
        setSaveState(draft !== null && draft !== doc.source_text ? "local draft" : "source saved", draft !== null && draft !== doc.source_text ? "draft" : "saved");
        setViewMode(viewMode);
        loadProject();
        const sourceControlPromise = loadSourceControl();
        if (!(doc.graphs && doc.graphs.length) && sourceIsTeachingEmpty(doc.source_text)) {
          Promise.resolve(sourceControlPromise).then(() => {
            if (latestDoc && latestDoc.source_id === doc.source_id && latestDoc.revision === doc.revision
              && !(latestDoc.graphs && latestDoc.graphs.length) && sourceIsTeachingEmpty(latestDoc.source_text)) {
              setTeachingEmptyCanvasState();
            }
          });
        }
        loadProofRail();
        loadCanvasActions({ skipRedraw: firstLoad });
        applySourceHash();
        if (firstLoad) fitGraph(true);
      })
      .catch((e) => {
        const offline = navigator.onLine === false;
        jump.textContent = offline ? "Canvas is offline" : "Canvas graph failed";
        details.textContent = String(e) + (canvasSession && canvasSession.last_good_program ? " · last-good graph retained" : "");
        if (canvasSession && canvasSession.last_good_program) {
          window.__jetCanvasLastGoodGraph = {
            revision: canvasSession.last_good_revision || null,
            program: canvasSession.last_good_program,
            retained: true
          };
        }
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

  function loadCanvasActions(options = {}) {
    const skipRedraw = options.skipRedraw === true;
    if (!latestDoc) return Promise.resolve(actionEntries);
    const loadRevision = latestDoc.revision;
    const loadSourceId = currentCanvasSourceId();
    if (canvasActionsLoading) {
      if (skipRedraw) canvasActionsSkipRedraw = true;
      if (canvasActionsLoadingRevision === loadRevision) return canvasActionsLoading;
      return canvasActionsLoading.then(() => loadCanvasActions(options));
    }
    canvasActionsLoadingRevision = loadRevision;
    canvasActionsSkipRedraw = skipRedraw;
    canvasActionsLoading = fetch(queryUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(Object.assign({ schema_version: 1, revision: loadRevision, op: "actions" }, loadSourceId ? { source_id: loadSourceId } : {}))
    })
      .then((r) => r.json().then(canvasPayload))
      .then((doc) => {
        doc = canvasPayload(doc);
        if (!latestDoc || latestDoc.revision !== loadRevision || currentCanvasSourceId() !== loadSourceId) return actionEntries;
        if (!doc || !Array.isArray(doc.actions)) throw new Error("checked action query returned no actions");
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
          insert_callee: action.insert_callee,
          rank: Number(action.rank || 0),
          rank_terms: action.rank_terms || [],
          source_span: action.source_span || null,
          signature: action.signature || "",
          summary: action.summary || "",
          source: action.source || "",
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
          args: Array.isArray(action.default_args) ? action.default_args : [],
          default_args: Array.isArray(action.default_args) ? action.default_args : []
        }));
        // `project_functions` is the query's source metadata view. The
        // authoritative action view already contains those exports with
        // authority and descriptor-owned ranking facts; adding both creates
        // duplicate menu rows for every project function.
        actionEntries = canvasActions;
        actionEntriesRevision = loadRevision;
        if (canvasActions.some((action) => action.kind === "canvas.core_catalog")) coreCatalogLoaded = true;
        if (latestDoc && latestDoc.revision === loadRevision) syncLibraryPanel(latestDoc);
        if (latestDoc && latestDoc.revision === loadRevision && !document.getElementById("execute-command-authority") && !canvasActionsSkipRedraw) drawGraph(latestDoc);
        return actionEntries;
      })
      .catch((error) => {
        if (!latestDoc || latestDoc.revision !== loadRevision || currentCanvasSourceId() !== loadSourceId) return actionEntries;
        const offline = navigator.onLine === false;
        setCanvasState(offline ? "offline" : "error", offline ? "Offline" : "Checked actions unavailable", offline
          ? "Jet source and the current graph stay visible. Reconnect, then retry the checked action query."
          : "Jet source and the current graph stay visible. Retry the checked action query or open Code.", [
          { label: "Open source", run: openSourceRecovery },
          { label: "Retry", primary: true, run: () => loadCanvasActions(options) }
        ]);
        setSaveState("source unchanged", "error");
        showToast(error && error.message ? error.message : "Checked action query failed", { isError: true });
        return actionEntries;
      })
      .finally(() => {
        canvasActionsLoading = null;
        canvasActionsLoadingRevision = null;
        canvasActionsSkipRedraw = false;
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
      .then((r) => r.json().then(canvasPayload))
      .then((doc) => {
        doc = canvasPayload(doc);
        if (!doc || !Array.isArray(doc.modules)) throw new Error("core library query returned no modules");
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
              callee: member.callee || callee,
              insert_callee: member.insert_callee || null,
              rank: Number(member.rank || 0),
              rank_terms: member.rank_terms || [],
              signature: member.signature || "",
              summary: member.summary || module.summary || "",
              source: member.source || "",
              available: member.available !== false,
              stageable: !!member.stageable,
              stage_reason_code: member.stage_reason_code || "",
              stage_reason: member.stage_reason || "",
              receiver_type: member.receiver_type || "",
              denied_reason: member.denied_reason || "",
              unavailable_reason_code: member.unavailable_reason_code || "",
              pure: !!member.pure,
              pins: member.pins || [],
              ret: member.ret || actionReturnType(member) || "Value",
              args: Array.isArray(member.default_args) ? member.default_args : [],
              default_args: Array.isArray(member.default_args) ? member.default_args : []
            }));
          }
        }
        mergeActionEntries(entries);
        if (!query) coreCatalogLoaded = true;
        window.__jetCanvasCoreCatalogPalette = entries.length;
        if (latestDoc) {
          if (!actionEntriesRevision) actionEntriesRevision = latestDoc.revision;
          syncLibraryPanel(latestDoc);
        }
        return actionEntries;
      })
      .catch((error) => {
        const offline = navigator.onLine === false;
        setCanvasState(offline ? "offline" : "error", offline ? "Offline" : "Core library unavailable", offline
          ? "Jet source stays visible. Reconnect, then retry the library query."
          : "Checked source stays visible. Retry the library query or open Code.", [
          { label: "Open source", run: openSourceRecovery },
          { label: "Retry", primary: true, run: () => loadCoreCatalogActions(query) }
        ]);
        setSaveState("source unchanged", "error");
        showToast(error && error.message ? error.message : "Core library query failed", { isError: true });
        return actionEntries;
      })
      .finally(() => { if (!query) coreCatalogLoading = null; });
    return coreCatalogLoading;
  }

  function cssEscape(s) {
    if (window.CSS && CSS.escape) return CSS.escape(s);
    return String(s).replace(/["\\]/g, "\\$&");
  }

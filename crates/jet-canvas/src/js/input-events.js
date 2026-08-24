
// Pointer, keyboard, toolbar, lens, and debug event handlers.

  function exactBodyHelperForExecTarget(graph, target) {
    const node = nodeForPin(graph, target);
    if (!node || !latestDoc || !latestDoc.source_text || !node.source_span) return null;
    const targetSource = latestDoc.source_text.slice(node.source_span.start, node.source_span.end).trim();
    if (!targetSource || targetSource.includes("\n")) return null;
    const helper = (latestDoc.graphs || []).find((candidate) => {
      if (!candidate || candidate.graph_id === graph.graph_id || !candidate.function) return false;
      const span = candidate.function.source_span;
      if (!span) return false;
      const open = latestDoc.source_text.indexOf("{", Number(span.start));
      if (open < 0 || open >= Number(span.end)) return false;
      let depth = 0;
      let quote = null;
      let escaped = false;
      for (let i = open; i < latestDoc.source_text.length; i++) {
        const ch = latestDoc.source_text[i];
        if (quote) {
          if (escaped) escaped = false;
          else if (ch === "\\") escaped = true;
          else if (ch === quote) quote = null;
          continue;
        }
        if (ch === '"' || ch === "'") {
          quote = ch;
        } else if (ch === "{") {
          depth += 1;
        } else if (ch === "}") {
          depth -= 1;
          if (depth === 0) {
            return latestDoc.source_text.slice(open + 1, i).trim() === targetSource;
          }
        }
      }
      return false;
    });
    return helper ? { name: helper.title, graph_id: helper.graph_id } : null;
  }

  function defaultExecConvergenceFunctionName(graph, target) {
    const node = nodeForPin(graph, target);
    const title = String(node && node.title || "shared")
      .replace(/[^A-Za-z0-9_]+/g, "_")
      .replace(/^([^A-Za-z_])/, "_$1")
      .replace(/_+/g, "_")
      .replace(/^$/, "shared");
    return `shared_${title}`;
  }

  function closeExecConvergencePreview() {
    window.__jetCanvasExecConvergencePreview = null;
    window.__jetCanvasLastConnectionPlan = null;
    if (latestDoc) drawGraph(latestDoc);
  }

  function renderExecConvergencePreview() {
    const preview = window.__jetCanvasExecConvergencePreview;
    if (!preview || !latestDoc || preview.revision !== latestDoc.revision) {
      if (preview) window.__jetCanvasExecConvergencePreview = null;
      return;
    }
    const helper = preview.helper || null;
    const strategy = preview.strategy || "extract";
    const functionName = preview.function_name || "shared_steps";
    const helperOption = helper
      ? `<label class="inline-row"><span><input type="radio" name="exec-convergence-strategy" data-exec-convergence-strategy="helper"${strategy === "helper" ? " checked" : ""}> Reuse exact-body helper</span><code>${escapeHtml(helper.name)}</code><small>The existing helper body matches this branch exactly.</small></label>`
      : `<div class="inline-row"><b>Reuse exact-body helper</b><span class="tag">No exact-body helper found. Keep the source unchanged while you choose another strategy.</span></div>`;
    const applyLabel = strategy === "duplicate" ? "Apply duplication (warning)" : "Apply convergence";
    details.innerHTML = `
      <div class="details-hero">
        <div class="details-titleline"><span class="node-glyph">⇄</span><div class="details-title"><p class="title">Execution convergence preview</p><div class="kind">No source written</div></div></div>
        <span>A second execution path reaches an input that already has a control connection. Choose how the shared body should converge.</span>
        <div class="details-chips"><span class="details-chip">preview only</span><span class="details-chip">revision ${escapeHtml(String(preview.revision).slice(0, 18))}</span></div>
      </div>
      <div class="pin-list">
        <label class="inline-row"><span><input type="radio" name="exec-convergence-strategy" data-exec-convergence-strategy="extract"${strategy === "extract" ? " checked" : ""}> Extract shared body (recommended)</span><small>One canonical helper body can serve both labeled execution paths.</small></label>
        ${helperOption}
        <label class="inline-row"><span><input type="radio" name="exec-convergence-strategy" data-exec-convergence-strategy="duplicate"${strategy === "duplicate" ? " checked" : ""}> Duplicate body</span><small><b>Warning:</b> copied source can drift. This choice is explicit and applies only after confirmation.</small></label>
        <label class="inline-row"><b>Helper name</b><input id="exec-convergence-function" data-exec-convergence-function value="${escapeHtml(functionName)}" spellcheck="false" autocomplete="off"><small>The name is part of the checked source transaction.</small></label>
      </div>
      <div class="edit-grid"><button id="apply-exec-convergence">${applyLabel}</button><button id="close-exec-convergence-preview">Close preview</button><span class="tag">Source and undo history are unchanged until Apply.</span></div>`;
    setDrawer("details");
    details.querySelectorAll("[data-exec-convergence-strategy]").forEach((input) => {
      input.addEventListener("change", () => {
        preview.strategy = input.getAttribute("data-exec-convergence-strategy") || "extract";
        window.__jetCanvasExecConvergencePreview = preview;
        renderExecConvergencePreview();
      });
    });
    const nameInput = document.getElementById("exec-convergence-function");
    if (nameInput) {
      nameInput.addEventListener("input", () => {
        preview.function_name = nameInput.value;
        window.__jetCanvasExecConvergencePreview = preview;
      });
      nameInput.focus();
      nameInput.select();
    }
    const apply = document.getElementById("apply-exec-convergence");
    if (apply) apply.addEventListener("click", () => {
      const name = String((document.getElementById("exec-convergence-function") || {}).value || "").trim();
      if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
        showToast("Convergence helper name must be a Jet identifier", { isError: true });
        return;
      }
      preview.function_name = name;
      postTransaction({
        schema_version: 1,
        op: "replace_source",
        source_edit: "exec_convergence",
        revision: latestDoc.revision,
        graph_id: preview.graph_id,
        from_pin_name: preview.from_pin_name,
        from_start: preview.from_source_span && preview.from_source_span.start,
        from_end: preview.from_source_span && preview.from_source_span.end,
        target_start: preview.target_source_span && preview.target_source_span.start,
        target_end: preview.target_source_span && preview.target_source_span.end,
        strategy: preview.strategy || "extract",
        function: name,
        helper_name: preview.helper && preview.helper.name || null
      });
    });
    const close = document.getElementById("close-exec-convergence-preview");
    if (close) close.addEventListener("click", closeExecConvergencePreview);
  }

  function openExecConvergencePreview(graph, fromPin, target) {
    const input = fromPin.direction === "input" ? fromPin : target;
    const fromNode = nodeForPin(graph, fromPin);
    const targetNode = nodeForPin(graph, input);
    const helper = exactBodyHelperForExecTarget(graph, input);
    window.__jetCanvasExecConvergencePreview = {
      graph_id: graph.graph_id,
      revision: latestDoc && latestDoc.revision,
      from_pin_id: fromPin.pin_id,
      to_pin_id: target.pin_id,
      from_pin_name: fromPin.name,
      from_source_span: fromNode && fromNode.source_span || fromPin.source_span || null,
      target_source_span: targetNode && targetNode.source_span || input.source_span || null,
      function_name: defaultExecConvergenceFunctionName(graph, input),
      incoming_wire_id: (graph.wires || []).find((wire) => wire.wire_kind === "control" && wire.to_pin === input.pin_id)?.wire_id || null,
      strategy: "extract",
      helper
    };
    window.__jetCanvasLastConnectionPlan = { ok: true, label: "Execution convergence preview", color: "#7dd3fc" };
    renderExecConvergencePreview();
    showToast("Execution convergence preview: source unchanged");
  }

  function canvasGestureIsCurrent(gesture) {
    const graph = currentGraphOrNull();
    return !!gesture && !!latestDoc && !!graph
      && gesture.graphId === graph.graph_id
      && gesture.revision === latestDoc.revision
      && gesture.sourceId === currentCanvasSourceId();
  }

  function restoreNodeGesture(gesture) {
    for (const [id, offset] of gesture.beforeOffsets || []) {
      if (offset) nodeOffsets.set(id, Object.assign({}, offset));
      else nodeOffsets.delete(id);
    }
    restoreMarqueeGesture(gesture);
  }

  function restoreMarqueeGesture(gesture) {
    selectedNodeIds = new Set(gesture.initialSelection || []);
    selectedNodeId = gesture.initialSelectedNodeId || null;
    selectionExplicitlyCleared = !!gesture.initialSelectionExplicitlyCleared;
    if (Object.prototype.hasOwnProperty.call(gesture, "initialSelectedVariableName")) {
      selectedVariableName = gesture.initialSelectedVariableName;
    }
  }

  function rejectStaleCanvasGesture(gesture) {
    if (canvasGestureIsCurrent(gesture)) return false;
    if (gesture && gesture.mode === "node") restoreNodeGesture(gesture);
    if (gesture && gesture.mode === "marquee") restoreMarqueeGesture(gesture);
    setCanvasState("stale", "Canvas gesture is stale", "The source or graph changed while this gesture was in progress. Nothing was saved; reload and retry.", [
      { label: "Show source", run: openSourceRecovery },
      { label: "Reload", primary: true, run: () => loadGraph() }
    ]);
    showToast("Canvas gesture is stale; reload and retry", { isError: true });
    return true;
  }

  canvas.addEventListener("click", function (ev) {
    if (window.__jetCanvasNoopClick) return;
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const pinEditor = hitPinDefaultEditorAt(x, y);
    if (pinEditor && applyPinDefaultEditor(pinEditor)) {
      ev.preventDefault();
      return;
    }
    if (pinEditor && applyNodeAffordance(pinEditor)) {
      ev.preventDefault();
      return;
    }
    if (hitPinAt(x, y)) {
      ev.preventDefault();
      return;
    }
    const found = hitNodeAt(x, y);
    // mousedown owns node selection so a following click cannot toggle or
    // collapse the selection a second time after a modifier gesture/drag.
    if (found) {
      ev.preventDefault();
      return;
    }
    else {
      const comment = hitCommentAt(x, y);
      if (comment) {
        selectedVariableName = null;
        selectedNodeIds = new Set([comment.box.comment_id]);
        selectedNodeId = comment.box.comment_id;
        if (latestDoc) drawGraph(latestDoc);
        ev.preventDefault();
      }
    }
  });

  canvas.addEventListener("dblclick", function (ev) {
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const comment = hitCommentAt(x, y);
    if (comment && comment.part === "title") {
      const title = window.prompt("Comment title", comment.box.title || "Comment");
      if (title !== null) {
        comment.box.title = title || "Comment";
        saveEditorState();
        drawGraph(latestDoc);
      }
      ev.preventDefault();
      return;
    }
    const found = hitNodeAt(x, y);
    if (found && openFunctionGraph(found.node.title)) ev.preventDefault();
  });

  canvas.addEventListener("mousedown", function (ev) {
    if (window.__jetCanvasNoopClick) return;
    // Right-click only opens the context menu (handled by the separate
    // "contextmenu" listener below, which does its own hit-testing). Letting
    // it fall through used to start a zero-movement "node" drag whose
    // mouseup unconditionally remembers the clicked node's on-screen
    // position — a single remembered position is enough to permanently
    // disable this graph's auto-layout reflow (see hasSavedNodePositions),
    // stranding every other node at raw unreflowed backend coordinates.
    if (ev.button === 2) return;
    const rect = canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    if (hitPinDefaultEditorAt(x, y)) return;
    const endpoint = hitWireEndpointAt(x, y);
    if (endpoint && endpoint.pin) {
      hoverPin = endpoint.pin;
      const graph = currentGraphOrNull();
      drag = { mode: "pin", pin: endpoint.pin, rewire: endpoint, graphId: graph && graph.graph_id, revision: latestDoc && latestDoc.revision, sourceId: currentCanvasSourceId(), x, y, mx: x, my: y };
      showToast("Rewire " + pinName(endpoint.pin));
      return;
    }
    const pin = hitPinAt(x, y);
    if (pin) {
      hoverPin = pin;
      const graph = currentGraphOrNull();
      drag = { mode: "pin", pin, graphId: graph && graph.graph_id, revision: latestDoc && latestDoc.revision, sourceId: currentCanvasSourceId(), x, y, mx: x, my: y };
      showToast(pin.name + ": " + pin.type);
      return;
    }
    setPendingPin(null);
    const found = hitNodeAt(x, y);
    if (found) {
      const initialSelection = new Set(selectedNodeIds);
      const initialSelectedNodeId = selectedNodeId;
      const initialSelectionExplicitlyCleared = selectionExplicitlyCleared;
      const initialSelectedVariableName = selectedVariableName;
      const modifier = ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : null;
      const alreadySelected = selectedNodeIds.has(found.node.node_id);
      if (modifier) {
        selectNode(found.node, modifier);
      } else if (!alreadySelected || selectedNodeIds.size === 0) {
        selectNode(found.node, "replace");
      } else {
        selectedVariableName = null;
        selectedNodeId = found.node.node_id;
        selectionExplicitlyCleared = false;
        setSourceHash(found.node.source_span || { start: 0, end: 0 });
      }
      const starts = new Map();
      const beforeOffsets = new Map();
      for (const id of selectedNodeIds) {
        const offset = nodeOffsets.get(id);
        beforeOffsets.set(id, offset ? Object.assign({}, offset) : null);
        starts.set(id, offset || { x: 0, y: 0 });
      }
      const graph = currentGraphOrNull();
      drag = {
        mode: "node",
        x,
        y,
        wx: wx(x),
        wy: wy(y),
        starts,
        beforeOffsets,
        initialSelection,
        initialSelectedNodeId,
        initialSelectionExplicitlyCleared,
        initialSelectedVariableName,
        graphId: graph && graph.graph_id,
        revision: latestDoc && latestDoc.revision,
        sourceId: currentCanvasSourceId(),
        moved: false
      };
    } else if (hitCommentAt(x, y)) {
      const comment = hitCommentAt(x, y);
      selectedVariableName = null;
      selectedNodeIds = new Set([comment.box.comment_id]);
      selectedNodeId = comment.box.comment_id;
      selectionExplicitlyCleared = false;
      const graph = currentGraphOrNull();
      const contained = comment.part === "title" ? nodesInsideComment(graph, comment.box) : [];
      const starts = new Map();
      for (const id of contained) starts.set(id, nodeOffsets.get(id) || { x: 0, y: 0 });
      drag = { mode: comment.part === "resize" ? "comment-resize" : "comment", x, y, wx: wx(x), wy: wy(y), box: comment.box, start: Object.assign({}, comment.box), starts };
    } else if (ev.button === 1 || ev.altKey || spaceDown) {
      drag = { mode: "pan", x, y, ox: view.x, oy: view.y };
    } else {
      const graph = currentGraphOrNull();
      drag = {
        mode: "marquee",
        x,
        y,
        mx: x,
        my: y,
        selectionMode: ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : "replace",
        initialSelection: new Set(selectedNodeIds),
        initialSelectedNodeId: selectedNodeId,
        initialSelectionExplicitlyCleared: selectionExplicitlyCleared,
        initialSelectedVariableName: selectedVariableName,
        graphId: graph && graph.graph_id,
        revision: latestDoc && latestDoc.revision,
        sourceId: currentCanvasSourceId(),
        moved: false
      };
    }
  });

  window.addEventListener("mousemove", function (ev) {
    const rect = canvas.getBoundingClientRect();
    lastPointer = graphPointFromClient(ev.clientX, ev.clientY);
    if (!drag) {
      const x = ev.clientX - rect.left;
      const y = ev.clientY - rect.top;
      const nextDiagnostic = hitDiagnosticAt(x, y);
      const nextHover = nextDiagnostic ? null : hitPinAt(x, y);
      const nextNode = nextDiagnostic || nextHover ? null : (hitNodeAt(x, y) || {}).node || null;
      if (nextDiagnostic !== hoverDiagnostic
        || (nextHover && !hoverPin)
        || (!nextHover && hoverPin)
        || (nextHover && hoverPin && nextHover.pin_id !== hoverPin.pin_id)
        || (nextNode && !hoverNode)
        || (!nextNode && hoverNode)
        || (nextNode && hoverNode && nextNode.node_id !== hoverNode.node_id)) {
        hoverDiagnostic = nextDiagnostic;
        hoverPin = nextHover;
        hoverNode = nextNode;
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
      drag.moved = drag.moved || Math.abs(dx) > 1 || Math.abs(dy) > 1;
      for (const [id, start] of drag.starts.entries()) nodeOffsets.set(id, { x: start.x + dx, y: start.y + dy });
    } else if (drag.mode === "marquee") {
      drag.mx = x;
      drag.my = y;
      drag.moved = drag.moved || Math.abs(x - drag.x) > 3 || Math.abs(y - drag.y) > 3;
      selectMarquee();
    } else if (drag.mode === "pin") {
      drag.mx = x;
      drag.my = y;
      hoverPin = hitPinAt(x, y);
    } else if (drag.mode === "comment") {
      const dx = wx(x) - drag.wx;
      const dy = wy(y) - drag.wy;
      drag.box.x = Math.round(drag.start.x + dx);
      drag.box.y = Math.round(drag.start.y + dy);
      for (const [id, start] of drag.starts.entries()) nodeOffsets.set(id, { x: start.x + dx, y: start.y + dy });
    } else if (drag.mode === "comment-resize") {
      const dx = wx(x) - drag.wx;
      const dy = wy(y) - drag.wy;
      drag.box.w = Math.max(160, Math.round(drag.start.w + dx));
      drag.box.h = Math.max(96, Math.round(drag.start.h + dy));
    }
    if (latestDoc) drawGraph(latestDoc);
  });

  window.addEventListener("mouseup", function (ev) {
    if (drag && (drag.mode === "node" || drag.mode === "marquee") && rejectStaleCanvasGesture(drag)) {
      drag = null;
      if (latestDoc) drawGraph(latestDoc);
      return;
    }
    if (drag && drag.mode === "node" && drag.moved) {
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      rememberSelectedNodePositions(graph);
      persistStagedNodePositions();
      showToast("Moved " + selectedNodeIds.size + " node" + (selectedNodeIds.size === 1 ? "" : "s") + " locally");
    }
    if (drag && drag.mode === "marquee" && !drag.moved && drag.selectionMode === "replace") {
      selectedNodeIds = new Set();
      selectedNodeId = null;
      selectionExplicitlyCleared = true;
      if (latestDoc) drawGraph(latestDoc);
    }
    if (drag && (drag.mode === "comment" || drag.mode === "comment-resize")) {
      const movedIds = drag.starts ? Array.from(drag.starts.keys()) : [];
      rememberNodePositionsById(currentGraphOrNull(), movedIds);
      persistStagedNodePositions(movedIds);
      saveEditorState();
      showToast(drag.mode === "comment-resize" ? "Comment resized" : "Comment moved");
    }
    let pinTransactionStarted = false;
    if (drag && drag.mode === "pin") {
      const rect = canvas.getBoundingClientRect();
      const target = hitPinAt(ev.clientX - rect.left, ev.clientY - rect.top) || hoverPin;
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      const moved = Math.abs(drag.mx - drag.x) > 5 || Math.abs(drag.my - drag.y) > 5;
      if (!moved && pendingPin && rejectStaleCanvasGesture(pendingPinContext)) {
        setPendingPin(null);
        drag = null;
        if (latestDoc) drawGraph(latestDoc);
        return;
      }
      if (!moved && pendingPin && target) {
        const previousTransaction = window.__jetCanvasLastTx;
        const done = completeConnection(pendingPin, target, graph);
        pinTransactionStarted = window.__jetCanvasLastTx !== previousTransaction;
        setPendingPin(done || pendingPin.pin_id === target.pin_id ? null : pendingPin);
      } else if (!moved && target && target.pin_id === drag.pin.pin_id) {
        if (pendingPin && pendingPin.pin_id === target.pin_id) {
          setPendingPin(null);
          showToast("Connection cancelled");
        } else {
          setPendingPin(drag.pin);
          showToast("Select destination pin");
        }
      } else if (target) {
        setPendingPin(null);
        const previousTransaction = window.__jetCanvasLastTx;
        completeConnection(drag.pin, target, graph);
        pinTransactionStarted = window.__jetCanvasLastTx !== previousTransaction;
      } else {
        setPendingPin(null);
        openPinMenu(drag.pin, ev.clientX, ev.clientY, { x: wx(ev.clientX - rect.left), y: wy(ev.clientY - rect.top) });
      }
    }
    const mouseupMode = drag && drag.mode;
    const mouseupMoved = !!(drag && drag.moved);
    drag = null;
    // A click already drew the selected node in mousedown. Avoid a second
    // full large-graph repaint when no node position changed.
    const pinAccepted = mouseupMode === "pin"
      && window.__jetCanvasLastConnectionPlan
      && window.__jetCanvasLastConnectionPlan.ok;
    if (latestDoc && (mouseupMode !== "node" || mouseupMoved) && !pinTransactionStarted && !pinAccepted) drawGraph(latestDoc);
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
      openContextMenu(ev.clientX, ev.clientY, found.node.title, nodeContextActions(graphWithViewState(graph), found.node));
      return;
    }
    const comment = hitCommentAt(x, y);
    if (comment) {
      selectedNodeIds = new Set([comment.box.comment_id]);
      selectedNodeId = comment.box.comment_id;
      const tintActions = COMMENT_TINTS.map((color) => ({ title: "Tint " + color, detail: "comment color", group: "Comment", run: () => setCommentTint(comment.box, color) }));
      openContextMenu(ev.clientX, ev.clientY, comment.box.title || "Comment", [
        { title: "Rename comment", detail: "local view", group: "Comment", run: () => {
          const title = window.prompt("Comment title", comment.box.title || "Comment");
          if (title !== null) { comment.box.title = title || "Comment"; saveEditorState(); drawGraph(latestDoc); }
        } },
        { title: "Delete comment", detail: "local view", group: "Comment", run: () => {
          editorState.commentBoxes = (editorState.commentBoxes || []).filter((box) => box.comment_id !== comment.box.comment_id);
          saveEditorState();
          drawGraph(latestDoc);
        } }
      ].concat(tintActions));
      return;
    }
    openGraphActionPalette(ev.clientX, ev.clientY, "", { x: wx(x), y: wy(y) });
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
  orgDistribute.addEventListener("click", () => distributeSelectedNodes("x"));
  orgTidy.addEventListener("click", tidyGraphLayout);
  bookmarkAdd.addEventListener("click", bookmarkCurrentGraph);
  bookmarkJump.addEventListener("click", jumpBookmark);
  favoriteAction.addEventListener("click", toggleFavoriteAction);
  if (checkCurrent) checkCurrent.addEventListener("click", checkCurrentSource);
  runCurrent.addEventListener("click", runCurrentGraph);
  const onboardingDismiss = document.getElementById("tour-dismiss");
  if (onboardingDismiss) onboardingDismiss.addEventListener("click", finishTour);
  const onboardingNext = document.getElementById("tour-next");
  if (onboardingNext) onboardingNext.addEventListener("click", nextTourStep);
  const onboardingBack = document.getElementById("tour-back");
  if (onboardingBack) onboardingBack.addEventListener("click", previousTourStep);
  const onboardingAction = document.getElementById("tour-action");
  if (onboardingAction) onboardingAction.addEventListener("click", runTourAction);
  const onboardingOpen = document.getElementById("tour-open");
  if (onboardingOpen) onboardingOpen.addEventListener("click", startTour);
  if (sourceEditor) sourceEditor.addEventListener("input", () => saveSourceDraft(sourceEditor.value));
  debugStart.addEventListener("click", () => runDebug(["s"]));
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
    const editingText = ev.target && ["INPUT", "TEXTAREA", "SELECT"].includes(ev.target.tagName);
    if (ev.key === " ") spaceDown = true;
    if (keyboardCheatSheetIsOpen()) {
      if (ev.key === "Escape") {
        ev.preventDefault();
        closeKeyboardCheatSheet();
      }
      return;
    }
    if (!editingText && ev.key === "?") {
      ev.preventDefault();
      openKeyboardCheatSheet();
      return;
    }
    if (ev.key === "Escape" && window.__jetCanvasExecConvergencePreview) {
      closeExecConvergencePreview();
      ev.preventDefault();
      return;
    }
    if (ev.key === "Escape") {
      const hadTransient = !!drag || !!pendingPin || contextMenu.classList.contains("is-open");
      if (drag && drag.mode === "node") restoreNodeGesture(drag);
      if (drag && drag.mode === "marquee") restoreMarqueeGesture(drag);
      drag = null;
      setPendingPin(null);
      closeContextMenu();
      if (latestDoc) drawGraph(latestDoc);
      if (hadTransient) {
        ev.preventDefault();
        return;
      }
    }
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
    if (!editingText && (ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "c") {
      ev.preventDefault();
      copySelection();
      return;
    }
    if (!editingText && (ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "v") {
      ev.preventDefault();
      if (ev.shiftKey) pasteAsStaged();
      else pasteSelection();
      return;
    }
    if (!editingText && ev.key === "F2" && pasteRenameChips.length) {
      ev.preventDefault();
      const selected = currentGraphOrNull()?.nodes?.find((node) =>
        node.kind === "binding" && node.node_id === selectedNodeId && pasteRenameChips.some((rename) => rename.to === node.title));
      beginPasteRename((selected && selected.title) || pasteRenameChips[0].to);
      return;
    }
    if (!editingText && (ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === "d") {
      ev.preventDefault();
      duplicateSelection();
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "a") {
      ev.preventDefault();
      alignSelectedNodes(ev.shiftKey ? "x" : "y");
      return;
    }
    if (ev.altKey && ev.key.toLowerCase() === "d") {
      ev.preventDefault();
      distributeSelectedNodes(ev.shiftKey ? "y" : "x");
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
    if (!editingText && ev.altKey && ev.key.toLowerCase() === "c") {
      ev.preventDefault();
      if (ev.shiftKey) expandSelectedCollapse();
      else collapseSelectedNodes();
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
    if (!editingText && ev.key.toLowerCase() === "c" && selectedNodeIds.size > 0) {
      ev.preventDefault();
      addCommentAroundSelection();
      return;
    }
    if (!editingText && (ev.key === "Delete" || ev.key === "Backspace") && deleteLocalSelection()) {
      ev.preventDefault();
      return;
    }
    if (ev.key === "Escape") {
      if (compactCanvasMode() && (leftDrawer.classList.contains("is-drawer-open") || rightDrawer.classList.contains("is-drawer-open"))) {
        setDrawer(null);
        return;
      }
      selectedNodeIds = new Set();
      selectedNodeId = null;
      selectionExplicitlyCleared = true;
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
      rememberSelectedNodePositions(currentGraphOrNull());
      if (latestDoc) drawGraph(latestDoc);
    }
  });

  window.addEventListener("keyup", function (ev) {
    if (ev.key === " ") spaceDown = false;
  });

  document.addEventListener("click", function (ev) {
    if (Date.now() - contextMenuOpenedAt < 250) return;
    if (!contextMenu.contains(ev.target)) closeContextMenu();
  });

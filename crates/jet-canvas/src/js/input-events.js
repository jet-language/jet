
// Pointer, keyboard, toolbar, lens, and debug event handlers.
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
    if (found) selectNode(found.node, ev.ctrlKey || ev.metaKey ? "toggle" : ev.shiftKey ? "add" : "replace");
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
      drag = { mode: "pin", pin: endpoint.pin, rewire: endpoint, x, y, mx: x, my: y };
      showToast("Rewire " + pinName(endpoint.pin));
      return;
    }
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
    } else if (hitCommentAt(x, y)) {
      const comment = hitCommentAt(x, y);
      selectedVariableName = null;
      selectedNodeIds = new Set([comment.box.comment_id]);
      selectedNodeId = comment.box.comment_id;
      const graph = currentGraphOrNull();
      const contained = comment.part === "title" ? nodesInsideComment(graph, comment.box) : [];
      const starts = new Map();
      for (const id of contained) starts.set(id, nodeOffsets.get(id) || { x: 0, y: 0 });
      drag = { mode: comment.part === "resize" ? "comment-resize" : "comment", x, y, wx: wx(x), wy: wy(y), box: comment.box, start: Object.assign({}, comment.box), starts };
    } else if (ev.button === 1 || ev.altKey || spaceDown) {
      drag = { mode: "pan", x, y, ox: view.x, oy: view.y };
    } else {
      drag = { mode: "marquee", x, y, mx: x, my: y, additive: ev.shiftKey || ev.ctrlKey || ev.metaKey };
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
      for (const [id, start] of drag.starts.entries()) nodeOffsets.set(id, { x: start.x + dx, y: start.y + dy });
    } else if (drag.mode === "marquee") {
      drag.mx = x;
      drag.my = y;
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
    if (drag && drag.mode === "node") {
      const graph = latestDoc ? currentGraph(latestDoc) : null;
      rememberSelectedNodePositions(graph);
      persistStagedNodePositions();
      showToast("Moved " + selectedNodeIds.size + " node" + (selectedNodeIds.size === 1 ? "" : "s") + " locally");
    }
    if (drag && (drag.mode === "comment" || drag.mode === "comment-resize")) {
      const movedIds = drag.starts ? Array.from(drag.starts.keys()) : [];
      rememberNodePositionsById(currentGraphOrNull(), movedIds);
      persistStagedNodePositions(movedIds);
      saveEditorState();
      showToast(drag.mode === "comment-resize" ? "Comment resized" : "Comment moved");
    }
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
          showToast("Select destination pin");
        }
      } else if (target) {
        setPendingPin(null);
        completeConnection(drag.pin, target, graph);
      } else {
        setPendingPin(null);
        openPinMenu(drag.pin, ev.clientX, ev.clientY, { x: wx(ev.clientX - rect.left), y: wy(ev.clientY - rect.top) });
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
    const editingText = ev.target && ["INPUT", "TEXTAREA", "SELECT"].includes(ev.target.tagName);
    if (ev.key === " ") spaceDown = true;
    if (ev.key === "Escape") {
      const hadTransient = !!drag || !!pendingPin || contextMenu.classList.contains("is-open");
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
      pasteSelection();
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

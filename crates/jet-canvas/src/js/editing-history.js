
// Local editor persistence, staged graph edits, command authority, and detail controls.
  function storedFlag(name) {
    try { return window.localStorage && window.localStorage.getItem(name) === "1"; }
    catch (_) { return false; }
  }

  function storeFlag(name, value) {
    try { if (window.localStorage) window.localStorage.setItem(name, value ? "1" : "0"); }
    catch (_) {}
  }

  function shortCalleeName(callee) {
    const parts = String(callee || "").split(".");
    return parts[parts.length - 1] || "edit";
  }

  function transactionUndoLabel(body) {
    if (!body) return "edit";
    if (body.op === "insert_call") return "insert " + shortCalleeName(body.callee);
    if (body.op === "edit_inline_expr") return "inline edit";
    if (body.op === "reorder_statements") return "reorder steps";
    if (body.op === "move_link") return "rewire";
    if (body.op === "break_link") return "break wire";
    if (body.op === "rename_binding") return "rename " + (body.from || "binding");
    if (body.op === "rename_function") return "rename " + (body.from || "function");
    if (body.op === "edit_function_signature") return "signature";
    if (body.op === "create_function") return "create " + (body.name || "function");
    if (body.op === "replace_source" && body.source_edit === "paste_clone") return "paste";
    if (body.op === "replace_source") return body.source_edit ? "source edit" : "source restore";
    if (body.op === "insert_branch") return "insert branch";
    if (body.op === "insert_switch") return "insert dispatch";
    if (body.op === "insert_loop") return "insert loop";
    if (body.op === "insert_fallible_rail") return "insert fallible rail";
    if (body.op === "create_comment_region") return "comment";
    if (body.op === "promote_to_binding") return "promote variable";
    if (body.op === "insert_visible_conversion") return "conversion";
    if (body.op === "extract_inline_expr") return "extract function";
    return String(body.op || "edit").replace(/_/g, " ");
  }

  function pushHistory(stack, entry) {
    stack.push(entry);
    while (stack.length > UNDO_DEPTH) stack.shift();
  }

  function recordUndoEntry(body, before, after) {
    if (!before || !after || before === after) return;
    pushHistory(undoStack, { before, after, label: transactionUndoLabel(body), op: body && body.op || "edit" });
    redoStack = [];
  }

  function projectStateKey(doc) {
    return "jet.canvas.editor:" + ((doc && doc.source_id) || "source");
  }

  function loadEditorState(doc) {
    try {
      editorState = JSON.parse(localStorage.getItem(projectStateKey(doc)) || "null") || editorState;
    } catch (_) {
      editorState = { bookmarks: [], favorites: [], actionUse: {}, rerouteKnots: [], nodePositions: {}, commentBoxes: [], stagedNodes: [], stagedWires: [], tourDismissed: false };
    }
    editorState.bookmarks ||= [];
    editorState.favorites ||= [];
    editorState.actionUse ||= {};
    editorState.rerouteKnots ||= [];
    editorState.nodePositions ||= {};
    editorState.commentBoxes ||= [];
    editorState.stagedNodes ||= [];
    editorState.stagedWires ||= [];
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
    const descriptor = nodeDescriptorForAction(action);
    return favorite + used + (descriptor && descriptor.palette.rank || 0);
  }

  function graphCommentBoxes(graph) {
    if (!graph) return [];
    editorState.commentBoxes ||= [];
    return editorState.commentBoxes.filter((box) => box.graph_id === graph.graph_id);
  }

  function graphStagedNodes(graph) {
    if (!graph) return [];
    editorState.stagedNodes ||= [];
    return editorState.stagedNodes.filter((node) => node.graph_id === graph.graph_id);
  }

  function graphStagedWires(graph) {
    if (!graph) return [];
    editorState.stagedWires ||= [];
    return editorState.stagedWires.filter((wire) => wire.graph_id === graph.graph_id);
  }

  function graphWithViewState(graph) {
    if (!graph) return graph;
    const staged = graphStagedNodes(graph);
    const stagedPins = staged.flatMap((node) => node.pins || []);
    const collapsed = (graph.regions || []).filter((region) => region.kind === "collapse");
    const hiddenIds = new Set();
    const collapsedNodes = [];
    for (const region of collapsed) {
      const inside = (graph.nodes || []).filter((node) =>
        node.node_id !== graph.entry_node
        && node.source_span
        && region.source_span
        && node.source_span.start >= region.source_span.start
        && node.source_span.end <= region.source_span.end);
      if (!inside.length) continue;
      inside.forEach((node) => hiddenIds.add(node.node_id));
      const first = inside.slice().sort((a, b) => a.source_span.start - b.source_span.start)[0];
      collapsedNodes.push({
        node_id: region.region_id,
        title: region.title || "Collapsed",
        kind: "collapse",
        archetype: "control",
        source_span: region.source_span,
        layout: first.layout,
        collapsed_region_id: region.region_id,
        edit_affordances: ["expand_collapsed_region"]
      });
    }
    const hiddenPins = new Set((graph.pins || [])
      .filter((pin) => hiddenIds.has(pin.node_id))
      .map((pin) => pin.pin_id));
    return Object.assign({}, graph, {
      nodes: (graph.nodes || []).filter((node) => !hiddenIds.has(node.node_id)).concat(collapsedNodes, staged),
      pins: (graph.pins || []).filter((pin) => !hiddenPins.has(pin.pin_id)).concat(stagedPins),
      wires: (graph.wires || []).filter((wire) => !hiddenPins.has(wire.from_pin) && !hiddenPins.has(wire.to_pin)).concat(graphStagedWires(graph))
    });
  }

  function newLocalId(prefix) {
    return prefix + ":" + Date.now().toString(36) + ":" + Math.random().toString(36).slice(2, 8);
  }

  function graphPointFromClient(x, y) {
    const rect = canvas.getBoundingClientRect();
    return { x: wx(x - rect.left), y: wy(y - rect.top) };
  }

  function viewportCenterGraphPoint() {
    const size = cssSize();
    return { x: wx(size.width / 2), y: wy(size.height / 2) };
  }

  function currentGraphOrNull() {
    return latestDoc ? currentGraph(latestDoc) : null;
  }

  function selectedGraphNodes(graph) {
    if (!graph) return [];
    return graph.nodes.filter((node) => selectedNodeIds.has(node.node_id));
  }

  function selectedGraphComments(graph) {
    if (!graph) return [];
    return graphCommentBoxes(graph).filter((box) => selectedNodeIds.has(box.comment_id));
  }

  function stagedPinsForAction(id, action) {
    const pins = (action.pins || []).map((pin, i) => Object.assign({}, pin, {
      pin_id: `${id}:pin:${pin.direction || "input"}:${pin.name || i}`,
      node_id: id,
      direction: pin.direction || "input",
      name: pin.name || (pin.direction === "output" ? "result" : "arg" + (i + 1)),
      type: pin.type || "Value",
      source_span: null
    }));
    if (pins.length) return pins;
    const descriptor = nodeDescriptorForAction(action);
    if (descriptor && descriptor.archetype === "control") {
      return [
        { pin_id: id + ":pin:input:exec", node_id: id, direction: "input", name: "exec", type: "exec", source_span: null },
        { pin_id: id + ":pin:output:then", node_id: id, direction: "output", name: "then", type: "exec", source_span: null }
      ];
    }
    const ret = action.ret || action.type || actionReturnType(action) || "Void";
    const list = [];
    if (!action.pure) list.push({ pin_id: id + ":pin:input:exec", node_id: id, direction: "input", name: "exec", type: "exec", source_span: null });
    if (ret && ret !== "Void") list.push({ pin_id: id + ":pin:output:result", node_id: id, direction: "output", name: "result", type: ret, source_span: null });
    if (!list.length) list.push({ pin_id: id + ":pin:output:done", node_id: id, direction: "output", name: "then", type: "exec", source_span: null });
    return list;
  }

  function createStagedNodeFromAction(action, graphPoint, opts = {}) {
    const graph = currentGraphOrNull();
    if (!graph || !action) return null;
    const id = opts.id || newLocalId("staged");
    const title = opts.title || action.title || action.insert_callee || action.callee || "node";
    const x = Number.isFinite(graphPoint && graphPoint.x) ? graphPoint.x : viewportCenterGraphPoint().x;
    const y = Number.isFinite(graphPoint && graphPoint.y) ? graphPoint.y : viewportCenterGraphPoint().y;
    const descriptor = nodeDescriptorForAction(action);
    if (!descriptor || !descriptor.palette.insertable) return null;
    const node = {
      node_id: id,
      graph_id: graph.graph_id,
      node_descriptor_id: descriptor.id,
      kind: descriptor.kind,
      archetype: descriptor.archetype,
      title,
      source_span: null,
      layout: { x: x / layoutScale.x, y: y / layoutScale.y },
      badges: ["not saved"],
      edit_affordances: ["materialize"],
      staged: true,
      action: {
        title,
        detail: action.detail || "",
        group: action.group || "",
        kind: action.kind || "",
        op: action.op || action.insert_op || "",
        node_descriptor_id: descriptor.id,
        module_path: action.module_path || "",
        signature: action.signature || "",
        pure: !!action.pure,
        pins: action.pins || [],
        ret: action.ret || actionReturnType(action) || "",
        type: action.type || "",
        callee: action.callee || "",
        insert_callee: action.insert_callee || action.callee || "",
        args: action.args || action.default_args || []
      }
    };
    node.pins = opts.pins || stagedPinsForAction(id, node.action);
    editorState.stagedNodes = (editorState.stagedNodes || []).filter((n) => n.node_id !== id).concat([node]);
    saveEditorState();
    selectedNodeId = id;
    selectedNodeIds = new Set([id]);
    showToast("Node staged. Connect it to save source.");
    if (latestDoc) drawGraph(latestDoc);
    window.__jetCanvasStagedNodes = graphStagedNodes(graph).length;
    return node;
  }

  function sourceLineStart(src, pos) {
    return src.lastIndexOf("\n", Math.max(0, pos - 1)) + 1;
  }

  function sourceLineEnd(src, pos) {
    const end = src.indexOf("\n", pos);
    return end < 0 ? src.length : end + 1;
  }

  function graphSourceInsertOffset(graph) {
    const src = latestDoc && latestDoc.source_text || "";
    const end = graph && graph.source_span && Number.isFinite(graph.source_span.end) ? graph.source_span.end : src.length;
    const close = src.lastIndexOf("}", Math.max(0, end - 1));
    return close >= 0 ? sourceLineStart(src, close) : src.length;
  }

  function selectedRealCloneSnippet(graph, nodes) {
    const src = latestDoc && latestDoc.source_text || "";
    if (!src || !graph || !nodes.length) return null;
    if (nodes.some((node) => node.staged || node.kind === "entry" || !node.source_span || node.source_span.end <= node.source_span.start)) return null;
    const spans = nodes.map((node) => ({ start: sourceLineStart(src, node.source_span.start), end: sourceLineEnd(src, node.source_span.end), node })).sort((a, b) => a.start - b.start);
    const start = spans[0].start;
    const end = spans[spans.length - 1].end;
    const snippet = src.slice(start, end);
    const trimmed = snippet.trim();
    if (!trimmed || /^fn\s/.test(trimmed) || /^pub\s+fn\s/.test(trimmed)) return null;
    for (const span of spans) {
      if (span.start < start || span.end > end) return null;
    }
    if (!/^\s*(if|loop|return|break|next|[A-Za-z_][A-Za-z0-9_]*(\s*[:=]|::|\())/m.test(snippet)) return null;
    return { text: snippet.endsWith("\n") ? snippet : snippet + "\n", title: nodes.map((n) => n.title || n.kind).join(", ") };
  }

  function selectedClipboardPayload() {
    const graph = graphWithViewState(currentGraphOrNull());
    if (!graph) return null;
    const nodes = selectedGraphNodes(graph);
    const comments = selectedGraphComments(currentGraphOrNull());
    if (!nodes.length && !comments.length) return null;
    const real = nodes.filter((node) => !node.staged);
    const staged = nodes.filter((node) => node.staged);
    const sourceClone = real.length === nodes.length ? selectedRealCloneSnippet(graph, real) : null;
    return {
      graph_id: graph.graph_id,
      source: sourceClone,
      staged: staged.map((node) => JSON.parse(JSON.stringify(node))),
      fallback_nodes: nodes.map((node) => ({
        title: node.title,
        kind: node.kind,
        archetype: node.archetype,
        action: node.action || { title: node.title, op: "insert_call", callee: node.title, insert_callee: node.title, ret: "Value", pins: [] },
        x: nodeX(node),
        y: nodeY(node)
      })),
      comments: comments.map((box) => Object.assign({}, box))
    };
  }

  function copySelection() {
    const payload = selectedClipboardPayload();
    if (!payload) return showToast("Select nodes to copy");
    clipboardState = payload;
    window.__jetCanvasClipboard = payload.source ? "source" : "staged";
    showToast("Copied selection");
  }

  function pasteSourceClone(payload, graph, point) {
    if (!payload.source || !latestDoc || !graph) return false;
    const src = latestDoc.source_text || "";
    const insert = graphSourceInsertOffset(graph);
    const text = payload.source.text.replace(/\s*$/, "\n");
    const next = src.slice(0, insert) + text + src.slice(insert);
    pendingInsertPlacement = { graph_id: graph.graph_id, title: payload.source.title.split(", ")[0] || "", x: point.x + 24, y: point.y + 24 };
    postTransaction({ schema_version: 1, op: "replace_source", revision: latestDoc.revision, source: next, source_edit: "paste_clone" });
    return true;
  }

  function pasteSelection() {
    if (!clipboardState) return showToast("Nothing copied");
    const graph = currentGraphOrNull();
    if (!graph) return;
    const point = lastPointer || viewportCenterGraphPoint();
    if (clipboardState.source && pasteSourceClone(clipboardState, graph, point)) {
      showToast("Pasted source-backed clone");
      return;
    }
    const pasted = [];
    const baseNodes = clipboardState.staged.length ? clipboardState.staged : clipboardState.fallback_nodes;
    for (const item of baseNodes) {
      const action = item.action || { title: item.title || "node", op: "insert_call", callee: item.title || "print", insert_callee: item.title || "print", ret: "Value" };
      const node = createStagedNodeFromAction(action, { x: point.x + 24 + pasted.length * 24, y: point.y + 24 + pasted.length * 18 });
      if (node) pasted.push(node.node_id);
    }
    for (const box of clipboardState.comments || []) {
      createCommentBox({ x: point.x + 24, y: point.y + 24, w: box.w || 260, h: box.h || 160 }, box.title || "Comment", box.color || COMMENT_TINTS[0], false);
    }
    selectedNodeIds = new Set(pasted);
    selectedNodeId = pasted[0] || selectedNodeId;
    showToast("Pasted staged copy");
    if (latestDoc) drawGraph(latestDoc);
  }

  function duplicateSelection() {
    copySelection();
    pasteSelection();
  }

  function deleteLocalSelection() {
    let changed = false;
    for (const id of Array.from(selectedNodeIds)) {
      if ((editorState.stagedNodes || []).some((node) => node.node_id === id)) {
        removeStagedNode(id);
        changed = true;
      }
      if ((editorState.commentBoxes || []).some((box) => box.comment_id === id)) {
        editorState.commentBoxes = (editorState.commentBoxes || []).filter((box) => box.comment_id !== id);
        changed = true;
      }
    }
    if (!changed) return false;
    saveEditorState();
    selectedNodeIds = new Set();
    selectedNodeId = null;
    showToast("Removed local item");
    if (latestDoc) drawGraph(latestDoc);
    return true;
  }

  function removeStagedNode(id) {
    editorState.stagedNodes = (editorState.stagedNodes || []).filter((node) => node.node_id !== id);
    editorState.stagedWires = (editorState.stagedWires || []).filter((wire) => wire.from_pin.indexOf(id + ":") !== 0 && wire.to_pin.indexOf(id + ":") !== 0);
    selectedNodeIds.delete(id);
    if (selectedNodeId === id) selectedNodeId = null;
    saveEditorState();
  }

  function persistStagedNodePositions(ids) {
    let changed = false;
    const allowed = ids ? new Set(ids) : selectedNodeIds;
    for (const node of editorState.stagedNodes || []) {
      if (!allowed.has(node.node_id)) continue;
      node.layout = { x: nodeX(node) / layoutScale.x, y: nodeY(node) / layoutScale.y };
      nodeOffsets.delete(node.node_id);
      autoNodeOffsets.delete(node.node_id);
      changed = true;
    }
    if (changed) saveEditorState();
  }

  function stagedNodeForPin(pin) {
    if (!pin) return null;
    return (editorState.stagedNodes || []).find((node) => node.node_id === pin.node_id) || null;
  }

  function materializeStagedConnection(fromPin, toPin, graph) {
    const fromStage = stagedNodeForPin(fromPin);
    const toStage = stagedNodeForPin(toPin);
    if (fromStage && toStage) {
      showToast("Connect staged nodes to a saved pin first");
      return true;
    }
    const staged = fromStage || toStage;
    if (!staged) return false;
    const realPin = fromStage ? toPin : fromPin;
    if (!realPin || stagedNodeForPin(realPin)) return false;
    if (!compatiblePin(fromPin, toPin)) {
      showToast(connectionPlan(graph, fromPin, toPin).label);
      return true;
    }
    const descriptor = nodeDescriptorForAction(staged.action);
    if (descriptor && descriptor.transaction && descriptor.transaction !== "insert_call" && descriptor.transaction !== "edit_inline_expr") {
      pendingInsertPlacement = { graph_id: selectedGraphId, title: staged.title, x: nodeX(staged), y: nodeY(staged) };
      removeStagedNode(staged.node_id);
      postTransaction({ schema_version: 1, op: descriptor.transaction, revision: latestDoc.revision, graph_id: selectedGraphId });
      window.__jetCanvasStagedMaterialization = "direct-staged-to-real";
      return true;
    }
    const tx = transactionForPaletteInsert(staged.action, realPin, { x: nodeX(staged), y: nodeY(staged) });
    if (!tx) {
      showToast("Staged node needs a saved insertion path");
      return true;
    }
    removeStagedNode(staged.node_id);
    postTransaction(tx);
    window.__jetCanvasStagedMaterialization = "direct-staged-to-real";
    return true;
  }

  function setNodeViewPosition(node, x, y) {
    nodeOffsets.set(node.node_id, {
      x: x - node.layout.x * layoutScale.x,
      y: y - node.layout.y * layoutScale.y
    });
    rememberNodePosition(currentGraphOrNull(), node);
  }

  function graphPositionStore(graph) {
    if (!graph) return null;
    editorState.nodePositions ||= {};
    editorState.nodePositions[graph.graph_id] ||= {};
    return editorState.nodePositions[graph.graph_id];
  }

  function hasSavedNodePositions(graph) {
    const store = graph && editorState.nodePositions && editorState.nodePositions[graph.graph_id];
    return !!store && Object.keys(store).length > 0;
  }

  function rememberNodePosition(graph, node) {
    const store = graphPositionStore(graph);
    if (!store || !node) return;
    store[node.node_id] = { x: nodeX(node), y: nodeY(node) };
  }

  function rememberSelectedNodePositions(graph) {
    if (!graph) return;
    // The first manual placement turns off automatic reflow for this graph.
    // Freeze every node at its currently rendered graph position at that
    // boundary; otherwise untouched nodes snap back to raw backend layout as
    // soon as the selected node is saved.
    const freezeCurrentLayout = !hasSavedNodePositions(graph);
    for (const node of graph.nodes || []) {
      if (freezeCurrentLayout || selectedNodeIds.has(node.node_id)) rememberNodePosition(graph, node);
    }
    saveEditorState();
  }

  function rememberNodePositionsById(graph, ids) {
    if (!graph || !ids) return;
    const wanted = new Set(ids);
    for (const node of graphWithViewState(graph).nodes || []) {
      if (wanted.has(node.node_id) && !node.staged) rememberNodePosition(graph, node);
    }
    saveEditorState();
  }

  function restoreNodePositions(graph) {
    if (!graph || (drag && drag.mode === "node")) return;
    const store = editorState.nodePositions && editorState.nodePositions[graph.graph_id];
    if (!store) return;
    for (const node of graph.nodes || []) {
      const pos = store[node.node_id];
      if (!pos || !Number.isFinite(pos.x) || !Number.isFinite(pos.y)) continue;
      nodeOffsets.set(node.node_id, {
        x: pos.x - node.layout.x * layoutScale.x,
        y: pos.y - node.layout.y * layoutScale.y
      });
    }
  }

  function applyPendingInsertPlacement(doc) {
    const pending = pendingInsertPlacement;
    if (!pending || !doc || !pending.graph_id) return;
    const graph = (doc.graphs || []).find((g) => g.graph_id === pending.graph_id);
    if (!graph) return;
    const wanted = String(pending.title || "").split(".").pop();
    const candidates = (graph.nodes || []).filter((node) => {
      if (graphPositionStore(graph) && graphPositionStore(graph)[node.node_id]) return false;
      return node.title === wanted || node.title === pending.title || String(pending.title || "").endsWith("." + node.title);
    });
    const node = candidates.sort((a, b) => (b.source_span && b.source_span.start || 0) - (a.source_span && a.source_span.start || 0))[0];
    if (!node) return;
    setNodeViewPosition(node, pending.x, pending.y);
    saveEditorState();
    pendingInsertPlacement = null;
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
    saveEditorState();
    showToast(axis === "y" ? "Aligned top" : "Aligned left");
    drawGraph(latestDoc);
  }

  function distributeSelectedNodes(axis) {
    const graph = currentGraphOrNull();
    const nodes = selectedGraphNodes(graph).slice().sort((a, b) =>
      axis === "y" ? nodeY(a) - nodeY(b) : nodeX(a) - nodeX(b));
    if (nodes.length < 3) return showToast("Select three nodes to distribute");
    const first = axis === "y" ? nodeY(nodes[0]) : nodeX(nodes[0]);
    const last = axis === "y" ? nodeY(nodes[nodes.length - 1]) : nodeX(nodes[nodes.length - 1]);
    const step = (last - first) / (nodes.length - 1);
    nodes.forEach((node, index) => {
      const x = axis === "x" ? first + step * index : nodeX(node);
      const y = axis === "y" ? first + step * index : nodeY(node);
      setNodeViewPosition(node, x, y);
    });
    saveEditorState();
    showToast(axis === "y" ? "Distributed vertically" : "Distributed horizontally");
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
    saveEditorState();
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
      runHud.textContent = "run permission loading";
      loadCanvasActions();
      return;
    }
    renderCommandAuthority(run);
  }

  function syncDebugActive() {
    document.body.classList.toggle("is-debug-active", !!debugOverlay);
  }

  function renderCommandAuthority(item) {
    const command = (item.command || []).join(" ");
    runState = { running: false, last: item.title + " permission ready" };
    runHud.textContent = item.available ? runState.last : (item.denied_reason || "command unavailable");
    runHud.classList.remove("is-running");
    window.__jetCanvasRunLoop = { graph_id: selectedGraphId, state: "authority_required", action_id: item.action_id, command: item.command || [] };
    details.innerHTML = `<h2>Command</h2><div class="signature-source"><code>${escapeHtml(command)}</code><span>${escapeHtml(item.writes || "none")} · ${item.requires_confirmation ? "confirmation required" : "read-only"}</span><button id="execute-command-authority">Run</button></div><div class="inline-row dev-only"><b>Permissions</b><code>${escapeHtml((item.authority || []).join("\n"))}</code></div>`;
    const execute = document.getElementById("execute-command-authority");
    if (execute) execute.addEventListener("click", () => executeCommandAuthority(item));
    showToast(item.available ? item.title + " ready" : item.denied_reason || "Command unavailable");
    loadProofRail();
  }

  function executeCommandAuthority(item) {
    if (!latestDoc || !item || !item.available) return;
    const confirmed = !item.requires_confirmation || window.confirm(item.title + " writes " + (item.writes || "outputs") + ". Continue?");
    if (!confirmed) return;
    const body = { schema_version: 1, revision: latestDoc.revision, action_id: item.action_id, confirmed };
    if (item.action_id === "canvas.command:check") body.source_text = sourceEditMode && sourceEditor ? sourceEditor.value : (latestDoc.source_text || "");
    runHud.textContent = item.title + " running";
    runHud.classList.add("is-running");
    fetch(commandUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((json) => ({ ok: r.ok, json })))
      .then((result) => {
        runHud.classList.remove("is-running");
        const doc = result.json || {};
        runHud.textContent = doc.success ? item.title + " passed" : item.title + " failed";
        details.innerHTML = `<h2>Receipt</h2><div class="signature-source"><code>${escapeHtml((doc.command || []).join(" "))}</code><span>${escapeHtml(doc.success ? "success" : "failed")} · ${escapeHtml(String(doc.exit_code ?? "?"))} · ${escapeHtml(String(doc.elapsed_ms || 0))}ms</span></div><div class="inline-row"><b>stdout</b><code>${escapeHtml(doc.stdout || "")}</code></div><div class="inline-row"><b>stderr</b><code>${escapeHtml(doc.stderr || "")}</code></div>`;
        if (doc.action_id === "canvas.command:check") acceptDiagnosticsPayload(doc, "Check");
        loadProofRail();
      })
      .catch((e) => {
        runHud.classList.remove("is-running");
        runHud.textContent = item.title + " failed";
        showToast(String(e));
      });
  }

  function checkCurrentSource() {
    if (!latestDoc) return;
    const item = actionEntries.find((entry) => entry.action_id === "canvas.command:check") || {
      action_id: "canvas.command:check",
      title: "Check project",
      command: ["jet", "check"],
      writes: "none",
      requires_confirmation: false,
      available: true
    };
    executeCommandAuthority(item);
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

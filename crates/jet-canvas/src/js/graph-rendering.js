
// Graph layout, node and wire rendering, minimap, comments, and hit maps.
  let lastDrawnSource = null;

  function rankedGraphLayout(graph) {
    const nodes = graph.nodes || [];
    const byId = new Map(nodes.map((node) => [node.node_id, node]));
    const pinNode = new Map((graph.pins || []).map((pin) => [pin.pin_id, pin.node_id]));
    const rank = new Map(nodes.map((node) => [node.node_id, node.node_id === graph.entry_node ? 0 : Math.max(0, Math.round(rawNodeX(node) / 220))]));
    for (let pass = 0; pass < nodes.length + 3; pass++) {
      let changed = false;
      for (const wire of graph.wires || []) {
        const from = pinNode.get(wire.from_pin);
        const to = pinNode.get(wire.to_pin);
        if (!from || !to || from === to) continue;
        const next = Math.max(rank.get(to) || 0, (rank.get(from) || 0) + 1);
        if (next !== (rank.get(to) || 0)) {
          rank.set(to, next);
          changed = true;
        }
      }
      if (!changed) break;
    }
    const columns = new Map();
    for (const node of nodes) {
      const r = rank.get(node.node_id) || 0;
      if (!columns.has(r)) columns.set(r, []);
      columns.get(r).push(node);
    }
    const sortedRanks = Array.from(columns.keys()).sort((a, b) => a - b);
    const colWidth = new Map();
    for (const r of sortedRanks) {
      colWidth.set(r, Math.max(...columns.get(r).map((node) => nodeSize(graph, node).w), 150));
    }
    const colX = new Map();
    let x = 80;
    for (const r of sortedRanks) {
      colX.set(r, x);
      x += (colWidth.get(r) || 150) + 56;
    }
    const positions = new Map();
    for (const r of sortedRanks) {
      const col = columns.get(r).slice().sort((a, b) => {
        const family = (node) => ({ entry: -2, value: -1, control: 1, exit: 2 }[nodeDescriptor(node) && nodeDescriptor(node).presentation.layout_family] || 0);
        return family(a) - family(b) || rawNodeY(a) - rawNodeY(b) || rawNodeX(a) - rawNodeX(b);
      });
      let y = 70;
      for (const node of col) {
        const size = nodeSize(graph, node);
        positions.set(node.node_id, { x: colX.get(r), y });
        y += size.h + 44;
      }
    }
    for (const wire of graph.wires || []) {
      const from = byId.get(pinNode.get(wire.from_pin));
      const to = byId.get(pinNode.get(wire.to_pin));
      if (!from || !to || from === to) continue;
      const fp = positions.get(from.node_id);
      const tp = positions.get(to.node_id);
      const fs = nodeSize(graph, from);
      if (fp && tp && tp.x < fp.x + fs.w + 40) tp.x = fp.x + fs.w + 40;
    }
    return positions;
  }

  function reflowGraph(graph) {
    if (!graph || !graph.nodes || graph.nodes.length === 0) return;
    if (drag && drag.mode === "node") return;
    autoNodeOffsets = new Map();
    if (hasSavedNodePositions(graph)) return;
    const colGap = 40;
    const ranked = rankedGraphLayout(graph);
    for (const node of graph.nodes) {
      const pos = ranked.get(node.node_id);
      if (pos) autoNodeOffsets.set(node.node_id, { x: pos.x - rawNodeX(node), y: pos.y - rawNodeY(node) });
    }
    const placed = [];
    for (const node of graph.nodes.slice().sort((a, b) => nodeY(a) - nodeY(b) || nodeX(a) - nodeX(b))) {
      const size = nodeSize(graph, node);
      let offset = autoNodeOffset(node);
      let box = { x: rawNodeX(node) + offset.x, y: rawNodeY(node) + offset.y, w: size.w, h: size.h };
      let moved = true;
      while (moved) {
        moved = false;
        for (const other of placed) {
          const overlapX = box.x < other.x + other.w + colGap && box.x + box.w + colGap > other.x;
          const overlapY = box.y < other.y + other.h + 28 && box.y + box.h + 28 > other.y;
          if (overlapX && overlapY) {
            box.y = other.y + other.h + 36;
            moved = true;
          }
        }
      }
      autoNodeOffsets.set(node.node_id, { x: offset.x, y: box.y - rawNodeY(node) });
      placed.push(box);
    }
  }

  function graphBounds(graph) {
    if (!graph || graph.nodes.length === 0) return { minX: 0, minY: 0, maxX: 600, maxY: 360 };
    restoreNodePositions(graph);
    reflowGraph(graph);
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of graph.nodes) {
      const size = nodeSize(graph, n);
      minX = Math.min(minX, nodeX(n));
      minY = Math.min(minY, nodeY(n));
      maxX = Math.max(maxX, nodeX(n) + size.w);
      maxY = Math.max(maxY, nodeY(n) + size.h);
    }
    for (const box of graphCommentBoxes(graph)) {
      minX = Math.min(minX, box.x || 0);
      minY = Math.min(minY, box.y || 0);
      maxX = Math.max(maxX, (box.x || 0) + (box.w || 260));
      maxY = Math.max(maxY, (box.y || 0) + (box.h || 160));
    }
    return { minX, minY, maxX, maxY };
  }

  function fitGraph() {
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    if (!graph) return;
    const b = graphBounds(graph);
    const size = cssSize();
    const compact = compactCanvasMode();
    const leftInset = 22;
    const topInset = compact ? (developerMode ? 154 : 52) : (developerMode ? 108 : 32);
    const bottomInset = 38;
    const zx = (size.width - leftInset - 28) / Math.max(1, b.maxX - b.minX);
    const zy = (size.height - topInset - bottomInset) / Math.max(1, b.maxY - b.minY);
    view.zoom = Math.max(.42, Math.min(1.05, Math.min(zx, zy)));
    view.x = leftInset - b.minX * view.zoom;
    view.y = topInset - b.minY * view.zoom;
    drawGraph(latestDoc);
  }

  function drawGrid(size) {
    ctx.fillStyle = "#101318";
    ctx.fillRect(0, 0, size.width, size.height);
    const major = 128 * view.zoom;
    const minor = 16 * view.zoom;
    ctx.lineWidth = 1;
    for (const step of [minor, major]) {
      if (step < 6) continue;
      ctx.strokeStyle = step === major ? "#20262f" : "#161a21";
      ctx.beginPath();
      let ox = view.x % step;
      let oy = view.y % step;
      for (let x = ox; x < size.width; x += step) { ctx.moveTo(x, 0); ctx.lineTo(x, size.height); }
      for (let y = oy; y < size.height; y += step) { ctx.moveTo(0, y); ctx.lineTo(size.width, y); }
      ctx.stroke();
    }
  }

  function drawTypeLegend(size) {
    if (!detailToggles.types || compactCanvasMode() || size.width < 760 || viewMode === "code") return;
    const items = [
      ["Exec", "exec"],
      ["Bool", "Bool"],
      ["Int", "Int"],
      ["Float", "Float"],
      ["String", "String"],
      ["Failure", "Value?"]
    ];
    const x = Math.max(14, size.width - 430);
    const y = 58;
    const w = Math.min(416, size.width - x - 12);
    const h = 34;
    roundRect(x, y, w, h, 6);
    ctx.fillStyle = "rgba(8,17,29,.78)";
    ctx.fill();
    ctx.strokeStyle = "rgba(54,90,127,.72)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.font = "10px ui-monospace, Consolas, monospace";
    ctx.textAlign = "left";
    let cursor = x + 12;
    for (const [label, type] of items) {
      const color = colorForType(type);
      ctx.beginPath();
      if (type === "exec") {
        ctx.moveTo(cursor, y + 12);
        ctx.lineTo(cursor + 10, y + 17);
        ctx.lineTo(cursor, y + 22);
        ctx.closePath();
      } else {
        ctx.arc(cursor + 5, y + 17, 5, 0, Math.PI * 2);
      }
      ctx.fillStyle = color;
      ctx.fill();
      ctx.fillStyle = "#b9c9df";
      ctx.fillText(label, cursor + 15, y + 20);
      cursor += Math.min(72, 22 + ctx.measureText(label).width);
    }
    ctx.textAlign = "left";
  }

  function isExecPin(pin) {
    return pinRail(pin) === "control";
  }

  function pinsForNode(graph, node, direction, exec) {
    return (graph.pins || []).filter((pin) => pin.node_id === node.node_id && pin.direction === direction && isExecPin(pin) === exec);
  }

  function nodeStyle(node, graph) {
    const descriptor = nodeDescriptor(node);
    const facts = descriptor && descriptor.presentation || {};
    const archetype = facts.style_archetype || descriptor && descriptor.archetype || node.archetype || "value";
    const style = Object.assign({ fill: "rgba(29,33,41,.96)" }, NODE_ARCHETYPE_STYLES[archetype] || NODE_ARCHETYPE_STYLES.value);
    const dataPin = pinsForNode(graph, node, "output", false)[0] || pinsForNode(graph, node, "input", false)[0] || {};
    const typeColor = colorForType(dataPin.type || "unknown");
    const accent = facts.accent === "type" ? typeColor : facts.accent === "archetype" || !facts.accent ? style.accent : facts.accent;
    const header = facts.header === "type" ? typeColor : facts.header === "archetype" || !facts.header ? style.header : facts.header;
    return Object.assign({}, style, { accent, header, label: facts.label || node.kind || "Node", glyph: facts.glyph || "ƒ", subtitle: "" });
  }

  function nodeSubtitle(node, graph) {
    const family = nodeDescriptor(node) && nodeDescriptor(node).presentation.layout_family;
    if (!node || family === "entry" || family === "control" || family === "exit" || isGetterCapsule(node)) return "";
    const modulePath = node.module_path || node.module || "";
    return modulePath && modulePath !== "builtin" ? modulePath : "";
  }

  function nodeKindLabel(node, graph) {
    return nodeStyle(node, graph).label || node.kind || "Node";
  }

  function nodeDescription(node, graph) {
    const hover = nodeDescriptor(node) && nodeDescriptor(node).presentation.hover;
    if (hover) return hover;
    if (!node) return "";
    const modulePath = node.module_path || node.module || "";
    return modulePath && modulePath !== "builtin" ? "Function from " + modulePath + "." : "Calls a function.";
  }

  function shouldDrawNodeBadge(node) {
    return developerMode && !!node && !!node.kind;
  }

  function isGetterCapsule(node) {
    return nodeDescriptor(node) && nodeDescriptor(node).presentation.shape === "capsule";
  }

  function isOperatorNode(node) {
    return node && node.archetype === "function_pure" && /^[+\-*\/%=!<>&|^]+$/.test(node.title || "");
  }

  function simpleEmbeddedValue(expr) {
    const s = String((expr && expr.source) || "").trim();
    return /^[A-Za-z_][A-Za-z0-9_]*$/.test(s) || /^-?\d+(\.\d+)?$/.test(s) || /^"[^"]*"$/.test(s);
  }

  function measureTextPx(font, text) {
    ctx.save();
    ctx.font = font;
    const width = ctx.measureText(String(text || "")).width;
    ctx.restore();
    return width;
  }

  function pinEditorWidth(graph, pin) {
    if (!pin || pin.direction !== "input" || isExecPin(pin)) return 0;
    const kind = editablePinKind(graph, pin);
    if (!kind) return 0;
    return kind === "bool" ? 24 : kind === "enum" ? 88 : 76;
  }

  function pinContentWidth(graph, node, pin) {
    const label = visiblePinLabelInGraph(graph, node, pin);
    const labelW = label ? measureTextPx(`11px ${UI_FONT}`, label) : 0;
    const chipW = !isExecPin(pin) && pin.direction === "output" ? Math.min(96, measureTextPx(`10px ${MONO_FONT}`, pin.type || "Value") + 16) : 0;
    const editorW = pinEditorWidth(graph, pin);
    const patternW = pin.pattern_source ? Math.min(128, measureTextPx(`10px ${MONO_FONT}`, pin.pattern_source) + 16) : 0;
    return PIN_DIAMETER / 2 + NODE_GRID + Math.max(labelW, patternW) + (chipW || editorW ? NODE_GRID + Math.max(chipW, editorW) : 0);
  }

  function measureNodeLayout(graph, node) {
    const allPins = (graph.pins || []).filter((p) => p.node_id === node.node_id);
    const inputPins = allPins.filter((p) => p.direction === "input");
    const outputPins = allPins.filter((p) => p.direction === "output");
    const compact = isGetterCapsule(node) || isOperatorNode(node);
    if (isGetterCapsule(node)) {
      const out = pinsForNode(graph, node, "output", false)[0] || {};
      const titleW = measureTextPx(`13px ${UI_FONT}`, node.title || "");
      const typeW = detailToggles.types ? measureTextPx(`10px ${MONO_FONT}`, out.type || "Value") : 0;
      return { w: Math.max(150, Math.ceil(34 + Math.max(titleW, typeW) + PIN_DIAMETER)), h: detailToggles.types ? 48 : 40, rows: 1, dataTop: 0, execTop: 0 };
    }
    if (isOperatorNode(node)) {
      const rowCount = Math.max(inputPins.length, outputPins.length, 1);
      const glyphW = measureTextPx(`26px ${UI_FONT}`, node.title || "") + 34;
      const leftW = Math.max(0, ...inputPins.map((p) => pinContentWidth(graph, node, p)));
      const rightW = Math.max(0, ...outputPins.map((p) => pinContentWidth(graph, node, p)));
      return { w: Math.max(88, Math.ceil(leftW + glyphW + rightW)), h: Math.max(58, NODE_PAD * 2 + rowCount * NODE_ROW_H), rows: rowCount, dataTop: NODE_PAD + NODE_ROW_H / 2, execTop: 0 };
    }
    const subtitle = nodeSubtitle(node, graph);
    const titleW = measureTextPx(`600 13px ${UI_FONT}`, node.title || "");
    const subtitleW = subtitle ? measureTextPx(`10px ${UI_FONT}`, subtitle) : 0;
    const headerW = NODE_PAD + 14 + NODE_GRID + Math.max(titleW, subtitleW) + NODE_PAD;
    const leftW = Math.max(0, ...inputPins.map((p) => pinContentWidth(graph, node, p)));
    const rightW = Math.max(0, ...outputPins.map((p) => pinContentWidth(graph, node, p)));
    const pinW = leftW + rightW + NODE_PAD * 4;
    const badgeW = shouldDrawNodeBadge(node) ? Math.min(118, measureTextPx(`9.2px ${MONO_FONT}`, String(node.kind || "").toUpperCase()) + 14) + NODE_PAD * 2 : 0;
    const multiInput = (node.edit_affordances || []).includes("append_multi_input");
    const armInput = (node.edit_affordances || []).includes("add_pattern_arm");
    const footerText = multiInput ? `+ ${inputPins.filter((p) => !isExecPin(p)).length} inputs` : armInput ? "+ arm" : "";
    const footerW = footerText ? measureTextPx(`10px ${MONO_FONT}`, footerText) + NODE_PAD * 4 : 0;
    const execRows = Math.max(pinsForNode(graph, node, "input", true).length, pinsForNode(graph, node, "output", true).length);
    const dataRows = Math.max(pinsForNode(graph, node, "input", false).length, pinsForNode(graph, node, "output", false).length);
    const rows = execRows + dataRows;
    const execTop = NODE_HEADER_H + NODE_PAD + NODE_ROW_H / 2;
    const dataTop = execTop + execRows * NODE_ROW_H + (execRows && dataRows ? NODE_GRID : 0);
    const bodyRowsH = rows ? execRows * NODE_ROW_H + dataRows * NODE_ROW_H + (execRows && dataRows ? NODE_GRID : 0) : 0;
    const inlineCount = Math.min(2, ((node && graph && graph.inline_exprs) || []).filter((e) => e.node_id === node.node_id).length);
    const inlineH = inlineCount ? NODE_GRID + inlineCount * 22 : 0;
    const footerH = footerText ? 22 : 0;
    const h = node.archetype === "entry" && rows === 0 ? NODE_HEADER_H : NODE_HEADER_H + NODE_PAD * 2 + bodyRowsH + inlineH + footerH;
    return { w: Math.ceil(Math.max(150, headerW, pinW, badgeW, footerW)), h: Math.ceil(Math.max(node.archetype === "entry" && rows === 0 ? NODE_HEADER_H : 64, h)), rows, execTop, dataTop, footerText };
  }

  function nodeSize(graph, node) {
    window.__jetCanvasMeasuredNodeSizing = true;
    return measureNodeLayout(graph, node);
  }

  function bezierPoint(from, to, t) {
    const controls = bezierControls(from, to);
    const c1 = controls.c1;
    const c2 = controls.c2;
    const mt = 1 - t;
    return {
      x: mt * mt * mt * from.x + 3 * mt * mt * t * c1.x + 3 * mt * t * t * c2.x + t * t * t * to.x,
      y: mt * mt * mt * from.y + 3 * mt * mt * t * c1.y + 3 * mt * t * t * c2.y + t * t * t * to.y
    };
  }

  function bezierControls(from, to) {
    const dx = Math.abs(to.x - from.x);
    const strength = Math.max(32 * view.zoom, Math.min(180 * view.zoom, dx * .48));
    return {
      c1: { x: from.x + strength, y: from.y },
      c2: { x: to.x - strength, y: to.y }
    };
  }

  function drawWireArrow(from, to, color, control) {
    const p = bezierPoint(from, to, .72);
    const q = bezierPoint(from, to, .66);
    const angle = Math.atan2(p.y - q.y, p.x - q.x);
    const len = (control ? 12 : 9) * view.zoom;
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.rotate(angle);
    ctx.beginPath();
    ctx.moveTo(len, 0);
    ctx.lineTo(-len * .55, -len * .55);
    ctx.lineTo(-len * .2, 0);
    ctx.lineTo(-len * .55, len * .55);
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.shadowColor = hexToRgba(color, .55);
    ctx.shadowBlur = control ? 10 : 6;
    ctx.fill();
    ctx.restore();
    ctx.shadowBlur = 0;
  }

  function drawWire(wire, from, to, activeWire, selectedWire) {
    const control = wire.wire_kind === "control" || isExecPin(from.pin);
    const color = activeWire ? "#facc15" : wireColor(wire, from);
    const controls = bezierControls(from, to);
    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.bezierCurveTo(controls.c1.x, controls.c1.y, controls.c2.x, controls.c2.y, to.x, to.y);
    ctx.strokeStyle = "rgba(1,6,12,.86)";
    ctx.lineWidth = control ? Math.max(4.5, 5.4 * view.zoom) : Math.max(3.2, 4.2 * view.zoom);
    ctx.shadowBlur = 0;
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.bezierCurveTo(controls.c1.x, controls.c1.y, controls.c2.x, controls.c2.y, to.x, to.y);
    ctx.strokeStyle = color;
    ctx.lineWidth = activeWire ? Math.max(3.2, (control ? 3.2 : 2.5) * view.zoom + .7) : control ? Math.max(2.5, 2.5 * view.zoom) : Math.max(1.8, 1.8 * view.zoom);
    ctx.shadowColor = activeWire ? "rgba(250,204,21,.72)" : hexToRgba(color, control || selectedWire ? .62 : .34);
    ctx.shadowBlur = activeWire ? 18 : control ? 13 : 8;
    ctx.stroke();
    ctx.shadowBlur = 0;
    if (control || selectedWire || activeWire) drawWireArrow(from, to, color, control);
  }

  function rememberWireEndpoint(wire, from, to) {
    const r = Math.max(8, 10 * view.zoom);
    wireEndpointHit.push({ x: from.x - r, y: from.y - r, w: r * 2, h: r * 2, cx: from.x, cy: from.y, wire, endpoint: "from", pin: from.pin, other: to.pin });
    wireEndpointHit.push({ x: to.x - r, y: to.y - r, w: r * 2, h: r * 2, cx: to.x, cy: to.y, wire, endpoint: "to", pin: to.pin, other: from.pin });
  }

  function hitWireEndpointAt(x, y) {
    let best = null;
    let bestDistance = Infinity;
    for (let i = wireEndpointHit.length - 1; i >= 0; i--) {
      const h = wireEndpointHit[i];
      if (x < h.x || x > h.x + h.w || y < h.y || y > h.y + h.h) continue;
      const dx = x - h.cx;
      const dy = y - h.cy;
      const d = dx * dx + dy * dy;
      if (d < bestDistance) {
        best = h;
        bestDistance = d;
      }
    }
    return best;
  }

  function drawRerouteKnots(graph) {
    const knots = (editorState.rerouteKnots || []).filter((k) => k.graph_id === graph.graph_id);
    for (const knot of knots) {
      const x = sx(knot.x), y = sy(knot.y);
      ctx.beginPath();
      ctx.arc(x, y, Math.max(5, 7 * view.zoom), 0, Math.PI * 2);
      ctx.fillStyle = "#0f172a";
      ctx.fill();
      ctx.strokeStyle = "#facc15";
      ctx.lineWidth = 2;
      ctx.stroke();
    }
  }

  function drawArchetypeHeader(node, style, x, y, w, headerH) {
    roundRect(x, y, w, headerH, 8 * view.zoom);
    const headerGrad = ctx.createLinearGradient(x, y, x + w, y);
    headerGrad.addColorStop(0, style.header || style.accent);
    headerGrad.addColorStop(1, style.header2 || "#151a21");
    ctx.fillStyle = headerGrad;
    ctx.fill();
  }

  function drawStagedOverlay(node, x, y, w, h) {
    if (!node || !node.staged) return;
    ctx.save();
    roundRect(x - 3 * view.zoom, y - 3 * view.zoom, w + 6 * view.zoom, h + 6 * view.zoom, 9 * view.zoom);
    ctx.strokeStyle = "rgba(246,211,101,.92)";
    ctx.lineWidth = Math.max(1.5, 1.5 * view.zoom);
    ctx.setLineDash([7 * view.zoom, 5 * view.zoom]);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.font = `${Math.max(8, 9.5 * view.zoom)}px ${MONO_FONT}`;
    const label = "not saved";
    const tw = ctx.measureText(label).width + 14 * view.zoom;
    roundRect(x + w - tw - 8 * view.zoom, y + 7 * view.zoom, tw, 18 * view.zoom, 4 * view.zoom);
    ctx.fillStyle = "rgba(47,35,13,.94)";
    ctx.fill();
    ctx.strokeStyle = "rgba(246,211,101,.64)";
    ctx.stroke();
    ctx.fillStyle = "#fde68a";
    ctx.textAlign = "center";
    ctx.fillText(label, x + w - tw / 2 - 8 * view.zoom, y + 19.5 * view.zoom);
    ctx.textAlign = "left";
    ctx.restore();
    window.__jetCanvasStagedNodeVisuals = "dashed-not-saved";
  }

  function drawDiagnosticBubble(node, x, y, w, entries, recordHit = true) {
    if (!entries || !entries.length) return;
    const severity = worstDiagnosticSeverity(entries);
    const color = severity === "error" ? "#ef4444" : "#f59e0b";
    const label = entries.length > 9 ? "9+" : String(entries.length);
    const r = Math.max(8, 10 * view.zoom);
    const cx = x + w - 11 * view.zoom;
    const cy = y - 3 * view.zoom;
    ctx.save();
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fillStyle = color;
    ctx.shadowColor = hexToRgba(color, .62);
    ctx.shadowBlur = 12 * view.zoom;
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.strokeStyle = "#1f090c";
    ctx.lineWidth = Math.max(1.5, 2 * view.zoom);
    ctx.stroke();
    ctx.fillStyle = "#fff7ed";
    ctx.font = `700 ${Math.max(8, 10 * view.zoom)}px ${MONO_FONT}`;
    ctx.textAlign = "center";
    ctx.fillText(label, cx, cy + 3.5 * view.zoom);
    ctx.restore();
    if (recordHit) diagnosticHit.push({ x: cx - r, y: cy - r, w: r * 2, h: r * 2, node, entries });
    window.__jetCanvasDiagnosticBubbles = true;
  }

  function drawNode(graph, node, inlineByNode, recordHit = true) {
    const size = nodeSize(graph, node);
    const layout = measureNodeLayout(graph, node);
    const w = size.w * view.zoom, h = size.h * view.zoom;
    const x = sx(nodeX(node)), y = sy(nodeY(node));
    if (view.zoom < .38) {
      const diagnostics = nodeDiagnostics(node);
      roundRect(x, y, Math.max(72, w), Math.max(18, 24 * view.zoom), 4 * view.zoom);
      ctx.fillStyle = selectedNodeIds.has(node.node_id) ? hexToRgba(nodeStyle(node, graph).accent, .28) : "rgba(29,33,41,.88)";
      ctx.fill();
      ctx.strokeStyle = selectedNodeIds.has(node.node_id) ? "#f5a623" : hexToRgba(nodeStyle(node, graph).accent, .48);
      ctx.stroke();
      ctx.fillStyle = "#dbeafe";
      ctx.font = `10px ${UI_FONT}`;
      ctx.fillText(clipText(node.title, 18), x + 8, y + 15);
      drawDiagnosticBubble(node, x, y, Math.max(72, w), diagnostics, recordHit);
      if (recordHit) hit.push({ x, y, w: Math.max(72, w), h: Math.max(18, 24 * view.zoom), node });
      return;
    }
    const selected = selectedNodeIds.has(node.node_id);
    const active = debugOverlay && debugOverlay.active_node_id === node.node_id;
    const searchHit = (searchState.spans || []).some((span) => spansOverlap(node.source_span, span));
    const diagnostics = nodeDiagnostics(node);
    const breakpoint = nodeBreakpoint(node);
    const style = nodeStyle(node, graph);
    const headerH = Math.min(NODE_HEADER_H, size.h) * view.zoom;

    if (isGetterCapsule(node)) {
      const out = pinsForNode(graph, node, "output", false)[0] || {};
      const color = colorForType(out.type || "Value");
      ctx.shadowColor = selected ? hexToRgba("#f5a623", .38) : "rgba(0,0,0,.45)";
      ctx.shadowBlur = selected ? 18 : 12;
      ctx.shadowOffsetY = 4;
      roundRect(x, y, w, h, 20 * view.zoom);
      ctx.fillStyle = hexToRgba(color, .12);
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      ctx.strokeStyle = selected ? "#f5a623" : color;
      ctx.lineWidth = selected ? 1.8 : 1;
      ctx.stroke();
      drawStagedOverlay(node, x, y, w, h);
      ctx.fillStyle = "#f8fbff";
      ctx.font = `${Math.max(11, 13 * view.zoom)}px ${UI_FONT}`;
      ctx.textAlign = "left";
      ctx.fillText(ellipsizeText(node.title, w - 40 * view.zoom), x + 16 * view.zoom, y + 25 * view.zoom);
      if (out.pin_id) drawPin(out, x + w, y + h / 2, "output", recordHit);
      if (detailToggles.types) {
        ctx.fillStyle = color;
        ctx.font = `${Math.max(8, 9.5 * view.zoom)}px ${MONO_FONT}`;
        ctx.fillText(ellipsizeText(out.type || "Value", w - 40 * view.zoom), x + 16 * view.zoom, y + 36 * view.zoom);
      }
      drawDiagnosticBubble(node, x, y, w, diagnostics, recordHit);
      if (recordHit) hit.push({ x, y, w, h, node });
      window.__jetCanvasGetterCapsules = true;
      return;
    }

    if (isOperatorNode(node)) {
      const inputs = pinsForNode(graph, node, "input", false);
      const outputs = pinsForNode(graph, node, "output", false);
      ctx.shadowColor = selected ? "rgba(245,166,35,.34)" : "rgba(0,0,0,.42)";
      ctx.shadowBlur = selected ? 18 : 10;
      ctx.shadowOffsetY = 4;
      roundRect(x, y, w, h, 8 * view.zoom);
      ctx.fillStyle = "rgba(29,33,41,.96)";
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      ctx.strokeStyle = selected ? "#f5a623" : hexToRgba(style.accent, .55);
      ctx.lineWidth = selected ? 1.8 : 1;
      ctx.stroke();
      drawStagedOverlay(node, x, y, w, h);
      ctx.fillStyle = "#f2f4f8";
      ctx.font = `${Math.max(19, 26 * view.zoom)}px ${UI_FONT}`;
      ctx.textAlign = "center";
      ctx.fillText(ellipsizeText(node.title, w - 28 * view.zoom), x + w / 2, y + h / 2 + 9 * view.zoom);
      ctx.textAlign = "left";
      inputs.forEach((p, i) => drawSocketRow(p, x, y + (NODE_PAD + NODE_ROW_H / 2 + i * NODE_ROW_H) * view.zoom, w, "input", recordHit));
      outputs.forEach((p, i) => drawSocketRow(p, x, y + (NODE_PAD + NODE_ROW_H / 2 + i * NODE_ROW_H) * view.zoom, w, "output", recordHit));
      drawDiagnosticBubble(node, x, y, w, diagnostics, recordHit);
      if (recordHit) hit.push({ x, y, w, h, node });
      return;
    }

    ctx.shadowColor = active ? "rgba(250,204,21,.50)" : selected ? "rgba(245,166,35,.34)" : searchHit ? "rgba(192,132,252,.35)" : "rgba(0,0,0,.45)";
    ctx.shadowBlur = active ? 28 : selected ? 22 : searchHit ? 18 : 12;
    ctx.shadowOffsetY = 4;
    roundRect(x, y, w, h, 8 * view.zoom);
    ctx.fillStyle = "rgba(29,33,41,.96)";
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.strokeStyle = active ? "#facc15" : selected ? "#f5a623" : searchHit ? "#c084fc" : "#0b0d11";
    ctx.lineWidth = active ? 2.2 : selected ? 1.5 : 1;
    ctx.stroke();
    drawStagedOverlay(node, x, y, w, h);
    if (nodeDescriptor(node) && nodeDescriptor(node).presentation.accent === "type" && !isGetterCapsule(node)) {
      ctx.fillStyle = style.accent;
      ctx.fillRect(x, y + 7 * view.zoom, Math.max(3, 3 * view.zoom), h - 14 * view.zoom);
      window.__jetCanvasBindingTypeAccent = true;
    }
    if (selected) {
      ctx.save();
      roundRect(x - 2 * view.zoom, y - 2 * view.zoom, w + 4 * view.zoom, h + 4 * view.zoom, 9 * view.zoom);
      ctx.strokeStyle = "rgba(245,166,35,.62)";
      ctx.shadowColor = "rgba(245,166,35,.32)";
      ctx.shadowBlur = 10 * view.zoom;
      ctx.lineWidth = 1.5 * view.zoom;
      ctx.stroke();
      ctx.restore();
    }

    roundRect(x, y, w, headerH, 8 * view.zoom);
    const headerGrad = ctx.createLinearGradient(x, y, x + w, y);
    headerGrad.addColorStop(0, style.header || style.accent);
    headerGrad.addColorStop(1, style.header2 || "#151a21");
    ctx.fillStyle = headerGrad;
    ctx.fill();
    ctx.fillStyle = "rgba(255,255,255,.86)";
    ctx.font = `${Math.max(10, 14 * view.zoom)}px ${UI_FONT}`;
    ctx.textAlign = "center";
    ctx.fillText(style.glyph || "ƒ", x + 16 * view.zoom, y + 17.5 * view.zoom);
    ctx.textAlign = "left";
    ctx.fillStyle = "#f8fbff";
    ctx.font = `600 ${Math.max(10, 13 * view.zoom)}px ${UI_FONT}`;
    ctx.fillText(ellipsizeText(node.title, w - 46 * view.zoom), x + 30 * view.zoom, y + 17.5 * view.zoom);
    const subtitle = nodeSubtitle(node, graph);
    if (subtitle) {
      ctx.fillStyle = "rgba(255,255,255,.65)";
      ctx.font = `${Math.max(8, 10 * view.zoom)}px ${UI_FONT}`;
      ctx.fillText(ellipsizeText(subtitle, w - 46 * view.zoom), x + 30 * view.zoom, y + 29.5 * view.zoom);
    }

    if (shouldDrawNodeBadge(node)) {
      const badgeText = String(node.kind || "").toUpperCase();
      ctx.font = `${Math.max(7.5, 9.2 * view.zoom)}px ${MONO_FONT}`;
      const badgeW = Math.min(118 * view.zoom, ctx.measureText(badgeText).width + 14 * view.zoom);
      const badgeY = y + headerH + 6 * view.zoom;
      roundRect(x + 14 * view.zoom, badgeY, badgeW, 20 * view.zoom, 4 * view.zoom);
      ctx.fillStyle = hexToRgba(style.accent, .14);
      ctx.fill();
      ctx.strokeStyle = hexToRgba(style.accent, .42);
      ctx.lineWidth = Math.max(.8, view.zoom);
      ctx.stroke();
      ctx.fillStyle = style.accent;
      ctx.textAlign = "center";
      ctx.fillText(clipText(badgeText, 16), x + 14 * view.zoom + badgeW / 2, badgeY + 13.5 * view.zoom);
      ctx.textAlign = "left";
    }

    if (breakpoint) {
      ctx.beginPath();
      ctx.arc(x + w - 17 * view.zoom, y + 17 * view.zoom, 6 * view.zoom, 0, Math.PI * 2);
      ctx.fillStyle = "#ef4444";
      ctx.fill();
    }

    const execIn = pinsForNode(graph, node, "input", true);
    const execOut = pinsForNode(graph, node, "output", true);
    const execTop = layout.execTop;
    execIn.forEach((p, i) => drawSocketRow(p, x, y + (execTop + i * NODE_ROW_H) * view.zoom, w, "input", recordHit));
    execOut.forEach((p, i) => drawSocketRow(p, x, y + (execTop + i * NODE_ROW_H) * view.zoom, w, "output", recordHit));

    const inputs = pinsForNode(graph, node, "input", false);
    const outputs = pinsForNode(graph, node, "output", false);
    const execRows = Math.max(execIn.length, execOut.length);
    const dataTop = layout.dataTop || (NODE_HEADER_H + NODE_PAD + NODE_ROW_H / 2);
    inputs.forEach((p, i) => {
      const rowY = y + (dataTop + i * NODE_ROW_H) * view.zoom;
      drawSocketRow(p, x, rowY, w, "input", recordHit);
      const editorMax = Math.max(24, Math.min(96, size.w - 92));
      const editorX = x + Math.min(w - (editorMax + NODE_PAD) * view.zoom, 74 * view.zoom);
      drawPinDefaultEditor(graph, p, editorX, rowY, recordHit, editorMax);
    });
    outputs.forEach((p, i) => drawSocketRow(p, x, y + (dataTop + i * NODE_ROW_H) * view.zoom, w, "output", recordHit));

    const inline = (selected || view.zoom >= .95 ? (inlineByNode.get(node.node_id) || []) : []).slice(0, 2);
    inline.forEach((expr, i) => {
      const cy = y + (dataTop + Math.max(inputs.length, outputs.length) * NODE_ROW_H + NODE_GRID + 9 + i * 22) * view.zoom;
      roundRect(x + 12 * view.zoom, cy - 13 * view.zoom, w - 24 * view.zoom, 18 * view.zoom, 5 * view.zoom);
      ctx.fillStyle = simpleEmbeddedValue(expr) ? "rgba(212,212,216,.16)" : "rgba(246,211,101,.11)";
      ctx.fill();
      ctx.strokeStyle = simpleEmbeddedValue(expr) ? "rgba(212,212,216,.38)" : "rgba(246,211,101,.24)";
      ctx.lineWidth = Math.max(.8, view.zoom);
      ctx.stroke();
      ctx.fillStyle = simpleEmbeddedValue(expr) ? "#e4e4e7" : "#f6d365";
      ctx.font = `${Math.max(9, 11 * view.zoom)}px ${MONO_FONT}`;
      ctx.fillText(ellipsizeText(expr.source, w - 38 * view.zoom), x + 19 * view.zoom, cy);
      if (simpleEmbeddedValue(expr)) window.__jetCanvasEmbeddedVariables = true;
    });

    if (layout.footerText) {
      ctx.font = `${Math.max(8, 10 * view.zoom)}px ${MONO_FONT}`;
      const badgeW = Math.min(w - 2 * NODE_PAD * view.zoom, ctx.measureText(layout.footerText).width + 16 * view.zoom);
      const badgeH = 18 * view.zoom;
      const bx = x + w - NODE_PAD * view.zoom - badgeW;
      const by = y + h - NODE_PAD * view.zoom - badgeH;
      roundRect(bx, by, badgeW, badgeH, 4 * view.zoom);
      ctx.fillStyle = "rgba(7,12,19,.92)";
      ctx.fill();
      ctx.strokeStyle = "rgba(138,143,152,.58)";
      ctx.lineWidth = Math.max(1, view.zoom);
      ctx.stroke();
      ctx.fillStyle = "#9db4d2";
      ctx.textAlign = "center";
      ctx.fillText(ellipsizeText(layout.footerText, badgeW - 12 * view.zoom), bx + badgeW / 2, by + 12.5 * view.zoom);
      ctx.textAlign = "left";
      if (recordHit) pinEditorHit.push({ x: bx, y: by, w: badgeW, h: badgeH, node, kind: layout.footerText.includes("arm") ? "add_pattern_arm" : "append_multi_input" });
      window.__jetCanvasMultiInputAppend = true;
    }

    drawDiagnosticBubble(node, x, y, w, diagnostics, recordHit);
    if (recordHit) hit.push({ x, y, w, h, node });
  }

  function visibleGraphNodes(graph) {
    const size = cssSize();
    const margin = 360;
    if (!graph || graph.nodes.length < 180) {
      window.__jetCanvasVirtualizationStats = { total: graph ? graph.nodes.length : 0, visible: graph ? graph.nodes.length : 0, lod: view.zoom < .38 };
      return graph ? graph.nodes : [];
    }
    const visible = graph.nodes.filter((node) => {
      const ns = nodeSize(graph, node);
      const x = sx(nodeX(node)), y = sy(nodeY(node));
      return x + ns.w * view.zoom > -margin && y + ns.h * view.zoom > -margin && x < size.width + margin && y < size.height + margin;
    });
    window.__jetCanvasVirtualizationStats = { total: graph.nodes.length, visible: visible.length, lod: view.zoom < .38 };
    return visible;
  }

  function drawGraph(doc) {
    const source = doc.source_text || "";
    const reprojected = lastDrawnSource !== null && lastDrawnSource !== source;
    lastDrawnSource = source;
    latestDoc = doc;
    loadDebugState(doc);
    const sourceGraph = currentGraph(doc);
    if (!sourceGraph) return;
    const graph = graphWithViewState(sourceGraph);
    if (reprojected) {
      hoverPin = null;
      hoverNode = null;
      hoverDiagnostic = null;
    } else if (hoverNode) {
      hoverNode = graph.nodes.find((node) => node.node_id === hoverNode.node_id) || null;
    }
    selectedGraphId = graph.graph_id;
    window.__jetCanvasSelectedGraphId = selectedGraphId;
    if (!selectedNodeId || (!graph.nodes.some((n) => n.node_id === selectedNodeId) && !graphCommentBoxes(sourceGraph).some((b) => b.comment_id === selectedNodeId))) selectedNodeId = graph.entry_node;
    if (selectedNodeIds.size === 0 && selectedNodeId) selectedNodeIds.add(selectedNodeId);
    selectedNodeIds = new Set([...selectedNodeIds].filter((id) => graph.nodes.some((n) => n.node_id === id) || graphCommentBoxes(sourceGraph).some((b) => b.comment_id === id)));
    syncGraphPicker(doc);
    syncGraphList(doc);
    syncGraphStrip(doc);
    syncVariablesList(graph);
    fit();
    const size = cssSize();
    drawGrid(size);
    hit = [];
    pinPoints = new Map();
    nodeBounds = new Map();
    pinHit = [];
    pinEditorHit = [];
    wireEndpointHit = [];
    diagnosticHit = [];
    graphSelect.value = selectedGraphId;
    const pins = new Map(graph.pins.map((p) => [p.pin_id, p]));
    connectedPinIds = new Set((graph.wires || []).flatMap((wire) => [wire.from_pin, wire.to_pin]));
    const nodes = new Map(graph.nodes.map((n) => [n.node_id, n]));
    const inlineByNode = new Map();
    for (const expr of graph.inline_exprs || []) {
      if (!inlineByNode.has(expr.node_id)) inlineByNode.set(expr.node_id, []);
      inlineByNode.get(expr.node_id).push(expr);
    }
    restoreNodePositions(graph);
    reflowGraph(graph);

    drawCommentRegions(graph);
    const visibleNodes = visibleGraphNodes(graph);
    const visibleIds = new Set(visibleNodes.map((node) => node.node_id));

    for (const node of visibleNodes) {
      drawNode(graph, node, inlineByNode);
    }

    for (const wire of graph.wires) {
      const from = pinPoints.get(wire.from_pin);
      const to = pinPoints.get(wire.to_pin);
      if (!from || !to) continue;
      if (!visibleIds.has(from.pin.node_id) && !visibleIds.has(to.pin.node_id)) continue;
      const activeWire = debugOverlay && debugOverlay.active_wire_id === wire.wire_id;
      const selectedWire = selectedNodeIds.has(from.pin.node_id) || selectedNodeIds.has(to.pin.node_id);
      rememberWireEndpoint(wire, from, to);
      drawWire(wire, from, to, activeWire, selectedWire);
      if (detailToggles.types && (activeWire || selectedWire || view.zoom >= 1.05)) drawWireTypeBadge(wire, from, to);
    }

    drawRerouteKnots(graph);

    for (const node of visibleNodes) {
      drawNode(graph, node, inlineByNode, false);
    }

    if (drag && drag.mode === "pin") {
      drawCompatibleDropTargets(graph, drag.pin);
      const from = pinPoints.get(drag.pin.pin_id);
      if (from) {
        const controls = bezierControls(from, { x: drag.mx, y: drag.my });
        const plan = connectionPlan(graph, drag.pin, hoverPin);
        syncWireStatus({ title: plan.ok ? "Wire preview" : "Wire refused", detail: plan.label, color: plan.color });
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.bezierCurveTo(controls.c1.x, controls.c1.y, controls.c2.x, controls.c2.y, drag.mx, drag.my);
        ctx.strokeStyle = plan.color;
        ctx.lineWidth = Math.max(2.5, 4 * view.zoom);
        ctx.setLineDash(plan.ok ? [12, 6] : [4, 6]);
        ctx.stroke();
        ctx.setLineDash([]);
        drawConnectionBadge(plan, drag.mx, drag.my);
      }
    }

    if (pendingPin && (!drag || drag.mode !== "pin")) {
      syncWireStatus({ title: "Destination needed", detail: pinName(pendingPin) + " : " + exactPinType(pendingPin), color: colorForType(pendingPin.type || "Value") });
      drawCompatibleDropTargets(graph, pendingPin);
      const from = pinPoints.get(pendingPin.pin_id);
      if (from) {
        ctx.beginPath();
        ctx.arc(from.x, from.y, Math.max(17, 22 * view.zoom), 0, Math.PI * 2);
        ctx.strokeStyle = "#7dd3fc";
        ctx.lineWidth = Math.max(1.5, 2.4 * view.zoom);
        ctx.setLineDash([8, 5]);
        ctx.stroke();
        ctx.setLineDash([]);
        drawConnectionBadge({ ok: true, label: "Select destination pin", color: "#7dd3fc" }, from.x + 36 * view.zoom, from.y - 12 * view.zoom);
      }
    }

    if (drag && drag.mode === "marquee") {
      const x = Math.min(drag.x, drag.mx), y = Math.min(drag.y, drag.my);
      const w = Math.abs(drag.mx - drag.x), h = Math.abs(drag.my - drag.y);
      ctx.setLineDash([6, 5]);
      ctx.strokeStyle = "#67e8f9";
      ctx.lineWidth = 1.5;
      ctx.strokeRect(x, y, w, h);
      ctx.fillStyle = "rgba(103,232,249,.08)";
      ctx.fillRect(x, y, w, h);
      ctx.setLineDash([]);
    }

    if (hoverPin && (!drag || drag.mode !== "pin")) drawPinHoverTooltip(hoverPin);
    drawMinimap(graph);
    drawTypeLegend(size);
    if (hoverDiagnostic && hoverDiagnostic.entries && hoverDiagnostic.entries[0]) {
      const entry = hoverDiagnostic.entries[0];
      syncWireStatus({ title: entry.code, detail: diagnosticFullText(entry), color: entry.severity === "error" ? "#ef4444" : "#f59e0b" });
    } else if (!pendingPin && (!drag || drag.mode !== "pin")) {
      if (hoverNode) {
        syncWireStatus({
          title: hoverNode.title,
          detail: nodeDescription(hoverNode, graph),
          color: nodeStyle(hoverNode, graph).accent
        });
      } else {
        syncWireStatus(null);
      }
    }
    const selectedNode = nodes.get(selectedNodeId);
    if (selectedVariableName) {
      renderVariableDetails(graph, selectedVariableName);
    } else {
      updateDetails(graph, selectedNode, graph.pins.filter((p) => p.node_id === selectedNodeId), inlineByNode.get(selectedNodeId) || []);
    }
    syncGraphOverview(graph, selectedNode);
    window.__jetCanvasNonblankPixels = graph.nodes.length > 0 ? 1 : 0;
    window.__jetCanvasPendingPin = pendingPin ? { pin_id: pendingPin.pin_id, name: pendingPin.name, type: pendingPin.type, direction: pendingPin.direction } : null;
    const hitMap = {
      graph_id: graph.graph_id,
      nodes: hit.map((h) => ({ node_id: h.node.node_id, title: h.node.title, kind: h.node.kind, x: h.x, y: h.y, w: h.w, h: h.h })),
      pins: pinHit.map((h) => ({ pin_id: h.pin.pin_id, node_id: h.pin.node_id, name: h.pin.name, type: h.pin.type, direction: h.pin.direction, role: h.pin.role || null, pattern_source: h.pin.pattern_source || null, append_op: h.pin.append_op || null, source_span: h.pin.source_span || null, pattern_source_span: h.pin.pattern_source_span || null, x: h.x, y: h.y, w: h.w, h: h.h, cx: h.cx, cy: h.cy })),
      diagnostics: diagnosticHit.map((h) => ({ node_id: h.node.node_id, count: h.entries.length, severity: worstDiagnosticSeverity(h.entries), codes: h.entries.map((entry) => entry.code) }))
    };
    nodeBounds = new Map(hitMap.nodes.map((n) => [n.node_id, n]));
    window.__jetCanvasHitMap = hitMap;
    window.__jetCanvasNodeBounds = Object.fromEntries(nodeBounds.entries());
    const rect = canvas.getBoundingClientRect();
    window.__jetCanvasPinPoints = Object.fromEntries(Array.from(pinPoints.entries()).map(([pin_id, point]) => [pin_id, {
      pin_id,
      canvas_x: point.x,
      canvas_y: point.y,
      client_x: rect.left + point.x,
      client_y: rect.top + point.y,
      name: point.pin && point.pin.name,
      type: point.pin && point.pin.type,
      direction: point.pin && point.pin.direction
      , role: point.pin && point.pin.role || null,
      append_op: point.pin && point.pin.append_op || null,
      source_span: point.pin && point.pin.source_span || null,
      pattern_source_span: point.pin && point.pin.pattern_source_span || null
    }]));
    window.__jetCanvasWireEndpoints = wireEndpointHit.map((h) => ({
      wire_id: h.wire && h.wire.wire_id,
      wire_kind: h.wire && h.wire.wire_kind,
      endpoint: h.endpoint,
      pin_id: h.pin && h.pin.pin_id,
      other_pin_id: h.other && h.other.pin_id,
      client_x: rect.left + h.cx,
      client_y: rect.top + h.cy,
      from_source_span: h.wire && h.wire.from_source_span,
      to_source_span: h.wire && h.wire.to_source_span
    }));
    window.__jetCanvasStagedRegistry = (editorState.stagedNodes || []).map((node) => ({
      node_id: node.node_id,
      node_descriptor_id: node.node_descriptor_id,
      title: node.title,
      kind: node.kind,
      graph_id: node.graph_id,
      pins: node.pins || []
    }));
    window.__jetCanvasTest = {
      hitMap,
      nodeBounds: window.__jetCanvasNodeBounds,
      pinPoints: window.__jetCanvasPinPoints,
      wireEndpoints: window.__jetCanvasWireEndpoints,
      stagedRegistry: window.__jetCanvasStagedRegistry,
      graphId: selectedGraphId,
      selectedNodeId,
      selectedNodeTitle: selectedNode && selectedNode.title || "",
      selectedVariableName,
      view: { x: view.x, y: view.y, zoom: view.zoom },
      sourceText: doc.source_text || "",
      doc: latestDoc,
      nodeDescriptors: latestDoc.node_descriptors || [],
      descriptorConsumption: graph.nodes.map((node) => {
        const descriptor = nodeDescriptor(node);
        return {
          node_id: node.node_id,
          node_descriptor_id: node.node_descriptor_id,
          presentation_label: descriptor && descriptor.presentation.label || "",
          presentation_glyph: descriptor && descriptor.presentation.glyph || "",
          hover: descriptor && descriptor.presentation.hover || "",
          default_editor: descriptor && descriptor.default_editor || ""
        };
      }),
      problems: activeDiagnostics().map((entry) => ({ code: entry.code, what: entry.what, severity: entry.severity, rendered: diagnosticFullText(entry), source_span: entry.source_span })),
      diagnosticsByNode: hitMap.diagnostics,
      nodeCount: graph.nodes.length,
      defaultEditorFactsConsumed: !!window.__jetCanvasDefaultEditorFacts,
      undoDepth: undoStack.length,
      redoDepth: redoStack.length,
      undoLimit: UNDO_DEPTH,
      lastToast: toast ? toast.textContent : "",
      hoveredNodeTitle: hoverNode && hoverNode.title || "",
      hoveredNodeDescription: hoverNode ? nodeDescription(hoverNode, graph) : "",
      graphTitle: graph.title || graph.graph_id,
      selectedNodeIds: Array.from(selectedNodeIds),
      savedNodePositions: JSON.parse(JSON.stringify((editorState.nodePositions || {})[graph.graph_id] || {})),
      renderedCommentRegions: renderedCommentRegions.map((region) => Object.assign({}, region)),
      favorites: (editorState.favorites || []).slice(),
      favoriteCandidate: actionEntries[0] && (actionEntries[0].action_id || actionEntries[0].callee || actionEntries[0].title) || "",
      favoriteCandidateTitle: actionEntries[0] && actionEntries[0].title || "",
      favoriteCandidateRank: actionEntries[0] ? rankAction(actionEntries[0]) : 0,
      loadCoreCatalog: (query) => loadCoreCatalogActions(query || "").then(() => window.__jetCanvasCoreCatalogPalette || actionEntries.length),
      openCoreCatalogPalette: (query) => { openCoreCatalogPalette(query || ""); return true; },
      openGraphActionPalette: (query) => { openGraphActionPalette(window.innerWidth / 2 - 210, 72, query || "", viewportCenterGraphPoint()); return true; },
      switchGraphByTitle: (title) => {
        const target = (latestDoc && latestDoc.graphs || []).find((g) => g.title === title || String(g.title || "").includes(title));
        if (!target) return false;
        switchGraph(target.graph_id);
        return true;
      },
      selectNodeTitles: (titles) => {
        const selected = (titles || []).map((title) =>
          graph.nodes.find((node) => node.title === title && node.source_span)).filter(Boolean);
        selectedNodeIds = new Set(selected.map((node) => node.node_id));
        selectedNodeId = selected.length ? selected[selected.length - 1].node_id : null;
        drawGraph(latestDoc);
        return selected.length;
      },
      actionEntries: () => actionEntries.map((entry) => {
        const availability = actionAvailability(entry, currentGraphOrNull());
        return {
          title: entry.title,
          detail: entry.detail || "",
          group: entry.group || "",
          kind: entry.kind || "",
          node_descriptor_id: entry.node_descriptor_id || "",
          module_path: entry.module_path || "",
          signature: entry.signature || "",
          summary: entry.summary || "",
          pure: !!entry.pure,
          pins: entry.pins || [],
          ret: entry.ret || "",
          op: entry.op || "",
          action_id: entry.action_id || "",
          callee: entry.callee || "",
          insert_callee: entry.insert_callee || entry.callee || "",
          args: entry.args || entry.default_args || [],
          available: availability.available,
          denied_reason: availability.reason,
          unavailable_reason_code: availability.code
        };
      }),
      openPinMenu: (nodeTitle, pinName) => {
        const g = currentGraphOrNull();
        if (!g) return false;
        const node = (g.nodes || []).find((n) => n.title === nodeTitle || String(n.title || "").includes(nodeTitle));
        if (!node) return false;
        const pins = (g.pins || []).filter((p) => p.node_id === node.node_id);
        const pin = pins.find((p) => p.name === pinName || String(p.name || "").includes(pinName))
          || pins.find((p) => p.direction === pinName)
          || pins[0];
        if (!pin) return false;
        const point = pinPoints.get(pin.pin_id);
        const r = canvas.getBoundingClientRect();
        const actions = functionsForPin(pin).map((entry) => ({
          title: entry.title,
          detail: entry.detail,
          group: paletteCategoryForAction(entry),
          kind: entry.kind,
          node_descriptor_id: entry.node_descriptor_id,
          module_path: entry.module_path,
          signature: entry.signature,
          summary: entry.summary,
          pure: entry.pure,
          pins: entry.pins,
          ret: entry.ret,
          action_id: entry.action_id,
          callee: entry.callee,
          insert_callee: entry.insert_callee,
          args: entry.args,
          available: entry.available,
          denied_reason: entry.denied_reason,
          unavailable_reason_code: entry.unavailable_reason_code,
          run: entry.run ? () => entry.run() : () => runPalette(entry, pin)
        }));
        openActionPalette(
          point ? r.left + point.x : r.left + 120,
          point ? r.top + point.y : r.top + 120,
          "Pin actions",
          actions,
          { pin }
        );
        return true;
      },
      setSourceEditor: (text) => {
        setSourceEditMode(true);
        if (sourceEditor) sourceEditor.value = String(text || "");
        return true;
      },
      postTransaction: (body) => {
        window.__jetCanvasLastTxResult = null;
        postTransaction(body);
        return true;
      },
      undo: undoTransaction,
      redo: redoTransaction,
      setViewMode: (mode) => { setViewMode(mode); return viewMode; },
      runCurrentGraph: () => { runCurrentGraph(); return true; },
      selectVariable: (name) => { selectVariable(name); return true; },
      copySelection: () => { copySelection(); return true; },
      pasteSelection: () => { pasteSelection(); return true; },
      checkCurrentSource,
      jumpProblem: (index) => {
        const entry = activeDiagnostics()[Number(index) || 0];
        if (!entry) return false;
        jumpToDiagnostic(entry);
        return true;
      }
    };
    canvas.dataset.hitMap = JSON.stringify(hitMap);
    updateGraphNav(graph);
    const rails = (graph.rails && graph.rails.kinds ? graph.rails.kinds.join(", ") : "data");
    graphMeta.textContent = graph.nodes.length + " nodes / " + graph.wires.length + " wires / " + rails;
    zoomLabel.textContent = Math.round(view.zoom * 100) + "%";
    if (toolbarZoom) toolbarZoom.textContent = zoomLabel.textContent;
    sourceId.textContent = doc.source_id || "source";
    revision.textContent = (doc.revision || "").slice(0, 18);
  }

  graphSelect.addEventListener("change", function () {
    switchGraph(graphSelect.value);
  });

  function drawMinimap(graph) {
    mini.clearRect(0, 0, minimap.width, minimap.height);
    mini.fillStyle = "#07101c";
    mini.fillRect(0, 0, minimap.width, minimap.height);
    const b = graphBounds(graph);
    const scale = Math.min((minimap.width - 20) / Math.max(1, b.maxX - b.minX), (minimap.height - 20) / Math.max(1, b.maxY - b.minY));
    for (const box of graphCommentBoxes(graph)) {
      mini.fillStyle = hexToRgba(box.color || COMMENT_TINTS[0], .58);
      mini.fillRect(10 + ((box.x || 0) - b.minX) * scale, 10 + ((box.y || 0) - b.minY) * scale, Math.max(12, (box.w || 260) * scale), Math.max(8, (box.h || 160) * scale));
    }
    for (const n of graph.nodes) {
      const size = nodeSize(graph, n);
      const style = nodeStyle(n, graph);
      mini.fillStyle = n.node_id === selectedNodeId ? "#f5a623" : style.accent;
      mini.fillRect(10 + (nodeX(n) - b.minX) * scale, 10 + (nodeY(n) - b.minY) * scale, Math.max(16, size.w * scale), Math.max(9, size.h * scale));
    }
  }

  function nodesInRegion(graph, region) {
    return (graph.nodes || []).filter((node) => spansOverlap(node.source_span, region.source_span));
  }

  function commentRegionBounds(graph, region) {
    const b = region.bounds || {};
    if (b.w > 0 && b.h > 0) return { x: b.x || 0, y: b.y || 0, w: b.w, h: b.h };
    const nodes = nodesInRegion(graph, region);
    if (nodes.length === 0) return { x: 120, y: 120, w: 360, h: 180 };
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const node of nodes) {
      const size = nodeSize(graph, node);
      minX = Math.min(minX, nodeX(node));
      minY = Math.min(minY, nodeY(node));
      maxX = Math.max(maxX, nodeX(node) + size.w);
      maxY = Math.max(maxY, nodeY(node) + size.h);
    }
    return { x: minX - 26, y: minY - 36, w: maxX - minX + 52, h: maxY - minY + 70 };
  }

  function drawCommentRegions(graph) {
    commentHit = [];
    renderedCommentRegions = [];
    for (const box of graphCommentBoxes(graph)) {
      const x = sx(box.x || 0), y = sy(box.y || 0), w = (box.w || 260) * view.zoom, h = (box.h || 160) * view.zoom;
      const selected = selectedNodeIds.has(box.comment_id);
      roundRect(x, y, w, h, 8 * view.zoom);
      ctx.fillStyle = hexToRgba(box.color || COMMENT_TINTS[0], .16);
      ctx.fill();
      ctx.strokeStyle = selected ? "#f5a623" : hexToRgba(box.color || COMMENT_TINTS[0], .72);
      ctx.lineWidth = selected ? 1.8 : 1.2;
      ctx.setLineDash([9, 6]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = hexToRgba(box.color || COMMENT_TINTS[0], .28);
      ctx.fillRect(x, y, w, Math.min(h, 28 * view.zoom));
      ctx.fillStyle = "#eaf3ff";
      ctx.font = `${Math.max(11, 13 * view.zoom)}px ${UI_FONT}`;
      ctx.fillText(ellipsizeText(box.title || "Comment", w - 24 * view.zoom), x + 12 * view.zoom, y + 19 * view.zoom);
      renderedCommentRegions.push({ title: box.title || "Comment", x, y, w, h, source_backed: false });
      const grip = Math.max(12, 14 * view.zoom);
      ctx.strokeStyle = hexToRgba(box.color || COMMENT_TINTS[0], .86);
      ctx.beginPath();
      ctx.moveTo(x + w - grip, y + h - 4 * view.zoom);
      ctx.lineTo(x + w - 4 * view.zoom, y + h - grip);
      ctx.stroke();
      commentHit.push({ x, y, w, h, box, part: "body" });
      commentHit.push({ x, y, w, h: Math.min(h, 30 * view.zoom), box, part: "title" });
      commentHit.push({ x: x + w - grip - 4, y: y + h - grip - 4, w: grip + 8, h: grip + 8, box, part: "resize" });
    }
    for (const region of (graph.regions || []).filter((r) => r.kind === "comment")) {
      const b = commentRegionBounds(graph, region);
      const x = sx(b.x), y = sy(b.y), w = b.w * view.zoom, h = b.h * view.zoom;
      roundRect(x, y, w, h, 7);
      ctx.fillStyle = hexToRgba(region.color, region.alpha);
      ctx.fill();
      ctx.strokeStyle = region.color || "#2563eb";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([8, 5]);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = "#eaf3ff";
      ctx.font = `${Math.max(11, 14 * view.zoom)}px "Segoe UI", system-ui, sans-serif`;
      ctx.fillText(region.title || "Comment", x + 12 * view.zoom, y + 23 * view.zoom);
      renderedCommentRegions.push({ title: region.title || "Comment", x, y, w, h, source_backed: true });
    }
  }

  function createCommentBox(bounds, title = "Comment", color = COMMENT_TINTS[0], select = true) {
    const graph = currentGraphOrNull();
    if (!graph) return null;
    const box = {
      comment_id: newLocalId("comment"),
      graph_id: graph.graph_id,
      title,
      color,
      x: Math.round(bounds.x || 0),
      y: Math.round(bounds.y || 0),
      w: Math.max(160, Math.round(bounds.w || 300)),
      h: Math.max(96, Math.round(bounds.h || 160))
    };
    editorState.commentBoxes = (editorState.commentBoxes || []).concat([box]);
    saveEditorState();
    if (select) {
      selectedNodeIds = new Set([box.comment_id]);
      selectedNodeId = box.comment_id;
    }
    window.__jetCanvasCommentBoxes = graphCommentBoxes(graph).length;
    showToast("Comment added");
    if (latestDoc) drawGraph(latestDoc);
    return box;
  }

  function commentBoundsAroundSelection(graph) {
    const nodes = selectedGraphNodes(graphWithViewState(graph));
    if (!nodes.length) {
      const point = lastPointer || viewportCenterGraphPoint();
      return { x: point.x - 16, y: point.y - 16, w: 300, h: 160 };
    }
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const node of nodes) {
      const size = nodeSize(graphWithViewState(graph), node);
      minX = Math.min(minX, nodeX(node));
      minY = Math.min(minY, nodeY(node));
      maxX = Math.max(maxX, nodeX(node) + size.w);
      maxY = Math.max(maxY, nodeY(node) + size.h);
    }
    return { x: minX - 34, y: minY - 46, w: maxX - minX + 68, h: maxY - minY + 86 };
  }

  function addCommentAroundSelection() {
    const graph = currentGraphOrNull();
    if (!graph) return;
    const selected = selectedGraphNodes(graphWithViewState(graph)).filter((node) => node.source_span);
    if (!selected.length) {
      createCommentBox(commentBoundsAroundSelection(graph), "Comment", COMMENT_TINTS[0], true);
      return;
    }
    const bounds = commentBoundsAroundSelection(graph);
    const title = window.prompt("Comment title", "Comment");
    if (!title) return;
    postTransaction({
      schema_version: 1,
      op: "create_comment_region",
      revision: latestDoc.revision,
      graph_id: graph.graph_id,
      start: Math.min(...selected.map((node) => node.source_span.start)),
      end: Math.max(...selected.map((node) => node.source_span.end)),
      title,
      color: COMMENT_TINTS[0],
      alpha: "0.18",
      bounds: [bounds.x, bounds.y, bounds.w, bounds.h].map((value) => Math.round(value)).join(",")
    });
  }

  function hitCommentAt(x, y) {
    for (let i = commentHit.length - 1; i >= 0; i--) {
      const h = commentHit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function nodesInsideComment(graph, box) {
    return (graphWithViewState(graph).nodes || []).filter((node) => {
      const size = nodeSize(graphWithViewState(graph), node);
      const nx = nodeX(node);
      const ny = nodeY(node);
      return nx >= box.x && ny >= box.y && nx + size.w <= box.x + box.w && ny + size.h <= box.y + box.h;
    }).map((node) => node.node_id);
  }

  function setCommentTint(box, color) {
    box.color = color;
    saveEditorState();
    if (latestDoc) drawGraph(latestDoc);
  }

  function regionsForNode(graph, node) {
    if (!node) return [];
    return (graph.regions || []).filter((region) => region.kind === "comment" && spansOverlap(region.source_span, node.source_span));
  }

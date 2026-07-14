
// Canvas drawing primitives, typed pins, inline editors, and action palettes.
  function roundRect(x, y, w, h, r) {
    const rr = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rr, y);
    ctx.lineTo(x + w - rr, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + rr);
    ctx.lineTo(x + w, y + h - rr);
    ctx.quadraticCurveTo(x + w, y + h, x + w - rr, y + h);
    ctx.lineTo(x + rr, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - rr);
    ctx.lineTo(x, y + rr);
    ctx.quadraticCurveTo(x, y, x + rr, y);
  }

  function drawPin(pin, x, y, dir, recordHit = true) {
    const color = colorForType(pin.type || "unknown");
    const connected = connectedPinIds.has(pin.pin_id);
    const r = isExecPin(pin) ? (PIN_DIAMETER * .62) * view.zoom : (PIN_DIAMETER / 2) * view.zoom;
    if (connected) {
      ctx.save();
      ctx.beginPath();
      ctx.arc(x, y, Math.max(8, 10 * view.zoom), 0, Math.PI * 2);
      ctx.fillStyle = hexToRgba(color, .25);
      ctx.shadowColor = hexToRgba(color, .45);
      ctx.shadowBlur = 4 * view.zoom;
      ctx.fill();
      ctx.restore();
    }
    ctx.beginPath();
    if (isExecPin(pin)) {
      ctx.moveTo(x - r * .9, y - r);
      ctx.lineTo(x + r * 1.25, y);
      ctx.lineTo(x - r * .9, y + r);
      ctx.closePath();
    } else {
      ctx.arc(x, y, r, 0, Math.PI * 2);
    }
    ctx.fillStyle = connected ? color : "rgba(8,12,18,.98)";
    ctx.fill();
    if (hoverPin && hoverPin.pin_id === pin.pin_id) {
      ctx.beginPath();
      ctx.arc(x, y, 11 * view.zoom, 0, Math.PI * 2);
      ctx.strokeStyle = "#fef08a";
      ctx.lineWidth = Math.max(1.3, 1.6 * view.zoom);
      ctx.stroke();
    }
    ctx.lineWidth = Math.max(1, PIN_STROKE * view.zoom);
    ctx.strokeStyle = color;
    ctx.stroke();
    if (recordHit) {
      const hitR = Math.max(12, 18 * view.zoom);
      pinPoints.set(pin.pin_id, { x, y, color, pin });
      pinHit.push({ x: x - hitR, y: y - hitR, w: hitR * 2, h: hitR * 2, cx: x, cy: y, pin });
    }
  }

  function pinName(pin) {
    if (!pin) return "";
    return pin.name || "";
  }

  function exactPinType(pin) {
    if (!pin) return "";
    return isExecPin(pin) ? "exec" : (pin.type || "Value");
  }

  function clipText(text, max) {
    const s = String(text || "");
    return s.length > max ? s.slice(0, Math.max(1, max - 1)) + "…" : s;
  }

  function ellipsizeText(text, maxWidth) {
    const s = String(text || "");
    if (ctx.measureText(s).width <= maxWidth) return s;
    let lo = 0, hi = s.length;
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      if (ctx.measureText(s.slice(0, mid) + "…").width <= maxWidth) lo = mid;
      else hi = mid - 1;
    }
    return s.slice(0, Math.max(1, lo)) + "…";
  }

  function drawPinLabel(pin, x, y, align) {
    const label = visiblePinLabel(pin);
    if (!label) return;
    ctx.font = `${Math.max(9, 11 * view.zoom)}px ${UI_FONT}`;
    ctx.textAlign = align;
    ctx.fillStyle = isExecPin(pin) ? "rgba(242,244,248,.86)" : "rgba(234,242,255,.70)";
    ctx.fillText(clipText(label, 24), x, y);
    ctx.textAlign = "left";
  }

  function visiblePinLabel(pin) {
    if (!pin) return "";
    if (!isExecPin(pin)) return pinName(pin);
    const graph = currentGraphOrNull();
    const node = graph && (graph.nodes || []).find((n) => n.node_id === pin.node_id);
    if (!node) return "";
    return visiblePinLabelInGraph(graph, node, pin);
  }

  function visiblePinLabelInGraph(graph, node, pin) {
    if (!pin) return "";
    if (!isExecPin(pin)) return pinName(pin);
    if (pin.pattern_source) return pin.pattern_source;
    const count = pinsForNode(graph, node, pin.direction, true).length;
    return count > 1 ? pinName(pin) : "";
  }

  function drawPinHoverTooltip(pin) {
    const point = pin && pinPoints.get(pin.pin_id);
    if (!point) return;
    const text = pinName(pin) + " : " + exactPinType(pin) + " - " + typeExplanation(pin.type);
    ctx.font = `${Math.max(10, 12 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 9 * view.zoom;
    const badgeW = Math.min(220 * view.zoom, ctx.measureText(text).width + padX * 2);
    const badgeH = 25 * view.zoom;
    const bx = Math.max(8, Math.min(point.x + 14 * view.zoom, cssSize().width - badgeW - 8));
    const by = Math.max(8, point.y - badgeH - 13 * view.zoom);
    roundRect(bx, by, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.95)";
    ctx.fill();
    ctx.strokeStyle = point.color;
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = point.color;
    ctx.textAlign = "left";
    ctx.fillText(clipText(text, 28), bx + padX, by + 17 * view.zoom);
  }

  function typeExplanation(type) {
    if (type === "exec" || type === "control") return "control rail";
    if (type === "Bool") return "branch condition";
    if (type === "String") return "text value";
    if (type === "Int") return "whole number";
    if (String(type || "").includes("Task")) return "task handle";
    if (String(type || "").includes("Event") || String(type || "").includes("Hook")) return "event value";
    return "source-backed Jet value";
  }

  function drawSourceChip(text, color, x, y, maxW, recordHit, hitData) {
    const boxW = Math.max(38 * view.zoom, Math.min(maxW, (ctx.measureText(text || "arm").width + 16 * view.zoom)));
    const boxH = 18 * view.zoom;
    roundRect(x, y - boxH / 2, boxW, boxH, 4 * view.zoom);
    ctx.fillStyle = "rgba(7,12,19,.92)";
    ctx.fill();
    ctx.strokeStyle = hexToRgba(color, .78);
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = "#dbeafe";
    ctx.font = `${Math.max(8, 10 * view.zoom)}px ${MONO_FONT}`;
    ctx.textAlign = "left";
    ctx.fillText(ellipsizeText(text || "arm", boxW - 14 * view.zoom), x + 7 * view.zoom, y + 3.5 * view.zoom);
    if (recordHit && hitData) pinEditorHit.push(Object.assign({ x, y: y - boxH / 2, w: boxW, h: boxH }, hitData));
    return boxW;
  }

  function drawSocketRow(pin, x, y, w, dir, recordHit) {
    const execInset = isExecPin(pin) ? 5 * view.zoom : 0;
    const px = dir === "input" ? x + execInset : x + w - execInset;
    const hasLabel = !!visiblePinLabel(pin);
    const labelX = dir === "input" ? x + (hasLabel ? 22 : 8) * view.zoom : x + w - (hasLabel ? 22 : 8) * view.zoom;
    const labelAlign = dir === "input" ? "left" : "right";
    drawPin(pin, px, y, dir, recordHit);
    if (pin.pattern_source) {
      const color = colorForType(pin.type || "exec");
      const maxW = Math.max(42, w * .42);
      const chipW = Math.min(maxW, (measureTextPx(`10px ${MONO_FONT}`, pin.pattern_source) + 16)) * view.zoom;
      const chipX = dir === "input" ? x + 22 * view.zoom : x + w - 22 * view.zoom - chipW;
      drawSourceChip(pin.pattern_source, color, chipX, y, chipW, recordHit, { pin, kind: "pattern_arm" });
      return;
    }
    drawPinLabel(pin, labelX, y + 4 * view.zoom, labelAlign);
  }

  function editablePinKind(pin) {
    const t = String(pin && pin.type || "");
    if (t === "Bool") return "bool";
    if (["Int", "I64", "U64", "Float", "F32", "F64"].includes(t)) return "number";
    if (t === "String" || t === "Char") return "string";
    if (/Enum|Variant/.test(t)) return "enum";
    return "";
  }

  function inlineDefaultForPin(graph, pin) {
    if (!graph || !pin || pin.direction !== "input" || connectedPinIds.has(pin.pin_id)) return null;
    const expr = inlineForPin(graph, pin);
    if (!expr) return null;
    return expr;
  }

  function defaultEditorValue(expr) {
    return String((expr && expr.source) || "").trim();
  }

  function isLiteralDefault(kind, source) {
    const s = String(source || "").trim();
    if (kind === "bool") return s === "true" || s === "false";
    if (kind === "number") return /^-?\d+(\.\d+)?$/.test(s);
    if (kind === "string") return /^"([^"\\]|\\.)*"$/.test(s) || /^'([^'\\]|\\.)'$/.test(s);
    if (kind === "enum") return /^\.?[A-Za-z_][A-Za-z0-9_.]*$/.test(s);
    return false;
  }

  function drawInlineExprChip(pin, expr, x, y, recordHit, maxWorldW = 150) {
    const source = defaultEditorValue(expr);
    const color = colorForType(pin.type || "unknown");
    ctx.font = `${Math.max(8, 10 * view.zoom)}px ${MONO_FONT}`;
    const boxW = Math.min(maxWorldW, Math.max(58, ctx.measureText(source || "expr").width / view.zoom + 18)) * view.zoom;
    const boxH = 20 * view.zoom;
    const bx = x;
    const by = y - boxH / 2;
    roundRect(bx, by, boxW, boxH, 4 * view.zoom);
    ctx.fillStyle = "rgba(7,12,19,.92)";
    ctx.fill();
    ctx.strokeStyle = hexToRgba(color, .78);
    ctx.lineWidth = Math.max(1, 1.2 * view.zoom);
    ctx.stroke();
    ctx.fillStyle = "#dbeafe";
    ctx.textAlign = "left";
    ctx.fillText(ellipsizeText(source || "expr", boxW - 16 * view.zoom), bx + 8 * view.zoom, y + 3.5 * view.zoom);
    if (recordHit) pinEditorHit.push({ x: bx, y: by, w: boxW, h: boxH, pin, expr, kind: "inline_expr" });
    window.__jetCanvasInlineExprChips = true;
    return boxW + 8 * view.zoom;
  }

  function drawPinDefaultEditor(graph, pin, x, y, recordHit, maxWorldW = 96) {
    const kind = editablePinKind(pin);
    if (!kind || pin.direction !== "input" || connectedPinIds.has(pin.pin_id)) return 0;
    const expr = inlineDefaultForPin(graph, pin);
    const source = defaultEditorValue(expr);
    if (expr && !isLiteralDefault(kind, source)) return drawInlineExprChip(pin, expr, x, y, recordHit, Math.min(150, maxWorldW));
    const color = colorForType(pin.type || "unknown");
    const boxW = Math.min(96, maxWorldW, kind === "bool" ? 24 : kind === "enum" ? 88 : 76) * view.zoom;
    const boxH = 20 * view.zoom;
    const bx = x;
    const by = y - boxH / 2;
    roundRect(bx, by, boxW, boxH, 4 * view.zoom);
    ctx.fillStyle = "rgba(7,12,19,.92)";
    ctx.fill();
    ctx.strokeStyle = expr ? hexToRgba(color, .62) : "rgba(107,114,128,.52)";
    ctx.lineWidth = Math.max(.8, view.zoom);
    ctx.stroke();
    if (kind === "bool") {
      ctx.strokeStyle = expr ? color : "rgba(138,143,152,.75)";
      ctx.lineWidth = Math.max(1, 1.4 * view.zoom);
      ctx.strokeRect(bx + 6 * view.zoom, by + 5 * view.zoom, 10 * view.zoom, 10 * view.zoom);
      if (source === "true") {
        ctx.beginPath();
        ctx.moveTo(bx + 8 * view.zoom, by + 10 * view.zoom);
        ctx.lineTo(bx + 11 * view.zoom, by + 13 * view.zoom);
        ctx.lineTo(bx + 16 * view.zoom, by + 7 * view.zoom);
        ctx.stroke();
      }
    } else {
      ctx.font = `${Math.max(8, 10 * view.zoom)}px ${kind === "string" ? UI_FONT : MONO_FONT}`;
      ctx.fillStyle = expr ? "rgba(238,247,255,.82)" : "rgba(138,143,152,.72)";
      ctx.textAlign = "left";
      ctx.fillText(ellipsizeText(source || "default", boxW - 18 * view.zoom), bx + 7 * view.zoom, y + 3.5 * view.zoom);
      if (kind === "enum") {
        ctx.fillStyle = color;
        ctx.fillText("▾", bx + boxW - 14 * view.zoom, y + 3.5 * view.zoom);
      }
    }
    if (recordHit) pinEditorHit.push({ x: bx, y: by, w: boxW, h: boxH, pin, expr, kind });
    window.__jetCanvasPinDefaultEditors = true;
    return boxW + 8 * view.zoom;
  }

  function compatibleActionType(accepted, actual) {
    if (!accepted || !actual) return true;
    if (accepted === actual) return true;
    if (accepted === "Any" || accepted === "Value") return true;
    if (actual === "Any" || actual === "Value") return true;
    return numericType(accepted) && numericType(actual);
  }

  function actionReturnType(action) {
    if (action.ret) return action.ret;
    const signature = String(action.signature || "");
    const m = signature.match(/->\s*([A-Za-z0-9_\[\]?:.]+)/);
    return m ? m[1] : "";
  }

  function actionInsertsNode(entry) {
    const op = entry && (entry.op || entry.insert_op || "");
    if (entry && (entry.kind === "project_function" || entry.kind === "canvas.core_catalog")) return true;
    if (["insert_print", "insert_branch", "insert_switch", "insert_loop", "insert_fallible_rail"].includes(op)) return true;
    if (entry && (entry.kind === "variable_get" || entry.kind === "variable_set")) return true;
    return false;
  }

  function graphIsFallible(graph) {
    const returns = graph && graph.function && String(graph.function.returns || "");
    return returns.includes("?");
  }

  function actionAvailability(action, graph = currentGraphOrNull()) {
    if (action && action.available === false) {
      return { available: false, code: action.unavailable_reason_code || "unavailable", reason: action.denied_reason || "This action is unavailable here." };
    }
    if (action && (action.op || action.insert_op) === "insert_fallible_rail" && !graphIsFallible(graph)) {
      return { available: false, code: "needs_fallible_function", reason: "Needs a fallible function." };
    }
    return { available: true, code: "", reason: "" };
  }

  function functionsForPin(pin) {
    if (!pin) return actionEntries.filter(actionInsertsNode);
    const targetType = pin.type || null;
    let entries = actionEntries.filter(actionInsertsNode).filter((entry) => {
      if (!targetType) return true;
      if (isExecPin(pin)) return pin.direction === "output"
        ? actionAcceptsExec(entry)
        : actionProducesExec(entry);
      if (pin.direction === "output") {
        return (entry.pins || []).some((p) => p.direction === "input" && compatibleActionType(p.type, targetType));
      }
      return compatibleActionType(pin.type, actionReturnType(entry) || entry.ret || "Value");
    });
    if (entries.length === 0) entries = actionEntries.filter(actionInsertsNode);
    return entries;
  }

  function actionAcceptsExec(entry) {
    const op = entry.op || entry.insert_op || "";
    if (["insert_branch", "insert_switch", "insert_loop", "insert_fallible_rail"].includes(op)) return true;
    if (entry.pure) return false;
    return entry.kind === "canvas.action" || entry.kind === "canvas.core_catalog" || entry.kind === "project_function" || op === "insert_call" || op === "insert_print";
  }

  function actionProducesExec(entry) {
    return actionAcceptsExec(entry);
  }

  function closeContextMenu() {
    contextMenu.classList.remove("is-open");
    contextMenu.innerHTML = "";
    contextMenuState = null;
  }

  function actionMatchesQuery(action, query) {
    return actionFuzzyScore(action, query) > -Infinity;
  }

  function fuzzyScoreText(value, query) {
    const text = String(value || "").toLowerCase();
    const q = String(query || "").trim().toLowerCase();
    if (!q) return 0;
    if (!text) return -Infinity;
    if (text === q) return 3000;
    if (text.startsWith(q)) return 2200 - Math.min(400, text.length - q.length);
    const idx = text.indexOf(q);
    if (idx >= 0) return 1600 - idx;
    let pos = -1;
    let first = -1;
    let last = -1;
    let gaps = 0;
    for (const ch of q) {
      const next = text.indexOf(ch, pos + 1);
      if (next < 0) return -Infinity;
      if (first < 0) first = next;
      if (pos >= 0) gaps += Math.max(0, next - pos - 1);
      pos = next;
      last = next;
    }
    const span = Math.max(1, last - first + 1);
    const density = q.length / span;
    return 900 + density * 500 - gaps * 4 - first;
  }

  function actionFuzzyScore(action, query) {
    if (!query) return 0;
    return Math.max(...[action.title, action.detail, action.group, action.kind, action.signature, action.summary, action.module_path, action.callee, action.ret, action.type]
      .map((value) => fuzzyScoreText(value, query)));
  }
  window.__jetCanvasFuzzyScore = fuzzyScoreText;

  function paletteCategoryForAction(action) {
    const group = String(action.group || "").toLowerCase();
    if (group.includes("flow") || group.includes("execution") || ["insert_branch", "insert_switch", "insert_loop", "insert_fallible_rail"].includes(action.op)) return "Execution";
    if (action.kind === "variable_get" || action.kind === "variable_set" || group.includes("variable") || group.includes("binding") || action.op === "promote_to_binding") return "Variables";
    if (action.kind === "canvas.core_catalog" || group.includes("core")) return "Core";
    if (action.kind === "project_function" || group.includes("project") || action.kind === "canvas.action") return "Project";
    return "Execution";
  }

  function paletteActionGlyph(action) {
    if (action.kind === "variable_get" || action.kind === "variable_set") return action.type || action.ret || "•";
    if (action.kind === "canvas.core_catalog" || action.kind === "project_function" || action.kind === "canvas.action") return action.pure ? "ƒ" : "ƒ";
    if (action.op === "insert_branch") return "◇";
    if (action.op === "insert_loop") return "↻";
    if (action.op === "insert_return" || String(action.title || "").toLowerCase().includes("return")) return "⏎";
    if (action.op === "insert_switch") return "⇉";
    if (paletteCategoryForAction(action) === "Execution") return "◇";
    return "•";
  }

  function paletteActionColor(action) {
    if (action.kind === "variable_get" || action.kind === "variable_set") return colorForType(action.type || action.ret || "unknown");
    if (action.kind === "canvas.core_catalog" || action.kind === "project_function" || action.kind === "canvas.action") return action.pure ? NODE_ARCHETYPE_STYLES.function_pure.accent : NODE_ARCHETYPE_STYLES.function_exec.accent;
    if (action.op === "insert_branch" || action.op === "insert_switch" || action.op === "insert_loop") return "#f2f4f8";
    return colorForType(action.ret || action.type || "unknown");
  }

  function paletteTypeSummary(action) {
    const signature = action.signature || action.detail || "";
    if (signature) return signature;
    if ((action.pins || []).length) return (action.pins || []).filter((p) => p.direction === "input").map((p) => p.type || "Value").join(", ") + " -> " + (action.ret || "Void");
    return action.detail || "";
  }

  function renderActionPalette() {
    if (!contextMenuState) return;
    const query = contextMenuState.query || "";
    const matches = contextMenuState.actions
      .map((action) => Object.assign({ __score: actionFuzzyScore(action, query) }, action))
      .filter((action) => action.__score > -Infinity)
      .sort((a, b) => b.__score - a.__score || rankAction(b) - rankAction(a) || String(a.module_path || "").localeCompare(String(b.module_path || "")) || String(a.title).localeCompare(String(b.title)));
    const context = contextMenuState.pin ? `${contextMenuState.pin.name}: ${contextMenuState.pin.type}` : `All nodes · ${matches.length}/${contextMenuState.actions.length}`;
    const port = contextMenuState.pin ? pinPortHtml(contextMenuState.pin.type || "Value") : "";
    const favorites = favoriteSet();
    const rowForAction = (action) => {
      const id = action.action_id || action.callee || action.title;
      const fav = favorites.has(id);
      const color = paletteActionColor(action);
      const availability = actionAvailability(action);
      const disabled = !availability.available;
      const reason = disabled ? availability.reason : "";
      return `<button class="action-result${fav ? " is-favorite" : ""}${disabled ? " is-disabled" : ""}" data-menu-action="${escapeAttr(action.index)}" data-available="${disabled ? "false" : "true"}" data-unavailable-reason-code="${escapeAttr(availability.code)}" aria-disabled="${disabled ? "true" : "false"}" title="${escapeAttr(reason)}" style="--action-color:${escapeAttr(disabled ? "#6b7280" : color)}"><span class="action-glyph">${escapeHtml(paletteActionGlyph(action))}</span><span>${fav ? "★ " : ""}${escapeHtml(action.title)}<small style="color:${escapeAttr(disabled ? "#9ca3af" : color)}">${escapeHtml(disabled ? reason : paletteTypeSummary(action))}</small></span></button>`;
    };
    const categories = ["Execution", "Variables", "Project", "Core", "Commands"].map((category) => {
      const limit = category === "Core" ? 1000 : category === "Project" ? 500 : category === "Variables" ? 200 : 64;
      const rows = matches.filter((action) => paletteCategoryForAction(action) === category).slice(0, limit);
      if (!rows.length && query) return "";
      let body = "<div class=\"action-empty\">No actions</div>";
      if (rows.length && category === "Core") {
        const modules = [];
        for (const action of rows) {
          const module = action.module_path || "core";
          let bucket = modules.find((item) => item.module === module);
          if (!bucket) {
            bucket = { module, rows: [] };
            modules.push(bucket);
          }
          bucket.rows.push(action);
        }
        body = modules.map((bucket) => `<h4>${escapeHtml(bucket.module)}</h4>${bucket.rows.map(rowForAction).join("")}`).join("");
      } else if (rows.length) {
        body = rows.map(rowForAction).join("");
      }
      return `<section class="action-category"><h3>${escapeHtml(category)}</h3>${body}</section>`;
    }).join("");
    const countTag = contextMenuState.pin ? `<span class="tag">${matches.length}/${contextMenuState.actions.length}</span>` : "";
    contextMenu.innerHTML = `<div class="action-palette-head"><div class="menu-title">${escapeHtml(contextMenuState.title)}</div><div class="action-context">${port}<span>${escapeHtml(context)}</span>${countTag}</div><input id="action-palette-search" placeholder="Search actions" value="${escapeAttr(query)}"></div><div class="action-results">${categories || "<div class=\"action-empty\">No matching actions</div>"}</div>`;
    const input = document.getElementById("action-palette-search");
    if (input) {
      input.addEventListener("input", () => {
        contextMenuState.query = input.value || "";
        renderActionPalette();
        const next = document.getElementById("action-palette-search");
        if (next) {
          next.focus();
          next.setSelectionRange(next.value.length, next.value.length);
        }
      });
      input.addEventListener("keydown", (ev) => {
        if (ev.key === "Escape") {
          ev.preventDefault();
          closeContextMenu();
        } else if (ev.key === "Enter") {
          const first = contextMenu.querySelector("[data-menu-action]");
          if (first) {
            ev.preventDefault();
            first.click();
          }
        }
      });
    }
    contextMenu.querySelectorAll("[data-menu-action]").forEach((button) => {
      button.addEventListener("click", () => {
        const action = contextMenuState && contextMenuState.actions.find((item) => String(item.index) === button.getAttribute("data-menu-action"));
        const availability = actionAvailability(action);
        if (!availability.available) {
          showToast(availability.reason);
          return;
        }
        closeContextMenu();
        if (action) {
          markActionUsed(action);
          action.run();
        }
      });
    });
  }

  function openActionPalette(x, y, title, actions, opts = {}) {
    contextMenuState = {
      title,
      actions: (actions.length ? actions : [{ title: "No compatible actions", detail: "source-backed only", group: "empty", run: () => {} }]).map((action, index) => Object.assign({ index }, action)),
      pin: opts.pin || null,
      context: opts.context || "",
      query: opts.query || "",
      graphPoint: opts.graphPoint || null
    };
    renderActionPalette();
    contextMenu.style.left = Math.min(x, window.innerWidth - 430) + "px";
    contextMenu.style.top = Math.min(y, window.innerHeight - 430) + "px";
    contextMenu.classList.add("is-open");
    contextMenuOpenedAt = Date.now();
    const input = document.getElementById("action-palette-search");
    if (input) input.focus();
  }

  function openContextMenu(x, y, title, actions) {
    openActionPalette(x, y, title, actions, { context: "node actions" });
  }

  function openPinMenu(pin, x, y, graphPoint) {
    if (pin && pin.role === "arm") {
      openActionPalette(x, y, "Pattern arm", [
        { title: "Edit pattern", detail: pin.pattern_source || "pattern", group: "Patterns", run: () => editPatternArm(pin) },
        { title: "Remove arm", detail: "delete source body", group: "Patterns", run: () => removePatternArm(pin) }
      ], { pin, context: "Pattern arm" });
      return;
    }
    if (pin && pin.append_op === "remove_multi_input_element") {
      openActionPalette(x, y, "Input element", [
        { title: "Remove element", detail: pin.name || "item", group: "Pins", run: () => removeMultiInputElement(pin) }
      ], { pin, context: "Input element" });
      return;
    }
    const entries = functionsForPin(pin).concat(variableActionsForGraph(currentGraphOrNull()).filter(actionInsertsNode));
    const actions = entries.map((entry) => ({
      title: entry.title,
      detail: entry.detail,
      group: paletteCategoryForAction(entry),
      kind: entry.kind,
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
    actions.unshift({
      title: "Create function accepting " + (pin.type || "Value"),
      detail: pin.name || "value",
      group: "function",
      run: () => {
        const base = String(pin.name || "value").replace(/[^A-Za-z0-9_]/g, "_").replace(/^[^A-Za-z_]+/, "") || "value";
        const name = window.prompt("Function name", "use_" + base);
        if (name) postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params: "value: " + (pin.type || "Int"), ret_type: "Void" });
      }
    });
    if (pin.direction === "input") {
      actions.unshift({
        title: "Promote pin to binding",
        detail: pin.type || "value",
        group: "refactor",
        run: () => {
          const name = window.prompt("Binding name", pin.name || "value");
          const graph = latestDoc ? currentGraph(latestDoc) : null;
          const expr = graph && (graph.inline_exprs || []).find((e) => e.source_span && pin.source_span && spansOverlap(e.source_span, pin.source_span));
          if (name && expr) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, name });
          else showToast("Select an inline expression to promote");
        }
      });
    }
    openActionPalette(x, y, "Add connected node", actions.filter(actionInsertsNode), { pin, context: "Insert node", graphPoint });
  }

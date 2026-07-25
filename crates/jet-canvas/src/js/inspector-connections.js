
// Inspector editors, selection, type compatibility, and source-backed connection planning.
  function debugRows(items) {
    return (items || []).map((item) => `<div class="pin-row"><b>${escapeHtml(item.name || "frame")}</b><br><span class="tag">${escapeHtml(item.value || String(item))}</span></div>`).join("");
  }

  function signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride) {
    const retType = retOverride !== undefined ? retOverride : (document.getElementById("function-return-type") && document.getElementById("function-return-type").value.trim()) || "Void";
    const ret = retType && retType !== "Void" ? " -> " + retType : "";
    const rows = [...details.querySelectorAll("[data-fn-param]")];
    const params = rows.map((row) => {
      const name = (row.querySelector("[data-param-name]") || {}).value || "";
      const type = (row.querySelector("[data-param-type]") || {}).value || "Int";
      const fallback = (row.querySelector("[data-param-default]") || {}).value || "";
      const defaultExpr = fallback.trim() ? " = " + fallback.trim() : "";
      return name.trim() + ": " + type.trim() + defaultExpr;
    }).filter((p) => !p.startsWith(":"));
    const visibility = fnMeta && fnMeta.visibility === "public" ? "pub " : "";
    return visibility + "fn " + ((fnMeta && fnMeta.name) || nodeTitle || "function") + "(" + params.join(", ") + ")" + ret;
  }

  function applyFunctionPins(graph, fnMeta, nodeTitle, retOverride) {
    const signature = signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride);
    window.__jetCanvasLastSignature = signature;
    postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
    return signature;
  }

  function nextParamName(fnMeta) {
    const used = new Set((fnMeta && fnMeta.params || []).map((p) => p.name || ""));
    let i = 1;
    while (used.has("input" + i)) i += 1;
    return "input" + i;
  }

  function pinPortHtml(type) {
    const t = type || "Value";
    const cls = t === "exec" || t === "control" ? " is-exec" : String(t).endsWith("?") ? " is-fallible" : "";
    return `<span class="pin-port${cls}" style="color:${escapeAttr(colorForType(t))}"></span>`;
  }

  function typeChipHtml(type) {
    return `<span class="type-chip" style="color:${escapeAttr(colorForType(type))}">${escapeHtml(type || "Value")}</span>`;
  }

  function functionParamRow(p, i) {
    const pinType = p && p.type ? p.type : "Int";
    const pinName = p && p.name ? p.name : "value";
    const defaultSource = p && p.default_source ? p.default_source : "";
    const defaultLabel = defaultSource ? "default " + defaultSource : "required";
    return `<div class="pin-editor-row" data-fn-param="${escapeAttr(i)}"><div class="pin-editor-title">${pinPortHtml(pinType)}<b>${escapeHtml(pinName)}</b>${typeChipHtml(pinType)}</div><div class="lane-meta">${escapeHtml(defaultLabel)}</div><div class="pin-tools"><input data-param-name="${escapeAttr(i)}" aria-label="Input pin name" title="Input pin name" value="${escapeAttr(pinName)}"><input data-param-type="${escapeAttr(i)}" aria-label="Input pin type" title="Input pin type" value="${escapeAttr(pinType)}"><input data-param-default="${escapeAttr(i)}" aria-label="Default expression" title="Default expression" placeholder="default" value="${escapeAttr(defaultSource)}"><button data-param-remove="${escapeAttr(i)}" title="Remove input pin">-</button></div></div>`;
  }

  function functionReturnRow(retType) {
    const outType = retType || "Void";
    const color = colorForType(outType);
    return `<div class="pin-editor-row"><div class="pin-editor-title"><span id="function-return-port" class="pin-port" style="color:${escapeAttr(color)}"></span><b>return</b><span id="function-return-type-chip" class="type-chip" style="color:${escapeAttr(color)}">${escapeHtml(outType)}</span></div><div id="function-return-meta" class="lane-meta">${outType === "Void" ? "no output value" : "output pin"}</div><div class="pin-tools output-pin-tools"><input id="function-return-type" aria-label="Return type" title="Return type" value="${escapeAttr(outType)}"><button id="set-function-output">Set</button><button id="remove-function-output">Void</button></div></div>`;
  }

  function variableByName(graph, name) {
    return graphVariables(graph).find((v) => v.name === name) || null;
  }

  function selectVariable(name) {
    selectedVariableName = name;
    selectedNodeId = null;
    selectedNodeIds = new Set();
    if (latestDoc) drawGraph(latestDoc);
  }

  function localInitExpr(graph, variable) {
    if (!graph || !variable || !variable.nodeId) return null;
    return (graph.inline_exprs || []).find((expr) => expr.node_id === variable.nodeId && (expr.role === "init" || expr.role === "value")) || null;
  }

  function signatureWithVariable(graph, variable, next) {
    const fnMeta = graph && graph.function;
    if (!fnMeta) return "";
    const params = (fnMeta.params || []).map((param) => {
      const name = param.name === variable.name ? (next.name || param.name) : param.name;
      const type = param.name === variable.name ? (next.type || param.type || "Int") : (param.type || "Int");
      const fallback = param.name === variable.name ? (next.defaultSource || "") : (param.default_source || "");
      return name + ": " + type + (String(fallback).trim() ? " = " + String(fallback).trim() : "");
    }).join(", ");
    const originalSignature = String(fnMeta.signature || "");
    const hasEffectArrow = originalSignature.includes("--[");
    const effects = fnMeta.effect_via ? "via " + fnMeta.effect_via : (fnMeta.effects || []).join(", ");
    const arrow = hasEffectArrow ? " --[" + effects + "]->" : " ->";
    const ret = fnMeta.returns && fnMeta.returns !== "Void" ? arrow + " " + fnMeta.returns : (hasEffectArrow ? arrow : "");
    const visibility = fnMeta.visibility === "public" ? "pub " : fnMeta.visibility === "package" ? "pub(package) " : "";
    return visibility + "fn " + (fnMeta.name || graph.title || "function") + "(" + params + ")" + ret;
  }

  function renderVariableDetails(graph, name) {
    const variable = variableByName(graph, name);
    if (!variable) {
      selectedVariableName = null;
      updateDetails(graph, graph.nodes.find((n) => n.node_id === graph.entry_node), [], []);
      return;
    }
    const color = colorForType(variable.type);
    details.style.setProperty("--node-accent", color);
    const isParam = variable.source === "input";
    const initExpr = localInitExpr(graph, variable);
    const defaultEditable = isParam || !!initExpr;
    const typeEditable = isParam;
    const nameEditable = isParam || variable.source === "local";
    details.innerHTML = `
      <div class="details-hero">
        <div class="details-titleline"><span class="node-glyph">•</span><div class="details-title"><p class="title">${escapeHtml(variable.name)}</p><div class="kind">${escapeHtml(isParam ? "Input variable" : "Local variable")}</div></div></div>
        <div class="details-chips"><span class="details-chip" style="color:${escapeAttr(color)}">${escapeHtml(variable.type)}</span><span class="details-chip">${escapeHtml(isParam ? "Function input" : "Inside this function")}</span></div>
      </div>
      <h2>Variable</h2>
      <div class="signature-board">
        <div class="edit-grid">
          <label>Name<input id="variable-name-input" ${nameEditable ? "" : "readonly"} value="${escapeAttr(variable.name)}"></label>
          <label>Type<input id="variable-type-input" ${typeEditable ? "" : "readonly"} value="${escapeAttr(variable.type)}"></label>
          <label>Default value<input id="variable-default-input" ${defaultEditable ? "" : "readonly"} placeholder="${isParam ? "optional" : "not set"}" value="${escapeAttr(variable.defaultSource || "")}"></label>
        </div>
        <div class="signature-actions">${nameEditable || typeEditable || defaultEditable ? "<button id=\"apply-variable-details\" class=\"primary\">Apply</button>" : "<div class=\"pin-empty\">This variable is read-only here.</div>"}</div>
      </div>`;
    const apply = document.getElementById("apply-variable-details");
    if (!apply) return;
    apply.addEventListener("click", () => {
      const nextName = document.getElementById("variable-name-input").value.trim();
      const nextType = document.getElementById("variable-type-input").value.trim() || variable.type;
      const nextDefault = document.getElementById("variable-default-input").value.trim();
      if (isParam) {
        const signature = signatureWithVariable(graph, variable, { name: nextName, type: nextType, defaultSource: nextDefault });
        postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
        selectedVariableName = nextName || variable.name;
        return;
      }
      if (nextName && nextName !== variable.name) {
        postTransaction({ schema_version: 1, op: "rename_binding", revision: latestDoc.revision, from: variable.name, to: nextName });
        selectedVariableName = nextName;
        return;
      }
      if (initExpr && nextDefault) {
        postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: initExpr.inline_expr_id, new_expr: nextDefault });
      }
    });
  }

  function syncReturnEditorPreview(type) {
    const outType = type && type.trim() ? type.trim() : "Void";
    const color = colorForType(outType);
    const port = document.getElementById("function-return-port");
    const chip = document.getElementById("function-return-type-chip");
    const meta = document.getElementById("function-return-meta");
    if (port) port.style.color = color;
    if (chip) {
      chip.style.color = color;
      chip.textContent = outType;
    }
    if (meta) meta.textContent = outType === "Void" ? "no output value" : "output pin";
  }

  function pinCardHtml(p) {
    const type = p.type || "Value";
    const color = colorForType(type);
    const rail = pinRail(p);
    const railLabel = rail === "control" ? "Execution" : "Value";
    const direction = p.direction === "input" ? "Input" : "Output";
    const flags = [direction, railLabel, p.fallible ? "can fail" : "", p.effect_grant_need ? "needs effect" : ""].filter(Boolean).join(" / ");
    return `<div class="pin-card" style="--pin-color:${escapeAttr(color)}">${pinPortHtml(isExecPin(p) ? "exec" : type)}<div class="pin-card-title"><b>${escapeHtml(p.name)}</b><small>${escapeHtml(flags)}<span class="type-detail"> - ${escapeHtml(type)}</span></small></div><button data-pin-menu="${escapeAttr(p.pin_id)}">Actions</button></div>`;
  }

  function updateDetails(graph, node, pins, inline) {
    if (!node) {
      details.innerHTML = "<div class=\"details-empty\"><b>No node selected</b><span class=\"tag\">Select, marquee, or use the graph tabs.</span></div>";
      return;
    }
    const style = nodeStyle(node, graph);
    details.style.setProperty("--node-accent", style.accent);
    const span = node.source_span || { start: 0, end: 0 };
    const pinRows = pins.map(pinCardHtml).join("");
    const inlineRows = inline.map((expr) => `<div class="inline-row"><b>${escapeHtml(expr.role)}</b><code>${escapeHtml(expr.source)}</code><div class="edit-grid"><input data-inline-id="${escapeHtml(expr.inline_expr_id)}" value="${escapeAttr(expr.source)}"><button data-inline-apply="${escapeHtml(expr.inline_expr_id)}">Apply expression</button><button data-inline-promote="${escapeHtml(expr.inline_expr_id)}">Promote to binding</button><button data-inline-convert="${escapeHtml(expr.inline_expr_id)}">Insert conversion</button><button data-inline-preview-extract="${escapeHtml(expr.inline_expr_id)}">Preview extract</button><button data-inline-extract="${escapeHtml(expr.inline_expr_id)}">Extract function</button></div></div>`).join("");
    const rename = node.kind === "binding" ? `<div class="edit-grid"><label>Rename binding<input id="rename-to" value="${escapeAttr(node.title)}"></label><button id="preview-rename">Preview rename</button><button id="rename-binding" class="primary">Rename</button></div>` : "";
    const calleeGraph = (node.kind === "call" || node.kind === "method") ? graphForFunctionName(node.title) : null;
    const calleeOpen = calleeGraph ? `<button id="open-callee-graph">Open ${escapeHtml(calleeGraph.title)} graph</button>` : "";
    const fnMeta = node.node_id === graph.entry_node ? graph.function : null;
    const fnReturnType = fnMeta ? (typeof fnMeta.returns === "string" ? fnMeta.returns : (fnMeta.returns && fnMeta.returns.type) || "Void") : "Void";
    const fnParams = fnMeta ? (fnMeta.params || []).map((p, i) => functionParamRow(p, i)).join("") : "";
    const fnReturnPanel = fnMeta ? functionReturnRow(fnReturnType) : "";
    const fnEvents = fnMeta ? (graph.event_views || []).map((event) => `<div class="pin-row"><b>${escapeHtml(event.title || event.function)}</b><br><span class="tag">Function event</span></div>`).join("") : "";
    const effectRows = fnMeta ? (fnMeta.effects || []).map((effect) => `<div class="pin-row"><b>${escapeHtml(effect)}</b><br><span class="tag">Effect</span></div>`).join("") : "";
    const markerRows = fnMeta ? [fnMeta.pure ? "Pure" : "", fnMeta.unsafe ? "Unsafe" : ""].filter(Boolean).map((marker) => `<div class="pin-row"><b>${escapeHtml(marker)}</b><br><span class="tag">Marker</span></div>`).join("") : "";
    const fnPanel = fnMeta ? `<h2>Function</h2><div class="signature-board"><div class="signature-head"><div><span class="sig-eyebrow">Function</span><b>${escapeHtml(fnMeta.visibility || "private")} ${escapeHtml(fnMeta.name || node.title)}</b><code>${escapeHtml(fnMeta.signature || "")}</code></div><button id="create-function" title="Create sibling function">New</button></div><div class="pin-lane"><div class="lane-head"><b>Inputs</b><span class="lane-meta">${(fnMeta.params || []).length}</span><button id="add-function-pin">+ Input</button></div><div class="pin-list" id="function-pin-list">${fnParams || "<div class=\"pin-empty\">No inputs</div>"}</div></div><div class="pin-lane"><div class="lane-head"><b>Output</b><span class="lane-meta">return type</span><button id="add-function-output">+ Output</button></div><div class="pin-list">${fnReturnPanel}</div></div><div class="signature-source"><span class="sig-eyebrow">Source signature</span><code>${escapeHtml(fnMeta.signature || "")}</code><input id="function-signature" value="${escapeAttr(fnMeta.signature || "")}"><div class="rename-strip"><input id="function-rename-to" aria-label="Function name" title="Function name" value="${escapeAttr(fnMeta.name || node.title)}"><button id="rename-function">Rename</button></div></div><div class="signature-actions"><button id="edit-function-signature">Apply signature</button><button id="apply-function-pins" class="primary">Apply pins</button></div></div>${effectRows || markerRows ? `<h2>Effects and markers</h2><div class="pin-list">${effectRows}${markerRows}</div>` : ""}${fnEvents ? `<h2>Events</h2><div class="pin-list">${fnEvents}</div>` : ""}` : "";
    const bpLabel = nodeBreakpoint(node) ? "Remove breakpoint" : "Set breakpoint";
    const locals = debugRows(debugOverlay && debugOverlay.locals);
    const watches = debugRows(debugOverlay && debugOverlay.watches);
    const stack = (debugOverlay && debugOverlay.call_stack || []).map((frame) => `<div class="pin-row"><span class="tag">${escapeHtml(frame)}</span></div>`).join("");
    const regionRows = regionsForNode(graph, node).map((region) => {
      const b = region.bounds || { x: 0, y: 0, w: 360, h: 180 };
      const bounds = [b.x || 0, b.y || 0, b.w || 360, b.h || 180].join(",");
      return `<div class="inline-row"><b>${escapeHtml(region.title || "Comment")}</b><code>${escapeHtml(region.region_id)}</code><div class="edit-grid"><input data-region-title="${escapeHtml(region.region_id)}" value="${escapeAttr(region.title || "Comment")}"><input data-region-color="${escapeHtml(region.region_id)}" value="${escapeAttr(region.color || "#2563eb")}"><input data-region-alpha="${escapeHtml(region.region_id)}" value="${escapeAttr(region.alpha || "0.18")}"><input data-region-bounds="${escapeHtml(region.region_id)}" value="${escapeAttr(bounds)}"><button data-region-apply="${escapeHtml(region.region_id)}">Apply comment</button><button data-region-delete="${escapeHtml(region.region_id)}">Delete comment</button></div></div>`;
    }).join("");
    const affords = (node.edit_affordances || []).slice(0, 4).map((a) => `<span class="details-chip">${escapeHtml(a)}</span>`).join("");
    details.innerHTML = `
      <div class="details-hero">
        <div class="details-titleline"><span class="node-glyph">${escapeHtml(style.glyph)}</span><div class="details-title"><p class="title">${escapeHtml(node.title)}</p><div class="kind">${escapeHtml(nodeKindLabel(node, graph))}</div></div></div>
        <span>${escapeHtml(nodeDescription(node, graph))}</span>
        <div class="details-chips dev-only"><span class="details-chip">${escapeHtml(node.kind)}</span><span class="details-chip type-detail">${pins.length} pins</span>${affords}</div>
        <div class="quick-actions"><button id="source-jump">Jump source</button><button id="find-references">Find refs</button>${calleeOpen ? calleeOpen.replace("<button", "<button class=\"wide\"") : ""}</div>
        <dl class="dev-only">
          <dt>span</dt><dd>${span.start}..${span.end}</dd>
          <dt>node</dt><dd>${escapeHtml(node.node_id)}</dd>
        </dl>
      </div>
      ${rename}
      ${fnPanel}
      <div class="debug-detail">
        <h2>Debug</h2>
        <div class="edit-grid"><button id="debug-toggle-break">${bpLabel}</button><button id="debug-add-watch">Add watch</button></div>
        <div class="pin-list">${locals || watches || stack ? locals + watches + stack : "<div class=\"tag\">no live values</div>"}</div>
      </div>
      <div class="diagnostic-detail">
        <h2>Comments</h2><div class="inline-list">${regionRows || "<div class=\"tag\">none</div>"}</div>
      </div>
      <div class="type-detail">
        <h2>Pins</h2><div class="pin-list">${pinRows || "<div class=\"tag\">none</div>"}</div>
        <h2>Inline</h2><div class="inline-list">${inlineRows || "<div class=\"tag\">none</div>"}</div>
      </div>
    `;
    const renameButton = document.getElementById("rename-binding");
    if (renameButton) {
      renameButton.addEventListener("click", () => {
        const to = document.getElementById("rename-to").value.trim();
        postTransaction({ schema_version: 1, op: "rename_binding", revision: latestDoc.revision, from: node.title, to });
      });
    }
    const previewRename = document.getElementById("preview-rename");
    if (previewRename) {
      previewRename.addEventListener("click", () => {
        const to = document.getElementById("rename-to").value.trim();
        postQuery({ op: "preview_rename", symbol: node.title, to });
      });
    }
    const renameFunction = document.getElementById("rename-function");
    if (renameFunction && fnMeta) {
      renameFunction.addEventListener("click", () => {
        const to = document.getElementById("function-rename-to").value.trim();
        postTransaction({ schema_version: 1, op: "rename_function", revision: latestDoc.revision, from: fnMeta.name, to });
      });
    }
    const editFunctionSignature = document.getElementById("edit-function-signature");
    if (editFunctionSignature && fnMeta) {
      editFunctionSignature.addEventListener("click", () => {
        const signature = document.getElementById("function-signature").value.trim();
        postTransaction({ schema_version: 1, op: "edit_function_signature", revision: latestDoc.revision, graph_id: graph.graph_id, signature });
      });
    }
    const createFunction = document.getElementById("create-function");
    if (createFunction) {
      createFunction.addEventListener("click", () => {
        const name = window.prompt("Function name", "helper");
        if (!name) return;
        const params = window.prompt("Parameters", "value: Int") || "";
        const ret_type = window.prompt("Return type", "Int") || "Int";
        postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params, ret_type });
      });
    }
    const openCalleeGraph = document.getElementById("open-callee-graph");
    if (openCalleeGraph && calleeGraph) {
      openCalleeGraph.addEventListener("click", () => openFunctionGraph(calleeGraph.title));
    }
    const addFunctionPin = document.getElementById("add-function-pin");
    if (addFunctionPin && fnMeta) {
      addFunctionPin.addEventListener("click", () => {
        const list = document.getElementById("function-pin-list");
        const i = "new" + Date.now();
        const row = document.createElement("div");
        row.innerHTML = functionParamRow({ name: nextParamName(fnMeta), type: "Int", default_source: "" }, i);
        const editorRow = row.firstElementChild;
        if (editorRow) {
          const empty = list.querySelector(".pin-empty");
          if (empty) empty.remove();
          list.appendChild(editorRow);
          const remove = editorRow.querySelector("[data-param-remove]");
          if (remove) remove.addEventListener("click", () => {
            editorRow.remove();
            applyFunctionPins(graph, fnMeta, node.title);
          });
          applyFunctionPins(graph, fnMeta, node.title);
        }
      });
    }
    ["apply-function-pins", "set-function-output", "remove-function-output", "add-function-output"].forEach((id) => {
      const button = document.getElementById(id);
      if (button) button.addEventListener("click", handleFunctionPinButton);
    });
    details.querySelectorAll("[data-param-remove]").forEach((button) => {
      button.addEventListener("click", () => {
        const row = button.closest("[data-fn-param]");
        if (row) row.remove();
        if (fnMeta) applyFunctionPins(graph, fnMeta, node.title);
      });
    });
    details.querySelectorAll("[data-pin-menu]").forEach((button) => {
      button.addEventListener("click", (ev) => {
        const pin = pins.find((p) => p.pin_id === button.getAttribute("data-pin-menu"));
        if (pin) openPinMenu(pin, ev.clientX, ev.clientY);
      });
    });
    const sourceJump = document.getElementById("source-jump");
    if (sourceJump) {
      sourceJump.addEventListener("click", () => {
        setSourceHash(span);
        showToast("Source location selected");
      });
    }
    const findReferences = document.getElementById("find-references");
    if (findReferences) {
      findReferences.addEventListener("click", () => postQuery({ op: "references", symbol: node.title }));
    }
    details.querySelectorAll("[data-inline-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-apply");
        const input = details.querySelector(`[data-inline-id="${cssEscape(id)}"]`);
        postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: id, new_expr: input.value });
      });
    });
    details.querySelectorAll("[data-inline-promote]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-promote");
        const name = window.prompt("Binding name", "value");
        if (name && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
          showToast("Binding name must be a Jet identifier");
          return;
        }
        if (name) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: id, name });
      });
    });
    details.querySelectorAll("[data-inline-convert]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-convert");
        const callee = window.prompt("Conversion function", "Float.from");
        if (callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: id, callee });
      });
    });
    details.querySelectorAll("[data-inline-preview-extract], [data-inline-extract]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-inline-preview-extract") || button.getAttribute("data-inline-extract");
        const name = window.prompt("Helper function", "extracted");
        if (!name) return;
        const op = button.hasAttribute("data-inline-preview-extract") ? "preview_extract_inline_expr" : "extract_inline_expr";
        postTransaction({ schema_version: 1, op, revision: latestDoc.revision, inline_expr_id: id, function: name, ret_type: "Int" });
      });
    });
    const toggle = document.getElementById("debug-toggle-break");
    if (toggle) toggle.addEventListener("click", () => toggleBreakpoint(node));
    const watch = document.getElementById("debug-add-watch");
    if (watch) watch.addEventListener("click", () => {
      const name = window.prompt("Watch local", node.title);
      addWatch(name && name.trim());
    });
    details.querySelectorAll("[data-region-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-region-apply");
        postTransaction({
          schema_version: 1,
          op: "edit_comment_region",
          revision: latestDoc.revision,
          region_id: id,
          title: details.querySelector(`[data-region-title="${cssEscape(id)}"]`).value,
          color: details.querySelector(`[data-region-color="${cssEscape(id)}"]`).value,
          alpha: details.querySelector(`[data-region-alpha="${cssEscape(id)}"]`).value,
          bounds: details.querySelector(`[data-region-bounds="${cssEscape(id)}"]`).value
        });
      });
    });
    details.querySelectorAll("[data-region-delete]").forEach((button) => {
      button.addEventListener("click", () => {
        const id = button.getAttribute("data-region-delete");
        postTransaction({ schema_version: 1, op: "delete_comment_region", revision: latestDoc.revision, region_id: id });
      });
    });
  }

  function selectNode(node, mode) {
    selectedVariableName = null;
    if (mode === "toggle") {
      if (selectedNodeIds.has(node.node_id)) selectedNodeIds.delete(node.node_id);
      else selectedNodeIds.add(node.node_id);
    } else if (mode === "add") {
      selectedNodeIds.add(node.node_id);
    } else {
      selectedNodeIds = new Set([node.node_id]);
    }
    selectedNodeId = node.node_id;
    const s = node.source_span || { start: 0, end: 0 };
    setSourceHash(s);
    if (latestDoc) drawGraph(latestDoc);
  }

  function hitNodeAt(x, y) {
    for (let i = hit.length - 1; i >= 0; i--) {
      const h = hit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function hitDiagnosticAt(x, y) {
    for (let i = diagnosticHit.length - 1; i >= 0; i--) {
      const h = diagnosticHit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function hitPinAt(x, y) {
    let best = null;
    let bestDistance = Infinity;
    for (let i = pinHit.length - 1; i >= 0; i--) {
      const h = pinHit[i];
      if (x < h.x || x > h.x + h.w || y < h.y || y > h.y + h.h) continue;
      const dx = x - (h.cx || h.x + h.w / 2);
      const dy = y - (h.cy || h.y + h.h / 2);
      const distance = dx * dx + dy * dy;
      if (distance < bestDistance) {
        best = h.pin;
        bestDistance = distance;
      }
    }
    return best;
  }

  function hitPinDefaultEditorAt(x, y) {
    for (let i = pinEditorHit.length - 1; i >= 0; i--) {
      const h = pinEditorHit[i];
      if (x >= h.x && x <= h.x + h.w && y >= h.y && y <= h.y + h.h) return h;
    }
    return null;
  }

  function applyPinDefaultEditor(hit) {
    if (!hit || !hit.pin) return false;
    if (hit.kind === "pattern_arm") {
      editPatternArm(hit.pin);
      return true;
    }
    if (!hit.expr) {
      showToast("Default value is read-only");
      return true;
    }
    let next = defaultEditorValue(hit.expr);
    if (hit.kind === "inline_expr") {
      next = window.prompt("Expression", next);
    } else if (hit.kind === "bool") {
      next = next === "true" ? "false" : "true";
    } else if (hit.kind === "number") {
      next = window.prompt("Default " + hit.pin.name, next || "0");
    } else if (hit.kind === "string") {
      const raw = window.prompt("Default " + hit.pin.name, next.replace(/^"|"$/g, ""));
      next = raw === null ? null : JSON.stringify(raw);
    } else if (hit.kind === "enum") {
      next = window.prompt("Default " + hit.pin.name, next || "." + hit.pin.name);
    }
    if (next === null || next === undefined || next === "") return true;
    postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: hit.expr.inline_expr_id, new_expr: next });
    return true;
  }

  function applyNodeAffordance(hit) {
    if (!hit || !hit.node) return false;
    if (hit.kind === "add_pattern_arm") {
      addPatternArm(hit.node);
      return true;
    }
    if (hit.kind === "append_multi_input") {
      appendMultiInput(hit.node);
      return true;
    }
    return false;
  }

  function numericType(type) {
    return ["Int", "Float", "F32", "F64"].includes(type || "");
  }

  function compatiblePin(from, to) {
    if (!from || !to || from.pin_id === to.pin_id) return false;
    if (from.direction === to.direction) return false;
    const out = from.direction === "output" ? from : to;
    const input = from.direction === "input" ? from : to;
    if (out.type === input.type) return true;
    return numericType(out.type) && numericType(input.type);
  }

  function connectionPlan(graph, fromPin, toPin) {
    if (!fromPin) return { ok: false, label: "No pin", color: "#fb7185" };
    if (!toPin) return { ok: true, label: "Release for actions", color: "#7dd3fc" };
    if (fromPin.pin_id === toPin.pin_id) return { ok: false, label: "Same pin", color: "#fb7185" };
    if (fromPin.direction === toPin.direction) {
      const expected = fromPin.direction === "output" ? "Drop on an input pin" : "Drop on an output pin";
      return { ok: false, label: expected, color: "#fb7185" };
    }
    const out = fromPin.direction === "output" ? fromPin : toPin;
    const input = fromPin.direction === "input" ? fromPin : toPin;
    if (!compatiblePin(fromPin, toPin)) return { ok: false, label: "Type mismatch " + (out.type || "?") + " -> " + (input.type || "?"), color: "#fb7185" };
    if (isExecPin(out) && isExecPin(input)) return { ok: true, label: "Reorder steps", color: "#a7f3d0" };
    if (!exactPinMatch(fromPin, toPin)) return { ok: true, label: "Insert visible conversion", color: "#f59e0b" };
    const wire = wireIntoPin(graph, input);
    const replacement = sourceExprForOutputPin(out);
    if (wire && replacement) return { ok: true, label: "Rewire source", color: "#a7f3d0" };
    return { ok: true, label: "Compatible preview", color: "#fde68a" };
  }

  function drawCompatibleDropTargets(graph, fromPin) {
    const seen = new Set();
    for (const hit of pinHit) {
      const pin = hit.pin;
      if (!compatiblePin(fromPin, pin) || seen.has(pin.pin_id)) continue;
      seen.add(pin.pin_id);
      const point = pinPoints.get(pin.pin_id);
      if (!point) continue;
      ctx.beginPath();
      ctx.arc(point.x, point.y, Math.max(13, 18 * view.zoom), 0, Math.PI * 2);
      ctx.strokeStyle = exactPinMatch(fromPin, pin) ? "#a7f3d0" : "#f59e0b";
      ctx.lineWidth = Math.max(1.4, 2.2 * view.zoom);
      ctx.shadowColor = exactPinMatch(fromPin, pin) ? "rgba(167,243,208,.52)" : "rgba(245,158,11,.44)";
      ctx.shadowBlur = 10;
      ctx.stroke();
      ctx.shadowBlur = 0;
    }
  }

  function drawConnectionBadge(plan, x, y) {
    const text = plan.label || "";
    ctx.font = `${Math.max(10, 12 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 9 * view.zoom;
    const badgeW = Math.min(230 * view.zoom, ctx.measureText(text).width + padX * 2);
    const badgeH = 26 * view.zoom;
    const bx = Math.min(x + 14 * view.zoom, cssSize().width - badgeW - 8);
    const by = Math.max(8, y - badgeH - 12 * view.zoom);
    roundRect(bx, by, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.94)";
    ctx.fill();
    ctx.strokeStyle = plan.color || "#7dd3fc";
    ctx.lineWidth = Math.max(1, view.zoom);
    ctx.stroke();
    ctx.fillStyle = plan.color || "#dbeafe";
    ctx.fillText(clipText(text, 28), bx + padX, by + 17 * view.zoom);
  }

  function drawWireTypeBadge(wire, from, to) {
    const label = wire.wire_kind === "control" ? "EXEC" : (from.pin && from.pin.type) || wire.wire_kind || "Value";
    const color = wireColor(wire, from);
    const mx = (from.x + to.x) / 2;
    const my = (from.y + to.y) / 2;
    ctx.font = `${Math.max(8, 10 * view.zoom)}px ui-monospace, Consolas, monospace`;
    const padX = 7 * view.zoom;
    const badgeW = Math.min(112 * view.zoom, ctx.measureText(label).width + padX * 2);
    const badgeH = 19 * view.zoom;
    roundRect(mx - badgeW / 2, my - badgeH / 2, badgeW, badgeH, 5 * view.zoom);
    ctx.fillStyle = "rgba(7,17,31,.90)";
    ctx.fill();
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.max(.8, view.zoom);
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.textAlign = "center";
    ctx.fillText(clipText(label, 16), mx, my + 3.5 * view.zoom);
    ctx.textAlign = "left";
  }

  function exactPinMatch(from, to) {
    if (!from || !to) return false;
    const out = from.direction === "output" ? from : to;
    const input = from.direction === "input" ? from : to;
    return out.type === input.type;
  }

  function sourceExprForOutputPin(pin) {
    if (!pin || pin.direction !== "output") return null;
    const name = pin.name || "";
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !["value", "result", "ok", "target"].includes(name)) return name;
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph && (graph.nodes || []).find((n) => n.node_id === pin.node_id);
    if (node && /^[A-Za-z_][A-Za-z0-9_]*$/.test(node.title) && node.kind === "binding") return node.title;
    if (node && /^[A-Za-z_][A-Za-z0-9_]*$/.test(node.title) && node.kind === "variable_get") return node.title;
    return null;
  }

  function wireIntoPin(graph, pin) {
    if (!graph || !pin) return null;
    return (graph.wires || []).find((w) => w.to_pin === pin.pin_id && w.source_span);
  }

  function inlineForPin(graph, pin) {
    if (!graph || !pin || !pin.source_span) return null;
    return (graph.inline_exprs || []).find((e) => e.source_span && spansOverlap(e.source_span, pin.source_span));
  }

  function selectMarquee() {
    if (!drag || drag.mode !== "marquee") return;
    const x0 = Math.min(drag.x, drag.mx), x1 = Math.max(drag.x, drag.mx);
    const y0 = Math.min(drag.y, drag.my), y1 = Math.max(drag.y, drag.my);
    const next = drag.additive ? new Set(selectedNodeIds) : new Set();
    for (const h of hit) {
      if (h.x < x1 && h.x + h.w > x0 && h.y < y1 && h.y + h.h > y0) next.add(h.node.node_id);
    }
    selectedNodeIds = next;
    selectedNodeId = [...selectedNodeIds][0] || selectedNodeId;
  }

  function completeConnection(fromPin, target, graph) {
    if (materializeStagedConnection(fromPin, target, graph)) return true;
    if (drag && drag.rewire && target && drag.rewire.wire && drag.rewire.wire.wire_kind === "control" && isExecPin(target)) {
      return completeExecRewire(drag.rewire, target, graph);
    }
    const plan = connectionPlan(graph, fromPin, target);
    window.__jetCanvasLastConnectionPlan = plan;
    if (compatiblePin(fromPin, target)) {
      const out = fromPin.direction === "output" ? fromPin : target;
      const input = fromPin.direction === "input" ? fromPin : target;
      const wire = wireIntoPin(graph, input);
      const replacement = sourceExprForOutputPin(out);
      if (exactPinMatch(fromPin, target) && wire && replacement) {
        postTransaction({ schema_version: 1, op: "move_link", revision: latestDoc.revision, wire_id: wire.wire_id, replacement });
      } else if (exactPinMatch(fromPin, target) && replacement) {
        const expr = inlineForPin(graph, input);
        if (expr) postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: replacement });
        else showToast("Input has no source expression to replace");
      } else if (!exactPinMatch(fromPin, target)) {
        const expr = inlineForPin(graph, input);
        const callee = window.prompt("Visible conversion function", (input.type || "Value") + ".from");
        if (expr && callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, callee });
        else showToast("Conversion needs an inline source expression");
      } else {
        showToast(plan.label + ": no safe source anchor");
      }
      return true;
    }
    if (target) showToast("Wire refused: " + plan.label);
    return false;
  }

  function nodeForPin(graph, pin) {
    if (!graph || !pin) return null;
    return (graph.nodes || []).find((node) => node.node_id === pin.node_id) || null;
  }

  function completeExecRewire(rewire, target, graph) {
    if (!rewire || !rewire.wire || !target || !graph) return false;
    const targetNode = nodeForPin(graph, target);
    if (!targetNode || !targetNode.source_span) {
      showToast("Wire refused: target step has no source span");
      return true;
    }
    const wire = rewire.wire;
    let moved = targetNode.source_span;
    let anchor = null;
    if (target.direction === "input") {
      anchor = wire.from_source_span;
    } else {
      const oldTarget = (graph.pins || []).find((pin) => pin.pin_id === wire.to_pin);
      const oldNode = nodeForPin(graph, oldTarget);
      moved = oldNode && oldNode.source_span;
      anchor = targetNode.source_span;
    }
    if (!moved || !anchor) {
      showToast("Wire refused: exec wire has no source anchor");
      return true;
    }
    postTransaction({
      schema_version: 1,
      op: "reorder_statements",
      revision: latestDoc.revision,
      graph_id: graph.graph_id,
      moved_start: moved.start,
      moved_end: moved.end,
      anchor_start: anchor.start,
      anchor_end: anchor.end,
      position: "after",
      wire_id: wire.wire_id
    });
    return true;
  }

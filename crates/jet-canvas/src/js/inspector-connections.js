
// Inspector editors, selection, type compatibility, and source-backed connection planning.
  let detailsEditorState = null;
  let fieldDescriptorSequence = 0;

  function appendText(parent, tag, className, value) {
    const element = document.createElement(tag);
    if (className) element.className = className;
    if (value !== undefined && value !== null) element.textContent = String(value);
    parent.appendChild(element);
    return element;
  }

  function appendSafeDescriptorAttributes(element, attributes) {
    for (const [name, value] of Object.entries(attributes || {})) {
      const attribute = String(name).toLowerCase();
      if (!/^(?:data|aria)-[a-z0-9_.:-]+$/.test(attribute)) continue;
      element.setAttribute(attribute, String(value));
    }
  }

  function appendButton(parent, id, label, className, attributes) {
    const button = document.createElement("button");
    button.type = "button";
    if (id) button.id = id;
    if (className) button.className = className;
    button.textContent = label;
    appendSafeDescriptorAttributes(button, attributes);
    parent.appendChild(button);
    return button;
  }

  function clearDom(parent) {
    while (parent && parent.firstChild) parent.removeChild(parent.firstChild);
  }

  function appendDetailsSection(parent, title, className) {
    const section = document.createElement("div");
    if (className) section.className = className;
    appendText(section, "h2", "", title);
    parent.appendChild(section);
    return section;
  }

  function descriptorOperation(operation) {
    if (typeof operation === "function") return { id: "field-operation", label: "Apply", run: operation };
    if (!operation || typeof operation.run !== "function") return null;
    return Object.assign({ id: "field-operation", label: "Apply", mode: "edit" }, operation);
  }

  function validatedFieldDescriptors(descriptors) {
    return (Array.isArray(descriptors) ? descriptors : []).map((field, index) => {
      if (!field || typeof field.label !== "string" || !Object.prototype.hasOwnProperty.call(field, "value")) return null;
      const apply_op = descriptorOperation(field.apply_op);
      return Object.assign({}, field, {
        key: String(field.key || field.id || field.label || index),
        apply_op,
        editable: field.editable === true && !!apply_op && apply_op.mode !== "action"
      });
    }).filter(Boolean);
  }

  function detailEditorValue(control) {
    if (!control) return "";
    if (control.type === "checkbox") return control.checked ? "true" : "false";
    return String(control.value === undefined ? "" : control.value);
  }

  function detailEditorSource(field, control) {
    const raw = detailEditorValue(control);
    if (typeof field.toSource === "function") return field.toSource(raw);
    if (field.kind === "scalar" && field.type === "String") return JSON.stringify(raw);
    return raw.trim();
  }

  function setDetailEditorValue(field, control, value) {
    if (!control) return;
    if (control.type === "checkbox") control.checked = String(value).trim() === "true" || value === true;
    else control.value = value === undefined || value === null ? "" : String(value);
  }

  function publishDetailsEditorState(state, event) {
    if (!state) return;
    window.__jetCanvasDetailsState = {
      phase: state.phase,
      selection: state.selectionKey,
      revision: state.revision,
      dirty: state.controls.filter((record) => detailEditorSource(record.field, record.control) !== record.initial).map((record) => record.field.key),
      event: event || "render"
    };
  }

  function setDetailsEditorPhase(state, phase, event) {
    if (!state || state !== detailsEditorState) return;
    state.phase = phase;
    for (const group of state.groups) {
      for (const button of group.buttons) button.disabled = phase === "applying";
    }
    publishDetailsEditorState(state, event);
  }

  function detailsEditorIsCurrent(state) {
    return !!state && state === detailsEditorState && details
      && details.dataset.detailsSelectionKey === state.selectionKey;
  }

  function detailsEditorSelectionIsCurrent(selectionKey) {
    return !!details && details.dataset.detailsSelectionKey === String(selectionKey || "none");
  }

  function beginDetailsEditor(selectionKey) {
    const state = {
      token: ++fieldDescriptorSequence,
      selectionKey: String(selectionKey || "none"),
      revision: latestDoc && latestDoc.revision || null,
      phase: "clean",
      controls: [],
      groups: []
    };
    detailsEditorState = state;
    details.dataset.detailsSelectionKey = state.selectionKey;
    publishDetailsEditorState(state, "selection");
    return state;
  }

  function fieldGroupFor(state, operation) {
    let group = state.groups.find((candidate) => candidate.operation === operation);
    if (!group) {
      group = { operation, fields: [], controls: [], buttons: [] };
      state.groups.push(group);
    }
    return group;
  }

  function invokeFieldOperation(operation, values, fields, state) {
    const result = operation.run(values, fields, {
      revision: state && state.revision || latestDoc && latestDoc.revision,
      selection: state && state.selectionKey
    });
    return result && typeof result.op === "string" ? postTransaction(result) : result;
  }

  function applyFieldGroup(state, group, event) {
    if (!detailsEditorIsCurrent(state)) {
      setDetailsEditorPhase(state, "refused", "selection-change");
      return false;
    }
    if (state.phase === "applying") return false;
    const dirty = group.fields.some((field) => {
      const record = state.controls.find((candidate) => candidate.field === field);
      return record && detailEditorSource(field, record.control) !== record.initial;
    });
    const requiresApply = group.fields.some((field) => field.applyWhenClean === true);
    if (!dirty && !requiresApply) {
      setDetailsEditorPhase(state, "clean", event || "noop");
      return false;
    }
    if (latestDoc && state.revision && latestDoc.revision !== state.revision) {
      setDetailsEditorPhase(state, "refused", "stale-revision");
      showToast("Details edit is stale; current source was kept", { isError: true });
      return false;
    }
    const values = {};
    for (const field of group.fields) {
      const record = state.controls.find((candidate) => candidate.field === field);
      values[field.key] = record ? detailEditorSource(field, record.control) : field.value;
    }
    setDetailsEditorPhase(state, "applying", event || "apply");
    let result;
    try {
      result = invokeFieldOperation(group.operation, values, group.fields, state);
    } catch (error) {
      setDetailsEditorPhase(state, "refused", "validation-error");
      showToast(String(error), { isError: true });
      return false;
    }
    if (!result || typeof result.then !== "function") {
      if (result && result.ok === false) setDetailsEditorPhase(state, "refused", "validation-error");
      else setDetailsEditorPhase(state, "clean", event || "apply");
      return true;
    }
    Promise.resolve(result).then((outcome) => {
      if (!outcome || outcome.ok === false) {
        if (state === detailsEditorState) setDetailsEditorPhase(state, "refused", "transaction-refused");
        else if (detailsEditorSelectionIsCurrent(state.selectionKey)) setDetailsEditorPhase(detailsEditorState, "refused", "transaction-refused");
      } else if (state === detailsEditorState) {
        setDetailsEditorPhase(state, "clean", event || "apply");
      }
    }).catch((error) => {
      if (state === detailsEditorState) setDetailsEditorPhase(state, "refused", "transaction-error");
      else if (detailsEditorSelectionIsCurrent(state.selectionKey)) setDetailsEditorPhase(detailsEditorState, "refused", "transaction-error");
      showToast(String(error), { isError: true });
    });
    return true;
  }

  function appendFieldControl(row, field, state, group) {
    const source = field.source !== undefined ? field.source : field.value;
    let control;
    if (field.editable) {
      if (field.kind === "enum" || field.kind === "reference") {
        control = document.createElement("select");
        for (const option of field.options || []) {
          const optionElement = document.createElement("option");
          optionElement.value = String(option.source || "");
          optionElement.textContent = String(option.name || option.source || "");
          optionElement.disabled = !!option.disabled;
          optionElement.selected = optionElement.value === String(source || "");
          control.appendChild(optionElement);
        }
      } else if (field.kind === "scalar" && field.type === "Bool") {
        control = document.createElement("input");
        control.type = "checkbox";
        control.checked = String(source || "").trim() === "true";
      } else {
        control = document.createElement(field.multiline ? "textarea" : "input");
        if (control.tagName === "INPUT") {
          control.type = field.kind === "scalar" && ["Int", "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "Float", "F32", "F64", "Decimal"].includes(field.type) ? "number" : "text";
        }
        if (field.placeholder) control.placeholder = field.placeholder;
        setDetailEditorValue(field, control, field.inputValue !== undefined ? field.inputValue : field.value);
      }
      if (field.id) control.id = field.id;
      control.setAttribute("data-details-input", field.key);
      control.setAttribute("data-detail-kind", field.kind || "expression");
      control.setAttribute("data-detail-type", field.type || "Value");
      if (field.inlineId) control.setAttribute("data-inline-id", field.inlineId);
      if (field.sourceSpan && Number.isFinite(field.sourceSpan.start) && Number.isFinite(field.sourceSpan.end)) {
        control.setAttribute("data-detail-source-start", field.sourceSpan.start);
        control.setAttribute("data-detail-source-end", field.sourceSpan.end);
      }
      appendSafeDescriptorAttributes(control, field.dataAttributes);
      row.appendChild(control);
      const record = { field, control, initial: detailEditorSource(field, control) };
      state.controls.push(record);
      group.fields.push(field);
      group.controls.push(control);
      const changed = (event) => {
        setDetailsEditorPhase(state, "dirty", event.type);
      };
      control.addEventListener("input", changed);
      control.addEventListener("change", changed);
      control.addEventListener("focus", () => publishDetailsEditorState(state, "focus"));
      control.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          setDetailEditorValue(field, control, field.inputValue !== undefined ? field.inputValue : field.value);
          const stillDirty = state.controls.some((record) => detailEditorSource(record.field, record.control) !== record.initial);
          setDetailsEditorPhase(state, stillDirty ? "dirty" : "clean", "escape");
        } else if (event.key === "Enter" && !(field.multiline && event.shiftKey)) {
          event.preventDefault();
          applyFieldGroup(state, group, "enter");
        }
      });
      control.addEventListener("blur", () => {
        window.setTimeout(() => {
          if (state !== detailsEditorState || group.controls.includes(document.activeElement)) return;
          applyFieldGroup(state, group, "blur");
        }, 0);
      });
    } else {
      control = appendText(row, field.multiline ? "pre" : "span", "details-value", field.displayValue !== undefined ? field.displayValue : field.value);
      control.setAttribute("data-details-value", field.key);
    }
    return control;
  }

  function appendFieldDescriptorRow(parent, field, state, group) {
    const layout = field.layout || "field";
    if (layout === "card") {
      const row = field.apply_op && field.apply_op.mode === "action" ? document.createElement("button") : document.createElement("div");
      row.className = field.className || "project-card";
      if (row.tagName === "BUTTON") row.type = "button";
      appendSafeDescriptorAttributes(row, field.buttonAttributes);
      appendText(row, "b", "", field.label);
      appendText(row, "small", "", field.value);
      if (field.detail !== undefined) appendText(row, "code", "", field.detail);
      parent.appendChild(row);
      if (field.apply_op && field.apply_op.mode === "action") {
        row.addEventListener("click", () => invokeFieldOperation(field.apply_op, {}, [field], state || { revision: latestDoc && latestDoc.revision, selectionKey: "project" }));
      }
      return row;
    }
    if (layout === "diagnostic") {
      const row = document.createElement("div");
      row.className = field.className || "problem-row";
      if (field.severity) row.setAttribute("data-severity", field.severity);
      appendSafeDescriptorAttributes(row, field.rowAttributes);
      appendText(row, "b", "", field.label);
      const value = field.apply_op && field.apply_op.mode === "action" ? appendButton(row, "", field.value, "", field.valueAttributes) : appendText(row, field.multiline ? "pre" : "span", "problem-detail", field.value);
      if (field.apply_op && field.apply_op.mode === "action") value.addEventListener("click", () => invokeFieldOperation(field.apply_op, {}, [field], state || { revision: latestDoc && latestDoc.revision, selectionKey: "diagnostic" }));
      for (const action of field.actions || []) {
        const button = appendButton(row, "", action.label, "", action.attributes);
        button.addEventListener("click", () => action.run && action.run());
      }
      if (field.detail !== undefined) appendText(row, "small", "", field.detail);
      if (field.fullText !== undefined) appendText(row, "pre", "problem-detail", field.fullText);
      parent.appendChild(row);
      return row;
    }
    const row = document.createElement("label");
    row.className = field.className || "details-field";
    row.setAttribute("data-details-field", field.key);
    appendText(row, "span", "", field.label);
    appendFieldControl(row, field, state, group);
    if (field.help) appendText(row, "small", "", field.help);
    parent.appendChild(row);
    return row;
  }

  function renderFieldDescriptors(container, descriptors, options = {}) {
    if (!container) return [];
    const fields = validatedFieldDescriptors(descriptors);
    const state = options.state || null;
    const list = document.createElement("div");
    if (options.fieldsClass !== undefined) list.className = options.fieldsClass;
    else list.className = "field-descriptor-list";
    container.appendChild(list);
    const groups = new Map();
    for (const field of fields) {
      const operation = field.editable ? field.apply_op : null;
      const group = operation && operation.mode !== "action" && state ? fieldGroupFor(state, operation) : null;
      if (group) groups.set(operation, group);
      appendFieldDescriptorRow(list, field, state, group);
      if (field.apply_op && field.apply_op.mode === "action" && !field.layout) {
        const button = appendButton(list, "", field.apply_op.label || "Open", "", field.apply_op.buttonAttributes);
        button.addEventListener("click", () => invokeFieldOperation(field.apply_op, {}, [field], state || { revision: latestDoc && latestDoc.revision, selectionKey: "field" }));
      }
    }
    if (state && groups.size) {
      const actions = document.createElement("div");
      actions.className = options.actionsClass || "signature-actions";
      for (const group of groups.values()) {
        const operation = group.operation;
        if (group.buttons.length) continue;
        const button = appendButton(actions, operation.buttonId, operation.label || "Apply", operation.primary === false ? "" : "primary", operation.buttonAttributes);
        button.setAttribute("data-field-apply", operation.id || "field-operation");
        button.addEventListener("click", () => applyFieldGroup(state, group, "button"));
        group.buttons.push(button);
        group.controls.push(button);
      }
      container.appendChild(actions);
    }
    return fields;
  }

  function debugRows(items) {
    const fragment = document.createDocumentFragment();
    for (const item of items || []) {
      const row = document.createElement("div");
      row.className = "pin-row";
      appendText(row, "b", "", item && item.name || "frame");
      appendText(row, "span", "tag", item && item.value || String(item));
      fragment.appendChild(row);
    }
    return fragment;
  }

  function signatureFromVisibleFunctionPins(fnMeta, nodeTitle, retOverride) {
    const retType = retOverride !== undefined ? retOverride : (document.getElementById("function-return-type") && document.getElementById("function-return-type").value.trim()) || "Void";
    const ret = retType && retType !== "Void" ? " -> " + retType : "";
    const rows = [...details.querySelectorAll("[data-fn-param]")];
    const params = rows.map((row) => {
      const name = (row.querySelector("[data-param-name]") || {}).value || "";
      const type = (row.querySelector("[data-param-type]") || {}).value || "Int";
      const fallback = (row.querySelector("[data-param-default]") || {}).value || "";
      const defaultExpr = fallback.trim() ? "{" + fallback.trim() + "}" : "";
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
    const port = document.createElement("span");
    port.className = "pin-port" + (t === "exec" || t === "control" ? " is-exec" : String(t).endsWith("?") ? " is-fallible" : "");
    port.style.color = colorForType(t);
    return port;
  }

  function typeChipHtml(type) {
    const chip = document.createElement("span");
    chip.className = "type-chip";
    chip.style.color = colorForType(type);
    chip.textContent = type || "Value";
    return chip;
  }

  function functionParamRow(p, i) {
    const pinType = p && p.type ? p.type : "Int";
    const pinName = p && p.name ? p.name : "value";
    const defaultSource = p && p.default_source ? p.default_source : "";
    const defaultLabel = defaultSource ? "default " + defaultSource : "required";
    const row = document.createElement("div");
    row.className = "pin-editor-row";
    row.setAttribute("data-fn-param", String(i));
    const title = document.createElement("div");
    title.className = "pin-editor-title";
    title.append(pinPortHtml(pinType), appendText(title, "b", "", pinName), typeChipHtml(pinType));
    row.appendChild(title);
    appendText(row, "div", "lane-meta", defaultLabel);
    const tools = document.createElement("div");
    tools.className = "pin-tools";
    const name = document.createElement("input");
    name.setAttribute("data-param-name", String(i));
    name.setAttribute("aria-label", "Input pin name");
    name.title = "Input pin name";
    name.value = pinName;
    const type = document.createElement("input");
    type.setAttribute("data-param-type", String(i));
    type.setAttribute("aria-label", "Input pin type");
    type.title = "Input pin type";
    type.value = pinType;
    const fallback = document.createElement("input");
    fallback.setAttribute("data-param-default", String(i));
    fallback.setAttribute("aria-label", "Default expression");
    fallback.title = "Default expression";
    fallback.placeholder = "default";
    fallback.value = defaultSource;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.setAttribute("data-param-remove", String(i));
    remove.title = "Remove input pin";
    remove.textContent = "-";
    tools.append(name, type, fallback, remove);
    row.appendChild(tools);
    return row;
  }

  function functionReturnRow(retType) {
    const outType = retType || "Void";
    const row = document.createElement("div");
    row.className = "pin-editor-row";
    const title = document.createElement("div");
    title.className = "pin-editor-title";
    const port = pinPortHtml(outType);
    port.id = "function-return-port";
    const chip = typeChipHtml(outType);
    chip.id = "function-return-type-chip";
    title.append(port, appendText(title, "b", "", "return"), chip);
    row.appendChild(title);
    appendText(row, "div", "lane-meta", outType === "Void" ? "no output value" : "output pin").id = "function-return-meta";
    const tools = document.createElement("div");
    tools.className = "pin-tools output-pin-tools";
    const input = document.createElement("input");
    input.id = "function-return-type";
    input.setAttribute("aria-label", "Return type");
    input.title = "Return type";
    input.value = outType;
    tools.append(input);
    appendButton(tools, "set-function-output", "Set");
    appendButton(tools, "remove-function-output", "Void");
    row.appendChild(tools);
    return row;
  }

  function variableByName(graph, name) {
    return graphVariables(graph).find((v) => v.name === name) || null;
  }

  function selectVariable(name) {
    selectedVariableName = name;
    selectedNodeId = null;
    selectedNodeIds = new Set();
    selectionExplicitlyCleared = true;
    if (latestDoc) drawGraph(latestDoc);
  }

  function localInitExpr(graph, variable) {
    if (!graph || !variable || !variable.nodeId) return null;
    const inline = (graph.inline_exprs || []).find((expr) => expr.node_id === variable.nodeId && (expr.role === "init" || expr.role === "value"));
    if (inline) return inline;
    const valuePin = (graph.pins || []).find((pin) => pin.node_id === variable.nodeId && pin.direction === "input" && !isExecPin(pin));
    const valueWire = valuePin && (graph.wires || []).find((wire) => wire.to_pin === valuePin.pin_id && wire.wire_kind === "data");
    const sourcePin = valueWire && (graph.pins || []).find((pin) => pin.pin_id === valueWire.from_pin);
    return sourcePin
      ? (graph.inline_exprs || []).find((expr) => expr.node_id === sourcePin.node_id && (expr.role === "init" || expr.role === "value")) || null
      : null;
  }

  function signatureWithVariable(graph, variable, next) {
    const fnMeta = graph && graph.function;
    if (!fnMeta) return "";
    const params = (fnMeta.params || []).map((param) => {
      const name = param.name === variable.name ? (next.name || param.name) : param.name;
      const type = param.name === variable.name ? (next.type || param.type || "Int") : (param.type || "Int");
      const fallback = param.name === variable.name ? (next.defaultSource || "") : (param.default_source || "");
      return name + ": " + type + (String(fallback).trim() ? "{" + String(fallback).trim() + "}" : "");
    }).join(", ");
    const originalSignature = String(fnMeta.signature || "");
    const hasEffectArrow = originalSignature.includes("-[");
    const effects = fnMeta.effect_via ? "via " + fnMeta.effect_via : (fnMeta.effects || []).join(", ");
    const arrow = hasEffectArrow ? " -[" + effects + "]>" : " ->";
    const ret = fnMeta.returns && fnMeta.returns !== "Void" ? arrow + " " + fnMeta.returns : (hasEffectArrow ? arrow : "");
    const visibility = fnMeta.visibility === "public" ? "pub " : fnMeta.visibility === "package" ? "pub(package) " : "";
    return visibility + "fn " + (fnMeta.name || graph.title || "function") + "(" + params + ")" + ret;
  }

  function detailFieldValue(field) {
    const source = String(field.source || "");
    if (field.kind === "scalar" && field.type === "String" && source.length >= 2 && source.startsWith("\"") && source.endsWith("\"")) {
      try {
        return JSON.parse(source);
      } catch (_) {
        return source.slice(1, -1);
      }
    }
    return source;
  }

  function detailExpressionFromElement(element) {
    if (!element) return "";
    if (element.type === "checkbox") return element.checked ? "true" : "false";
    const value = String(element.value || "");
    if (element.dataset.detailKind === "scalar" && element.dataset.detailType === "String") return JSON.stringify(value);
    return value.trim();
  }

  function enumOptions(type, source) {
    const current = String(source || "").trim();
    const options = enumVariantsForType(type).map((variant) => ({ name: variant.name, source: variant.source }));
    if (!current) return [{ name: "Select a value…", source: "", disabled: true }, ...options];
    if (!options.some((option) => option.source === current)) options.unshift({ name: current, source: current });
    return options;
  }

  function referenceOptions(graph, variable, source) {
    const current = String(source || "").trim();
    const options = graphVariables(graph)
      .filter((candidate) => candidate.name !== variable.name && candidate.type === variable.type)
      .map((candidate) => ({ name: candidate.name, source: candidate.name }));
    if (current && !options.some((option) => option.source === current)) options.unshift({ name: current, source: current });
    return options;
  }

  function valueDetailField(graph, variable, expr, inlineId, applyOp) {
    const source = expr ? expr.source : variable.defaultSource || "";
    let kind = detailEditorKind(variable.type, source, graph);
    if (!source.trim() && kind === "scalar" && variable.type === "Bool") kind = "expression";
    return {
      key: inlineId ? "inline-value" : "value",
      id: inlineId ? "" : "variable-default-input",
      inlineId,
      label: inlineId ? "Expression" : "Default value",
      type: variable.type,
      kind,
      source,
      sourceSpan: expr && expr.source_span || null,
      value: detailFieldValue({ type: variable.type, kind, source }),
      inputValue: detailFieldValue({ type: variable.type, kind, source }),
      options: kind === "enum" ? enumOptions(variable.type, source) : kind === "reference" ? referenceOptions(graph, variable, source) : [],
      applyWhenClean: kind === "enum" && !source.trim(),
      editable: Boolean(inlineId || variable.source === "input" || expr) && !!applyOp,
      apply_op: applyOp || null,
      placeholder: variable.source === "input" && !source ? "optional" : ""
    };
  }

  function metadataDetailsHtml(meta) {
    if (!meta) return null;
    const row = document.createElement("div");
    row.className = "details-meta";
    row.setAttribute("data-details-meta", "true");
    appendText(row, "span", "details-chip", meta.category || "Canvas metadata");
    appendText(row, "span", "tag", meta.tunable ? "tunable" : "not tunable");
    return row;
  }

  function variableDetailDescriptors(graph, variable, initExpr) {
    const isParam = variable.source === "input";
    const signatureOp = {
      id: "variable-signature",
      label: "Apply",
      buttonId: "apply-variable-details",
      primary: true,
      run: (values, fields, context) => {
        if (isParam && values.name) selectedVariableName = values.name;
        return {
          schema_version: 1,
          op: "edit_function_signature",
          revision: context.revision,
          graph_id: graph.graph_id,
          signature: signatureWithVariable(graph, variable, {
            name: values.name,
            type: values.type,
            defaultSource: values.value
          })
        };
      }
    };
    const localOp = !isParam ? {
      id: "variable-local",
      label: "Apply",
      buttonId: "apply-variable-details",
      primary: true,
      run: (values, fields, context) => {
        if (values.name && values.name !== variable.name) {
          selectedVariableName = values.name;
          return { schema_version: 1, op: "rename_binding", revision: context.revision, from: variable.name, to: values.name };
        }
        return initExpr
          ? { schema_version: 1, op: "edit_inline_expr", revision: context.revision, inline_expr_id: initExpr.inline_expr_id, new_expr: values.value }
          : null;
      }
    } : null;
    return [
      { key: "name", id: "variable-name-input", label: "Name", kind: "expression", type: "Identifier", source: variable.name, value: variable.name, editable: isParam || variable.source === "local", apply_op: isParam ? signatureOp : variable.source === "local" ? localOp : null },
      { key: "type", id: "variable-type-input", label: "Type", kind: "expression", type: "Type", source: variable.type, value: variable.type, editable: isParam, apply_op: isParam ? signatureOp : null },
      valueDetailField(graph, variable, initExpr, null, isParam ? signatureOp : localOp)
    ];
  }

  function functionDetailDescriptors(graph, node, fnMeta) {
    const signatureOp = {
      id: "function-signature",
      label: "Apply signature",
      buttonId: "edit-function-signature",
      primary: true,
      run: (values, fields, context) => ({
        schema_version: 1,
        op: "edit_function_signature",
        revision: context.revision,
        graph_id: graph.graph_id,
        signature: String(values.signature || "").trim()
      })
    };
    const renameOp = {
      id: "function-rename",
      label: "Rename",
      buttonId: "rename-function",
      run: (values, fields, context) => ({
        schema_version: 1,
        op: "rename_function",
        revision: context.revision,
        from: fnMeta.name,
        to: String(values.name || "").trim()
      })
    };
    return [
      { key: "signature", id: "function-signature", label: "Signature", kind: "expression", type: "Signature", value: fnMeta.signature || "", source: fnMeta.signature || "", editable: true, apply_op: signatureOp, multiline: true },
      { key: "name", id: "function-rename-to", label: "Name", kind: "expression", type: "Identifier", value: fnMeta.name || node.title, source: fnMeta.name || node.title, editable: true, apply_op: renameOp }
    ];
  }

  function nodeDetailDescriptors(node, graph, pins) {
    const span = node.source_span || { start: 0, end: 0 };
    return [
      { key: "title", label: "Title", value: node.title || "", editable: false },
      { key: "kind", label: "Kind", value: node.kind || "Node", editable: false },
      { key: "description", label: "Description", value: nodeDescription(node, graph), editable: false, multiline: true },
      { key: "pins", label: "Pins", value: String(pins.length), editable: false },
      { key: "span", label: "Source span", value: String(span.start) + ".." + String(span.end), editable: false }
    ];
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
    const state = beginDetailsEditor("variable:" + variable.name);
    clearDom(details);
    const hero = document.createElement("div");
    hero.className = "details-hero";
    const titleLine = document.createElement("div");
    titleLine.className = "details-titleline";
    appendText(titleLine, "span", "node-glyph", "•");
    const title = document.createElement("div");
    title.className = "details-title";
    appendText(title, "p", "title", variable.name);
    appendText(title, "div", "kind", variable.source === "input" ? "Input variable" : "Local variable");
    titleLine.appendChild(title);
    hero.appendChild(titleLine);
    const chips = document.createElement("div");
    chips.className = "details-chips";
    const typeChip = appendText(chips, "span", "details-chip", variable.type);
    typeChip.style.color = color;
    appendText(chips, "span", "details-chip", variable.source === "input" ? "Function input" : "Inside this function");
    hero.appendChild(chips);
    const meta = metadataDetailsHtml(variable.meta);
    if (meta) hero.appendChild(meta);
    details.appendChild(hero);
    appendDetailsSection(details, "Variable");
    const board = document.createElement("div");
    board.className = "signature-board";
    renderFieldDescriptors(board, variableDetailDescriptors(graph, variable, localInitExpr(graph, variable)), { state, fieldsClass: "edit-grid details-fields" });
    details.appendChild(board);
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
    const card = document.createElement("div");
    card.className = "pin-card";
    card.style.setProperty("--pin-color", color);
    card.appendChild(pinPortHtml(isExecPin(p) ? "exec" : type));
    const title = document.createElement("div");
    title.className = "pin-card-title";
    appendText(title, "b", "", p.name);
    const small = appendText(title, "small", "", flags);
    appendText(small, "span", "type-detail", " - " + type);
    card.appendChild(title);
    appendButton(card, "", "Actions", "", { "data-pin-menu": p.pin_id });
    return card;
  }

  function inlinePinType(graph, expr) {
    const pins = (graph && graph.pins || []).filter((pin) => pin.node_id === expr.node_id && pin.direction === "input" && !isExecPin(pin));
    return (pins.find((pin) => pin.name === expr.role) || pins[0] || {}).type || "Value";
  }

  function inlinePinFacts(graph, expr) {
    if (!graph || !expr) return null;
    const pins = (graph.pins || []).filter((pin) => pin.node_id === expr.node_id && pin.direction === "input" && !isExecPin(pin));
    return pins.find((pin) => pin.name === expr.role)
      || pins.find((pin) => pin.source_span && expr.source_span && spansOverlap(pin.source_span, expr.source_span))
      || pins[0]
      || null;
  }

  function graphForInlineExpr(inlineId) {
    for (const graph of (latestDoc && latestDoc.graphs) || []) {
      const expr = (graph.inline_exprs || []).find((candidate) => candidate.inline_expr_id === inlineId);
      if (expr) return { graph, expr };
    }
    return null;
  }

  function typeWithoutFallibility(type) {
    return String(type || "").trim().replace(/\?$/, "");
  }

  function isFunctionType(type) {
    return /^(?:fn\b|Fn\b|Function\b)/.test(String(type || "").trim()) || /=>/.test(String(type || ""));
  }

  function isUnknownType(type) {
    return !type || type === "unknown";
  }

  function pinGraphMatches(graph, pin) {
    if (!graph || !pin) return false;
    const staged = stagedNodeForPin(pin);
    if (staged) return staged.graph_id === graph.graph_id;
    if ((graph.nodes || []).some((node) => node.node_id === pin.node_id)) return true;
    return String(pin.pin_id || "").startsWith(graph.graph_id + ":")
      || String(pin.node_id || "").startsWith(graph.graph_id + ":");
  }

  function dataCompatibilityPlan(out, input) {
    const outType = String(out && out.type || "Value");
    const inputType = String(input && input.type || "Value");
    const outBase = typeWithoutFallibility(outType);
    const inputBase = typeWithoutFallibility(inputType);
    const outFallible = !!(out && out.fallible) || outType.endsWith("?");
    const inputFallible = !!(input && input.fallible) || inputType.endsWith("?");
    const outAbility = String(out && out.ability || "");
    const inputAbility = String(input && input.ability || "");

    if (isFunctionType(outBase) && !isFunctionType(inputBase)) {
      return { ok: false, label: "Function value cannot connect to " + inputBase + " input", color: "#fb7185" };
    }
    if (outAbility !== inputAbility) {
      return { ok: false, label: "Capability mismatch " + (outAbility || "value") + " -> " + (inputAbility || "value"), color: "#fb7185" };
    }
    if (outFallible && !inputFallible) {
      return { ok: false, label: "Fallible output cannot connect to infallible " + inputBase + " input", color: "#fb7185" };
    }
    if (isUnknownType(outBase) || isUnknownType(inputBase)) {
      return { ok: false, label: "Unknown data type " + outBase + " -> " + inputBase, color: "#fb7185" };
    }
    if (outBase === inputBase) {
      return { ok: true, exact: true, label: "Connect " + inputBase, color: colorForType(outType) };
    }
    if (numericType(outBase) && numericType(inputBase)) {
      return { ok: true, exact: false, label: "Insert visible conversion " + outBase + " -> " + inputBase, color: colorForType(outType) };
    }
    return { ok: false, label: "Type mismatch " + outBase + " -> " + inputBase, color: "#fb7185" };
  }

  function expressionFacts(graph, source) {
    const text = String(source || "").trim();
    const fallible = text.includes("?") && !text.includes("??");
    const ability = /^&\s*/.test(text) ? "&" : "";
    if (!text) return { type: "unknown", known: true, invalid: true, label: "Expression is required" };
    if (/^\"(?:[^\"\\]|\\.)*\"$/.test(text)) return { type: "String", known: true, fallible, ability };
    if (/^'(?:[^'\\]|\\.)'$/.test(text)) return { type: "Char", known: true, fallible, ability };
    if (/^(?:true|false)$/.test(text)) return { type: "Bool", known: true, fallible, ability };
    if (/^[+-]?(?:\d+\.\d*|\.\d+)(?:[eE][+-]?\d+)?$/.test(text)) return { type: "Float", known: true, fallible, ability };
    if (/^[+-]?\d+$/.test(text)) return { type: "Int", known: true, fallible, ability };
    if (/^(?:!|not\b)|&&|\|\||==|!=|<=|>=|<|>/.test(text)) return { type: "Bool", known: true, fallible, ability };
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(text)) {
      const pin = (graph.pins || []).find((candidate) => candidate.direction === "output" && candidate.name === text);
      if (pin) return { type: pin.type || "unknown", known: true, fallible: !!pin.fallible, ability: pin.ability || "" };
      const node = (graph.nodes || []).find((candidate) => candidate.title === text);
      if (node) {
        const output = (graph.pins || []).find((candidate) => candidate.node_id === node.node_id && candidate.direction === "output" && !isExecPin(candidate));
        if (output) return { type: output.type || "unknown", known: true, fallible: !!output.fallible, ability: output.ability || "" };
      }
      if ((latestDoc && latestDoc.graphs || []).some((candidate) => candidate.title === text || candidate.function && candidate.function.name === text)) {
        return { type: "fn", known: true, functionValue: true, fallible, ability };
      }
      return { type: "unknown", known: false, label: "Unknown value " + text };
    }
    const arithmetic = text.match(/^[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?/g);
    if (/[+\-*\/%]/.test(text) && arithmetic && arithmetic.length) {
      const parts = arithmetic.map((part) => expressionFacts(graph, part));
      if (parts.every((part) => part.known && numericType(part.type))) {
        return { type: parts.some((part) => part.type === "Float") ? "Float" : "Int", known: true, fallible, ability };
      }
    }
    const callee = text.match(/^([A-Za-z_][A-Za-z0-9_.]*)\s*\(/);
    if (callee) {
      const node = (graph.nodes || []).find((candidate) => candidate.title === callee[1] || candidate.title === "." + callee[1]);
      const output = node && (graph.pins || []).find((candidate) => candidate.node_id === node.node_id && candidate.direction === "output" && !isExecPin(candidate));
      if (output) return { type: output.type || "unknown", known: true, fallible: !!output.fallible, ability: output.ability || "" };
    }
    return { type: null, known: false, fallible, ability };
  }

  function inlineEditPlan(body) {
    if (!body || body.op !== "edit_inline_expr") return null;
    const found = graphForInlineExpr(body.inline_expr_id);
    if (!found) return null;
    if (latestDoc && body.revision && body.revision !== latestDoc.revision) {
      return { ok: false, label: "Source changed since this editor opened", color: "#fb7185" };
    }
    const input = inlinePinFacts(found.graph, found.expr);
    if (!input) return null;
    const facts = expressionFacts(found.graph, body.new_expr);
    if (facts.invalid) return { ok: false, label: facts.label, color: "#fb7185" };
    if (!facts.known || !facts.type) return null;
    const plan = dataCompatibilityPlan({ type: facts.type, fallible: facts.fallible, ability: facts.ability }, input);
    if (plan.ok && !plan.exact) {
      return { ok: false, label: "Inline value type " + facts.type + " does not match " + input.type, color: "#fb7185" };
    }
    return plan;
  }

  function inlineDetailField(graph, expr, applyOp) {
    const type = inlinePinType(graph, expr);
    const pseudo = { name: expr.role, type, source: "local", defaultSource: expr.source };
    return valueDetailField(graph, pseudo, expr, expr.inline_expr_id, applyOp);
  }

  function appendFunctionPanel(parent, graph, node, fnMeta, state) {
    const section = appendDetailsSection(parent, "Function");
    const board = document.createElement("div");
    board.className = "signature-board";
    const head = document.createElement("div");
    head.className = "signature-head";
    const headText = document.createElement("div");
    appendText(headText, "span", "sig-eyebrow", "Function");
    appendText(headText, "b", "", (fnMeta.visibility || "private") + " " + (fnMeta.name || node.title));
    appendText(headText, "code", "", fnMeta.signature || "");
    head.appendChild(headText);
    const createFunction = appendButton(head, "create-function", "New");
    createFunction.title = "Create sibling function";
    board.appendChild(head);
    const meta = metadataDetailsHtml(fnMeta.meta);
    if (meta) board.appendChild(meta);

    const inputLane = document.createElement("div");
    inputLane.className = "pin-lane";
    const inputHead = document.createElement("div");
    inputHead.className = "lane-head";
    appendText(inputHead, "b", "", "Inputs");
    appendText(inputHead, "span", "lane-meta", (fnMeta.params || []).length);
    appendButton(inputHead, "add-function-pin", "+ Input");
    inputLane.appendChild(inputHead);
    const inputList = document.createElement("div");
    inputList.className = "pin-list";
    inputList.id = "function-pin-list";
    for (const [index, parameter] of (fnMeta.params || []).entries()) inputList.appendChild(functionParamRow(parameter, index));
    if (!inputList.firstChild) appendText(inputList, "div", "pin-empty", "No inputs");
    inputLane.appendChild(inputList);
    board.appendChild(inputLane);

    const outputLane = document.createElement("div");
    outputLane.className = "pin-lane";
    const outputHead = document.createElement("div");
    outputHead.className = "lane-head";
    appendText(outputHead, "b", "", "Output");
    appendText(outputHead, "span", "lane-meta", "return type");
    appendButton(outputHead, "add-function-output", "+ Output");
    outputLane.appendChild(outputHead);
    const outputList = document.createElement("div");
    outputList.className = "pin-list";
    outputList.appendChild(functionReturnRow(typeof fnMeta.returns === "string" ? fnMeta.returns : (fnMeta.returns && fnMeta.returns.type) || "Void"));
    outputLane.appendChild(outputList);
    board.appendChild(outputLane);

    const source = document.createElement("div");
    source.className = "signature-source";
    appendText(source, "span", "sig-eyebrow", "Source signature");
    appendText(source, "code", "", fnMeta.signature || "");
    const fields = document.createElement("div");
    renderFieldDescriptors(fields, functionDetailDescriptors(graph, node, fnMeta), { state, fieldsClass: "edit-grid", actionsClass: "signature-actions" });
    source.appendChild(fields);
    board.appendChild(source);

    const pinActions = document.createElement("div");
    pinActions.className = "signature-actions";
    appendButton(pinActions, "apply-function-pins", "Apply pins", "primary");
    board.appendChild(pinActions);
    section.appendChild(board);

    const effects = (fnMeta.effects || []).map((effect) => ({ label: effect, value: "Effect" }));
    const markers = [fnMeta.pure ? "Pure" : "", fnMeta.unsafe ? "Unsafe" : ""].filter(Boolean).map((marker) => ({ label: marker, value: "Marker" }));
    if (effects.length || markers.length) {
      const effectsSection = appendDetailsSection(parent, "Effects and markers");
      const list = document.createElement("div");
      list.className = "pin-list";
      for (const row of effects.concat(markers)) {
        const item = document.createElement("div");
        item.className = "pin-row";
        appendText(item, "b", "", row.label);
        appendText(item, "span", "tag", row.value);
        list.appendChild(item);
      }
      effectsSection.appendChild(list);
    }
    const events = fnMeta && graph.event_views || [];
    if (events.length) {
      // The old <h2>Events</h2> contract is built with createElement.
      const eventsSection = appendDetailsSection(parent, "Events");
      const list = document.createElement("div");
      list.className = "pin-list";
      for (const event of events) {
        const item = document.createElement("div");
        item.className = "pin-row";
        appendText(item, "b", "", event.title || event.function);
        appendText(item, "span", "tag", "Function event");
        list.appendChild(item);
      }
      eventsSection.appendChild(list);
    }
  }

  function appendInlineDetails(parent, graph, expr, state) {
    const row = document.createElement("div");
    row.className = "inline-row";
    appendText(row, "b", "", expr.role);
    appendText(row, "code", "", expr.source);
    const operation = {
      id: "inline:" + expr.inline_expr_id,
      label: "Apply expression",
      buttonAttributes: { "data-inline-apply": expr.inline_expr_id },
      run: (values, fields, context) => ({
        schema_version: 1,
        op: "edit_inline_expr",
        revision: context.revision,
        inline_expr_id: expr.inline_expr_id,
        new_expr: values["inline-value"]
      })
    };
    const fieldHost = document.createElement("div");
    renderFieldDescriptors(fieldHost, [inlineDetailField(graph, expr, operation)], { state, fieldsClass: "edit-grid", actionsClass: "edit-grid" });
    row.appendChild(fieldHost);
    const actions = document.createElement("div");
    actions.className = "edit-grid";
    appendButton(actions, "", "Promote to binding", "", { "data-inline-promote": expr.inline_expr_id });
    appendButton(actions, "", "Insert conversion", "", { "data-inline-convert": expr.inline_expr_id });
    appendButton(actions, "", "Preview extract", "", { "data-inline-preview-extract": expr.inline_expr_id });
    appendButton(actions, "", "Extract function", "", { "data-inline-extract": expr.inline_expr_id });
    row.appendChild(actions);
    parent.appendChild(row);
  }

  function appendCommentRegion(parent, graph, region, state) {
    const row = document.createElement("div");
    row.className = "inline-row";
    appendText(row, "b", "", region.title || "Comment");
    appendText(row, "code", "", region.region_id);
    const bounds = region.bounds || { x: 0, y: 0, w: 360, h: 180 };
    const id = String(region.region_id);
    const operation = {
      id: "region:" + id,
      label: "Apply comment",
      buttonAttributes: { "data-region-apply": id },
      run: (values, fields, context) => ({
        schema_version: 1,
        op: "edit_comment_region",
        revision: context.revision,
        region_id: id,
        title: values.title,
        color: values.color,
        alpha: values.alpha,
        bounds: values.bounds
      })
    };
    const fields = [
      { key: "title", label: "Title", value: region.title || "Comment", editable: true, apply_op: operation, dataAttributes: { "data-region-title": id } },
      { key: "color", label: "Color", value: region.color || "#2563eb", editable: true, apply_op: operation, dataAttributes: { "data-region-color": id } },
      { key: "alpha", label: "Alpha", value: region.alpha || "0.18", editable: true, apply_op: operation, dataAttributes: { "data-region-alpha": id } },
      { key: "bounds", label: "Bounds", value: [bounds.x || 0, bounds.y || 0, bounds.w || 360, bounds.h || 180].join(","), editable: true, apply_op: operation, dataAttributes: { "data-region-bounds": id } }
    ];
    const fieldHost = document.createElement("div");
    renderFieldDescriptors(fieldHost, fields, { state, fieldsClass: "edit-grid", actionsClass: "edit-grid" });
    row.appendChild(fieldHost);
    appendButton(row, "", "Delete comment", "", { "data-region-delete": id });
    parent.appendChild(row);
  }

  function updateDetails(graph, node, pins, inline) {
    if (!node) {
      beginDetailsEditor("none");
      clearDom(details);
      const empty = document.createElement("div");
      empty.className = "details-empty";
      appendText(empty, "b", "", "No node selected");
      appendText(empty, "span", "tag", "Select, marquee, or use the graph tabs.");
      details.appendChild(empty);
      return;
    }
    const style = nodeStyle(node, graph);
    details.style.setProperty("--node-accent", style.accent);
    const span = node.source_span || { start: 0, end: 0 };
    const state = beginDetailsEditor("node:" + (graph.graph_id || "graph") + ":" + node.node_id);
    const calleeGraph = (node.kind === "call" || node.kind === "method") ? graphForFunctionName(node.title) : null;
    const fnMeta = node.node_id === graph.entry_node ? graph.function : null;
    const bpLabel = nodeBreakpoint(node) ? "Remove breakpoint" : "Set breakpoint";
    clearDom(details);

    const hero = document.createElement("div");
    hero.className = "details-hero";
    const titleLine = document.createElement("div");
    titleLine.className = "details-titleline";
    appendText(titleLine, "span", "node-glyph", style.glyph);
    const title = document.createElement("div");
    title.className = "details-title";
    appendText(title, "p", "title", node.title);
    appendText(title, "div", "kind", nodeKindLabel(node, graph));
    titleLine.appendChild(title);
    hero.appendChild(titleLine);
    appendText(hero, "span", "", nodeDescription(node, graph));
    const chips = document.createElement("div");
    chips.className = "details-chips dev-only";
    appendText(chips, "span", "details-chip", node.kind);
    appendText(chips, "span", "details-chip type-detail", pins.length + " pins");
    for (const affordance of (node.edit_affordances || []).slice(0, 4)) appendText(chips, "span", "details-chip", affordance);
    hero.appendChild(chips);
    if (pasteRenameChips.length) {
      const pasteChips = document.createElement("div");
      pasteChips.className = "details-chips";
      pasteChips.setAttribute("data-paste-renames", "true");
      for (const rename of pasteRenameChips) appendText(pasteChips, "span", "details-chip", "rename " + rename.from + " → " + rename.to);
      hero.appendChild(pasteChips);
    }
    const quickActions = document.createElement("div");
    quickActions.className = "quick-actions";
    appendButton(quickActions, "source-jump", "Jump source");
    appendButton(quickActions, "find-references", "Find refs");
    if (calleeGraph) {
      const open = appendButton(quickActions, "open-callee-graph", "Open " + calleeGraph.title + " graph", "wide");
      open.addEventListener("click", () => openFunctionGraph(calleeGraph.title));
    }
    hero.appendChild(quickActions);
    const facts = document.createElement("div");
    facts.className = "dev-only";
    renderFieldDescriptors(facts, nodeDetailDescriptors(node, graph, pins), { state, fieldsClass: "details-facts" });
    hero.appendChild(facts);
    details.appendChild(hero);

    if (node.kind === "binding") {
      const renameBox = document.createElement("div");
      renameBox.className = "edit-grid";
      const renameOp = {
        id: "node-rename",
        label: "Rename",
        buttonId: "rename-binding",
        primary: true,
        run: (values, fields, context) => ({ schema_version: 1, op: "rename_binding", revision: context.revision, from: node.title, to: values.name })
      };
      renderFieldDescriptors(renameBox, [{ key: "name", id: "rename-to", label: "Rename binding", value: node.title, source: node.title, editable: true, apply_op: renameOp }], { state, fieldsClass: "", actionsClass: "edit-grid" });
      const preview = appendButton(renameBox, "preview-rename", "Preview rename");
      preview.addEventListener("click", () => postQuery({ op: "preview_rename", symbol: node.title, to: document.getElementById("rename-to").value.trim() }));
      details.appendChild(renameBox);
    }
    if (fnMeta) appendFunctionPanel(details, graph, node, fnMeta, state);

    const debug = appendDetailsSection(details, "Debug", "debug-detail");
    const debugActions = document.createElement("div");
    debugActions.className = "edit-grid";
    appendButton(debugActions, "debug-toggle-break", bpLabel);
    appendButton(debugActions, "debug-add-watch", "Add watch");
    debug.appendChild(debugActions);
    const live = document.createElement("div");
    live.className = "pin-list";
    const debugFragment = debugRows(debugOverlay && debugOverlay.locals);
    if (debugOverlay && debugOverlay.watches) debugFragment.appendChild(debugRows(debugOverlay.watches));
    if (debugOverlay && debugOverlay.call_stack) debugFragment.appendChild(debugRows(debugOverlay.call_stack.map((frame) => ({ value: frame }))));
    if (debugFragment.childNodes.length) live.appendChild(debugFragment);
    else appendText(live, "div", "tag", "no live values");
    debug.appendChild(live);

    const comments = appendDetailsSection(details, "Comments", "diagnostic-detail");
    const commentList = document.createElement("div");
    commentList.className = "inline-list";
    const regions = regionsForNode(graph, node);
    for (const region of regions) appendCommentRegion(commentList, graph, region, state);
    if (!regions.length) appendText(commentList, "div", "tag", "none");
    comments.appendChild(commentList);

    const types = appendDetailsSection(details, "Pins", "type-detail");
    const pinList = document.createElement("div");
    pinList.className = "pin-list";
    for (const pin of pins) pinList.appendChild(pinCardHtml(pin));
    if (!pins.length) appendText(pinList, "div", "tag", "none");
    types.appendChild(pinList);
    appendText(types, "h2", "", "Inline");
    const inlineList = document.createElement("div");
    inlineList.className = "inline-list";
    for (const expr of inline) appendInlineDetails(inlineList, graph, expr, state);
    if (!inline.length) appendText(inlineList, "div", "tag", "none");
    types.appendChild(inlineList);

    details.querySelectorAll("[data-param-remove]").forEach((button) => {
      button.addEventListener("click", () => {
        const row = button.closest("[data-fn-param]");
        if (row) row.remove();
        if (fnMeta) applyFunctionPins(graph, fnMeta, node.title);
      });
    });
    details.querySelectorAll("[data-pin-menu]").forEach((button) => {
      button.addEventListener("click", (event) => {
        const pin = pins.find((candidate) => candidate.pin_id === button.getAttribute("data-pin-menu"));
        if (pin) openPinMenu(pin, event.clientX, event.clientY);
      });
    });
    const sourceJump = document.getElementById("source-jump");
    if (sourceJump) sourceJump.addEventListener("click", () => {
      setSourceHash(span);
      showToast("Source location selected");
    });
    const findReferences = document.getElementById("find-references");
    if (findReferences) findReferences.addEventListener("click", () => postQuery({ op: "references", symbol: node.title }));
    const createFunction = document.getElementById("create-function");
    if (createFunction) createFunction.addEventListener("click", () => {
      const name = window.prompt("Function name", "helper");
      if (!name) return;
      const params = window.prompt("Parameters", "value: Int") || "";
      const ret_type = window.prompt("Return type", "Int") || "Int";
      postTransaction({ schema_version: 1, op: "create_function", revision: latestDoc.revision, name, params, ret_type });
    });
    const addFunctionPin = document.getElementById("add-function-pin");
    if (addFunctionPin && fnMeta) addFunctionPin.addEventListener("click", () => {
      const list = document.getElementById("function-pin-list");
      const row = functionParamRow({ name: nextParamName(fnMeta), type: "Int", default_source: "" }, "new" + Date.now());
      const empty = list.querySelector(".pin-empty");
      if (empty) empty.remove();
      list.appendChild(row);
      const remove = row.querySelector("[data-param-remove]");
      if (remove) remove.addEventListener("click", () => {
        row.remove();
        applyFunctionPins(graph, fnMeta, node.title);
      });
      applyFunctionPins(graph, fnMeta, node.title);
    });
    ["apply-function-pins", "set-function-output", "remove-function-output", "add-function-output"].forEach((id) => {
      const button = document.getElementById(id);
      if (button) button.addEventListener("click", handleFunctionPinButton);
    });
    details.querySelectorAll("[data-inline-promote]").forEach((button) => button.addEventListener("click", () => {
      const id = button.getAttribute("data-inline-promote");
      const name = window.prompt("Binding name", "value");
      if (name && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) return showToast("Binding name must be a Jet identifier");
      if (name) postTransaction({ schema_version: 1, op: "promote_to_binding", revision: latestDoc.revision, inline_expr_id: id, name });
    }));
    details.querySelectorAll("[data-inline-convert]").forEach((button) => button.addEventListener("click", () => {
      const id = button.getAttribute("data-inline-convert");
      const callee = window.prompt("Conversion function", "Float.from");
      if (callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: id, callee });
    }));
    details.querySelectorAll("[data-inline-preview-extract], [data-inline-extract]").forEach((button) => button.addEventListener("click", () => {
      const id = button.getAttribute("data-inline-preview-extract") || button.getAttribute("data-inline-extract");
      const name = window.prompt("Helper function", "extracted");
      if (!name) return;
      const op = button.hasAttribute("data-inline-preview-extract") ? "preview_extract_inline_expr" : "extract_inline_expr";
      postTransaction({ schema_version: 1, op, revision: latestDoc.revision, inline_expr_id: id, function: name, ret_type: "Int" });
    }));
    const toggle = document.getElementById("debug-toggle-break");
    if (toggle) toggle.addEventListener("click", () => toggleBreakpoint(node));
    const watch = document.getElementById("debug-add-watch");
    if (watch) watch.addEventListener("click", () => addWatch((window.prompt("Watch local", node.title) || "").trim()));
    details.querySelectorAll("[data-region-delete]").forEach((button) => button.addEventListener("click", () => {
      const id = button.getAttribute("data-region-delete");
      postTransaction({ schema_version: 1, op: "delete_comment_region", revision: latestDoc.revision, region_id: id });
    }));
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
    selectionExplicitlyCleared = selectedNodeIds.size === 0;
    selectedNodeId = selectedNodeIds.has(node.node_id) ? node.node_id : [...selectedNodeIds].at(-1) || null;
    const s = node.source_span || { start: 0, end: 0 };
    if (selectedNodeId === node.node_id) setSourceHash(s);
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
    if (isExecPin(out) || isExecPin(input)) return isExecPin(out) && isExecPin(input);
    return dataCompatibilityPlan(out, input).ok;
  }

  function connectionPlan(graph, fromPin, toPin, context) {
    if (!fromPin) return { ok: false, label: "No pin", color: "#fb7185" };
    if (!toPin) return { ok: true, label: "Release for actions", color: "#7dd3fc" };
    if (!graph || !pinGraphMatches(graph, fromPin) || !pinGraphMatches(graph, toPin)
      || (context && context.graphId && context.graphId !== graph.graph_id)) {
      return { ok: false, label: "Pins belong to different graphs", color: "#fb7185" };
    }
    if (context && latestDoc && context.revision && context.revision !== latestDoc.revision) {
      return { ok: false, label: "Source changed since drag started", color: "#fb7185" };
    }
    if (context && context.sourceId && currentCanvasSourceId() !== context.sourceId) {
      return { ok: false, label: "Source file changed since drag started", color: "#fb7185" };
    }
    if (fromPin.pin_id === toPin.pin_id) return { ok: false, label: "Same pin", color: "#fb7185" };
    if (fromPin.direction === toPin.direction) {
      const expected = fromPin.direction === "output" ? "Drop on an input pin" : "Drop on an output pin";
      return { ok: false, label: expected, color: "#fb7185" };
    }
    const out = fromPin.direction === "output" ? fromPin : toPin;
    const input = fromPin.direction === "input" ? fromPin : toPin;
    if (isExecPin(out) !== isExecPin(input)) return { ok: false, label: "Control and data pins cannot connect", color: "#fb7185" };
    if (isExecPin(out)) return { ok: true, label: "Reorder steps", color: "#f8fafc" };
    const compatibility = dataCompatibilityPlan(out, input);
    if (!compatibility.ok) return compatibility;
    if (!compatibility.exact) return compatibility;
    const wire = wireIntoPin(graph, input);
    const replacement = sourceExprForOutputPin(out);
    if (wire && replacement) return { ok: true, label: "Rewire source", color: compatibility.color };
    return { ok: true, label: "Connect " + (input.type || "Value"), color: compatibility.color };
  }

  function drawCompatibleDropTargets(graph, fromPin, context) {
    const seen = new Set();
    for (const hit of pinHit) {
      const pin = hit.pin;
      const plan = connectionPlan(graph, fromPin, pin, context);
      if (!plan.ok || seen.has(pin.pin_id)) continue;
      seen.add(pin.pin_id);
      const point = pinPoints.get(pin.pin_id);
      if (!point) continue;
      ctx.beginPath();
      ctx.arc(point.x, point.y, Math.max(13, 18 * view.zoom), 0, Math.PI * 2);
      ctx.strokeStyle = plan.color || colorForType(pin.type || "Value");
      ctx.lineWidth = Math.max(1.4, 2.2 * view.zoom);
      ctx.shadowColor = hexToRgba(plan.color || colorForType(pin.type || "Value"), .52);
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

  function sourceExpressionFromNode(node) {
    if (!node || !latestDoc || !latestDoc.source_text || !node.source_span) return null;
    const source = latestDoc.source_text;
    const start = Number(node.source_span.start);
    const nameEnd = Number(node.source_span.end);
    if (!Number.isInteger(start) || !Number.isInteger(nameEnd) || start < 0 || nameEnd < start) return null;
    let end = nameEnd;
    const head = source.slice(start, nameEnd).trim();
    const isCall = node.kind === "function"
      && (node.archetype === "function_pure" || node.archetype === "function_exec")
      && node.title === head;
    if (node.kind === "function" && !isCall) return null;
    if (isCall) {
      const open = source.indexOf("(", nameEnd);
      if (open >= nameEnd && !source.slice(nameEnd, open).trim()) {
        let depth = 0;
        let quote = null;
        let escaped = false;
        for (let i = open; i < source.length; i++) {
          const ch = source[i];
          if (quote) {
            if (escaped) escaped = false;
            else if (ch === "\\") escaped = true;
            else if (ch === quote) quote = null;
            continue;
          }
          if (ch === '"' || ch === "'") {
            quote = ch;
          } else if (ch === "(") {
            depth += 1;
          } else if (ch === ")") {
            depth -= 1;
            if (depth === 0) {
              end = i + 1;
              break;
            }
          }
        }
      }
      if (end === nameEnd) return null;
    }
    const text = source.slice(start, end).trim();
    return text && !text.includes("\n") ? text : null;
  }

  function qualifiedSourceExpression(source) {
    return /^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/.test(String(source || "").trim());
  }

  function sourceExprForOutputPin(pin) {
    if (!pin || pin.direction !== "output") return null;
    const graph = latestDoc ? currentGraph(latestDoc) : null;
    const node = graph && (graph.nodes || []).find((n) => n.node_id === pin.node_id);
    if (node && ["entry", "binding", "variable_get"].includes(node.kind)) {
      const name = node.kind === "entry" ? pin.name : node.title;
      if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) return name;
    }
    return sourceExpressionFromNode(node);
  }

  function wireIntoPin(graph, pin) {
    if (!graph || !pin) return null;
    return (graph.wires || []).find((w) => w.to_pin === pin.pin_id && w.source_span);
  }

  function isSecondExecDrop(graph, fromPin, target) {
    if (!graph || !fromPin || !target || fromPin.pin_id === target.pin_id) return false;
    if (!isExecPin(fromPin) || !isExecPin(target) || fromPin.direction === target.direction) return false;
    const input = fromPin.direction === "input" ? fromPin : target;
    return (graph.wires || []).some((wire) => wire.wire_kind === "control" && wire.to_pin === input.pin_id);
  }

  function inlineForPin(graph, pin) {
    if (!graph || !pin || !pin.source_span) return null;
    const owner = (graph.inline_exprs || []).find((e) => e.node_id === pin.node_id
      && (e.role === pin.name || (pin.name === "value" && ["init", "value"].includes(e.role))));
    return owner || (graph.inline_exprs || []).find((e) => e.source_span && spansOverlap(e.source_span, pin.source_span));
  }

  function selectMarquee() {
    if (!drag || drag.mode !== "marquee") return;
    const x0 = Math.min(drag.x, drag.mx), x1 = Math.max(drag.x, drag.mx);
    const y0 = Math.min(drag.y, drag.my), y1 = Math.max(drag.y, drag.my);
    const next = drag.selectionMode === "add" || drag.selectionMode === "toggle"
      ? new Set(drag.initialSelection || [])
      : new Set();
    const inside = new Set();
    for (const h of hit) {
      if (h.x < x1 && h.x + h.w > x0 && h.y < y1 && h.y + h.h > y0) inside.add(h.node.node_id);
    }
    for (const h of commentHit) {
      if (h.x < x1 && h.x + h.w > x0 && h.y < y1 && h.y + h.h > y0) inside.add(h.box.comment_id);
    }
    if (drag.selectionMode === "toggle") {
      for (const id of inside) {
        if (next.has(id)) next.delete(id);
        else next.add(id);
      }
    } else {
      for (const id of inside) next.add(id);
    }
    selectedNodeIds = next;
    selectionExplicitlyCleared = selectedNodeIds.size === 0;
    selectedNodeId = [...selectedNodeIds].at(-1) || null;
  }

  function completeConnection(fromPin, target, graph) {
    const plan = connectionPlan(graph, fromPin, target, drag);
    window.__jetCanvasLastConnectionPlan = plan;
    if (!plan.ok) {
      if (target) showToast("Wire refused: " + plan.label, { isError: true });
      return true;
    }
    if (!drag?.rewire
      && isSecondExecDrop(graph, fromPin, target)
      && !stagedNodeForPin(fromPin)
      && !stagedNodeForPin(target)) {
      openExecConvergencePreview(graph, fromPin, target);
      return true;
    }
    if (materializeStagedConnection(fromPin, target, graph)) return true;
    if (drag && drag.rewire && target && drag.rewire.wire && drag.rewire.wire.wire_kind === "control" && isExecPin(target)) {
      return completeExecRewire(drag.rewire, target, graph);
    }
    if (plan.ok) {
      const out = fromPin.direction === "output" ? fromPin : target;
      const input = fromPin.direction === "input" ? fromPin : target;
      const wire = wireIntoPin(graph, input);
      const replacement = sourceExprForOutputPin(out);
      const expr = inlineForPin(graph, input);
      if (plan.exact && wire && replacement && qualifiedSourceExpression(replacement)) {
        postTransaction({ schema_version: 1, op: "move_link", revision: latestDoc.revision, wire_id: wire.wire_id, replacement });
      } else if (plan.exact && replacement && expr) {
        postTransaction({ schema_version: 1, op: "edit_inline_expr", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, new_expr: replacement });
      } else if (!plan.exact) {
        const expr = inlineForPin(graph, input);
        const callee = window.prompt("Visible conversion function", (input.type || "Value") + ".from");
        if (expr && callee) postTransaction({ schema_version: 1, op: "insert_visible_conversion", revision: latestDoc.revision, inline_expr_id: expr.inline_expr_id, callee });
        else showToast("Conversion needs an inline source expression");
      } else {
        showToast(plan.label + ": no safe source anchor");
      }
      return true;
    }
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

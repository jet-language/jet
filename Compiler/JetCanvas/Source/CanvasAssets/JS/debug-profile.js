  // The Debug and Profile views: every debugger panel at once (source, call
  // stack, locals, watches, tasks and the console) over the one debug session
  // the graph overlay uses, and the profile timeline of one .jettrace, which
  // the server reduces to the window shown. State here is `var` so the views
  // can render before this file's statements run.
  var profileState = { doc: null, trace: "", zoom: [], busy: false, selected: null, dragFrom: null, dragTo: null };
  var PROFILE_LANE_COLORS = { spans: "#60a5fa", zones: "#a78bfa", waits: "#f59e0b", io: "#34d399" };
  var PROFILE_LANE_HEIGHT = 44;
  var PROFILE_AXIS_HEIGHT = 22;
  var PROFILE_LABEL_WIDTH = 64;

  function debugViewElement(id) {
    return document.getElementById(id);
  }

  function debugViewRow(parts, className) {
    const row = document.createElement("div");
    row.className = "row" + (className ? " " + className : "");
    for (const part of parts) {
      const span = document.createElement("span");
      if (part[1]) span.className = part[1];
      span.textContent = part[0];
      row.appendChild(span);
    }
    return row;
  }

  function debugViewFill(id, rows, empty) {
    const host = debugViewElement(id);
    if (!host) return;
    host.innerHTML = "";
    if (!rows.length) {
      const note = document.createElement("div");
      note.className = "empty";
      note.textContent = empty;
      host.appendChild(note);
      return;
    }
    for (const row of rows) host.appendChild(row);
  }

  // The 1-based lines that carry a breakpoint at the current revision.
  function debugViewBreakLines(source) {
    const lines = new Set();
    if (!latestDoc) return lines;
    const encoder = new TextEncoder();
    const bytes = encoder.encode(source);
    for (const mark of (debugState.breakpoints || [])) {
      if (mark.revision !== latestDoc.revision || !mark.source_span) continue;
      let line = 1;
      const limit = Math.min(mark.source_span.start, bytes.length);
      for (let i = 0; i < limit; i++) if (bytes[i] === 10) line++;
      lines.add(line);
    }
    return lines;
  }

  // A line breakpoint, anchored to the line's byte span like a node's.
  function toggleLineBreakpoint(line) {
    if (!latestDoc || typeof latestDoc.source_text !== "string") return;
    loadDebugState(latestDoc);
    const encoder = new TextEncoder();
    const lines = latestDoc.source_text.split("\n");
    if (line < 1 || line > lines.length) return;
    let start = 0;
    for (let i = 0; i < line - 1; i++) start += encoder.encode(lines[i]).length + 1;
    const end = start + encoder.encode(lines[line - 1].replace(/\r$/, "")).length;
    const marked = debugViewBreakLines(latestDoc.source_text);
    if (marked.has(line)) {
      const bytes = encoder.encode(latestDoc.source_text);
      debugState.breakpoints = debugState.breakpoints.filter((mark) => {
        if (mark.revision !== latestDoc.revision || !mark.source_span) return true;
        let at = 1;
        for (let i = 0; i < Math.min(mark.source_span.start, bytes.length); i++) if (bytes[i] === 10) at++;
        return at !== line;
      });
    } else {
      if (debugState.breakpoints.length >= 128) {
        showToast("Breakpoint limit reached; source was kept", { isError: true });
        return;
      }
      const span = { start, end };
      debugState.breakpoints.push({ anchor: spanAnchor(span), source_span: span, revision: latestDoc.revision });
    }
    saveDebugState();
    renderDebugView();
    if (debugSessionId) runDebug([]);
  }

  function renderDebugView() {
    if (typeof viewMode === "undefined" || viewMode !== "debug") return;
    const overlay = debugOverlay;
    const state = debugViewElement("debug-view-state");
    if (state) {
      const line = overlay && overlay.active_line ? " · line " + overlay.active_line : "";
      state.textContent = (debugSessionId ? "Paused" : overlay ? "Finished" : "No session") + line;
    }
    const recorded = !!(debugSessionInfo && debugSessionInfo.tier === "native-recorded");
    const reverse = debugViewElement("debug-view-reverse");
    if (reverse) reverse.disabled = !recorded;
    for (const id of ["debug-view-continue", "debug-view-next", "debug-view-step", "debug-view-finish", "debug-view-stop"]) {
      const button = debugViewElement(id);
      if (button) button.disabled = !debugSessionId;
    }
    // Source: the whole entry from the graph document, or the overlay's
    // window around the stop.
    let firstLine = 1;
    let lines = [];
    if (latestDoc && typeof latestDoc.source_text === "string") {
      lines = latestDoc.source_text.split("\n");
      if (lines.length && lines[lines.length - 1] === "") lines.pop();
    } else if (overlay && overlay.source) {
      firstLine = overlay.source.first_line || 1;
      lines = overlay.source.lines || [];
    }
    const breaks = latestDoc && typeof latestDoc.source_text === "string" ? debugViewBreakLines(latestDoc.source_text) : new Set();
    const active = overlay && overlay.active_line;
    const sourceRows = lines.map((text, index) => {
      const number = firstLine + index;
      const classes = (number === active ? "is-active" : "") + (breaks.has(number) ? " is-break" : "");
      const row = debugViewRow([[(breaks.has(number) ? "● " : "") + number, "gutter"], [text, ""]], classes.trim());
      row.dataset.line = String(number);
      row.addEventListener("click", () => toggleLineBreakpoint(number));
      return row;
    });
    debugViewFill("debug-source-lines", sourceRows, "No source yet");
    const activeRow = document.querySelector("#debug-source-lines .is-active");
    if (activeRow && activeRow.scrollIntoView) activeRow.scrollIntoView({ block: "center" });
    const stack = (overlay && overlay.call_stack) || [];
    debugViewFill("debug-stack-rows", stack.map((frame) => debugViewRow([[frame, ""]])), "Not paused");
    const locals = (overlay && overlay.locals) || [];
    debugViewFill("debug-locals-rows", locals.map((local) => debugViewRow([[local.name, "key"], [local.value, ""], [local.type, "type"]])), "Not paused");
    const watched = (overlay && overlay.watches) || [];
    const watchRows = (debugState.watches || []).map((name) => {
      const found = watched.find((watch) => watch.name === name);
      return debugViewRow([[name, "key"], [found ? found.value : "(not in scope)", ""], [found ? found.type : "", "type"]]);
    });
    debugViewFill("debug-watches-rows", watchRows, "No watches");
    const tasks = (overlay && overlay.tasks) || [];
    debugViewFill("debug-tasks-rows", tasks.map((task) => debugViewRow([[typeof task === "string" ? task : JSON.stringify(task), ""]])), "No task list from this session");
    const trace = (overlay && overlay.trace) || [];
    debugViewFill("debug-console-log", trace.map((line) => debugViewRow([[line, ""]])), "Start the entry to debug it");
    const log = debugViewElement("debug-console-log");
    if (log) log.scrollTop = log.scrollHeight;
  }

  function debugViewCommand(commands) {
    if (!debugSessionId && commands.length) return;
    runDebug(commands);
  }

  function profileViewUrl() {
    return window.__JET_CANVAS_PROFILE__ || ((window.__JET_CANVAS_BASE__ || "/canvas") + "/profile");
  }

  function profileViewSetState(text) {
    const state = debugViewElement("profile-view-state");
    if (state) state.textContent = text;
  }

  // Ask for the profile of `trace` ("" profiles the entry) over the window
  // `from`..`to` (null for the whole trace).
  function requestProfile(trace, from, to) {
    if (!latestDoc || profileState.busy) return;
    const timeline = debugViewElement("profile-timeline");
    const width = timeline ? Math.max(1, Math.min(4096, Math.floor(timeline.clientWidth || timeline.width) - PROFILE_LABEL_WIDTH)) : 800;
    const body = { schema_version: 1, revision: latestDoc.revision, width };
    const sourceId = currentCanvasSourceId();
    if (sourceId) body.source_id = sourceId;
    if (trace) body.trace = trace;
    if (from !== null && to !== null) {
      body.from_ns = Math.floor(from);
      body.to_ns = Math.ceil(to);
    }
    profileState.busy = true;
    profileViewSetState(trace ? "Reading trace…" : "Profiling the entry…");
    fetch(profileViewUrl(), { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((r) => r.json().then((j) => ({ ok: r.ok, json: canvasPayload(j) })))
      .then((result) => {
        profileState.busy = false;
        if (!result.ok || !result.json || result.json.ok === false) {
          const message = ((result.json && result.json.message) || "Profile rejected").split("\n")[0];
          profileViewSetState(message);
          showToast(message, { isError: true });
          return;
        }
        profileState.doc = result.json;
        profileState.trace = result.json.trace || trace;
        profileState.selected = null;
        renderProfileView();
      })
      .catch(() => {
        profileState.busy = false;
        profileViewSetState("Profile request failed");
      });
  }

  function profileViewNs(value) {
    if (value >= 1e9) return (value / 1e9).toFixed(3) + " s";
    if (value >= 1e6) return (value / 1e6).toFixed(3) + " ms";
    if (value >= 1e3) return (value / 1e3).toFixed(1) + " µs";
    return value + " ns";
  }

  function renderProfileView() {
    if (typeof viewMode === "undefined" || viewMode !== "profile") return;
    const doc = profileState.doc;
    const timeline = debugViewElement("profile-timeline");
    if (!timeline) return;
    const width = Math.max(200, timeline.clientWidth || 1200);
    const lanes = (doc && doc.lanes) || [];
    const height = PROFILE_AXIS_HEIGHT + Math.max(4, lanes.length) * PROFILE_LANE_HEIGHT;
    timeline.width = width;
    timeline.height = height;
    const g = timeline.getContext("2d");
    g.fillStyle = "#07101a";
    g.fillRect(0, 0, width, height);
    if (!doc) {
      profileViewSetState("No trace");
      g.fillStyle = "#64748b";
      g.font = "13px system-ui, sans-serif";
      g.fillText("Profile the entry, or open a .jettrace, to draw its timeline.", 16, 32);
      debugViewFill("profile-hot-rows", [], "No samples");
      debugViewFill("profile-detail-rows", [], "Drag across the timeline to zoom; click a mark to see it");
      return;
    }
    const from = doc.window.from_ns;
    const to = Math.max(doc.window.to_ns, from + 1);
    const plot = width - PROFILE_LABEL_WIDTH;
    const x = (ns) => PROFILE_LABEL_WIDTH + (ns - from) / (to - from) * plot;
    profileViewSetState(profileViewNs(to - from) + " shown · " + doc.samples + " samples · " + (profileState.trace || ""));
    // The time axis.
    g.fillStyle = "#8aa4c2";
    g.font = "11px ui-monospace, monospace";
    for (let tick = 0; tick <= 4; tick++) {
      const at = PROFILE_LABEL_WIDTH + plot * tick / 4;
      g.fillRect(at, PROFILE_AXIS_HEIGHT - 6, 1, 6);
      const label = profileViewNs(Math.round((to - from) * tick / 4));
      g.fillText(label, Math.min(at + 2, width - g.measureText(label).width - 2), 12);
    }
    profileState.hits = [];
    lanes.forEach((lane, index) => {
      const top = PROFILE_AXIS_HEIGHT + index * PROFILE_LANE_HEIGHT;
      g.fillStyle = index % 2 ? "#0b1520" : "#0a121c";
      g.fillRect(0, top, width, PROFILE_LANE_HEIGHT);
      g.fillStyle = "#8aa4c2";
      g.fillText(lane.name, 6, top + PROFILE_LANE_HEIGHT / 2 + 4);
      g.fillStyle = PROFILE_LANE_COLORS[lane.name] || "#94a3b8";
      for (const mark of lane.marks) {
        const left = Math.max(PROFILE_LABEL_WIDTH, x(mark.start_ns));
        const right = Math.min(width, x(mark.end_ns));
        const w = Math.max(1, right - left);
        const selected = profileState.selected === mark;
        g.globalAlpha = mark.count > 1 ? 0.7 : 1;
        g.fillRect(left, top + 6, w, PROFILE_LANE_HEIGHT - 12);
        if (selected) {
          g.strokeStyle = "#f8fafc";
          g.strokeRect(left, top + 6, w, PROFILE_LANE_HEIGHT - 12);
        }
        g.globalAlpha = 1;
        if (w > 40) {
          g.save();
          g.fillStyle = "#0a1018";
          g.beginPath();
          g.rect(left, top, w, PROFILE_LANE_HEIGHT);
          g.clip();
          g.fillText(mark.label + (mark.count > 1 ? " ×" + mark.count : ""), left + 3, top + PROFILE_LANE_HEIGHT / 2 + 4);
          g.restore();
          g.fillStyle = PROFILE_LANE_COLORS[lane.name] || "#94a3b8";
        }
        profileState.hits.push({ left, right: left + w, top, bottom: top + PROFILE_LANE_HEIGHT, mark, lane: lane.name });
      }
    });
    if (profileState.dragFrom !== null && profileState.dragTo !== null) {
      g.fillStyle = "rgba(96,165,250,.18)";
      const left = Math.min(profileState.dragFrom, profileState.dragTo);
      g.fillRect(left, 0, Math.abs(profileState.dragTo - profileState.dragFrom), height);
    }
    debugViewFill("profile-hot-rows", (doc.hot_lines || []).map((line) => debugViewRow([[line, ""]])), "No CPU samples in this trace");
    const selected = profileState.selected;
    debugViewFill("profile-detail-rows", selected ? [
      debugViewRow([["label", "key"], [selected.label, ""]]),
      debugViewRow([["start", "key"], [profileViewNs(selected.start_ns), ""]]),
      debugViewRow([["length", "key"], [profileViewNs(selected.end_ns - selected.start_ns), ""]]),
      debugViewRow([["events", "key"], [String(selected.count), ""]])
    ] : [], "Drag across the timeline to zoom; click a mark to see it");
  }

  // The trace time under canvas x `px`.
  function profileViewTime(px) {
    const doc = profileState.doc;
    const timeline = debugViewElement("profile-timeline");
    if (!doc || !timeline) return 0;
    const plot = timeline.width - PROFILE_LABEL_WIDTH;
    const from = doc.window.from_ns;
    const to = Math.max(doc.window.to_ns, from + 1);
    return from + Math.max(0, Math.min(plot, px - PROFILE_LABEL_WIDTH)) / plot * (to - from);
  }

  function profileViewZoom(from, to) {
    const doc = profileState.doc;
    if (!doc || to - from < 1) return;
    profileState.zoom.push([doc.window.from_ns, doc.window.to_ns]);
    requestProfile(profileState.trace, from, to);
  }

  function bindDebugProfileViews() {
    const bind = (id, handler) => {
      const element = debugViewElement(id);
      if (element) element.addEventListener("click", handler);
    };
    bind("debug-view-run", () => {
      if (debugSessionId) stopDebug();
      runDebug([]);
    });
    bind("debug-view-continue", () => debugViewCommand(["continue"]));
    bind("debug-view-next", () => debugViewCommand(["next"]));
    bind("debug-view-step", () => debugViewCommand(["step"]));
    bind("debug-view-finish", () => debugViewCommand(["finish"]));
    bind("debug-view-stop", () => stopDebug());
    bind("debug-view-reverse", () => debugViewCommand(["reverse-next"]));
    const consoleInput = debugViewElement("debug-console-input");
    if (consoleInput) consoleInput.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter" || !consoleInput.value.trim()) return;
      const command = consoleInput.value.trim();
      consoleInput.value = "";
      if (debugSessionId) runDebug([command]);
      else runDebug([]);
    });
    const watchInput = debugViewElement("debug-watch-input");
    if (watchInput) watchInput.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter" || !watchInput.value.trim()) return;
      addWatch(watchInput.value.trim());
      watchInput.value = "";
      if (debugSessionId) runDebug([]);
      else renderDebugView();
    });
    document.addEventListener("keydown", (ev) => {
      if (viewMode !== "debug" || (ev.target && ev.target.tagName === "INPUT")) return;
      const commands = { F5: debugSessionId ? ["continue"] : [], F10: ["next"], F11: ev.shiftKey ? ["finish"] : ["step"] };
      if (!(ev.key in commands)) return;
      ev.preventDefault();
      if (ev.key === "F5" && !debugSessionId) runDebug([]);
      else debugViewCommand(commands[ev.key]);
    });
    bind("profile-view-run", () => {
      profileState.zoom = [];
      requestProfile("", null, null);
    });
    bind("profile-view-open", () => {
      const input = debugViewElement("profile-view-trace");
      const path = input ? input.value.trim() : "";
      if (!path) return;
      profileState.zoom = [];
      requestProfile(path, null, null);
    });
    bind("profile-view-zoom-out", () => {
      const back = profileState.zoom.pop();
      if (back) requestProfile(profileState.trace, back[0], back[1]);
    });
    bind("profile-view-reset", () => {
      if (!profileState.trace) return;
      profileState.zoom = [];
      requestProfile(profileState.trace, null, null);
    });
    const timeline = debugViewElement("profile-timeline");
    if (!timeline) return;
    const local = (ev) => {
      const box = timeline.getBoundingClientRect();
      return { x: (ev.clientX - box.left) * (timeline.width / Math.max(1, box.width)), y: (ev.clientY - box.top) * (timeline.height / Math.max(1, box.height)) };
    };
    timeline.addEventListener("mousedown", (ev) => {
      profileState.dragFrom = local(ev).x;
      profileState.dragTo = null;
    });
    timeline.addEventListener("mousemove", (ev) => {
      if (profileState.dragFrom === null) return;
      profileState.dragTo = local(ev).x;
      renderProfileView();
    });
    timeline.addEventListener("mouseup", (ev) => {
      const at = local(ev);
      const from = profileState.dragFrom;
      profileState.dragFrom = null;
      profileState.dragTo = null;
      if (from !== null && Math.abs(at.x - from) > 4) {
        profileViewZoom(profileViewTime(Math.min(from, at.x)), profileViewTime(Math.max(from, at.x)));
        return;
      }
      const hit = (profileState.hits || []).find((h) => at.x >= h.left && at.x <= h.right && at.y >= h.top && at.y <= h.bottom);
      profileState.selected = hit ? hit.mark : null;
      renderProfileView();
    });
    timeline.addEventListener("wheel", (ev) => {
      const doc = profileState.doc;
      if (!doc) return;
      ev.preventDefault();
      const centre = profileViewTime(local(ev).x);
      const span = doc.window.to_ns - doc.window.from_ns;
      const scale = ev.deltaY < 0 ? 0.5 : 2;
      const next = Math.max(1, span * scale);
      const from = Math.max(doc.window.first_ns, centre - (centre - doc.window.from_ns) * scale);
      profileViewZoom(from, Math.min(Math.max(doc.window.last_ns, from + 1), from + next));
    }, { passive: false });
    window.addEventListener("resize", () => renderProfileView());
  }

  bindDebugProfileViews();

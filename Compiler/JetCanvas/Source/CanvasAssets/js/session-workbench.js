// Resident session identity, output launcher, embedded preview, and server rail.
  let canvasSession = null;
  let canvasRunSelection = { output: "", target: "" };
  const CANVAS_SESSION_LEASE_INTERVAL_MS = 80;
  let canvasSessionLeaseTimer = null;
  let canvasSessionLeaseInFlight = false;
  let canvasSessionLeaseActive = true;

  function scheduleCanvasSessionLease() {
    if (!canvasSessionLeaseActive || !canvasSession || canvasSessionLeaseTimer !== null) return;
    canvasSessionLeaseTimer = window.setTimeout(() => {
      canvasSessionLeaseTimer = null;
      if (!canvasSessionLeaseActive) return;
      if (canvasSessionLeaseInFlight) {
        scheduleCanvasSessionLease();
        return;
      }
      canvasSessionLeaseInFlight = true;
      loadCanvasSession().finally(() => {
        canvasSessionLeaseInFlight = false;
        scheduleCanvasSessionLease();
      });
    }, CANVAS_SESSION_LEASE_INTERVAL_MS);
  }

  function stopCanvasSessionLease() {
    canvasSessionLeaseActive = false;
    if (canvasSessionLeaseTimer !== null) {
      window.clearTimeout(canvasSessionLeaseTimer);
      canvasSessionLeaseTimer = null;
    }
  }

  if (typeof window.addEventListener === "function") {
    window.addEventListener("pagehide", stopCanvasSessionLease);
    window.addEventListener("pageshow", () => {
      canvasSessionLeaseActive = true;
      scheduleCanvasSessionLease();
    });
  }

  function workbenchListener(session) {
    const listeners = session && session.listeners;
    return listeners && (listeners.application || listeners.canvas) || {};
  }

  function workbenchPort(session) {
    const port = Number(workbenchListener(session).port);
    return Number.isFinite(port) && port > 0 ? port : 0;
  }

  function applicationListener(session) {
    const listeners = session && session.listeners;
    return listeners && listeners.application || {};
  }

  function listenerPort(listener) {
    const port = Number(listener && listener.port);
    return Number.isFinite(port) && port > 0 ? port : 0;
  }

  function listenerEndpoint(listener) {
    const port = listenerPort(listener);
    return port ? `${listener.host || location.hostname}:${port}` : "port pending";
  }

  function applicationPort(session) {
    return listenerPort(applicationListener(session));
  }

  function applicationPreviewUrl(session) {
    const listener = applicationListener(session);
    const port = applicationPort(session);
    if (!port) return "";
    const url = new URL("/", window.location.href);
    if (listener.host) url.hostname = listener.host;
    url.port = String(port);
    return url.href;
  }

  function syncWorkbenchContext(project = latestProject, session = canvasSession) {
    const packageName = project && (project.packages || []).find((pkg) => pkg && pkg.name)?.name;
    const projectName = packageName || (project && project.entry) || (project && project.mode) || "Project";
    const selectedOutput = canvasRunSelection.output || session && session.run && session.run.output;
    const rows = outputRows(project);
    const firstOutput = rows[0] && (rows[0].name || rows[0].target || rows[0].output || rows[0].kind);
    const outputName = selectedOutput || firstOutput || "default";
    const acceptedRevision = session && (session.accepted_revision || session.source_revision);
    const revisionName = acceptedRevision || (latestDoc && latestDoc.revision) || "pending";
    const listener = workbenchListener(session);
    const port = workbenchPort(session);
    const values = {
      "workbench-project": projectName,
      "workbench-output": outputName,
      "workbench-revision": String(revisionName).slice(0, 12),
      "workbench-port": port ? `${listener.host || location.hostname}:${port}` : "pending"
    };
    for (const [id, value] of Object.entries(values)) {
      const element = document.getElementById(id);
      if (!element) continue;
      element.textContent = value;
      element.title = String(value);
    }
  }

  function canvasCapability(name) {
    const capabilities = window.__jetCanvasCapabilities || {};
    return capabilities[name] === true;
  }

  function canvasSurfaceSupported(element) {
    const capability = element.getAttribute("data-capability");
    return (!capability || canvasCapability(capability)) && !element.hidden;
  }

  function syncCanvasLayout(project) {
    if (!project || !project.capabilities || !editorState) return;
    const panels = Array.from(document.querySelectorAll("[data-canvas-panel]"))
      .filter(canvasSurfaceSupported)
      .map((panel) => panel.getAttribute("data-canvas-panel"))
      .filter(Boolean);
    const views = Array.from(document.querySelectorAll("[data-session-view]"))
      .filter(canvasSurfaceSupported)
      .map((view) => view.getAttribute("data-session-view"))
      .filter(Boolean);
    const saved = editorState.layout && typeof editorState.layout === "object"
      ? editorState.layout
      : {};
    const retainSupported = (values, available) => {
      const ordered = [];
      for (const value of Array.isArray(values) ? values : []) {
        if (available.includes(value) && !ordered.includes(value)) ordered.push(value);
      }
      for (const value of available) {
        if (!ordered.includes(value)) ordered.push(value);
      }
      return ordered;
    };
    const layout = {
      panels: retainSupported(saved.panels, panels),
      views: retainSupported(saved.views, views)
    };
    const changed = JSON.stringify(saved) !== JSON.stringify(layout);
    editorState.layout = layout;
    window.__jetCanvasLayout = layout;
    if (changed) saveEditorState();
  }

  function syncCanvasCapabilities(project) {
    const ready = !!(project && project.capabilities && typeof project.capabilities === "object");
    const capabilities = ready ? project.capabilities : {};
    document.querySelectorAll("[data-capability]").forEach((panel) => {
      const capability = panel.getAttribute("data-capability");
      const supported = ready && capabilities[capability] === true;
      if (!supported && panel.contains(document.activeElement) && canvas) canvas.focus();
      panel.hidden = !supported;
      panel.inert = !supported;
      if (supported) panel.removeAttribute("aria-hidden");
      else panel.setAttribute("aria-hidden", "true");
    });
    window.__jetCanvasCapabilities = capabilities;
    if (ready) {
      syncCanvasLayout(project);
      if (typeof renderTour === "function") renderTour();
    }
  }

  function canvasClientId() {
    let id = "";
    try { id = sessionStorage.getItem("jet-dev-client") || ""; } catch (_) {}
    if (!id) {
      id = (self.crypto && crypto.randomUUID) ? crypto.randomUUID() : String(Date.now()) + "-canvas";
      try { sessionStorage.setItem("jet-dev-client", id); } catch (_) {}
    }
    return id;
  }

  function canvasSessionPayload(value) {
    if (value && value.session && typeof value.session === "object") return value.session;
    return value && value.schema === "jet.report/v1" && value.canvas && value.canvas.session
      ? value.canvas.session
      : null;
  }

  function syncCanvasServers(project = latestProject, session = canvasSession) {
    const list = document.getElementById("server-list");
    const count = document.getElementById("server-count");
    if (!list) return;
    const listeners = session && session.listeners || {};
    const state = session && session.state || "starting";
    const rows = [];
    if (listenerPort(listeners.canvas)) {
      rows.push({
        name: "Canvas",
        port: listenerEndpoint(listeners.canvas),
        detail: `${state} · Canvas control · diagnostics · one session`,
        state
      });
    }
    if (listenerPort(listeners.application)) {
      rows.push({
        name: "App Preview",
        port: listenerEndpoint(listeners.application),
        detail: `${state} · application preview · application-owned routes · one session`,
        state
      });
    }
    if (!rows.length) {
      rows.push({
        name: "Resident session",
        port: "port pending",
        detail: `${state} · resident session`,
        state
      });
    }
    for (const service of (project && project.services) || []) {
      const ports = Array.isArray(service.ports) ? service.ports.join(", ") : "";
      const run = Array.isArray(service.run) ? service.run.join(" ") : service.run || "catalog/default";
      rows.push({
        name: service.name || "Custom server",
        port: ports || "port assigned by service",
        detail: `${service.enable === false ? "disabled" : "enabled"} · external process · ${run}${service.ready ? ` · ready: ${service.ready}` : ""}`,
        state: service.enable === false ? "disabled" : "external"
      });
    }
    list.replaceChildren();
    for (const row of rows) {
      const card = document.createElement("article");
      card.className = "server-card";
      card.dataset.state = row.state;
      if (session && session.id) card.dataset.sessionId = session.id;
      const head = document.createElement("div");
      head.className = "server-card-head";
      const title = document.createElement("b");
      title.textContent = row.name;
      const endpoint = document.createElement("code");
      endpoint.textContent = row.port;
      head.append(title, endpoint);
      const detail = document.createElement("small");
      detail.textContent = row.detail;
      card.append(head, detail);
      list.appendChild(card);
    }
    if (count) count.textContent = String(rows.length);
    window.__jetCanvasServers = rows.map((row) => ({ ...row, sessionId: session && session.id || null }));
  }

  function syncCanvasSession(session) {
    if (!session || !session.id) return;
    const previousSessionId = canvasSession && canvasSession.id;
    canvasSession = session;
    if (previousSessionId && previousSessionId !== session.id) {
      canvasRunSelection = { output: "", target: "" };
    }
    if (!canvasRunSelection.output && session.run && session.run.output) {
      canvasRunSelection.output = String(session.run.output);
    }
    if (!canvasRunSelection.target && session.run && session.run.target) {
      canvasRunSelection.target = String(session.run.target);
    }
    const projectContext = session.project_context || {};
    if (projectContext.source_id) selectedSourceId = projectContext.source_id;
    const shortId = String(session.id).slice(-18);
    const currentRevision = session.source_revision || session.accepted_revision || "uncommitted";
    const identity = document.getElementById("session-identity");
    const footerSession = document.getElementById("session-id");
    if (identity) {
      identity.textContent = `${shortId} · ${session.state || "starting"} · ${session.clients || 0} client${session.clients === 1 ? "" : "s"}`;
      identity.title = session.id;
      identity.dataset.sessionId = session.id;
      identity.dataset.sourceRevision = currentRevision;
      const sessionCard = identity.closest(".workbench-session");
      if (sessionCard) sessionCard.dataset.sessionState = session.state || "starting";
    }
    if (footerSession) {
      footerSession.textContent = `Session ${shortId}`;
      footerSession.title = session.id;
    }
    document.querySelectorAll("[data-session-view]").forEach((view) => {
      const name = view.getAttribute("data-session-view") || "view";
      const displayName = { "custom servers": "Custom Servers" }[name] || name;
      view.textContent = `${displayName} · ${shortId} · ${String(currentRevision).slice(-12)}`;
      view.title = `${displayName} · ${session.id} · ${currentRevision}`;
      view.dataset.sessionId = session.id;
      view.dataset.sourceRevision = currentRevision;
      view.dataset.sessionState = session.state || "starting";
    });
    const preview = document.getElementById("preview-link");
    const listener = applicationListener(session);
    const port = applicationPort(session);
    const previewUrl = applicationPreviewUrl(session);
    if (preview) {
      preview.href = previewUrl || "/";
      preview.textContent = previewUrl
        ? `Open App Preview · ${listener.host || "localhost"}:${port}`
        : "Preview is starting";
    }
    const previewFrame = document.getElementById("preview-frame");
    if (previewFrame) {
      previewFrame.title = `App Preview · ${session.state || "starting"}`;
      previewFrame.dataset.sessionId = session.id;
      previewFrame.dataset.sessionState = session.state || "starting";
      if (previewUrl && previewFrame.dataset.previewUrl !== previewUrl) {
        previewFrame.src = previewUrl;
        previewFrame.dataset.previewUrl = previewUrl;
      } else if (!previewUrl && previewFrame.dataset.previewUrl) {
        previewFrame.src = "about:blank";
        delete previewFrame.dataset.previewUrl;
      }
    }
    const previewState = document.getElementById("preview-status");
    if (previewState) {
      const good = session.last_good_program || "none";
      previewState.textContent = `${session.state || "starting"} · last good ${good}`;
      previewState.dataset.state = session.state || "starting";
    }
    window.__jetCanvasSession = {
      id: session.id,
      sourceRevision: currentRevision,
      acceptedRevision: session.accepted_revision || null,
      lastGoodRevision: session.last_good_revision || null,
      lastGoodProgram: session.last_good_program || null,
      lastGoodViews: session.last_good_views || {},
      projectContext,
      run: session.run || { output: null, target: null },
      debugger: session.debugger || { state: "idle" },
      state: session.state || "starting",
      clients: session.clients || 0,
      history: session.history || { count: 0, receipts: [] },
      listeners: session.listeners || {},
      preview: {
        url: previewUrl || null,
        host: listener.host || null,
        port: port || null,
        sameOrigin: !!previewUrl && new URL(previewUrl, window.location.href).origin === window.location.origin
      }
    };
    syncWorkbenchContext(latestProject, session);
    syncCanvasCapabilities(latestProject);
    syncCanvasOutputs(latestProject);
    syncCanvasServers(latestProject, session);
  }

  function canvasSessionPayloadFromReport(value) {
    const payload = value && value.schema === "jet.report/v1" && value.canvas ? value.canvas : value;
    return payload && payload.session && typeof payload.session === "object" ? payload.session : null;
  }

  function canvasSessionUrl() {
    return (window.__JET_CANVAS_BASE__ || "/canvas") + "/session";
  }

  function loadCanvasSession() {
    return fetch(canvasSessionUrl() + "?client_id=" + encodeURIComponent(canvasClientId()), { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw new Error("session request failed (" + response.status + ")");
        return response.json();
      })
      .then((value) => {
        const session = canvasSessionPayload(value) || canvasSessionPayloadFromReport(value);
        if (!session) throw new Error("session response has no resident session");
        syncCanvasSession(session);
        scheduleCanvasSessionLease();
        return session;
      })
      .catch(() => canvasSession);
  }

  function outputRows(project) {
    if (!project) return [];
    if (Array.isArray(project.outputs) && project.outputs.length) return project.outputs;
    const rows = [];
    for (const pkg of project.packages || []) {
      for (const output of pkg.outputs || []) rows.push(output);
    }
    if (rows.length) return rows;
    return project.targets || [];
  }

  function targetRows(project) {
    if (!project) return [];
    const rows = [];
    const add = (name, detail) => {
      if (name) rows.push({ name: String(name), detail: detail || "" });
    };
    const selected = canvasRunSelection.target || canvasSession && canvasSession.run && canvasSession.run.target;
    if (selected) add(selected, "selected run target");
    for (const target of project.targets || []) {
      if (!target) continue;
      add(
        target.name || target.target,
        [target.kind, target.profile].filter(Boolean).join(" · ")
      );
    }
    for (const pkg of project.packages || []) {
      for (const target of pkg && pkg.targets || []) {
        if (!target) continue;
        add(
          target.name || target.target,
          `${pkg.name || "package"} profile${target.profile ? ` · ${target.profile}` : ""}`
        );
      }
    }
    const seen = new Set();
    return rows.filter((row) => {
      if (!row.name || seen.has(row.name)) return false;
      seen.add(row.name);
      return true;
    });
  }

  function canvasChoiceCard(row, kind, selected) {
    const name = String(row.name || row.target || row.output || row.kind || kind);
    const card = document.createElement("button");
    card.type = "button";
    card.className = `project-card ${kind}-card` + (name === selected ? " is-active" : "");
    card.setAttribute("role", "option");
    card.setAttribute("aria-selected", name === selected ? "true" : "false");
    card.dataset[kind === "output" ? "canvasOutput" : "canvasTarget"] = name;
    card.addEventListener("click", () => {
      if (kind === "output") selectCanvasOutput(name);
      else selectCanvasTarget(name);
    });
    const title = document.createElement("b");
    title.textContent = name;
    const detail = document.createElement("small");
    detail.textContent = [row.kind, row.entry || row.path, row.detail, row.provenance]
      .filter(Boolean)
      .join(" · ") || `valid ${kind}`;
    card.append(title, detail);
    return card;
  }

  function syncCanvasOutputs(project) {
    const list = document.getElementById("output-list");
    const targetList = document.getElementById("target-list");
    const count = document.getElementById("output-count");
    const targetCount = document.getElementById("target-count");
    if (!list || !targetList) return;
    const rows = outputRows(project);
    const targets = targetRows(project);
    syncWorkbenchContext(project, canvasSession);
    const outputPanel = document.getElementById("output-panel");
    if (outputPanel) {
      if (!rows.length && !targets.length && outputPanel.contains(document.activeElement) && canvas) canvas.focus();
      outputPanel.hidden = rows.length === 0 && targets.length === 0;
    }
    list.replaceChildren();
    targetList.replaceChildren();
    if (count) count.textContent = String(rows.length);
    if (targetCount) targetCount.textContent = String(targets.length);
    if (!rows.length && !targets.length) {
      const empty = document.createElement("span");
      empty.className = "tag";
      empty.textContent = "No outputs or targets discovered yet";
      list.appendChild(empty);
      window.__jetCanvasOutputs = [];
      window.__jetCanvasTargets = [];
      syncCanvasLayout(project);
      return;
    }
    const selectedOutput = canvasSelectedOutput();
    for (const row of rows) {
      list.appendChild(canvasChoiceCard(row, "output", selectedOutput));
    }
    if (!rows.length) {
      const empty = document.createElement("span");
      empty.className = "tag";
      empty.textContent = "No outputs discovered yet";
      list.appendChild(empty);
    }
    const selectedTarget = canvasSelectedTarget();
    for (const row of targets) {
      targetList.appendChild(canvasChoiceCard(row, "target", selectedTarget));
    }
    if (!targets.length) {
      const empty = document.createElement("span");
      empty.className = "tag";
      empty.textContent = "No targets discovered yet";
      targetList.appendChild(empty);
    }
    window.__jetCanvasOutputs = rows.map((row) => ({
      name: row.name || row.target || row.output || row.kind || "output",
      kind: row.kind || "",
      entry: row.entry || row.path || ""
    }));
    window.__jetCanvasTargets = targets.map((row) => ({
      name: row.name,
      detail: row.detail || ""
    }));
    syncCanvasLayout(project);
  }

  function selectCanvasSelection(kind, value) {
    if (value) canvasRunSelection[kind] = String(value);
    syncCanvasOutputs(latestProject);
    syncWorkbenchContext(latestProject, canvasSession);
    return Promise.resolve(canvasSession);
  }

  function selectCanvasOutput(output) {
    return selectCanvasSelection("output", output);
  }

  function selectCanvasTarget(target) {
    return selectCanvasSelection("target", target);
  }

  function canvasSelectedOutput() {
    return canvasRunSelection.output || canvasSession && canvasSession.run && canvasSession.run.output || "";
  }

  function canvasSelectedTarget() {
    return canvasRunSelection.target || canvasSession && canvasSession.run && canvasSession.run.target || "";
  }

  window.__jetCanvasSessionApi = {
    clientId: canvasClientId,
    load: loadCanvasSession,
    selectOutput: selectCanvasOutput,
    selectTarget: selectCanvasTarget,
    selectedOutput: canvasSelectedOutput,
    selectedTarget: canvasSelectedTarget
  };
  // D-DX-DEVTOOLS-UX1=D / card #2429: Canvas renders the canonical
  // BrowserWebAdapter panel/view projection published by Session and the
  // first-party DevtoolsPanelCatalog (crates/jet-codegen/src/Prelude/Core/
  // DevtoolsPanelCatalog.rs). Panel identity, order, availability, and
  // event-to-panel membership are decided once, server-side; this layer
  // only displays the published panels/nodes and reflects selection back.
  const JET_DEVTOOLS_PROTOCOL = "jet.devtools.v1";
  // BrowserSessionState is a closed six-value UX state, not a panel
  // catalog. `token` matches the frozen pill/lens CSS in
  // crates/jet-canvas/src/html.rs (which still spells these with
  // underscores); `label` is the human-readable text.
  const DEVTOOLS_STATES = {
    idle: { token: "idle", label: "Idle" },
    building: { token: "building", label: "Building" },
    "build-failed": { token: "build_failed", label: "Build failed" },
    "runtime-failure": { token: "runtime_failure", label: "Runtime failure" },
    "hot-reload": { token: "hot_reload", label: "Hot reload" },
    "tests-failing": { token: "tests_failing", label: "Tests failing" }
  };
  let devtoolsFrame = null;
  let devtoolsSelection = null;
  let devtoolsCursor = null;
  let devtoolsOrigin = "browser";
  let devtoolsPollTimer = null;
  let devtoolsPollInFlight = false;
  let devtoolsChannel = null;
  let devtoolsLensReturnFocus = null;
  let devtoolsWorkbenchReturnFocus = null;

  function devtoolsUrl(suffix = "") {
    const configured = String(window.__JET_DEVTOOLS_URL__ || "").trim();
    const base = configured || "/__jet_devtools";
    if (!suffix) return base;
    return base.replace(/\/$/, "") + "/" + String(suffix).replace(/^\//, "");
  }

  function devtoolsObject(value) {
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  }

  // Accept only the already-projected panel/view stream published by
  // BrowserWebAdapter::view_json (still JSON at the `/__jet_devtools`
  // transport boundary). No raw event envelope is decoded here.
  function devtoolsFrameFrom(value) {
    const payload = devtoolsObject(value);
    if (!payload || payload.protocol !== JET_DEVTOOLS_PROTOCOL) return null;
    if (!Array.isArray(payload.panels)) return null;
    if (payload.selection !== null && payload.selection !== undefined && !devtoolsObject(payload.selection)) return null;
    return payload;
  }

  function devtoolsPanels() {
    return devtoolsFrame && Array.isArray(devtoolsFrame.panels) ? devtoolsFrame.panels : [];
  }

  // Resolve a panel by its canonical id or its canonical title. Static pill
  // markup (crates/jet-canvas/src/html.rs) still spells a few panels by
  // title; id and title are both canonical fields already on the panel, so
  // this never invents panel identity or a fallback panel list.
  function devtoolsPanel(idOrTitle) {
    const key = String(idOrTitle || "");
    if (!key) return null;
    return devtoolsPanels().find((panel) => panel.panel_id === key || panel.title === key) || null;
  }

  function devtoolsResolvePanelId(idOrTitle) {
    const panel = devtoolsPanel(idOrTitle);
    if (panel) return panel.panel_id;
    return String(idOrTitle || (devtoolsPanels()[0] || {}).panel_id || "");
  }

  function devtoolsPanelNodes(panel) {
    return panel && Array.isArray(panel.nodes) ? panel.nodes : [];
  }

  // A panel always publishes at least one node: recorded facts ("tree"
  // nodes) or a single synthetic "availability" status node explaining why
  // it is empty or unavailable. Only "tree" nodes are recorded facts.
  function devtoolsPanelFactNodes(panel) {
    return devtoolsPanelNodes(panel).filter((node) => node.kind === "tree");
  }

  function devtoolsPanelIsUnavailable(panel) {
    return !!(panel && typeof panel.status === "string" && panel.status.startsWith("unavailable:"));
  }

  function devtoolsNodeLabel(node, fallback = "event") {
    return String(node && node.label || fallback);
  }

  // Each recorded fact publishes its source/kind/entity/fields/payload as
  // labeled children (see BrowserWebAdapter::panel_from_projection); read
  // them by label, never by position or by re-deriving meaning from raw
  // event JSON.
  function devtoolsNodeField(node, label) {
    const children = node && Array.isArray(node.children) ? node.children : [];
    const child = children.find((entry) => entry.label === label);
    return child && child.value !== null && child.value !== undefined ? String(child.value) : "";
  }

  function devtoolsNodeSummary(node) {
    if (!node) return "No recorded fact";
    if (node.kind !== "tree") return node.status || "No recorded fact";
    const values = [
      devtoolsNodeField(node, "kind"),
      devtoolsNodeField(node, "entity"),
      devtoolsNodeField(node, "source")
    ].filter(Boolean);
    return values.join(" · ") || devtoolsNodeLabel(node);
  }

  function devtoolsSelectionRecord(panelId, itemKey = null) {
    return {
      panel_id: String(panelId || (devtoolsPanels()[0] || {}).panel_id || ""),
      item_key: itemKey === null || itemKey === undefined || itemKey === "" ? null : String(itemKey)
    };
  }

  function devtoolsSelectionPanel(selection) {
    return String(selection && selection.panel_id || "");
  }

  function devtoolsSelectionItemKey(selection) {
    const value = selection && selection.item_key;
    return value === null || value === undefined ? "" : String(value);
  }

  function devtoolsSelectedPanel() {
    const panels = devtoolsPanels();
    if (!panels.length) return null;
    return devtoolsPanel(devtoolsSelectionPanel(devtoolsSelection)) || panels[0];
  }

  function devtoolsSelectedNode() {
    const panel = devtoolsSelectedPanel();
    if (!panel) return null;
    const itemKey = devtoolsSelectionItemKey(devtoolsSelection);
    const nodes = devtoolsPanelNodes(panel);
    const requested = itemKey ? nodes.find((node) => node.id === itemKey) : null;
    if (requested) return requested;
    const facts = devtoolsPanelFactNodes(panel);
    return facts[facts.length - 1] || nodes[0] || null;
  }

  function devtoolsStateInfo() {
    const state = (devtoolsFrame && devtoolsFrame.state) || "idle";
    return DEVTOOLS_STATES[state] || { token: state, label: state };
  }

  function devtoolsSetText(id, value) {
    const element = document.getElementById(id);
    if (element) element.textContent = String(value ?? "");
    return element;
  }

  function devtoolsSetExpanded(surfaceId, open) {
    if (!surfaceId) return;
    for (const button of document.querySelectorAll(`[aria-controls="${surfaceId}"]`)) {
      button.setAttribute("aria-expanded", String(open));
    }
  }

  function devtoolsSetSurface(element, open) {
    if (!element) return;
    element.hidden = !open;
    element.inert = !open;
    element.setAttribute("aria-hidden", open ? "false" : "true");
    element.classList.toggle("is-open", open);
    devtoolsSetExpanded(element.id, open);
  }

  function devtoolsRenderPill() {
    const shell = document.getElementById("jet-devtools-shell");
    if (!shell) return;
    const info = devtoolsStateInfo();
    const status = (devtoolsFrame && devtoolsObject(devtoolsFrame.status)) || {};
    shell.dataset.state = info.token;
    const build = devtoolsPanel("build");
    devtoolsSetText("jet-devtools-build", (build && build.status) || "waiting");
    const traces = devtoolsPanel("traces");
    devtoolsSetText("jet-devtools-reload", traces ? String(devtoolsPanelFactNodes(traces).length) : "—");
    devtoolsSetText("jet-devtools-errors", status.diagnostic ? "1" : "0");
    const tests = devtoolsPanel("tests");
    devtoolsSetText("jet-devtools-tests", status.test_state || (tests && tests.status) || "—");
    const queries = devtoolsPanel("queries");
    devtoolsSetText("jet-devtools-fetching", queries ? String(devtoolsPanelFactNodes(queries).length) : "0");
    const panelCount = devtoolsPanels().length;
    const statusElement = devtoolsSetText("jet-devtools-status", `${info.label} · ${panelCount} panel${panelCount === 1 ? "" : "s"}`);
    if (statusElement) statusElement.dataset.state = info.token;
  }

  function devtoolsSetCard(parent, panelId, title, body, node) {
    const card = document.createElement("article");
    card.className = "jet-devtools-card";
    card.dataset.panel = panelId;
    const heading = document.createElement("h4");
    heading.textContent = title;
    const copy = document.createElement("p");
    copy.textContent = body || "No recorded fact";
    card.append(heading, copy);
    card.tabIndex = 0;
    card.setAttribute("role", "button");
    card.setAttribute("aria-label", `${title}: ${body || "No recorded fact"}`);
    if (node) card.dataset.devtoolsNode = node.id;
    const activate = () => devtoolsSelectNode(panelId, node, "browser lens");
    card.addEventListener("click", activate);
    card.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter" || ev.key === " ") {
        ev.preventDefault();
        activate();
      }
    });
    parent.appendChild(card);
  }

  function devtoolsRenderLens() {
    const panel = devtoolsSelectedPanel();
    const node = devtoolsSelectedNode();
    devtoolsSetText("jet-devtools-lens-context", panel ? panel.title : "waiting for jet.devtools.v1");
    devtoolsSetText("jet-devtools-lens-selection", devtoolsNodeLabel(node, panel ? panel.title : "No selection"));
    devtoolsSetText("jet-devtools-lens-description", node ? devtoolsNodeSummary(node) : "Waiting for the first recorded fact.");
    const cards = document.getElementById("jet-devtools-lens-cards");
    if (cards) {
      cards.replaceChildren();
      for (const cardPanel of devtoolsPanels()) {
        const facts = devtoolsPanelFactNodes(cardPanel);
        const latest = facts[facts.length - 1] || devtoolsPanelNodes(cardPanel)[0] || null;
        devtoolsSetCard(cards, cardPanel.panel_id, cardPanel.title, devtoolsNodeSummary(latest), latest);
      }
    }
    const select = document.getElementById("jet-devtools-panel-select");
    const drawerList = document.getElementById("jet-devtools-panel-list");
    if (select && drawerList) {
      const panels = devtoolsPanels();
      const desiredIds = panels.map((entry) => entry.panel_id);
      const currentIds = Array.from(select.options).map((option) => option.value);
      if (currentIds.join("\u0000") !== desiredIds.join("\u0000")) {
        select.replaceChildren();
        for (const entry of panels) {
          const option = document.createElement("option");
          option.value = entry.panel_id;
          option.textContent = entry.title;
          select.appendChild(option);
        }
      }
      const fallbackId = (panels[0] || {}).panel_id || "";
      const requested = panel ? panel.panel_id : fallbackId;
      select.value = desiredIds.includes(requested) ? requested : fallbackId;
      const drawerPanel = devtoolsPanel(select.value);
      drawerList.replaceChildren();
      const rows = devtoolsPanelNodes(drawerPanel);
      if (!rows.length) {
        const empty = document.createElement("p");
        empty.className = "jet-devtools-empty";
        empty.textContent = "No recorded facts for this panel.";
        drawerList.appendChild(empty);
      } else {
        for (const row of rows.slice(-12).reverse()) {
          const button = document.createElement("button");
          button.type = "button";
          button.className = "jet-devtools-event-row";
          button.dataset.devtoolsNode = row.id;
          button.textContent = `${devtoolsNodeLabel(row)} · ${devtoolsNodeSummary(row)}`;
          button.addEventListener("click", () => devtoolsSelectNode(drawerPanel.panel_id, row, "browser lens"));
          drawerList.appendChild(button);
        }
      }
    }
    const range = document.getElementById("jet-devtools-lens-time");
    const max = (devtoolsFrame && Number(devtoolsFrame.sequence)) || 0;
    if (range) {
      range.max = String(Math.max(1, max));
      range.value = String(Math.min(Number(devtoolsCursor ?? max ?? 0), Number(range.max)));
    }
    devtoolsRenderCursorLabel("jet-devtools-lens-time-label");
  }

  function devtoolsRenderTree() {
    const tree = document.getElementById("jet-devtools-tree");
    if (!tree) return;
    tree.replaceChildren();
    const selectedId = (devtoolsSelectedPanel() || {}).panel_id;
    for (const panel of devtoolsPanels()) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "jet-devtools-tree-item";
      button.dataset.devtoolsPanel = panel.panel_id;
      button.setAttribute("role", "treeitem");
      button.setAttribute("aria-selected", String(panel.panel_id === selectedId));
      button.textContent = devtoolsPanelIsUnavailable(panel)
        ? `${panel.title} · unavailable`
        : `${panel.title} · ${devtoolsPanelFactNodes(panel).length}`;
      button.addEventListener("click", () => devtoolsSelectPanel(panel.panel_id, "browser workbench"));
      tree.appendChild(button);
    }
  }

  function devtoolsNodeSequenceLabel(node) {
    const match = /^event:(\d+)$/.exec(String(node && node.id || ""));
    return match ? `#${match[1]}` : "•";
  }

  function devtoolsRenderTimeline() {
    const timeline = document.getElementById("jet-devtools-timeline");
    if (!timeline) return;
    timeline.replaceChildren();
    const selected = devtoolsSelectedNode();
    let rendered = 0;
    for (const panel of devtoolsPanels()) {
      const nodes = devtoolsPanelFactNodes(panel).slice(-6).reverse();
      for (const node of nodes) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "jet-devtools-timeline-event";
        button.dataset.devtoolsPanel = panel.panel_id;
        button.dataset.devtoolsNode = node.id;
        button.setAttribute("aria-current", String(selected === node));
        const time = document.createElement("code");
        time.textContent = devtoolsNodeSequenceLabel(node);
        const label = document.createElement("span");
        label.textContent = `${panel.title} · ${devtoolsNodeLabel(node)}`;
        const detail = document.createElement("small");
        detail.textContent = devtoolsNodeSummary(node);
        button.append(time, label, detail);
        button.addEventListener("click", () => devtoolsSelectNode(panel.panel_id, node, "browser workbench"));
        timeline.appendChild(button);
        rendered += 1;
      }
    }
    if (!rendered) {
      const empty = document.createElement("p");
      empty.className = "jet-devtools-empty";
      empty.textContent = "Waiting for jet.devtools.v1 panel facts.";
      timeline.appendChild(empty);
    }
  }

  function devtoolsRenderInspector() {
    const panel = devtoolsSelectedPanel();
    const node = devtoolsSelectedNode();
    devtoolsSetText("jet-devtools-inspector-panel", panel ? panel.title : "Build");
    devtoolsSetText("jet-devtools-inspector-title", devtoolsNodeLabel(node, "No selection"));
    devtoolsSetText("jet-devtools-inspector-source", node ? devtoolsNodeField(node, "source") || "source unpublished" : "source unpublished");
    const details = document.getElementById("jet-devtools-inspector-facts");
    if (!details) return;
    details.replaceChildren();
    const facts = node && Array.isArray(node.children) ? node.children : [];
    if (!facts.length) {
      const empty = document.createElement("p");
      empty.className = "jet-devtools-empty";
      empty.textContent = node && node.status ? node.status : "No published values. The stream remains payload-free by default.";
      details.appendChild(empty);
      return;
    }
    for (const fact of facts) {
      const row = document.createElement("div");
      row.className = "jet-devtools-fact";
      const name = document.createElement("span");
      name.textContent = fact.label;
      const display = document.createElement("code");
      display.textContent = fact.value === null || fact.value === undefined ? "" : String(fact.value);
      row.append(name, display);
      details.appendChild(row);
    }
  }

  function devtoolsRenderCursorLabel(id) {
    const label = document.getElementById(id);
    if (!label) return;
    const rawSequence = devtoolsFrame ? devtoolsFrame.sequence : null;
    const sequence = rawSequence === null || rawSequence === undefined ? null : Number(rawSequence);
    const cursor = devtoolsCursor !== null && devtoolsCursor !== undefined ? Number(devtoolsCursor) : sequence;
    label.textContent = cursor === null || (sequence !== null && cursor >= sequence) ? "live" : `#${cursor}`;
  }

  function devtoolsRenderWorkbench() {
    const info = devtoolsStateInfo();
    devtoolsSetText("jet-devtools-workbench-state", `${info.label} · origin ${devtoolsOrigin}`);
    devtoolsRenderTree();
    devtoolsRenderTimeline();
    devtoolsRenderInspector();
    devtoolsRenderCursorLabel("jet-devtools-workbench-time");
    const range = document.getElementById("jet-devtools-workbench-range");
    const max = (devtoolsFrame && Number(devtoolsFrame.sequence)) || 0;
    if (range) {
      range.max = String(Math.max(1, max));
      range.value = String(Math.min(Number(devtoolsCursor ?? max ?? 0), Number(range.max)));
    }
  }

  function devtoolsRender() {
    const shell = document.getElementById("jet-devtools-shell");
    if (!shell) return;
    shell.hidden = false;
    devtoolsRenderPill();
    devtoolsRenderLens();
    devtoolsRenderWorkbench();
    window.__jetDevtoolsState = {
      protocol: JET_DEVTOOLS_PROTOCOL,
      state: (devtoolsFrame && devtoolsFrame.state) || "idle",
      panels: devtoolsPanels().map((panel) => ({ id: panel.panel_id, title: panel.title, status: panel.status })),
      selection: devtoolsSelection,
      cursor: devtoolsCursor,
      origin: devtoolsOrigin
    };
  }

  function devtoolsPublish(kind, value) {
    const message = {
      protocol: JET_DEVTOOLS_PROTOCOL,
      kind,
      value,
      session_id: devtoolsFrame && devtoolsFrame.session_id || null,
      origin: devtoolsOrigin,
      selection: devtoolsSelection,
      cursor: devtoolsCursor
    };
    if (devtoolsChannel) {
      try { devtoolsChannel.postMessage(message); } catch (_) {}
    }
    try {
      localStorage.setItem("jet.devtools.v1", JSON.stringify(Object.assign({ sent_at: Date.now() }, message)));
    } catch (_) {}
    const endpoint = kind === "selection" ? "selection" : "cursor";
    fetch(devtoolsUrl(endpoint), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(message)
    }).catch(() => {});
  }

  function devtoolsSelectNode(panelIdOrTitle, node, origin = "browser") {
    devtoolsOrigin = origin;
    devtoolsSelection = devtoolsSelectionRecord(devtoolsResolvePanelId(panelIdOrTitle), node ? node.id : null);
    devtoolsPublish("selection", devtoolsSelection);
    devtoolsRender();
  }

  function devtoolsSelectPanel(panelIdOrTitle, origin = "browser") {
    devtoolsOrigin = origin;
    devtoolsSelection = devtoolsSelectionRecord(devtoolsResolvePanelId(panelIdOrTitle), null);
    devtoolsPublish("selection", devtoolsSelection);
    devtoolsRender();
  }

  function devtoolsSetCursor(value, origin = "browser") {
    const number = Number(value);
    if (!Number.isFinite(number)) return;
    devtoolsOrigin = origin;
    devtoolsCursor = Math.max(0, Math.trunc(number));
    devtoolsPublish("cursor", devtoolsCursor);
    devtoolsRender();
  }

  function devtoolsApplyMessage(message) {
    if (!message || message.protocol !== JET_DEVTOOLS_PROTOCOL) return;
    const frameSession = devtoolsFrame && String(devtoolsFrame.session_id || "");
    const messageSession = message.session_id === null || message.session_id === undefined
      ? ""
      : String(message.session_id);
    if (frameSession && messageSession && frameSession !== messageSession) return;
    const origin = String(message.origin || "remote host");
    if (message.kind === "selection") {
      const selection = message.value;
      devtoolsSelection = selection === null
        ? null
        : devtoolsObject(selection)
          ? devtoolsSelectionRecord(devtoolsSelectionPanel(selection), devtoolsSelectionItemKey(selection))
          : devtoolsSelection;
      devtoolsOrigin = origin;
    } else if (message.kind === "cursor") {
      const value = Number(message.value);
      if (Number.isFinite(value)) devtoolsCursor = Math.max(0, Math.trunc(value));
      devtoolsOrigin = origin;
    } else {
      if (Object.prototype.hasOwnProperty.call(message, "selection")) {
        const selection = message.selection;
        devtoolsSelection = selection === null
          ? null
          : devtoolsObject(selection)
            ? devtoolsSelectionRecord(devtoolsSelectionPanel(selection), devtoolsSelectionItemKey(selection))
            : devtoolsSelection;
      }
      const value = Number(message.cursor);
      if (Number.isFinite(value)) devtoolsCursor = Math.max(0, Math.trunc(value));
      devtoolsOrigin = origin;
    }
    devtoolsRender();
  }

  // The scrub cursor set via POST /__jet_devtools/cursor is echoed on every
  // panel from the canonical projection (JetDevtoolsPanelCatalog::project);
  // every panel carries the same value, so the first present one is
  // authoritative. This is the confirmed server position, not a guess.
  function devtoolsCanonicalCursor(frame) {
    const panels = frame && Array.isArray(frame.panels) ? frame.panels : [];
    for (const panel of panels) {
      if (panel.cursor !== null && panel.cursor !== undefined && Number.isFinite(Number(panel.cursor))) {
        return Number(panel.cursor);
      }
    }
    return null;
  }

  function devtoolsApplyFrame(frame) {
    if (!frame) return false;
    const previousSession = devtoolsFrame && String(devtoolsFrame.session_id || "");
    const nextSession = String(frame.session_id || "");
    devtoolsFrame = frame;
    if (previousSession && nextSession && previousSession !== nextSession) {
      devtoolsSelection = null;
    }
    if (Object.prototype.hasOwnProperty.call(frame, "selection")) {
      const selection = frame.selection;
      devtoolsSelection = selection === null
        ? null
        : devtoolsObject(selection)
          ? devtoolsSelectionRecord(selection.panel_id, selection.node_id)
          : devtoolsSelection;
    }
    const panelCursor = devtoolsCanonicalCursor(frame);
    if (panelCursor !== null) {
      devtoolsCursor = panelCursor;
    } else {
      const sequence = frame.sequence;
      devtoolsCursor = sequence === null || sequence === undefined ? devtoolsCursor : Number(sequence);
    }
    devtoolsRender();
    return true;
  }

  function devtoolsPoll() {
    if (devtoolsPollInFlight) return;
    devtoolsPollInFlight = true;
    fetch(devtoolsUrl(), { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw new Error(`devtools request failed (${response.status})`);
        return response.json();
      })
      .then((value) => {
        const frame = devtoolsFrameFrom(value);
        if (frame) devtoolsApplyFrame(frame);
        else if (!devtoolsFrame) {
          const shell = document.getElementById("jet-devtools-shell");
          if (shell) shell.hidden = true;
        }
      })
      .catch(() => {
        if (!devtoolsFrame) {
          const shell = document.getElementById("jet-devtools-shell");
          if (shell) shell.hidden = true;
        }
      })
      .finally(() => {
        devtoolsPollInFlight = false;
        devtoolsPollTimer = window.setTimeout(devtoolsPoll, 350);
      });
  }

  function devtoolsOpenLens() {
    const lens = document.getElementById("jet-devtools-lens");
    if (!lens) return;
    const workbench = document.getElementById("jet-devtools-workbench");
    if (workbench && !workbench.hidden) devtoolsCloseWorkbench();
    devtoolsLensReturnFocus = document.activeElement;
    devtoolsSetSurface(lens, true);
    const close = document.getElementById("jet-devtools-lens-close");
    if (close) close.focus({ preventScroll: true });
  }

  function devtoolsCloseLens() {
    const lens = document.getElementById("jet-devtools-lens");
    if (!lens) return;
    devtoolsSetSurface(lens, false);
    if (devtoolsLensReturnFocus && document.contains(devtoolsLensReturnFocus)) {
      devtoolsLensReturnFocus.focus({ preventScroll: true });
    }
    devtoolsLensReturnFocus = null;
  }

  function devtoolsOpenWorkbench() {
    const workbench = document.getElementById("jet-devtools-workbench");
    if (!workbench) return;
    const lens = document.getElementById("jet-devtools-lens");
    if (lens && !lens.hidden) devtoolsCloseLens();
    devtoolsWorkbenchReturnFocus = document.activeElement;
    devtoolsSetSurface(workbench, true);
    const first = workbench.querySelector("button, input, [tabindex]");
    if (first) first.focus({ preventScroll: true });
  }

  function devtoolsCloseWorkbench() {
    const workbench = document.getElementById("jet-devtools-workbench");
    if (!workbench) return;
    devtoolsSetSurface(workbench, false);
    if (devtoolsWorkbenchReturnFocus && document.contains(devtoolsWorkbenchReturnFocus)) {
      devtoolsWorkbenchReturnFocus.focus({ preventScroll: true });
    }
    devtoolsWorkbenchReturnFocus = null;
  }

  function devtoolsCopySelectedNode() {
    const node = devtoolsSelectedNode();
    const panel = devtoolsSelectedPanel();
    const payload = node || panel;
    if (!payload) return;
    const text = JSON.stringify(payload, null, 2);
    if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
      navigator.clipboard.writeText(text).catch(() => {});
    }
    const toast = document.getElementById("toast");
    if (toast) {
      toast.textContent = "Copied the selected jet.devtools.v1 fact";
      window.setTimeout(() => { if (toast.textContent.includes("Copied the selected")) toast.textContent = ""; }, 1800);
    }
  }

  function devtoolsBindControls() {
    const shell = document.getElementById("jet-devtools-shell");
    if (!shell || shell.dataset.bound === "true") return;
    shell.dataset.bound = "true";
    const lens = document.getElementById("jet-devtools-open-lens");
    const workbench = document.getElementById("jet-devtools-open-workbench");
    const lensClose = document.getElementById("jet-devtools-lens-close");
    const workbenchClose = document.getElementById("jet-devtools-workbench-close");
    const copy = document.getElementById("jet-devtools-copy");
    if (lens) lens.addEventListener("click", devtoolsOpenLens);
    if (workbench) workbench.addEventListener("click", devtoolsOpenWorkbench);
    if (lensClose) lensClose.addEventListener("click", devtoolsCloseLens);
    if (workbenchClose) workbenchClose.addEventListener("click", devtoolsCloseWorkbench);
    if (copy) copy.addEventListener("click", devtoolsCopySelectedNode);
    for (const button of shell.querySelectorAll("[data-devtools-panel]")) {
      button.addEventListener("click", () => {
        devtoolsSelectPanel(button.dataset.devtoolsPanel, "browser pill");
        devtoolsOpenLens();
      });
    }
    if (panelSelect) panelSelect.addEventListener("change", () => {
      devtoolsSelectPanel(panelSelect.value, "browser lens");
    });
    for (const id of ["jet-devtools-lens-time", "jet-devtools-workbench-range"]) {
      const range = document.getElementById(id);
      if (range) range.addEventListener("input", () => devtoolsSetCursor(range.value));
    }
    window.addEventListener("keydown", (event) => {
      const key = String(event.key || "").toLowerCase();
      const target = event.target;
      const editing = target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
      if (!editing && event.altKey && key === "j") {
        event.preventDefault();
        const lensElement = document.getElementById("jet-devtools-lens");
        if (lensElement && !lensElement.hidden) devtoolsCloseLens();
        else devtoolsOpenLens();
      } else if (!editing && event.ctrlKey && event.shiftKey && key === "w") {
        event.preventDefault();
        const workbenchElement = document.getElementById("jet-devtools-workbench");
        if (workbenchElement && !workbenchElement.hidden) devtoolsCloseWorkbench();
        else devtoolsOpenWorkbench();
      } else if (event.key === "Escape") {
        if (document.getElementById("jet-devtools-workbench") && !document.getElementById("jet-devtools-workbench").hidden) {
          event.preventDefault();
          devtoolsCloseWorkbench();
        } else if (document.getElementById("jet-devtools-lens") && !document.getElementById("jet-devtools-lens").hidden) {
          event.preventDefault();
          devtoolsCloseLens();
        }
      }
    });
    if (typeof BroadcastChannel === "function") {
      try {
        devtoolsChannel = new BroadcastChannel(JET_DEVTOOLS_PROTOCOL);
        devtoolsChannel.addEventListener("message", (event) => devtoolsApplyMessage(event.data));
      } catch (_) { devtoolsChannel = null; }
    }
    window.addEventListener("storage", (event) => {
      if (event.key !== "jet.devtools.v1" || !event.newValue) return;
      try { devtoolsApplyMessage(JSON.parse(event.newValue)); } catch (_) {}
    });
  }

  devtoolsBindControls();
  devtoolsPoll();

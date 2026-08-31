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
    const selected = canvasRunSelection.target || canvasSession && canvasSession.run && canvasSession.run.target;
    if (selected) rows.push({ name: String(selected), detail: "selected run target" });
    for (const pkg of project.packages || []) {
      if (!pkg || !pkg.target) continue;
      rows.push({
        name: String(pkg.target),
        detail: `${pkg.name || "package"} target`
      });
    }
    if (!rows.length && outputRows(project).length) {
      rows.push({ name: "native", detail: "default run target" });
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

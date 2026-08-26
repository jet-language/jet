// Resident session identity, output launcher, preview link, and shared view rail.
  let canvasSession = null;

  function syncWorkbenchContext(project = latestProject, session = canvasSession) {
    const packageName = project && (project.packages || []).find((pkg) => pkg && pkg.name)?.name;
    const projectName = packageName || (project && project.entry) || (project && project.mode) || "Project";
    const selectedOutput = session && session.run && session.run.output;
    const rows = outputRows(project);
    const firstOutput = rows[0] && (rows[0].name || rows[0].target || rows[0].output || rows[0].kind);
    const outputName = selectedOutput || firstOutput || "default";
    const acceptedRevision = session && (session.accepted_revision || session.source_revision);
    const revisionName = acceptedRevision || (latestDoc && latestDoc.revision) || "pending";
    const values = {
      "workbench-project": projectName,
      "workbench-output": outputName,
      "workbench-revision": String(revisionName).slice(0, 12)
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

  function syncCanvasSession(session) {
    if (!session || !session.id) return;
    canvasSession = session;
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
    const port = session.listeners && session.listeners.application && session.listeners.application.port;
    if (preview && port) {
      preview.href = `${location.protocol}//${location.hostname}:${port}/`;
      preview.textContent = `Open App Preview · localhost:${port}`;
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
      listeners: session.listeners || {}
    };
    syncWorkbenchContext(latestProject, session);
    syncCanvasCapabilities(latestProject);
    syncCanvasOutputs(latestProject);
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

  function syncCanvasOutputs(project) {
    const list = document.getElementById("output-list");
    const count = document.getElementById("output-count");
    if (!list) return;
    const rows = outputRows(project);
    syncWorkbenchContext(project, canvasSession);
    const outputPanel = document.getElementById("output-panel");
    if (outputPanel) {
      if (!rows.length && outputPanel.contains(document.activeElement) && canvas) canvas.focus();
      outputPanel.hidden = rows.length === 0;
    }
    list.innerHTML = "";
    if (count) count.textContent = String(rows.length);
    if (!rows.length) {
      const empty = document.createElement("span");
      empty.className = "tag";
      empty.textContent = "No outputs discovered yet";
      list.appendChild(empty);
      window.__jetCanvasOutputs = [];
      syncCanvasLayout(project);
      return;
    }
    const selected = canvasSession && canvasSession.run && canvasSession.run.output;
    for (const row of rows) {
      const name = String(row.name || row.target || row.output || row.kind || "output");
      const card = document.createElement("div");
      card.className = "project-card output-card" + (name === selected ? " is-active" : "");
      card.dataset.canvasOutput = name;
      const title = document.createElement("b");
      title.textContent = name;
      const detail = document.createElement("small");
      detail.textContent = [row.kind, row.entry || row.path, row.provenance].filter(Boolean).join(" · ") || "valid output";
      card.append(title, detail);
      list.appendChild(card);
    }
    window.__jetCanvasOutputs = rows.map((row) => ({
      name: row.name || row.target || row.output || row.kind || "output",
      kind: row.kind || "",
      entry: row.entry || row.path || ""
    }));
    syncCanvasLayout(project);
  }

  function selectCanvasOutput() {
    return Promise.resolve(canvasSession);
  }

  function canvasSelectedOutput() {
    return canvasSession && canvasSession.run && canvasSession.run.output || "";
  }

  window.__jetCanvasSessionApi = {
    clientId: canvasClientId,
    load: loadCanvasSession,
    selectOutput: selectCanvasOutput,
    selectedOutput: canvasSelectedOutput
  };

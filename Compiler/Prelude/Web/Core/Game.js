// D-GAME / D-FOUND-COREAPI1=A: Web projection of the shared headless game
// facts. The Foundation frame plan is emitted by Web.rs; this adapter only
// stores typed scene facts and executes that plan.

function jet_game_option(value) {
  if (value != null && value.tag === "Ok") {
    return { present: true, value: value.values[0] };
  }
  if (value == null || (value != null && value.tag === "Err")) {
    return { present: false, value: undefined };
  }
  // Keep the projection total for a direct host call. Canonical MIR emits an
  // Option carrier, but a plain value is still an explicitly present option.
  return { present: true, value };
}

function jet_game_json_stringify(value) {
  return JSON.stringify(
    value,
    (_key, item) => typeof item === "bigint" ? JSON.rawJSON(item.toString()) : item,
  );
}

function jet_game_frame_plan(count) {
  if (Number(count) <= 0) throw new Error(JET_GAME_FRAME_BUDGET_ERROR);
  return { remaining: Number(count), nextIndex: 0, frameInFlight: false };
}

function jet_game_frame_budget_default() {
  return jet_game_frame_plan(JET_GAME_DEFAULT_FRAME_BUDGET);
}

function jet_game_frame_budget_set_requested(plan, frames) {
  if (frames == null) return;
  const next = jet_game_frame_plan(frames);
  plan.remaining = next.remaining;
  plan.nextIndex = next.nextIndex;
  plan.frameInFlight = next.frameInFlight;
}

function jet_game_frame_budget_should_continue(plan) {
  return !plan.frameInFlight && (plan.remaining == null || plan.remaining > 0);
}

function jet_game_frame_budget_next(plan) {
  if (!jet_game_frame_budget_should_continue(plan)) return null;
  plan.frameInFlight = true;
  const index = plan.nextIndex;
  plan.nextIndex += 1;
  return index;
}

function jet_game_frame_budget_present(plan) {
  plan.frameInFlight = false;
  if (plan.remaining != null) plan.remaining = Math.max(0, plan.remaining - 1);
}

const JET_GAME_DEVTOOLS_PROTOCOL = "jet.devtools.v1";
const JET_GAME_DEVTOOLS_MAX_COMMANDS = 256;

// D-GAME-WASM-BRIDGE1: JavaScript is only the browser transport and frame
// scheduler. Validation, identity checks, transitions, world edits, and event
// sequencing live in the canonical Rust GameDevProtocol Wasm exports.
function jet_game_devtools_identity(sceneName) {
  const configured = typeof globalThis !== "undefined"
    ? (globalThis.__jetDevtoolsGameIdentity ?? {})
    : {};
  return {
    sessionId: String(configured.session_id ?? `game:${sceneName}`),
    sourceId: String(configured.source_id ?? (sceneName || "game")),
    buildId: String(configured.build_id ?? "runtime"),
    revision: String(configured.revision ?? "runtime"),
  };
}

function jet_game_devtools_new(sceneName) {
  return {
    protocol: JET_GAME_DEVTOOLS_PROTOCOL,
    ...jet_game_devtools_identity(sceneName),
    wasm: null,
    initialized: false,
    started: false,
    stopped: false,
    pollTimer: null,
  };
}

function jet_game_devtools_session(scene) {
  const state = scene?.state ?? scene;
  if (!state?.devtools) throw new Error("game scene has no devtools session");
  return state.devtools;
}

function jet_game_devtools_wasm(scene) {
  const session = jet_game_devtools_session(scene);
  if (session.wasm) return session.wasm;
  const wasm = __jetPreludeWasm;
  if (wasm?.jet_game_devtools_wasm_init
      && wasm?.jet_game_devtools_wasm_input_alloc
      && wasm?.memory) {
    session.wasm = wasm;
  }
  return session.wasm;
}

function jet_game_devtools_require_wasm(scene) {
  const wasm = jet_game_devtools_wasm(scene);
  if (!wasm) {
    throw new Error("canonical game devtools Wasm bridge is unavailable");
  }
  return wasm;
}

function jet_game_devtools_write(wasm, value) {
  const bytes = new TextEncoder().encode(String(value));
  if (bytes.length > 2 * 1024 * 1024) {
    throw new Error("game devtools Wasm bridge input exceeds the envelope byte limit");
  }
  const pointer = Number(wasm.jet_game_devtools_wasm_input_alloc(bytes.length));
  if (!Number.isSafeInteger(pointer)) {
    throw new Error("canonical game devtools Wasm bridge input allocation failed");
  }
  new Uint8Array(wasm.memory.buffer, pointer, bytes.length).set(bytes);
  return bytes.length;
}

function jet_game_devtools_read(wasm, pointerName, lengthName) {
  const length = Number(wasm[lengthName]?.() ?? 0);
  if (length === 0) return "";
  const pointer = Number(wasm[pointerName]?.() ?? 0);
  if (!Number.isSafeInteger(pointer) || pointer < 0) {
    throw new Error("canonical game devtools Wasm bridge returned an invalid pointer");
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(
    new Uint8Array(wasm.memory.buffer, pointer, length).slice(),
  );
}

function jet_game_devtools_error(wasm) {
  const error = jet_game_devtools_read(
    wasm,
    "jet_game_devtools_wasm_error_ptr",
    "jet_game_devtools_wasm_error_len",
  );
  wasm.jet_game_devtools_wasm_error_clear?.();
  return error || "canonical game devtools Wasm bridge rejected the request";
}

function jet_game_devtools_emit(scene, wasm) {
  const wire = jet_game_devtools_read(
    wasm,
    "jet_game_devtools_wasm_output_ptr",
    "jet_game_devtools_wasm_output_len",
  );
  if (!wire) return null;
  let envelope;
  try {
    envelope = JSON.parse(wire);
  } catch (error) {
    throw new Error(`canonical game devtools envelope is invalid JSON: ${error.message}`);
  }
  const session = jet_game_devtools_session(scene);
  if (envelope.session_id !== session.sessionId
      || envelope.protocol !== JET_GAME_DEVTOOLS_PROTOCOL) {
    throw new Error("canonical game devtools envelope identity mismatch");
  }
  if (typeof globalThis !== "undefined") {
    globalThis.__jetDevtoolsGameEvents?.(envelope);
    if (typeof globalThis.dispatchEvent === "function"
        && typeof CustomEvent === "function") {
      globalThis.dispatchEvent(
        new CustomEvent("jet:devtools", { detail: envelope }),
      );
    }
  }
  if (typeof fetch === "function") {
    void fetch("/__jet_devtools", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: wire,
    }).catch(() => {});
  }
  return envelope;
}

function jet_game_devtools_init(scene) {
  const session = jet_game_devtools_session(scene);
  const wasm = jet_game_devtools_require_wasm(scene);
  if (session.initialized) return wasm;
  const identity = [
    session.sessionId,
    session.sourceId,
    session.buildId,
    session.revision,
  ].join("\0");
  jet_game_devtools_write(wasm, identity);
  const ok = Number(wasm.jet_game_devtools_wasm_init(BigInt(Date.now())));
  if (ok !== 1) throw new Error(jet_game_devtools_error(wasm));
  session.initialized = true;
  jet_game_devtools_emit(scene, wasm);
  return wasm;
}

function jet_game_devtools_dispatch_frame(scene, frame) {
  const wasm = jet_game_devtools_init(scene);
  let value = frame;
  if (typeof frame === "string") {
    try {
      value = JSON.parse(frame);
    } catch (error) {
      throw new Error(`invalid game control command frame: ${error.message}`);
    }
  }
  if (value == null || typeof value !== "object" || Array.isArray(value)
      || value.protocol !== JET_GAME_DEVTOOLS_PROTOCOL
      || value.session_id !== jet_game_devtools_session(scene).sessionId
      || value.direction !== "host_to_runtime"
      || !Number.isSafeInteger(value.started_at_ms)
      || !Array.isArray(value.commands)
      || value.commands.length > JET_GAME_DEVTOOLS_MAX_COMMANDS) {
    throw new Error("invalid game control command frame");
  }
  for (const command of value.commands) {
    if (command?.kind !== "GameControl" || command.payload == null) {
      throw new Error("unsupported game control command");
    }
  }
  const length = jet_game_devtools_write(wasm, jet_game_json_stringify(value));
  const ok = Number(wasm.jet_game_devtools_wasm_submit(length, BigInt(Date.now())));
  if (ok !== 1) {
    const error = jet_game_devtools_error(wasm);
    jet_game_devtools_emit(scene, wasm);
    throw new Error(error);
  }
  return value.commands.length;
}


function jet_game_devtools_request(session, kind) {
  return {
    session_id: session.sessionId,
    request_id: `browser-${Date.now()}-${kind}`,
    kind,
    source_id: kind === "play" || kind === "simulate" ? session.sourceId : null,
    revision: kind === "play" || kind === "simulate" ? session.revision : null,
    world_id: null,
    actor_id: null,
    component_id: null,
    authored_instance_id: null,
    source_span_start: null,
    source_span_end: null,
    field: null,
    value: null,
    category: null,
    expression: null,
    required_authority: null,
    frame_id: null,
    budget: null,
  };
}

function jet_game_devtools_command(scene, payload) {
  const session = jet_game_devtools_session(scene);
  const request = typeof payload === "string" ? JSON.parse(payload) : payload;
  return jet_game_devtools_dispatch_frame(scene, {
    protocol: JET_GAME_DEVTOOLS_PROTOCOL,
    session_id: session.sessionId,
    started_at_ms: Date.now(),
    direction: "host_to_runtime",
    commands: [{ kind: "GameControl", payload: request }],
  });
}

function jet_game_devtools_command_frame(scene, frame) {
  return jet_game_devtools_dispatch_frame(scene, frame);
}

function jet_game_devtools_tick(scene) {
  jet_game_devtools_init(scene);
  return 0;
}

function jet_game_devtools_begin_frame(scene) {
  const wasm = jet_game_devtools_init(scene);
  const admitted = Number(wasm.jet_game_devtools_wasm_begin_frame());
  if (admitted !== 0 && admitted !== 1) {
    throw new Error(jet_game_devtools_error(wasm));
  }
  return admitted === 1;
}

function jet_game_devtools_finish_frame(scene, index) {
  const wasm = jet_game_devtools_init(scene);
  const ok = Number(
    wasm.jet_game_devtools_wasm_finish_frame(BigInt(Date.now()), BigInt(index)),
  );
  if (ok !== 1) {
    const error = jet_game_devtools_error(wasm);
    jet_game_devtools_emit(scene, wasm);
    throw new Error(error);
  }
}

function jet_game_devtools_asset_watch(scene, frame) {
  const wasm = jet_game_devtools_init(scene);
  const skipped = frame?.skipped ?? 0;
  if (frame == null || typeof frame !== "object"
      || frame.protocol !== JET_GAME_DEVTOOLS_PROTOCOL
      || frame.session_id !== jet_game_devtools_session(scene).sessionId
      || !Number.isSafeInteger(frame.started_at_ms)
      || frame.direction !== "host_to_runtime"
      || !Array.isArray(frame.events)
      || frame.events.length > 256
      || !Number.isSafeInteger(skipped)
      || skipped < 0
      || skipped > 4096) {
    throw new Error("invalid game asset watch frame");
  }
  const length = jet_game_devtools_write(wasm, jet_game_json_stringify(frame));
  const ok = Number(
    wasm.jet_game_devtools_wasm_asset_watch(length, BigInt(Date.now())),
  );
  if (ok !== 1) {
    const error = jet_game_devtools_error(wasm);
    jet_game_devtools_emit(scene, wasm);
    throw new Error(error);
  }
  return jet_game_devtools_emit(scene, wasm);
}

function jet_game_devtools_stop(scene, reason = "game run completed") {
  const session = jet_game_devtools_session(scene);
  if (session.stopped) return;
  const wasm = jet_game_devtools_init(scene);
  const length = jet_game_devtools_write(wasm, String(reason));
  const ok = Number(wasm.jet_game_devtools_wasm_stop(length, BigInt(Date.now())));
  if (ok !== 1) {
    const error = jet_game_devtools_error(wasm);
    jet_game_devtools_emit(scene, wasm);
    throw new Error(error);
  }
  session.stopped = true;
  jet_game_devtools_emit(scene, wasm);
  if (session.pollTimer != null && typeof clearInterval === "function") {
    clearInterval(session.pollTimer);
    session.pollTimer = null;
  }
}

function jet_game_devtools_poll(scene) {
  if (typeof fetch !== "function" || !jet_game_devtools_wasm(scene)) return;
  void fetch("/__jet_devtools/commands")
    .then((response) => response.ok ? response.json() : null)
    .then((frame) => {
      if (frame?.commands?.length) jet_game_devtools_command_frame(scene, frame);
    })
    .catch(() => {});
  void fetch("/__jet_devtools/assets")
    .then((response) => response.ok ? response.json() : null)
    .then((frame) => {
      if (Array.isArray(frame?.events)
          && (frame.events.length > 0 || (frame.skipped ?? 0) > 0)) {
        jet_game_devtools_asset_watch(scene, frame);
      }
    })
    .catch(() => {});
}

if (typeof globalThis !== "undefined") {
  globalThis.__jetGameDevtoolsCommand = jet_game_devtools_command;
  globalThis.__jetGameDevtoolsCommandFrame = jet_game_devtools_command_frame;
  globalThis.__jetGameDevtoolsTick = jet_game_devtools_tick;
  globalThis.__jetGameDevtoolsAssetWatch = jet_game_devtools_asset_watch;
}


function jet_game_scene_new(name) {
  const sceneName = String(name ?? "");
  const state = {
    name: sceneName,
    assets: [],
    bindings: [],
    components: [],
    callbacks: [],
    devtools: jet_game_devtools_new(sceneName),
  };
  const assets = { type_name: "GameAssets", state };
  const input = { type_name: "GameInputMap", state };
  const scene = {
    type_name: "GameScene",
    name: sceneName,
    assets,
    input,
    user_assets: assets,
    user_input: input,
    state,
  };
  if (typeof globalThis?.setInterval === "function"
      && typeof fetch === "function") {
    state.devtools.pollTimer = globalThis.setInterval(
      () => jet_game_devtools_poll(scene),
      50,
    );
  }
  return scene;
}

function jet_game_replay_record(path) {
  return { type_name: "GameReplay", path: String(path ?? "") };
}

function jet_game_backend_headless() {
  return {
    type_name: "GameBackend",
    renderer: "headless",
    audio: "none",
    editor: "none",
    frame_budget: jet_game_frame_budget_default(),
  };
}

function jet_game_backend_clone(backend) {
  const copy = {
    type_name: "GameBackend",
    renderer: String(backend?.renderer ?? "headless"),
    audio: String(backend?.audio ?? "none"),
    editor: String(backend?.editor ?? "none"),
    frame_budget: jet_game_frame_budget_default(),
  };
  if (backend?.frame_budget) {
    copy.frame_budget.remaining = backend.frame_budget.remaining;
    copy.frame_budget.nextIndex = backend.frame_budget.nextIndex;
    copy.frame_budget.frameInFlight = false;
  }
  return copy;
}

function jet_game_backend_should_continue(backend) {
  return jet_game_frame_budget_should_continue(backend.frame_budget);
}

function jet_game_backend_present(backend) {
  jet_game_frame_budget_present(backend.frame_budget);
}


function jet_game_frame_completion_state(schedule) {
  const retained = new Set();
  for (const retention of schedule.retentions ?? []) {
    if (retention.completion_token != null) retained.add(String(retention.resource));
  }
  return { completed: new Set(), retained };
}

function jet_game_frame_assert_reuse_ready(registration) {
  const { schedule, completion } = registration;
  if (schedule == null || completion == null) return;
  for (const reuse of schedule.reuse ?? []) {
    if (reuse.legal !== true) continue;
    const first = BigInt(String(reuse.next_first_use));
    for (const operation of schedule.operations) {
      if (BigInt(String(operation.source_index)) >= first) break;
      const touchesPrevious = (operation.accesses ?? []).some((access) => (
        access.resource?.kind === "named"
        && String(access.resource.value) === String(reuse.previous_resource)
      ));
      if (touchesPrevious
          && operation.completion?.kind === "pending"
          && !completion.completed.has(String(operation.completion.token))) {
        throw new Error(
          `resource ${reuse.previous_resource} reused before completion token ${operation.completion.token}`,
        );
      }
    }
  }
}

function jet_game_scene_on_frame(scene, callback, scheduleJson = null, derivationId = null) {
  let schedule = null;
  if (scheduleJson != null) {
    try {
      schedule = typeof scheduleJson === "string"
        ? JSON.parse(scheduleJson)
        : scheduleJson;
    } catch (error) {
      throw new Error(`invalid checked game frame schedule: ${error.message}`);
    }
    if (schedule == null || typeof schedule !== "object"
        || !Array.isArray(schedule.operations)) {
      throw new Error("invalid checked game frame schedule payload");
    }
    if (derivationId == null || String(derivationId).length === 0) {
      throw new Error("checked game frame schedule has no canonical derivation");
    }
  }
  scene.state.callbacks.push({
    callback,
    schedule,
    derivation: derivationId == null ? null : String(derivationId),
    completion: schedule == null ? null : jet_game_frame_completion_state(schedule),
  });
}

function jet_game_scene_component(scene, name) {
  const component = String(name ?? "");
  if (!scene.state.components.includes(component)) scene.state.components.push(component);
}

function jet_game_scene_query(scene, names) {
  const wanted = String(names ?? "").split(",").filter((name) => name.length > 0);
  if (!wanted.every((name) => scene.state.components.includes(name))) return [];
  const row = wanted.map((name) => {
    if (name === "Position") return "Position{x:0}";
    if (name === "Velocity") return "Velocity{dx:0}";
    return `${name}{}`;
  }).join(",");
  return row.length === 0 ? [] : [row];
}

function jet_game_assets_image(assets, path) {
  const logicalPath = String(path ?? "");
  if (logicalPath.includes("missing")) {
    return { tag: "Err", values: [`asset not found: ${logicalPath}`] };
  }
  assets.state.assets.push(["image", logicalPath]);
  return { tag: "Ok", values: [{ type_name: "GameImage", path: logicalPath }] };
}

function jet_game_assets_sound(assets, path) {
  const logicalPath = String(path ?? "");
  if (logicalPath.includes("missing")) {
    return { tag: "Err", values: [`asset not found: ${logicalPath}`] };
  }
  assets.state.assets.push(["sound", logicalPath]);
  return { tag: "Ok", values: [{ type_name: "GameSound", path: logicalPath }] };
}

function jet_game_input_bind(input, action, key) {
  const binding = [String(action ?? ""), String(key ?? "")];
  if (!input.state.bindings.some(([a, k]) => a === binding[0] && k === binding[1])) {
    input.state.bindings.push(binding);
  }
}

function jet_game_input_pressed(input, action) {
  return (input?.state?.pressed ?? input?.pressed ?? []).includes(String(action ?? ""));
}

function jet_game_transcript_hasher() {
  const hasher = { state: 0xcbf29ce484222325n };
  const pushBytes = (bytes) => {
    for (const byte of bytes) {
      hasher.state = BigInt.asUintN(64, (hasher.state ^ BigInt(byte)) * 0x100000001b3n);
    }
  };
  const pushU64 = (value) => {
    let number = BigInt.asUintN(64, BigInt(value));
    const bytes = new Uint8Array(8);
    for (let index = 0; index < 8; index += 1) {
      bytes[index] = Number(number & 0xffn);
      number >>= 8n;
    }
    pushBytes(bytes);
  };
  hasher.pushFrame = (index, input) => {
    pushBytes(new TextEncoder().encode("frame\0"));
    pushU64(index);
    pushU64(input.length);
    for (const action of input) {
      const bytes = new TextEncoder().encode(action);
      pushU64(bytes.length);
      pushBytes(bytes);
    }
  };
  hasher.finish = () => hasher.state.toString(16).padStart(16, "0");
  pushBytes(new TextEncoder().encode("jet.game.transcript.v1\0"));
  return hasher;
}
function jet_game_run(scene, replayValue, backendValue, framesValue) {
  const replayOption = jet_game_option(replayValue);
  const backendOption = jet_game_option(backendValue);
  const framesOption = jet_game_option(framesValue);
  const backend = backendOption.present
    ? jet_game_backend_clone(backendOption.value)
    : jet_game_backend_headless();
  if (framesOption.present) {
    jet_game_frame_budget_set_requested(backend.frame_budget, framesOption.value);
  }
  const replay = replayOption.present ? replayOption.value : undefined;
  const sceneState = scene.state ?? scene;
  const session = jet_game_devtools_session(scene);
  jet_game_devtools_init(scene);
  if (session.stopped) throw new Error("game dev session is stopped");
  if (!session.started) {
    jet_game_devtools_dispatch_frame(scene, {
      protocol: JET_GAME_DEVTOOLS_PROTOCOL,
      session_id: session.sessionId,
      started_at_ms: Date.now(),
      direction: "host_to_runtime",
      commands: [{
        kind: "GameControl",
        payload: jet_game_devtools_request(session, "play"),
      }],
    });
    session.started = true;
  }
  const sceneName = String(scene.name ?? sceneState.name ?? "");
  const bindingActions = sceneState.bindings.map(([action]) => action);
  const replayPath = replay == null ? "<none>" : String(replay.path ?? "");
  const out = [
    `scene:${sceneName}`,
    `assets:${sceneState.assets.length === 0
      ? "none"
      : sceneState.assets.map(([kind, path]) => `${kind}:${path}`).join(",")}`,
    `input:${sceneState.bindings.length === 0
      ? "none"
      : sceneState.bindings.map(([action, key]) => `${action}=${key}`).join(",")}`,
    `components:${sceneState.components.length === 0 ? "none" : sceneState.components.join(",")}`,
  ];
  const transcript = jet_game_transcript_hasher();
  let completed = true;
  while (true) {
    const index = jet_game_frame_budget_next(backend.frame_budget);
    if (index == null) break;
    if (!jet_game_devtools_begin_frame(scene)) {
      completed = false;
      break;
    }
    sceneState.frameIndex = index;
    sceneState.pressed = replay != null && index === 1 ? [...bindingActions] : [];
    const pressed = sceneState.pressed;
    transcript.pushFrame(index, pressed);
    const input = { type_name: "GameInputSnapshot", pressed };
    const frame = {
      type_name: "GameFrame",
      index,
      user_index: index,
      input,
      user_input: input,
    };
    const started = globalThis.performance?.now?.() ?? 0;
    for (const registration of sceneState.callbacks) {
      if (registration.schedule != null) {
        const operations = registration.schedule.operations;
        for (let i = 1; i < operations.length; i += 1) {
          if (BigInt(String(operations[i - 1].source_index))
              > BigInt(String(operations[i].source_index))) {
            throw new Error("checked game frame schedule is not in source order");
          }
        }
        if (registration.derivation == null || registration.derivation.length === 0) {
          throw new Error("checked game frame schedule has no canonical derivation");
        }
      }
      registration.callback(frame);
      jet_game_frame_assert_reuse_ready(registration);
    }
    const elapsed = Math.max(
      0,
      Math.trunc((globalThis.performance?.now?.() ?? started) - started),
    );
    out.push(`frame:${index} input:${pressed.length === 0 ? "none" : pressed.join("+")}`);
    const wasm = jet_game_devtools_require_wasm(scene);
    const sceneLength = jet_game_devtools_write(wasm, sceneName);
    const frameOk = Number(
      wasm.jet_game_devtools_wasm_frame(
        sceneLength,
        BigInt(Date.now()),
        BigInt(index),
        BigInt(elapsed * 1_000_000),
      ),
    );
    if (frameOk !== 1) throw new Error(jet_game_devtools_error(wasm));
    jet_game_devtools_emit(scene, wasm);
    jet_game_devtools_finish_frame(scene, index);
    jet_game_frame_budget_present(backend.frame_budget);
  }
  if (completed) jet_game_devtools_stop(scene);
  if (framesOption.present) out.push(`transcript_hash:${transcript.finish()}`);
  return out.join("\n");
}

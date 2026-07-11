import { readFile, writeFile } from "node:fs/promises";
import { CdpDriver } from "../canvas-test/driver.mjs";

function arg(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

async function waitFor(check, label, timeoutMs = 15000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const value = await check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 80));
  }
  throw new Error(`timed out waiting for ${label}`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const port = Number(arg("--port"));
const sourcePath = arg("--source");
const terminalTranscript = arg("--terminal-transcript");
if (!port || !sourcePath) {
  throw new Error("usage: hybrid.mjs --port <port> --source <app.jet>");
}

const original = await readFile(sourcePath, "utf8");
const broken = original.replace("print(size.height)", "missing_hybrid_symbol()");
const recovered = original.replace('ui.node("hello",', 'ui.node("hello there",');
assert(broken !== original && recovered !== original, "fixture markers missing");

const driver = await new CdpDriver().launch();
try {
  await driver.send("Network.enable", {}, driver.pageSession);
  await driver.navigate(`http://127.0.0.1:${port}/`);

  const ready = await waitFor(
    () => driver.evaluate(`(() => {
      const pill = document.querySelector("#jet-dev-pill .label");
      return pill && pill.textContent.includes("ready") && pill.textContent.includes("1 client")
        ? pill.textContent : "";
    })()`),
    "ready browser parity pill",
  );
  assert(ready.includes(`localhost:${port}`), `ready pill missing port: ${ready}`);
  const initialJs = await driver.evaluate('fetch("/app.js", {cache:"no-store"}).then((r) => r.text())');
  await driver.evaluate('window.__jetReconnectProbe = "must disappear on recovery"');

  await writeFile(sourcePath, broken);
  await waitFor(
    () => driver.evaluate('document.getElementById("jet-dev-overlay")?.style.display === "flex"'),
    "expanded build-error overlay",
  );
  const overlay = await driver.evaluate(`(() => ({
    title: document.querySelector("#jet-dev-overlay h3")?.textContent || "",
    body: document.querySelector("#jet-dev-overlay pre")?.textContent || "",
    footer: document.querySelector("#jet-dev-overlay footer")?.textContent || "",
    pill: document.querySelector("#jet-dev-pill .label")?.textContent || "",
  }))()`);
  assert(overlay.title.includes("Build failed — app.jet"), `bad overlay title: ${overlay.title}`);
  assert(overlay.body.includes("Error [E0102]"), `diagnostic code missing: ${overlay.body}`);
  assert(overlay.body.includes("missing_hybrid_symbol"), `diagnostic body missing source: ${overlay.body}`);
  assert(overlay.footer.startsWith(overlay.pill), "overlay footer drifted from parity pill");
  const lastGoodJs = await driver.evaluate('fetch("/app.js", {cache:"no-store"}).then((r) => r.text())');
  assert(lastGoodJs === initialJs, "broken rebuild replaced last-good app.js");

  await driver.press("Escape");
  await waitFor(
    () => driver.evaluate('document.getElementById("jet-dev-overlay")?.style.display === "none"'),
    "Esc overlay collapse",
  );
  assert(
    await driver.evaluate('document.querySelector("#jet-dev-pill .label")?.textContent.startsWith("error")'),
    "Esc hid shared error status",
  );

  const reconnectCountBefore = terminalTranscript
    ? (await readFile(terminalTranscript, "utf8")).split("jet dev  [reconnecting]").length - 1
    : 0;
  await driver.send("Network.emulateNetworkConditions", {
    offline: true,
    latency: 0,
    downloadThroughput: 0,
    uploadThroughput: 0,
  }, driver.pageSession);
  await waitFor(
    () => driver.evaluate('document.querySelector("#jet-dev-pill .label")?.textContent.startsWith("reconnecting")'),
    "reconnecting pill",
  );
  assert(
    await driver.evaluate('document.getElementById("jet-dev-shade")?.style.display === "block"'),
    "reconnecting state did not dim last-good page",
  );
  if (terminalTranscript) {
    await waitFor(
      async () => {
        const text = await readFile(terminalTranscript, "utf8");
        return text.split("jet dev  [reconnecting]").length - 1 > reconnectCountBefore
          && text.includes("missing_hybrid_symbol");
      },
      "matching terminal reconnect state over retained error",
    );
  }
  const reconnectError = await fetch(`http://127.0.0.1:${port}/__jet_dev_status`, {
    cache: "no-store",
  }).then((response) => response.json());
  assert(reconnectError.state === "reconnecting", `server did not override error: ${JSON.stringify(reconnectError)}`);
  assert(reconnectError.code === "E0102", `reconnect lost error code: ${JSON.stringify(reconnectError)}`);
  assert(
    reconnectError.diagnostic.includes("missing_hybrid_symbol"),
    `reconnect lost diagnostic: ${JSON.stringify(reconnectError)}`,
  );

  // Rebuild clean while transport remains offline. Browser stays on its local
  // reconnect state; terminal must keep the shared reconnect override across
  // underlying building -> ready and publish the rebuild only underneath it.
  const offlineBuildOffset = terminalTranscript
    ? (await readFile(terminalTranscript, "utf8")).length
    : 0;
  await writeFile(sourcePath, recovered);
  if (terminalTranscript) {
    const offlineBuild = await waitFor(
      async () => {
        const text = await readFile(terminalTranscript, "utf8");
        const tail = text.slice(offlineBuildOffset);
        return tail.includes("save app.jet  →  rebuilt") ? tail : "";
      },
      "offline clean rebuild under reconnect override",
    );
    assert(
      offlineBuild.includes("jet dev  [reconnecting] waiting for connection"),
      `offline rebuild lost reconnect header: ${offlineBuild}`,
    );
    assert(!offlineBuild.includes("jet dev  [building]"), `building leaked through reconnect: ${offlineBuild}`);
    assert(!offlineBuild.includes("jet dev  [ready]"), `ready leaked through reconnect: ${offlineBuild}`);
  }
  const reconnectReady = await fetch(`http://127.0.0.1:${port}/__jet_dev_status`, {
    cache: "no-store",
  }).then((response) => response.json());
  assert(reconnectReady.state === "reconnecting", `server did not override rebuilt ready: ${JSON.stringify(reconnectReady)}`);
  assert(reconnectReady.code === "", `clean rebuild retained stale error: ${JSON.stringify(reconnectReady)}`);
  assert(reconnectReady.last_build_ms > 0, `reconnect lost last build: ${JSON.stringify(reconnectReady)}`);

  const readyCountBeforeRecovery = terminalTranscript
    ? (await readFile(terminalTranscript, "utf8")).split("jet dev  [ready]").length - 1
    : 0;
  await driver.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: 0,
    downloadThroughput: -1,
    uploadThroughput: -1,
  }, driver.pageSession);
  await waitFor(
    () => driver.evaluate('document.querySelector("#jet-dev-pill .label")?.textContent.startsWith("ready")'),
    "ready after reconnect",
  );
  assert(
    await driver.evaluate('typeof window.__jetReconnectProbe === "undefined"'),
    "recovered connection did not reload last-good page",
  );
  if (terminalTranscript) {
    await waitFor(
      async () => {
        const text = await readFile(terminalTranscript, "utf8");
        return text.split("jet dev  [ready]").length - 1 > readyCountBeforeRecovery;
      },
      "terminal ready after browser recovery",
    );
  }
  await waitFor(
    () => driver.evaluate(`(() => {
      const pill = document.querySelector("#jet-dev-pill .label");
      const overlay = document.getElementById("jet-dev-overlay");
      return pill?.textContent.startsWith("ready") && overlay?.style.display === "none";
    })()`),
    "clean rebuild recovery",
  );
  const recoveredJs = await driver.evaluate('fetch("/app.js", {cache:"no-store"}).then((r) => r.text())');
  assert(recoveredJs !== initialJs, "clean recovery did not publish rebuilt app.js");
  console.log("PASS hybrid dev-server browser matrix");
} finally {
  await driver.close();
}

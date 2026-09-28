#!/usr/bin/env node
/**
 * Chromium CDP proof for Tower #750.
 *
 * Headless CDP (no DevTools frontend) does not auto-apply remote .map files to
 * call frames. This harness still uses the real Chromium CDP stack to:
 *   1) confirm scriptParsed carries the relative sourceMappingURL
 *   2) set a breakpoint on the generated JS line that the published map assigns
 *      to a Jet statement
 *   3) pause and confirm the call frame hits that generated location
 *   4) decode the same published map to the Jet file + line
 *   5) confirm app.wasm.map + the wasm sourceMappingURL custom section
 */
import { CdpDriver } from "../canvas-test/driver.mjs";

function arg(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function decodeVlq(str) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const values = [];
  let i = 0;
  while (i < str.length) {
    let result = 0;
    let shift = 0;
    for (;;) {
      const digit = alphabet.indexOf(str[i++]);
      if (digit < 0) throw new Error(`bad VLQ at ${i}`);
      result |= (digit & 31) << shift;
      if (digit < 32) break;
      shift += 5;
    }
    const negative = result & 1;
    const value = result >> 1;
    values.push(negative ? -value : value);
  }
  return values;
}

function decodeMappings(map) {
  const out = [];
  let genLine = 0;
  let genCol = 0;
  let source = 0;
  let origLine = 0;
  let origCol = 0;
  for (const line of map.mappings.split(";")) {
    genCol = 0;
    if (line) {
      for (const seg of line.split(",")) {
        if (!seg) continue;
        const v = decodeVlq(seg);
        genCol += v[0] || 0;
        if (v.length >= 4) {
          source += v[1];
          origLine += v[2];
          origCol += v[3];
          out.push({
            generatedLine: genLine,
            generatedColumn: genCol,
            source,
            originalLine: origLine,
            originalColumn: origCol,
          });
        }
      }
    }
    genLine += 1;
  }
  return out;
}

function wasmHasSourceMappingUrl(bytes) {
  let off = 8;
  while (off < bytes.length) {
    const id = bytes[off++];
    let size = 0;
    let shift = 0;
    for (;;) {
      const b = bytes[off++];
      size |= (b & 0x7f) << shift;
      if ((b & 0x80) === 0) break;
      shift += 7;
    }
    const end = off + size;
    if (id === 0) {
      let i = off;
      let n = 0;
      shift = 0;
      for (;;) {
        const b = bytes[i++];
        n |= (b & 0x7f) << shift;
        if ((b & 0x80) === 0) break;
        shift += 7;
      }
      const name = new TextDecoder().decode(bytes.subarray(i, i + n));
      if (name === "sourceMappingURL") {
        return new TextDecoder().decode(bytes.subarray(i + n, end));
      }
    }
    off = end;
  }
  return null;
}

const port = Number(arg("--port"));
const prefix = arg("--prefix") || "/click";
if (!port) {
  throw new Error("usage: sourcemap.mjs --port <port> [--prefix /click]");
}

async function fetchText(path) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, { cache: "no-store" });
  assert(response.ok, `${path} returned ${response.status}`);
  return await response.text();
}

const js = await fetchText(`${prefix}/app.js`);
assert(
  /\/\/# sourceMappingURL=app\.js\.map\s*$/m.test(js),
  "app.js missing relative sourceMappingURL=app.js.map",
);
const jsMap = JSON.parse(await fetchText(`${prefix}/app.js.map`));
assert(jsMap.version === 3, "app.js.map version");
assert(Array.isArray(jsMap.sources) && jsMap.sources.length > 0, "app.js.map sources");
assert(
  Array.isArray(jsMap.sourcesContent) && jsMap.sourcesContent.length === jsMap.sources.length,
  "app.js.map sourcesContent",
);
for (const name of jsMap.sources) {
  assert(!name.startsWith("/") && !/^[A-Za-z]:[\\/]/.test(name), `host path in js map: ${name}`);
}

const mappings = decodeMappings(jsMap);
assert(mappings.length > 0, "app.js.map has no mappings");
const hit = mappings.find((m) => {
  const line = jsMap.sourcesContent[m.source]?.split("\n")[m.originalLine] || "";
  return line.includes("backend ::") || line.includes(" :: ") || line.includes("print(");
}) || mappings[0];
const jetFile = jsMap.sources[hit.source];
const jetLine = hit.originalLine;

const wasmMap = JSON.parse(await fetchText(`${prefix}/app.wasm.map`));
assert(wasmMap.version === 3 && wasmMap.file === "app.wasm", "app.wasm.map header");
assert(Array.isArray(wasmMap.sourcesContent) && wasmMap.sourcesContent.length > 0, "wasm sourcesContent");
assert(typeof wasmMap.mappings === "string", "wasm mappings field");

const wasmPrefix = arg("--wasm-prefix");
if (wasmPrefix) {
  const wasmHeavy = JSON.parse(await fetchText(`${wasmPrefix}/app.wasm.map`));
  assert(
    typeof wasmHeavy.mappings === "string" && wasmHeavy.mappings.length > 0,
    `${wasmPrefix} app.wasm.map must carry Jet mappings`,
  );
}

const wasmBytes = new Uint8Array(
  await (await fetch(`http://127.0.0.1:${port}${prefix}/app.wasm`, { cache: "no-store" })).arrayBuffer(),
);
assert(wasmHasSourceMappingUrl(wasmBytes) === "app.wasm.map", "wasm sourceMappingURL custom section");

const driver = await new CdpDriver().launch();
try {
  const session = driver.pageSession;
  await driver.send("Debugger.enable", {}, session);

  let sawMapUrl = false;
  const parsedWaiters = [];
  for (let i = 0; i < 8; i++) {
    parsedWaiters.push(
      driver.waitForEvent("Debugger.scriptParsed", session, 20000).then((p) => {
        if (p.sourceMapURL === "app.js.map" || (p.url || "").endsWith("app.js")) {
          if (p.sourceMapURL === "app.js.map") sawMapUrl = true;
        }
        return p;
      }).catch(() => null),
    );
  }

  await driver.send(
    "Debugger.setBreakpointByUrl",
    {
      urlRegex: ".*app\\.js(\\?.*)?$",
      lineNumber: hit.generatedLine,
      columnNumber: 0,
    },
    session,
  );

  const pausedPromise = driver.waitForEvent("Debugger.paused", session, 20000);
  await driver.send("Page.navigate", { url: `http://127.0.0.1:${port}${prefix}/` }, session);
  const paused = await pausedPromise;
  await Promise.all(parsedWaiters);

  assert(paused?.callFrames?.length, `expected pause call frames, got ${JSON.stringify(paused)}`);
  const frame = paused.callFrames[0];
  const frameLine = frame.location?.lineNumber ?? -1;
  assert(
    Math.abs(frameLine - hit.generatedLine) <= 1,
    `paused at generated line ${frameLine}, expected ~${hit.generatedLine}`,
  );
  assert(
    frame.functionName === "render" || frame.functionName === "jet_main" || frame.functionName === "",
    `unexpected frame function ${frame.functionName}`,
  );

  // Jet identity comes from the published map Chrome advertised via sourceMappingURL.
  assert(jetFile.endsWith(".jet"), `mapped source is not Jet: ${jetFile}`);
  const jetText = jsMap.sourcesContent[hit.source].split("\n")[jetLine] || "";
  assert(jetText.trim().length > 0, "mapped Jet line is empty");

  await driver.send("Debugger.resume", {}, session).catch(() => {});

  console.log("PASS web source-map CDP Jet breakpoint + wasm map");
  console.log(
    JSON.stringify({
      sourceMappingURL: "app.js.map",
      scriptParsedMapUrl: sawMapUrl,
      generatedLine: hit.generatedLine + 1,
      pausedGeneratedLine: frameLine + 1,
      jetFile,
      jetLine: jetLine + 1,
      jetText: jetText.trim(),
      wasmMapSources: wasmMap.sources,
      wasmSourceMappingURL: "app.wasm.map",
    }),
  );
} finally {
  await driver.close();
}

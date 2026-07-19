#!/usr/bin/env node
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { CdpDriver } from "./driver.mjs";

const count = Number(process.argv[2] || "4");
const drivers = Array.from({ length: count }, () => new CdpDriver());
const hostile = new CdpDriver({ chromeTempRoot: process.env.TMPDIR });

let launchError;
let originalSetTimeout;
try {
  try {
    await hostile.launch();
  } catch (error) {
    launchError = error;
  }
  assert(launchError, "hostile Chromium launch unexpectedly succeeded");
  assert.match(hostile.stderr, /Socket path too long/);
  assert.equal(hostile.failure, launchError, "Chromium launch did not retain its root failure");
  originalSetTimeout = globalThis.setTimeout;
  globalThis.setTimeout = () => {
    throw new Error("late CDP event allocated a timer after fatal failure");
  };
  await assert.rejects(
    hostile.waitForEvent("Page.loadEventFired", hostile.pageSession),
    (error) => error === launchError,
  );
  await assert.rejects(hostile.navigate("about:blank"), (error) => error === launchError);
} finally {
  if (originalSetTimeout) globalThis.setTimeout = originalSetTimeout;
  await hostile.close();
}
assert.equal(hostile.pending.size, 0, "failed Chromium retained pending CDP commands");
assert.equal(hostile.sessions.size, 0, "failed Chromium retained pending CDP events");

try {
  await Promise.all(drivers.map((driver) => driver.launch()));
  const profiles = drivers.map((driver) => driver.userDataDir);
  if (new Set(profiles).size !== count) throw new Error("Chromium profiles are not isolated");
  await Promise.all(drivers.map((driver) => driver.send("Browser.getVersion")));

  const peerDriver = drivers[0];
  await peerDriver.send("Runtime.addBinding", { name: "jetLifecycleProbe" }, peerDriver.pageSession);
  const timedOutPeer = peerDriver.waitForEvent("Runtime.bindingCalled", peerDriver.pageSession, 0);
  const survivingPeer = peerDriver.waitForEvent("Runtime.bindingCalled", peerDriver.pageSession);
  await assert.rejects(timedOutPeer, /CDP event timeout: Runtime\.bindingCalled/);
  await peerDriver.evaluate("jetLifecycleProbe('peer')");
  assert.equal((await survivingPeer).payload, "peer");
  await assert.rejects(
    peerDriver.waitForEvent("Jet.lifecycleTimeout", undefined, 0),
    /CDP event timeout: Jet\.lifecycleTimeout/,
  );
  assert.equal(peerDriver.sessions.size, 0, "timed-out CDP events retained an empty session key");

  const pendingEvents = Promise.allSettled(
    drivers.map((driver) => driver.waitForEvent("Jet.lifecycleProbe")),
  );
  await Promise.all(drivers.map((driver) => driver.close()));
  const eventResults = await pendingEvents;
  if (eventResults.some((result) => result.status !== "rejected")) {
    throw new Error("closing Chromium did not reject pending CDP events");
  }
} finally {
  await Promise.allSettled(drivers.map((driver) => driver.close()));
}

for (const driver of [...drivers, hostile]) {
  if (existsSync(driver.userDataDir)) throw new Error(`profile was not removed: ${driver.userDataDir}`);
  if (driver.child && driver.child.exitCode === null && driver.child.signalCode === null) {
    throw new Error(`Chromium child was not reaped: ${driver.child.pid}`);
  }
}
console.log(`PASS ${count} isolated Chromium lifecycles`);

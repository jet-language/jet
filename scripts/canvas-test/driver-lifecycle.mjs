#!/usr/bin/env node
import { existsSync } from "node:fs";
import { CdpDriver } from "./driver.mjs";

const count = Number(process.argv[2] || "4");
const drivers = Array.from({ length: count }, () => new CdpDriver());

try {
  await Promise.all(drivers.map((driver) => driver.launch()));
  const profiles = drivers.map((driver) => driver.userDataDir);
  if (new Set(profiles).size !== count) throw new Error("Chromium profiles are not isolated");
  await Promise.all(drivers.map((driver) => driver.send("Browser.getVersion")));
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

for (const driver of drivers) {
  if (existsSync(driver.userDataDir)) throw new Error(`profile was not removed: ${driver.userDataDir}`);
  if (driver.child && driver.child.exitCode === null && driver.child.signalCode === null) {
    throw new Error(`Chromium child was not reaped: ${driver.child.pid}`);
  }
}
console.log(`PASS ${count} isolated Chromium lifecycles`);

#!/usr/bin/env node
import { GeckoDriver } from "./driver.mjs";

const driver = await new GeckoDriver().launch();
if (!driver.processGroup || !driver.child?.pid) {
  await driver.close();
  throw new Error("Gecko lifecycle probe requires an owned Unix process group");
}
const group = driver.child.pid;
driver.baseUrl = "http://127.0.0.1:1";
let cleanupError = null;
try {
  await driver.close();
} catch (error) {
  cleanupError = error;
}
if (!cleanupError) throw new Error("failed WebDriver session deletion was not propagated");
try {
  process.kill(-group, 0);
  throw new Error(`Gecko process group ${group} survived failed session deletion`);
} catch (error) {
  if (error.code !== "ESRCH") throw error;
}
console.log("PASS failed Gecko session deletion leaves no browser process group");

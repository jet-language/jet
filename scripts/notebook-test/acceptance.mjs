#!/usr/bin/env node

import { createDriver } from "../canvas-test/driver.mjs";

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

const browser = argument("--browser") || "chromium";
const port = argument("--port");
const token = argument("--token");
const savePath = argument("--save-path");
const mergePath = argument("--merge-path");
if (!port || !token || !savePath || !mergePath) {
  throw new Error("usage: acceptance.mjs --browser BROWSER --port PORT --token TOKEN --save-path PATH --merge-path PATH");
}

const options = {};
if (process.env.NOTEBOOK_CHROMIUM) options.chrome = process.env.NOTEBOOK_CHROMIUM;
if (process.env.NOTEBOOK_FIREFOX) options.firefox = process.env.NOTEBOOK_FIREFOX;
if (process.env.NOTEBOOK_GECKODRIVER) options.geckodriver = process.env.NOTEBOOK_GECKODRIVER;
const driver = createDriver(browser, options);

const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
async function evaluate(expression) {
  return await driver.evaluate(expression);
}

async function waitFor(check, label) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    try {
      if (await check()) return;
    } catch {
      // The page is still rendering or an async request is still in flight.
    }
    await pause(80);
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function click(selector) {
  const encoded = JSON.stringify(selector);
  await evaluate(`(() => { const node = document.querySelector(${encoded}); if (!node) throw new Error(${JSON.stringify(`missing ${selector}`)}); node.click(); return true; })()`);
}

async function setValue(selector, value) {
  const encodedSelector = JSON.stringify(selector);
  const encodedValue = JSON.stringify(value);
  await evaluate(`(() => { const node = document.querySelector(${encodedSelector}); if (!node) throw new Error(${JSON.stringify(`missing ${selector}`)}); node.value = ${encodedValue}; node.dispatchEvent(new Event('input', {bubbles:true})); node.dispatchEvent(new Event('change', {bubbles:true})); node.blur(); return true; })()`);
}

async function status() {
  return await evaluate("document.querySelector('#status')?.textContent || ''");
}

async function outputText() {
  return await evaluate("Array.from(document.querySelectorAll('.output pre')).map(node => node.textContent).join('\\n')");
}

async function setCellSource(index, source) {
  const encodedSource = JSON.stringify(source);
  await evaluate(`(() => { const area = document.querySelectorAll('#cells .cell textarea')[${index}]; if (!area) throw new Error('missing cell source'); area.value = ${encodedSource}; area.dispatchEvent(new Event('input', {bubbles:true})); area.blur(); return true; })()`);
}

async function clickCellAction(label) {
  const encoded = JSON.stringify(label);
  await evaluate(`(() => { const button = Array.from(document.querySelectorAll('#cells .cell:first-child button')).find(node => node.textContent === ${encoded}); if (!button) throw new Error(${JSON.stringify(`missing cell action ${label}`)}); button.click(); return true; })()`);
}

async function queueInput(value) {
  await setValue("#stdin", value);
  await click("#send-stdin");
  await waitFor(async () => (await status()).includes("input queued"), "queued input");
}

async function runClient(client) {
  await setValue("#client", client);
  await queueInput("Ada");
  await clickCellAction("Run");
  await waitFor(async () => (await status()).includes("ran="), `${client} run`);
  await waitFor(async () => {
    const text = await outputText();
    return text.includes("ambient-eprint") && text.includes("Ada");
  }, `${client} output`);
}

const source = `#Grant(caps: IO, FS) {
    eprint("ambient-eprint")
    name :: input("name: ") ?? "fallback"
    assert(name == "Ada")
    write_file("browser-notebook.txt", name) ?? panic("write failed")
    assert(file_exists("browser-notebook.txt"))
    assert_eq(file_exists("browser-notebook.txt"), true)
    print(read_file(Path.from("browser-notebook.txt")) ?? panic("read failed"))
}`;

try {
  await driver.launch();
  await driver.navigate(`http://127.0.0.1:${port}/#token=${encodeURIComponent(token)}`);
  await waitFor(async () => (await evaluate("document.title")) === "Jet notebook", "notebook page");
  await waitFor(async () => (await status()).includes("0 cells"), "initial notebook state");

  await click("#new-jet");
  await waitFor(async () => (await evaluate("document.querySelectorAll('#cells .cell').length")) === 1, "Jet cell");
  await setCellSource(0, source);
  await runClient("first-party");
  await runClient("canvas");
  await runClient("jupyter");

  await clickCellAction("Inspect");
  await waitFor(async () => (await status()).includes("inspected="), "inspect action");
  await clickCellAction("Debug");
  await waitFor(async () => (await status()).includes("inspected="), "debug action");
  await queueInput("Ada");
  await clickCellAction("Profile");
  await waitFor(async () => (await status()).includes("profiled="), "profile action");
  await click("#interrupt");
  await waitFor(async () => (await status()).includes("interrupt requested"), "interrupt action");

  await click("#new-md");
  await waitFor(async () => (await evaluate("document.querySelectorAll('#cells .cell').length")) === 2, "Markdown cell");
  await setCellSource(1, "# First-hour result\nThe shared session ran this source.");

  await setValue("#path", savePath);
  await click("#save");
  await waitFor(async () => (await status()).includes("saved="), "save");
  await click("#open");
  await waitFor(async () => (await status()).includes("opened"), "open");
  await click("#reopen");
  await waitFor(async () => (await status()).includes("reopened"), "reopen");
  await waitFor(async () => (await evaluate("document.querySelectorAll('#cells .cell').length")) === 2, "reopened cells");
  await waitFor(async () => (await evaluate("document.querySelectorAll('#cells textarea')[0]?.value || ''")) === source, "reopened source");

  await setValue("#path", mergePath);
  await click("#merge");
  await waitFor(async () => (await status()).includes("merged by stable cell ID"), "merge");
  await waitFor(async () => (await evaluate("document.body.textContent.includes('merged from another document')")), "merged Markdown");

  await click("#export-ipynb");
  await waitFor(async () => { const text = await status(); return text.includes("loss") || text.includes("no loss"); }, "ipynb export");
  await click("#export-jet");
  await waitFor(async () => { const text = await status(); return text.includes("loss") || text.includes("no loss"); }, "Jet export");

  console.log(`PASS notebook browser matrix (${browser})`);
} finally {
  await driver.close();
}

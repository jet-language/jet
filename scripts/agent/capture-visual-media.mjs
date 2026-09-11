#!/usr/bin/env node

import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { CdpDriver } from '../canvas-test/driver.mjs';

const ROOT = resolve(dirname(new URL(import.meta.url).pathname), '../..');
const MEDIA = resolve(ROOT, 'docs/proposals/visual-acceptance/media');
const defaults = [
  {
    source: 'docs/proposals/prototypes/devtools-ux/D-pill-lens-workbench.html',
    output: 'devtools-browser.png',
  },
  {
    source: 'docs/proposals/automatic-build-optimization/mockups/terminal.html',
    output: 'build-terminal.png',
    prepare: `document.querySelector('[data-state="tty-explain"]')?.click()`,
  },
  {
    source: 'docs/proposals/automatic-build-optimization/mockups/report.html',
    output: 'build-report.png',
  },
  {
    source: 'docs/proposals/prototypes/ballot-surface.html',
    output: 'tower-ballot.png',
    height: 1800,
  },
];

function args(argv) {
  const out = { width: 1440, height: 1000, source: null, output: null, exerciseDialog: false };
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (key === '--page') { out.source = value; i += 1; }
    else if (key === '--output') { out.output = value; i += 1; }
    else if (key === '--width') { out.width = Number(value); i += 1; }
    else if (key === '--height') { out.height = Number(value); i += 1; }
    else if (key === '--exercise-dialog') out.exerciseDialog = true;
    else throw new Error(`unknown or incomplete option: ${key}`);
  }
  if (!!out.source !== !!out.output) throw new Error('--page and --output must be used together');
  if (!Number.isInteger(out.width) || out.width < 360 || !Number.isInteger(out.height) || out.height < 300)
    throw new Error('capture dimensions must be integers at least 360×300');
  return out;
}

async function capture(driver, item, width, height, exerciseDialog) {
  width = item.width || width;
  height = item.height || height;
  const source = resolve(ROOT, item.source);
  const output = resolve(item.output.includes('/') ? ROOT : MEDIA, item.output);
  await mkdir(dirname(output), { recursive: true });
  await driver.send('Emulation.setDeviceMetricsOverride', {
    width, height, deviceScaleFactor: 1, mobile: false,
  }, driver.pageSession);
  await driver.send('Page.addScriptToEvaluateOnNewDocument', { source: `(() => {
    window.__captureErrors = [];
    addEventListener('error', event => window.__captureErrors.push(String(event.error || event.message)));
    addEventListener('unhandledrejection', event => window.__captureErrors.push(String(event.reason)));
    const originalError = console.error;
    console.error = (...values) => { window.__captureErrors.push(values.map(String).join(' ')); originalError(...values); };
  })()` }, driver.pageSession);
  await driver.navigate(pathToFileURL(source).href);
  await driver.evaluate(`document.fonts.ready.then(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))`);
  if (item.prepare) await driver.evaluate(item.prepare);
  await driver.evaluate(`
    const captureStyle = document.createElement('style');
    captureStyle.textContent = '*,*::before,*::after{animation:none!important;transition:none!important}';
    document.head.append(captureStyle);
    document.querySelectorAll('.reveal, section').forEach(node => node.classList.add('is-visible'));
    scrollTo(0, 0);
  `);
  const viewport = await driver.evaluate(`({ width: innerWidth, scrollWidth: document.documentElement.scrollWidth })`);
  if (viewport.scrollWidth > viewport.width) throw new Error(`${item.source}: horizontal overflow ${viewport.scrollWidth}px > ${viewport.width}px`);
  if (exerciseDialog) {
    const opened = await driver.evaluate(`document.querySelector('.media-button')?.click(); document.querySelector('dialog')?.open === true`);
    if (!opened) throw new Error(`${item.source}: dialog did not open`);
    await driver.send('Input.dispatchKeyEvent', {
      type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27,
    }, driver.pageSession);
    await driver.send('Input.dispatchKeyEvent', {
      type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27,
    }, driver.pageSession);
    await driver.evaluate(`new Promise(resolve => requestAnimationFrame(resolve))`);
    const escaped = await driver.evaluate(`document.querySelector('dialog')?.open === false`);
    if (!escaped) throw new Error(`${item.source}: Escape did not close dialog`);
    const closed = await driver.evaluate(`document.querySelector('.media-button')?.click(); document.querySelector('[data-close]')?.click(); document.querySelector('dialog')?.open === false`);
    if (!closed) throw new Error(`${item.source}: close button did not close dialog`);
  }
  const result = await driver.send('Page.captureScreenshot', {
    format: 'png', fromSurface: true, captureBeyondViewport: false,
  }, driver.pageSession);
  await writeFile(output, Buffer.from(result.data, 'base64'));
  const errors = await driver.evaluate('window.__captureErrors || []');
  if (errors.length) throw new Error(`${item.source}: console errors:\n${errors.join('\n')}`);
  process.stdout.write(`${item.output} ${width}x${height} <- ${item.source}; overflow OK; console errors 0${exerciseDialog ? '; dialog Escape/close OK' : ''}\n`);
}

const options = args(process.argv.slice(2));
const items = options.source
  ? [{ source: options.source, output: options.output }]
  : defaults;
const driver = await new CdpDriver({ chrome: process.env.CHROMIUM || 'chromium' }).launch();
try {
  for (const item of items) await capture(driver, item, options.width, options.height, options.exerciseDialog);
} finally {
  await driver.close();
}

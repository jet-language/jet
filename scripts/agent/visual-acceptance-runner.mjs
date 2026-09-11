#!/usr/bin/env node

// Canonical visual-acceptance entry point. It validates the durable gallery
// model and can refresh its prototype captures; it never generates owner
// instructions or launches product programs for the owner.

import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, extname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const DEFAULT_MANIFEST = 'scripts/agent/visual-acceptance-manifest.json';
const SCHEMA = 'jet.visual-acceptance.media.v1';
const MEDIA_ROOT = 'docs/proposals/visual-acceptance/media/';
const IMAGE_EXT = new Set(['.png', '.jpg', '.jpeg', '.webp', '.gif']);
const VIDEO_EXT = new Set(['.webm', '.mp4']);

function usage(message) {
  if (message) process.stderr.write(`${message}\n\n`);
  process.stderr.write('Usage: visual-acceptance-runner.mjs [--check|--capture] [--json]\n');
  process.exit(message ? 2 : 0);
}

function args(argv) {
  const options = { mode: 'check', json: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--check' || arg === '--capture') options.mode = arg.slice(2);
    else if (arg === '--json') options.json = true;
    else if (arg === '--help' || arg === '-h') usage();
    else usage(`Unknown option: ${arg}`);
  }
  return options;
}

function safeRepoPath(path) {
  return typeof path === 'string' && path.length > 0 && !isAbsolute(path)
    && !path.includes('\\') && !path.includes('\0') && !/^[a-z][a-z0-9+.-]*:/i.test(path)
    && !/[?#%]/.test(path) && !path.split('/').some(part => part === '.' || part === '..');
}

function validate(manifest) {
  const errors = [];
  const needFile = (path, label) => {
    if (!safeRepoPath(path)) errors.push(`${label} must be a safe repo-relative path`);
    else if (!existsSync(join(ROOT, path))) errors.push(`${label} does not exist: ${path}`);
  };
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) errors.push('manifest must be an object');
  if (manifest?.schema !== SCHEMA) errors.push(`schema must be ${SCHEMA}`);
  needFile(manifest?.gallery, 'gallery');
  needFile(manifest?.captureScript, 'captureScript');
  if (manifest?.captureScript !== 'scripts/agent/capture-visual-media.mjs') errors.push('captureScript must name the canonical capture helper');
  if (!Array.isArray(manifest?.visualMedia) || !manifest.visualMedia.length) errors.push('visualMedia must be a non-empty array');
  const seen = new Set();
  for (const [index, item] of (manifest?.visualMedia || []).entries()) {
    const at = `visualMedia[${index}]`;
    if (!item || typeof item !== 'object' || Array.isArray(item)) { errors.push(`${at} must be an object`); continue; }
    if (!['image', 'video'].includes(item.kind)) errors.push(`${at}.kind must be image or video`);
    if (!safeRepoPath(item.path) || !item.path.startsWith(MEDIA_ROOT)) errors.push(`${at}.path must be below ${MEDIA_ROOT}`);
    else {
      const extensions = item.kind === 'video' ? VIDEO_EXT : IMAGE_EXT;
      if (!extensions.has(extname(item.path).toLowerCase())) errors.push(`${at}.path has the wrong media extension`);
      needFile(item.path, `${at}.path`);
      if (seen.has(item.path)) errors.push(`${at}.path is duplicated`);
      seen.add(item.path);
    }
    if (typeof item.alt !== 'string' || item.alt.trim().length < 8) errors.push(`${at}.alt must meaningfully describe the media`);
    if (typeof item.caption !== 'string' || !item.caption.trim()) errors.push(`${at}.caption is required`);
    if (item.state != null && (typeof item.state !== 'string' || !item.state.trim())) errors.push(`${at}.state must be a non-empty string`);
    if (item.poster != null) {
      if (item.kind !== 'video') errors.push(`${at}.poster is only valid for video`);
      if (!safeRepoPath(item.poster) || !item.poster.startsWith(MEDIA_ROOT) || !IMAGE_EXT.has(extname(item.poster).toLowerCase())) errors.push(`${at}.poster must be an image below ${MEDIA_ROOT}`);
      else needFile(item.poster, `${at}.poster`);
    }
  }
  for (const [index, source] of (manifest?.sources || []).entries()) {
    needFile(source?.source, `sources[${index}].source`);
    if (!seen.has(source?.media)) errors.push(`sources[${index}].media does not name visualMedia`);
  }
  return errors;
}

const options = args(process.argv.slice(2));
let manifest;
try { manifest = JSON.parse(readFileSync(join(ROOT, DEFAULT_MANIFEST), 'utf8')); }
catch (error) { process.stderr.write(`Cannot read manifest: ${error.message}\n`); process.exit(1); }

if (options.mode === 'capture') {
  const result = spawnSync(process.execPath, [join(ROOT, 'scripts/agent/capture-visual-media.mjs')], { cwd: ROOT, stdio: 'inherit' });
  if (result.error) { process.stderr.write(`${result.error.message}\n`); process.exit(1); }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

const errors = validate(manifest);
if (errors.length) {
  process.stderr.write(`${errors.map(error => `- ${error}`).join('\n')}\n`);
  process.exit(1);
}

const result = { gallery: manifest.gallery, visualMedia: manifest.visualMedia };
process.stdout.write(options.json ? `${JSON.stringify(result, null, 2)}\n` : `Visual acceptance gallery: ${manifest.gallery}\nCaptured evidence: ${manifest.visualMedia.length} items\n`);

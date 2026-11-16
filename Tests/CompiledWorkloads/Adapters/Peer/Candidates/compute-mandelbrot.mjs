import fs from "node:fs";
const raw = fs.readFileSync(process.argv[2], "utf8");
function reject(label, named) {
  return `${named ? `case=${label}\n` : ""}reject=invalid-number\nsamples=0\nchecksum=0\n`;
}
function runCase(label, named, values) {
  if (!values || ["width", "height", "stride", "iterations"].some(key => !Object.hasOwn(values, key))) return reject(label, named);
  const { width, height, stride, iterations } = values;
  if (![width, height, stride, iterations].every(Number.isInteger) || width <= 0 || width > 16000 || height <= 0 || height > 16000 || stride <= 0 || stride > 16000 || iterations <= 0 || iterations > 64) return reject(label, named);
  const sx = Math.ceil(width / stride);
  const sy = Math.ceil(height / stride);
  if (sx <= 0 || sy <= 0 || sx > Math.floor(25000000 / sy)) return reject(label, named);
  let samples = 0;
  let checksum = 0;
  for (let y = 0; y < height; y += stride) {
    const cy = (y / height) * 2 - 1;
    for (let x = 0; x < width; x += stride) {
      const cx = (x / width) * 3.5 - 2.5;
      let zx = 0;
      let zy = 0;
      let count = 0;
      while (count < iterations) {
        const nextX = zx * zx - zy * zy + cx;
        const nextY = 2 * zx * zy + cy;
        zx = nextX;
        zy = nextY;
        count += 1;
        if (zx * zx + zy * zy > 4) break;
      }
      checksum = (checksum + count) % 1000000007;
      samples += 1;
    }
  }
  return `${named ? `case=${label}\n` : ""}samples=${samples}\nchecksum=${checksum}\n`;
}
function parse(lines, batch) {
  const values = {};
  let output = "";
  let label = "";
  let active = false;
  let failed = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || (batch && trimmed.startsWith("#"))) continue;
    const split = trimmed.split("=");
    if (split.length !== 2) { failed = true; continue; }
    const key = split[0].trim();
    const value = split[1].trim();
    if (batch && key === "case") {
      if (active) output += runCase(label, true, failed ? null : values);
      label = value;
      Object.keys(values).forEach(name => delete values[name]);
      failed = !label;
      active = true;
      continue;
    }
    if (batch && !active) { failed = true; continue; }
    if (!["width", "height", "stride", "iterations"].includes(key) || !/^-?[0-9]+$/.test(value) || Object.hasOwn(values, key)) {
      failed = true;
      continue;
    }
    values[key] = Number(value);
  }
  if (batch) return active ? output + runCase(label, true, failed ? null : values) : reject("batch", true);
  return runCase("", false, failed ? null : values);
}
process.stdout.write(parse(raw.split(/\r?\n/), raw.includes("case=")));

import { readdir, rename } from "node:fs/promises";

const directory = process.argv[2] ?? "photos";
const pattern = /^IMG_(\d+)\.jpeg$/;
const entries = await readdir(directory, { withFileTypes: true });
const files = entries.filter((entry) => entry.isFile());
const photos = files
  .map((entry) => {
    const match = pattern.exec(entry.name);
    return match ? { name: entry.name, number: Number(match[1]) } : null;
  })
  .filter(Boolean)
  .sort((a, b) => a.number - b.number || a.name.localeCompare(b.name));
for (const [index, file] of photos.entries()) {
  const destination = `photo-${String(index + 1).padStart(4, "0")}.jpg`;
  await rename(`${directory}/${file.name}`, `${directory}/${destination}`);
  console.log(`renamed ${file.name} -> ${destination}`);
}
console.log(`renamed ${photos.length} skipped ${files.length - photos.length}`);

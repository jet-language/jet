import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";

if (process.argv.length !== 3) {
  throw new Error("usage: media_asset_inventory INPUT_ROOT");
}

const types = { ppm: "image/x-portable-pixmap", svg: "image/svg+xml", mp3: "audio/mpeg" };
function files(root) {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}

for (const path of files(process.argv[2]).sort()) {
  const name = relative(process.argv[2], path).replaceAll("\\", "/");
  const extension = name.split(".").pop();
  const mediaType = types[extension];
  if (!mediaType) {
    console.log(`reject|${name}|extension`);
  } else {
    console.log(`asset|${name}|${mediaType}|${readFileSync(path).byteLength}`);
  }
}

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

export function preparedFiles(directory) {
  const files = [];
  function visit(relative) {
    for (const entry of fs.readdirSync(path.join(directory, relative), {withFileTypes: true}).sort((a, b) => a.name.localeCompare(b.name))) {
      const name = path.join(relative, entry.name);
      if (entry.isDirectory()) visit(name);
      else if (entry.isFile()) files.push({path: name, sha256: crypto.createHash('sha256').update(fs.readFileSync(path.join(directory, name))).digest('hex')});
      else throw Error(`prepared corpus contains a non-file input: ${name}`);
    }
  }
  visit(''); return files;
}
export function consumePrepared(source, destination, manifest) {
  if (!Array.isArray(manifest) || !manifest.length || JSON.stringify(preparedFiles(source)) !== JSON.stringify(manifest)) throw Error('prepared corpus inventory or digest differs');
  if (fs.existsSync(destination)) throw Error('prepared corpus destination already exists');
  fs.cpSync(source, destination, {recursive: true});
  if (JSON.stringify(preparedFiles(destination)) !== JSON.stringify(manifest)) throw Error('prepared corpus copy differs');
  return manifest;
}

// A page whose browser has a directory picker and IndexedDB. The person
// picks the fixture's exports folder, whose handle reads and writes the
// folder on disk, each answer coming later, as a browser's does. The
// page's IndexedDB keeps each handle's folder in folders.json beside the
// page, so a later visit finds it; a handle from an earlier visit asks the
// person again (queryPermission is `prompt`), who allows it. `node page.mjs
// other` is a browser with neither, as on another device.
import fs from "node:fs";
import path from "node:path";

const other = process.argv[2] === "other";
const stored = "folders.json";
const later = (value) => new Promise((resolve) => setTimeout(() => resolve(value), 10));

class FileHandle {
  constructor(at) {
    this.kind = "file";
    this.name = path.basename(at);
    this.at = at;
  }
  getFile() {
    return later(new File([fs.readFileSync(this.at)], this.name));
  }
  // The browser's writable: the bytes go to a copy beside the file, which
  // replaces it only on close.
  createWritable() {
    const parts = [];
    return later({
      write: async (bytes) => parts.push(Buffer.from(bytes)),
      close: async () => {
        fs.writeFileSync(`${this.at}.crswap`, Buffer.concat(parts));
        fs.renameSync(`${this.at}.crswap`, this.at);
      },
      abort: async () => {},
    });
  }
}

class FolderHandle {
  constructor(at, picked) {
    this.kind = "directory";
    this.name = path.basename(at);
    this.at = at;
    this.picked = picked;
  }
  async *values() {
    for (const name of fs.readdirSync(this.at)) {
      const at = path.join(this.at, name);
      yield fs.statSync(at).isDirectory() ? new FolderHandle(at, true) : new FileHandle(at);
    }
  }
  getFileHandle(name) {
    return later(new FileHandle(path.join(this.at, name)));
  }
  queryPermission() {
    return later(this.picked ? "granted" : "prompt");
  }
  requestPermission() {
    console.log(`the page asks again for ${this.name}`);
    return later("granted");
  }
}

// IndexedDB's requests, each answered later: a store's put keeps a
// handle's folder under its key, and its get gives a handle from an
// earlier visit.
const answer = (request, result, then) =>
  setTimeout(() => {
    request.result = result;
    request.onsuccess?.();
    then?.();
  }, 10);
const indexedDB = {
  open() {
    const opening = {};
    answer(opening, {
      transaction() {
        const transaction = {};
        const folders = fs.existsSync(stored) ? JSON.parse(fs.readFileSync(stored, "utf8")) : {};
        const done = () => transaction.oncomplete?.();
        transaction.objectStore = () => ({
          put(folder, key) {
            const request = {};
            folders[key] = folder.at;
            fs.writeFileSync(stored, JSON.stringify(folders));
            answer(request, key, done);
            return request;
          },
          get(key) {
            const request = {};
            answer(request, folders[key] === undefined ? undefined : new FolderHandle(folders[key], false), done);
            return request;
          },
        });
        return transaction;
      },
      close() {},
    });
    return opening;
  },
};

if (!other) {
  globalThis.showDirectoryPicker = () => later(new FolderHandle(path.resolve("../../exports"), true));
  globalThis.indexedDB = indexedDB;
}
globalThis.document = { addEventListener: () => {} };
const { jet_main } = await import("./app.js");
await jet_main();

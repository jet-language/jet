// A page with no person: the document hands the web host's default provider
// a link to click, and the click records the download it starts, the name and
// the file's bytes, as a browser would save them.
const downloads = [];
const files = new Map();
URL.createObjectURL = (blob) => {
  const address = "blob:jet/" + files.size;
  files.set(address, blob);
  return address;
};
URL.revokeObjectURL = (address) => files.delete(address);
globalThis.document = {
  addEventListener: () => {},
  documentElement: { append: () => {} },
  createElement: (tag) => ({
    tag,
    href: "",
    download: "",
    click() {
      if (this.tag === "a" && this.download !== "") downloads.push({ name: this.download, file: files.get(this.href) });
    },
    remove: () => {},
  }),
};
const { jet_main } = await import("./app.js");
await jet_main();
for (const { name, file } of downloads) {
  process.stdout.write("downloaded: " + name + " " + file.type + "\n" + (await file.text()));
}
// The provider revokes the file's address a minute later; nothing is left to wait for.
process.exit(0);

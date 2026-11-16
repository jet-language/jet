// At most 32 requests in flight: 32 workers drain one shared index.
const codes = new Array(100);
let next = 0;
async function worker() {
  while (next < codes.length) {
    const i = next++;
    const resp = await fetch(`http://127.0.0.1:18411/item/${i}`);
    await resp.arrayBuffer();
    codes[i] = resp.status;
  }
}
await Promise.all(Array.from({ length: 32 }, worker));
console.log(`ok=${codes.filter((code) => code === 200).length}`);

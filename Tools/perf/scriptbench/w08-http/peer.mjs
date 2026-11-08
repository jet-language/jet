let total = 0;
for (let i = 0; i < 200; i++) {
  const resp = await fetch(`http://127.0.0.1:18080/item/${i}`);
  total += (await resp.arrayBuffer()).byteLength;
}
console.log("requests 200");
console.log(`bytes ${total}`);

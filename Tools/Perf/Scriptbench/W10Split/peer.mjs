const lines = [];
for (let i = 0; i < 20000; i++) lines.push(`alpha beta gamma delta epsilon ${i}`);
const text = lines.join("\n");
let count = 0;
let long = 0;
let bytes = 0;
for (const line of text.split("\n")) {
  for (const word of line.split(" ")) {
    count += 1;
    bytes += word.length;
    if (word.length > 4) long += 1;
  }
}
console.log(`words=${count} long=${long} bytes=${bytes}`);

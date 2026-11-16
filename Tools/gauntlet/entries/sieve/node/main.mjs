const limit = Number(process.argv[2] ?? 10000000);
const prime = new Uint8Array(limit);
prime.fill(1);
if (limit > 0) prime[0] = 0;
if (limit > 1) prime[1] = 0;
for (let candidate = 2; candidate * candidate < limit; candidate += 1) {
  if (!prime[candidate]) continue;
  for (let multiple = candidate * candidate; multiple < limit; multiple += candidate) prime[multiple] = 0;
}
let count = 0;
let largest = 0;
for (let value = 2; value < limit; value += 1) {
  if (!prime[value]) continue;
  count += 1;
  largest = value;
}
console.log(`count ${count}`);
console.log(`largest ${largest}`);

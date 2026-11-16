import { triangular } from "./main.mjs";

let sum = 0;
for (let i = 1; i <= 12; i += 1) {
  const value = triangular(i);
  console.log(`t${i} ${value}`);
  sum += value;
}
console.log(`sum ${sum}`);

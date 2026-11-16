const SIDE = 4096;
const ELEMENTS = SIDE * SIDE;

const left = new Float64Array(ELEMENTS);
const right = new Float64Array(ELEMENTS);
left.fill(1.0);
right.fill(2.0);

const mapped = new Float64Array(ELEMENTS);
for (let i = 0; i < ELEMENTS; i += 1) mapped[i] = left[i] + right[i];

const corner = mapped[ELEMENTS - 1];
let checksum = 0.0;
for (const value of mapped) checksum += value;
if (corner !== 3.0 || checksum !== 50331648.0) throw new Error("tensor map validation failed");
console.log(`shape:[4096, 4096] numel:${ELEMENTS} corner:${corner.toFixed(1)} checksum:${checksum}`);

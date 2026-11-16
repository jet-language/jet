import fs from "node:fs";

const raw = fs.readFileSync(process.argv[2], "utf8");
const capacity = 4;
const modulus = 1_000_003;
const queue = [];
let head = 0;
let tail = 0;
let acceptedEnqueues = 0;
let acceptedDequeues = 0;
let rejects = 0;
let digest = 17;
const output = [`capacity=${capacity}`];

function foldDigest(step, code, value) {
  const valuePart = ((value % modulus) + modulus) % modulus;
  digest = (digest * 131 + step * 17 + code * 257 + valuePart * 3 +
    head * 5 + tail * 7 + queue.length * 11) % modulus;
}

function state(step, operation, result, value) {
  const valuePart = value === undefined ? "" : `|value=${value}`;
  output.push(`step=${step}|op=${operation}${valuePart}|result=${result}|head=${head}|tail=${tail}|count=${queue.length}`);
}

let step = 0;
for (const line of raw.split(/\r?\n/)) {
  if (line.length === 0 || line.startsWith("#")) continue;
  step += 1;
  const fields = line.split("|");
  const operation = fields[0];
  const valueText = fields[1];
  const hasExtra = fields.length > 2;
  if (!hasExtra && operation === "ENQ") {
    const value = valueText === undefined || !/^-?\d+$/.test(valueText) ? undefined : Number(valueText);
    if (value === undefined || !Number.isSafeInteger(value)) {
      rejects += 1;
      foldDigest(step, 5, 0);
      state(step, "malformed", "reject-malformed");
    } else if (queue.length >= capacity) {
      rejects += 1;
      foldDigest(step, 3, value);
      state(step, "enqueue", "reject-overflow", value);
    } else {
      queue.push(value);
      tail = (tail + 1) % capacity;
      acceptedEnqueues += 1;
      foldDigest(step, 1, value);
      state(step, "enqueue", "accepted", value);
    }
  } else if (!hasExtra && operation === "DEQ" && valueText === undefined) {
    if (queue.length === 0) {
      rejects += 1;
      foldDigest(step, 4, 0);
      state(step, "dequeue", "reject-underflow");
    } else {
      const value = queue.shift();
      head = (head + 1) % capacity;
      acceptedDequeues += 1;
      foldDigest(step, 2, value);
      state(step, "dequeue", "accepted", value);
    }
  } else {
    rejects += 1;
    foldDigest(step, 5, 0);
    state(step, "malformed", "reject-malformed");
  }
}

output.push(`summary|accepted_enqueues=${acceptedEnqueues}|accepted_dequeues=${acceptedDequeues}|rejects=${rejects}|head=${head}|tail=${tail}|count=${queue.length}`);
output.push(`digest=${digest}`);
process.stdout.write(output.join("\n") + "\n");

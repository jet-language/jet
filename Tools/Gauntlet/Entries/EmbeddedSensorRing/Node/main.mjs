const ticks = Number.parseInt(process.argv[2], 10);

function crc16(word) {
  let crc = 65535;
  let value = word;
  for (let bit = 0; bit < 16; bit += 1) {
    const top = Math.trunc(crc / 32768);
    const incoming = value % 2;
    crc = (crc * 2) % 65536;
    if ((top + incoming) % 2 === 1) crc = (crc + 4129) % 65536;
    value = Math.trunc(value / 2);
  }
  return crc;
}

function signedDiv8(value) {
  return value >= 0 ? Math.trunc(value / 8) : -Math.trunc(-value / 8);
}

function clamp(value, low, high) {
  return Math.max(low, Math.min(high, value));
}

const ring = Array(16).fill(0);
let ringPosition = 0;
let runningSum = 0;
let integrator = 0;
let actuator = 0;
let watchdog = 0;
let watchdogResets = 0;
let accepted = 0;
let rejected = 0;
let faults = 0;
let controlReg = 4095;
let statusReg = 0;
let mmioChecksum = 0;

for (let tick = 0; tick < ticks; tick += 1) {
  const raw = (tick * 73 + 19) % 4096;
  const frame = (raw * 17 + tick * 31 + 7) % 65536;
  const expectedCrc = crc16(frame);
  let receivedCrc = expectedCrc;
  if (tick % 127 === 0) receivedCrc = (expectedCrc + 1) % 65536;
  watchdog += 1;

  if (receivedCrc !== expectedCrc) {
    rejected += 1;
    faults += 1;
    statusReg = 2;
  } else {
    const sample = (raw * 3 + 11) % 4096;
    const old = ring[ringPosition];
    ring[ringPosition] = sample;
    ringPosition += 1;
    if (ringPosition === 16) ringPosition = 0;
    runningSum += sample - old;
    const mean = Math.trunc(runningSum / 16);
    const error = 2048 - mean;
    integrator += error;
    integrator = clamp(integrator, -8192, 8192);
    let command = error * 4 + signedDiv8(integrator);
    command = clamp(command, -4095, 4095);
    actuator = command;
    accepted += 1;
    statusReg = 1;
  }

  controlReg = actuator + 4095;
  mmioChecksum = (mmioChecksum * 33 + statusReg * 257 + controlReg + receivedCrc) % 1000000007;
  if (watchdog === 64) {
    watchdog = 0;
    watchdogResets += 1;
  }
}

let ringChecksum = 0;
for (const sample of ring) ringChecksum = (ringChecksum * 131 + sample) % 1000000007;
console.log(`ticks ${ticks}`);
console.log(`accepted ${accepted} rejected ${rejected}`);
console.log(`watchdog_resets ${watchdogResets} faults ${faults}`);
console.log(`actuator ${actuator}`);
console.log(`mmio_checksum ${mmioChecksum}`);
console.log(`ring_checksum ${ringChecksum}`);

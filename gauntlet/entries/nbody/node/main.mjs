const steps = Number(process.argv[2] ?? 50000);
const pi = 3.141592653589793;
const solarMass = 4 * pi * pi;
const daysPerYear = 365.24;
const bodies = [
  { x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, mass: solarMass },
  { x: 4.84143144246472090, y: -1.16032004402742839, z: -0.103622044471123109, vx: 0.00166007664274403694 * daysPerYear, vy: 0.00769901118419740425 * daysPerYear, vz: -0.0000690460016972063023 * daysPerYear, mass: 0.000954791938424326609 * solarMass },
  { x: 8.34336671824457987, y: 4.12479856412430479, z: -0.403523417114321381, vx: -0.00276742510726862411 * daysPerYear, vy: 0.00499852801234917238 * daysPerYear, vz: 0.0000230417297573763929 * daysPerYear, mass: 0.000285885980666130812 * solarMass },
  { x: 12.8943695621391310, y: -15.1111514016986312, z: -0.223307578892655734, vx: 0.00296460137564761618 * daysPerYear, vy: 0.00237847173959480950 * daysPerYear, vz: -0.0000296589568540237556 * daysPerYear, mass: 0.0000436624404335156298 * solarMass },
  { x: 15.3796971148509165, y: -25.9193146099879641, z: 0.179258772950371181, vx: 0.00268067772490389322 * daysPerYear, vy: 0.00162824170038242201 * daysPerYear, vz: -0.0000951592254519715870 * daysPerYear, mass: 0.0000515138902046611451 * solarMass },
];

let px = 0;
let py = 0;
let pz = 0;
for (const body of bodies.slice(1)) {
  px += body.vx * body.mass;
  py += body.vy * body.mass;
  pz += body.vz * body.mass;
}
bodies[0].vx = -px / solarMass;
bodies[0].vy = -py / solarMass;
bodies[0].vz = -pz / solarMass;

function energy() {
  let value = 0;
  for (const body of bodies) value += 0.5 * body.mass * (body.vx ** 2 + body.vy ** 2 + body.vz ** 2);
  for (let i = 0; i < bodies.length; i += 1) {
    for (let j = i + 1; j < bodies.length; j += 1) {
      const a = bodies[i];
      const b = bodies[j];
      const dx = a.x - b.x;
      const dy = a.y - b.y;
      const dz = a.z - b.z;
      value -= (a.mass * b.mass) / Math.sqrt(dx * dx + dy * dy + dz * dz);
    }
  }
  return value;
}

function advance() {
  for (let i = 0; i < bodies.length; i += 1) {
    for (let j = i + 1; j < bodies.length; j += 1) {
      const a = bodies[i];
      const b = bodies[j];
      const dx = a.x - b.x;
      const dy = a.y - b.y;
      const dz = a.z - b.z;
      const distance = Math.sqrt(dx * dx + dy * dy + dz * dz);
      const magnitude = 0.01 / (distance * distance * distance);
      a.vx -= dx * b.mass * magnitude;
      a.vy -= dy * b.mass * magnitude;
      a.vz -= dz * b.mass * magnitude;
      b.vx += dx * a.mass * magnitude;
      b.vy += dy * a.mass * magnitude;
      b.vz += dz * a.mass * magnitude;
    }
  }
  for (const body of bodies) {
    body.x += 0.01 * body.vx;
    body.y += 0.01 * body.vy;
    body.z += 0.01 * body.vz;
  }
}

console.log(energy().toFixed(9));
for (let i = 0; i < steps; i += 1) advance();
console.log(energy().toFixed(9));

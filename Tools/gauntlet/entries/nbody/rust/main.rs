use std::env;

#[derive(Clone, Copy)]
struct Body { x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, mass: f64 }

fn energy(bodies: &[Body]) -> f64 {
    let mut total = 0.0;
    for i in 0..bodies.len() {
        let b = bodies[i];
        total += 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz);
        for j in (i + 1)..bodies.len() {
            let other = bodies[j];
            let dx = b.x - other.x;
            let dy = b.y - other.y;
            let dz = b.z - other.z;
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            total -= b.mass * other.mass / distance;
        }
    }
    total
}

fn offset_momentum(bodies: &mut [Body]) {
    let (mut px, mut py, mut pz) = (0.0, 0.0, 0.0);
    for b in &bodies[1..] {
        px += b.vx * b.mass;
        py += b.vy * b.mass;
        pz += b.vz * b.mass;
    }
    bodies[0].vx = -px / bodies[0].mass;
    bodies[0].vy = -py / bodies[0].mass;
    bodies[0].vz = -pz / bodies[0].mass;
}

fn advance(bodies: &mut [Body], steps: usize) {
    let dt = 0.01;
    for _ in 0..steps {
        for i in 0..bodies.len() {
            for j in (i + 1)..bodies.len() {
                let dx = bodies[i].x - bodies[j].x;
                let dy = bodies[i].y - bodies[j].y;
                let dz = bodies[i].z - bodies[j].z;
                let distance_sq = dx * dx + dy * dy + dz * dz;
                let mag = dt / (distance_sq * distance_sq.sqrt());
                bodies[i].vx -= dx * bodies[j].mass * mag;
                bodies[i].vy -= dy * bodies[j].mass * mag;
                bodies[i].vz -= dz * bodies[j].mass * mag;
                bodies[j].vx += dx * bodies[i].mass * mag;
                bodies[j].vy += dy * bodies[i].mass * mag;
                bodies[j].vz += dz * bodies[i].mass * mag;
            }
        }
        for b in bodies.iter_mut() {
            b.x += dt * b.vx;
            b.y += dt * b.vy;
            b.z += dt * b.vz;
        }
    }
}

fn main() {
    let pi = std::f64::consts::PI;
    let solar_mass = 4.0 * pi * pi;
    let year = 365.24;
    let mut bodies = [
        Body { x: 0.0, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, mass: solar_mass },
        Body { x: 4.84143144246472090, y: -1.16032004402742839, z: -0.103622044471123109, vx: 0.00166007664274403694 * year, vy: 0.00769901118419740425 * year, vz: -0.0000690460016972063023 * year, mass: 0.000954791938424326609 * solar_mass },
        Body { x: 8.34336671824457987, y: 4.12479856412430479, z: -0.403523417114321381, vx: -0.00276742510726862411 * year, vy: 0.00499852801234917238 * year, vz: 0.0000230417297573763929 * year, mass: 0.000285885980666130812 * solar_mass },
        Body { x: 12.8943695621391310, y: -15.1111514016986312, z: -0.223307578892655734, vx: 0.00296460137564761618 * year, vy: 0.00237847173959480950 * year, vz: -0.0000296589568540237556 * year, mass: 0.0000436624404335156298 * solar_mass },
        Body { x: 15.3796971148509165, y: -25.9193146099879641, z: 0.179258772950371181, vx: 0.00268067772490389322 * year, vy: 0.00162824170038242201 * year, vz: -0.0000951592254519715870 * year, mass: 0.0000515138902046611451 * solar_mass },
    ];
    offset_momentum(&mut bodies);
    println!("{:.9}", energy(&bodies));
    let steps = env::args().nth(1).unwrap_or_else(|| "0".into()).parse().unwrap_or(0);
    advance(&mut bodies, steps);
    println!("{:.9}", energy(&bodies));
}

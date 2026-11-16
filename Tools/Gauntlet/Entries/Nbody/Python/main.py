import math
import sys

PI = 3.141592653589793
SOLAR_MASS = 4.0 * PI * PI
YEAR = 365.24

def energy(bodies):
    total = 0.0
    for i, b in enumerate(bodies):
        total += 0.5 * b[6] * (b[3] * b[3] + b[4] * b[4] + b[5] * b[5])
        for other in bodies[i + 1:]:
            dx = b[0] - other[0]
            dy = b[1] - other[1]
            dz = b[2] - other[2]
            distance = math.sqrt(dx * dx + dy * dy + dz * dz)
            total -= b[6] * other[6] / distance
    return total

def offset_momentum(bodies):
    px = py = pz = 0.0
    for b in bodies[1:]:
        px += b[3] * b[6]
        py += b[4] * b[6]
        pz += b[5] * b[6]
    bodies[0][3] = -px / bodies[0][6]
    bodies[0][4] = -py / bodies[0][6]
    bodies[0][5] = -pz / bodies[0][6]

def advance(bodies, steps):
    dt = 0.01
    for _ in range(steps):
        for i in range(len(bodies)):
            for j in range(i + 1, len(bodies)):
                dx = bodies[i][0] - bodies[j][0]
                dy = bodies[i][1] - bodies[j][1]
                dz = bodies[i][2] - bodies[j][2]
                distance_sq = dx * dx + dy * dy + dz * dz
                mag = dt / (distance_sq * math.sqrt(distance_sq))
                bodies[i][3] -= dx * bodies[j][6] * mag
                bodies[i][4] -= dy * bodies[j][6] * mag
                bodies[i][5] -= dz * bodies[j][6] * mag
                bodies[j][3] += dx * bodies[i][6] * mag
                bodies[j][4] += dy * bodies[i][6] * mag
                bodies[j][5] += dz * bodies[i][6] * mag
        for b in bodies:
            b[0] += dt * b[3]
            b[1] += dt * b[4]
            b[2] += dt * b[5]

bodies = [
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS],
    [4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694 * YEAR, 0.00769901118419740425 * YEAR, -0.0000690460016972063023 * YEAR, 0.000954791938424326609 * SOLAR_MASS],
    [8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411 * YEAR, 0.00499852801234917238 * YEAR, 0.0000230417297573763929 * YEAR, 0.000285885980666130812 * SOLAR_MASS],
    [12.8943695621391310, -15.1111514016986312, -0.223307578892655734, 0.00296460137564761618 * YEAR, 0.00237847173959480950 * YEAR, -0.0000296589568540237556 * YEAR, 0.0000436624404335156298 * SOLAR_MASS],
    [15.3796971148509165, -25.9193146099879641, 0.179258772950371181, 0.00268067772490389322 * YEAR, 0.00162824170038242201 * YEAR, -0.0000951592254519715870 * YEAR, 0.0000515138902046611451 * SOLAR_MASS],
]
offset_momentum(bodies)
print(f"{energy(bodies):.9f}")
advance(bodies, int(sys.argv[1]))
print(f"{energy(bodies):.9f}")

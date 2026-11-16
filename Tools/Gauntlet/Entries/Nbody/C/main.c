#include <math.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct { double x, y, z, vx, vy, vz, mass; } Body;

static double energy(const Body *bodies, int count) {
    double total = 0.0;
    for (int i = 0; i < count; ++i) {
        const Body b = bodies[i];
        total += 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz);
        for (int j = i + 1; j < count; ++j) {
            const Body other = bodies[j];
            const double dx = b.x - other.x;
            const double dy = b.y - other.y;
            const double dz = b.z - other.z;
            const double distance = sqrt(dx * dx + dy * dy + dz * dz);
            total -= b.mass * other.mass / distance;
        }
    }
    return total;
}

static void offset_momentum(Body *bodies) {
    double px = 0.0, py = 0.0, pz = 0.0;
    for (int i = 1; i < 5; ++i) {
        px += bodies[i].vx * bodies[i].mass;
        py += bodies[i].vy * bodies[i].mass;
        pz += bodies[i].vz * bodies[i].mass;
    }
    bodies[0].vx = -px / bodies[0].mass;
    bodies[0].vy = -py / bodies[0].mass;
    bodies[0].vz = -pz / bodies[0].mass;
}

static void advance(Body *bodies, int steps) {
    const double dt = 0.01;
    for (int step = 0; step < steps; ++step) {
        for (int i = 0; i < 5; ++i) {
            for (int j = i + 1; j < 5; ++j) {
                const double dx = bodies[i].x - bodies[j].x;
                const double dy = bodies[i].y - bodies[j].y;
                const double dz = bodies[i].z - bodies[j].z;
                const double distance_sq = dx * dx + dy * dy + dz * dz;
                const double mag = dt / (distance_sq * sqrt(distance_sq));
                bodies[i].vx -= dx * bodies[j].mass * mag;
                bodies[i].vy -= dy * bodies[j].mass * mag;
                bodies[i].vz -= dz * bodies[j].mass * mag;
                bodies[j].vx += dx * bodies[i].mass * mag;
                bodies[j].vy += dy * bodies[i].mass * mag;
                bodies[j].vz += dz * bodies[i].mass * mag;
            }
        }
        for (int i = 0; i < 5; ++i) {
            bodies[i].x += dt * bodies[i].vx;
            bodies[i].y += dt * bodies[i].vy;
            bodies[i].z += dt * bodies[i].vz;
        }
    }
}

int main(int argc, char **argv) {
    const double pi = 3.141592653589793;
    const double solar_mass = 4.0 * pi * pi;
    const double year = 365.24;
    Body bodies[5] = {
        {0.0, 0.0, 0.0, 0.0, 0.0, 0.0, solar_mass},
        {4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694 * year, 0.00769901118419740425 * year, -0.0000690460016972063023 * year, 0.000954791938424326609 * solar_mass},
        {8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411 * year, 0.00499852801234917238 * year, 0.0000230417297573763929 * year, 0.000285885980666130812 * solar_mass},
        {12.8943695621391310, -15.1111514016986312, -0.223307578892655734, 0.00296460137564761618 * year, 0.00237847173959480950 * year, -0.0000296589568540237556 * year, 0.0000436624404335156298 * solar_mass},
        {15.3796971148509165, -25.9193146099879641, 0.179258772950371181, 0.00268067772490389322 * year, 0.00162824170038242201 * year, -0.0000951592254519715870 * year, 0.0000515138902046611451 * solar_mass}
    };
    offset_momentum(bodies);
    printf("%.9f\n", energy(bodies, 5));
    advance(bodies, atoi(argv[1]));
    printf("%.9f\n", energy(bodies, 5));
}

const std = @import("std");

const Body = struct { x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, mass: f64 };

fn energy(bodies: []const Body) f64 {
    var total: f64 = 0.0;
    for (bodies, 0..) |b, i| {
        total += 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz);
        for (bodies[i + 1 ..]) |other| {
            const dx = b.x - other.x;
            const dy = b.y - other.y;
            const dz = b.z - other.z;
            const distance = @sqrt(dx * dx + dy * dy + dz * dz);
            total -= b.mass * other.mass / distance;
        }
    }
    return total;
}

fn offsetMomentum(bodies: []Body) void {
    var px: f64 = 0.0;
    var py: f64 = 0.0;
    var pz: f64 = 0.0;
    for (bodies[1..]) |b| {
        px += b.vx * b.mass;
        py += b.vy * b.mass;
        pz += b.vz * b.mass;
    }
    bodies[0].vx = -px / bodies[0].mass;
    bodies[0].vy = -py / bodies[0].mass;
    bodies[0].vz = -pz / bodies[0].mass;
}

fn advance(bodies: []Body, steps: usize) void {
    const dt: f64 = 0.01;
    for (0..steps) |_| {
        for (bodies, 0..) |_, i| {
            for (bodies[i + 1 ..], i + 1..) |_, j| {
                const dx = bodies[i].x - bodies[j].x;
                const dy = bodies[i].y - bodies[j].y;
                const dz = bodies[i].z - bodies[j].z;
                const distance_sq = dx * dx + dy * dy + dz * dz;
                const mag = dt / (distance_sq * @sqrt(distance_sq));
                bodies[i].vx -= dx * bodies[j].mass * mag;
                bodies[i].vy -= dy * bodies[j].mass * mag;
                bodies[i].vz -= dz * bodies[j].mass * mag;
                bodies[j].vx += dx * bodies[i].mass * mag;
                bodies[j].vy += dy * bodies[i].mass * mag;
                bodies[j].vz += dz * bodies[i].mass * mag;
            }
        }
        for (bodies) |*b| {
            b.x += dt * b.vx;
            b.y += dt * b.vy;
            b.z += dt * b.vz;
        }
    }
}

pub fn main(init: std.process.Init) !void {
    const pi: f64 = 3.141592653589793;
    const solar_mass = 4.0 * pi * pi;
    const year: f64 = 365.24;
    var bodies = [_]Body{
        .{ .x = 0.0, .y = 0.0, .z = 0.0, .vx = 0.0, .vy = 0.0, .vz = 0.0, .mass = solar_mass },
        .{ .x = 4.84143144246472090, .y = -1.16032004402742839, .z = -0.103622044471123109, .vx = 0.00166007664274403694 * year, .vy = 0.00769901118419740425 * year, .vz = -0.0000690460016972063023 * year, .mass = 0.000954791938424326609 * solar_mass },
        .{ .x = 8.34336671824457987, .y = 4.12479856412430479, .z = -0.403523417114321381, .vx = -0.00276742510726862411 * year, .vy = 0.00499852801234917238 * year, .vz = 0.0000230417297573763929 * year, .mass = 0.000285885980666130812 * solar_mass },
        .{ .x = 12.8943695621391310, .y = -15.1111514016986312, .z = -0.223307578892655734, .vx = 0.00296460137564761618 * year, .vy = 0.00237847173959480950 * year, .vz = -0.0000296589568540237556 * year, .mass = 0.0000436624404335156298 * solar_mass },
        .{ .x = 15.3796971148509165, .y = -25.9193146099879641, .z = 0.179258772950371181, .vx = 0.00268067772490389322 * year, .vy = 0.00162824170038242201 * year, .vz = -0.0000951592254519715870 * year, .mass = 0.0000515138902046611451 * solar_mass },
    };
    offsetMomentum(&bodies);
    var out_buffer: [128]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);
    try stdout.interface.print("{d:.9}\n", .{energy(&bodies)});
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    const steps = try std.fmt.parseInt(usize, args[1], 10);
    advance(&bodies, steps);
    try stdout.interface.print("{d:.9}\n", .{energy(&bodies)});
    try stdout.interface.flush();
}

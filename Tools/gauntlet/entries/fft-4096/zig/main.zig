const std = @import("std");

const N: usize = 4096;
const SCALAR_COUNT: usize = N * 2;
// Every real and imaginary scalar must be within this absolute tolerance.
const TOLERANCE: f64 = 1.0e-9;

const Complex = struct {
    re: f64,
    im: f64,
};

fn multiply(a: Complex, b: Complex) Complex {
    return .{
        .re = a.re * b.re - a.im * b.im,
        .im = a.re * b.im + a.im * b.re,
    };
}

fn fft(input: []const f64, spectrum: []Complex) void {
    const n = input.len;
    var i: usize = 0;
    while (i < n) : (i += 1) {
        spectrum[i] = .{ .re = input[i], .im = 0.0 };
    }

    var j: usize = 0;
    i = 1;
    while (i < n) : (i += 1) {
        var bit = n >> 1;
        while ((j & bit) != 0) : (bit >>= 1) {
            j ^= bit;
        }
        j ^= bit;
        if (i < j) {
            const temporary = spectrum[i];
            spectrum[i] = spectrum[j];
            spectrum[j] = temporary;
        }
    }

    const pi: f64 = 3.141592653589793;
    var width: usize = 2;
    while (width <= n) : (width <<= 1) {
        const half = width >> 1;
        const angle = -2.0 * pi / @as(f64, @floatFromInt(width));
        const wlen = Complex{ .re = @cos(angle), .im = @sin(angle) };
        var start: usize = 0;
        while (start < n) : (start += width) {
            var w = Complex{ .re = 1.0, .im = 0.0 };
            var offset: usize = 0;
            while (offset < half) : (offset += 1) {
                const left = start + offset;
                const right = left + half;
                const u = spectrum[left];
                const v = multiply(spectrum[right], w);
                spectrum[left] = .{ .re = u.re + v.re, .im = u.im + v.im };
                spectrum[right] = .{ .re = u.re - v.re, .im = u.im - v.im };
                w = multiply(w, wlen);
            }
        }
    }
}

pub fn main(init: std.process.Init) !void {
    var input = [_]f64{0.0} ** N;
    input[0] = 1.0;
    var spectrum: [N]Complex = undefined;
    fft(&input, &spectrum);

    var output: [SCALAR_COUNT]f64 = undefined;
    var i: usize = 0;
    while (i < N) : (i += 1) {
        output[2 * i] = spectrum[i].re;
        output[2 * i + 1] = spectrum[i].im;
    }

    var failures: usize = 0;
    if (output.len != SCALAR_COUNT) {
        failures += 1;
    } else {
        i = 0;
        while (i < N) : (i += 1) {
            if (!(@abs(output[2 * i] - 1.0) <= TOLERANCE)) {
                failures += 1;
            }
            if (!(@abs(output[2 * i + 1]) <= TOLERANCE)) {
                failures += 1;
            }
        }
    }
    if (failures != 0) return error.FftValidationFailed;

    var out_buffer: [128]u8 = undefined;
    var stdout = std.Io.File.Writer.init(.stdout(), init.io, &out_buffer);
    try stdout.interface.print("fft4096 checked=8192 tolerance=1e-9\n", .{});
    try stdout.interface.flush();
}

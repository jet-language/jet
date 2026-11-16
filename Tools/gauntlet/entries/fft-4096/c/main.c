#include <math.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

enum { N = 4096, SCALAR_COUNT = 8192 };
// Every real and imaginary scalar must be within this absolute tolerance.
static const double TOLERANCE = 1.0e-9;

typedef struct {
    double re;
    double im;
} Complex;

static Complex multiply(Complex a, Complex b) {
    Complex result = {
        a.re * b.re - a.im * b.im,
        a.re * b.im + a.im * b.re,
    };
    return result;
}

static void fft(const double *input, Complex *spectrum, size_t n) {
    for (size_t i = 0; i < n; ++i) {
        spectrum[i].re = input[i];
        spectrum[i].im = 0.0;
    }

    size_t j = 0;
    for (size_t i = 1; i < n; ++i) {
        size_t bit = n >> 1;
        while ((j & bit) != 0) {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if (i < j) {
            Complex temporary = spectrum[i];
            spectrum[i] = spectrum[j];
            spectrum[j] = temporary;
        }
    }

    const double pi = 3.141592653589793;
    for (size_t width = 2; width <= n; width <<= 1) {
        const size_t half = width >> 1;
        const double angle = -2.0 * pi / (double)width;
        const Complex wlen = { cos(angle), sin(angle) };
        for (size_t start = 0; start < n; start += width) {
            Complex w = { 1.0, 0.0 };
            for (size_t offset = 0; offset < half; ++offset) {
                const size_t left = start + offset;
                const size_t right = left + half;
                const Complex u = spectrum[left];
                const Complex v = multiply(spectrum[right], w);
                spectrum[left].re = u.re + v.re;
                spectrum[left].im = u.im + v.im;
                spectrum[right].re = u.re - v.re;
                spectrum[right].im = u.im - v.im;
                w = multiply(w, wlen);
            }
        }
    }
}

int main(void) {
    double *input = calloc(N, sizeof(*input));
    Complex *spectrum = malloc(N * sizeof(*spectrum));
    double *output = malloc(SCALAR_COUNT * sizeof(*output));
    if (input == NULL || spectrum == NULL || output == NULL) {
        free(input);
        free(spectrum);
        free(output);
        return 2;
    }
    input[0] = 1.0;

    fft(input, spectrum, N);
    for (size_t i = 0; i < N; ++i) {
        output[2 * i] = spectrum[i].re;
        output[2 * i + 1] = spectrum[i].im;
    }

    size_t failures = 0;
    if (SCALAR_COUNT != N * 2) {
        ++failures;
    } else {
        for (size_t i = 0; i < N; ++i) {
            if (!(fabs(output[2 * i] - 1.0) <= TOLERANCE)) {
                ++failures;
            }
            if (!(fabs(output[2 * i + 1]) <= TOLERANCE)) {
                ++failures;
            }
        }
    }

    free(input);
    free(spectrum);
    free(output);
    if (failures != 0) {
        fprintf(stderr, "fft validation failed: %zu\n", failures);
        return 1;
    }
    puts("fft4096 checked=8192 tolerance=1e-9");
    return 0;
}

import numpy as np

N = 4096
SCALAR_COUNT = N * 2
# Every real and imaginary scalar must be within this absolute tolerance.
TOLERANCE = 1.0e-9


def main():
    signal = np.zeros(N, dtype=np.float64)
    signal[0] = 1.0

    # NumPy's default fft is a forward, unnormalized transform.
    spectrum = np.fft.fft(signal)
    output = np.empty(SCALAR_COUNT, dtype=np.float64)
    output[0::2] = spectrum.real
    output[1::2] = spectrum.imag

    failures = 0
    if output.size != SCALAR_COUNT:
        failures += 1
    else:
        for i in range(N):
            if not (abs(float(output[2 * i]) - 1.0) <= TOLERANCE):
                failures += 1
            if not (abs(float(output[2 * i + 1])) <= TOLERANCE):
                failures += 1
    if failures:
        raise RuntimeError(f"fft validation failed: {failures}")
    print("fft4096 checked=8192 tolerance=1e-9")


if __name__ == "__main__":
    main()

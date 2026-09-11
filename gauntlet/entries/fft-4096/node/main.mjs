const N = 4096;
const SCALAR_COUNT = N * 2;
// Every real and imaginary scalar must be within this absolute tolerance.
const TOLERANCE = 1.0e-9;

function multiply(aRe, aIm, bRe, bIm) {
  return [aRe * bRe - aIm * bIm, aRe * bIm + aIm * bRe];
}

function fft(input) {
  const n = input.length;
  const real = new Float64Array(input);
  const imag = new Float64Array(n);

  let j = 0;
  for (let i = 1; i < n; i += 1) {
    let bit = n >> 1;
    while ((j & bit) !== 0) {
      j ^= bit;
      bit >>= 1;
    }
    j ^= bit;
    if (i < j) {
      [real[i], real[j]] = [real[j], real[i]];
      [imag[i], imag[j]] = [imag[j], imag[i]];
    }
  }

  for (let width = 2; width <= n; width *= 2) {
    const half = width >> 1;
    const angle = -2 * Math.PI / width;
    const wlenRe = Math.cos(angle);
    const wlenIm = Math.sin(angle);
    for (let start = 0; start < n; start += width) {
      let wRe = 1;
      let wIm = 0;
      for (let offset = 0; offset < half; offset += 1) {
        const left = start + offset;
        const right = left + half;
        const [vRe, vIm] = multiply(real[right], imag[right], wRe, wIm);
        const uRe = real[left];
        const uIm = imag[left];
        real[left] = uRe + vRe;
        imag[left] = uIm + vIm;
        real[right] = uRe - vRe;
        imag[right] = uIm - vIm;
        [wRe, wIm] = multiply(wRe, wIm, wlenRe, wlenIm);
      }
    }
  }
  return { real, imag };
}

const input = new Float64Array(N);
input[0] = 1;
const spectrum = fft(input);
const output = new Float64Array(SCALAR_COUNT);
for (let i = 0; i < N; i += 1) {
  output[2 * i] = spectrum.real[i];
  output[2 * i + 1] = spectrum.imag[i];
}

let failures = 0;
if (output.length !== SCALAR_COUNT) {
  failures += 1;
} else {
  for (let i = 0; i < N; i += 1) {
    if (!(Math.abs(output[2 * i] - 1) <= TOLERANCE)) failures += 1;
    if (!(Math.abs(output[2 * i + 1]) <= TOLERANCE)) failures += 1;
  }
}
if (failures !== 0) throw new Error(`fft validation failed: ${failures}`);
console.log("fft4096 checked=8192 tolerance=1e-9");

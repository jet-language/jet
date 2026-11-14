package main

import (
	"fmt"
	"math"
	"math/bits"
	"os"
)

const (
	n           = 4096
	scalarCount = 8192
	// Every real and imaginary scalar must be within this absolute tolerance.
	tolerance = 1.0e-9
)

// fft is an iterative radix-2 transform: a bit-reversal permutation, then
// butterflies of doubling width with a twiddle factor stepped per offset.
func fft(input []float64) []complex128 {
	size := len(input)
	spectrum := make([]complex128, size)
	shift := 64 - uint(bits.TrailingZeros(uint(size)))
	for i, x := range input {
		spectrum[bits.Reverse64(uint64(i))>>shift] = complex(x, 0)
	}
	for width := 2; width <= size; width <<= 1 {
		half := width >> 1
		angle := -2 * math.Pi / float64(width)
		step := complex(math.Cos(angle), math.Sin(angle))
		for start := 0; start < size; start += width {
			w := complex(1, 0)
			low := spectrum[start : start+half]
			high := spectrum[start+half : start+width]
			for k := range low {
				u, v := low[k], high[k]*w
				low[k], high[k] = u+v, u-v
				w *= step
			}
		}
	}
	return spectrum
}

func main() {
	input := make([]float64, n)
	input[0] = 1
	spectrum := fft(input)
	output := make([]float64, scalarCount)
	for i, c := range spectrum {
		output[2*i], output[2*i+1] = real(c), imag(c)
	}
	failures := 0
	for i := 0; i < n; i++ {
		if !(math.Abs(output[2*i]-1) <= tolerance) {
			failures++
		}
		if !(math.Abs(output[2*i+1]) <= tolerance) {
			failures++
		}
	}
	if failures != 0 {
		fmt.Fprintf(os.Stderr, "fft validation failed: %d\n", failures)
		os.Exit(1)
	}
	os.Stdout.WriteString("fft4096 checked=8192 tolerance=1e-9\n")
}

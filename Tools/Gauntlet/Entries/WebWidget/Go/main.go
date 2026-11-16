package main

//go:wasmexport triangular
func triangular(n int32) int32 {
	var total int32
	for i := int32(1); i <= n; i++ {
		total += i
	}
	return total
}

// main stays parked so the host can keep calling the export.
func main() {
	select {}
}

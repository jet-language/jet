package main

import (
	"bufio"
	"encoding/binary"
	"fmt"
	"math"
	"os"
)

const (
	fnvOffset = 0xcbf29ce484222325
	fnvPrime  = 0x100000001b3
)

func main() {
	path := "records.bin"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	data, err := os.ReadFile(path)
	if err != nil || len(data) < 8 || string(data[:4]) != "JGB1" {
		os.Exit(1)
	}
	le := binary.LittleEndian
	count := le.Uint32(data[4:8])
	rest := data[8:]
	sum := 0.0
	hash := uint64(fnvOffset)
	for i := uint32(0); i < count; i++ {
		// id:u32 value:f64 name_len:u16, then name_len name bytes
		if len(rest) < 14 {
			os.Exit(1)
		}
		id := le.Uint32(rest)
		value := math.Float64frombits(le.Uint64(rest[4:]))
		nameLen := int(le.Uint16(rest[12:]))
		rest = rest[14:]
		if len(rest) < nameLen {
			os.Exit(1)
		}
		if id%7 == 0 {
			sum += value
		}
		for _, b := range rest[:nameLen] {
			hash = (hash ^ uint64(b)) * fnvPrime
		}
		rest = rest[nameLen:]
	}
	out := bufio.NewWriter(os.Stdout)
	fmt.Fprintf(out, "records %d\n", count)
	fmt.Fprintf(out, "sum7 %.6f\n", sum)
	fmt.Fprintf(out, "fnv %016x\n", hash)
	out.Flush()
}

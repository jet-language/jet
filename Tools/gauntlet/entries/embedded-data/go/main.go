package main

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"fmt"
	"math"
	"os"
)

const frameSize = 12 // channel:u8 flags:u8 sample:u16 tick:u32 energy:u16 load:u16

func main() {
	path := "telemetry.bin"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	data, err := os.ReadFile(path)
	if err != nil || len(data) < 8 || !bytes.Equal(data[:4], []byte("EDB1")) {
		os.Exit(1)
	}
	le := binary.LittleEndian
	count := le.Uint32(data[4:8])
	frames := data[8:]
	if uint64(len(frames)) < uint64(count)*frameSize {
		os.Exit(1)
	}
	var checksum uint64
	for _, b := range data {
		checksum += uint64(b)
	}

	var valid, sampleMax, sampleSum, energySum, loadSum, tickSum uint64
	var channels [4]uint64
	sampleMin := uint64(math.MaxUint16)
	for i := uint32(0); i < count; i++ {
		frame := frames[uint64(i)*frameSize:][:frameSize]
		channel := frame[0]
		if channel >= 4 {
			os.Exit(1)
		}
		if frame[1] != 0 {
			continue
		}
		sample := uint64(le.Uint16(frame[2:]))
		valid++
		channels[channel]++
		sampleMin = min(sampleMin, sample)
		sampleMax = max(sampleMax, sample)
		sampleSum += sample
		tickSum += uint64(le.Uint32(frame[4:]))
		energySum += uint64(le.Uint16(frame[8:]))
		loadSum += uint64(le.Uint16(frame[10:]))
	}

	out := bufio.NewWriter(os.Stdout)
	fmt.Fprintf(out, "frames %d\n", count)
	fmt.Fprintf(out, "valid %d\n", valid)
	fmt.Fprintf(out, "channels %d %d %d %d\n", channels[0], channels[1], channels[2], channels[3])
	fmt.Fprintf(out, "sample %d %d %d\n", sampleMin, sampleMax, sampleSum)
	fmt.Fprintf(out, "energy %d\n", energySum)
	fmt.Fprintf(out, "load %d\n", loadSum)
	fmt.Fprintf(out, "ticks %d\n", tickSum)
	fmt.Fprintf(out, "checksum %d\n", checksum)
	out.Flush()
}

package main

import (
	"bufio"
	"fmt"
	"os"
	"strconv"
)

// crc16 shifts the 16 bits of word in low bit first; the polynomial step adds
// 4129 modulo 65536, as every rail of this entry does.
func crc16(word int64) int64 {
	crc, value := int64(65535), word
	for bit := 0; bit < 16; bit++ {
		top := crc >> 15
		incoming := value & 1
		crc = (crc << 1) & 0xFFFF
		if top^incoming == 1 {
			crc = (crc + 4129) & 0xFFFF
		}
		value >>= 1
	}
	return crc
}

// signedDiv8 divides toward zero, which Go's / already does.
func signedDiv8(value int64) int64 { return value / 8 }

func clamp(value, low, high int64) int64 { return min(max(value, low), high) }

func main() {
	ticks, err := strconv.ParseInt(os.Args[1], 10, 64)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	var ring [16]int64
	ringPosition := 0
	var runningSum, integrator, actuator, watchdog, watchdogResets int64
	var accepted, rejected, faults, statusReg, mmioChecksum int64
	controlReg := int64(4095)

	for tick := int64(0); tick < ticks; tick++ {
		raw := (tick*73 + 19) % 4096
		frame := (raw*17 + tick*31 + 7) % 65536
		expectedCRC := crc16(frame)
		receivedCRC := expectedCRC
		if tick%127 == 0 {
			receivedCRC = (expectedCRC + 1) % 65536
		}
		watchdog++

		if receivedCRC != expectedCRC {
			rejected++
			faults++
			statusReg = 2
		} else {
			sample := (raw*3 + 11) % 4096
			old := ring[ringPosition]
			ring[ringPosition] = sample
			ringPosition = (ringPosition + 1) % len(ring)
			runningSum += sample - old
			mean := runningSum / 16
			e := 2048 - mean
			integrator = clamp(integrator+e, -8192, 8192)
			actuator = clamp(e*4+signedDiv8(integrator), -4095, 4095)
			accepted++
			statusReg = 1
		}

		controlReg = actuator + 4095
		mmioChecksum = (mmioChecksum*33 + statusReg*257 + controlReg + receivedCRC) % 1000000007
		if watchdog == 64 {
			watchdog = 0
			watchdogResets++
		}
	}

	ringChecksum := int64(0)
	for _, sample := range ring {
		ringChecksum = (ringChecksum*131 + sample) % 1000000007
	}
	out := bufio.NewWriter(os.Stdout)
	fmt.Fprintf(out, "ticks %d\n", ticks)
	fmt.Fprintf(out, "accepted %d rejected %d\n", accepted, rejected)
	fmt.Fprintf(out, "watchdog_resets %d faults %d\n", watchdogResets, faults)
	fmt.Fprintf(out, "actuator %d\n", actuator)
	fmt.Fprintf(out, "mmio_checksum %d\n", mmioChecksum)
	fmt.Fprintf(out, "ring_checksum %d\n", ringChecksum)
	out.Flush()
}

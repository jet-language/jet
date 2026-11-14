package main

import (
	"fmt"
	"math"
	"os"
	"strconv"
)

type body struct{ x, y, z, vx, vy, vz, mass float64 }

const (
	solarMass = 4 * math.Pi * math.Pi
	year      = 365.24
	dt        = 0.01
)

func energy(bodies []body) float64 {
	total := 0.0
	for i, b := range bodies {
		total += 0.5 * b.mass * (b.vx*b.vx + b.vy*b.vy + b.vz*b.vz)
		for _, other := range bodies[i+1:] {
			dx, dy, dz := b.x-other.x, b.y-other.y, b.z-other.z
			total -= b.mass * other.mass / math.Sqrt(dx*dx+dy*dy+dz*dz)
		}
	}
	return total
}

func offsetMomentum(bodies []body) {
	var px, py, pz float64
	for _, b := range bodies[1:] {
		px += b.vx * b.mass
		py += b.vy * b.mass
		pz += b.vz * b.mass
	}
	bodies[0].vx = -px / bodies[0].mass
	bodies[0].vy = -py / bodies[0].mass
	bodies[0].vz = -pz / bodies[0].mass
}

func advance(bodies *[5]body, steps int) {
	for step := 0; step < steps; step++ {
		for i := range bodies {
			bi := &bodies[i]
			for j := i + 1; j < len(bodies); j++ {
				bj := &bodies[j]
				dx, dy, dz := bi.x-bj.x, bi.y-bj.y, bi.z-bj.z
				distanceSq := dx*dx + dy*dy + dz*dz
				mag := dt / (distanceSq * math.Sqrt(distanceSq))
				bi.vx -= dx * bj.mass * mag
				bi.vy -= dy * bj.mass * mag
				bi.vz -= dz * bj.mass * mag
				bj.vx += dx * bi.mass * mag
				bj.vy += dy * bi.mass * mag
				bj.vz += dz * bi.mass * mag
			}
		}
		for i := range bodies {
			b := &bodies[i]
			b.x += dt * b.vx
			b.y += dt * b.vy
			b.z += dt * b.vz
		}
	}
}

func main() {
	steps, err := strconv.Atoi(os.Args[1])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	bodies := [5]body{
		{0, 0, 0, 0, 0, 0, solarMass},
		{4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694 * year, 0.00769901118419740425 * year, -0.0000690460016972063023 * year, 0.000954791938424326609 * solarMass},
		{8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411 * year, 0.00499852801234917238 * year, 0.0000230417297573763929 * year, 0.000285885980666130812 * solarMass},
		{12.8943695621391310, -15.1111514016986312, -0.223307578892655734, 0.00296460137564761618 * year, 0.00237847173959480950 * year, -0.0000296589568540237556 * year, 0.0000436624404335156298 * solarMass},
		{15.3796971148509165, -25.9193146099879641, 0.179258772950371181, 0.00268067772490389322 * year, 0.00162824170038242201 * year, -0.0000951592254519715870 * year, 0.0000515138902046611451 * solarMass},
	}
	offsetMomentum(bodies[:])
	fmt.Printf("%.9f\n", energy(bodies[:]))
	advance(&bodies, steps)
	fmt.Printf("%.9f\n", energy(bodies[:]))
}

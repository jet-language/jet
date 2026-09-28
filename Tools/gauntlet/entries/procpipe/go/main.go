package main

import (
	"context"
	"fmt"
	"os/exec"
	"strings"
	"time"
)

func main() {
	calc, _ := exec.Command("python3", "-c", "print(6*7)").Output()
	fmt.Printf("calc %s\n", strings.TrimSpace(string(calc)))

	source := exec.Command("python3", "-c", "print('b');print('a');print('c')")
	sourceOut, _ := source.StdoutPipe()
	sorter := exec.Command("sort")
	sorter.Stdin = sourceOut
	_ = source.Start()
	sorted, _ := sorter.Output()
	_ = source.Wait()
	fmt.Printf("sorted %s\n", strings.Join(strings.Split(strings.TrimSpace(string(sorted)), "\n"), ","))

	ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
	defer cancel()
	slow := exec.CommandContext(ctx, "python3", "-c", "import time;time.sleep(5)")
	_ = slow.Run()
	if ctx.Err() == context.DeadlineExceeded {
		fmt.Println("slow timeout")
	}

	checked := exec.Command("python3", "-c", "import sys;sys.exit(3)").Run()
	if error, ok := checked.(*exec.ExitError); ok {
		fmt.Printf("exit %d\n", error.ExitCode())
	}
}

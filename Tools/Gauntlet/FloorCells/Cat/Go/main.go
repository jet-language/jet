package main

import (
	"bufio"
	"io"
	"os"
)

func main() {
	in := bufio.NewReaderSize(os.Stdin, 65536)
	out := bufio.NewWriterSize(os.Stdout, 65536)
	for {
		line, err := in.ReadBytes('\n')
		if len(line) > 0 {
			n := len(line)
			if n > 0 && line[n-1] == '\n' {
				n--
			}
			if n > 0 && line[n-1] == '\r' {
				n--
			}
			if _, werr := out.Write(line[:n]); werr != nil {
				os.Exit(1)
			}
			if werr := out.WriteByte('\n'); werr != nil {
				os.Exit(1)
			}
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			os.Exit(1)
		}
	}
	if err := out.Flush(); err != nil {
		os.Exit(1)
	}
}

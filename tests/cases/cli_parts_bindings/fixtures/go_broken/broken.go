package main

import "C"

//export broken
func broken(a int64) int64 {
    return a +
}

func main() {}

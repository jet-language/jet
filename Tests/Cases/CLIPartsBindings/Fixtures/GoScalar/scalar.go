package main

/*
#include <stdint.h>
*/
import "C"

//export add_i64
func add_i64(a int64, b int64) int64 {
    return a + b
}

func main() {}

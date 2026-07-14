package main

/*
#include <stdint.h>
*/
import "C"
import "runtime/cgo"

//export new_handle
func new_handle(value int64) uintptr {
	return uintptr(cgo.NewHandle(value))
}

//export consume_handle
func consume_handle(handle uintptr) int64 {
	owned := cgo.Handle(handle)
	value := owned.Value().(int64)
	owned.Delete()
	return value
}

func main() {}

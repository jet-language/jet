package demo

import "fmt"

func Add(left int, right int) int {
    total := left + right
    return total
}

func run() {
    fmt.Println(Add(2, 3))
}

func Unsupported(values []int) int {
    return len(values)
}

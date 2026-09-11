package main

import (
    projection "ffi-projection-go/target/bindings"
)

func main() {
    initial := []byte{10, 20, 30}
    replacement := []byte{40, 50, 60}
    document := projection.ResourceDocument{}
    stale := projection.ResourceView{}
    if document.Open(initial) != projection.ResourceOK {
        panic("open failed")
    }
    if document.Bytes(&stale) != projection.ResourceOK {
        panic("bytes failed")
    }
    value, status := document.At(&stale, 1)
    if status != projection.ResourceOK || value != 20 {
        panic("initial read failed")
    }
    if document.Replace(replacement) != projection.ResourceOK {
        panic("replace failed")
    }
    _, status = document.At(&stale, 1)
    if status != projection.ResourceExpiredView {
        panic("stale view accepted")
    }
    fresh := projection.ResourceView{}
    if document.Bytes(&fresh) != projection.ResourceOK {
        panic("fresh bytes failed")
    }
    value, status = document.At(&fresh, 1)
    if status != projection.ResourceOK || value != 50 {
        panic("replacement read failed")
    }
    if document.Close() != projection.ResourceOK {
        panic("close failed")
    }
    if document.Close() != projection.ResourceClosed {
        panic("double close accepted")
    }
    if projection.Add(41) != 42 {
        panic("unexpected Jet result")
    }
}

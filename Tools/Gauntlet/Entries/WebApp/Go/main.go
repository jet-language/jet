package main

import (
	"fmt"
	"syscall/js"
)

// mount builds the counter into the document's #app root through syscall/js.
func mount() {
	document := js.Global().Get("document")
	root := document.Call("getElementById", "app")
	count := 0
	display := document.Call("createElement", "div")
	increment := document.Call("createElement", "button")
	reset := document.Call("createElement", "button")
	increment.Set("textContent", "increment")
	reset.Set("textContent", "reset")

	render := func() { display.Set("textContent", fmt.Sprintf("count: %d", count)) }
	increment.Call("addEventListener", "click", js.FuncOf(func(js.Value, []js.Value) any {
		count++
		render()
		return nil
	}))
	reset.Call("addEventListener", "click", js.FuncOf(func(js.Value, []js.Value) any {
		count = 0
		render()
		return nil
	}))
	render()
	root.Call("appendChild", display)
	root.Call("appendChild", increment)
	root.Call("appendChild", reset)
}

// main stays parked so the click handlers keep running.
func main() {
	mount()
	select {}
}

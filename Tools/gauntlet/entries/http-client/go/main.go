package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
)

type Item struct {
	ID   int
	Name string
	Qty  int
}

type ItemList struct {
	Items []Item
}

func get(url string, value any) int {
	response, err := http.Get(url)
	if err != nil {
		panic(err)
	}
	defer response.Body.Close()
	if err := json.NewDecoder(response.Body).Decode(value); err != nil {
		panic(err)
	}
	return response.StatusCode
}

func main() {
	base := "http://127.0.0.1:18400"
	if len(os.Args) > 1 {
		base = os.Args[1]
	}
	var listing ItemList
	if status := get(base+"/items", &listing); status != http.StatusOK {
		panic("listing request failed")
	}
	fmt.Printf("items %d\n", len(listing.Items))
	for _, id := range []int{2, 5, 99} {
		var item Item
		status := get(fmt.Sprintf("%s/items/%d", base, id), &item)
		if status == http.StatusNotFound {
			fmt.Printf("item %d missing\n", id)
		} else {
			fmt.Printf("item %d %s qty=%d\n", item.ID, item.Name, item.Qty)
		}
	}
	total := 0
	for _, item := range listing.Items {
		total += item.Qty
	}
	fmt.Printf("total-qty %d\n", total)
}

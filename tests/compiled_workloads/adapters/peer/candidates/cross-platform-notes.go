package main
import (
    "bufio"
    "fmt"
    "os"
    "strings"
)
func main() {
    if len(os.Args) != 2 { os.Exit(64) }
    file, err := os.Open(os.Args[1]); if err != nil { os.Exit(2) }; defer file.Close()
    notes, focus, search, persistence, readonly, corrupt := 0, "title", "not-run", "not-saved", "not-run", "not-run"
    active, saved, readOnlyStorage, unknown := false, false, false, false
    query, title, body := "", "", ""
    updateSearch := func() { if strings.Contains(title, query) || strings.Contains(body, query) { search = "found" } else { search = "not-found" } }
    scanner := bufio.NewScanner(file)
    for scanner.Scan() {
        line := scanner.Text(); separator := strings.IndexByte(line, ':'); if separator < 0 { unknown = true; continue }
        key, value := line[:separator], line[separator+1:]
        if key == "key" {
            switch value {
            case "add": notes++; active = true; title, body = "", ""; focus = "title"
            case "edit": if notes > 0 { active = true }; focus = "title"
            case "search": updateSearch()
            case "save": if !readOnlyStorage { saved = true; persistence = "saved" }
            case "reload": if saved { persistence = "reloaded" }
            case "readonly": readOnlyStorage = true; readonly = "blocked"
            case "corrupt": corrupt = "rejected"
            default: unknown = true
            }
        } else if key == "title" || key == "body" {
            if active { if key == "title" { title = value } else { body = value }; if search != "not-run" { updateSearch() } }
        } else if key == "query" {
            query = value; if search != "not-run" { updateSearch() }
        } else { unknown = true }
    }
    fmt.Printf("notes=%d\nfocus=%s\nsearch=%s\npersistence=%s\nreadonly=%s\ncorrupt=%s\n", notes, focus, search, persistence, readonly, corrupt)
    if unknown { fmt.Println("reject=unknown-key") }
}

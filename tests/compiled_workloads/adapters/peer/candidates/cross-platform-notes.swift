import Foundation

let arguments = CommandLine.arguments
if arguments.count != 2 { exit(64) }
guard let text = try? String(contentsOfFile: arguments[1], encoding: .utf8) else { exit(2) }
var notes = 0
var focus = "title"
var search = "not-run"
var persistence = "not-saved"
var readonly = "not-run"
var corrupt = "not-run"
var active = false
var saved = false
var readOnlyStorage = false
var unknown = false
var query = ""
var title = ""
var body = ""
func updateSearch() {
    search = title.contains(query) || body.contains(query) ? "found" : "not-found"
}
for line in text.split(whereSeparator: \.isNewline) {
    let fields = line.split(separator: ":", maxSplits: 1).map(String.init)
    guard fields.count == 2 else { unknown = true; continue }
    let key = fields[0], value = fields[1]
    if key == "key" {
        switch value {
        case "add": notes += 1; active = true; title = ""; body = ""; focus = "title"
        case "edit": if notes > 0 { active = true }; focus = "title"
        case "search": updateSearch()
        case "save": if !readOnlyStorage { saved = true; persistence = "saved" }
        case "reload": if saved { persistence = "reloaded" }
        case "readonly": readOnlyStorage = true; readonly = "blocked"
        case "corrupt": corrupt = "rejected"
        default: unknown = true
        }
    } else if key == "title" {
        if active { title = value; if search != "not-run" { updateSearch() } }
    } else if key == "body" {
        if active { body = value; if search != "not-run" { updateSearch() } }
    } else if key == "query" {
        query = value; if search != "not-run" { updateSearch() }
    } else {
        unknown = true
    }
}
print("notes=\(notes)")
print("focus=\(focus)")
print("search=\(search)")
print("persistence=\(persistence)")
print("readonly=\(readonly)")
print("corrupt=\(corrupt)")
if unknown { print("reject=unknown-key") }

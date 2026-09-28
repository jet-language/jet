use std::fs;

#[derive(Clone)]
struct Task { id: i64, text: String, done: bool }

fn skip(s: &[u8], p: &mut usize) { while *p < s.len() && s[*p].is_ascii_whitespace() { *p += 1; } }
fn expect(s: &[u8], p: &mut usize, c: u8) { skip(s, p); assert_eq!(s[*p], c); *p += 1; }
fn string(s: &[u8], p: &mut usize) -> String {
    expect(s, p, b'"');
    let mut out = String::new();
    while *p < s.len() && s[*p] != b'"' {
        if s[*p] == b'\\' {
            *p += 1;
            let c = s[*p];
            out.push(match c { b'n' => '\n', b'r' => '\r', b't' => '\t', b'"' => '"', b'\\' => '\\', _ => c as char });
        } else { out.push(s[*p] as char); }
        *p += 1;
    }
    expect(s, p, b'"');
    out
}
fn number(s: &[u8], p: &mut usize) -> i64 {
    skip(s, p); let start = *p;
    while *p < s.len() && (s[*p].is_ascii_digit() || s[*p] == b'-') { *p += 1; }
    std::str::from_utf8(&s[start..*p]).unwrap().parse().unwrap()
}
fn boolean(s: &[u8], p: &mut usize) -> bool {
    skip(s, p);
    if s[*p..].starts_with(b"true") { *p += 4; true } else { *p += 5; false }
}
fn load() -> Vec<Task> {
    let Ok(raw) = fs::read("tasks.json") else { return Vec::new() };
    let s = raw.as_slice(); let mut p = 0;
    expect(s, &mut p, b'{'); expect(s, &mut p, b'"');
    while s[p] != b'"' { p += 1; } p += 1; expect(s, &mut p, b':'); expect(s, &mut p, b'[');
    let mut tasks = Vec::new(); skip(s, &mut p);
    while p < s.len() && s[p] != b']' {
        expect(s, &mut p, b'{'); expect(s, &mut p, b'"'); while s[p] != b'"' { p += 1; } p += 1; expect(s, &mut p, b':');
        let id = number(s, &mut p); expect(s, &mut p, b','); expect(s, &mut p, b'"'); while s[p] != b'"' { p += 1; } p += 1; expect(s, &mut p, b':');
        let text = string(s, &mut p); expect(s, &mut p, b','); expect(s, &mut p, b'"'); while s[p] != b'"' { p += 1; } p += 1; expect(s, &mut p, b':');
        let done = boolean(s, &mut p); expect(s, &mut p, b'}'); tasks.push(Task { id, text, done }); skip(s, &mut p);
        if p < s.len() && s[p] == b',' { p += 1; skip(s, &mut p); }
    }
    tasks
}
fn escaped(text: &str) -> String { text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t") }
fn save(tasks: &[Task]) {
    let body = tasks.iter().map(|t| format!(r##"{{"id":{},"text":"{}","done":{}}}"##, t.id, escaped(&t.text), t.done)).collect::<Vec<_>>().join(",");
    fs::write("tasks.json", format!(r##"{{"tasks":[{}]}}"##, body)).unwrap();
}
fn main() {
    let args: Vec<String> = std::env::args().collect(); let command = args.get(1).map(String::as_str).unwrap_or(""); let mut tasks = load();
    match command {
        "add" => { let text = args.get(2).cloned().unwrap_or_default(); let id = tasks.len() as i64 + 1; tasks.push(Task { id, text: text.clone(), done: false }); save(&tasks); println!("added {} {}", id, text); }
        "done" => { let id = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(-1); if let Some(task) = tasks.iter_mut().find(|t| t.id == id) { task.done = true; save(&tasks); println!("done {}", id); } else { println!("no task {}", id); } }
        "list" => { let mut open = 0; let mut done = 0; for task in &tasks { if task.done { println!("[x] {} {}", task.id, task.text); done += 1; } else { println!("[ ] {} {}", task.id, task.text); open += 1; } } println!("open {} done {}", open, done); }
        _ => {}
    }
}

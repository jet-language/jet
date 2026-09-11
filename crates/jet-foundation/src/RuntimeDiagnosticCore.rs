// One allocation-free runtime report renderer for every execution target.
use core::fmt::{self, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetRuntimeDiagnosticRow {
    pub code: &'static str,
    pub what: &'static str,
    pub why: &'static str,
    pub fix: &'static str,
    pub template_holes: &'static [&'static str],
}

#[derive(Clone, Copy)]
pub struct JetRuntimeStopContext<'a> {
    pub file: &'a str,
    pub line: u32,
    pub function: &'a str,
    pub source_line: &'a str,
    pub column: u32,
    pub caret_len: u32,
    pub expected_type: &'a str,
}

pub fn jet_write_diagnostic_template(
    out: &mut dyn fmt::Write,
    template: &str,
    mut hole: impl FnMut(&mut dyn fmt::Write, &str) -> fmt::Result,
) -> fmt::Result {
    let bytes = template.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' && bytes.get(index + 1) == Some(&b'{') {
            out.write_char('{')?;
            index += 2;
            continue;
        }
        if bytes[index] == b'}' && bytes.get(index + 1) == Some(&b'}') {
            out.write_char('}')?;
            index += 2;
            continue;
        }
        if bytes[index] == b'{' {
            if let Some(close_offset) = template[index + 1..].find('}') {
                let close = index + 1 + close_offset;
                let name = &template[index + 1..close];
                if !name.is_empty()
                    && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    hole(out, name)?;
                    index = close + 1;
                    continue;
                }
            }
        }
        let character = template[index..].chars().next().expect("template character boundary");
        out.write_char(character)?;
        index += character.len_utf8();
    }
    Ok(())
}

// Only an initial ASCII lowercase word can change. All protected type names
// start uppercase; `fn` is the one lowercase reserved word in this rule.
#[derive(Default)]
struct ProseStart {
    offset: usize,
    first: Option<usize>,
    token_len: usize,
    is_fn: bool,
    preserve: bool,
    period: bool,
    finished: bool,
}
impl fmt::Write for ProseStart {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for ch in text.chars() {
            let offset = self.offset;
            self.offset += ch.len_utf8();
            if self.finished { continue; }
            if self.first.is_none() {
                if ch.is_whitespace() || matches!(ch, '*' | '~' | '_') { continue; }
                if !ch.is_ascii_lowercase() {
                    self.finished = true;
                    continue;
                }
                self.first = Some(offset);
                self.token_len = 1;
                self.is_fn = ch == 'f';
                continue;
            }
            if ch.is_whitespace() || matches!(ch, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '!') {
                self.finished = true;
                continue;
            }
            if self.period { self.preserve = true; }
            self.period = ch == '.';
            if self.period { continue; }
            self.token_len += 1;
            self.is_fn &= self.token_len == 2 && ch == 'n';
            self.preserve |= ch.is_ascii_digit() || matches!(ch, '_' | '/' | '\\' | '@' | '#');
        }
        Ok(())
    }
}

pub fn jet_write_sentence_case(out: &mut dyn fmt::Write, value: &dyn fmt::Display) -> fmt::Result {
    let mut start = ProseStart::default();
    write!(&mut start, "{value}")?;
    let change = start.first.filter(|_| !start.preserve && !(start.is_fn && start.token_len == 2));
    struct Capitalize<'a> {
        out: &'a mut dyn fmt::Write,
        offset: usize,
        change: Option<usize>,
    }
    impl fmt::Write for Capitalize<'_> {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            let begin = self.offset;
            self.offset += text.len();
            if let Some(change) = self.change.filter(|change| *change >= begin && *change < self.offset) {
                let index = change - begin;
                self.out.write_str(&text[..index])?;
                self.out.write_char((text.as_bytes()[index] as char).to_ascii_uppercase())?;
                self.out.write_str(&text[index + 1..])
            } else {
                self.out.write_str(text)
            }
        }
    }
    write!(&mut Capitalize { out, offset: 0, change }, "{value}")
}

pub struct JetRuntimeStopField<'a> {
    row: JetRuntimeDiagnosticRow,
    template: &'static str,
    context: JetRuntimeStopContext<'a>,
    message: &'a dyn fmt::Display,
    raw_message: bool,
}
impl fmt::Display for JetRuntimeStopField<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Raw<'a, 'b>(&'a JetRuntimeStopField<'b>);
        impl fmt::Display for Raw<'_, '_> {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                let field = self.0;
                if field.raw_message { return write!(out, "{}", field.message); }
                jet_write_diagnostic_template(out, field.template, |out, name| {
                    assert!(field.row.template_holes.contains(&name), "missing diagnostic template hole `{name}`");
                    match name {
                        "file" => out.write_str(field.context.file),
                        "line" | "n" => write!(out, "{}", field.context.line),
                        "fn" => out.write_str(field.context.function),
                        "type" => out.write_str(field.context.expected_type),
                        _ => write!(out, "{}", field.message),
                    }
                })
            }
        }
        jet_write_sentence_case(out, &Raw(self))
    }
}

pub fn jet_runtime_stop_fields<'a>(
    row: JetRuntimeDiagnosticRow,
    code: &str,
    context: JetRuntimeStopContext<'a>,
    message: &'a dyn fmt::Display,
) -> [JetRuntimeStopField<'a>; 3] {
    [(row.what, code == "E3005"), (row.why, false), (row.fix, false)].map(|(template, raw_message)| JetRuntimeStopField {
        row, template, context, message, raw_message,
    })
}

pub fn jet_runtime_stop_has_context(code: &str) -> bool {
    matches!(code, "E3001" | "E3012" | "E3014")
}

pub fn jet_runtime_stop_status(row: Option<JetRuntimeDiagnosticRow>) -> i32 {
    if row.is_some() { 70 } else { 101 }
}

pub fn jet_write_runtime_stop(
    out: &mut dyn fmt::Write,
    row: Option<JetRuntimeDiagnosticRow>,
    code: &'static str,
    context: JetRuntimeStopContext<'_>,
    message: &dyn fmt::Display,
    locals: Option<&dyn fmt::Display>,
) -> fmt::Result {
    let Some(row) = row else {
        return write!(out, "Internal error: runtime diagnostic `{code}` is not an active runtime row\n Why: Jet could not resolve this stop through the active diagnostic registry\n Fix: report this as a Jet compiler or host defect\nMore: jet-lang.dev/e/{code}\n");
    };
    let [what, why, fix] = jet_runtime_stop_fields(row, code, context, message);
    writeln!(out, "Stop [{code}]: {what}")?;
    let show_context = jet_runtime_stop_has_context(code);
    if !context.file.is_empty() {
        write!(out, "  --> {}:{}", context.file, context.line)?;
        if show_context && !context.function.is_empty() { write!(out, " in {}", context.function)?; }
        out.write_char('\n')?;
    }
    if show_context && !context.source_line.is_empty() {
        let mut digits = 1;
        let mut line = context.line;
        while line >= 10 { digits += 1; line /= 10; }
        write!(out, "{:width$}|\n{} | {}\n{:width$}| ", "", context.line, context.source_line, "", width = digits + 3)?;
        for _ in 0..context.column.saturating_sub(1) { out.write_char(' ')?; }
        for _ in 0..context.caret_len.max(1) { out.write_char('^')?; }
        out.write_char('\n')?;
    }
    if show_context {
        if let Some(locals) = locals { writeln!(out, "locals: {locals}")?; }
    }
    writeln!(out, " Why: {why}\n Fix: {fix}\nMore: jet-lang.dev/e/{code}")
}

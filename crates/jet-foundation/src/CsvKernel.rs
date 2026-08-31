//! Dependency-free CSV engine shared by whole-value and streaming readers.
//!
//! The parser keeps the physical opening line of each record. A record may
//! span several physical lines when a quoted field contains a line ending.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvOptions {
    pub delimiter: char,
    pub header: bool,
    pub skip_blank: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            header: false,
            skip_blank: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvRecord {
    pub fields: Vec<String>,
    pub line: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsvErrorKind {
    Syntax,
    Truncated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvError {
    pub kind: CsvErrorKind,
    pub record: usize,
    pub line: i64,
    pub column: i64,
    pub field: usize,
    pub reason: String,
}

impl CsvError {
    pub fn message(&self) -> String {
        format!(
            "E2701: CSV row {}, line {}, column {} — {}",
            self.record, self.line, self.column, self.reason
        )
    }
}

pub fn delimiter(value: &str) -> Result<char, String> {
    let mut chars = value.chars();
    let Some(delimiter) = chars.next() else {
        return Err(delimiter_error());
    };
    if chars.next().is_some() || matches!(delimiter, '"' | '\r' | '\n') {
        return Err(delimiter_error());
    }
    Ok(delimiter)
}

fn delimiter_error() -> String {
    "E2701: CSV delimiter must be exactly one character other than quote or line ending".to_string()
}

fn delimiter_name(delimiter: char) -> &'static str {
    if delimiter == ',' {
        "comma"
    } else {
        "delimiter"
    }
}

pub struct CsvParser {
    options: CsvOptions,
    fields: Vec<String>,
    field: String,
    quoted: bool,
    after_quote: bool,
    pending_cr: bool,
    pending_cr_line: i64,
    pending_cr_column: i64,
    record: usize,
    line: i64,
    column: i64,
    record_line: i64,
    record_has_content: bool,
    ended_record: bool,
    saw_input: bool,
}

impl CsvParser {
    pub fn new(options: CsvOptions) -> Result<Self, String> {
        if matches!(options.delimiter, '"' | '\r' | '\n') {
            return Err(delimiter_error());
        }
        Ok(Self {
            options,
            fields: Vec::new(),
            field: String::new(),
            quoted: false,
            after_quote: false,
            pending_cr: false,
            pending_cr_line: 1,
            pending_cr_column: 1,
            record: 1,
            line: 1,
            column: 0,
            record_line: 1,
            record_has_content: false,
            ended_record: false,
            saw_input: false,
        })
    }

    pub fn field_len_bytes(&self) -> usize {
        self.field.len()
    }

    pub fn current_field(&self) -> usize {
        self.fields.len()
    }

    pub fn current_record(&self) -> usize {
        self.record
    }

    pub fn line(&self) -> i64 {
        self.line
    }

    pub fn column(&self) -> i64 {
        self.column
    }

    /// Number of decoded field bytes that `push(ch)` will append.
    ///
    /// A CRLF inside a quoted field is held after the CR and appended as one
    /// decoded line ending when its LF arrives, so it contributes two bytes
    /// at that point. Streaming limits use this instead of guessing from the
    /// input character alone.
    pub fn append_len_bytes(&self, ch: char) -> usize {
        if self.pending_cr {
            return usize::from(self.quoted && ch == '\n') * 2;
        }
        if self.quoted {
            return if self.after_quote {
                usize::from(ch == '"')
            } else {
                usize::from(ch != '"') * ch.len_utf8()
            };
        }
        if matches!(ch, '"' | '\r' | '\n') || ch == self.options.delimiter {
            0
        } else {
            ch.len_utf8()
        }
    }

    pub fn would_append(&self, ch: char) -> bool {
        self.append_len_bytes(ch) != 0
    }

    pub fn capacity_bytes(&self) -> usize {
        self.field
            .capacity()
            .saturating_add(
                self.fields
                    .capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
            )
            .saturating_add(
                self.fields
                    .iter()
                    .map(|field| field.capacity())
                    .sum::<usize>(),
            )
    }

    pub fn push(&mut self, ch: char) -> Result<Option<CsvRecord>, CsvError> {
        self.saw_input = true;
        if self.pending_cr {
            if ch != '\n' {
                return Err(self.error_at(
                    CsvErrorKind::Syntax,
                    self.pending_cr_line,
                    self.pending_cr_column,
                    "bare CR is not a record ending",
                ));
            }
            self.pending_cr = false;
            self.column += 1;
            self.line += 1;
            self.column = 0;
            if self.quoted {
                self.field.push('\r');
                self.field.push('\n');
                return Ok(None);
            }
            return Ok(self.finish_record());
        }

        self.ended_record = false;
        self.column += 1;

        if self.quoted {
            if self.after_quote {
                return match ch {
                    '"' => {
                        self.field.push('"');
                        self.after_quote = false;
                        Ok(None)
                    }
                    ch if ch == self.options.delimiter => {
                        self.fields.push(std::mem::take(&mut self.field));
                        self.quoted = false;
                        self.after_quote = false;
                        Ok(None)
                    }
                    '\r' => {
                        self.quoted = false;
                        self.after_quote = false;
                        self.pending_cr = true;
                        self.pending_cr_line = self.line;
                        self.pending_cr_column = self.column;
                        Ok(None)
                    }
                    '\n' => {
                        self.quoted = false;
                        self.after_quote = false;
                        self.line += 1;
                        self.column = 0;
                        Ok(self.finish_record())
                    }
                    _ => Err(self.error(
                        CsvErrorKind::Syntax,
                        format!(
                            "only quote, {}, CRLF, LF, or EOF may follow a closing quote",
                            delimiter_name(self.options.delimiter)
                        ),
                    )),
                };
            }
            if ch == '"' {
                self.after_quote = true;
            } else {
                self.field.push(ch);
                if ch == '\n' {
                    self.line += 1;
                    self.column = 0;
                }
            }
            return Ok(None);
        }

        match ch {
            '"' if self.field.is_empty() => {
                self.quoted = true;
                self.record_has_content = true;
                Ok(None)
            }
            '"' => Err(self.error(
                CsvErrorKind::Syntax,
                "quote inside an unquoted field".to_string(),
            )),
            ch if ch == self.options.delimiter => {
                self.fields.push(std::mem::take(&mut self.field));
                Ok(None)
            }
            '\r' => {
                self.pending_cr = true;
                self.pending_cr_line = self.line;
                self.pending_cr_column = self.column;
                Ok(None)
            }
            '\n' => {
                self.line += 1;
                self.column = 0;
                Ok(self.finish_record())
            }
            _ => {
                self.field.push(ch);
                self.record_has_content = true;
                Ok(None)
            }
        }
    }

    pub fn finish(&mut self) -> Result<Option<CsvRecord>, CsvError> {
        if self.pending_cr {
            return Err(self.error_at(
                CsvErrorKind::Syntax,
                self.pending_cr_line,
                self.pending_cr_column,
                "bare CR is not a record ending",
            ));
        }
        if self.quoted && !self.after_quote {
            return Err(self.error_at(
                CsvErrorKind::Truncated,
                self.line,
                self.column + 1,
                "quoted field ended before its closing quote",
            ));
        }
        if self.ended_record || !self.saw_input {
            return Ok(None);
        }
        self.quoted = false;
        self.after_quote = false;
        Ok(self.finish_record())
    }

    fn finish_record(&mut self) -> Option<CsvRecord> {
        self.fields.push(std::mem::take(&mut self.field));
        let number = self.record;
        let line = self.record_line;
        let has_content = self.record_has_content;
        self.record += 1;
        self.record_line = self.line;
        self.record_has_content = false;
        self.ended_record = true;

        if self.options.header && number == 1 {
            self.fields.clear();
            return None;
        }
        if self.options.skip_blank && !has_content {
            self.fields.clear();
            return None;
        }
        Some(CsvRecord {
            fields: std::mem::take(&mut self.fields),
            line,
        })
    }

    fn error(&self, kind: CsvErrorKind, reason: String) -> CsvError {
        CsvError {
            kind,
            record: self.record,
            line: self.line,
            column: self.column,
            field: self.fields.len(),
            reason,
        }
    }

    fn error_at(&self, kind: CsvErrorKind, line: i64, column: i64, reason: &str) -> CsvError {
        CsvError {
            kind,
            record: self.record,
            line,
            column,
            field: self.fields.len(),
            reason: reason.to_string(),
        }
    }
}

pub fn parse(text: &str, options: CsvOptions) -> Result<Vec<CsvRecord>, String> {
    let mut parser = CsvParser::new(options)?;
    let mut records = Vec::new();
    for ch in text.chars() {
        if let Some(record) = parser.push(ch).map_err(|error| error.message())? {
            records.push(record);
        }
    }
    if let Some(record) = parser.finish().map_err(|error| error.message())? {
        records.push(record);
    }
    Ok(records)
}

pub fn render(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains(',')
                        || field.contains('"')
                        || field.contains('\n')
                        || field.contains('\r')
                    {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{delimiter, parse, CsvOptions};

    #[test]
    fn options_and_physical_lines_apply_to_records() {
        let rows = parse(
            "name\tage\r\n\r\nAda\t36\r\n\"Grace\r\nHopper\"\t85",
            CsvOptions {
                delimiter: '\t',
                header: true,
                skip_blank: true,
            },
        )
        .expect("valid CSV");
        assert_eq!(rows[0].fields, vec!["Ada", "36"]);
        assert_eq!(rows[0].line, 3);
        assert_eq!(rows[1].fields, vec!["Grace\r\nHopper", "85"]);
        assert_eq!(rows[1].line, 4);
    }

    #[test]
    fn blank_quoted_empty_field_is_not_a_blank_record() {
        let rows = parse(
            "\n\"\"\n",
            CsvOptions {
                delimiter: ',',
                header: false,
                skip_blank: true,
            },
        )
        .expect("valid CSV");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fields, vec![""]);
        assert_eq!(rows[0].line, 2);
    }

    #[test]
    fn skip_blank_ignores_records_with_only_separators() {
        let rows = parse(
            ",\n \n,\n",
            CsvOptions {
                delimiter: ',',
                header: false,
                skip_blank: true,
            },
        )
        .expect("valid CSV");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fields, vec![" "]);
        assert_eq!(rows[0].line, 2);
    }

    #[test]
    fn malformed_input_uses_the_same_engine() {
        let error = parse("a,\"unterminated", CsvOptions::default()).unwrap_err();
        assert!(error.contains("quoted field ended before its closing quote"));
        assert!(parse("a,\"ok\"junk", CsvOptions::default()).is_err());
        assert!(parse("a\rb", CsvOptions::default()).is_err());

        let multiline_error = parse(
            "header\r\n\"bad\r\nvalue",
            CsvOptions {
                delimiter: ',',
                header: true,
                skip_blank: false,
            },
        )
        .unwrap_err();
        assert!(multiline_error.contains("line 3"));
    }

    #[test]
    fn delimiter_requires_one_safe_character() {
        assert!(delimiter("").unwrap_err().starts_with("E2701:"));
        assert!(delimiter("ab").unwrap_err().starts_with("E2701:"));
        assert!(delimiter("\"").unwrap_err().starts_with("E2701:"));
        assert_eq!(delimiter("\t"), Ok('\t'));
    }
}

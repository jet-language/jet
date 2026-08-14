// D-REPORT-TEST1=A: one test-result report for generated harnesses and
// `jet prove`. Keep this source dependency-free so AOT can embed it and the
// compiler can call the same renderer for its host-side report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetTestReport {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl JetTestReport {
    pub const fn new(passed: usize, failed: usize, skipped: usize) -> Self {
        Self { passed, failed, skipped }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} passed, {} failed, {} skipped",
            self.passed, self.failed, self.skipped
        )
    }

    /// Summary member used by `jet prove`'s canonical proof report.
    pub fn json_summary(&self) -> String {
        format!(
            "{{\"failed\":{},\"passed\":{},\"selected\":{},\"skipped\":{}}}",
            self.failed,
            self.passed,
            self.passed + self.failed + self.skipped,
            self.skipped
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetTestFailure {
    pub code: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub function: String,
    pub source_line: String,
    pub col: u32,
    pub caret: u32,
    pub detail: Option<String>,
    pub cause: Vec<String>,
    pub clears: usize,
}

impl JetTestFailure {
    pub fn new(
        code: &str,
        message: &str,
        file: &str,
        line: u32,
        function: &str,
        source_line: &str,
        col: u32,
        caret: u32,
    ) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            file: file.to_string(),
            line,
            function: function.to_string(),
            source_line: source_line.to_string(),
            col,
            caret,
            detail: None,
            cause: Vec::new(),
            clears: 0,
        }
    }

    pub fn fallback(message: &str) -> Self {
        Self::new("E3001", message, "", 0, "", "", 0, 0)
    }

    pub fn render_detail(&self) -> String {
        let mut out = format!("  Stop [{}]: {}\n", self.code, self.message);
        if !self.file.is_empty() && self.line != 0 {
            let line_text = self.line.to_string();
            let margin = line_text.len();
            let function = if self.function.is_empty() {
                String::new()
            } else {
                format!(" in {}", self.function)
            };
            out.push_str(&format!(
                "    --> {}:{}{}\n",
                self.file, self.line, function
            ));
            out.push_str(&format!("      {}|\n", " ".repeat(margin)));
            out.push_str(&format!(
                "   {} | {}\n",
                line_text, self.source_line
            ));
            out.push_str(&format!(
                "      {}| {}{}\n",
                " ".repeat(margin),
                " ".repeat(self.col.saturating_sub(1) as usize),
                "^".repeat(self.caret.max(1) as usize)
            ));
        }
        if let Some(detail) = &self.detail {
            for line in detail.lines() {
                out.push_str(&format!("    {}\n", line));
            }
        }
        out
    }

    pub fn json(&self) -> String {
        let mut report = ReportEnvelope::new(
            "test",
            "stop",
            self.code.clone(),
            self.message.clone(),
            "a checked test assertion evaluated false",
            "fix the expected value or the code under test, then rerun the test",
        );
        report.detail = self.detail.clone();
        if !self.file.is_empty() {
            report.file = Some(ReportPath::from_process(&self.file));
        }
        report.line = (self.line != 0).then_some(self.line as usize);
        report.col = (self.col != 0).then_some(self.col as usize);
        report.cause = self.cause.clone();
        report.clears = self.clears;
        report.json()
    }

    pub fn with_causes(mut self, cause: Vec<String>) -> Self {
        self.cause = cause;
        self
    }

    pub fn with_clears(mut self, clears: usize) -> Self {
        self.clears = clears;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::JetTestFailure;

    #[test]
    fn json_carries_causes_and_clear_count() {
        let report = JetTestFailure::new("E3001", "failed", "x.jet", 1, "run", "", 1, 1)
            .with_causes(vec!["E0109".into(), "E0108".into()])
            .with_clears(3);
        assert!(
            report
                .json()
                .contains("\"cause\":[\"E0109\",\"E0108\"],\"clears\":3")
        );
    }
}

thread_local! {
    static JET_TEST_FAILURE: std::cell::RefCell<Option<JetTestFailure>> = const { std::cell::RefCell::new(None) };
}

pub fn jet_test_failure(
    file: &str,
    line: u32,
    function: &str,
    source_line: &str,
    col: u32,
    caret: u32,
    message: &str,
) -> String {
    let mut report = JetTestFailure::new(
        "E3001",
        message,
        file,
        line,
        function,
        source_line,
        col,
        caret,
    );
    if let Some(pending) = jet_testing_take_failure() {
        report.message = pending.message();
        report.detail = Some(pending.detail());
    }
    let message = report.message.clone();
    JET_TEST_FAILURE.with(|slot| *slot.borrow_mut() = Some(report));
    message
}

pub fn jet_test_take_failure() -> Option<JetTestFailure> {
    JET_TEST_FAILURE.with(|slot| slot.borrow_mut().take())
}

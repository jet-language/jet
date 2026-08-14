const JET_TESTING_DIFF_MAX_BYTES: usize = 64 * 1024;
const JET_TESTING_DIFF_MAX_LINES: usize = 256;
const JET_TESTING_DIFF_MAX_OUTPUT: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetTestingFailure {
    pub(crate) message: String,
    pub(crate) detail: String,
}

thread_local! {
    static JET_TESTING_FAILURE: std::cell::RefCell<Option<JetTestingFailure>> = const { std::cell::RefCell::new(None) };
}

fn jet_testing_record_failure(message: String, detail: String) {
    JET_TESTING_FAILURE.with(|slot| {
        *slot.borrow_mut() = Some(JetTestingFailure { message, detail });
    });
}

fn jet_testing_take_failure() -> Option<JetTestingFailure> {
    JET_TESTING_FAILURE.with(|slot| slot.borrow_mut().take())
}

struct JetTestingDiffLines<'a> {
    lines: Vec<&'a str>,
    truncated: bool,
}

fn jet_testing_diff_lines(input: &str) -> JetTestingDiffLines<'_> {
    let mut end = input.len().min(JET_TESTING_DIFF_MAX_BYTES);
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &input[..end];
    let mut lines = Vec::new();
    let mut truncated_by_lines = false;
    for line in prefix.split_inclusive('\n') {
        if lines.len() == JET_TESTING_DIFF_MAX_LINES {
            truncated_by_lines = true;
            break;
        }
        lines.push(line);
    }
    JetTestingDiffLines {
        lines,
        truncated: end < input.len() || truncated_by_lines,
    }
}

fn jet_testing_diff_push(output: &mut String, truncated: &mut bool, text: &str) {
    if *truncated {
        return;
    }
    if output.len() + text.len() <= JET_TESTING_DIFF_MAX_OUTPUT {
        output.push_str(text);
        return;
    }
    let marker = "\n... diff output truncated ...\n";
    let available = JET_TESTING_DIFF_MAX_OUTPUT.saturating_sub(output.len());
    if available >= marker.len() {
        let mut text_len = available - marker.len();
        while text_len > 0 && !text.is_char_boundary(text_len) {
            text_len -= 1;
        }
        output.push_str(&text[..text_len]);
        output.push_str(marker);
    } else if available > 0 {
        let mut marker_len = available;
        while marker_len > 0 && !marker.is_char_boundary(marker_len) {
            marker_len -= 1;
        }
        output.push_str(&marker[..marker_len]);
    }
    *truncated = true;
}

fn jet_testing_diff_line(output: &mut String, truncated: &mut bool, marker: char, line: &str) {
    let mut text = String::with_capacity(line.len() + 2);
    text.push(marker);
    text.push_str(line);
    if !line.ends_with('\n') {
        text.push('\n');
    }
    jet_testing_diff_push(output, truncated, &text);
}

fn jet_testing_unified_diff(path: &str, expected: &str, actual: &str) -> String {
    let expected_lines = jet_testing_diff_lines(expected);
    let actual_lines = jet_testing_diff_lines(actual);
    let rows = expected_lines.lines.len() + 1;
    let cols = actual_lines.lines.len() + 1;
    let mut lcs = vec![0usize; rows * cols];
    for row in (0..expected_lines.lines.len()).rev() {
        for col in (0..actual_lines.lines.len()).rev() {
            let index = row * cols + col;
            lcs[index] = if expected_lines.lines[row] == actual_lines.lines[col] {
                1 + lcs[(row + 1) * cols + col + 1]
            } else {
                lcs[(row + 1) * cols + col].max(lcs[row * cols + col + 1])
            };
        }
    }

    let mut output = String::new();
    let mut truncated = false;
    jet_testing_diff_push(
        &mut output,
        &mut truncated,
        &format!("--- expected {}\n+++ actual {}\n", path, path),
    );
    if expected_lines.truncated {
        jet_testing_diff_push(&mut output, &mut truncated, "# expected input truncated\n");
    }
    if actual_lines.truncated {
        jet_testing_diff_push(&mut output, &mut truncated, "# actual input truncated\n");
    }
    jet_testing_diff_push(
        &mut output,
        &mut truncated,
        &format!(
            "@@ -1,{} +1,{} @@\n",
            expected_lines.lines.len(),
            actual_lines.lines.len()
        ),
    );

    let mut row = 0;
    let mut col = 0;
    while row < expected_lines.lines.len() || col < actual_lines.lines.len() {
        if row < expected_lines.lines.len()
            && col < actual_lines.lines.len()
            && expected_lines.lines[row] == actual_lines.lines[col]
        {
            jet_testing_diff_line(&mut output, &mut truncated, ' ', expected_lines.lines[row]);
            row += 1;
            col += 1;
        } else if col < actual_lines.lines.len()
            && (row == expected_lines.lines.len()
                || lcs[row * cols + col + 1] >= lcs[(row + 1) * cols + col])
        {
            jet_testing_diff_line(&mut output, &mut truncated, '+', actual_lines.lines[col]);
            col += 1;
        } else if row < expected_lines.lines.len() {
            jet_testing_diff_line(&mut output, &mut truncated, '-', expected_lines.lines[row]);
            row += 1;
        }
    }
    output
}

pub(crate) fn jet_testing_temp_dir_path(prefix: &str) -> String {
    let safe: String = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let tid: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let path = std::env::temp_dir().join(format!(
        "jet_test_{}_{}_{}",
        safe,
        std::process::id(),
        tid
    ));
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::create_dir_all(&path);
    path.to_string_lossy().into_owned()
}

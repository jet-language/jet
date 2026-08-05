#[allow(dead_code)]
pub fn jet_progress_render(
    description: &str,
    format: &str,
    count: usize,
    total: Option<usize>,
    elapsed: f64,
    no_color: bool,
) -> String {
    let total_text = total.map_or_else(|| "?".to_string(), |n| n.to_string());
    let percent = total
        .filter(|n| *n > 0)
        .map(|n| (((count.min(n) as u128) * 100) / (n as u128)).to_string())
        .unwrap_or_else(|| if total == Some(0) { "100".to_string() } else { "?".to_string() });
    let remaining = match total {
        Some(n) if count >= n => "0.00s".to_string(),
        Some(n) if count > 0 => {
            let seconds = elapsed * (n - count) as f64 / count as f64;
            format!("{seconds:.2}s")
        }
        Some(_) | None => "?".to_string(),
    };
    let rate = if elapsed > 0.0 {
        format!("{:.2}/s", count as f64 / elapsed)
    } else {
        "?/s".to_string()
    };
    let template = if format.is_empty() {
        "{description} {percent}% {count}/{total} elapsed {elapsed} remaining {remaining} rate {rate}"
    } else {
        format
    };
    let mut rendered = template.to_string();
    for (key, value) in [
        ("description", description),
        ("percent", percent.as_str()),
        ("count", &count.to_string()),
        ("total", total_text.as_str()),
        ("elapsed", &format!("{elapsed:.2}s")),
        ("remaining", remaining.as_str()),
        ("rate", rate.as_str()),
    ] {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    if no_color {
        jet_progress_strip_ansi(&rendered)
    } else {
        rendered
    }
}

#[allow(dead_code)]
fn jet_progress_strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut escape = false;
    for ch in text.chars() {
        if escape {
            if ch.is_ascii_alphabetic() {
                escape = false;
            }
        } else if ch == '\x1b' {
            escape = true;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::jet_progress_render;

    #[test]
    fn renders_all_fields_and_zero_remaining_at_completion() {
        let rendered = jet_progress_render(
            "work",
            "{description}|{percent}|{count}|{total}|{elapsed}|{remaining}|{rate}",
            4,
            Some(4),
            2.0,
            false,
        );
        assert_eq!(rendered, "work|100|4|4|2.00s|0.00s|2.00/s");
    }

    #[test]
    fn no_color_removes_ansi_sequences() {
        let rendered = jet_progress_render("work", "\x1b[32m{description}\x1b[0m", 1, Some(2), 1.0, true);
        assert_eq!(rendered, "work");
    }

    #[test]
    fn default_format_exposes_runtime_metrics() {
        let rendered = jet_progress_render("work", "", 2, Some(4), 2.0, false);
        assert!(rendered.contains("elapsed 2.00s"));
        assert!(rendered.contains("remaining 2.00s"));
        assert!(rendered.contains("rate 1.00/s"));
    }

    #[test]
    fn unknown_total_is_rendered_without_fake_percent_or_remaining() {
        let rendered = jet_progress_render(
            "work",
            "{description}|{percent}|{count}|{total}|{remaining}",
            2,
            None,
            1.0,
            false,
        );
        assert_eq!(rendered, "work|?|2|?|?");
    }
}

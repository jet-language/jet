#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorProfile {
    Ansi16,
    Ansi256,
    TrueColor,
    Ascii,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub profile: ColorProfile,
    pub color: bool,
    pub unicode: bool,
    pub mouse: bool,
    pub resize: bool,
    pub clipboard: bool,
    pub width: usize,
    pub height: usize,
}

pub fn capabilities(profile: ColorProfile, width: usize, height: usize) -> Capabilities {
    let ascii = profile == ColorProfile::Ascii;
    Capabilities {
        profile,
        color: !ascii,
        unicode: !ascii,
        mouse: !ascii,
        resize: !ascii,
        clipboard: !ascii,
        width: width.max(1),
        height: height.max(1),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Ansi16(u8),
    Ansi256(u8),
    Rgb(u8, u8, u8),
}

pub fn color_ansi16(index: i64) -> Color {
    Color::Ansi16(index.clamp(0, 15) as u8)
}

pub fn color_ansi256(index: i64) -> Color {
    Color::Ansi256(index.clamp(0, 255) as u8)
}

pub fn color_rgb(red: i64, green: i64, blue: i64) -> Color {
    Color::Rgb(
        red.clamp(0, 255) as u8,
        green.clamp(0, 255) as u8,
        blue.clamp(0, 255) as u8,
    )
}

fn rgb_ansi256(red: u8, green: u8, blue: u8) -> u8 {
    let cube = |value: u8| -> u8 {
        if value < 48 {
            0
        } else if value < 115 {
            1
        } else {
            ((value as u16 - 55) / 40).min(5) as u8
        }
    };
    16 + 36 * cube(red) + 6 * cube(green) + cube(blue)
}

fn rgb_ansi16(red: u8, green: u8, blue: u8) -> u8 {
    let bright = u8::from(red.max(green).max(blue) > 170);
    let mut index = 0;
    if red > 96 {
        index |= 1;
    }
    if green > 96 {
        index |= 2;
    }
    if blue > 96 {
        index |= 4;
    }
    if index == 0 && red.max(green).max(blue) > 48 {
        index = 7;
    }
    index + bright * 8
}

fn color_code(color: Color, profile: ColorProfile, background: bool) -> String {
    let base = if background { 48 } else { 38 };
    match (color, profile) {
        (Color::Rgb(red, green, blue), ColorProfile::TrueColor) => {
            format!("{base};2;{red};{green};{blue}")
        }
        (Color::Rgb(red, green, blue), ColorProfile::Ansi256) => {
            format!("{base};5;{}", rgb_ansi256(red, green, blue))
        }
        (Color::Rgb(red, green, blue), ColorProfile::Ansi16) => {
            color_code(Color::Ansi16(rgb_ansi16(red, green, blue)), profile, background)
        }
        (Color::Ansi256(index), ColorProfile::TrueColor)
        | (Color::Ansi256(index), ColorProfile::Ansi256) => {
            format!("{base};5;{index}")
        }
        (Color::Ansi256(index), ColorProfile::Ansi16) => {
            color_code(Color::Ansi16(index % 16), profile, background)
        }
        (Color::Ansi16(index), ColorProfile::TrueColor)
        | (Color::Ansi16(index), ColorProfile::Ansi256) => {
            format!("{base};5;{}", index % 16)
        }
        (Color::Ansi16(index), ColorProfile::Ansi16) => {
            let index = index % 16;
            if index < 8 {
                format!("{}", base - 8 + i32::from(index))
            } else {
                format!("{}", base + 44 + i32::from(index))
            }
        }
        (_, ColorProfile::Ascii) => String::new(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
}

pub fn style_text(text: &str, style: &Style, capabilities: &Capabilities) -> String {
    let text = if capabilities.unicode {
        text.to_string()
    } else {
        ascii(text)
    };
    if !capabilities.color || capabilities.profile == ColorProfile::Ascii {
        return text;
    }
    let mut codes = Vec::new();
    if style.bold {
        codes.push("1".to_string());
    }
    if style.dim {
        codes.push("2".to_string());
    }
    if style.underline {
        codes.push("4".to_string());
    }
    if let Some(color) = style.foreground {
        codes.push(color_code(color, capabilities.profile, false));
    }
    if let Some(color) = style.background {
        codes.push(color_code(color, capabilities.profile, true));
    }
    if codes.is_empty() {
        text
    } else {
        format!("\x1b[{}m{text}\x1b[0m", codes.join(";"))
    }
}

pub fn ascii(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '─' | '━' | '═' => '-',
            '│' | '┃' | '║' => '|',
            '┌' | '┏' | '╔' | '┐' | '┓' | '╗' | '└' | '┗' | '╚' | '┘' | '┛' | '╝'
            | '├' | '┤' | '┬' | '┴' | '┼' | '╋' | '╬' => '+',
            '→' | '⇒' | '➜' => '>',
            '←' | '⇐' => '<',
            '↑' | '⇑' => '^',
            '↓' | '⇓' => 'v',
            ch if ch.is_ascii() => ch,
            _ => '?',
        })
        .collect()
}

pub fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut escape = false;
    for ch in text.chars() {
        if escape {
            if ch.is_ascii_alphabetic() || ch == '@' {
                escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            escape = true;
        } else {
            output.push(ch);
        }
    }
    output
}

pub fn char_width(ch: char) -> usize {
    if ch.is_control()
        || matches!(
            ch,
            '\u{0300}'..='\u{036f}'
                | '\u{0483}'..='\u{0489}'
                | '\u{0591}'..='\u{05bd}'
                | '\u{0610}'..='\u{061a}'
                | '\u{064b}'..='\u{065f}'
                | '\u{1ab0}'..='\u{1aff}'
                | '\u{1dc0}'..='\u{1dff}'
                | '\u{20d0}'..='\u{20ff}'
                | '\u{fe20}'..='\u{fe2f}'
        )
    {
        return 0;
    }
    if matches!(
        ch,
        '\u{1100}'..='\u{115f}'
            | '\u{2329}'..='\u{232a}'
            | '\u{2e80}'..='\u{303e}'
            | '\u{3040}'..='\u{a4cf}'
            | '\u{ac00}'..='\u{d7a3}'
            | '\u{f900}'..='\u{faff}'
            | '\u{fe10}'..='\u{fe19}'
            | '\u{fe30}'..='\u{fe6f}'
            | '\u{ff00}'..='\u{ff60}'
            | '\u{ffe0}'..='\u{ffe6}'
            | '\u{1f300}'..='\u{1faff}'
    ) {
        2
    } else {
        1
    }
}

pub fn display_width(text: &str) -> usize {
    strip_ansi(text).chars().map(char_width).sum()
}

pub fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut used = 0;
    for ch in strip_ansi(text).chars() {
        let char_width = char_width(ch);
        if used + char_width > width {
            break;
        }
        output.push(ch);
        used += char_width;
    }
    output
}

pub fn pad(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let padding = width.saturating_sub(display_width(&text));
    format!("{text}{}", " ".repeat(padding))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    Length(f64),
    Min(f64),
    Max(f64),
    Percent(f64),
    Fill(u16),
}

pub fn length(value: f64) -> Constraint {
    Constraint::Length(nonnegative(value))
}

pub fn min(value: f64) -> Constraint {
    Constraint::Min(nonnegative(value))
}

pub fn max(value: f64) -> Constraint {
    Constraint::Max(nonnegative(value))
}

pub fn percent(value: f64) -> Constraint {
    Constraint::Percent(if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    })
}

pub fn fill(weight: f64) -> Constraint {
    Constraint::Fill(if weight.is_finite() {
        weight.max(0.0).round().min(u16::MAX as f64) as u16
    } else {
        0
    })
}

fn nonnegative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

pub fn layout(area: Rect, direction: Direction, constraints: &[Constraint]) -> Vec<Rect> {
    if constraints.is_empty() {
        return Vec::new();
    }
    let total = match direction {
        Direction::Horizontal => nonnegative(area.width),
        Direction::Vertical => nonnegative(area.height),
    };
    let desired: Vec<f64> = constraints
        .iter()
        .map(|constraint| match constraint {
            Constraint::Length(value)
            | Constraint::Min(value)
            | Constraint::Max(value) => nonnegative(*value),
            Constraint::Percent(value) => total * nonnegative(*value).min(100.0) / 100.0,
            Constraint::Fill(_) => 0.0,
        })
        .collect();
    let desired_total: f64 = desired.iter().sum();
    let scale = if desired_total > total && desired_total > 0.0 {
        total / desired_total
    } else {
        1.0
    };
    let mut sizes: Vec<f64> = desired.iter().map(|value| value * scale).collect();
    let remaining = (total - sizes.iter().sum::<f64>()).max(0.0);
    let fill_weight: f64 = constraints
        .iter()
        .map(|constraint| match constraint {
            Constraint::Fill(weight) => f64::from(*weight),
            _ => 0.0,
        })
        .sum();
    if fill_weight > 0.0 {
        for (size, constraint) in sizes.iter_mut().zip(constraints.iter()) {
            if let Constraint::Fill(weight) = constraint {
                *size = remaining * f64::from(*weight) / fill_weight;
            }
        }
    }
    let mut cursor = 0.0;
    sizes
        .into_iter()
        .map(|size| {
            let frame = match direction {
                Direction::Horizontal => Rect {
                    x: area.x + cursor,
                    y: area.y,
                    width: size,
                    height: area.height,
                },
                Direction::Vertical => Rect {
                    x: area.x,
                    y: area.y + cursor,
                    width: area.width,
                    height: size,
                },
            };
            cursor += size;
            frame
        })
        .collect()
}

// D-DISPLAYDBG1 / I9: one Display/Debug value formatter for every Rust-backed
// execution seam. Engines may marshal a carrier into these impls, but they do
// not choose a container shape or re-encode an outcome's clean/told meaning.

fn jet_text_map<I>(entries: I) -> String
where
    I: IntoIterator<Item = (String, String)>,
{
    let entries = entries
        .into_iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        "[:]".to_string()
    } else {
        format!("[{}]", entries.join(", "))
    }
}

fn jet_text_debug_optional(payload: Option<String>) -> String {
    match payload {
        Some(payload) => format!("Val({payload})"),
        None => "None".to_string(),
    }
}

impl<T: JetDisplay> JetDisplay for &T {
    fn jet_display(&self) -> String {
        (**self).jet_display()
    }
}
impl<T: JetDebug> JetDebug for &T {
    fn jet_debug(&self) -> String {
        (**self).jet_debug()
    }
}

impl<T: JetDisplay> JetDisplay for [T] {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for [T] {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}

impl<T: JetDisplay> JetDisplay for Vec<T> {
    fn jet_display(&self) -> String {
        self.as_slice().jet_display()
    }
}
impl<T: JetDebug> JetDebug for Vec<T> {
    fn jet_debug(&self) -> String {
        self.as_slice().jet_debug()
    }
}

impl<T: JetDisplay, const N: usize> JetDisplay for [T; N] {
    fn jet_display(&self) -> String {
        self.as_slice().jet_display()
    }
}
impl<T: JetDebug, const N: usize> JetDebug for [T; N] {
    fn jet_debug(&self) -> String {
        self.as_slice().jet_debug()
    }
}

impl<T: JetDisplay> JetDisplay for std::collections::HashSet<T> {
    fn jet_display(&self) -> String {
        let mut parts: Vec<String> = self.iter().map(|value| value.jet_display()).collect();
        parts.sort();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for std::collections::HashSet<T> {
    fn jet_debug(&self) -> String {
        let mut parts: Vec<String> = self.iter().map(|value| value.jet_debug()).collect();
        parts.sort();
        format!("[{}]", parts.join(", "))
    }
}

impl<T: Ord + JetDisplay> JetDisplay for std::collections::BTreeSet<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + JetDebug> JetDebug for std::collections::BTreeSet<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}

impl<T: Ord + Clone + JetDisplay> JetDisplay for std::collections::BinaryHeap<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self
            .clone()
            .into_sorted_vec()
            .into_iter()
            .rev()
            .map(|value| value.jet_display())
            .collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: Ord + Clone + JetDebug> JetDebug for std::collections::BinaryHeap<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self
            .clone()
            .into_sorted_vec()
            .into_iter()
            .rev()
            .map(|value| value.jet_debug())
            .collect();
        format!("[{}]", parts.join(", "))
    }
}

impl<T: JetDisplay> JetDisplay for std::collections::VecDeque<T> {
    fn jet_display(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.jet_display()).collect();
        format!("[{}]", parts.join(", "))
    }
}
impl<T: JetDebug> JetDebug for std::collections::VecDeque<T> {
    fn jet_debug(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.jet_debug()).collect();
        format!("[{}]", parts.join(", "))
    }
}

impl<K: Ord + JetDisplay, V: JetDisplay> JetDisplay for std::collections::BTreeMap<K, V> {
    fn jet_display(&self) -> String {
        jet_text_map(
            self.iter()
                .map(|(key, value)| (key.jet_display(), value.jet_display())),
        )
    }
}
impl<K: Ord + JetDebug, V: JetDebug> JetDebug for std::collections::BTreeMap<K, V> {
    fn jet_debug(&self) -> String {
        jet_text_map(
            self.iter()
                .map(|(key, value)| (key.jet_debug(), value.jet_debug())),
        )
    }
}

// D-FAIL-CARRIER1=A: the clean report is the optional carrier's absence. The
// same carrier is a told result for every other error type.
impl JetDisplay for JetAbsent {
    fn jet_display(&self) -> String {
        "null".to_string()
    }

    fn jet_report_is_clean() -> bool {
        true
    }
}
impl JetDebug for JetAbsent {
    fn jet_debug(&self) -> String {
        "null".to_string()
    }

    fn jet_report_is_clean() -> bool {
        true
    }
}

impl JetDisplay for std::convert::Infallible {
    fn jet_display(&self) -> String {
        match *self {}
    }
}
impl JetDebug for std::convert::Infallible {
    fn jet_debug(&self) -> String {
        match *self {}
    }
}

impl JetDisplay for JetTaskFailure {
    fn jet_display(&self) -> String {
        match self {
            JetTaskFailure::Cancelled => "Cancelled".to_string(),
            JetTaskFailure::DeadlineBlown => "DeadlineBlown".to_string(),
            JetTaskFailure::Panicked(reason) => format!("Panicked({reason})"),
        }
    }
}
impl JetDebug for JetTaskFailure {
    fn jet_debug(&self) -> String {
        match self {
            JetTaskFailure::Cancelled => "Cancelled".to_string(),
            JetTaskFailure::DeadlineBlown => "DeadlineBlown".to_string(),
            JetTaskFailure::Panicked(reason) => format!("Panicked({reason:?})"),
        }
    }
}

impl<T: JetDisplay, E: JetDisplay> JetDisplay for JetOutcome<T, E> {
    fn jet_display(&self) -> String {
        let clean = <E as JetDisplay>::jet_report_is_clean();
        match self {
            Ok(value) if clean => value.jet_display(),
            Ok(value) => format!("Ok({})", value.jet_display()),
            Err(error) if clean => error.jet_display(),
            Err(error) => format!("Err({})", error.jet_display()),
        }
    }
}
impl<T: JetDebug, E: JetDebug> JetDebug for JetOutcome<T, E> {
    fn jet_debug(&self) -> String {
        let clean = <E as JetDebug>::jet_report_is_clean();
        match self {
            Ok(value) if clean => jet_text_debug_optional(Some(value.jet_debug())),
            Ok(value) => format!("Ok({})", value.jet_debug()),
            Err(_) if clean => jet_text_debug_optional(None),
            Err(error) => format!("Err({})", error.jet_debug()),
        }
    }
}

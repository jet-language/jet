// D-PROCESS-SESSION2=D: the stable terminal capability facts have one
// semantic source. Engine adapters only marshal this result into their own
// ProcessSpec/Set representation.
mod jet_process_policy {
    const TERMINAL_FACTS: &[&str] = &["terminal", "resize", "raw"];
    const NO_TERMINAL_FACTS: &[&str] = &[];

    pub fn terminal_facts(pty_supported: bool) -> &'static [&'static str] {
        if pty_supported {
            TERMINAL_FACTS
        } else {
            NO_TERMINAL_FACTS
        }
    }
}

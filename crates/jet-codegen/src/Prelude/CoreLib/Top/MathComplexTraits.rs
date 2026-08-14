// AOT display adapters for the shared Complex value. The value and arithmetic
// stay in MathLibPure; this file only follows the native Prelude trait seam.

impl JetShow for JetComplex {
    fn jet_show(&self) -> String {
        self.to_string_rep()
    }
}

impl JetDisplay for JetComplex {
    fn jet_display(&self) -> String {
        self.to_string_rep()
    }
}

impl JetDebug for JetComplex {
    fn jet_debug(&self) -> String {
        self.to_string_rep()
    }
}

// #4586: a Core record holding a `WebFormTyped` (core.web
// `WebFormValidationChain.form`) derives Debug and prints through its
// structural JetShow/JetDebug impls, which need the field's own. All three
// render `form.show()` (`jet_web_forms_typed_show`, I9: one meaning).
// Generated-program text only: the other hosts that include WebForms.rs have
// no JetShow/JetDebug traits.
impl std::fmt::Debug for JetWebFormTyped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&jet_web_forms_typed_show(self))
    }
}

impl JetShow for JetWebFormTyped {
    fn jet_show(&self) -> String {
        jet_web_forms_typed_show(self)
    }
}

impl JetDebug for JetWebFormTyped {
    fn jet_debug(&self) -> String {
        jet_web_forms_typed_show(self)
    }
}

// #4586: a Core record holding a `Session` (core.web `App.sessions`) prints
// through its structural JetShow/JetDebug impls, which call the field's own.
// Both render the redacted `session_show` projection (I9: one meaning; the
// bearer identifier never prints). Generated-program text only: the other
// hosts that include AuthSession.rs have no JetShow/JetDebug traits.
impl JetShow for JetAuthSession {
    fn jet_show(&self) -> String {
        jet_auth_session_show(self)
    }
}

impl JetDebug for JetAuthSession {
    fn jet_debug(&self) -> String {
        jet_auth_session_show(self)
    }
}

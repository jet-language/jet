// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — the runtime reflection floor.
// `JetReflectValue` is the whole-value handle (`type_name`/`display` always
// populated; `fields` non-empty only when the reflected value was a known
// user struct — built entirely at the call site, `Codegen/TIR/emit.rs`
// `("core.reflect", "of")`). `JetReflectField` is one struct field's name
// and its `.jet_show()`-rendered value. Both are plain data — no runtime
// type registry, no raw-pointer/audited-region casting of any kind (I1):
// everything here is a string captured at compile time from the call
// site's already-known static type.

#[derive(Clone)]
struct JetReflectValue {
    type_name: String,
    display: String,
    fields: Vec<JetReflectField>,
}

#[derive(Clone)]
struct JetReflectField {
    name: String,
    value: String,
}

impl JetReflectValue {
    fn type_name(&self) -> String {
        self.type_name.clone()
    }
    fn display(&self) -> String {
        self.display.clone()
    }
    fn fields(&self) -> Vec<JetReflectField> {
        self.fields.clone()
    }
}

impl JetReflectField {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn value(&self) -> String {
        self.value.clone()
    }
}

impl JetShow for JetReflectValue {
    fn jet_show(&self) -> String {
        format!("Value({})", self.type_name)
    }
}
impl JetShow for JetReflectField {
    fn jet_show(&self) -> String {
        format!("Field({}: {})", self.name, self.value)
    }
}

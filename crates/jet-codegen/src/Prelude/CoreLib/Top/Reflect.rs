// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — the runtime reflection floor.
// `JetReflectValue` is the whole-value handle (`type_name`/`display` always
// populated; `fields` non-empty only when the reflected value was a known
// user struct — built entirely at the call site, `Codegen/TIR/emit.rs`
// `("core.reflect", "of")`). `JetReflectField` is one struct field's name
// and a nested `Value` handle for its typed value. Both are plain data — no
// runtime type registry, no raw-pointer/audited-region casting of any kind
// (I1): the call site builds the same recursively typed shape from its
// already-known static type.

#[derive(Clone)]
struct JetReflectValue {
    type_name: String,
    path: String,
    display: String,
    fields: Vec<JetReflectField>,
}

#[derive(Clone)]
struct JetReflectField {
    name: String,
    value: JetReflectValue,
}

impl JetReflectValue {
    fn type_name(&self) -> String {
        self.type_name.clone()
    }
    fn path(&self) -> String {
        self.path.clone()
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
    fn value(&self) -> JetReflectValue {
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
        format!("Field({}: {})", self.name, self.value.display)
    }
}

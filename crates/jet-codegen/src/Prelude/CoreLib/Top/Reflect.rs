// D-METAREFLECT1 / D-ANY-JAI1: `reflect.of(x)` is a runtime projection of
// registered field rows. A field carries another `JetReflectValue`; text is a
// projection (`display()`), never the semantic field storage. There is no
// runtime type registry or raw-pointer/audited-region casting (I1).

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
    fn from_field<T: JetDisplay>(value: &T, type_name: &str, path: &str) -> Self {
        Self {
            type_name: type_name.to_string(),
            path: path.to_string(),
            display: value.jet_display(),
            fields: Vec::new(),
        }
    }

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
impl JetDisplay for JetReflectValue {
    fn jet_display(&self) -> String {
        self.display()
    }
}
impl JetShow for JetReflectField {
    fn jet_show(&self) -> String {
        format!("Field({}: {})", self.name, self.value.display())
    }
}

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
    fn from_field<T: JetShow>(value: &T, type_name: &str, path: &str) -> Self {
        Self {
            type_name: type_name.to_string(),
            path: path.to_string(),
            // A reflected outer value is checked for `Display`; an individual
            // field is not. Keep the typed Value total by using the universal
            // show projection for the field's display text.
            display: value.jet_show(),
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
fn jet_reflect_value_finish(
    type_name: String,
    path: String,
    display: String,
    fields: Vec<JetReflectField>,
) -> JetReflectValue {
    JetReflectValue {
        type_name,
        path,
        display,
        fields,
    }
}

fn jet_reflect_value_from_field<T: JetShow>(
    value: &T,
    type_name: &str,
    path: &str,
) -> JetReflectValue {
    JetReflectValue::from_field(value, type_name, path)
}

fn jet_reflect_field_new(name: String, value: JetReflectValue) -> JetReflectField {
    JetReflectField { name, value }
}

fn jet_reflect_value_type_name(value: &JetReflectValue) -> String {
    value.type_name()
}

fn jet_reflect_value_path(value: &JetReflectValue) -> String {
    value.path()
}

fn jet_reflect_value_display(value: &JetReflectValue) -> String {
    value.display()
}

fn jet_reflect_value_fields(value: &JetReflectValue) -> Vec<JetReflectField> {
    value.fields()
}

fn jet_reflect_field_name(value: &JetReflectField) -> String {
    value.name()
}

fn jet_reflect_field_value(value: &JetReflectField) -> JetReflectValue {
    value.value()
}

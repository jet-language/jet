// D-DX-PLUGIN1=D / card #2424: the Core side of a panel is typed and
// payload-light. Discovery remains a compiler fact; this file supplies the
// portable value/render seams and the one explicit publication gate.

/// The function shape every discovered panel must satisfy after sema has
/// checked its `State` parameter and `UiNode` result.
pub type JetDevtoolsPanelRender<State> = fn(State) -> JetUiNode;

/// A typed panel handle for host adapters. It contains no startup registration
/// state: the compiler's panel catalog supplies the handles to each host.
#[derive(Clone, Copy)]
pub struct JetDevtoolsPanel<State> {
    pub identity: &'static str,
    pub render: JetDevtoolsPanelRender<State>,
}

impl<State> JetDevtoolsPanel<State> {
    pub const fn new(identity: &'static str, render: JetDevtoolsPanelRender<State>) -> Self {
        Self { identity, render }
    }

    pub fn render(&self, state: State) -> JetUiNode {
        (self.render)(state)
    }
}

/// A value that may cross the explicit `value:` publication gate. The closed
/// protocol value set keeps every host on the same payload vocabulary.
pub trait JetDevtoolsPublishValue {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue;
}

impl JetDevtoolsPublishValue for JetDevtoolsValue {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        self
    }
}

impl JetDevtoolsPublishValue for bool {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Bool(self)
    }
}

impl JetDevtoolsPublishValue for i64 {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Int(self)
    }
}

impl JetDevtoolsPublishValue for i32 {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Int(i64::from(self))
    }
}

impl JetDevtoolsPublishValue for usize {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Int(self as i64)
    }
}

impl JetDevtoolsPublishValue for f64 {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Float(self)
    }
}

impl JetDevtoolsPublishValue for String {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Text(self)
    }
}

impl JetDevtoolsPublishValue for &str {
    fn into_jet_devtools_value(self) -> JetDevtoolsValue {
        JetDevtoolsValue::Text(self.to_string())
    }
}

/// Publish one checked panel state value through the canonical global sink.
/// The compiler supplies package/module/panel identity as hidden ABI strings;
/// source code exposes only `field:` and `value:` labels.
pub fn jet_devtools_publish<Value>(
    package: impl Into<String>,
    module: impl Into<String>,
    panel: impl Into<String>,
    field: impl Into<String>,
    value: Value,
) where
    Value: JetDevtoolsPublishValue,
{
    let package = package.into();
    let module = module.into();
    let panel = panel.into();
    let field = field.into();
    let source = format!("{package}::{module}");
    let entity = format!("{package}::{module}::{panel}");
    let fields = format!(
        "{{\"package\":\"{}\",\"module\":\"{}\",\"panel\":\"{}\",\"field\":\"{}\"}}",
        jet_devtools_panel_json_escape(&package),
        jet_devtools_panel_json_escape(&module),
        jet_devtools_panel_json_escape(&panel),
        jet_devtools_panel_json_escape(&field),
    );
    let payload = JetDevtoolsPayload::one(field, value.into_jet_devtools_value());
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    let Ok(event) = JetDevtoolsEvent::from_parts_with_payload(
        timestamp,
        source,
        "Custom",
        entity,
        fields,
        None,
    )
    .and_then(|event| event.publish(payload)) else {
        return;
    };
    jet_devtools_publish_event(event);
}

fn jet_devtools_panel_json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

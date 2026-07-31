//! Interpreter marshalling for the canonical Prelude WebApp runtime.

use crate::Comptime::CtValue;
use crate::Diagnostics::Diagnostic;

use super::{unsupported, EvalCtx, EvalWebApp, EvalWebAppStep};

const APP_HANDLE: &str = "__JetTirWebApp";
const APP_STATE: &str = "__JetTirWebAppState";
const APP_STEP: &str = "__JetTirWebAppStep";
const PAGE_VALUE: &str = "__JetTirWebPage";

fn field<'a>(fields: &'a [(String, CtValue)], name: &str) -> Option<&'a CtValue> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn handle_index(value: &CtValue) -> Option<usize> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != APP_HANDLE {
        return None;
    }
    match field(fields, "index") {
        Some(CtValue::Int(index)) => usize::try_from(*index).ok(),
        _ => None,
    }
}

fn handle_value(index: usize) -> CtValue {
    CtValue::Struct {
        type_name: APP_HANDLE.to_string(),
        fields: vec![("index".to_string(), CtValue::Int(index as i64))],
    }
}

fn step_value(step: &EvalWebAppStep) -> CtValue {
    CtValue::Struct {
        type_name: APP_STEP.to_string(),
        fields: vec![
            ("method".to_string(), CtValue::Str(step.method.clone())),
            ("args".to_string(), CtValue::List(step.args.clone())),
        ],
    }
}

impl EvalCtx<'_> {
    pub(super) fn eval_web_core_call(
        &mut self,
        method: &str,
        args: Vec<CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        match (method, args.as_slice()) {
            ("app", []) => {
                let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
                let index = runtime.web_apps.len();
                runtime.web_apps.push(EvalWebApp { steps: Vec::new() });
                Ok(handle_value(index))
            }
            ("page", [CtValue::Str(title), CtValue::Str(body)]) => Ok(CtValue::Struct {
                type_name: PAGE_VALUE.to_string(),
                fields: vec![
                    ("title".to_string(), CtValue::Str(title.clone())),
                    ("body".to_string(), CtValue::Str(body.clone())),
                ],
            }),
            _ => Err(unsupported(
                &format!("core.web.{method} interpreter arguments"),
                self.span(),
            )),
        }
    }

    fn web_app_state_value(&self, index: usize) -> Result<CtValue, Diagnostic> {
        let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let app = runtime
            .web_apps
            .get(index)
            .ok_or_else(|| unsupported("WebApp handle", self.span()))?;
        Ok(CtValue::Struct {
            type_name: APP_STATE.to_string(),
            fields: vec![(
                "steps".to_string(),
                CtValue::List(app.steps.iter().map(step_value).collect()),
            )],
        })
    }

    pub(super) fn eval_web_app_method(
        &mut self,
        recv: &CtValue,
        method: &str,
        args: Vec<CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let index = handle_index(recv)
            .ok_or_else(|| unsupported("WebApp interpreter handle", self.span()))?;
        match method {
            "route" | "page" | "layout" | "action" | "form" | "data" | "mount"
            | "routes" | "security" | "assets" | "split" | "code_split" | "cache"
            | "a11y" | "adapter" | "csr" | "ssr" | "ssg" | "stream" | "streaming"
            | "island" | "hydration_dev" | "hydration_release" => {
                let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
                let app = runtime
                    .web_apps
                    .get_mut(index)
                    .ok_or_else(|| unsupported("WebApp handle", self.span()))?;
                app.steps.push(EvalWebAppStep {
                    method: method.to_string(),
                    args,
                });
                Ok(recv.clone())
            }
            "facts_json" => {
                let mut state = self.web_app_state_value(index)?;
                crate::Comptime::try_ambient_handle(
                    "WebAppFacts",
                    &mut state,
                    &mut [],
                    self.span(),
                )
                .unwrap_or_else(|| Err(unsupported("WebApp facts host", self.span())))
            }
            "serve" => {
                let mut state = self.web_app_state_value(index)?;
                let mut port = args;
                let server = crate::Comptime::try_ambient_handle(
                    "WebAppServe",
                    &mut state,
                    &mut port,
                    self.span(),
                )
                .unwrap_or_else(|| Err(unsupported("WebApp serve host", self.span())))?;
                self.run_web_app_server(server)
            }
            _ => Err(unsupported(
                &format!("WebApp.{method} interpreter method"),
                self.span(),
            )),
        }
    }

    fn run_web_app_server(&mut self, mut server: CtValue) -> Result<CtValue, Diagnostic> {
        loop {
            let request = crate::Comptime::try_ambient_handle(
                "WebAppNext",
                &mut server,
                &mut [],
                self.span(),
            )
            .unwrap_or_else(|| Err(unsupported("WebApp callback host", self.span())))?;
            let CtValue::Struct { fields, .. } = request else {
                return Err(unsupported("WebApp callback request", self.span()));
            };
            let id = field(&fields, "id")
                .cloned()
                .ok_or_else(|| unsupported("WebApp callback request id", self.span()))?;
            let callable = field(&fields, "callable")
                .cloned()
                .ok_or_else(|| unsupported("WebApp callback callable", self.span()))?;
            let argv = match field(&fields, "args") {
                Some(CtValue::List(args)) => args.clone(),
                _ => return Err(unsupported("WebApp callback arguments", self.span())),
            };
            let result = self.call_callable(&callable, argv)?;
            let mut reply_args = [id, result];
            crate::Comptime::try_ambient_handle(
                "WebAppReply",
                &mut server,
                &mut reply_args,
                self.span(),
            )
            .unwrap_or_else(|| Err(unsupported("WebApp callback reply host", self.span())))?;
        }
    }
}

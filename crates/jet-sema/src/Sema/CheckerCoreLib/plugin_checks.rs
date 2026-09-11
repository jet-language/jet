use super::alloc_ptrs::result_ty;
use super::serde_diags::wrong_core_arity;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::{Checker, PluginExportFact, PluginInterface};
use crate::Sema::Effects::Effect;
use crate::AST::Type;

impl<'a> Checker<'a> {
    /// Whether one checked value has a recursively closed Component Model
    /// representation.  Named values still need ordinary Encode/Decode facts:
    /// a plugin interface does not invent a record layout from its spelling.
    fn plugin_component_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Int | Type::Float | Type::Bool | Type::String => true,
            Type::List(inner)
            | Type::Option(inner)
            | Type::FixedList { elem: inner, .. }
            | Type::InlineRange { base: inner, .. }
            | Type::Tagged { inner, .. } => self.plugin_component_type(inner),
            Type::Result { ok, err } => {
                self.plugin_component_type(ok) && self.plugin_component_type(err)
            }
            Type::Named(name) | Type::Apply { name, .. } => {
                !matches!(
                    name.as_str(),
                    "DataTree"
                        | "JSON"
                        | "Authority"
                        | "Secret"
                        | "Date"
                        | "LocalDate"
                        | "LocalTime"
                        | "DateTime"
                        | "Duration"
                ) && self.is_encodable(ty)
                    && self.is_decodable(ty)
            }
            _ => false,
        }
    }

    fn plugin_method_diagnostic(
        &mut self,
        method: &str,
        interface: Option<&PluginInterface>,
        span: Span,
    ) {
        let (what, detail, fix) = match interface {
            Some(interface) => (
                format!(
                    "plugin interface `{}` has no export `{method}`",
                    interface.artifact
                ),
                "a loaded plugin exposes only the exports in its registered Component interface"
                    .to_string(),
                "use a registered export name or update the plugin interface snapshot".to_string(),
            ),
            None => (
                format!("plugin export `{method}` has no registered Component interface"),
                "plugin members are resolved only from a statically known artifact and its frozen interface"
                    .to_string(),
                "pass a literal registered artifact to `core.plugin.load`, then call one of its exports"
                    .to_string(),
            ),
        };
        self.diags.push(Diagnostic::error(
            "E1257",
            what,
            detail,
            fix,
            Some(span),
        ));
    }

    fn infer_plugin_args(&mut self, args: &mut [crate::AST::CallArg]) {
        for arg in args {
            self.infer(&mut arg.expr);
        }
    }

    fn check_plugin_export(
        &mut self,
        export: &PluginExportFact,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        if args.len() != export.params.len() {
            self.diags.push(wrong_core_arity(
                &export.name,
                export.params.len(),
                args.len(),
                span,
            ));
            self.infer_plugin_args(args);
            return export.return_type.clone();
        }

        for (index, ((_, expected), arg)) in export.params.iter().zip(args).enumerate() {
            let expected = self.resolve_type(expected.clone());
            if !self.plugin_component_type(&expected) {
                self.diags.push(Diagnostic::error(
                    "E1260",
                    format!(
                        "plugin export `{}` uses unsupported Component value `{}`",
                        export.name,
                        expected.show()
                    ),
                    "plugin parameters and results must use recursively closed Component Model shapes"
                        .to_string(),
                    "use Int, Float, Bool, String, or a #Codable record/list/Option/Result shape"
                        .to_string(),
                    Some(arg.expr.span()),
                ));
            }
            self.expect_core_arg(&export.name, index, &expected, arg);
        }
        export.return_type.clone()
    }

    /// Resolve one canonical export from a statically known plugin interface.
    ///
    /// The old homogeneous `.call*` conveniences are intentionally absent:
    /// each source method name is an interface export, and its complete
    /// parameter/result contract is checked here before TIR sees the call.
    pub(crate) fn check_plugin_method(
        &mut self,
        method: &str,
        interface: Option<&PluginInterface>,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Option<Type>> {
        self.record_effect(Effect::Exec.name(), span);

        if matches!(method, "call" | "call_int" | "call_bool" | "call_text") {
            self.diags.push(Diagnostic::error(
                "E1257",
                format!("`Plugin.{method}` is retired"),
                "plugin handles expose the frozen named exports of their registered Component interface"
                    .to_string(),
                "write the plugin's exported method directly, for example `plugin.index(rows)`"
                    .to_string(),
                Some(span),
            ));
            self.infer_plugin_args(args);
            return Some(Some(result_ty(Type::String, Type::String)));
        }

        let Some(interface) = interface else {
            self.plugin_method_diagnostic(method, None, span);
            self.infer_plugin_args(args);
            return Some(Some(result_ty(Type::String, Type::String)));
        };
        let Some(export) = interface.exports.get(method) else {
            self.plugin_method_diagnostic(method, Some(interface), span);
            self.infer_plugin_args(args);
            return Some(Some(result_ty(Type::String, Type::String)));
        };

        let result = self.check_plugin_export(export, args, span);
        let result = result.map(|ty| self.resolve_type(ty)).unwrap_or_else(|| {
            Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())
        });
        Some(Some(result_ty(result, Type::String)))
    }
}

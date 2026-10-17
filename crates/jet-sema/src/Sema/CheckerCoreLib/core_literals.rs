use super::core_types::{dyn_object_pairs_ty, json_ty, u8_ty};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::suggest_field;
use crate::Syntax;
use crate::AST::Type;
impl<'a> Checker<'a> {
    pub(crate) fn check_core_json_lit(
        &mut self,
        variant: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let json = json_ty();
        if variant == "object" && args.is_empty() {
            return Some(json);
        }
        if variant == "from" {
            if args.len() != 1 {
                self.diags.push(Diagnostic::error("E0306",
                    format!("Dyn.from expects one value, got {}", args.len()),
                    "Dyn.from calls one value's Encode contract".to_string(),
                    "pass one value".to_string(), Some(span)));
            }
            for arg in args.iter_mut() {
                let saved = self.expected_type.take();
                let ty = self.infer(&mut arg.expr);
                self.expected_type = saved;
                if let Some(ty) = ty {
                    if !self.is_encodable(&ty) {
                        self.diags.push(Diagnostic::error("E0905",
                            format!("`{}` does not implement `Encode`", ty.show()),
                            "`Dyn.from` calls the value's Encode contract".to_string(),
                            "derive or implement `Encode` on this type".to_string(), Some(arg.span)));
                    }
                }
            }
            return Some(json);
        }
        let expected = match variant {
            "Null" => Vec::new(),
            "Bool" => vec![Type::Bool],
            "Int" => vec![Type::Int],
            "Float" => vec![Type::Float],
            "Text" => vec![Type::String],
            "Bytes" => vec![Type::List(Box::new(u8_ty()))],
            "DateTime" => vec![Type::Named("ZonedDateTime".to_string())],
            "Number" if self.in_unsafe
                && self.name_ledger.module_alias(self.module_idx)
                    == Some(jet_foundation::CoreSourceParts::CORE_ENCODING_STREAM_ALIAS) => vec![Type::String],
            "Array" => vec![Type::List(Box::new(json.clone()))],
            // D-DATATREE-ORDER1=A: an object keeps its fields in arrival
            // order. A key-value literal keeps the written order, a
            // `[String: Dyn]` Map hands over its sorted order, and a
            // `[(key: String, value: Dyn)]` pair list keeps list order.
            "Object" if self.is_object_pairs_arg(args) => vec![dyn_object_pairs_ty()],
            "Object" => vec![Type::Map {
                key: Box::new(Type::String),
                key_span: None,
                value: Box::new(json.clone()),
            }],
            _ => {
                // No typed edit here: this dynamic enum literal exposes
                // only the whole literal span, so replacement would erase
                // its type and payload.
                let candidates = ["Null", "Bool", "Int", "Float", "Text", "Bytes", "DateTime", "Array", "Object"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let mut fix = "check the variant name".to_string();
                if let Some(s) = suggest_field(variant, &candidates) {
                    fix = format!("did you mean `{}`?", s);
                }
                self.diags.push(Diagnostic::error(
                    "E0304",
                    format!("`{}` has no variant `{}`", Syntax::TYPE_DATA, variant),
                    "the dynamic `Dyn` value exposes Null/Bool/Int/Float/Text/Bytes/DateTime/Array/Object"
                        .to_string(),
                    fix,
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(json);
            }
        };
        if args.len() != expected.len() {
            self.diags.push(Diagnostic::error(
                    "E0306",
                    format!(
                        "`{}.{}` expects {} value{}, got {}",
                        Syntax::TYPE_DATA,
                        variant,
                        expected.len(),
                        if expected.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "each `Dyn` variant has a fixed payload (Bool/Int/Float→scalar, Text→String, Bytes→[U8], Array→[Dyn], Object→[(key: String, value: Dyn)] or [String: Dyn])".to_string(),
                    "check the variant payload".to_string(),
                    Some(span),
                ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(want) = expected.get(i) {
                self.expect_core_arg_moving(variant, i, want, arg);
                if arg.convention == crate::AST::AccessConvention::Move {
                    self.note_exact_move(&arg.expr, Some(want), arg.span);
                }
            } else {
                self.infer(&mut arg.expr);
            }
        }
        Some(json)
    }

    /// D-DATATREE-ORDER1=A: `Dyn.Object` takes the ordered
    /// `[(key: String, value: Dyn)]` pair list when its argument is a
    /// local of that type or a list literal; anything else is read as a Map.
    fn is_object_pairs_arg(&self, args: &[crate::AST::CallArg]) -> bool {
        let [arg] = args else {
            return false;
        };
        match &arg.expr {
            crate::AST::Expr::ListLit(..) => true,
            crate::AST::Expr::Ident(name, _) => self
                .lookup(name)
                .is_some_and(|local| local.ty == dyn_object_pairs_ty()),
            _ => false,
        }
    }

    /// D-DBDRIVER1: `DBValue.Null` / `.Int(n)` / `.Float(f)` / `.Text(s)` /
    /// `.Bool(b)` / `.Blob(bytes)` — the tagged SQL parameter/column value
    /// construction. Mirrors `check_core_json_lit` exactly (same dynamic-value
    /// mechanism, SQL-shaped variants); `Int` stays `Type::Int` (64-bit), never
    /// widened through `Float`. `Blob` carries a `[U8]` byte sequence.
    pub(crate) fn check_core_dbvalue_lit(
        &mut self,
        variant: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Type> {
        let dbvalue = Type::Named(Syntax::TYPE_DB_VALUE.to_string());
        let expected = match variant {
            "Null" => Vec::new(),
            "Int" => vec![Type::Int],
            "Float" => vec![Type::Float],
            "Text" => vec![Type::String],
            "Bool" => vec![Type::Bool],
            "Blob" => vec![Type::List(Box::new(u8_ty()))],
            _ => {
                // No typed edit here: this dynamic enum literal exposes
                // only the whole literal span, so replacement would erase
                // its type and payload.
                let candidates = ["Null", "Int", "Float", "Text", "Bool", "Blob"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let mut fix = "check the variant name".to_string();
                if let Some(s) = suggest_field(variant, &candidates) {
                    fix = format!("did you mean `{}`?", s);
                }
                self.diags.push(Diagnostic::error(
                    "E0304",
                    format!("`{}` has no variant `{}`", Syntax::TYPE_DB_VALUE, variant),
                    "`DBValue` is the tagged SQL parameter/column value: Null/Int/Float/Text/Bool/Blob"
                        .to_string(),
                    fix,
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(dbvalue);
            }
        };
        if args.len() != expected.len() {
            self.diags.push(Diagnostic::error(
                    "E0306",
                    format!(
                        "`{}.{}` expects {} value{}, got {}",
                        Syntax::TYPE_DB_VALUE,
                        variant,
                        expected.len(),
                        if expected.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "each `DBValue` variant has a fixed payload (Int→64-bit int, Float→float, Text→String, Bool→bool, Blob→[U8], Null→none)".to_string(),
                    "check the variant payload".to_string(),
                    Some(span),
                ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(want) = expected.get(i) {
                self.expect_core_arg_moving(variant, i, want, arg);
                if arg.convention == crate::AST::AccessConvention::Move {
                    self.note_exact_move(&arg.expr, Some(want), arg.span);
                }
                // A DBValue/JSON constructor stores its payload in an owning
                // enum slot. Copy a live owned local once, matching map-literal
                // payload semantics without cloning a last-use value.
                let copy_owned_value = arg.convention != crate::AST::AccessConvention::Move
                    && matches!(&arg.expr, crate::AST::Expr::Ident(name, _) if self
                .lookup(name)
                .is_some_and(|info| {
                    info.param_conv.is_none()
                        && !crate::Sema::Diagnostics::type_is_copy(&info.ty)
                        && self.is_name_live_after(name)
                }));
                if copy_owned_value && self.is_cloneable_type(want) {
                    self.insert_implicit_copy(&mut arg.expr, want, want);
                }
            } else {
                self.infer(&mut arg.expr);
            }
        }
        Some(dbvalue)
    }
}

//! Interpreter `core.args` — same algorithm as AOT `Args.rs` (include!).

mod native {
    use std::cell::RefCell;

    use crate::AST::CtValue;
    use crate::Diagnostics::{Diagnostic, Span};
    use crate::Comptime::Diagnostics::unsupported;

    trait JetShow {
        fn jet_show(&self) -> String;
    }
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Args.rs");

    thread_local! {
        static SPECS: RefCell<Vec<JetArgsSpec>> = const { RefCell::new(Vec::new()) };
        static PARSED: RefCell<Vec<JetParsedArgs>> = const { RefCell::new(Vec::new()) };
    }

    fn push_spec(spec: JetArgsSpec) -> i64 {
        SPECS.with(|slot| {
            let mut v = slot.borrow_mut();
            v.push(spec);
            v.len() as i64
        })
    }

    fn take_spec(id: i64) -> Option<JetArgsSpec> {
        SPECS.with(|slot| {
            let v = slot.borrow();
            let idx = id.saturating_sub(1) as usize;
            v.get(idx).cloned()
        })
    }

    fn replace_spec(id: i64, spec: JetArgsSpec) -> i64 {
        SPECS.with(|slot| {
            let mut v = slot.borrow_mut();
            let idx = id.saturating_sub(1) as usize;
            if idx < v.len() {
                v[idx] = spec;
                id
            } else {
                v.push(spec);
                v.len() as i64
            }
        })
    }

    fn push_parsed(parsed: JetParsedArgs) -> i64 {
        PARSED.with(|slot| {
            let mut v = slot.borrow_mut();
            v.push(parsed);
            v.len() as i64
        })
    }

    fn with_parsed<R>(id: i64, f: impl FnOnce(&JetParsedArgs) -> R) -> Option<R> {
        PARSED.with(|slot| {
            let v = slot.borrow();
            let idx = id.saturating_sub(1) as usize;
            v.get(idx).map(|p| f(p))
        })
    }

    fn spec_value(id: i64) -> CtValue {
        CtValue::Struct {
            type_name: "ArgsSpec".to_string(),
            fields: vec![("id".to_string(), CtValue::Int(id))],
        }
    }

    fn parsed_value(id: i64) -> CtValue {
        CtValue::Struct {
            type_name: "ParsedArgs".to_string(),
            fields: vec![("id".to_string(), CtValue::Int(id))],
        }
    }

    fn spec_id(recv: &CtValue) -> Option<i64> {
        match recv {
            CtValue::Struct { type_name, fields } if type_name == "ArgsSpec" => fields
                .iter()
                .find_map(|(n, v)| match (n.as_str(), v) {
                    ("id", CtValue::Int(i)) => Some(*i),
                    _ => None,
                }),
            _ => None,
        }
    }

    fn parsed_id(recv: &CtValue) -> Option<i64> {
        match recv {
            CtValue::Struct { type_name, fields } if type_name == "ParsedArgs" => fields
                .iter()
                .find_map(|(n, v)| match (n.as_str(), v) {
                    ("id", CtValue::Int(i)) => Some(*i),
                    _ => None,
                }),
            _ => None,
        }
    }

    fn as_str(v: &CtValue, span: Span) -> Result<String, Diagnostic> {
        match v {
            CtValue::Str(s) => Ok(s.clone()),
            _ => Err(unsupported("args expects text", span)),
        }
    }

    fn str_arg(args: &[CtValue], i: usize, span: Span) -> Result<String, Diagnostic> {
        args.get(i)
            .ok_or_else(|| unsupported("args: missing argument", span))
            .and_then(|v| as_str(v, span))
    }

    pub fn core_args_spec() -> CtValue {
        spec_value(push_spec(jet_args_spec()))
    }

    pub fn eval_handle(
        op: &str,
        recv: &mut CtValue,
        args: &mut [CtValue],
        span: Span,
    ) -> Option<Result<CtValue, Diagnostic>> {
        let result = match op {
            "ArgsSpecFlag" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(id, jet_args_flag(spec, &name, &help))))
            }
            "ArgsSpecFlagShort" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let short = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_flag_short(spec, &name, &short, &help),
                )))
            }
            "ArgsSpecOption" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_option(spec, &name, &help, &meta),
                )))
            }
            "ArgsSpecOptionDefault" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let default = match str_arg(args, 3, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_option_default(spec, &name, &help, &meta, &default),
                )))
            }
            "ArgsSpecOptionInt" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_option_int(spec, &name, &help, &meta),
                )))
            }
            "ArgsSpecOptionFloat" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_option_float(spec, &name, &help, &meta),
                )))
            }
            "ArgsSpecOptionChoice" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let choices = match str_arg(args, 3, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_option_choice(spec, &name, &help, &meta, &choices),
                )))
            }
            "ArgsSpecRepeat" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_repeat(spec, &name, &help, &meta),
                )))
            }
            "ArgsSpecRequiredOption" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let meta = match str_arg(args, 2, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_required_option(spec, &name, &help, &meta),
                )))
            }
            "ArgsSpecPositional" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_positional(spec, &name, &help),
                )))
            }
            "ArgsSpecSubcommand" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let help = match str_arg(args, 1, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let nested_id = spec_id(args.get(2)?)?;
                let nested = take_spec(nested_id)?;
                Ok(spec_value(replace_spec(
                    id,
                    jet_args_subcommand(spec, &name, &help, nested),
                )))
            }
            "ArgsSpecVersion" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let version = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(spec_value(replace_spec(id, jet_args_version(spec, &version))))
            }
            "ArgsSpecCompletion" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let shell = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(CtValue::Str(jet_args_completion(&spec, &shell)))
            }
            "ArgsSpecHelp" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                Ok(CtValue::Str(spec.help()))
            }
            "ArgsSpecParse" => {
                let id = spec_id(recv)?;
                let spec = take_spec(id)?;
                let argv = match args.first() {
                    Some(CtValue::List(items)) => items
                        .iter()
                        .map(|v| as_str(v, span))
                        .collect::<Result<Vec<_>, _>>(),
                    _ => Err(unsupported("ArgsSpec.parse expects a list of text", span)),
                };
                let argv = match argv {
                    Ok(a) => a,
                    Err(e) => return Some(Err(e)),
                };
                match jet_args_parse(&spec, &argv) {
                    Ok(parsed) => Ok(CtValue::ResOk(Box::new(parsed_value(push_parsed(parsed))))),
                    Err(msg) => Ok(CtValue::ResErr(Box::new(CtValue::Str(msg)))),
                }
            }
            "ParsedArgsFlag" => {
                let id = parsed_id(recv)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(CtValue::Bool(
                    with_parsed(id, |p| jet_parsed_flag(p, &name)).unwrap_or(false),
                ))
            }
            "ParsedArgsOption" => {
                let id = parsed_id(recv)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(match with_parsed(id, |p| jet_parsed_option(p, &name)).flatten() {
                    Some(s) => CtValue::Some(Box::new(CtValue::Str(s))),
                    None => CtValue::None(crate::AST::Type::String),
                })
            }
            "ParsedArgsOptionInt" => {
                let id = parsed_id(recv)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(
                    match with_parsed(id, |p| jet_parsed_option_int(p, &name)).flatten() {
                        Some(n) => CtValue::Some(Box::new(CtValue::Int(n))),
                        None => CtValue::None(crate::AST::Type::Int),
                    },
                )
            }
            "ParsedArgsOptionFloat" => {
                let id = parsed_id(recv)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                Ok(
                    match with_parsed(id, |p| jet_parsed_option_float(p, &name)).flatten() {
                        Some(n) => {
                            CtValue::Some(Box::new(CtValue::Float(crate::AST::CtFloat::f64(n))))
                        }
                        None => CtValue::None(crate::AST::Type::Float),
                    },
                )
            }
            "ParsedArgsOptions" => {
                let id = parsed_id(recv)?;
                let name = match str_arg(args, 0, span) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let items = with_parsed(id, |p| jet_parsed_options(p, &name)).unwrap_or_default();
                Ok(CtValue::List(items.into_iter().map(CtValue::Str).collect()))
            }
            "ParsedArgsSubcommand" => {
                let id = parsed_id(recv)?;
                Ok(
                    match with_parsed(id, |p| jet_parsed_subcommand(p)).flatten() {
                        Some(s) => CtValue::Some(Box::new(CtValue::Str(s))),
                        None => CtValue::None(crate::AST::Type::String),
                    },
                )
            }
            "ParsedArgsPositional" => {
                let id = parsed_id(recv)?;
                let idx = match args.first() {
                    Some(CtValue::Int(n)) => *n,
                    _ => return Some(Err(unsupported("ParsedArgs.positional expects Int", span))),
                };
                Ok(
                    match with_parsed(id, |p| jet_parsed_positional(p, idx)).flatten() {
                        Some(s) => CtValue::Some(Box::new(CtValue::Str(s))),
                        None => CtValue::None(crate::AST::Type::String),
                    },
                )
            }
            _ => return None,
        };
        Some(result)
    }
}

pub use native::{core_args_spec, eval_handle};

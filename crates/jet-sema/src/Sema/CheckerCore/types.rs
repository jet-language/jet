use crate::AST::Type;
use crate::Generics::substitute_type;
use crate::Sema::Checker;
use std::collections::HashMap;
impl<'a> Checker<'a> {
        pub(crate) fn resolve_type(&self, ty: Type) -> Type {
            match ty {
                // D-ENC-DYN1=A+: `Json`/`Toml`/`Yaml`/`Csv` are type aliases over the one
                // dynamic `Data` value — canonicalize every alias to `Data` so they unify.
                Type::Named(n) if crate::Syntax::is_data_type_name(&n) => {
                    Type::Named(crate::Syntax::TYPE_DATA.to_string())
                }
                // D-ENCSTREAM-SURFACE1=A: shared types live in core.encoding
                // and format handles live in their codec modules. Canonicalize
                // the user's import alias while retaining one runtime type.
                Type::Named(n)
                    if n.split_once('.').is_some_and(|(alias, leaf)| {
                        self.core_imports.get(alias).is_some_and(|module| {
                            (module == "core.encoding" && matches!(leaf,
                                "DataTree" | "EncodingLimits" | "EncodingError" |
                                "EncodingCause" | "EncodingFormat" |
                                "EncodingErrorKind" | "DataEvent")) ||
                            matches!((module.as_str(), leaf),
                                ("core.encoding.json", "JSONReader" | "JSONWriter") |
                                ("core.encoding.jsonl", "JSONLReader" | "JSONLWriter") |
                                ("core.encoding.csv", "CSVReader" | "CSVWriter") |
                                ("core.encoding.xml", "XMLReader" | "XMLWriter") |
                                ("core.encoding.cbor", "CBORReader" | "CBORWriter"))
                        })
                    }) => Type::Named(n.split_once('.').unwrap().1.to_string()),
                // D-EMAIL-SMTP-SURFACE1=A: core.email value annotations may use
                // the caller's module alias while lowering to one Core type.
                Type::Named(n)
                    if n.split_once('.').is_some_and(|(alias, leaf)| {
                        self.core_imports.get(alias).is_some_and(|module| {
                            module == "core.email" && matches!(leaf,
                                "Address" | "Message" | "Attachment" | "Envelope" |
                                "SmtpSecurity" | "RecipientPolicy" | "RecipientReport" |
                                "SendReport" | "EmailError")
                        })
                    }) => Type::Named(n.split_once('.').unwrap().1.to_string()),
                // D-ENV-MUTATE1=A: Core docs spell the exported error through
                // the user's chosen module alias (`env.EnvError`). Canonicalize
                // that qualified spelling to the one built-in runtime type.
                Type::Named(n)
                    if n.split_once('.').is_some_and(|(alias, leaf)| {
                        leaf == "EnvError"
                            && self
                                .core_imports
                                .get(alias)
                                .is_some_and(|module| module == "core.env")
                    }) =>
                {
                    Type::Named("EnvError".to_string())
                }
                Type::Named(n) if self.trait_reg.is_trait_name(&n) && !self.registry.contains(&n) => {
                    Type::TraitObject(vec![n])
                }
                Type::List(inner) => Type::List(Box::new(self.resolve_type(*inner))),
                Type::Apply { name, args } if self.registry.is_type_alias(&name) => {
                    if let Some((params, target)) = self.registry.type_alias(&name) {
                        let subst: std::collections::HashMap<String, Type> = params
                            .iter()
                            .zip(args.iter().cloned())
                            .map(|(p, a)| (p.name.clone(), a))
                            .collect();
                        let expanded = substitute_type(target, &subst);
                        return self.resolve_type(expanded);
                    }
                    Type::Apply { name, args }
                }
                Type::Apply { name, args } => Type::Apply {
                    name,
                    args: args.into_iter().map(|a| self.resolve_type(a)).collect(),
                },
                Type::Option(inner) => Type::Option(Box::new(self.resolve_type(*inner))),
                Type::Map {
                    key,
                    key_span,
                    value,
                } => Type::Map {
                    key: Box::new(self.resolve_type(*key)),
                    key_span,
                    value: Box::new(self.resolve_type(*value)),
                },
                Type::Result { ok, err } => Type::Result {
                    ok: Box::new(self.resolve_type(*ok)),
                    err: Box::new(self.resolve_type(*err)),
                },
                Type::Tuple(fields) => Type::Tuple(
                    fields
                        .into_iter()
                        .map(|(n, t)| (n, Box::new(self.resolve_type(*t))))
                        .collect(),
                ),
                other => other,
            }
        }
    
        pub(crate) fn type_param_has_bound(&self, ty: &Type, bound: &str) -> bool {
            match ty {
                Type::Named(n) => self
                    .type_param_scope
                    .iter()
                    .find(|p| p.name == *n)
                    .is_some_and(|p| p.bounds.iter().any(|b| b == bound)),
                _ => false,
            }
        }
    
        pub(crate) fn struct_subst(
            &self,
            type_name: &str,
            type_args: &[Type],
        ) -> HashMap<String, Type> {
            let params = self
                .trait_reg
                .struct_params
                .get(type_name)
                .cloned()
                .unwrap_or_default();
            if params.is_empty() {
                return HashMap::new();
            }
            if type_args.is_empty() {
                params
                    .iter()
                    .map(|p| (p.name.clone(), Type::Named(p.name.clone())))
                    .collect()
            } else {
                params
                    .iter()
                    .zip(type_args.iter())
                    .map(|(p, a)| (p.name.clone(), a.clone()))
                    .collect()
            }
        }
    
}

use super::*;
use crate::ast::{
    AccessConvention, EnumDef,
    Func, Item,
    Program, ProgramBundle, StructDef, Type, VariantPayload,
};
use crate::ffi::FfiLink;
use crate::generics;
use crate::syntax;
use std::collections::{HashMap, HashSet};
pub(crate) struct Cx {
    /// Top-level function name -> parameter conventions+types.
    pub(crate) sigs: HashMap<String, Vec<(AccessConvention, Type)>>,
    /// Top-level function name -> function value type (M8).
    pub(crate) fn_types: HashMap<String, Type>,
    /// `(TypeName, method)` -> parameter conventions+types (including `self`).
    pub(crate) method_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    pub(crate) consts: HashMap<String, String>,
    pub(crate) type_names: HashSet<String>,
    pub(crate) trait_names: HashSet<String>,
    pub(crate) struct_fields: HashMap<String, Vec<(String, Type)>>,
    pub(crate) enum_variants: HashMap<String, Vec<(String, VariantPayload)>>,
    /// variant name -> owning enum type (for pattern lowering)
    pub(crate) variant_owner: HashMap<String, String>,
    /// Recursive-type edges that need `Box<…>` in Rust (`(owner, edge_key)`).
    pub(crate) boxed_edges: HashSet<(String, String)>,
    pub(crate) cloneable: HashSet<String>,
    pub(crate) comparable: HashSet<String>,
    /// S55: explicit `derive Comparable;` → PartialOrd in Rust.
    pub(crate) partial_ord: HashSet<String>,
    pub(crate) src: String,
    pub(crate) file: String,
    /// When true, `require`/`require_eq` unwind instead of exiting (test bodies).
    pub(crate) test_mode: bool,
    /// Import alias -> Rust module name (`user_scoring`).
    pub(crate) import_mods: HashMap<String, String>,
    /// Cross-module pub type name -> Rust module path (e.g. `Note` -> `user_note`).
    pub(crate) foreign_types: HashMap<String, String>,
    /// D-MOD4: `(alias, item)` -> `(real Rust module, real fn)` for `pub use`
    /// re-exports, so `text.wrap` lowers to the module that actually defines it.
    pub(crate) reexport_calls: HashMap<(String, String), (String, String)>,
    /// `(import alias, function)` -> parameter conventions for cross-module calls.
    pub(crate) import_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    /// Import alias -> compiler-known std module (`std.fs`, `std.json`, ...).
    pub(crate) std_imports: HashMap<String, String>,
    /// M10 helpers proven reachable by sema.
    pub(crate) used_std: HashSet<String>,
    /// Empty at the entry module, `super::` inside generated import modules.
    pub(crate) root_prefix: String,
    /// M7: rustc crate name for the FFI bridge (`jet_ffi_…`).
    pub(crate) ffi_crate: Option<String>,
    /// M7: Jet function name -> wrapper symbol in the FFI crate.
    pub(crate) extern_funcs: HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    pub(crate) code_modules: HashSet<String>,
    /// D-MOD3: unqualified inline-module items (name → "alias__method").
    pub(crate) unqualified_inline: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items (name → (rust_mod_name, fn_name)).
    pub(crate) unqualified_file: HashMap<String, (String, String)>,
    /// S62/M9: (TypeName, method_name) pairs that come from trait impls — these
    /// are called without the `user_` prefix in Rust (the trait impl owns the name).
    pub(crate) trait_methods: HashSet<(String, String)>,
    /// E2-M12 D-OBS1: name of the Jet function currently being emitted, so
    /// jet_panic_rich can include the function name in the panic report.
    pub(crate) current_fn: std::cell::RefCell<String>,
}

pub(crate) const MOD_USE: &str = "use super::{JetShow, jet_panic, jet_panic_rich, jet_trace_err, jet_index_vec, jet_unpack_vec, jet_slice_vec, jet_index_map, jet_map_insert, jet_char_len, jet_string_split, jet_string_slice, jet_list_map, jet_list_map_mut, jet_list_filter, jet_list_each, jet_list_each_ref, jet_list_each_mut, jet_list_find, jet_list_any, jet_list_all, jet_list_sort_by, jet_list_reduce, jet_map_each};\n\n";

pub(crate) fn is_json_type_name(name: &str) -> bool {
    name == syntax::TYPE_JSON || name == "Json"
}

fn std_rust_type_name(name: &str) -> Option<&'static str> {
    match name {
        n if is_json_type_name(n) => Some("Json"),
        n if n == syntax::TYPE_JSON_ERROR || n == "JsonError" => Some("JsonError"),
        n if n == syntax::TYPE_IO_ERROR || n == "IoError" => Some("IoError"),
        n if n == syntax::TYPE_UTF8_ERROR || n == "Utf8Error" => Some("Utf8Error"),
        "ProcessResult" => Some("ProcessResult"),
        "Stopwatch" => Some("Stopwatch"),
        "Closed" => Some("Closed"),
        _ => None,
    }
}

/// E2-M7: file handle types are top-level in the prelude (not in `jet_std`).
fn file_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "FileReader" => Some("JetFileReader"),
        "FileWriter" => Some("JetFileWriter"),
        // FileLines is an internal sema marker; it should never appear in emitted Rust.
        "FileLines" => Some("()"),
        _ => None,
    }
}

/// E2-M10: networking opaque types map to top-level prelude structs.
pub(crate) fn net_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "TcpListener" => Some("JetTcpListener"),
        "TcpStream" => Some("JetTcpStream"),
        "HttpRequest" => Some("JetHttpRequest"),
        "HttpResponse" => Some("JetHttpResponse"),
        _ => None,
    }
}

impl Cx {
    pub(crate) fn field_rust_type(&self, owner: &str, edge: &str, ty: &Type) -> String {
        let base = self.rust_type(ty);
        if self
            .boxed_edges
            .contains(&(owner.to_string(), edge.to_string()))
        {
            format!("Box<{}>", base)
        } else {
            base
        }
    }

    pub(crate) fn struct_field_rust(&self, s: &StructDef, edge: &str, ty: &Type) -> String {
        let base = match ty {
            Type::Named(n) if s.type_params.iter().any(|p| p.name == *n) => n.clone(),
            _ => self.rust_type(ty),
        };
        if self
            .boxed_edges
            .contains(&(s.name.clone(), edge.to_string()))
        {
            format!("Box<{base}>")
        } else {
            base
        }
    }

    pub(crate) fn rust_type(&self, ty: &Type) -> String {
        match ty {
            Type::Int => "i64".to_string(),
            Type::Float => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "char".to_string(),
            Type::List(inner) => format!("Vec<{}>", self.rust_type(inner)),
            Type::Map { key, value } => format!(
                "std::collections::BTreeMap<{}, {}>",
                self.rust_type(key),
                self.rust_type(value)
            ),
            Type::Shared(inner) => format!("std::sync::Arc<{}>", self.rust_type(inner)),
            Type::Option(inner) => format!("Option<{}>", self.rust_type(inner)),
            Type::Result { ok, err } => {
                format!("Result<{}, {}>", self.rust_type(ok), self.rust_type(err))
            }
            // Items inside an imported file live in `mod user_<alias>`; the
            // module provides the namespace, so item names stay plain.
            Type::Named(name)
                if generics::is_type_var_name(name) && !self.type_names.contains(name) =>
            {
                name.clone()
            }
            Type::Named(name) if name == "Unit" => "()".to_string(),
            Type::Named(name) if name == "U8" => "u8".to_string(),
            Type::Named(name) if name == "Error" => "String".to_string(),
            // E2-M7: file handle types are top-level in the prelude (not in jet_std).
            Type::Named(name) if file_handle_rust_type(name).is_some() => {
                format!("{}{}", self.root_prefix, file_handle_rust_type(name).unwrap())
            }
            // E2-M10: networking opaque types are top-level in the prelude.
            Type::Named(name) if net_handle_rust_type(name).is_some() => {
                format!("{}{}", self.root_prefix, net_handle_rust_type(name).unwrap())
            }
            Type::Named(name) if std_rust_type_name(name).is_some() => {
                format!(
                    "{}jet_std::{}",
                    self.root_prefix,
                    std_rust_type_name(name).unwrap()
                )
            }
            Type::Named(name) if self.trait_names.contains(name) => {
                format!("Box<dyn {}>", generics::user_trait_rust(name))
            }
            Type::Named(name) if self.foreign_types.contains_key(name.as_str()) => {
                let rust_mod = &self.foreign_types[name.as_str()];
                format!("{}{}::user_{name}", self.root_prefix, rust_mod)
            }
            Type::Named(name) => format!("user_{name}"),
            Type::Apply { name, args } if name == "Task" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetTask<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "Channel" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetChannel<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "Sender" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetSender<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // S58 (E2-M13): `Ptr<T>` lowers to a Rust raw pointer `*mut T`.
            // Memory safety is enforced in sema (the `@unsafe` gate); codegen
            // is dumb.
            Type::Apply { name, args } if name == syntax::TYPE_PTR && args.len() == 1 => {
                format!("*mut {}", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } => {
                if args.is_empty() {
                    format!("user_{name}")
                } else {
                    format!(
                        "user_{name}<{args}>",
                        args = args
                            .iter()
                            .map(|a| self.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Type::TraitObject(t) => format!("Box<dyn {}>", generics::user_trait_rust(t)),
            Type::Fn { params, ret } => self.rust_fn_trait(params, ret.as_deref(), false),
            Type::Tuple(fields) => tuple_struct_name(&tuple_fields_plain(fields)),
            // S76: [T#N] erases to Vec<T> at codegen (I3 — all size checks live in sema).
            Type::FixedList { elem, .. } => format!("Vec<{}>", self.rust_type(elem)),
        }
    }

    pub(crate) fn rust_fn_trait(&self, params: &[Type], ret: Option<&Type>, mut_capture: bool) -> String {
        let ps = params
            .iter()
            .map(|p| self.rust_type(p))
            .collect::<Vec<_>>()
            .join(", ");
        let r = ret
            .map(|t| self.rust_type(t))
            .unwrap_or_else(|| "()".to_string());
        let trait_name = if mut_capture { "FnMut" } else { "Fn" };
        format!("Box<dyn {}({}) -> {}>", trait_name, ps, r)
    }

    pub(crate) fn mangle_name(&self, name: &str) -> String {
        mangle(name)
    }

    pub(crate) fn type_prefix(&self, type_name: &str) -> String {
        format!("user_{}", type_name)
    }
}

pub(crate) fn rust_param_type(cx: &Cx, convention: AccessConvention, ty: &Type) -> String {
    let base = cx.rust_type(ty);
    if matches!(ty, Type::Named(n) if cx.trait_names.contains(n))
        || matches!(ty, Type::TraitObject(_))
    {
        return match convention {
            AccessConvention::Read => format!("&{base}"),
            AccessConvention::Mutate => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    if matches!(ty, Type::Named(n) if generics::is_type_var_name(n)) {
        return base;
    }
    if matches!(ty, Type::Fn { .. }) {
        return base;
    }
    match convention {
        AccessConvention::Read if ty.is_scalar() => base,
        AccessConvention::Read => format!("&{}", base),
        AccessConvention::Mutate => format!("&mut {}", base),
        AccessConvention::Move => base,
    }
}

pub(crate) fn rust_return_type(cx: &Cx, ty: &Type, is_view: bool) -> String {
    let base = cx.rust_type(ty);
    if is_view {
        format!("&{}", base)
    } else {
        base
    }
}

/// What a Jet name looks like in Rust expression position.
#[derive(Clone)]
pub(crate) struct Slot {
    pub(crate) rust_name: String,
    /// The Rust binding is a reference; emit `(*name)` to get the value.
    pub(crate) deref: bool,
    pub(crate) jet_ty: Option<Type>,
}

pub(crate) fn build_cx(prog: &Program, src: &str, file: &str) -> Cx {
    let extern_funcs = extern_func_map(&prog.items);
    build_cx_items(&prog.items, src, file, None, &extern_funcs)
}

fn extern_func_map(items: &[Item]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::ExternRust(block) = item {
            for ef in &block.functions {
                map.insert(ef.name.clone(), format!("jet_ffi_{}", ef.name));
            }
        }
    }
    map
}

pub(crate) fn bundle_extern_funcs(bundle: &ProgramBundle) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        map.extend(extern_func_map(&module.items));
    }
    map
}

pub(crate) fn build_cx_items(
    items: &[Item],
    src: &str,
    file: &str,
    link: Option<&FfiLink>,
    extern_funcs: &HashMap<String, String>,
) -> Cx {
    let mut cx = Cx {
        sigs: HashMap::new(),
        fn_types: HashMap::new(),
        method_sigs: HashMap::new(),
        consts: HashMap::new(),
        type_names: HashSet::new(),
        trait_names: HashSet::new(),
        struct_fields: HashMap::new(),
        enum_variants: HashMap::new(),
        variant_owner: HashMap::new(),
        boxed_edges: HashSet::new(),
        cloneable: HashSet::new(),
        comparable: HashSet::new(),
        partial_ord: HashSet::new(),
        src: src.to_string(),
        file: file.to_string(),
        test_mode: false,
        import_mods: HashMap::new(),
        foreign_types: HashMap::new(),
        reexport_calls: HashMap::new(),
        import_sigs: HashMap::new(),
        std_imports: HashMap::new(),
        used_std: HashSet::new(),
        root_prefix: String::new(),
        ffi_crate: link.map(|l| l.crate_name.clone()),
        extern_funcs: extern_funcs.clone(),
        code_modules: HashSet::new(),
        unqualified_inline: HashMap::new(),
        unqualified_file: HashMap::new(),
        trait_methods: HashSet::new(),
        current_fn: std::cell::RefCell::new(String::new()),
    };

    for item in items {
        match item {
            Item::Func(f) => {
                let type_params: HashSet<String> =
                    f.type_params.iter().map(|p| p.name.clone()).collect();
                cx.sigs.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|p| {
                            let conv = if matches!(&p.ty, Type::Named(n) if type_params.contains(n))
                            {
                                AccessConvention::Move
                            } else {
                                p.convention
                            };
                            (conv, p.ty.clone())
                        })
                        .collect(),
                );
                cx.fn_types.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: f.params.iter().map(|p| p.ty.clone()).collect(),
                        ret: f.return_type.clone().map(Box::new),
                    },
                );
            }
            Item::Struct(s) => {
                cx.type_names.insert(s.name.clone());
                cx.struct_fields.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .filter(|f| !f.is_stored_ref)
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                );
            }
            Item::Enum(e) => {
                cx.type_names.insert(e.name.clone());
                cx.enum_variants.insert(
                    e.name.clone(),
                    e.variants
                        .iter()
                        .map(|v| (v.name.clone(), v.payload.clone()))
                        .collect(),
                );
                for v in &e.variants {
                    cx.variant_owner.insert(v.name.clone(), e.name.clone());
                }
            }
            Item::Const(c) => {
                if c.is_comptime {
                    // Inline the evaluated literal at every reference.
                    let serialized =
                        c.ct.as_ref()
                            .map(|v| v.serialize())
                            .unwrap_or_else(|| "Default::default()".to_string());
                    cx.consts.insert(c.name.clone(), serialized);
                } else {
                    cx.consts
                        .insert(c.name.clone(), mangle(&c.name).to_uppercase());
                }
            }
            Item::ExternRust(block) => {
                for ef in &block.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
            Item::CModule(cm) => {
                // S59: C boundary functions register like extern rust so that
                // cross-module call sites resolve argument conventions.
                for ef in &cm.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
            Item::Trait(t) => {
                cx.trait_names.insert(t.name.clone());
            }
            Item::Impl(_) | Item::Test(_) | Item::Module(_) => {}
            Item::CodeModule(cm) => {
                // D-MOD2: register inline module alias and add mangled function sigs.
                if let Some(body) = &cm.body {
                    cx.code_modules.insert(cm.name.clone());
                    for inner in body {
                        if let Item::Func(f) = inner {
                            let mangled = format!("{}__{}", cm.name, f.name);
                            cx.sigs.insert(
                                mangled.clone(),
                                f.params.iter().map(|p| (p.convention, p.ty.clone())).collect(),
                            );
                            cx.fn_types.insert(
                                mangled,
                                Type::Fn {
                                    params: f.params.iter().map(|p| p.ty.clone()).collect(),
                                    ret: f.return_type.clone().map(Box::new),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    for item in items {
        match item {
            Item::Struct(s) => {
                cx.boxed_edges.extend(find_struct_box_edges(s, &cx));
                if type_is_cloneable_struct(s, &cx.type_names) {
                    cx.cloneable.insert(s.name.clone());
                }
                if type_is_comparable_struct(s, &cx.type_names) {
                    cx.comparable.insert(s.name.clone());
                }
                for (t, _) in &s.derives {
                    if t == generics::COMPARABLE {
                        cx.partial_ord.insert(s.name.clone());
                        cx.comparable.insert(s.name.clone());
                    }
                }
                for m in &s.methods {
                    cx.method_sigs
                        .insert((s.name.clone(), m.name.clone()), method_sig_params(m));
                }
            }
            Item::Enum(e) => {
                cx.boxed_edges.extend(find_enum_box_edges(e, &cx));
                if type_is_cloneable_enum(e, &cx.type_names) {
                    cx.cloneable.insert(e.name.clone());
                }
                if type_is_comparable_enum(e, &cx.type_names) {
                    cx.comparable.insert(e.name.clone());
                }
                for m in &e.methods {
                    cx.method_sigs
                        .insert((e.name.clone(), m.name.clone()), method_sig_params(m));
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    cx.method_sigs
                        .insert((i.type_name.clone(), m.name.clone()), method_sig_params(m));
                    // S62: track trait-impl methods so call sites know not to mangle.
                    if i.trait_name.is_some() {
                        cx.trait_methods.insert((i.type_name.clone(), m.name.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    cx
}

/// Parameter conventions for a method, excluding `self` — call-site args
/// align positionally with this list (the receiver is emitted separately).
fn method_sig_params(f: &Func) -> Vec<(AccessConvention, Type)> {
    f.params
        .iter()
        .filter(|p| p.name != syntax::KW_SELF)
        .map(|p| (p.convention, p.ty.clone()))
        .collect()
}

pub(crate) fn type_is_cloneable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    s.fields
        .iter()
        .all(|f| !f.is_stored_ref && field_type_cloneable(&f.ty, types))
}

pub(crate) fn type_is_cloneable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_cloneable(t, types),
        VariantPayload::Named(fs) => fs.iter().all(|f| field_type_cloneable(&f.ty, types)),
    })
}

fn field_type_cloneable(ty: &Type, types: &HashSet<String>) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            field_type_cloneable(inner, types)
        }
        Type::Map { key, value } => {
            field_type_cloneable(key, types) && field_type_cloneable(value, types)
        }
        Type::Result { ok, err } => {
            field_type_cloneable(ok, types) && field_type_cloneable(err, types)
        }
        Type::Named(n) if generics::is_type_var_name(n) => true,
        Type::Named(n) => types.contains(n),
        Type::Apply { args, .. } => args.iter().all(|a| field_type_cloneable(a, types)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_cloneable(t, types)),
        Type::TraitObject(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_cloneable(elem, types),
    }
}

pub(crate) fn type_is_comparable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    s.fields
        .iter()
        .all(|f| !f.is_stored_ref && field_type_comparable(&f.ty, types))
}

pub(crate) fn type_is_comparable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_comparable(t, types),
        VariantPayload::Named(fs) => fs.iter().all(|f| field_type_comparable(&f.ty, types)),
    })
}

pub(crate) fn field_type_comparable(ty: &Type, types: &HashSet<String>) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::Option(inner) => field_type_comparable(inner, types),
        Type::Result { ok, err } => {
            field_type_comparable(ok, types) && field_type_comparable(err, types)
        }
        Type::List(inner) => field_type_comparable(inner, types),
        Type::Named(n) if generics::is_type_var_name(n) => true,
        Type::Named(n) => types.contains(n),
        Type::Apply { args, .. } => args.iter().all(|a| field_type_comparable(a, types)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_comparable(t, types)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_comparable(elem, types),
    }
}

fn find_struct_box_edges(s: &StructDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for f in &s.fields {
        if f.is_stored_ref {
            continue;
        }
        walk_type_edge(
            &s.name,
            &f.name,
            &f.ty,
            &mut vec![s.name.clone()],
            cx,
            &mut boxed,
        );
    }
    boxed
}

fn find_enum_box_edges(e: &EnumDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(t, _) => walk_type_edge(
                &e.name,
                &v.name,
                t,
                &mut vec![e.name.clone()],
                cx,
                &mut boxed,
            ),
            VariantPayload::Named(fs) => {
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    walk_type_edge(
                        &e.name,
                        &key,
                        &f.ty,
                        &mut vec![e.name.clone()],
                        cx,
                        &mut boxed,
                    );
                }
            }
        }
    }
    boxed
}

fn walk_type_edge(
    owner: &str,
    edge: &str,
    ty: &Type,
    stack: &mut Vec<String>,
    cx: &Cx,
    boxed: &mut HashSet<(String, String)>,
) {
    match ty {
        Type::Named(n) if cx.type_names.contains(n) => {
            if stack.iter().any(|s| s == n) {
                boxed.insert((owner.to_string(), edge.to_string()));
                return;
            }
            stack.push(n.clone());
            if let Some(fields) = cx.struct_fields.get(n) {
                for (fname, fty) in fields {
                    walk_type_edge(n, fname, fty, stack, cx, boxed);
                }
            }
            if let Some(vars) = cx.enum_variants.get(n) {
                for (vname, payload) in vars {
                    match payload {
                        VariantPayload::Unit => {}
                        VariantPayload::Single(t, _) => {
                            walk_type_edge(n, vname, t, stack, cx, boxed);
                        }
                        VariantPayload::Named(fs) => {
                            for f in fs {
                                let key = format!("{}.{}", vname, f.name);
                                walk_type_edge(n, &key, &f.ty, stack, cx, boxed);
                            }
                        }
                    }
                }
            }
            stack.pop();
        }
        Type::Option(inner) | Type::List(inner) | Type::Shared(inner) => {
            walk_type_edge(owner, edge, inner, stack, cx, boxed);
        }
        Type::Map { key, value } => {
            walk_type_edge(owner, edge, key, stack, cx, boxed);
            walk_type_edge(owner, edge, value, stack, cx, boxed);
        }
        Type::Char => {}
        _ => {}
    }
}


use super::*;
use crate::Generics;
use crate::AST::{
    ConstAttr, DistinctDef, EnumDef, Expr, Field, Func, ImplDef, Item, Marker, RustConstKind,
    StrPart, StructDef, TraitImplBlock, Type, VariantPayload,
};
use std::collections::HashMap;

/// D-FIELDPOL1: the Rust expression that reads field `f` off `self` — a
/// getter call `(self).user_field()` for a computed field (it's not a struct
/// member), a plain member read `(self).user_field` otherwise. Used anywhere
/// codegen renders a field's *value* (JetShow/JetDebug, `@[Codable]` encode)
/// outside the struct's own member-list emission.
fn field_self_read(f: &Field) -> String {
    let m = mangle(&f.name);
    if f.computed.is_some() {
        format!("(self).{m}()")
    } else {
        format!("(self).{m}")
    }
}

fn type_mentions_gc(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, .. } if name == Syntax::GC_TYPE => true,
        Type::Option(inner) => type_mentions_gc(inner),
        _ => false,
    }
}

fn struct_has_view_field(cx: &Cx, s: &StructDef) -> bool {
    s.fields.iter().any(|f| cx.type_contains_view(&f.ty))
}

fn add_view_lifetime_generic(generics: String) -> String {
    if generics.is_empty() {
        "<'__jet_view>".to_string()
    } else if let Some(rest) = generics.strip_prefix('<') {
        format!("<'__jet_view, {rest}")
    } else {
        generics
    }
}

fn add_view_lifetime_arg(args: String) -> String {
    if args.is_empty() {
        "<'__jet_view>".to_string()
    } else if let Some(rest) = args.strip_prefix('<') {
        format!("<'__jet_view, {rest}")
    } else {
        args
    }
}

fn emit_gc_trace_impl(s: &StructDef, out: &mut String) {
    if !s.fields.iter().any(|f| type_mentions_gc(&f.ty)) {
        return;
    }
    let mut trace_body = String::new();
    for f in &s.fields {
        if type_mentions_gc(&f.ty) {
            trace_body.push_str(&format!(
                "        jet_gc::GcTrace::trace(&self.{}, out);\n",
                mangle(&f.name)
            ));
        }
    }
    out.push_str(&format!(
        "impl jet_gc::GcTrace for {name} {{\n    fn trace(&self, out: &mut Vec<jet_gc::ObjectId>) {{\n{trace_body}    }}\n}}\n\n",
        name = user_type_rust(&s.name),
        trace_body = trace_body
    ));
}

pub(crate) fn emit_struct(cx: &Cx, s: &StructDef, out: &mut String) {
    let has_view_field = struct_has_view_field(cx, s);
    let clone_extra = if !s.type_params.is_empty() && cx.cloneable.contains(&s.name) {
        Generics::rust_extra_clone_bounds(&s.type_params)
    } else {
        HashMap::new()
    };
    let mut type_params = if s.type_params.is_empty() {
        String::new()
    } else {
        Generics::rust_type_param_list(&s.type_params, &clone_extra)
    };
    if has_view_field {
        type_params = add_view_lifetime_generic(type_params);
    }
    let has_fn_field = s.fields.iter().any(|f| matches!(f.ty, Type::Fn { .. }));
    let mut derives: Vec<&str> = Vec::new();
    if !has_fn_field && s.type_params.is_empty() {
        derives.push("Debug");
    }
    if cx.cloneable.contains(&s.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&s.name) {
        derives.push("PartialEq");
    }
    if cx.hashable.contains(&s.name) {
        derives.push("Eq");
        derives.push("Hash");
    }
    if cx.partial_ord.contains(&s.name) {
        derives.push("PartialOrd");
    }
    // Visibility is enforced by sema (E0605); Rust-level `pub` everywhere
    // keeps cross-module references compiling (R2: sema is the gatekeeper).
    // D-REPRC1: `#layout(c)` stamps `#[repr(C)]` before any `#[derive(...)]`.
    let repr_c = s.layout == Some(crate::AST::StructLayout::C);
    if derives.is_empty() {
        if repr_c {
            out.push_str(&format!(
                "#[repr(C)]\npub struct {}{} {{\n",
                user_type_rust(&s.name),
                type_params
            ));
        } else {
            out.push_str(&format!(
                "pub struct {}{} {{\n",
                user_type_rust(&s.name),
                type_params
            ));
        }
    } else if repr_c {
        out.push_str(&format!(
            "#[repr(C)]\n#[derive({})]\npub struct {}{} {{\n",
            derives.join(", "),
            user_type_rust(&s.name),
            type_params
        ));
    } else {
        out.push_str(&format!(
            "#[derive({})]\npub struct {}{} {{\n",
            derives.join(", "),
            user_type_rust(&s.name),
            type_params
        ));
    }
    // D-FIELDPOL1: a computed field is never a Rust struct member. Sema
    // (`CheckerFieldPolicy`) already synthesized it as an ordinary method on
    // `s.methods`, so it's emitted below via the normal `emit_type_impl`
    // method-emission path — nothing extra to do here but skip it as a field.
    for f in s.fields.iter().filter(|f| f.computed.is_none()) {
        let field_ty = cx.struct_field_rust_with_view_lifetime(s, &f.name, &f.ty);
        out.push_str(&format!("    pub {}: {},\n", mangle(&f.name), field_ty));
    }
    out.push_str("}\n\n");
    emit_gc_trace_impl(s, out);
    if !s.type_params.is_empty() {
        let jetshow_extra = Generics::rust_extra_jetshow_bounds(&s.type_params);
        let mut impl_bounds = jetshow_extra.clone();
        for (k, v) in &clone_extra {
            impl_bounds
                .entry(k.clone())
                .or_default()
                .extend(v.iter().cloned());
        }
        let tp_bounds = Generics::rust_type_param_list(&s.type_params, &impl_bounds);
        let mut tp_plain = Generics::type_param_rust_list(&s.type_params);
        if has_view_field {
            tp_plain = add_view_lifetime_arg(tp_plain);
        }
        let show_body = if has_fn_field {
            format!("\"{} {{ ... }}\".to_string()", s.name)
        } else {
            let fmt_fields: String = s
                .fields
                .iter()
                .map(|f| format!("{}: {{}}", f.name))
                .collect::<Vec<_>>()
                .join(", ");
            let show_fields: String = s
                .fields
                .iter()
                .map(|f| format!("({}).jet_show()", field_self_read(f)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("format!(\"{}({})\", {})", s.name, fmt_fields, show_fields)
        };
        let tp_bounds = if has_view_field {
            add_view_lifetime_generic(tp_bounds)
        } else {
            tp_bounds
        };
        out.push_str(&format!(
            "impl{} JetShow for {}{} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
            tp_bounds,
            user_type_rust(&s.name),
            tp_plain,
            show_body
        ));
        let jetdebug_extra = Generics::rust_extra_jetdebug_bounds(&s.type_params);
        let mut debug_impl_bounds = jetdebug_extra.clone();
        for (k, v) in &clone_extra {
            debug_impl_bounds
                .entry(k.clone())
                .or_default()
                .extend(v.iter().cloned());
        }
        let mut debug_tp_bounds = Generics::rust_type_param_list(&s.type_params, &debug_impl_bounds);
        if has_view_field {
            debug_tp_bounds = add_view_lifetime_generic(debug_tp_bounds);
        }
        let debug_body = struct_jet_debug_body(s, has_fn_field);
        out.push_str(&format!(
            "impl{} JetDebug for {}{} {{\n    fn jet_debug(&self) -> String {{ {} }}\n}}\n\n",
            debug_tp_bounds,
            user_type_rust(&s.name),
            tp_plain,
            debug_body
        ));
        if !cx.display_types.contains(&s.name) {
            out.push_str(&format!(
                "impl{} JetDisplay for {}{} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n",
                tp_bounds,
                user_type_rust(&s.name),
                tp_plain,
            ));
        }
    } else {
        let show_body = if has_fn_field {
            format!("\"{} {{ ... }}\".to_string()", s.name)
        } else {
            "format!(\"{:?}\", self)".to_string()
        };
        let impl_generic = if has_view_field { "<'__jet_view>" } else { "" };
        let type_arg = if has_view_field { "<'__jet_view>" } else { "" };
        out.push_str(&format!(
            "impl{impl_generic} JetShow for {}{type_arg} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
            user_type_rust(&s.name),
            show_body
        ));
        let debug_body = struct_jet_debug_body(s, has_fn_field);
        out.push_str(&format!(
            "impl{impl_generic} JetDebug for {}{type_arg} {{\n    fn jet_debug(&self) -> String {{ {} }}\n}}\n\n",
            user_type_rust(&s.name),
            debug_body
        ));
        if !cx.display_types.contains(&s.name) {
            out.push_str(&format!(
                "impl{impl_generic} JetDisplay for {}{type_arg} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n",
                user_type_rust(&s.name),
            ));
        }
    }
    emit_struct_cli(cx, s, out);
    emit_struct_patchable(cx, s, out);
    if s.layout == Some(crate::AST::StructLayout::Columnar) {
        emit_columnar_storage(cx, s, out);
    }
}

/// D-SOA1 / D-SOA2A=C: emit the struct-of-arrays storage type for a
/// `#layout(columnar)` struct `S`. A `[S]` collection lowers to `user_S_columns`
/// (one `Vec` per field). The type exposes the v1 list surface as inherent
/// methods (`new`, `len`, `is_empty`, `push`, `gather`, `from_aos`, `iter_aos`)
/// so the existing dumb codegen routes columnar list ops through it (R1, I3). It
/// is serialization-transparent (D-SOA2D): `JetShow`/`user_Encode`/`user_Decode`
/// render the gathered AoS form, byte-identical to a `Vec<S>`.
fn emit_columnar_storage(cx: &Cx, s: &StructDef, out: &mut String) {
    // D-FIELDPOL1: a computed field is never a stored column.
    let fields: Vec<&Field> = s.fields.iter().filter(|f| f.computed.is_none()).collect();
    let name = &s.name;
    let cn = format!("user_{name}_columns");

    let mut derives: Vec<&str> = vec!["Debug"];
    if cx.cloneable.contains(name) {
        derives.push("Clone");
    }
    out.push_str(&format!("#[derive({})]\n", derives.join(", ")));
    out.push_str(&format!("pub struct {cn} {{\n"));
    for f in &fields {
        out.push_str(&format!(
            "    pub {}: Vec<{}>,\n",
            mangle(&f.name),
            cx.rust_type(&f.ty)
        ));
    }
    out.push_str("}\n\n");

    out.push_str(&format!("impl {cn} {{\n"));
    // new() — empty columns.
    out.push_str("    pub fn new() -> Self {\n        Self {\n");
    for f in &fields {
        out.push_str(&format!("            {}: Vec::new(),\n", mangle(&f.name)));
    }
    out.push_str("        }\n    }\n");
    // len / is_empty — driven by the first column (all columns stay in sync).
    let first = mangle(&fields[0].name);
    out.push_str(&format!(
        "    pub fn len(&self) -> usize {{ self.{first}.len() }}\n"
    ));
    out.push_str(&format!(
        "    pub fn is_empty(&self) -> bool {{ self.{first}.is_empty() }}\n"
    ));
    // push(S) — distribute one logical value across the columns.
    out.push_str(&format!(
        "    pub fn push(&mut self, __v: user_{name}) {{\n"
    ));
    for f in &fields {
        let m = mangle(&f.name);
        out.push_str(&format!("        self.{m}.push(__v.{m});\n"));
    }
    out.push_str("    }\n");
    // gather(i) — reconstruct the logical S at index i (cloning each column cell).
    out.push_str(&format!(
        "    pub fn gather(&self, __i: usize) -> user_{name} {{\n        user_{name} {{\n"
    ));
    for f in &fields {
        let m = mangle(&f.name);
        out.push_str(&format!("            {m}: self.{m}[__i].clone(),\n"));
    }
    out.push_str("        }\n    }\n");
    // gather_at(i) — bounds-checked index-read producing a logical S. Mirrors the
    // `jet_index_vec` panic message so `xs[i]` reports identically AoS vs columnar.
    out.push_str(&format!(
        "    pub fn gather_at(&self, __i: i64, __file: &str, __line: u32) -> user_{name} {{\n        let __len = self.len() as i64;\n        if __i < 0 || __i >= __len {{ jet_panic(__file, __line, &format!(\"the list has {{}} items, so position {{}} doesn't exist\", __len, __i)); }}\n        self.gather(__i as usize)\n    }}\n"
    ));
    // from_aos(Vec<S>) — build columns from an array-of-structs (list literals).
    out.push_str(&format!(
        "    pub fn from_aos(__xs: Vec<user_{name}>) -> Self {{\n        let mut __c = Self::new();\n        for __x in __xs {{ __c.push(__x); }}\n        __c\n    }}\n"
    ));
    // to_aos / iter_aos — materialize for any op that needs a Vec<S> view.
    out.push_str(&format!(
        "    pub fn to_aos(&self) -> Vec<user_{name}> {{ (0..self.len()).map(|__i| self.gather(__i)).collect() }}\n"
    ));
    out.push_str(&format!(
        "    pub fn iter_aos(&self) -> impl Iterator<Item = user_{name}> + '_ {{ (0..self.len()).map(move |__i| self.gather(__i)) }}\n"
    ));
    out.push_str("}\n\n");

    // JetShow — render identically to a `Vec<S>` (the AoS form), so `println`
    // output is unchanged by the layout (D-SOA2D extends to display).
    out.push_str(&format!(
        "impl JetShow for {cn} {{\n    fn jet_show(&self) -> String {{ self.to_aos().jet_show() }}\n}}\n\n"
    ));

    // Serialization transparency (D-SOA2D): encode/decode as the AoS array, so
    // `json.to_string` of a columnar list equals the plain `[S]` output. Only
    // emit the impl for the trait the element struct derives.
    // Built-in derives are expanded into ordinary trait blocks before codegen,
    // so inspect the checked protocol impls as well as any legacy marker still
    // present. This adapter belongs to the physical columnar storage type; the
    // logical struct codec itself remains the generated Jet implementation.
    let enc = s.derives.iter().any(|(t, _)| t == Generics::ENCODE)
        || s.trait_impls
            .iter()
            .any(|block| block.trait_name == Generics::ENCODE);
    let dec = s.derives.iter().any(|(t, _)| t == Generics::DECODE)
        || s.trait_impls
            .iter()
            .any(|block| block.trait_name == Generics::DECODE);
    if enc {
        out.push_str(&format!(
            "impl user_Encode for {cn} {{\n    fn jet_encode(&self) -> jet_std::DataTree {{ self.to_aos().jet_encode() }}\n}}\n\n"
        ));
    }
    if dec {
        out.push_str(&format!(
            "impl user_Decode for {cn} {{\n    fn jet_decode(__t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {{\n        let __xs: Vec<user_{name}> = <Vec<user_{name}> as user_Decode>::jet_decode(__t)?;\n        Ok(Self::from_aos(__xs))\n    }}\n}}\n\n"
        ));
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// D-CLIFLAG1 (c7cliflag): `@[Cli]` derive codegen — sibling of the
// `@[Codable]` serde codegen just above, generating onto `core.args`'s
// `ArgsSpec`/`ParsedArgs` builder (the same `jet_args_*`/`jet_parsed_*`
// prelude functions a hand-written `.flag()/.option()` chain compiles to,
// I8: no second parser). Field-shape validation already ran in sema
// (`Sema::CheckerCli::validate_cli_items`, E1305/E1306) — every field
// reaching this function is a supported shape; the `_ => unreachable!()`
// arms below are a real invariant, not a TODO.
// ──────────────────────────────────────────────────────────────────────────────

/// D-CLIFLAG1: a runtime expression converting an owned `String` (from
/// `ParsedArgs`) into `ty`. `Int`/`Float` are fallible (`return Err(...)`
/// on a bad value, in the same "core.args runtime error" voice as
/// `jet_args_parse`'s own messages — no new diagnostic code, I8); `String`
/// and `Path` are infallible conversions.
fn cli_scalar_from_string(ty: &Type, var: &str, flag: &str, root_prefix: &str) -> String {
    match ty {
        Type::Int => format!(
            "match {var}.parse::<i64>() {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a whole number\\n\\n{{}}\", {flag:?}, {var}, __spec.help())) }}"
        ),
        Type::Float => format!(
            "match {var}.parse::<f64>() {{ Ok(__n) => __n, Err(_) => return Err(format!(\"invalid value for --{{}}: `{{}}` is not a number\\n\\n{{}}\", {flag:?}, {var}, __spec.help())) }}"
        ),
        Type::String => format!("{var}.clone()"),
        Type::Named(n) if n == "Path" => format!("{root_prefix}jet_path_from(&{var})"),
        _ => unreachable!("is_cli_scalar gates this to Int/Float/String/Path"),
    }
}

/// D-CLIFLAG1: emit `__jet_cli_spec_<Name>`/`__jet_cli_decode_<Name>` for a
/// `@[Cli]`-derived struct. See the pinned field-mapping rule in
/// docs/spec/spec.md ("Typed entry-signature CLI parsing (D-CLIFLAG1)").
fn emit_struct_cli(cx: &Cx, s: &StructDef, out: &mut String) {
    let Some(schema) = jet_foundation::CliSchema::command_schema(s) else {
        return;
    };
    let cn = user_type_rust(&s.name);
    let root = &cx.root_prefix;

    let mut spec_body = String::new();
    spec_body.push_str(&format!("    let __s = {root}jet_args_spec();\n"));
    let mut decode_lines = String::new();

    // CliSchema is the checked projection shared with `jet inspect dossier`.
    // Codegen consumes it rather than reconstructing shell mapping rules.
    for input in &schema.inputs {
        let f = s
            .fields
            .iter()
            .find(|field| field.name == input.field)
            .expect("CliSchema fields originate from this struct");
        let flag = &input.flag;
        let help = &input.help;
        let m = mangle(&input.field);

        match &input.shape {
            jet_foundation::CliSchema::CliInputShape::Flag => {
                spec_body.push_str(&format!(
                    "    let __s = {root}jet_args_flag(__s, &{flag:?}.to_string(), &{help:?}.to_string());\n"
                ));
                decode_lines.push_str(&format!(
                    "    let {m}: bool = {root}jet_parsed_flag(__parsed, &{flag:?}.to_string());\n"
                ));
            }
            jet_foundation::CliSchema::CliInputShape::Value {
                optional: true, ..
            } => {
                let Type::Option(inner) = &f.ty else {
                    unreachable!("optional CliSchema input comes from an Option field")
                };
                let metavar = input.metavar.as_deref().unwrap_or("VALUE");
                spec_body.push_str(&format!(
                    "    let __s = {root}jet_args_option(__s, &{flag:?}.to_string(), &{help:?}.to_string(), &{metavar:?}.to_string());\n"
                ));
                let rust = cx.rust_type(inner);
                let conv = cli_scalar_from_string(inner, "__v", &flag, root);
                decode_lines.push_str(&format!(
                    "    let {m}: Option<{rust}> = match {root}jet_parsed_option(__parsed, &{flag:?}.to_string()) {{ Some(__v) => Some({conv}), None => None }};\n"
                ));
            }
            jet_foundation::CliSchema::CliInputShape::Value {
                optional: false,
                default,
                ..
            } => {
                let ty = &f.ty;
                let metavar = input.metavar.as_deref().unwrap_or("VALUE");
                spec_body.push_str(&format!(
                    "    let __s = {root}jet_args_option(__s, &{flag:?}.to_string(), &{help:?}.to_string(), &{metavar:?}.to_string());\n"
                ));
                let rust = cx.rust_type(ty);
                let conv = cli_scalar_from_string(ty, "__v", &flag, root);
                let absent = match default {
                    Some(jet_foundation::CliSchema::CliDefault::Value(value)) => {
                        value.serialize()
                    }
                    Some(jet_foundation::CliSchema::CliDefault::TypeDefault) => {
                        "Default::default()".to_string()
                    }
                    Some(jet_foundation::CliSchema::CliDefault::Recorded(_)) => {
                        unreachable!("recorded defaults are read from artifacts, never sema input")
                    }
                    None => format!(
                        "return Err(format!(\"missing required flag --{{}}\\n\\n{{}}\", {flag:?}, __spec.help()))"
                    ),
                };
                decode_lines.push_str(&format!(
                    "    let {m}: {rust} = match {root}jet_parsed_option(__parsed, &{flag:?}.to_string()) {{ Some(__v) => {conv}, None => {absent} }};\n"
                ));
            }
        }
    }
    spec_body.push_str("    __s\n");

    out.push_str(&format!(
        "pub(crate) fn __jet_cli_spec_{name}() -> {root}JetArgsSpec {{\n{spec_body}}}\n\n",
        name = s.name
    ));

    let inits: Vec<String> = s
        .fields
        .iter()
        .filter(|f| f.computed.is_none())
        .map(|f| mangle(&f.name))
        .collect();
    out.push_str(&format!(
        "pub(crate) fn __jet_cli_decode_{name}(__spec: &{root}JetArgsSpec, __parsed: &{root}JetParsedArgs) -> Result<{cn}, String> {{\n{decode_lines}    Ok({cn} {{ {inits} }})\n}}\n\n",
        name = s.name,
        inits = inits.join(", ")
    ));
}

// ──────────────────────────────────────────────────────────────────────────────
// D-PATCH1 (card #181): `@[Patchable]` — nested `T.Patch` + apply/diff/merge.
// ──────────────────────────────────────────────────────────────────────────────

fn emit_struct_patchable(_cx: &Cx, s: &StructDef, out: &mut String) {
    if !s
        .derives
        .iter()
        .any(|(t, _)| t == crate::Syntax::CONTRACT_PATCHABLE)
    {
        return;
    }
    let base_rust = user_type_rust(&s.name);
    let patch_name = format!("{}.Patch", s.name);
    let patch_rust = user_type_rust(&patch_name);

    let mut apply_fields = Vec::new();
    let mut diff_fields = Vec::new();
    let mut merge_fields = Vec::new();
    // D-FIELDPOL1: a computed field is never a `T.Patch` member (see
    // `Sema::CheckerPatchable`) — skip it here too, or `self.<field>`/
    // `__new.<field>` would reference a Rust field that no longer exists.
    for f in s.fields.iter().filter(|f| f.computed.is_none()) {
        let m = mangle(&f.name);
        apply_fields.push(format!(
            "{m}: __p.{m}.clone().unwrap_or_else(|| self.{m}.clone())"
        ));
        diff_fields.push(format!(
            "{m}: if __new.{m} != __old.{m} {{ Some(__new.{m}) }} else {{ None }}"
        ));
        merge_fields.push(format!("{m}: __other.{m}.or_else(|| self.{m}.clone())"));
    }

    out.push_str(&format!("impl {base_rust} {{\n"));
    out.push_str(&format!(
        "    pub fn user_apply(&self, __p: {patch_rust}) -> {base_rust} {{\n        {base_rust} {{ {} }}\n    }}\n",
        apply_fields.join(", ")
    ));
    out.push_str(&format!(
        "    pub fn user_diff(__new: {base_rust}, __old: {base_rust}) -> {patch_rust} {{\n        {patch_rust} {{ {} }}\n    }}\n",
        diff_fields.join(", ")
    ));
    out.push_str("}\n\n");

    out.push_str(&format!("impl {patch_rust} {{\n"));
    out.push_str(&format!(
        "    pub fn user_merge(&self, __other: {patch_rust}) -> {patch_rust} {{\n        {patch_rust} {{ {} }}\n    }}\n",
        merge_fields.join(", ")
    ));
    out.push_str("}\n\n");
}

/// S12/D-CLIFLAG1: Jet's only program entry is `fn run`. Rust still needs
/// `fn main`, so synthesize a wrapper. Zero-arg `run` calls straight through;
/// typed `run(args: T)` / `run(cmd: Enum)` generates down onto the same
/// `__jet_cli_spec_*`/`__jet_cli_decode_*` functions `emit_struct_cli`
/// produced, and the same `core.args` runtime surface a hand-written
/// `.parse(io.args())` call would hit (I8: no second parser).
pub(crate) fn emit_cli_entry_if_needed(
    cx: &Cx,
    items: &[Item],
    cli_items: &[Item],
    out: &mut String,
) {
    let Some(run_fn) = items.iter().find_map(|i| match i {
        Item::Func(f) if f.name == "run" => Some(f),
        _ => None,
    }) else {
        return;
    };
    if run_fn.params.is_empty() {
        if is_fallible_void_entry_return(run_fn) {
            out.push_str(
                "fn main() {\n    jet_std_env_init();\n    if let Err(__jet_err) = user_run() {\n        eprintln!(\"{}\", __jet_err);\n        std::process::exit(1);\n    }\n}\n\n",
            );
        } else {
            out.push_str("fn main() {\n    jet_std_env_init();\n    user_run();\n}\n\n");
        }
        return;
    }
    if run_fn.params.len() != 1 {
        return;
    }
    let param_ty = run_fn.params[0].ty.clone();
    let Type::Named(type_name) = &param_ty else {
        return;
    };
    let name = type_name.rsplit('.').next().unwrap_or(type_name);
    let local_type = !type_name.contains('.')
        && items.iter().any(|item| match item {
            Item::Struct(structure) => structure.name == name,
            Item::Enum(enumeration) => enumeration.name == name,
            _ => false,
        });
    let param_rust = cx.rust_type(&param_ty);
    let helper_prefix = if local_type {
        String::new()
    } else {
        param_rust
            .rsplit_once("::")
            .map(|(module, _)| format!("{module}::"))
            .unwrap_or_default()
    };
    let conv = cx
        .sigs
        .get("run")
        .and_then(|params| params.first())
        .map(|(c, _)| *c)
        .unwrap_or(run_fn.params[0].convention);
    let by_ref = rust_param_type(cx, conv, &param_ty).starts_with('&');
    let arg_expr = |value: &str| -> String {
        if by_ref {
            format!("&{value}")
        } else {
            value.to_string()
        }
    };

    if let Some(s) = cli_items.iter().find_map(|i| match i {
        Item::Struct(s) if &s.name == name => Some(s),
        _ => None,
    }) {
        if s.derives.iter().any(|(t, _)| t == "Cli") {
            out.push_str(&format!(
                "fn main() {{\n    jet_std_env_init();\n    let __argv = jet_std_io_args();\n    let __spec = {helper_prefix}__jet_cli_spec_{name}();\n    match jet_args_parse(&__spec, &__argv) {{\n        Ok(__parsed) => {{\n            if jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ println!(\"{{}}\", __spec.help()); return; }}\n            match {helper_prefix}__jet_cli_decode_{name}(&__spec, &__parsed) {{\n                Ok(__args) => {{ user_run({call_arg}); }}\n                Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n            }}\n        }}\n        Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n    }}\n}}\n\n",
                name = name,
                call_arg = arg_expr("__args"),
            ));
        }
        return;
    }

    if let Some(e) = cli_items.iter().find_map(|i| match i {
        Item::Enum(e) if &e.name == name => Some(e),
        _ => None,
    }) {
        let schema = jet_foundation::CliSchema::schema_for_type(cli_items, name)
            .expect("sema-approved enum entry has one checked command schema");
        emit_cli_subcommand_entry(
            cx,
            e,
            &schema,
            &helper_prefix,
            &param_rust,
            &arg_expr,
            out,
        );
    }
}

fn is_fallible_void_entry_return(f: &Func) -> bool {
    matches!(
        &f.return_type,
        Some(Type::Result { ok, err })
            if matches!(ok.as_ref(), Type::Named(n) if n == crate::Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n) if n == crate::Syntax::TYPE_ERROR)
    )
}

/// D-CLIFLAG1: the `enum Cmd { Serve(ServeArgs) Import(ImportArgs) }` case.
/// The first positional token picks the variant by its lowercased name; the
/// rest of argv is re-parsed against that variant's own `@[Cli]` spec. Given
/// zero arguments (no subcommand token at all — the shape the zero-arg
/// golden-example convention exercises), the generated `main` prints the
/// command list and exits 0 rather than erroring: a bare invocation asking
/// "what can this program do" is not a user mistake.
fn emit_cli_subcommand_entry(
    cx: &Cx,
    e: &EnumDef,
    schema: &jet_foundation::CliSchema::CliCommandSchema,
    helper_prefix: &str,
    enum_rust: &str,
    arg_expr: &dyn Fn(&str) -> String,
    out: &mut String,
) {
    let cmd_names: Vec<String> = schema.commands.iter().map(|command| command.name.clone()).collect();
    let usage_lines = cmd_names
        .iter()
        .map(|c| format!("  {c}"))
        .collect::<Vec<_>>()
        .join("\\n");

    let mut arms = String::new();
    for v in &e.variants {
        let VariantPayload::Single(Type::Named(payload_name), _) = &v.payload else {
            // Sema's E1307 already rejects this shape; unreachable at codegen.
            continue;
        };
        let tag = mangle_variant(&v.name);
        let ctor = format!("{enum_rust}::{tag}(__payload)");
        arms.push_str(&format!(
            "        {sub:?} => {{\n            let __spec = {helper_prefix}__jet_cli_spec_{payload}();\n            match jet_args_parse(&__spec, &__rest) {{\n                Ok(__parsed) => {{\n                    if jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ println!(\"{{}}\", __spec.help()); return; }}\n                    match {helper_prefix}__jet_cli_decode_{payload}(&__spec, &__parsed) {{\n                        Ok(__payload) => {{ user_run({call_arg}); }}\n                        Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n                    }}\n                }}\n                Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n            }}\n        }}\n",
            sub = v.name.to_lowercase(),
            payload = payload_name,
            call_arg = arg_expr(&ctor),
        ));
    }
    let _ = cx;

    out.push_str(&format!(
        "fn main() {{\n    jet_std_env_init();\n    let __argv = jet_std_io_args();\n    if __argv.len() < 2 || __argv[1] == \"--help\" {{\n        println!(\"Usage: <program> <command> [options]\\n\\nCommands:\\n{usage}\");\n        return;\n    }}\n    let __sub = __argv[1].to_lowercase();\n    let mut __rest: Vec<String> = vec![format!(\"{{}} {{}}\", __argv[0], __sub)];\n    __rest.extend_from_slice(&__argv[2..]);\n    match __sub.as_str() {{\n{arms}        __other => {{\n            eprintln!(\"unknown command `{{}}`\\n\\nknown commands: {cmds}\", __other);\n            std::process::exit(2);\n        }}\n    }}\n}}\n\n",
        usage = usage_lines,
        arms = arms,
        cmds = cmd_names.join(", "),
    ));
}

pub(crate) fn emit_enum(cx: &Cx, e: &EnumDef, out: &mut String) {
    let mut derives = vec!["Debug"];
    if cx.cloneable.contains(&e.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&e.name) {
        derives.push("PartialEq");
    }
    if cx.hashable.contains(&e.name) {
        derives.push("Eq");
        derives.push("Hash");
    }
    if let Some(tag) = e.c_layout_tag() {
        let repr = match tag {
            crate::AST::CEnumTag::CInt => "C",
            crate::AST::CEnumTag::U8 => "C, u8", crate::AST::CEnumTag::I8 => "C, i8",
            crate::AST::CEnumTag::U16 => "C, u16", crate::AST::CEnumTag::I16 => "C, i16",
            crate::AST::CEnumTag::U32 => "C, u32", crate::AST::CEnumTag::I32 => "C, i32",
            crate::AST::CEnumTag::U64 => "C, u64", crate::AST::CEnumTag::I64 => "C, i64",
        };
        emit_c_enum_declaration(e, tag, out);
        out.push_str(&format!("#[repr({repr})]\n"));
    }
    out.push_str(&format!(
        "#[derive({})]\npub enum user_{} {{\n",
        derives.join(", "),
        e.name
    ));
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {
                if let Some(d) = v.discriminant {
                    out.push_str(&format!("    {} = {},\n", mangle_variant(&v.name), d));
                } else {
                    out.push_str(&format!("    {},\n", mangle_variant(&v.name)));
                }
            }
            VariantPayload::Single(t, _) => {
                let ty = cx.field_rust_type(&e.name, &v.name, t);
                let d = v.discriminant.map(|n| format!(" = {n}")).unwrap_or_default();
                out.push_str(&format!("    {}({}){},\n", mangle_variant(&v.name), ty, d));
            }
            VariantPayload::Named(fs) => {
                out.push_str(&format!("    {} {{\n", mangle_variant(&v.name)));
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    let ty = cx.field_rust_type(&e.name, &key, &f.ty);
                    out.push_str(&format!("        {}: {},\n", mangle(&f.name), ty));
                }
                let d = v.discriminant.map(|n| format!(" = {n}")).unwrap_or_default();
                out.push_str(&format!("    }}{},\n", d));
            }
        }
    }
    out.push_str("}\n\n");
    out.push_str(&format!(
        "impl JetShow for user_{} {{\n    fn jet_show(&self) -> String {{ format!(\"{{:?}}\", self) }}\n}}\n\n",
        e.name
    ));
    out.push_str(&format!(
        "impl JetDebug for user_{} {{\n    fn jet_debug(&self) -> String {{ format!(\"{{:?}}\", self) }}\n}}\n\n",
        e.name
    ));
    if !cx.display_types.contains(&e.name) {
        out.push_str(&format!(
            "impl JetDisplay for user_{} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n",
            e.name
        ));
    }
}

fn c_decl_type(ty: &Type) -> String {
    match ty {
        Type::Int => "long long".into(), Type::Float => "double".into(),
        Type::Bool => "_Bool".into(), Type::Char => "uint32_t".into(),
        Type::IntN { signed, bits } => format!("{}int{}_t", if *signed { "" } else { "u" }, bits),
        Type::Float32 => "float".into(), Type::Named(n) => n.clone(),
        Type::Tagged { inner, .. } => c_decl_type(inner),
        _ => "/* rejected by sema */ void".into(),
    }
}

/// D-REPRC2: exact C declaration carried beside generated Rust so bindgen/header
/// extraction can copy it byte-for-byte. This is compilable C, not a prose sketch.
fn emit_c_enum_declaration(e: &EnumDef, tag: crate::AST::CEnumTag, out: &mut String) {
    let payload = e.variants.iter().any(|v| !matches!(v.payload, VariantPayload::Unit));
    out.push_str("/* D-REPRC2-C-DECL\n#include <stdint.h>\n#include <stdbool.h>\n");
    if !payload && tag == crate::AST::CEnumTag::CInt {
        out.push_str(&format!("typedef enum {} {{\n", e.name));
        for (i, v) in e.variants.iter().enumerate() {
            let d = v.discriminant.unwrap_or(i as i64);
            out.push_str(&format!("  {}_{} = {},\n", e.name, v.name.replace('.', "_"), d));
        }
        out.push_str(&format!("}} {};\n", e.name));
    } else {
        let tag_ty = match tag { crate::AST::CEnumTag::CInt => "int", crate::AST::CEnumTag::U8 => "uint8_t", crate::AST::CEnumTag::I8 => "int8_t", crate::AST::CEnumTag::U16 => "uint16_t", crate::AST::CEnumTag::I16 => "int16_t", crate::AST::CEnumTag::U32 => "uint32_t", crate::AST::CEnumTag::I32 => "int32_t", crate::AST::CEnumTag::U64 => "uint64_t", crate::AST::CEnumTag::I64 => "int64_t" };
        out.push_str(&format!("typedef {} {}_Tag;\nenum {{\n", tag_ty, e.name));
        for (i, v) in e.variants.iter().enumerate() { out.push_str(&format!("  {}_{} = {},\n", e.name, v.name.replace('.', "_"), v.discriminant.unwrap_or(i as i64))); }
        out.push_str("};\n");
        if payload {
            out.push_str(&format!("typedef union {}_Payload {{\n", e.name));
            for v in &e.variants { match &v.payload {
                VariantPayload::Unit => {}
                VariantPayload::Single(t, _) => out.push_str(&format!("  {} {};\n", c_decl_type(t), v.name.replace('.', "_"))),
                VariantPayload::Named(fs) => { out.push_str("  struct {\n"); for f in fs { out.push_str(&format!("    {} {};\n", c_decl_type(&f.ty), f.name)); } out.push_str(&format!("  }} {};\n", v.name.replace('.', "_"))); }
            }}
            out.push_str(&format!("}} {}_Payload;\ntypedef struct {} {{ {}_Tag tag; {}_Payload payload; }} {};\n", e.name, e.name, e.name, e.name, e.name));
        }
    }
    out.push_str("D-REPRC2-C-DECL */\n");
}

// ──────────────────────────────────────────────────────────────────────────────
// Shared marker/casing helpers for CLI projection, columnar storage adapters,
// and published-schema migration machinery. Struct/enum codec derives do not
// emit here: Registration renders ordinary Jet impl source and re-enters the
// parser/sema/TIR pipeline.
// ──────────────────────────────────────────────────────────────────────────────

fn serde_marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|m| m.name == name)
}
fn serde_has(markers: &[Marker], name: &str) -> bool {
    markers.iter().any(|m| m.name == name)
}
fn marker_str_arg(m: &Marker) -> Option<String> {
    match m.args.first() {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}
fn cap_word(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
// D-SERDE3 (= C): wire-casing transform on a snake_case Jet field name.
fn apply_rename_all(style: &str, name: &str) -> String {
    let words: Vec<&str> = name.split('_').filter(|w| !w.is_empty()).collect();
    match style {
        crate::Syntax::RENAME_ALL_CAMEL => {
            let mut it = words.iter();
            let first = it.next().copied().unwrap_or("").to_string();
            first + &it.map(|w| cap_word(w)).collect::<String>()
        }
        crate::Syntax::RENAME_ALL_PASCAL => words.iter().map(|w| cap_word(w)).collect(),
        crate::Syntax::RENAME_ALL_KEBAB => words.join("-"),
        crate::Syntax::RENAME_ALL_SCREAMING => words
            .iter()
            .map(|w| w.to_uppercase())
            .collect::<Vec<_>>()
            .join("_"),
        // snake (and any unrecognized — sema rejects those with E2409)
        _ => words.join("_"),
    }
}
fn container_rename_all(markers: &[Marker]) -> Option<String> {
    serde_marker(markers, crate::Syntax::ATTR_RENAME_ALL).and_then(|m| match m.args.first() {
        Some(Expr::Ident(n, _)) => Some(n.clone()),
        _ => None,
    })
}
fn field_wire_key(style: Option<&str>, f: &Field) -> String {
    if let Some(m) = serde_marker(&f.serde_markers, crate::Syntax::ATTR_RENAME) {
        if let Some(s) = marker_str_arg(m) {
            return s;
        }
    }
    match style {
        Some(st) => apply_rename_all(st, &f.name),
        None => f.name.clone(),
    }
}
/// D-MIGRATE4: the migration blocks for a struct, when the runtime chain
/// applies: `@PublishedSchema`, concrete (no type params), with at least one
/// `migration { }` block in the module. Mirrors the gate in
/// `Sema::desugar_migrations` — the two must agree on which types get runtime
/// machinery, since sema pre-lowers the converter/default functions the step
/// functions call.
fn migration_blocks<'a>(cx: &'a Cx, s: &StructDef) -> Option<&'a [crate::AST::MigrationDecl]> {
    // `@PublishedSchema struct` sets the flag; the grouped
    // `@[PublishedSchema, Codable]` spelling leaves the marker in `derives`.
    let published = s.is_published_schema
        || s.derives
            .iter()
            .any(|(t, _)| t == crate::Syntax::ATTR_PUBLISHED_SCHEMA);
    if !published || !s.type_params.is_empty() {
        return None;
    }
    let blocks = cx.migrations.get(&s.name)?;
    if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    }
}

/// The wire key a field name carries for migration shape detection: the
/// current struct's `#[Rename]`/`RenameAll` treatment when the name is a
/// current field, else the container casing style applied to the bare name
/// (fields that only exist in historical shapes can't carry markers).
fn migration_wire_key(style: Option<&str>, s: &StructDef, name: &str) -> String {
    if let Some(f) = s.fields.iter().find(|f| f.name == name) {
        return field_wire_key(style, f);
    }
    match style {
        Some(st) => apply_rename_all(st, name),
        None => name.to_string(),
    }
}

/// D-MIGRATE4: the historical field-name shapes of a published record, derived
/// at compile time by inverting the migration chain from the current shape.
/// Returns `shapes[0] = v1 (oldest) … shapes[K-1] = vK`; the current shape is
/// `v{K+1}`. Each shape is a sorted set of wire keys.
fn migration_shapes(
    style: Option<&str>,
    s: &StructDef,
    blocks: &[crate::AST::MigrationDecl],
) -> Vec<Vec<String>> {
    use crate::AST::MigrationOp;
    let mut shape: std::collections::BTreeSet<String> = s
        .fields
        .iter()
        .filter(|f| !serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP))
        .map(|f| field_wire_key(style, f))
        .collect();
    let mut shapes: Vec<Vec<String>> = Vec::with_capacity(blocks.len());
    for block in blocks.iter().rev() {
        for op in &block.ops {
            match op {
                MigrationOp::Add { field, .. } => {
                    shape.remove(&migration_wire_key(style, s, field));
                }
                MigrationOp::Remove { field, .. } => {
                    shape.insert(migration_wire_key(style, s, field));
                }
                MigrationOp::Rename { from, to, .. } => {
                    shape.remove(&migration_wire_key(style, s, to));
                    shape.insert(migration_wire_key(style, s, from));
                }
                // A type change never moves a key.
                MigrationOp::Change { .. } => {}
            }
        }
        shapes.push(shape.iter().cloned().collect());
    }
    shapes.reverse(); // oldest (v1) first
    shapes
}

/// D-MIGRATE4: the `jet_decode_traced` override emitted inside the type's
/// `user_Decode` impl. Tries the current shape first (plain `jet_decode` — the
/// cheap happy path, and the documented "prefer newest" ambiguity rule); on
/// failure detects which historical shape the data's key set matches, newest
/// first, and walks the step functions forward from there. Data matching no
/// shape returns the original decode error unchanged.
fn emit_migration_chain_walker(cx: &Cx, s: &StructDef, style: Option<&str>, out: &mut String) {
    let blocks = migration_blocks(cx, s).expect("caller checked");
    let shapes = migration_shapes(style, s, blocks);
    let k = shapes.len();
    out.push_str(&format!(
        "    // D-MIGRATE4: @PublishedSchema migration chain — v1..v{} are historical shapes.\n",
        k
    ));
    out.push_str("    fn jet_decode_traced(__t: &jet_std::DataTree) -> Result<(Self, jet_std::MigrationStatus), jet_std::DecodeError> {\n");
    out.push_str("        let __err = match Self::jet_decode(__t) {\n");
    out.push_str("            Ok(__v) => return Ok((__v, jet_std::MigrationStatus::fresh())),\n");
    out.push_str("            Err(__e) => __e,\n");
    out.push_str("        };\n");
    out.push_str("        let __keys = jet_std::jet_datatree_key_set(__t);\n");
    // Newest historical shape first (prefer the newest matching version).
    for j in (0..k).rev() {
        let shape = &shapes[j];
        let cond = if shape.is_empty() {
            "__keys.is_empty()".to_string()
        } else {
            let lits: Vec<String> = shape.iter().map(|kk| format!("{:?}", kk)).collect();
            format!(
                "__keys.len() == {} && [{}].iter().all(|__k| __keys.contains(*__k))",
                shape.len(),
                lits.join(", ")
            )
        };
        out.push_str(&format!("        if {} {{\n", cond));
        out.push_str("            let mut __pairs = match __t { jet_std::DataTree::Object(__es) => __es.clone(), _ => return Err(__err) };\n");
        out.push_str("            let mut __steps: Vec<String> = Vec::new();\n");
        for i in j..k {
            out.push_str(&format!(
                "            jet_migrate_step_{}_{}(&mut __pairs)?;\n",
                s.name,
                i + 1
            ));
            out.push_str(&format!(
                "            __steps.push(\"v{}->v{}\".to_string());\n",
                i + 1,
                i + 2
            ));
        }
        out.push_str("            let __tree = jet_std::DataTree::Object(__pairs);\n");
        out.push_str("            let __v = Self::jet_decode(&__tree)?;\n");
        out.push_str(&format!(
            "            return Ok((__v, jet_std::MigrationStatus {{ migrated: true, from: \"v{}\".to_string(), steps: __steps }}));\n",
            j + 1
        ));
        out.push_str("        }\n");
    }
    out.push_str("        Err(__err)\n");
    out.push_str("    }\n");
}

/// D-MIGRATE4: one step function per migration block —
/// `jet_migrate_step_<Type>_<i>` rewrites a decoded object's pairs from shape
/// v<i> to shape v<i+1>. `rename` moves a key, `remove` drops one, `add`
/// evaluates the sema-lowered default function and encodes it in, `change`
/// decodes the old field type, runs the sema-lowered converter (or the
/// `impl Old -> New` conversion, D-MIGRATE2B), and encodes the result back.
fn emit_migration_step_fns(cx: &Cx, s: &StructDef, style: Option<&str>, out: &mut String) {
    use crate::AST::MigrationOp;
    let blocks = migration_blocks(cx, s).expect("caller checked");
    for (idx, block) in blocks.iter().enumerate() {
        out.push_str(&format!(
            "// D-MIGRATE4: migration step v{} -> v{} for @PublishedSchema `{}`.\n",
            idx + 1,
            idx + 2,
            s.name
        ));
        out.push_str(&format!(
            "fn jet_migrate_step_{}_{}(__pairs: &mut Vec<(String, jet_std::DataTree)>) -> Result<(), jet_std::DecodeError> {{\n",
            s.name,
            idx + 1
        ));
        for op in &block.ops {
            match op {
                MigrationOp::Rename { from, to, .. } => {
                    let from_key = migration_wire_key(style, s, from);
                    let to_key = migration_wire_key(style, s, to);
                    out.push_str(&format!(
                        "    for __p in __pairs.iter_mut() {{ if __p.0 == {from_key:?} {{ __p.0 = {to_key:?}.to_string(); }} }}\n"
                    ));
                }
                MigrationOp::Remove { field, .. } => {
                    let key = migration_wire_key(style, s, field);
                    out.push_str(&format!("    __pairs.retain(|__p| __p.0 != {key:?});\n"));
                }
                MigrationOp::Add {
                    field, default_fn, ..
                } => {
                    let key = migration_wire_key(style, s, field);
                    let df = default_fn.as_deref().unwrap_or_else(|| {
                        jet_foundation::ice!(
                            None,
                            "migration `add {}` on `{}` reached codegen without its sema-lowered default function (I3)",
                            field, s.name
                        )
                    });
                    out.push_str(&format!(
                        "    __pairs.push(({key:?}.to_string(), user_Encode::jet_encode(&{}())));\n",
                        mangle(df)
                    ));
                }
                MigrationOp::Change {
                    field,
                    from_ty,
                    to_ty,
                    conv_fn,
                    ..
                } => {
                    let key = migration_wire_key(style, s, field);
                    let old_rust = cx.rust_type(from_ty);
                    // Inline `via { … }` → the sema-lowered converter fn;
                    // no `via` → the `impl Old -> New` conversion fn (D-MIGRATE2B).
                    let conv = match conv_fn {
                        Some(f) => mangle(f),
                        None => crate::Sema::error_conv_fn_name(&from_ty.name(), &to_ty.name()),
                    };
                    out.push_str(&format!(
                        "    for __p in __pairs.iter_mut() {{\n        if __p.0 == {key:?} {{\n            let __old: {old_rust} = <{old_rust} as user_Decode>::jet_decode(&__p.1).map_err(|__e| jet_std::DecodeError::under({key:?}, __e))?;\n            __p.1 = user_Encode::jet_encode(&{conv}(__old));\n        }}\n    }}\n"
                    ));
                }
            }
        }
        out.push_str("    Ok(())\n}\n\n");
    }
}

pub(crate) fn emit_type_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::AST::TypeParam],
    methods: &[Func],
    out: &mut String,
) {
    if methods.is_empty() {
        return;
    }
    let previous_type_params = cx.current_type_params.replace(
        type_params
            .iter()
            .map(|param| param.name.clone())
            .collect(),
    );
    let mut lowered = Vec::with_capacity(methods.len());
    for method in methods {
        if !TIR::tir_covers_method(method, type_name, cx) {
            jet_foundation::ice!(
                None,
                "codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
                method.name
            );
        }
        lowered.push(TIR::lower_method(method, type_name, cx));
    }
    let param_names: std::collections::HashSet<&str> =
        type_params.iter().map(|param| param.name.as_str()).collect();
    for method in &mut lowered {
        let mut clone_params = std::collections::HashSet::new();
        for ty in &method.clone_types {
            Generics::collect_type_param_mentions(ty, &param_names, &mut clone_params);
        }
        let bounds: Vec<String> = type_params
            .iter()
            .filter(|param| clone_params.contains(&param.name))
            .map(|param| format!("{}: Clone", param.name))
            .collect();
        method.generics = if bounds.is_empty() {
            String::new()
        } else {
            format!(" where {}", bounds.join(", "))
        };
    }
    let tp_use = Generics::type_param_rust_list(type_params);
    let clone_bounds = std::collections::HashMap::new();
    let tp_impl = Generics::rust_type_param_list(type_params, &clone_bounds);
    out.push_str(&format!(
        "impl{} {}{} {{\n",
        tp_impl,
        user_type_rust(type_name),
        tp_use
    ));
    for method in &lowered {
        TIR::emit_tir_func(method, cx, out);
    }
    cx.current_type_params.replace(previous_type_params);
    out.push_str("}\n\n");
}

/// Type parameters owned by a named declaration. A top-level inherent `impl`
/// has no second generic declaration in Jet; it inherits this identity from
/// its target exactly like an in-type method block.
pub(crate) fn type_params_for_name<'a>(
    items: &'a [Item],
    type_name: &str,
) -> &'a [crate::AST::TypeParam] {
    items
        .iter()
        .find_map(|item| match item {
            Item::Struct(s) if s.name == type_name => Some(s.type_params.as_slice()),
            Item::Enum(e) if e.name == type_name => Some(e.type_params.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

pub(crate) fn emit_trait_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::AST::TypeParam],
    block: &TraitImplBlock,
    struct_def: Option<&StructDef>,
    out: &mut String,
) {
    let tp_use = Generics::type_param_rust_list(type_params);
    let tp_impl = if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) {
        let mut extra: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Some(wire) = cx.serde_wire_params.get(type_name) {
            for name in wire {
                extra.entry(name.clone()).or_default().push(block.trait_name.clone());
            }
        }
        for (name, bounds) in Generics::rust_extra_clone_bounds(type_params) {
            extra.entry(name).or_default().extend(bounds);
        }
        Generics::rust_type_param_list(type_params, &extra)
    } else {
        Generics::rust_type_param_list(type_params, &std::collections::HashMap::new())
    };
    out.push_str(&format!(
        "impl{} {} for {}{} {{\n",
        tp_impl,
        Generics::user_trait_rust(&block.trait_name),
        user_type_rust(type_name),
        tp_use
    ));
    // D-LIB2: bind each associated type the trait declared (`type Item = i64;`).
    for (name, _, ty) in &block.assoc_type_impls {
        out.push_str(&format!(
            "    type {} = {};\n",
            name,
            Traits::rust_type_name(ty)
        ));
    }
    for m in &block.methods {
        emit_trait_method(cx, &block.trait_name, type_name, m, out, 1);
    }
    // R11/D-SERDE2: built-in Decode derives are now ordinary Jet trait
    // impls. Keep the migration override attached to that impl instead of the
    // retired compiler-owned serde emitter; otherwise published historical
    // data silently bypasses its migration chain.
    let migration_struct = struct_def.filter(|s|
        block.trait_name == crate::Generics::DECODE && migration_blocks(cx, s).is_some()
    );
    let migration_style = migration_struct.and_then(|s| container_rename_all(&s.serde_markers));
    if let Some(s) = migration_struct {
        emit_migration_chain_walker(cx, s, migration_style.as_deref(), out);
    }
    out.push_str("}\n\n");
    if let Some(s) = migration_struct {
        emit_migration_step_fns(cx, s, migration_style.as_deref(), out);
    }
    if block.trait_name == crate::Syntax::TRAIT_DISPLAY {
        out.push_str(&format!(
            "impl JetDisplay for {} {{\n    fn jet_display(&self) -> String {{ <{} as {}>::display(self) }}\n}}\n\n",
            user_type_rust(type_name),
            user_type_rust(type_name),
            Generics::user_trait_rust(crate::Syntax::TRAIT_DISPLAY),
        ));
    }
}

fn struct_jet_debug_body(s: &StructDef, has_fn_field: bool) -> String {
    if has_fn_field {
        return format!("\"{} {{ ... }}\".to_string()", s.name);
    }
    if s.fields.is_empty() {
        return format!("\"{} {{}}\".to_string()", s.name);
    }
    let parts: Vec<String> = s
        .fields
        .iter()
        .map(|f| {
            if f.redact {
                format!("\"{}: [redacted]\".to_string()", f.name)
            } else {
                format!(
                    "format!(\"{}: {{}}\", ({}).jet_debug())",
                    f.name,
                    field_self_read(f)
                )
            }
        })
        .collect();
    format!(
        "format!(\"{} {{{{ {{}} }}}}\", [{}].join(\", \"))",
        s.name,
        parts.join(", ")
    )
}

pub(crate) fn emit_external_trait_impl(
    cx: &Cx,
    i: &ImplDef,
    struct_def: Option<&StructDef>,
    out: &mut String,
) {
    let trait_name = i.trait_name.as_deref().unwrap_or("");
    let codec_params = if matches!(trait_name, crate::Generics::ENCODE | crate::Generics::DECODE) {
        i.methods.first().map(|m| m.type_params.as_slice()).unwrap_or(&[])
    } else {
        &[]
    };
    let tp_use = Generics::type_param_rust_list(codec_params);
    let tp_impl = if codec_params.is_empty() {
        String::new()
    } else {
        let mut extra: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Some(wire) = cx.serde_wire_params.get(&i.type_name) {
            for name in wire {
                extra.entry(name.clone()).or_default().push(trait_name.to_string());
            }
        }
        for (name, bounds) in Generics::rust_extra_clone_bounds(codec_params) {
            extra.entry(name).or_default().extend(bounds);
        }
        Generics::rust_type_param_list(codec_params, &extra)
    };
    out.push_str(&format!(
        "impl{} {} for {}{} {{\n",
        tp_impl,
        Generics::user_trait_rust(trait_name),
        user_type_rust(&i.type_name),
        tp_use,
    ));
    // D-LIB2: bind each associated type the trait declared (`type Item = i64;`).
    for (name, _, ty) in &i.assoc_type_impls {
        out.push_str(&format!(
            "    type {} = {};\n",
            name,
            Traits::rust_type_name(ty)
        ));
    }
    if let Some(field) = &i.delegation_field {
        // S62: delegation — emit forwarding methods directly to avoid the method
        // call mangling that the standard path applies (trait methods are not
        // prefixed with `user_` in Rust).
        for m in &i.methods {
            // c109 Phase 15: route a covered delegation method through the typed IR.
            // Byte-identical Rust (golden parity); the whole method is structural so
            // every fact is resolved at lowering (I3).
            if TIR::tir_covers_delegation_method(m, field, cx) {
                let tir = TIR::lower_delegation_method(m, field, cx);
                TIR::emit_tir_func(&tir, cx, out);
                continue;
            }
            // c109 Phase N: the TIR is the only codegen seam (R7). A gate-miss here is
            // a construct the typed IR does not cover — an internal compiler error
            // (I2-class, exit 101), never an AST fallback.
            jet_foundation::ice!(
                None,
                "codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
                m.name
            );
        }
    } else {
        for m in &i.methods {
            emit_trait_method(cx, trait_name, &i.type_name, m, out, 1);
        }
    }
    let migration_struct = struct_def.filter(|s|
        trait_name == crate::Generics::DECODE && migration_blocks(cx, s).is_some()
    );
    let migration_style = migration_struct.and_then(|s| container_rename_all(&s.serde_markers));
    if let Some(s) = migration_struct {
        emit_migration_chain_walker(cx, s, migration_style.as_deref(), out);
    }
    out.push_str("}\n\n");
    if let Some(s) = migration_struct {
        emit_migration_step_fns(cx, s, migration_style.as_deref(), out);
    }
    if trait_name == crate::Syntax::TRAIT_DISPLAY {
        out.push_str(&format!(
            "impl JetDisplay for {} {{\n    fn jet_display(&self) -> String {{ <{} as {}>::display(self) }}\n}}\n\n",
            user_type_rust(&i.type_name),
            user_type_rust(&i.type_name),
            Generics::user_trait_rust(crate::Syntax::TRAIT_DISPLAY),
        ));
    }
}

fn emit_trait_method(
    cx: &Cx,
    trait_name: &str,
    type_name: &str,
    f: &Func,
    out: &mut String,
    indent: usize,
) {
    // c109 Phase N: the typed IR is the only codegen seam (R7). A trait-impl
    // method always emits at indent 1 inside the `impl Trait for user_<T>` block
    // the caller opened; it lowers + emits through the TIR. A gate-miss is an
    // internal compiler error (I2-class), never an AST fallback.
    debug_assert_eq!(
        indent, 1,
        "trait methods always emit at impl-block indent 1"
    );
    if TIR::tir_covers_trait_method(f, type_name, cx, trait_name) {
        let tir = TIR::lower_trait_method(f, type_name, cx, trait_name);
        TIR::emit_tir_func(&tir, cx, out);
        return;
    }
    jet_foundation::ice!(
        None,
        "codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
        f.name
    );
}

/// D-DIST1/D-DIST3 (ratified 2026-06-19/20): emit a `#[repr(transparent)]`
/// newtype for a distinct type declaration. The inner field is `pub` so
/// codegen can access it for `.raw()` (lowers to `.0`).
pub(crate) fn emit_distinct(cx: &Cx, d: &DistinctDef, out: &mut String) {
    let base_rust = cx.rust_type(&d.base);
    // All distinct types are Debug + Clone + PartialEq (always comparable with
    // their own kind) + Copy when the base is Copy.
    let base_is_copy = matches!(d.base, Type::Int | Type::Float | Type::Bool | Type::Char);
    let mut derives = vec!["Debug", "Clone", "PartialEq"];
    if base_is_copy {
        derives.push("Copy");
    }
    if d.is_numeric {
        // PartialOrd needed for ordered comparisons; also useful for @Numeric types.
        derives.push("PartialOrd");
    }
    out.push_str(&format!(
        "#[repr(transparent)]\n#[derive({})]\npub struct user_{}(pub {});\n\n",
        derives.join(", "),
        d.name,
        base_rust
    ));
    // JetShow: display the base value wrapped in the type name.
    out.push_str(&format!(
        "impl JetShow for user_{} {{\n    fn jet_show(&self) -> String {{\n        format!(\"{}({{}})\", (self.0).jet_show())\n    }}\n}}\n\n",
        d.name, d.name
    ));
    // JetDebug: a distinct type wrapped in a struct/enum field is debug-rendered
    // via the derived container `jet_debug`, which calls `.jet_debug()` on each
    // field. D-STYLEUNIT1 (Tower c134) makes a distinct field a covered value
    // type, so the newtype must satisfy `JetDebug` — render the base value's own
    // debug wrapped in the type name (`Meters(10.0)`), mirroring `jet_show`.
    out.push_str(&format!(
        "impl JetDebug for user_{} {{\n    fn jet_debug(&self) -> String {{\n        format!(\"{}({{}})\", (self.0).jet_debug())\n    }}\n}}\n\n",
        d.name, d.name
    ));
    // .raw() method: unwrap to the base type.
    out.push_str(&format!(
        "impl user_{} {{\n    pub fn raw(&self) -> {} {{ self.0 }}\n}}\n\n",
        d.name, base_rust
    ));
    if let Some((lo, hi, _)) = d.range {
        out.push_str(&format!(
            "impl user_{n} {{\n    pub fn try_new(__v: {base}) -> Result<user_{n}, String> {{\n        if __v >= {lo} && __v <= {hi} {{ Ok(user_{n}(__v)) }} else {{ Err(format!(\"value {{}} is outside {n}'s range {lo}..{hi}\", __v)) }}\n    }}\n}}\n\n",
            n = d.name,
            base = base_rust,
            lo = lo,
            hi = hi
        ));
    }
    // @Numeric: implement Add, Sub, Mul, Div (same-type arithmetic).
    if d.is_numeric {
        for (trait_name, op) in &[("Add", "+"), ("Sub", "-"), ("Mul", "*"), ("Div", "/")] {
            if d.range.is_some() {
                out.push_str(&format!(
                    "impl std::ops::{}<user_{n}> for user_{n} {{\n    type Output = {base};\n    fn {lc}(self, rhs: user_{n}) -> {base} {{ self.0 {op} rhs.0 }}\n}}\n\n",
                    trait_name,
                    n = d.name,
                    base = base_rust,
                    lc = trait_name.to_lowercase(),
                    op = op
                ));
            } else {
                out.push_str(&format!(
                    "impl std::ops::{}<user_{n}> for user_{n} {{\n    type Output = user_{n};\n    fn {lc}(self, rhs: user_{n}) -> user_{n} {{ user_{n}(self.0 {op} rhs.0) }}\n}}\n\n",
                    trait_name,
                    n = d.name,
                    lc = trait_name.to_lowercase(),
                    op = op
                ));
            }
        }
    }
    // D-CAPBUNDLE1 `@Printable`: forward `{value}` interpolation (JetDisplay)
    // to the base value's own rendering — a distinct type starts inert, so
    // without this marker sema never lets a value reach here (E0138).
    if d.is_printable {
        out.push_str(&format!(
            "impl JetDisplay for user_{n} {{\n    fn jet_display(&self) -> String {{ (self.0).jet_display() }}\n}}\n\n",
            n = d.name
        ));
    }
    // D-CAPBUNDLE1 `@CodableAsBase`: encode/decode via the base type's own
    // wire representation (`user_Encode`/`user_Decode`, the same traits
    // struct/enum `#[Codable]` derives target — I8: one wire mechanism).
    if d.is_codable_as_base {
        out.push_str(&format!(
            "impl user_Encode for user_{n} {{\n    fn jet_encode(&self) -> jet_std::DataTree {{ (self.0).jet_encode() }}\n}}\n\n",
            n = d.name
        ));
        out.push_str(&format!(
            "impl user_Decode for user_{n} {{\n    fn jet_decode(__t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {{ Ok(user_{n}(<{base} as user_Decode>::jet_decode(__t)?)) }}\n}}\n\n",
            n = d.name,
            base = base_rust
        ));
    }
}

pub(crate) fn emit_const(c: &crate::AST::ConstDef, out: &mut String) {
    // S57 (M9.5): comptime values are inlined at use sites (registered into
    // `cx.consts`), so there is no top-level item to emit.
    if c.is_comptime {
        return;
    }
    let (val, ty) = match &c.value {
        // D-SG9: a const declared at a fixed width keeps that width.
        Expr::Int(n, _, Some((signed, bits)), _) => {
            let rust = format!("{}{}", if *signed { 'i' } else { 'u' }, bits);
            (format!("{n}{rust}"), rust)
        }
        Expr::Int(n, _, None, _) => (format!("{}i64", n), "i64".to_string()),
        Expr::Float(v, _, is_f32) => {
            if *is_f32 {
                (format!("{:?}f32", v), "f32".to_string())
            } else {
                (format!("{:?}f64", v), "f64".to_string())
            }
        }
        Expr::Bool(b, _) => (b.to_string(), "bool".to_string()),
        _ => ("0i64".to_string(), "i64".to_string()),
    };
    let inline = if c.attrs.contains(&ConstAttr::ForceInline) {
        "#[inline]\n"
    } else {
        ""
    };
    let kw = match c.rust_kind {
        RustConstKind::Const => "const",
        RustConstKind::Static => "static",
    };
    out.push_str(&format!(
        "{}{} {}: {} = {};\n\n",
        inline,
        kw,
        mangle(&c.name).to_uppercase(),
        ty,
        val
    ));
}

pub(crate) fn emit_func(cx: &Cx, f: &Func, out: &mut String) {
    // c148: expose the current function's type-parameter names so `rust_type` and
    // `rust_param_type` can recognize multi-char params (e.g. `Kind`) in addition
    // to the single-letter heuristic. Cleared on exit (normal or panic).
    *cx.current_type_params.borrow_mut() = f.type_params.iter().map(|p| p.name.clone()).collect();
    // c109 Phase N: the typed IR (TIR) is the only codegen seam (R7). Every
    // reachable function lowers + emits through the TIR; a gate-miss is an
    // internal compiler error (I2-class), never an AST fallback.
    if TIR::tir_covers(f, cx) {
        let tir = TIR::lower_func(f, cx);
        if f.pre.is_empty() && f.post.is_empty() {
            TIR::emit_tir_func(&tir, cx, out);
        } else {
            emit_func_with_contracts(cx, f, &tir, out);
        }
        cx.current_type_params.borrow_mut().clear();
        return;
    }
    cx.current_type_params.borrow_mut().clear();
    jet_foundation::ice!(
        None,
        "codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
        f.name
    );
}

/// D-PREPOST1: wrap a function's normally-emitted body with `@Pre`/`@Post`
/// runtime guards (E3005). `@Pre` clauses are checked entry guards, emitted
/// right after the opening brace. When `@Post` clauses are present, the
/// original body is wrapped in an immediately-invoked closure so `result`
/// binds the return value at every exit point — Rust's own closure `return`
/// semantics do the "checked before each return" work, instead of a bespoke
/// control-flow rewrite (R1: dumb codegen; the TIR-emitted body text is
/// reused byte-for-byte, only re-indented around it). A violated clause
/// panics via `jet_contract_fail` naming the clause's own message text.
///
/// Known v1 limitation: a `@Post` condition that reads a parameter the
/// original body *moves* can hit a Rust "used after move" error, since the
/// body now runs inside a closure that may capture that parameter by value.
/// `@Post` conditions should read `result` (and any parameter the body only
/// borrows), matching every shipped example.
fn emit_func_with_contracts(cx: &Cx, f: &Func, tir: &TIR::TFunc, out: &mut String) {
    let mut body_buf = String::new();
    TIR::emit_tir_func(tir, cx, &mut body_buf);
    // The signature line is `"{vis}{unsafe}fn name<gen>(params)[-> ret] {\n"` —
    // no bare `{`/`}` appears before the opening brace in ordinary Rust type
    // syntax, so the first `" {\n"` reliably ends the signature.
    let split_at = body_buf.find(" {\n").map(|i| i + 3).unwrap_or(0);
    let sig = &body_buf[..split_at];
    let rest = &body_buf[split_at..];
    let body_only = &rest[..rest.len().saturating_sub("}\n\n".len())];

    out.push_str(sig);
    for clause in &f.pre {
        let cond = TIR::render_contract_cond(f, &clause.cond, None, cx);
        let (_, line, _) = TIR::tir_src_line_at(&cx.src, clause.span.start);
        out.push_str(&format!(
            "    let __jet_contract_ok = {cond};\n    jet_proof_record(1, if __jet_contract_ok {{ 0 }} else {{ 1 }}, \"Pre\", &{msg}, {file}, {line});\n    if !__jet_contract_ok {{ jet_contract_fail({file}, {line}, \"Pre\", &{msg}); }}\n",
            cond = cond,
            file = escape_rust_str(&cx.file),
            line = line,
            msg = escape_rust_str(&clause.message),
        ));
    }
    if f.post.is_empty() {
        out.push_str(body_only);
    } else {
        let ret_ty = f
            .return_type
            .clone()
            .unwrap_or(crate::AST::Type::Named("Unit".to_string()));
        let ret_annot = TIR::rust_return_type(cx, &ret_ty);
        out.push_str(&format!("    let __jet_result = (|| -> {ret_annot} {{\n"));
        out.push_str(body_only);
        out.push_str("    })();\n");
        for clause in &f.post {
            let cond =
                TIR::render_contract_cond(f, &clause.cond, Some(("__jet_result", &ret_ty)), cx);
            let (_, line, _) = TIR::tir_src_line_at(&cx.src, clause.span.start);
            out.push_str(&format!(
                "    let __jet_contract_ok = {cond};\n    jet_proof_record(1, if __jet_contract_ok {{ 0 }} else {{ 1 }}, \"Post\", &{msg}, {file}, {line});\n    if !__jet_contract_ok {{ jet_contract_fail({file}, {line}, \"Post\", &{msg}); }}\n",
                cond = cond,
                file = escape_rust_str(&cx.file),
                line = line,
                msg = escape_rust_str(&clause.message),
            ));
        }
        out.push_str("    __jet_result\n");
    }
    out.push_str("}\n\n");
}

/// D-ERR-CONV: emit a standalone Rust function for `impl Source -> Target { body }`.
/// The function is called by the `map_err` closure emitted in `Expression.rs`
/// when a `TryConvert::Typed` node is encountered.
pub(crate) fn emit_error_conv(cx: &Cx, ec: &crate::AST::ErrorConvDef, out: &mut String) {
    let fn_name = crate::Sema::error_conv_fn_name(&ec.from_ty, &ec.to_ty);
    let from_rust = cx.rust_type(&crate::AST::Type::Named(ec.from_ty.clone()));
    let to_rust = cx.rust_type(&crate::AST::Type::Named(ec.to_ty.clone()));
    *cx.current_fn.borrow_mut() = fn_name.clone();
    out.push_str(&format!(
        "pub fn {fn_name}(user_self: {from}) -> {to} {{\n",
        fn_name = fn_name,
        from = from_rust,
        to = to_rust,
    ));
    // c109: the TIR is the only codegen seam (R7). The body lowers + emits through the
    // TIR; a gate-miss is an internal compiler error (I2-class), never an AST fallback.
    if TIR::tir_covers_error_conv_body(&ec.body, cx) {
        TIR::emit_tir_error_conv_body(&ec.body, &ec.from_ty, cx, out);
        out.push_str("}\n\n");
        return;
    }
    jet_foundation::ice!(
        None,
        "codegen reached an error-conversion body construct the typed IR does not cover ({} -> {}) — compiler bug (I2/R7)",
        ec.from_ty, ec.to_ty
    );
}

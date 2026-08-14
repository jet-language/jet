use crate::jet_generated_format as jet_format;
use super::*;
use crate::Generics;
use crate::AST::{
    ConstAttr, DistinctDef, EnumDef, Expr, Field, Func, ImplDef, Item, Marker, RustConstKind,
    StrPart, StructDef, TraitImplBlock, Type, VariantPayload,
};
use std::collections::HashMap;

/// D-FIELDPOL1: the Rust expression that reads field `f` off `self` — a
/// getter call `(self).__jet_field()` for a computed field (it's not a struct
/// member), a plain member read `(self).__jet_field` otherwise. Used anywhere
/// codegen renders a field's *value* (JetShow/JetDebug, `#[Codable]` encode)
/// outside the struct's own member-list emission.
fn field_self_read(f: &Field) -> String {
    let m = mangle(&f.name);
    if f.computed.is_some() {
        format!("(self).{m}()")
    } else {
        format!("(self).{m}")
    }
}

fn struct_has_view_field(cx: &Cx, s: &StructDef) -> bool {
    s.fields.iter().any(|f| cx.type_contains_view(&f.ty))
}

fn enum_has_view_payload(cx: &Cx, e: &EnumDef) -> bool {
    e.variants.iter().any(|variant| match &variant.payload {
        VariantPayload::Unit => false,
        VariantPayload::Single(ty, _) => cx.type_contains_view(ty),
        VariantPayload::Named(fields) => {
            fields.iter().any(|field| cx.type_contains_view(&field.ty))
        }
    })
}

fn nominal_shape_types(cx: &Cx, type_name: &str) -> Vec<Type> {
    if let Some(fields) = cx.struct_fields.get(type_name) {
        return fields.iter().map(|(_, ty)| ty.clone()).collect();
    }
    cx.enum_variants
        .get(type_name)
        .into_iter()
        .flat_map(|variants| variants.iter())
        .flat_map(|(_, payload)| match payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(ty, _) => vec![ty.clone()],
            VariantPayload::Named(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
        })
        .collect()
}

fn distinct_has_derive(d: &DistinctDef, name: &str) -> bool {
    d.derives.iter().any(|(derive, _)| derive == name)
}

fn add_view_lifetime_generic(generics: String) -> String {
    if generics.is_empty() {
        jet_format!("<'{jet_prefix}view>")
    } else if let Some(rest) = generics.strip_prefix('<') {
        jet_format!("<'{jet_prefix}view, {rest}")
    } else {
        generics
    }
}

fn add_view_lifetime_arg(args: String) -> String {
    if args.is_empty() {
        jet_format!("<'{jet_prefix}view>")
    } else if let Some(rest) = args.strip_prefix('<') {
        jet_format!("<'{jet_prefix}view, {rest}")
    } else {
        args
    }
}

pub(crate) fn emit_struct(cx: &Cx, s: &StructDef, out: &mut String) {
    let has_view_field = struct_has_view_field(cx, s);
    let has_mutable_view_field = s
        .fields
        .iter()
        .any(|field| cx.type_contains_mutable_view(&field.ty));
    let clone_shape: Vec<Type> = s
        .fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| field.ty.clone())
        .collect();
    let clone_extra = if !s.type_params.is_empty() && cx.cloneable.contains(&s.name) {
        Generics::rust_extra_clone_bounds_for_types(&s.type_params, &clone_shape)
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
    let has_shared_guard_field = s
        .fields
        .iter()
        .any(|f| cx.type_contains_shared_guard(&f.ty));
    // Backend representation derives only. Jet capability implementations are
    // expanded into parsed Jet items in Sema/Registration/Derives.rs; these
    // Rust attributes do not grant or validate a Jet capability.
    let mut rust_derives: Vec<&str> = Vec::new();
    if !has_fn_field && !has_shared_guard_field && s.type_params.is_empty() {
        rust_derives.push("Debug");
    }
    if cx.cloneable.contains(&s.name)
        && !has_shared_guard_field
        && !has_mutable_view_field
    {
        rust_derives.push("Clone");
    }
    if cx.hashable.contains(&s.name) && !has_shared_guard_field {
        // Eq/Hash are Rust storage traits for hash-backed collections. Keep
        // their required PartialEq representation derive separate from Jet's
        // Equatable implementation.
        rust_derives.push("PartialEq");
        rust_derives.push("Eq");
        rust_derives.push("Hash");
    }
    // Visibility is enforced by sema (E0605); Rust-level `pub` everywhere
    // keeps cross-module references compiling (R2: sema is the gatekeeper).
    // D-REPRC1: `#layout(c)` stamps `#[repr(C)]` before representation attrs.
    let repr_c = s.layout == Some(crate::AST::StructLayout::C);
    if rust_derives.is_empty() {
        if repr_c {
            out.push_str(&format!(
                "#[repr(C)]\npub struct {}{} {{\n",
                mangle_path(&s.name),
                type_params
            ));
        } else {
            out.push_str(&format!(
                "pub struct {}{} {{\n",
                mangle_path(&s.name),
                type_params
            ));
        }
    } else if repr_c {
        out.push_str(&format!(
            "#[repr(C)]\n#[derive({})]\npub struct {}{} {{\n",
            rust_derives.join(", "),
            mangle_path(&s.name),
            type_params
        ));
    } else {
        out.push_str(&format!(
            "#[derive({})]\npub struct {}{} {{\n",
            rust_derives.join(", "),
            mangle_path(&s.name),
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
    if let Some(memo_fields) = cx.memo_fields.get(&s.name) {
        for (field, ty) in memo_fields {
            let storage = crate::Syntax::memo_storage_name(field);
            let field_ty = cx.struct_field_rust_with_view_lifetime(s, field, ty);
            out.push_str(&format!(
                "    pub {storage}: {}JetMemo<{field_ty}>,\n",
                cx.root_prefix
            ));
        }
    }
    if cx.published_schemas.contains(&s.name) {
        out.push_str(&format!(
            "    pub {}: {},\n",
            crate::Syntax::PUBLISHED_UNKNOWN_FIELDS,
            cx.rust_type(&Type::Named(crate::Syntax::TYPE_DATA.to_string()))
        ));
    }
    out.push_str("}\n\n");
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
        if cx.auto_printable.contains(&s.name) && !has_shared_guard_field {
            out.push_str(&format!(
                "impl{} JetShow for {}{} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
                tp_bounds,
                mangle_path(&s.name),
                tp_plain,
                show_body
            ));
        }
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
        if cx.auto_debug.contains(&s.name) && !has_shared_guard_field {
            out.push_str(&format!(
                "impl{} JetDebug for {}{} {{\n    fn jet_debug(&self) -> String {{ {} }}\n}}\n\n",
                debug_tp_bounds,
                mangle_path(&s.name),
                tp_plain,
                debug_body
            ));
        }
        if cx.auto_printable.contains(&s.name)
            && !has_shared_guard_field
            && !cx.display_types.contains(&s.name)
        {
            out.push_str(&format!(
                "impl{} JetDisplay for {}{} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n",
                tp_bounds,
                mangle_path(&s.name),
                tp_plain,
            ));
        }
    } else {
        // I2: Rust's `{:?}` would leak the mangled `__jet_Point { __jet_x: … }`
        // form. Render Jet-source names instead — the same body `jet_debug` uses.
        let show_body = struct_jet_debug_body(s, has_fn_field);
        let impl_generic = if has_view_field {
            jet_format!("<'{jet_prefix}view>")
        } else {
            String::new()
        };
        let type_arg = if has_view_field {
            jet_format!("<'{jet_prefix}view>")
        } else {
            String::new()
        };
        if cx.auto_printable.contains(&s.name) && !has_shared_guard_field {
            out.push_str(&format!(
                "impl{impl_generic} JetShow for {}{type_arg} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
                mangle_path(&s.name),
                show_body
            ));
        }
        let debug_body = struct_jet_debug_body(s, has_fn_field);
        if cx.auto_debug.contains(&s.name) && !has_shared_guard_field {
            out.push_str(&format!(
                "impl{impl_generic} JetDebug for {}{type_arg} {{\n    fn jet_debug(&self) -> String {{ {} }}\n}}\n\n",
                mangle_path(&s.name),
                debug_body
            ));
        }
        if cx.auto_printable.contains(&s.name)
            && !has_shared_guard_field
            && !cx.display_types.contains(&s.name)
        {
            out.push_str(&format!(
                "impl{impl_generic} JetDisplay for {}{type_arg} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n",
                mangle_path(&s.name),
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
/// `#layout(columnar)` struct `S`. A `[S]` collection lowers to `__jet_S_columns`
/// (one `Vec` per field). The type exposes the v1 list surface as inherent
/// methods (`new`, `len`, `is_empty`, `push`, `gather`, `from_aos`, `iter_aos`)
/// so the existing dumb codegen routes columnar list ops through it (R1, I3). It
/// is serialization-transparent (D-SOA2D): `JetShow`/`__jet_Encode`/`__jet_Decode`
/// render the gathered AoS form, byte-identical to a `Vec<S>`.
fn emit_columnar_storage(cx: &Cx, s: &StructDef, out: &mut String) {
    // D-FIELDPOL1: a computed field is never a stored column.
    let fields: Vec<&Field> = s.fields.iter().filter(|f| f.computed.is_none()).collect();
    let name = &s.name;
    let rust_name = mangle_path(name);
    let cn = jet_foundation::Names::mangle_path(&format!("{name}_columns"));

    let mut rust_derives: Vec<&str> = vec!["Debug"];
    if cx.cloneable.contains(name) {
        rust_derives.push("Clone");
    }
    out.push_str(&format!("#[derive({})]\n", rust_derives.join(", ")));
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
        "    pub fn push(&mut self, __v: {rust_name}) {{\n",
        rust_name = rust_name
    ));
    for f in &fields {
        let m = mangle(&f.name);
        out.push_str(&format!("        self.{m}.push(__v.{m});\n"));
    }
    out.push_str("    }\n");
    // gather(i) — reconstruct the logical S at index i (cloning each column cell).
    out.push_str(&format!(
        "    pub fn gather(&self, __i: usize) -> {rust_name} {{\n        {rust_name} {{\n",
        rust_name = rust_name
    ));
    for f in &fields {
        let m = mangle(&f.name);
        out.push_str(&format!("            {m}: self.{m}[__i].clone(),\n"));
    }
    if let Some(memo_fields) = cx.memo_fields.get(&s.name) {
        for field in memo_fields.keys() {
            let storage = crate::Syntax::memo_storage_name(field);
            out.push_str(&format!(
                "            {storage}: {}JetMemo::new(),\n",
                cx.root_prefix
            ));
        }
    }
    out.push_str("        }\n    }\n");
    // gather_at(i) — bounds-checked index-read producing a logical S. Reuses the
    // shared list stop so `xs[i]` reports identically AoS vs columnar.
    out.push_str(&format!(
        "    pub fn gather_at(&self, __i: i64, __file: &str, __line: u32) -> {rust_name} {{\n        let __len = self.len() as i64;\n        if __i < 0 || __i >= __len {{ jet_arithmetic_stop(__file, __line, &jet_list_bounds_message(__len, __i)); }}\n        self.gather(__i as usize)\n    }}\n",
        rust_name = rust_name
    ));
    // from_aos(Vec<S>) — build columns from an array-of-structs (list literals).
    out.push_str(&format!(
        "    pub fn from_aos(__xs: Vec<{rust_name}>) -> Self {{\n        let mut __c = Self::new();\n        for __x in __xs {{ __c.push(__x); }}\n        __c\n    }}\n",
        rust_name = rust_name
    ));
    // to_aos / iter_aos — materialize for any op that needs a Vec<S> view.
    out.push_str(&format!(
        "    pub fn to_aos(&self) -> Vec<{rust_name}> {{ (0..self.len()).map(|__i| self.gather(__i)).collect() }}\n",
        rust_name = rust_name
    ));
    out.push_str(&format!(
        "    pub fn iter_aos(&self) -> impl Iterator<Item = {rust_name}> + '_ {{ (0..self.len()).map(move |__i| self.gather(__i)) }}\n",
        rust_name = rust_name
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
            "impl __jet_Encode for {cn} {{\n    fn jet_encode(&self) -> jet_std::DataTree {{ self.to_aos().jet_encode() }}\n}}\n\n"
        ));
    }
    if dec {
        out.push_str(&format!(
            "impl __jet_Decode for {cn} {{\n    fn jet_decode(__t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {{\n        let __xs: Vec<{rust_name}> = <Vec<{rust_name}> as __jet_Decode>::jet_decode(__t)?;\n        Ok(Self::from_aos(__xs))\n    }}\n}}\n\n",
            rust_name = rust_name
        ));
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// D-CLIFLAG1 (c7cliflag): `#[CLI]` derive codegen — sibling of the
// `#[Codable]` serde codegen just above, generating onto `core.args`'s
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

fn cli_option_spec_line(
    root: &str,
    input: &jet_foundation::CLISchema::CLIInputSchema,
    metavar: &str,
) -> String {
    let help = input.builder_help();
    let flag = &input.flag;
    let value = match input.value_kind() {
        jet_foundation::CLISchema::CLIValueKind::Int => "Int",
        jet_foundation::CLISchema::CLIValueKind::Float => "Float",
        jet_foundation::CLISchema::CLIValueKind::Bool
        | jet_foundation::CLISchema::CLIValueKind::String
        | jet_foundation::CLISchema::CLIValueKind::Path => "String",
    };
    format!(
        "    let __s = {root}jet_args_option_base(__s, &{flag:?}.to_string(), {short:?}.map(str::to_string), &{help:?}.to_string(), &{metavar:?}.to_string(), None, {env:?}.map(str::to_string), false, false, {root}JetArgValueKind::{value});\n",
        short = input.short.as_deref(),
        env = input.env.as_deref(),
    )
}

fn cli_helper_name(kind: &str, type_name: &str) -> String {
    jet_foundation::Names::mangle_path(&format!("cli_{kind}_{type_name}"))
}

/// D-CLIFLAG1: emit `__jet_cli_spec_<Name>`/`__jet_cli_decode_<Name>` for a
/// `#[CLI]`-derived struct. See the pinned field-mapping rule in
/// docs/spec/spec.md ("Typed entry-signature CLI parsing (D-CLIFLAG1)").
fn emit_struct_cli(cx: &Cx, s: &StructDef, out: &mut String) {
    let Some(schema) = jet_foundation::CLISchema::command_schema(s) else {
        return;
    };
    let cn = mangle_path(&s.name);
    let root = &cx.root_prefix;

    let mut spec_body = String::new();
    spec_body.push_str(&format!("    let __s = {root}jet_args_spec();\n"));
    if let Some(description) = &schema.description {
        spec_body.push_str(&format!(
            "    let __s = {root}jet_args_description(__s, &{description:?}.to_string());\n"
        ));
    }
    let mut decode_lines = String::new();

    // CLISchema is the checked projection shared with `jet inspect dossier`.
    // Codegen consumes it rather than reconstructing shell mapping rules.
    for input in &schema.inputs {
        let f = s
            .fields
            .iter()
            .find(|field| field.name == input.field)
            .expect("CLISchema fields originate from this struct");
        let flag = &input.flag;
        let help = &input.help;
        let m = mangle(&input.field);

        match &input.shape {
            jet_foundation::CLISchema::CLIInputShape::Flag => {
                if let Some(short) = &input.short {
                    spec_body.push_str(&format!(
                        "    let __s = {root}jet_args_flag_short(__s, &{flag:?}.to_string(), &{short:?}.to_string(), &{help:?}.to_string());\n"
                    ));
                } else {
                    spec_body.push_str(&format!(
                        "    let __s = {root}jet_args_flag(__s, &{flag:?}.to_string(), &{help:?}.to_string());\n"
                    ));
                }
                decode_lines.push_str(&format!(
                    "    let {m}: bool = {root}jet_parsed_flag(__parsed, &{flag:?}.to_string());\n"
                ));
            }
            jet_foundation::CLISchema::CLIInputShape::Value {
                optional: true, ..
            } => {
                let Type::Option(inner) = &f.ty else {
                    unreachable!("optional CLISchema input comes from an Option field")
                };
                let metavar = input.metavar.as_deref().unwrap_or("VALUE");
                spec_body.push_str(&cli_option_spec_line(root, input, metavar));
                let rust = cx.rust_type(inner);
                let conv = cli_scalar_from_string(inner, "__v", &flag, root);
                // D-FAIL-CARRIER1=A: an optional CLI input is the carrier, not an `Option`.
                decode_lines.push_str(&format!(
                    "    let {m}: {root}JetOutcome<{rust}, {root}JetAbsent> = match {root}jet_parsed_option(__parsed, &{flag:?}.to_string()) {{ Ok(__v) => Ok({conv}), Err({root}JetAbsent) => Err({root}JetAbsent) }};\n"
                ));
            }
            jet_foundation::CLISchema::CLIInputShape::Value {
                optional: false,
                default,
                ..
            } => {
                let ty = &f.ty;
                let metavar = input.metavar.as_deref().unwrap_or("VALUE");
                // Named `--flag` always accepted. Required values without a
                // default also register a same-named positional (D-CLI-POS1=A)
                // unless schema.positional is None (`#[Flag]` opt-out).
                spec_body.push_str(&cli_option_spec_line(root, input, metavar));
                if input.positional.is_some() {
                    spec_body.push_str(&format!(
                        "    let __s = {root}jet_args_positional(__s, &{flag:?}.to_string(), &{help:?}.to_string());\n"
                    ));
                }
                let rust = cx.rust_type(ty);
                let conv = cli_scalar_from_string(ty, "__v", &flag, root);
                let absent = match default {
                    Some(jet_foundation::CLISchema::CLIDefault::Value(value)) => {
                        value.serialize()
                    }
                    Some(jet_foundation::CLISchema::CLIDefault::TypeDefault) => {
                        "Default::default()".to_string()
                    }
                    Some(jet_foundation::CLISchema::CLIDefault::Recorded(_)) => {
                        unreachable!("recorded defaults are read from artifacts, never sema input")
                    }
                    None if input.positional.is_some() => format!(
                        "return Err(format!(\"missing required argument {{}}\\n\\n{{}}\", {flag:?}, __spec.help()))"
                    ),
                    None => format!(
                        "return Err(format!(\"missing required flag --{{}}\\n\\n{{}}\", {flag:?}, __spec.help()))"
                    ),
                };
                // Named wins: ArgsSpec merges bare positionals into options under
                // the same name when the named form is absent, so one decode path.
                decode_lines.push_str(&format!(
                    "    let {m}: {rust} = match {root}jet_parsed_option(__parsed, &{flag:?}.to_string()) {{ Ok(__v) => {conv}, Err(_) => {absent} }};\n"
                ));
            }
        }
    }
    spec_body.push_str("    __s\n");

    let spec_name = cli_helper_name("spec", &s.name);
    let decode_name = cli_helper_name("decode", &s.name);
    out.push_str(&format!(
        "pub(crate) fn {spec_name}() -> {root}JetArgsSpec {{\n{spec_body}}}\n\n",
        spec_name = spec_name
    ));

    let mut inits: Vec<String> = s
        .fields
        .iter()
        .filter(|f| f.computed.is_none())
        .map(|f| mangle(&f.name))
        .collect();
    if let Some(memo_fields) = cx.memo_fields.get(&s.name) {
        inits.extend(memo_fields.keys().map(|field| {
            format!(
                "{}: {}JetMemo::new()",
                crate::Syntax::memo_storage_name(field),
                cx.root_prefix,
            )
        }));
    }
    out.push_str(&format!(
        "pub(crate) fn {decode_name}(__spec: &{root}JetArgsSpec, __parsed: &{root}JetParsedArgs) -> Result<{cn}, String> {{\n{decode_lines}    Ok({cn} {{ {inits} }})\n}}\n\n",
        decode_name = decode_name,
        inits = inits.join(", ")
    ));
}

// ──────────────────────────────────────────────────────────────────────────────
// D-PATCH1 (card #181): `#[Patchable]` — nested `T.Patch` + apply/diff/merge.
// ──────────────────────────────────────────────────────────────────────────────

fn emit_struct_patchable(cx: &Cx, s: &StructDef, out: &mut String) {
    if !s
        .derives
        .iter()
        .any(|(t, _)| t == crate::Syntax::MARKER_PATCHABLE)
    {
        return;
    }
    let base_rust = mangle_path(&s.name);
    let patch_name = format!("{}.Patch", s.name);
    let patch_rust = mangle_path(&patch_name);

    let mut apply_fields = Vec::new();
    let mut diff_fields = Vec::new();
    let mut merge_fields = Vec::new();
    // D-FIELDPOL1: a computed field is never a `T.Patch` member (see
    // `Sema::CheckerPatchable`) — skip it here too, or `self.<field>`/
    // `__new.<field>` would reference a Rust field that no longer exists.
    for f in s.fields.iter().filter(|f| f.computed.is_none()) {
        let m = mangle(&f.name);
        apply_fields.push(format!(
            "{m}: __p.{m}.clone().unwrap_or_else(|_| self.{m}.clone())"
        ));
        diff_fields.push(format!(
            "{m}: if __new.{m} != __old.{m} {{ Ok(__new.{m}) }} else {{ Err(JetAbsent) }}"
        ));
        merge_fields.push(format!(
            "{m}: __other.{m}.clone().or_else(|_| self.{m}.clone())"
        ));
    }
    if let Some(memo_fields) = cx.memo_fields.get(&s.name) {
        for field in memo_fields.keys() {
            let storage = crate::Syntax::memo_storage_name(field);
            apply_fields.push(format!(
                "{storage}: {}JetMemo::new()",
                cx.root_prefix
            ));
        }
    }

    out.push_str(&format!("impl {base_rust} {{\n"));
    out.push_str(&jet_name_format!(
        "    pub fn {name_prefix}apply(&self, __p: {patch_rust}) -> {base_rust} {{\n        {base_rust} {{ {} }}\n    }}\n",
        apply_fields.join(", ")
    ));
    out.push_str(&jet_name_format!(
        "    pub fn {name_prefix}diff(__new: {base_rust}, __old: {base_rust}) -> {patch_rust} {{\n        {patch_rust} {{ {} }}\n    }}\n",
        diff_fields.join(", ")
    ));
    out.push_str("}\n\n");

    out.push_str(&format!("impl {patch_rust} {{\n"));
    out.push_str(&jet_name_format!(
        "    pub fn {name_prefix}merge(&self, __other: {patch_rust}) -> {patch_rust} {{\n        {patch_rust} {{ {} }}\n    }}\n",
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
    let job_dispatch = emit_job_dispatch(cx, items, cli_items, out);
    let run_fn = items.iter().find_map(|i| match i {
        Item::Func(f) if f.name == "run" => Some(f),
        _ => None,
    });
    let output = items.iter().find_map(|item| match item {
        Item::Const(value) => value.resolved_output.as_ref().filter(|output| output.selected),
        _ => None,
    });
    let (callable, params, entry_error, serve_app, service_target) = if let Some(output) = output {
        (
            output.lowered_name.clone(),
            output.params.clone(),
            output
                .return_type
                .as_ref()
                .and_then(|ty| entry_error(cx, ty)),
            output.return_type.as_ref().is_some_and(returns_app),
            output.kind == crate::AST::OutputKind::Service,
        )
    } else if let Some(run_fn) = run_fn {
        let params = cx.sigs.get("run").cloned().unwrap_or_else(|| {
            run_fn
                .params
                .iter()
                .map(|param| (param.convention, param.ty.clone()))
                .collect()
        });
        (
            mangle("run"),
            params,
            run_fn
                .return_type
                .as_ref()
                .and_then(|ty| entry_error(cx, ty)),
            run_fn.return_type.as_ref().is_some_and(returns_app),
            false,
        )
    } else {
        return;
    };
    let sentry_init = format!(
        "    {}jet_mem::jet_sentry_set_hardened({});\n",
        cx.root_prefix, cx.package_hardened
    );
    if params.is_empty() {
        let invoke = emit_entry_invocation(
            &callable,
            None,
            entry_error,
            serve_app,
            service_target,
            "    ",
        );
        let dispatch = job_dispatch
            .as_deref()
            .map(|name| format!("    let __argv = jet_std_io_args();\n    if {name}(&__argv) {{ return; }}\n"))
            .unwrap_or_default();
        out.push_str(&format!("fn main() {{\n    jet_std_env_init();\n{sentry_init}    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n{dispatch}{invoke}}}\n\n"));
        return;
    }
    if params.len() != 1 {
        return;
    }
    let (conv, param_ty) = params[0].clone();
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
        if s.derives.iter().any(|(t, _)| t == "CLI") {
            let spec_name = cli_helper_name("spec", name);
            let decode_name = cli_helper_name("decode", name);
            let call_arg = arg_expr("__args");
            let invoke = emit_entry_invocation(
                &callable,
                Some(&call_arg),
                entry_error,
                serve_app,
                service_target,
                "                ",
            );
            let dispatch = job_dispatch
                .as_deref()
                .map(|name| format!("    if {name}(&__argv) {{ return; }}\n"))
                .unwrap_or_default();
            let dispatch = format!("{sentry_init}{dispatch}");
            out.push_str(&format!(
                "fn main() {{\n    jet_std_env_init();\n    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n    let __argv = jet_std_io_args();\n{dispatch}    let __spec = {helper_prefix}{spec_name}();\n    match jet_args_parse(&__spec, &__argv) {{\n        Ok(__parsed) => {{\n            if jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ println!(\"{{}}\", __spec.help()); return; }}\n            match {helper_prefix}{decode_name}(&__spec, &__parsed) {{\n                Ok(__args) => {{\n{invoke}                }}\n                Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n            }}\n        }}\n        Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n    }}\n}}\n\n",
                spec_name = spec_name,
                decode_name = decode_name,
                dispatch = dispatch,
            ));
        }
        return;
    }

    if let Some(e) = cli_items.iter().find_map(|i| match i {
        Item::Enum(e) if &e.name == name => Some(e),
        _ => None,
    }) {
        let schema = jet_foundation::CLISchema::schema_for_type(cli_items, name)
            .expect("sema-approved enum entry has one checked command schema");
        emit_cli_subcommand_entry(
            cx,
            e,
            &schema,
            &helper_prefix,
            &param_rust,
            &callable,
            &arg_expr,
            entry_error,
            serve_app,
            service_target,
            job_dispatch.as_deref(),
            out,
        );
    }
}

fn emit_job_dispatch(
    cx: &Cx,
    items: &[Item],
    cli_items: &[Item],
    out: &mut String,
) -> Option<String> {
    let jobs: Vec<&Func> = items
        .iter()
        .filter_map(|item| match item {
            Item::Func(function) if function.is_task => Some(function),
            _ => None,
        })
        .collect();
    if jobs.is_empty() {
        return None;
    }
    let dispatch = mangle_generated("job_dispatch");
    let mut entries = String::new();
    for function in jobs {
        let wrapper = mangle_generated(&format!("job_{}", function.name));
        emit_job_wrapper(cx, items, cli_items, function, &wrapper, out);
        let scope = match function
            .task_metadata
            .as_ref()
            .map(|metadata| metadata.scope)
            .unwrap_or_default()
        {
            crate::AST::JobScope::Dev => "JetJobScope::Dev",
            crate::AST::JobScope::Ship => "JetJobScope::Ship",
            crate::AST::JobScope::Internal => "JetJobScope::Internal",
        };
        entries.push_str(&format!(
            "        JetJobEntry {{ name: {name:?}, scope: {scope}, invoke: {wrapper} }},\n",
            name = function.name,
        ));
    }
    out.push_str(&format!(
        "fn {dispatch}(__argv: &[String]) -> bool {{\n    let __jobs = [\n{entries}    ];\n    jet_job_dispatch(__argv, &__jobs)\n}}\n\n",
        dispatch = dispatch,
        entries = entries,
    ));
    Some(dispatch)
}

fn emit_job_wrapper(
    cx: &Cx,
    items: &[Item],
    cli_items: &[Item],
    function: &Func,
    wrapper: &str,
    out: &mut String,
) {
    let params = cx.sigs.get(&function.name).cloned().unwrap_or_else(|| {
        function
            .params
            .iter()
            .map(|param| (param.convention, param.ty.clone()))
            .collect()
    });
    let callable = mangle(&function.name);
    let entry_error = function
        .return_type
        .as_ref()
        .and_then(|ty| entry_error(cx, ty));
    let serve_app = function
        .return_type
        .as_ref()
        .is_some_and(returns_app);
    out.push_str(&format!("fn {wrapper}(__argv: &[String]) {{\n"));
    if params.is_empty() {
        out.push_str("    if __argv.get(1).is_some_and(|arg| arg == \"--help\") {\n        println!(\"Usage: {}\", __argv.first().map(String::as_str).unwrap_or(\"\"));\n        return;\n    }\n");
        out.push_str(&emit_entry_invocation(
            &callable,
            None,
            entry_error,
            serve_app,
            false,
            "    ",
        ));
        out.push_str("}\n\n");
        return;
    }
    if params.len() != 1 {
        out.push_str("    eprintln!(\"job entry accepts zero or one argument\");\n    std::process::exit(2);\n}\n\n");
        return;
    }
    let (conv, param_ty) = params[0].clone();
    let Type::Named(type_name) = &param_ty else {
        out.push_str("    eprintln!(\"job entry argument must be a named CLI type\");\n    std::process::exit(2);\n}\n\n");
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
    let by_ref = rust_param_type(cx, conv, &param_ty).starts_with('&');
    let arg_expr = |value: &str| -> String {
        if by_ref {
            format!("&{value}")
        } else {
            value.to_string()
        }
    };
    if let Some(structure) = cli_items.iter().find_map(|item| match item {
        Item::Struct(structure) if structure.name == name => Some(structure),
        _ => None,
    }) {
        if structure.derives.iter().any(|(derive, _)| derive == "CLI") {
            let spec_name = cli_helper_name("spec", name);
            let decode_name = cli_helper_name("decode", name);
            let invoke = emit_entry_invocation(
                &callable,
                Some(&arg_expr("__args")),
                entry_error,
                serve_app,
                false,
                "                ",
            );
            out.push_str(&format!(
                "    let __spec = {helper_prefix}{spec_name}();\n    match jet_args_parse(&__spec, __argv) {{\n        Ok(__parsed) => {{\n            if jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ println!(\"{{}}\", __spec.help()); return; }}\n            match {helper_prefix}{decode_name}(&__spec, &__parsed) {{\n                Ok(__args) => {{\n{invoke}                }}\n                Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n            }}\n        }}\n        Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n    }}\n}}\n\n",
                spec_name = spec_name,
                decode_name = decode_name,
            ));
            return;
        }
    }
    if let Some(enumeration) = cli_items.iter().find_map(|item| match item {
        Item::Enum(enumeration) if enumeration.name == name => Some(enumeration),
        _ => None,
    }) {
        let schema = jet_foundation::CLISchema::schema_for_type(cli_items, name)
            .expect("sema-approved enum job has one checked command schema");
        emit_cli_subcommand_job_wrapper(
            enumeration,
            &schema,
            &helper_prefix,
            &param_rust,
            &callable,
            &arg_expr,
            entry_error,
            serve_app,
            out,
        );
        return;
    }
    out.push_str("    eprintln!(\"job entry argument has no CLI schema\");\n    std::process::exit(2);\n}\n\n");
}

fn emit_cli_subcommand_job_wrapper(
    e: &EnumDef,
    schema: &jet_foundation::CLISchema::CLICommandSchema,
    helper_prefix: &str,
    enum_rust: &str,
    callable: &str,
    arg_expr: &dyn Fn(&str) -> String,
    entry_error: Option<EntryError>,
    serve_app: bool,
    out: &mut String,
) {
    let cmd_names: Vec<String> = schema.commands.iter().map(|command| command.name.clone()).collect();
    let usage_lines = cmd_names
        .iter()
        .map(|name| format!("  {name}"))
        .collect::<Vec<_>>()
        .join("\\n");
    let mut arms = String::new();
    for variant in &e.variants {
        let VariantPayload::Single(Type::Named(payload_name), _) = &variant.payload else {
            continue;
        };
        let tag = mangle_path(&variant.name);
        let call_arg = arg_expr(&format!("{enum_rust}::{tag}(__payload)"));
        let invoke = emit_entry_invocation(
            callable,
            Some(&call_arg),
            entry_error,
            serve_app,
            false,
            "                        ",
        );
        let spec_name = cli_helper_name("spec", payload_name);
        let decode_name = cli_helper_name("decode", payload_name);
        arms.push_str(&format!(
            "        {sub:?} => {{\n            let __spec = jet_args_program({helper_prefix}{spec_name}(), &__rest[0]);\n            match jet_args_parse(&__spec, &__rest) {{\n                Ok(__parsed) => {{\n                    if jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ println!(\"{{}}\", __spec.help()); return; }}\n                    match {helper_prefix}{decode_name}(&__spec, &__parsed) {{\n                        Ok(__payload) => {{\n{invoke}                        }}\n                        Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n                    }}\n                }}\n                Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n            }}\n        }}\n",
            sub = variant.name.to_lowercase(),
            spec_name = spec_name,
            decode_name = decode_name,
        ));
    }
    out.push_str(&format!(
        "    if __argv.len() < 2 || __argv[1] == \"--help\" {{\n        println!(\"Usage: {{}} <command> [options]\\n\\nCommands:\\n{usage}\", __argv.first().map(String::as_str).unwrap_or(\"\"));\n        return;\n    }}\n    let __sub = __argv[1].to_lowercase();\n    let mut __rest: Vec<String> = vec![format!(\"{{}} {{}}\", __argv[0], __sub)];\n    __rest.extend_from_slice(&__argv[2..]);\n    match __sub.as_str() {{\n{arms}        __other => {{\n            eprintln!(\"unknown command `{{}}`\", __other);\n            std::process::exit(2);\n        }}\n    }}\n}}\n\n",
        usage = usage_lines,
        arms = arms,
    ));
}

fn emit_entry_invocation(
    callable: &str,
    argument: Option<&str>,
    entry_error: Option<EntryError>,
    serve_app: bool,
    service_target: bool,
    indent: &str,
) -> String {
    let call = argument.map_or_else(|| format!("{callable}()"), |arg| format!("{callable}({arg})"));
    let error_text = |error: &str| match entry_error {
        Some(EntryError::Jet) => format!("jet_entry_error_text_jet(&{error})"),
        Some(EntryError::Rust) => format!("jet_entry_error_text(&{error})"),
        None => unreachable!("entry error text requested for an infallible entry"),
    };
    if service_target && entry_error.is_some() {
        let app = mangle_generated("service_app");
        let error = mangle_generated("service_error");
        if serve_app {
            return format!(
                "{indent}jet_runtime_boundary(|| match {call} {{\n{indent}    Ok({app}) => {app}.serve(),\n{indent}    Err({error}) => jet_service_edge_report({text}),\n{indent}}});\n",
                text = error_text(&error),
            );
        }
        return format!(
            "{indent}jet_runtime_boundary(|| {{\n{indent}    if let Err({error}) = {call} {{\n{indent}        jet_service_edge_report({text});\n{indent}    }}\n{indent}}});\n",
            text = error_text(&error),
        );
    }
    if serve_app {
        return match entry_error {
            Some(_) => format!(
                "{indent}jet_runtime_boundary(|| match {call} {{\n{indent}    Ok({app}) => {app}.serve(),\n{indent}    Err({error}) => jet_entry_error_exit({text}),\n{indent}}});\n",
                app = mangle_generated("entry_app"),
                error = mangle_generated("entry_error"),
                text = error_text(&mangle_generated("entry_error")),
            ),
            None => format!("{indent}jet_runtime_boundary(|| {call}).serve();\n"),
        };
    }
    match entry_error {
        Some(_) => {
            let error = mangle_generated("entry_error");
            format!(
                "{indent}jet_runtime_boundary(|| {{\n{indent}    if let Err({error}) = {call} {{\n{indent}        jet_entry_error_exit({text});\n{indent}    }}\n{indent}}});\n",
                text = error_text(&error),
            )
        }
        None => format!("{indent}jet_runtime_boundary(|| {call});\n"),
    }
}

#[derive(Clone, Copy)]
enum EntryError {
    Jet,
    Rust,
}

fn entry_error(cx: &Cx, ty: &Type) -> Option<EntryError> {
    let Type::Result { err, .. } = ty else {
        return None;
    };
    let uses_jet_display = match err.as_ref() {
        Type::Named(name) => {
            cx.auto_printable.contains(name) || cx.has_display_type(name)
        }
        _ => false,
    };
    Some(if uses_jet_display {
        EntryError::Jet
    } else {
        EntryError::Rust
    })
}

fn returns_app(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => name == "App",
        Type::Result { ok, .. } => matches!(ok.as_ref(), Type::Named(name) if name == "App"),
        _ => false,
    }
}

/// D-CLIFLAG1: the `enum Cmd { Serve(ServeArgs) Import(ImportArgs) }` case.
/// The first positional token picks the variant by its lowercased name; the
/// rest of argv is re-parsed against that variant's own `#[CLI]` spec. Given
/// zero arguments (no subcommand token at all — the shape the zero-arg
/// golden-example convention exercises), the generated `main` prints the
/// command list and exits 0 rather than erroring: a bare invocation asking
/// "what can this program do" is not a user mistake.
fn emit_cli_subcommand_entry(
    cx: &Cx,
    e: &EnumDef,
    schema: &jet_foundation::CLISchema::CLICommandSchema,
    helper_prefix: &str,
    enum_rust: &str,
    callable: &str,
    arg_expr: &dyn Fn(&str) -> String,
    entry_error: Option<EntryError>,
    serve_app: bool,
    service_target: bool,
    job_dispatch: Option<&str>,
    out: &mut String,
) {
    let sentry_init = format!(
        "    {}jet_mem::jet_sentry_set_hardened({});\n",
        cx.root_prefix, cx.package_hardened
    );
    let cmd_names: Vec<String> = schema.commands.iter().map(|command| command.name.clone()).collect();
    let usage_lines = cmd_names
        .iter()
        .map(|c| {
            let summary = schema
                .commands
                .iter()
                .find(|command| command.name == *c)
                .and_then(|command| command.description.as_deref())
                .and_then(|description| description.lines().next())
                .filter(|summary| !summary.is_empty());
            summary.map_or_else(|| format!("  {c}"), |summary| format!("  {c:<20} {summary}"))
        })
        .collect::<Vec<_>>()
        .join("\\n");

    let mut arms = String::new();
    for v in &e.variants {
        let VariantPayload::Single(Type::Named(payload_name), _) = &v.payload else {
            // Sema's E1307 already rejects this shape; unreachable at codegen.
            continue;
        };
        let tag = mangle_path(&v.name);
        let ctor = format!("{enum_rust}::{tag}(__payload)");
        let call_arg = arg_expr(&ctor);
        let invoke = emit_entry_invocation(
            callable,
            Some(&call_arg),
            entry_error,
            serve_app,
            service_target,
            "                        ",
        );
        let spec_name = cli_helper_name("spec", payload_name);
        let decode_name = cli_helper_name("decode", payload_name);
        let spec_init = schema
            .commands
            .iter()
            .find(|command| command.name == v.name.to_lowercase())
            .and_then(|command| command.description.as_deref())
            .map_or_else(
                || format!(
                    "            let __spec = jet_args_program({helper_prefix}{spec_name}(), &__rest[0]);\n"
                ),
                |description| {
                    format!(
                        "            let __spec = jet_args_description(jet_args_program({helper_prefix}{spec_name}(), &__rest[0]), &{description:?}.to_string());\n"
                    )
                },
            );
        arms.push_str(&format!(
            "        {sub:?} => {{\n{spec_init}            match jet_args_parse(&__spec, &__rest) {{\n                Ok(__parsed) => {{\n                    if jet_parsed_flag(&__parsed, &\"help\".to_string()) {{ println!(\"{{}}\", __spec.help()); return; }}\n                    match {helper_prefix}{decode_name}(&__spec, &__parsed) {{\n                        Ok(__payload) => {{\n{invoke}                        }}\n                        Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n                    }}\n                }}\n                Err(__e) => {{ eprintln!(\"{{}}\", __e); std::process::exit(2); }}\n            }}\n        }}\n",
            sub = v.name.to_lowercase(),
            decode_name = decode_name,
            spec_init = spec_init,
        ));
    }
    let root_description = schema
        .description
        .as_deref()
        .map(|description| format!("{description}\n\n"))
        .unwrap_or_default();
    let dispatch = format!(
        "{sentry_init}{}",
        job_dispatch
            .map(|name| format!("    if {name}(&__argv) {{ return; }}\n    if __argv.len() < 2 || __argv[1] == \"--help\" {{\n"))
            .unwrap_or_else(|| {
                "    if __argv.len() < 2 || __argv[1] == \"--help\" {\n".to_string()
            })
    );
    let main = format!(
        "fn main() {{\n    jet_std_env_init();\n    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n    let __argv = jet_std_io_args();\n    if __argv.len() < 2 || __argv[1] == \"--help\" {{\n        let __prog = jet_args_program_name(__argv.first().map(String::as_str).unwrap_or(\"\"));\n        let __description = {root_description:?};\n        println!(\"Usage: {{}} <command> [options]\\n\\n{{}}Commands:\\n{usage}\", __prog, __description);\n        return;\n    }}\n    let __sub = __argv[1].to_lowercase();\n    let mut __rest: Vec<String> = vec![format!(\"{{}} {{}}\", __argv[0], __sub)];\n    __rest.extend_from_slice(&__argv[2..]);\n    match __sub.as_str() {{\n{arms}        __other => {{\n            eprintln!(\"unknown command `{{}}`\\n\\nknown commands: {cmds}\", __other);\n            std::process::exit(2);\n        }}\n    }}\n}}\n\n",
        usage = usage_lines,
        arms = arms,
        cmds = cmd_names.join(", "),
    );
    let main = main.replacen(
        "    if __argv.len() < 2 || __argv[1] == \"--help\" {\n",
        &dispatch,
        1,
    );
    out.push_str(&main);
}

/// D-UNIONTYPE1=A: emit one compiler-generated enum per canonical anonymous
/// union used in the program. Variant tags are member type names.
pub(crate) fn emit_anonymous_unions(cx: &Cx, items: &[Item], out: &mut String) {
    let mut seen = std::collections::BTreeSet::new();
    fn walk(ty: &Type, seen: &mut std::collections::BTreeSet<String>, out_members: &mut Vec<Vec<Type>>) {
        match ty {
            Type::Union(members) => {
                let name = crate::AST::union_enum_name(members);
                if seen.insert(name) {
                    out_members.push(members.clone());
                }
                for m in members {
                    walk(m, seen, out_members);
                }
            }
            Type::List(inner) | Type::Shared(inner) | Type::Option(inner) | Type::Tagged { inner, .. } => {
                walk(inner, seen, out_members)
            }
            Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
                walk(key, seen, out_members);
                walk(value, seen, out_members);
            }
            Type::Fn { params, ret, .. } => {
                for p in params {
                    walk(p, seen, out_members);
                }
                if let Some(r) = ret {
                    walk(r, seen, out_members);
                }
            }
            Type::Apply { args, .. } => {
                for a in args {
                    walk(a, seen, out_members);
                }
            }
            Type::Tuple(fields) => {
                for (_, t) in fields {
                    walk(t, seen, out_members);
                }
            }
            Type::FixedList { elem, .. } => walk(elem, seen, out_members),
            _ => {}
        }
    }
    fn walk_item(item: &Item, seen: &mut std::collections::BTreeSet<String>, out_members: &mut Vec<Vec<Type>>) {
        match item {
            Item::Struct(s) => {
                for f in &s.fields {
                    walk(&f.ty, seen, out_members);
                }
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    match &v.payload {
                        VariantPayload::Single(ty, _) => walk(ty, seen, out_members),
                        VariantPayload::Named(fs) => {
                            for f in fs {
                                walk(&f.ty, seen, out_members);
                            }
                        }
                        VariantPayload::Unit => {}
                    }
                }
            }
            Item::Func(f) => {
                for p in &f.params {
                    walk(&p.ty, seen, out_members);
                }
                if let Some(ret) = &f.return_type {
                    walk(ret, seen, out_members);
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    for p in &m.params {
                        walk(&p.ty, seen, out_members);
                    }
                    if let Some(ret) = &m.return_type {
                        walk(ret, seen, out_members);
                    }
                }
            }
            Item::CodeModule(m) => {
                if let Some(nested) = &m.body {
                    for nested in nested {
                        walk_item(nested, seen, out_members);
                    }
                }
            }
            _ => {}
        }
    }
    let mut unions = Vec::new();
    for item in items {
        walk_item(item, &mut seen, &mut unions);
    }
    let (encode_unions, decode_unions) = union_codec_needs(items);
    for members in unions {
        let name = crate::AST::union_enum_name(&members);
        let rust_name = mangle_path(&name);
        let empty = std::collections::HashSet::new();
        let has_shared_guard = members
            .iter()
            .any(|member| cx.type_contains_shared_guard(member));
        let hashable = members
            .iter()
            .all(|m| crate::Codegen::Context::field_type_hashable(m, &cx.hashable, &empty));
        let mut rust_derives = Vec::new();
        if !has_shared_guard {
            rust_derives.extend(["Debug", "Clone", "PartialEq"]);
        }
        if !has_shared_guard && hashable {
            rust_derives.push("Eq");
            rust_derives.push("Hash");
        }
        if !rust_derives.is_empty() {
            out.push_str(&format!("#[derive({})]\n", rust_derives.join(", ")));
        }
        out.push_str(&format!("pub enum {rust_name} {{\n"));
        for m in &members {
            let tag = crate::AST::union_member_tag(m);
            out.push_str(&format!("    {tag}({}),\n", cx.rust_type(m)));
        }
        out.push_str("}\n\n");
        if !has_shared_guard {
            out.push_str(&format!(
                "impl JetShow for {rust_name} {{\n    fn jet_show(&self) -> String {{\n        match self {{\n"
            ));
            for m in &members {
                let tag = crate::AST::union_member_tag(m);
                out.push_str(&format!(
                    "            Self::{tag}(v) => crate::jet_debug_union(v.jet_show()),\n"
                ));
            }
            out.push_str("        }\n    }\n}\n\n");
            out.push_str(&format!(
                "impl JetDebug for {rust_name} {{\n    fn jet_debug(&self) -> String {{\n        match self {{\n"
            ));
            for m in &members {
                let tag = crate::AST::union_member_tag(m);
                out.push_str(&format!(
                    "            Self::{tag}(v) => crate::jet_debug_union(v.jet_debug()),\n"
                ));
            }
            out.push_str("        }\n    }\n}\n\n");
            out.push_str(&format!(
                "impl JetDisplay for {rust_name} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n"
            ));
        }
        if encode_unions.contains(&name) {
            out.push_str(&format!(
                "impl __jet_Encode for {rust_name} {{\n    fn jet_encode(&self) -> jet_std::DataTree {{\n        match self {{\n"
            ));
            for m in &members {
                let tag = crate::AST::union_member_tag(m);
                out.push_str(&format!(
                    "            Self::{tag}(v) => __jet_Encode::jet_encode(v),\n"
                ));
            }
            out.push_str("        }\n    }\n}\n\n");
        }
        if decode_unions.contains(&name) {
            out.push_str(&format!(
                "impl __jet_Decode for {rust_name} {{\n    fn jet_decode(__t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {{\n        match __t {{\n"
            ));
            for m in &members {
                let tag = crate::AST::union_member_tag(m);
                let rust = cx.rust_type(m);
                let Some(shape_pat) = union_member_datatree_pat(items, m) else {
                    continue;
                };
                out.push_str(&format!(
                    "            {shape_pat} => Ok(Self::{tag}(<{rust} as __jet_Decode>::jet_decode(__t)?)),\n"
                ));
            }
            out.push_str(
                "            _ => Err(jet_std::FieldError::one(\"no matching union member\")),\n        }\n    }\n}\n\n",
            );
        }
    }
}

fn union_codec_needs(
    items: &[Item],
) -> (
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    fn collect(
        ty: &Type,
        encode: bool,
        decode: bool,
        encodes: &mut std::collections::BTreeSet<String>,
        decodes: &mut std::collections::BTreeSet<String>,
    ) {
        if let Type::Union(members) = ty {
            let name = crate::AST::union_enum_name(members);
            if encode {
                encodes.insert(name.clone());
            }
            if decode {
                decodes.insert(name);
            }
        }
        match ty {
            Type::List(inner)
            | Type::Shared(inner)
            | Type::Option(inner)
            | Type::Tagged { inner, .. }
            | Type::FixedList { elem: inner, .. } => {
                collect(inner, encode, decode, encodes, decodes)
            }
            Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
                collect(key, encode, decode, encodes, decodes);
                collect(value, encode, decode, encodes, decodes);
            }
            Type::Apply { args, .. } | Type::Union(args) => {
                for arg in args {
                    collect(arg, encode, decode, encodes, decodes);
                }
            }
            Type::Tuple(fields) => {
                for (_, field) in fields {
                    collect(field, encode, decode, encodes, decodes);
                }
            }
            Type::Fn { params, ret, .. } => {
                for param in params {
                    collect(param, encode, decode, encodes, decodes);
                }
                if let Some(ret) = ret {
                    collect(ret, encode, decode, encodes, decodes);
                }
            }
            _ => {}
        }
    }
    fn walk_items(
        items: &[Item],
        encodes: &mut std::collections::BTreeSet<String>,
        decodes: &mut std::collections::BTreeSet<String>,
    ) {
        for item in items {
            match item {
                Item::Struct(s) => {
                    let encode = s
                        .derives
                        .iter()
                        .any(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | "Codable"));
                    let decode = s
                        .derives
                        .iter()
                        .any(|(name, _)| matches!(name.as_str(), crate::Generics::DECODE | "Codable"));
                    for field in &s.fields {
                        if !field
                            .serde_markers
                            .iter()
                            .any(|marker| marker.name == crate::Syntax::MARKER_SKIP)
                        {
                            collect(&field.ty, encode, decode, encodes, decodes);
                        }
                    }
                }
                Item::Enum(e) => {
                    let encode = e
                        .derives
                        .iter()
                        .any(|(name, _)| matches!(name.as_str(), crate::Generics::ENCODE | "Codable"));
                    let decode = e
                        .derives
                        .iter()
                        .any(|(name, _)| matches!(name.as_str(), crate::Generics::DECODE | "Codable"));
                    for variant in &e.variants {
                        match &variant.payload {
                            VariantPayload::Single(ty, _) => {
                                collect(ty, encode, decode, encodes, decodes)
                            }
                            VariantPayload::Named(fields) => {
                                for field in fields {
                                    collect(&field.ty, encode, decode, encodes, decodes);
                                }
                            }
                            VariantPayload::Unit => {}
                        }
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        walk_items(body, encodes, decodes);
                    }
                }
                _ => {}
            }
        }
    }
    let mut encodes = std::collections::BTreeSet::new();
    let mut decodes = std::collections::BTreeSet::new();
    walk_items(items, &mut encodes, &mut decodes);
    (encodes, decodes)
}

fn union_member_datatree_pat(items: &[Item], ty: &Type) -> Option<String> {
    crate::AST::resolved_decode_wire_shapes(items, ty).map(|shapes| {
        shapes
            .into_iter()
            .map(|shape| match shape {
                crate::AST::SerdeWireShape::Null => "jet_std::DataTree::Null",
                crate::AST::SerdeWireShape::Int => "jet_std::DataTree::Int(_)",
                crate::AST::SerdeWireShape::Float => "jet_std::DataTree::Float(_)",
                crate::AST::SerdeWireShape::Bool => "jet_std::DataTree::Bool(_)",
                crate::AST::SerdeWireShape::Text => "jet_std::DataTree::Text(_)",
                crate::AST::SerdeWireShape::Array => "jet_std::DataTree::Array(_)",
                crate::AST::SerdeWireShape::Object => "jet_std::DataTree::Object(_)",
            })
            .collect::<Vec<_>>()
            .join(" | ")
    })
}

pub(crate) fn emit_enum(cx: &Cx, e: &EnumDef, out: &mut String) {
    let rust_name = mangle_path(&e.name);
    let has_view_payload = enum_has_view_payload(cx, e);
    let has_mutable_view_payload = e.variants.iter().any(|variant| match &variant.payload {
        VariantPayload::Unit => false,
        VariantPayload::Single(ty, _) => cx.type_contains_mutable_view(ty),
        VariantPayload::Named(fields) => fields
            .iter()
            .any(|field| cx.type_contains_mutable_view(&field.ty)),
    });
    let has_shared_guard = e.variants.iter().any(|variant| match &variant.payload {
        VariantPayload::Unit => false,
        VariantPayload::Single(ty, _) => cx.type_contains_shared_guard(ty),
        VariantPayload::Named(fields) => fields
            .iter()
            .any(|field| cx.type_contains_shared_guard(&field.ty)),
    });
    // Debug/Clone are backend representation traits. Equality and ordering
    // are ordinary Jet impls produced by sema and are never Rust-derived here.
    let mut rust_derives = Vec::new();
    if !has_shared_guard {
        rust_derives.push("Debug");
    }
    if !has_shared_guard
        && !has_mutable_view_payload
        && cx.cloneable.contains(&e.name)
    {
        rust_derives.push("Clone");
    }
    if !has_shared_guard && cx.hashable.contains(&e.name) {
        rust_derives.push("PartialEq");
        rust_derives.push("Eq");
        rust_derives.push("Hash");
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
    if !rust_derives.is_empty() {
        out.push_str(&format!("#[derive({})]\n", rust_derives.join(", ")));
    }
    let view_generic = if has_view_payload {
        jet_format!("<'{jet_prefix}view>")
    } else {
        String::new()
    };
    out.push_str(&format!("pub enum {rust_name}{view_generic} {{\n"));
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {
                if let Some(d) = v.discriminant {
                    out.push_str(&format!("    {} = {},\n", mangle_path(&v.name), d));
                } else {
                    out.push_str(&format!("    {},\n", mangle_path(&v.name)));
                }
            }
            VariantPayload::Single(t, _) => {
                let ty = cx.enum_field_rust_with_view_lifetime(&e.name, &v.name, t);
                let d = v.discriminant.map(|n| format!(" = {n}")).unwrap_or_default();
                out.push_str(&format!("    {}({}){},\n", mangle_path(&v.name), ty, d));
            }
            VariantPayload::Named(fs) => {
                out.push_str(&format!("    {} {{\n", mangle_path(&v.name)));
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    let ty = cx.enum_field_rust_with_view_lifetime(&e.name, &key, &f.ty);
                    out.push_str(&format!("        {}: {},\n", mangle(&f.name), ty));
                }
                let d = v.discriminant.map(|n| format!(" = {n}")).unwrap_or_default();
                out.push_str(&format!("    }}{},\n", d));
            }
        }
    }
    out.push_str("}\n\n");
    let impl_generic = if has_view_payload {
        jet_format!("<'{jet_prefix}view>")
    } else {
        String::new()
    };
    let type_arg = &impl_generic;
    if !has_shared_guard && cx.auto_printable.contains(&e.name) {
        out.push_str(&format!(
            "impl{impl_generic} JetShow for {rust_name}{type_arg} {{\n    fn jet_show(&self) -> String {{ {body} }}\n}}\n\n",
            body = enum_jet_render_body(e)
        ));
    }
    if !has_shared_guard && cx.auto_debug.contains(&e.name) {
        out.push_str(&format!(
            "impl{impl_generic} JetDebug for {rust_name}{type_arg} {{\n    fn jet_debug(&self) -> String {{ {body} }}\n}}\n\n",
            body = enum_jet_render_body(e)
        ));
    }
    if !has_shared_guard
        && cx.auto_printable.contains(&e.name)
        && !cx.display_types.contains(&e.name)
    {
        out.push_str(&format!(
            "impl{impl_generic} JetDisplay for {rust_name}{type_arg} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n"
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
    if let Some(crate::AST::CtValue::Str(value)) = &m.ct {
        return Some(value.clone());
    }
    match m.args.first() {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}
// D-SERDE3 (= C) + D-ACRO-CASE1=A: wire-casing transform. Snake field names are
// the common path; PascalCase inputs use the mechanical acronym split.
fn apply_rename_all(style: &str, name: &str) -> String {
    match style {
        crate::Syntax::RENAME_ALL_CAMEL => crate::Syntax::to_camel_acronym(name),
        crate::Syntax::RENAME_ALL_PASCAL => crate::Syntax::to_pascal_acronym(name),
        crate::Syntax::RENAME_ALL_KEBAB => crate::Syntax::to_snake_acronym(name).replace('_', "-"),
        crate::Syntax::RENAME_ALL_SCREAMING => crate::Syntax::to_shouty_acronym(name),
        // snake (and any unrecognized — sema rejects those with E2409)
        _ => crate::Syntax::to_snake_acronym(name),
    }
}
pub(super) fn container_rename_all(markers: &[Marker]) -> Option<String> {
    serde_marker(markers, crate::Syntax::MARKER_RENAME_ALL).and_then(|m| match m.args.first() {
        Some(Expr::Ident(n, _)) => Some(n.clone()),
        _ => None,
    })
}
pub(super) fn field_wire_key(style: Option<&str>, f: &Field) -> String {
    if let Some(m) = serde_marker(&f.serde_markers, crate::Syntax::MARKER_RENAME) {
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
/// applies: `#PublishedSchema`, concrete (no type params), with at least one
/// `migration { }` block in the module. Mirrors the gate in
/// `Sema::desugar_migrations` — the two must agree on which types get runtime
/// machinery, since sema pre-lowers the converter/default functions the step
/// functions call.
pub(super) fn migration_blocks<'a>(
    cx: &'a Cx,
    s: &StructDef,
) -> Option<&'a [crate::AST::MigrationDecl]> {
    // `#PublishedSchema struct` sets the flag; the grouped
    // `#[PublishedSchema, Codable]` spelling leaves the marker in `derives`.
    let published = s.is_published_schema
        || s.derives
            .iter()
            .any(|(t, _)| t == crate::Syntax::MARKER_PUBLISHED_SCHEMA);
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
pub(super) fn migration_wire_key(
    style: Option<&str>,
    s: &StructDef,
    name: &str,
) -> String {
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
pub(super) fn migration_shapes(
    style: Option<&str>,
    s: &StructDef,
    blocks: &[crate::AST::MigrationDecl],
) -> Vec<Vec<String>> {
    use crate::AST::MigrationOp;
    let mut shape: std::collections::BTreeSet<String> = s
        .fields
        .iter()
        .filter(|f| !serde_has(&f.serde_markers, crate::Syntax::MARKER_SKIP))
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
/// `__jet_Decode` impl. Tries the current shape first (plain `jet_decode` — the
/// cheap happy path, and the documented "prefer newest" ambiguity rule); on
/// failure detects which historical shape the data's key set matches, newest
/// first, and walks the step functions forward from there. Data matching no
/// shape returns the original decode error unchanged.
fn emit_migration_chain_walker(cx: &Cx, s: &StructDef, style: Option<&str>, out: &mut String) {
    let blocks = migration_blocks(cx, s).expect("caller checked");
    let shapes = migration_shapes(style, s, blocks);
    let k = shapes.len();
    out.push_str(&format!(
        "    // D-MIGRATE4: #PublishedSchema migration chain — v1..v{} are historical shapes.\n",
        k
    ));
    out.push_str("    fn jet_decode_traced(__t: &jet_std::DataTree) -> Result<(Self, jet_std::MigrationStatus), Vec<jet_std::FieldError>> {\n");
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
                "            __steps.push({:?}.to_string());\n",
                crate::Codegen::TIR::migration_step_name(i)
            ));
        }
        out.push_str("            let __tree = jet_std::DataTree::Object(__pairs);\n");
        out.push_str("            let __v = Self::jet_decode(&__tree)?;\n");
        out.push_str(&format!(
            "            return Ok((__v, jet_std::MigrationStatus {{ migrated: true, from: {:?}.to_string(), steps: __steps }}));\n",
            crate::Codegen::TIR::migration_shape_name(j)
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
/// `impl Old => New` conversion, D-MIGRATE2B), and encodes the result back.
fn emit_migration_step_fns(cx: &Cx, s: &StructDef, style: Option<&str>, out: &mut String) {
    use crate::AST::MigrationOp;
    let blocks = migration_blocks(cx, s).expect("caller checked");
    for (idx, block) in blocks.iter().enumerate() {
        out.push_str(&format!(
            "// D-MIGRATE4: migration step v{} -> v{} for #PublishedSchema `{}`.\n",
            idx + 1,
            idx + 2,
            s.name
        ));
        out.push_str(&format!(
            "fn jet_migrate_step_{}_{}(__pairs: &mut Vec<(String, jet_std::DataTree)>) -> Result<(), Vec<jet_std::FieldError>> {{\n",
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
                        "    __pairs.push(({key:?}.to_string(), __jet_Encode::jet_encode(&{}())));\n",
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
                    // no `via` → the `impl Old => New` conversion fn (D-MIGRATE2B).
                    let conv = match conv_fn {
                        Some(f) => mangle(f),
                        None => crate::Sema::error_conv_fn_name(&from_ty.name(), &to_ty.name()),
                    };
                    out.push_str(&format!(
                        "    for __p in __pairs.iter_mut() {{\n        if __p.0 == {key:?} {{\n            let __old: {old_rust} = <{old_rust} as __jet_Decode>::jet_decode(&__p.1).map_err(|__e| jet_std::FieldError::under_errors({key:?}, __e))?;\n            __p.1 = __jet_Encode::jet_encode(&{conv}(__old));\n        }}\n    }}\n"
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
        let existing = method.generics.clone();
        method.generics = if bounds.is_empty() {
            existing
        } else if existing.is_empty() {
            format!(" where {}", bounds.join(", "))
        } else {
            format!("{existing} where {}", bounds.join(", "))
        };
    }
    let tp_use = Generics::type_param_rust_list(type_params);
    let clone_shape = nominal_shape_types(cx, type_name);
    let impl_bounds = if cx.cloneable.contains(type_name) {
        Generics::rust_extra_clone_bounds_for_types(type_params, &clone_shape)
    } else {
        std::collections::HashMap::new()
    };
    let tp_impl = Generics::rust_type_param_list(type_params, &impl_bounds);
    out.push_str(&format!(
        "impl{} {}{} {{\n",
        tp_impl,
        mangle_path(type_name),
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
    let lowered_methods: Vec<_> = block
        .methods
        .iter()
        .map(|method| {
            if !TIR::tir_covers_trait_method(method, type_name, cx, &block.trait_name) {
                jet_foundation::ice!(
                    None,
                    "codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
                    method.name
                );
            }
            TIR::lower_trait_method(method, type_name, cx, &block.trait_name)
        })
        .collect();
    let mut extra: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    if cx.cloneable.contains(type_name) {
        let clone_shape = struct_def
            .map(|definition| {
                definition
                    .fields
                    .iter()
                    .filter(|field| field.computed.is_none())
                    .map(|field| field.ty.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| nominal_shape_types(cx, type_name));
        for (name, bounds) in
            Generics::rust_extra_clone_bounds_for_types(type_params, &clone_shape)
        {
            extra.entry(name).or_default().extend(bounds);
        }
    }
    let param_names: std::collections::HashSet<&str> =
        type_params.iter().map(|param| param.name.as_str()).collect();
    for method in &lowered_methods {
        let mut mentions = std::collections::HashSet::new();
        for ty in &method.clone_types {
            Generics::collect_type_param_mentions(ty, &param_names, &mut mentions);
        }
        for name in mentions {
            extra.entry(name).or_default().push("Clone".to_string());
        }
    }
    if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) {
        if let Some(wire) = cx.serde_wire_params.get(type_name) {
            for name in wire {
                extra.entry(name.clone()).or_default().push(block.trait_name.clone());
            }
        }
    }
    let tp_impl = Generics::rust_type_param_list(type_params, &extra);
    out.push_str(&format!(
        "impl{} {} for {}{} {{\n",
        tp_impl,
        crate::Codegen::mangle(&block.trait_name),
        mangle_path(type_name),
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
    for method in &lowered_methods {
        TIR::emit_tir_func(method, cx, out);
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
            mangle_path(type_name),
            mangle_path(type_name),
            crate::Codegen::mangle(crate::Syntax::TRAIT_DISPLAY),
        ));
    }
    if block.trait_name == crate::Syntax::TRAIT_DEBUG {
        out.push_str(&format!(
            "impl JetDebug for {}{} {{\n    fn jet_debug(&self) -> String {{ <{}{} as __jet_Debug>::debug(self) }}\n}}\n\n",
            mangle_path(type_name),
            tp_use,
            mangle_path(type_name),
            tp_use,
        ));
    }
}

/// I2: render an enum value with Jet-source names. Rust's derived `Debug` would
/// print the mangled `__jet_Red` / `__jet_Some(__jet_x: …)` form. Payloads render
/// through `jet_debug` — the same rule struct bodies use, so a `String` payload
/// keeps its quotes in both lenses.
fn enum_jet_render_body(e: &EnumDef) -> String {
    let method = "jet_debug";
    let mut arms = String::new();
    for v in &e.variants {
        let pat = mangle_path(&v.name);
        match &v.payload {
            VariantPayload::Unit => {
                arms.push_str(&format!(
                    "            Self::{pat} => \"{}\".to_string(),\n",
                    v.name
                ));
            }
            VariantPayload::Single(_, _) => {
                arms.push_str(&format!(
                    "            Self::{pat}(__v) => format!(\"{}({{}})\", (__v).{method}()),\n",
                    v.name
                ));
            }
            VariantPayload::Named(fs) => {
                let binds: Vec<String> = fs.iter().map(|f| mangle(&f.name)).collect();
                let parts: Vec<String> = fs
                    .iter()
                    .map(|f| {
                        format!(
                            "format!(\"{}: {{}}\", ({}).{method}())",
                            f.name,
                            mangle(&f.name)
                        )
                    })
                    .collect();
                arms.push_str(&format!(
                    "            Self::{pat} {{ {} }} => format!(\"{} {{{{ {{}} }}}}\", [{}].join(\", \")),\n",
                    binds.join(", "),
                    v.name,
                    parts.join(", ")
                ));
            }
        }
    }
    format!("match self {{\n{arms}        }}")
}

fn struct_jet_debug_body(s: &StructDef, has_fn_field: bool) -> String {
    if has_fn_field {
        return format!("\"{} {{ ... }}\".to_string()", s.name);
    }
    let fields: Vec<String> = s
        .fields
        .iter()
        .map(|f| {
            if f.redact {
                format!(
                    "({:?}.to_string(), \"[redacted]\".to_string())",
                    f.name
                )
            } else {
                format!(
                    "({:?}.to_string(), ({}).jet_debug())",
                    f.name,
                    field_self_read(f)
                )
            }
        })
        .collect();
    format!(
        "crate::jet_debug_record({:?}, [{}])",
        s.name,
        fields.join(", ")
    )
}

pub(crate) fn emit_external_trait_impl(
    cx: &Cx,
    i: &ImplDef,
    struct_def: Option<&StructDef>,
    out: &mut String,
) {
    let trait_name = i.trait_name.as_deref().unwrap_or("");
    let impl_params = if matches!(trait_name, crate::Generics::ENCODE | crate::Generics::DECODE) {
        i.methods.first().map(|m| m.type_params.as_slice()).unwrap_or(&[])
    } else {
        // A top-level trait impl inherits its target's generic parameters.
        // This is the same owner scope used by an in-type trait block and by
        // a typed derive body expanded into this item.
        struct_def.map(|definition| definition.type_params.as_slice()).unwrap_or(&[])
    };
    let tp_use = Generics::type_param_rust_list(impl_params);
    let tp_impl = if impl_params.is_empty() {
        String::new()
    } else {
        let mut extra: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Some(wire) = cx.serde_wire_params.get(&i.type_name) {
            for name in wire {
                extra.entry(name.clone()).or_default().push(trait_name.to_string());
            }
        }
        let clone_shape = struct_def
            .map(|definition| {
                definition
                    .fields
                    .iter()
                    .filter(|field| field.computed.is_none())
                    .map(|field| field.ty.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (name, bounds) in
            Generics::rust_extra_clone_bounds_for_types(impl_params, &clone_shape)
        {
            extra.entry(name).or_default().extend(bounds);
        }
        Generics::rust_type_param_list(impl_params, &extra)
    };
    out.push_str(&format!(
        "impl{} {} for {}{} {{\n",
        tp_impl,
        crate::Codegen::mangle(trait_name),
        mangle_path(&i.type_name),
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
        // prefixed with `__jet_` in Rust).
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
            mangle_path(&i.type_name),
            mangle_path(&i.type_name),
            crate::Codegen::mangle(crate::Syntax::TRAIT_DISPLAY),
        ));
    }
    if trait_name == crate::Syntax::TRAIT_DEBUG {
        out.push_str(&format!(
            "impl JetDebug for {}{} {{\n    fn jet_debug(&self) -> String {{ <{}{} as __jet_Debug>::debug(self) }}\n}}\n\n",
            mangle_path(&i.type_name),
            tp_use,
            mangle_path(&i.type_name),
            tp_use,
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
    // method always emits at indent 1 inside the `impl Trait for __jet_<T>` block
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
    let rust_name = mangle_path(&d.name);
    let base_rust = cx.rust_type(&d.base);
    // Backend representation derives only. The distinct type's Jet
    // Equatable/Comparable implementations come from the sema source path.
    let base_is_copy = matches!(d.base, Type::Int | Type::Float | Type::Bool | Type::Char);
    let mut rust_derives = vec!["Debug", "Clone"];
    if base_is_copy {
        rust_derives.push("Copy");
    }
    if distinct_has_derive(d, crate::Syntax::MARKER_NUMERIC)
        && !distinct_has_derive(d, crate::Generics::COMPARABLE)
    {
        // #Numeric keeps its specialized native ordering rule. #Comparable
        // dispatches through the sema-generated Jet hook instead.
        // Rust's `PartialOrd` derive requires `PartialEq`; this is a backend
        // representation prerequisite, not a new Jet capability.
        rust_derives.push("PartialEq");
        rust_derives.push("PartialOrd");
    }
    out.push_str(&format!(
        "#[repr(transparent)]\n#[derive({})]\npub struct {rust_name}(pub {base});\n\n",
        rust_derives.join(", "),
        base = base_rust
    ));
    // Unit-family print follows Display so an explicit Display impl overrides
    // the generated magnitude + symbol default. Other distinct types keep
    // their existing debug-shaped JetShow output.
    if cx.unit_labels.contains_key(&d.name) {
        out.push_str(&format!(
            "impl JetShow for {rust_name} {{\n    fn jet_show(&self) -> String {{ self.jet_display() }}\n}}\n\n"
        ));
    } else {
        out.push_str(&format!(
            "impl JetShow for {rust_name} {{\n    fn jet_show(&self) -> String {{\n        format!(\"{display_name}({{}})\", (self.0).jet_show())\n    }}\n}}\n\n",
            display_name = d.name
        ));
    }
    // JetDebug: a distinct type wrapped in a struct/enum field is debug-rendered
    // via the derived container `jet_debug`, which calls `.jet_debug()` on each
    // field. D-STYLEUNIT1 (Tower c134) makes a distinct field a covered value
    // type, so the newtype must satisfy `JetDebug` — render the base value's own
    // debug wrapped in the type name (`Meters(10.0)`), mirroring `jet_show`.
    out.push_str(&format!(
        "impl JetDebug for {rust_name} {{\n    fn jet_debug(&self) -> String {{\n        format!(\"{display_name}({{}})\", (self.0).jet_debug())\n    }}\n}}\n\n",
        display_name = d.name
    ));
    // .raw() method: unwrap to the base type.
    let raw_value = if base_is_copy { "self.0" } else { "self.0.clone()" };
    out.push_str(&format!(
        "impl {rust_name} {{\n    pub fn raw(&self) -> {base} {{ {raw} }}\n}}\n\n",
        base = base_rust,
        raw = raw_value
    ));
    if d.quantity.is_some() {
        out.push_str(&format!(
            "impl crate::JetQuantity for {rust_name} {{\n    fn raw(&self) -> f64 {{ self.0 }}\n    fn from_float(value: f64) -> Self {{ {rust_name}(value) }}\n}}\n\n"
        ));
    }
    if let Some((lo, hi, _)) = d.range {
        out.push_str(&format!(
            "impl {rust_name} {{\n    pub fn try_new(__v: {base}) -> Result<{rust_name}, String> {{\n        if __v >= {lo} && __v <= {hi} {{ Ok({rust_name}(__v)) }} else {{ Err(format!(\"value {{}} is outside {display_name}'s range {lo}..{hi}\", __v)) }}\n    }}\n}}\n\n",
            display_name = d.name,
            base = base_rust,
            lo = lo,
            hi = hi
        ));
    }
    // #Numeric: implement Add, Sub, Mul, Div (same-type arithmetic).
    if distinct_has_derive(d, crate::Syntax::MARKER_NUMERIC) {
        for (trait_name, op) in &[("Add", "+"), ("Sub", "-"), ("Mul", "*"), ("Div", "/")] {
            if d.range.is_none() {
                out.push_str(&jet_name_format!(
                    "impl {name_prefix}{trait_name} for {rust_name} {{ fn {method}(&self, rhs: &Self) -> Self {{ {rust_name}(self.0 {op} rhs.0) }} }}\n\n",
                    trait_name = trait_name,
                    method = trait_name.to_lowercase(),
                    op = op
                ));
            }
            if d.range.is_some() {
                out.push_str(&format!(
                    "impl std::ops::{trait_name}<{rust_name}> for {rust_name} {{\n    type Output = {base};\n    fn {lc}(self, rhs: {rust_name}) -> {base} {{ self.0 {op} rhs.0 }}\n}}\n\n",
                    trait_name = trait_name,
                    base = base_rust,
                    lc = trait_name.to_lowercase(),
                    op = op
                ));
            } else {
                out.push_str(&format!(
                    "impl std::ops::{trait_name}<{rust_name}> for {rust_name} {{\n    type Output = {rust_name};\n    fn {lc}(self, rhs: {rust_name}) -> {rust_name} {{ {rust_name}(self.0 {op} rhs.0) }}\n}}\n\n",
                    trait_name = trait_name,
                    lc = trait_name.to_lowercase(),
                    op = op
                ));
            }
        }
    }
    if let Some(label) = cx.unit_labels.get(&d.name) {
        if !cx.display_types.contains(&d.name) {
            out.push_str(&format!(
                "impl JetDisplay for {rust_name} {{\n    fn jet_display(&self) -> String {{ format!(\"{{}} {symbol}\", (self.0).to_string()) }}\n}}\n\n",
                symbol = label.symbol,
            ));
        }
    }
    // D-CAPBUNDLE1 `#Printable`: forward `{value}` interpolation (JetDisplay)
    // to the base value's own rendering — a distinct type starts inert, so
    // without this marker sema never lets a value reach here (E0138).
    if distinct_has_derive(d, crate::Generics::PRINTABLE) {
        out.push_str(&format!(
            "impl JetDisplay for {rust_name} {{\n    fn jet_display(&self) -> String {{ (self.0).jet_display() }}\n}}\n\n"
        ));
    }
    // D-CAPBUNDLE1 `#CodableAsBase`: encode/decode via the base type's own
    // wire representation (`__jet_Encode`/`__jet_Decode`, the same traits
    // struct/enum `#[Codable]` derives target — I8: one wire mechanism).
    if distinct_has_derive(d, crate::Generics::ENCODE)
        && distinct_has_derive(d, crate::Generics::DECODE)
        && !cx.used_core.is_empty()
    {
        out.push_str(&format!(
            "impl crate::__jet_Encode for {rust_name} {{\n    fn jet_encode(&self) -> crate::jet_std::DataTree {{ crate::__jet_Encode::jet_encode(&self.0) }}\n}}\n\n"
        ));
        if let Some((lo, hi, _)) = d.range {
            out.push_str(&format!(
                "impl crate::__jet_Decode for {rust_name} {{\n    fn jet_decode(__t: &crate::jet_std::DataTree) -> Result<Self, Vec<crate::jet_std::FieldError>> {{\n        let __value = <{base} as crate::__jet_Decode>::jet_decode(__t)?;\n        if __value < ({lo} as {base}) || __value > ({hi} as {base}) {{\n            return Err(crate::jet_std::FieldError::one(\"expected {name} within {lo}..{hi}\"));\n        }}\n        Ok({rust_name}(__value))\n    }}\n}}\n\n",
                name = d.name,
                base = base_rust,
                lo = lo,
                hi = hi,
            ));
        } else {
            out.push_str(&format!(
                "impl crate::__jet_Decode for {rust_name} {{\n    fn jet_decode(__t: &crate::jet_std::DataTree) -> Result<Self, Vec<crate::jet_std::FieldError>> {{ Ok({rust_name}(<{base} as crate::__jet_Decode>::jet_decode(__t)?)) }}\n}}\n\n",
                base = base_rust
            ));
        }
    }
}

pub(crate) fn emit_const(c: &crate::AST::ConstDef, out: &mut String) {
    // S57 (M9.5): plain comptime values are inlined at use sites (registered into
    // `cx.consts`), so there is no top-level item to emit.
    // D-CONSTMARK1: `#Static comptime` still emits a Rust static when ForceStatic.
    let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
    if (c.is_comptime && !force_static)
        || matches!(&c.ty, Some(Type::Named(name)) if name == crate::Syntax::TYPE_OUTPUT)
    {
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
    // D-FFI-INLINE1: the executable definition lives in the hidden bridge;
    // calls are already routed through `extern_funcs`.
    if f.inline_foreign.is_some() {
        return;
    }
    // c148: expose the current function's type-parameter names so `rust_type` and
    // `rust_param_type` can recognize multi-char params (e.g. `Kind`) in addition
    // to the single-letter heuristic. Cleared on exit (normal or panic).
    *cx.current_type_params.borrow_mut() = f.type_params.iter().map(|p| p.name.clone()).collect();
    // c109 Phase N: the typed IR (TIR) is the only codegen seam (R7). Every
    // reachable function lowers + emits through the TIR; a gate-miss is an
    // internal compiler error (I2-class), never an AST fallback.
    if TIR::tir_covers(f, cx) {
        let tir = TIR::lower_func(f, cx);
        TIR::emit_tir_func(&tir, cx, out);
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

/// D-ERR-CONV: emit a standalone Rust function for `impl Source => Target { body }`.
/// The function is called by the `map_err` closure emitted in `Expression.rs`
/// when a `TryConvert::Typed` node is encountered.
pub(crate) fn emit_error_conv(cx: &Cx, ec: &crate::AST::ErrorConvDef, out: &mut String) {
    let fn_name = crate::Sema::error_conv_fn_name(&ec.from_ty, &ec.to_ty);
    let from_rust = cx.rust_type(&crate::AST::Type::Named(ec.from_ty.clone()));
    let to_rust = cx.rust_type(&crate::AST::Type::Named(ec.to_ty.clone()));
    *cx.current_fn.borrow_mut() = fn_name.clone();
    out.push_str(&jet_format!(
        "pub fn {fn_name}({jet_prefix}self: {from}) -> {to} {{\n",
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

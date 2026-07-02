use super::*;
use crate::Generics;
use crate::AST::{
    ConstAttr, DistinctDef, EnumDef, Expr, Field, Func, ImplDef, Marker, RustConstKind, StrPart,
    StructDef, TraitImplBlock, Type, Variant, VariantPayload,
};
use std::collections::HashMap;

fn type_mentions_gc(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, .. } if name == Syntax::GC_TYPE => true,
        Type::Option(inner) => type_mentions_gc(inner),
        _ => false,
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
        "impl jet_gc::GcTrace for {name} {{\n    fn trace(&self, out: &mut Vec<usize>) {{\n{trace_body}    }}\n}}\n\n",
        name = user_type_rust(&s.name),
        trace_body = trace_body
    ));
}

fn struct_lifetimes(fields: &[Field]) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for f in fields {
        if !f.is_stored_ref {
            continue;
        }
        let label = f
            .stored_ref_label
            .clone()
            .unwrap_or_else(|| "src".to_string());
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

pub(crate) fn emit_struct(cx: &Cx, s: &StructDef, out: &mut String) {
    let lifetimes = struct_lifetimes(&s.fields);
    let clone_extra = if !s.type_params.is_empty() && cx.cloneable.contains(&s.name) {
        Generics::rust_extra_clone_bounds(&s.type_params)
    } else {
        HashMap::new()
    };
    let gen = if s.type_params.is_empty() {
        String::new()
    } else {
        Generics::rust_type_param_list(&s.type_params, &clone_extra)
    };
    let lt_params = if lifetimes.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            lifetimes
                .iter()
                .map(|l| format!("'{}", l))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let type_params = if gen.is_empty() {
        lt_params.clone()
    } else if lt_params.is_empty() {
        gen
    } else {
        format!(
            "<{}, {}>",
            &gen[1..gen.len() - 1],
            &lt_params[1..lt_params.len() - 1]
        )
    };
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
    for f in &s.fields {
        let field_ty = if f.is_stored_ref {
            let label = f
                .stored_ref_label
                .clone()
                .unwrap_or_else(|| "src".to_string());
            format!("&'{} {}", label, cx.rust_type(&f.ty))
        } else {
            cx.struct_field_rust(s, &f.name, &f.ty)
        };
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
        let tp_plain = Generics::type_param_rust_list(&s.type_params);
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
                .map(|f| format!("((self).{}).jet_show()", mangle(&f.name)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("format!(\"{}({})\", {})", s.name, fmt_fields, show_fields)
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
        let debug_tp_bounds = Generics::rust_type_param_list(&s.type_params, &debug_impl_bounds);
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
    } else if lifetimes.is_empty() {
        let show_body = if has_fn_field {
            format!("\"{} {{ ... }}\".to_string()", s.name)
        } else {
            "format!(\"{:?}\", self)".to_string()
        };
        out.push_str(&format!(
            "impl JetShow for {} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
            user_type_rust(&s.name),
            show_body
        ));
        let debug_body = struct_jet_debug_body(s, has_fn_field);
        out.push_str(&format!(
            "impl JetDebug for {} {{\n    fn jet_debug(&self) -> String {{ {} }}\n}}\n\n",
            user_type_rust(&s.name),
            debug_body
        ));
        if !cx.display_types.contains(&s.name) {
            out.push_str(&format!(
                "impl JetDisplay for {} {{\n    fn jet_display(&self) -> String {{ self.jet_show() }}\n}}\n\n",
                user_type_rust(&s.name),
            ));
        }
    } else {
        let lt = lifetimes
            .iter()
            .map(|l| format!("'{}", l))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "impl<{}> JetShow for {}<{}> {{\n    fn jet_show(&self) -> String {{ format!(\"{{:?}}\", self) }}\n}}\n\n",
            lt,
            user_type_rust(&s.name),
            lt
        ));
    }
    emit_struct_serde(cx, s, out);
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
    // Stored-ref fields cannot be columnar (a column is owned storage); sema's
    // whole-struct rule plus the value-field assumption holds for the v1 surface.
    let fields: Vec<&Field> = s.fields.iter().filter(|f| !f.is_stored_ref).collect();
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
    let enc = s.derives.iter().any(|(t, _)| t == Generics::ENCODE);
    let dec = s.derives.iter().any(|(t, _)| t == Generics::DECODE);
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

pub(crate) fn emit_enum(cx: &Cx, e: &EnumDef, out: &mut String) {
    let mut derives = vec!["Debug"];
    if cx.cloneable.contains(&e.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&e.name) {
        derives.push("PartialEq");
    }
    out.push_str(&format!(
        "#[derive({})]\npub enum user_{} {{\n",
        derives.join(", "),
        e.name
    ));
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {
                out.push_str(&format!("    {},\n", mangle(&v.name)));
            }
            VariantPayload::Single(t, _) => {
                let ty = cx.field_rust_type(&e.name, &v.name, t);
                out.push_str(&format!("    {}({}),\n", mangle(&v.name), ty));
            }
            VariantPayload::Named(fs) => {
                out.push_str(&format!("    {} {{\n", mangle(&v.name)));
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    let ty = cx.field_rust_type(&e.name, &key, &f.ty);
                    out.push_str(&format!("        {}: {},\n", mangle(&f.name), ty));
                }
                out.push_str("    },\n");
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
    emit_enum_serde(cx, e, out);
}

// ──────────────────────────────────────────────────────────────────────────────
// D-SERDE: built-in `Encode`/`Decode` derive codegen.
//
// The `@[Codable]`/`@[Encode]`/`@[Decode]` markers lower here to compiler-owned
// `impl user_Encode`/`impl user_Decode` blocks that walk the type's fields/variants
// over the `jet_std::DataTree` model. Plain std Rust — no proc-macros, no `unsafe`
// (I1/I6). Field/container attributes (D-SERDE3/5/7/8) are applied during the walk.
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
fn lit_rust(e: &Expr) -> String {
    match e {
        Expr::Int(n, _, _) => format!("{}i64", n),
        Expr::Float(f, _, _) => format!("{:?}f64", f),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(s) => format!("{:?}.to_string()", s),
            _ => "Default::default()".to_string(),
        },
        _ => "Default::default()".to_string(),
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
fn field_default_rust(f: &Field) -> Option<String> {
    let m = serde_marker(&f.serde_markers, crate::Syntax::ATTR_DEFAULT)?;
    Some(match m.args.first() {
        Some(arg) => lit_rust(arg),
        None => "Default::default()".to_string(),
    })
}

fn emit_struct_serde(cx: &Cx, s: &StructDef, out: &mut String) {
    let enc = s.derives.iter().any(|(t, _)| t == Generics::ENCODE);
    let dec = s.derives.iter().any(|(t, _)| t == Generics::DECODE);
    if !enc && !dec {
        return;
    }
    let style = container_rename_all(&s.serde_markers);
    let style = style.as_deref();
    let tp_plain = Generics::type_param_rust_list(&s.type_params);
    // D-SERDE9/10: the wire-reaching type params (those a non-skipped field type
    // mentions) carry the injected serde bound; phantom/skip-only params don't.
    let wire_types: Vec<&Type> = s
        .fields
        .iter()
        .filter(|f| !serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP))
        .map(|f| &f.ty)
        .collect();
    let clone_all = cx.cloneable.contains(&s.name);
    let enc_header = serde_impl_header(&s.type_params, &wire_types, Generics::ENCODE, clone_all);
    let dec_header = serde_impl_header(&s.type_params, &wire_types, Generics::DECODE, clone_all);

    if enc {
        out.push_str(&format!(
            "impl{enc_header} user_Encode for user_{}{tp_plain} {{\n    fn jet_encode(&self) -> jet_std::DataTree {{\n        let mut __o: Vec<(String, jet_std::DataTree)> = Vec::new();\n",
            s.name
        ));
        for f in &s.fields {
            if serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP) {
                continue;
            }
            let m = mangle(&f.name);
            if serde_has(&f.serde_markers, crate::Syntax::ATTR_FLATTEN) {
                out.push_str(&format!(
                    "        if let jet_std::DataTree::Object(mut __es) = (self.{m}).jet_encode() {{ __o.append(&mut __es); }}\n"
                ));
                continue;
            }
            let key = field_wire_key(style, f);
            if matches!(f.ty, Type::Option(_)) {
                // D-SERDE5 owner-Q: an absent optional is omitted from the wire.
                out.push_str(&format!(
                    "        if let Some(__v) = &self.{m} {{ __o.push(({key:?}.to_string(), __v.jet_encode())); }}\n"
                ));
            } else {
                out.push_str(&format!(
                    "        __o.push(({key:?}.to_string(), (self.{m}).jet_encode()));\n"
                ));
            }
        }
        out.push_str("        jet_std::DataTree::Object(__o)\n    }\n}\n\n");
    }

    if dec {
        let deny = serde_has(&s.serde_markers, crate::Syntax::ATTR_DENY_UNKNOWN_FIELDS);
        let has_flatten = s
            .fields
            .iter()
            .any(|f| serde_has(&f.serde_markers, crate::Syntax::ATTR_FLATTEN));
        out.push_str(&format!(
            "impl{dec_header} user_Decode for user_{}{tp_plain} {{\n    fn jet_decode(__t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {{\n",
            s.name
        ));
        // D-SERDE8: `#[DenyUnknownFields]` errors on a wire key the struct doesn't
        // declare (E2412). Skipped when a `#[Flatten]` field absorbs extra keys.
        if deny && !has_flatten {
            let keys: Vec<String> = s
                .fields
                .iter()
                .filter(|f| !serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP))
                .map(|f| format!("{:?}", field_wire_key(style, f)))
                .collect();
            out.push_str(&format!(
                "        if let jet_std::DataTree::Object(__es) = __t {{ for (__k, _) in __es {{ if ![{}].contains(&__k.as_str()) {{ return Err(jet_std::DecodeError::new(format!(\"E2412: unknown field `{{}}`\", __k))); }} }} }}\n",
                keys.join(", ")
            ));
        }
        for f in &s.fields {
            let m = mangle(&f.name);
            let rust = cx.rust_type(&f.ty);
            if serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP) {
                let d = field_default_rust(f).unwrap_or_else(|| "Default::default()".to_string());
                out.push_str(&format!("        let {m}: {rust} = {d};\n"));
                continue;
            }
            if serde_has(&f.serde_markers, crate::Syntax::ATTR_FLATTEN) {
                out.push_str(&format!(
                    "        let {m}: {rust} = <{rust} as user_Decode>::jet_decode(__t).map_err(|__e| jet_std::DecodeError::under({:?}, __e))?;\n",
                    f.name
                ));
                continue;
            }
            let key = field_wire_key(style, f);
            let absent = if matches!(f.ty, Type::Option(_)) {
                "None".to_string()
            } else if let Some(d) = field_default_rust(f) {
                d
            } else {
                // E2410: a required field is missing on the wire.
                format!(
                    "return Err(jet_std::DecodeError::new(\"E2410: missing required field `{}`\".to_string()))",
                    f.name
                )
            };
            out.push_str(&format!(
                "        let {m}: {rust} = match jet_std::datatree_get(__t, {key:?}) {{ Some(__v) => <{rust} as user_Decode>::jet_decode(__v).map_err(|__e| jet_std::DecodeError::under({key:?}, __e))?, None => {absent} }};\n"
            ));
        }
        let inits: Vec<String> = s.fields.iter().map(|f| mangle(&f.name)).collect();
        out.push_str(&format!(
            "        Ok(user_{} {{ {} }})\n    }}\n}}\n\n",
            s.name,
            inits.join(", ")
        ));
    }
}

/// D-SERDE9/10: the `impl<…>` generics clause for a generic serde impl, injecting
/// the `Encode`/`Decode` bound on the wire-reaching params. Empty for a concrete
/// type. `bound` is `Encode` or `Decode`. `clone_all` adds the structural `Clone`
/// bound (on every param) that the generated type carries when it derives Clone —
/// the serde impls must satisfy the same bound the struct/enum header declares.
fn serde_impl_header(
    params: &[crate::AST::TypeParam],
    wire_types: &[&Type],
    bound: &str,
    clone_all: bool,
) -> String {
    if params.is_empty() {
        return String::new();
    }
    let mut extra = Generics::rust_extra_serde_bounds(params, wire_types, bound);
    if clone_all {
        for (k, v) in Generics::rust_extra_clone_bounds(params) {
            extra.entry(k).or_default().extend(v);
        }
    }
    Generics::rust_type_param_list(params, &extra)
}

fn variant_wire_name(v: &Variant) -> String {
    serde_marker(&v.serde_markers, crate::Syntax::ATTR_RENAME)
        .and_then(marker_str_arg)
        .unwrap_or_else(|| v.name.clone())
}

fn emit_enum_serde(cx: &Cx, e: &EnumDef, out: &mut String) {
    let enc = e.derives.iter().any(|(t, _)| t == Generics::ENCODE);
    let dec = e.derives.iter().any(|(t, _)| t == Generics::DECODE);
    if !enc && !dec {
        return;
    }
    // D-SERDE7: externally tagged by default; `#[Tag("k")]` selects internal tagging,
    // `#[Untagged]` selects untagged. Internal/untagged are validated in sema.
    let tag = serde_marker(&e.serde_markers, crate::Syntax::ATTR_TAG).and_then(marker_str_arg);
    let untagged = serde_has(&e.serde_markers, crate::Syntax::ATTR_UNTAGGED);
    let tp_plain = Generics::type_param_rust_list(&e.type_params);
    // D-SERDE9/10: every variant payload type reaches the wire (enums have no
    // field-level `#[Skip]`), so they all contribute wire-reaching params.
    let wire_types: Vec<&Type> = e
        .variants
        .iter()
        .flat_map(|v| match &v.payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(t, _) => vec![t],
            VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
        })
        .collect();
    let clone_all = cx.cloneable.contains(&e.name);
    let enc_header = serde_impl_header(&e.type_params, &wire_types, Generics::ENCODE, clone_all);
    let dec_header = serde_impl_header(&e.type_params, &wire_types, Generics::DECODE, clone_all);

    if enc {
        out.push_str(&format!(
            "impl{enc_header} user_Encode for user_{}{tp_plain} {{\n    fn jet_encode(&self) -> jet_std::DataTree {{\n        match self {{\n",
            e.name
        ));
        for v in &e.variants {
            let vm = mangle(&v.name);
            let wire = variant_wire_name(v);
            let body = encode_variant_body(cx, &v.payload, &wire, tag.as_deref(), untagged);
            match &v.payload {
                VariantPayload::Unit => {
                    out.push_str(&format!(
                        "            user_{}::{} => {},\n",
                        e.name, vm, body.0
                    ));
                }
                VariantPayload::Single(..) => {
                    out.push_str(&format!(
                        "            user_{}::{}(__0) => {},\n",
                        e.name, vm, body.0
                    ));
                }
                VariantPayload::Named(fs) => {
                    let binds: Vec<String> = fs.iter().map(|f| mangle(&f.name)).collect();
                    out.push_str(&format!(
                        "            user_{}::{} {{ {} }} => {},\n",
                        e.name,
                        vm,
                        binds.join(", "),
                        body.0
                    ));
                }
            }
        }
        out.push_str("        }\n    }\n}\n\n");
    }

    if dec {
        emit_enum_decode(cx, e, tag.as_deref(), untagged, &dec_header, &tp_plain, out);
    }
}

// Returns the encode expression for one variant arm. Field bindings (`__0` for a
// single payload, mangled names for a named payload) are already in scope.
fn encode_variant_body(
    cx: &Cx,
    payload: &VariantPayload,
    wire: &str,
    tag: Option<&str>,
    untagged: bool,
) -> (String, ()) {
    let _ = cx;
    let expr = match (payload, tag, untagged) {
        // ── Untagged: just the payload, no tag wrapper. ──
        (VariantPayload::Unit, _, true) => "jet_std::DataTree::Null".to_string(),
        (VariantPayload::Single(..), _, true) => "__0.jet_encode()".to_string(),
        (VariantPayload::Named(fs), _, true) => {
            format!("jet_std::DataTree::Object(vec![{}])", named_pairs(fs))
        }
        // ── Internally tagged: tag key + inlined fields (unit/named). ──
        (VariantPayload::Unit, Some(k), false) => format!(
            "jet_std::DataTree::Object(vec![({k:?}.to_string(), jet_std::DataTree::Text({wire:?}.to_string()))])"
        ),
        (VariantPayload::Named(fs), Some(k), false) => {
            let mut pairs = format!(
                "({k:?}.to_string(), jet_std::DataTree::Text({wire:?}.to_string()))"
            );
            let np = named_pairs(fs);
            if !np.is_empty() {
                pairs.push_str(", ");
                pairs.push_str(&np);
            }
            format!("jet_std::DataTree::Object(vec![{pairs}])")
        }
        // A single (tuple) payload can't be internally tagged; sema rejects it, so
        // fall back to the external shape here for safety.
        (VariantPayload::Single(..), Some(_), false) => format!(
            "jet_std::DataTree::Object(vec![({wire:?}.to_string(), __0.jet_encode())])"
        ),
        // ── Externally tagged (default). ──
        (VariantPayload::Unit, None, false) => {
            format!("jet_std::DataTree::Text({wire:?}.to_string())")
        }
        (VariantPayload::Single(..), None, false) => format!(
            "jet_std::DataTree::Object(vec![({wire:?}.to_string(), __0.jet_encode())])"
        ),
        (VariantPayload::Named(fs), None, false) => format!(
            "jet_std::DataTree::Object(vec![({wire:?}.to_string(), jet_std::DataTree::Object(vec![{}]))])",
            named_pairs(fs)
        ),
    };
    (expr, ())
}

fn named_pairs(fs: &[crate::AST::VariantField]) -> String {
    fs.iter()
        .map(|f| {
            let m = mangle(&f.name);
            format!("({:?}.to_string(), {m}.jet_encode())", f.name)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_enum_decode(
    cx: &Cx,
    e: &EnumDef,
    tag: Option<&str>,
    untagged: bool,
    dec_header: &str,
    tp_plain: &str,
    out: &mut String,
) {
    out.push_str(&format!(
        "impl{dec_header} user_Decode for user_{}{tp_plain} {{\n    fn jet_decode(__t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {{\n",
        e.name
    ));
    if untagged {
        // Untagged: try each variant's shape in declaration order; first success wins.
        for v in &e.variants {
            let cons = decode_variant_from(cx, &e.name, v, "__t");
            out.push_str(&format!(
                "        if let Ok(__r) = (|| -> Result<Self, jet_std::DecodeError> {{ Ok({}) }})() {{ return Ok(__r); }}\n",
                cons
            ));
        }
        out.push_str(
            "        Err(jet_std::DecodeError::new(\"no untagged variant matched\".to_string()))\n    }\n}\n\n",
        );
        return;
    }
    if let Some(k) = tag {
        // Internally tagged: read the tag, then build from the sibling fields.
        out.push_str(&format!(
            "        let __tag = match jet_std::datatree_get(__t, {k:?}) {{ Some(jet_std::DataTree::Text(__s)) => __s.clone(), _ => return Err(jet_std::DecodeError::new(\"missing tag `{k}`\".to_string())) }};\n        match __tag.as_str() {{\n"
        ));
        for v in &e.variants {
            let wire = variant_wire_name(v);
            let cons = decode_variant_from(cx, &e.name, v, "__t");
            out.push_str(&format!("            {wire:?} => Ok({cons}),\n"));
        }
        out.push_str(&format!(
            "            __other => Err(jet_std::DecodeError::new(format!(\"unknown variant `{{}}`\", __other))),\n        }}\n    }}\n}}\n\n"
        ));
        return;
    }
    // Externally tagged (default): a unit variant is a bare string; a payload variant
    // is a single-key object `{{\"Variant\": payload}}`.
    let has_unit = e
        .variants
        .iter()
        .any(|v| matches!(v.payload, VariantPayload::Unit));
    if has_unit {
        out.push_str("        if let jet_std::DataTree::Text(__s) = __t {\n            match __s.as_str() {\n");
        for v in &e.variants {
            if matches!(v.payload, VariantPayload::Unit) {
                let wire = variant_wire_name(v);
                out.push_str(&format!(
                    "                {wire:?} => return Ok(user_{}::{}),\n",
                    e.name,
                    mangle(&v.name)
                ));
            }
        }
        out.push_str("                _ => {}\n            }\n        }\n");
    }
    out.push_str("        if let jet_std::DataTree::Object(__es) = __t {\n            if __es.len() == 1 {\n                let (__k, __v) = &__es[0];\n                match __k.as_str() {\n");
    for v in &e.variants {
        if matches!(v.payload, VariantPayload::Unit) {
            continue;
        }
        let wire = variant_wire_name(v);
        let cons = decode_variant_from(cx, &e.name, v, "__v");
        out.push_str(&format!(
            "                    {wire:?} => return Ok({cons}),\n"
        ));
    }
    out.push_str("                    _ => {}\n                }\n            }\n        }\n");
    out.push_str(&format!(
        "        Err(jet_std::DecodeError::new(\"no matching variant for `{}`\".to_string()))\n    }}\n}}\n\n",
        e.name
    ));
}

// Build a variant constructor that decodes its payload from the DataTree expr `src`.
// For internal tagging, named fields read from the same object as the tag; for the
// external/untagged shapes, `src` is the payload sub-tree.
fn decode_variant_from(cx: &Cx, enum_name: &str, v: &Variant, src: &str) -> String {
    let vm = mangle(&v.name);
    match &v.payload {
        VariantPayload::Unit => format!("user_{}::{}", enum_name, vm),
        VariantPayload::Single(t, _) => {
            let rust = cx.rust_type(t);
            format!(
                "user_{}::{}(<{rust} as user_Decode>::jet_decode({src}).map_err(|__e| jet_std::DecodeError::under({:?}, __e))?)",
                enum_name, vm, v.name
            )
        }
        VariantPayload::Named(fs) => {
            let parts: Vec<String> = fs
                .iter()
                .map(|f| {
                    let m = mangle(&f.name);
                    let rust = cx.rust_type(&f.ty);
                    let absent = if matches!(f.ty, Type::Option(_)) {
                        "None".to_string()
                    } else {
                        format!(
                            "return Err(jet_std::DecodeError::new(\"E2410: missing required field `{}`\".to_string()))",
                            f.name
                        )
                    };
                    format!(
                        "{m}: match jet_std::datatree_get({src}, {:?}) {{ Some(__fv) => <{rust} as user_Decode>::jet_decode(__fv).map_err(|__e| jet_std::DecodeError::under({:?}, __e))?, None => {absent} }}",
                        f.name, f.name
                    )
                })
                .collect();
            format!("user_{}::{} {{ {} }}", enum_name, vm, parts.join(", "))
        }
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
    let tp = Generics::type_param_rust_list(type_params);
    out.push_str(&format!(
        "impl{} {}{} {{\n",
        tp,
        user_type_rust(type_name),
        tp
    ));
    for m in methods {
        emit_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
}

pub(crate) fn emit_trait_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::AST::TypeParam],
    block: &TraitImplBlock,
    out: &mut String,
) {
    let tp = Generics::type_param_rust_list(type_params);
    out.push_str(&format!(
        "impl{} {} for {}{} {{\n",
        tp,
        Generics::user_trait_rust(&block.trait_name),
        user_type_rust(type_name),
        tp
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
        emit_trait_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
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
                    "format!(\"{}: {{}}\", ((self).{}).jet_debug())",
                    f.name,
                    mangle(&f.name)
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

pub(crate) fn emit_external_trait_impl(cx: &Cx, i: &ImplDef, out: &mut String) {
    let trait_name = i.trait_name.as_deref().unwrap_or("");
    out.push_str(&format!(
        "impl {} for {} {{\n",
        Generics::user_trait_rust(trait_name),
        user_type_rust(&i.type_name)
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
            panic!(
                "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
                m.name
            );
        }
    } else {
        for m in &i.methods {
            emit_trait_method(cx, &i.type_name, m, out, 1);
        }
    }
    out.push_str("}\n\n");
    if trait_name == crate::Syntax::TRAIT_DISPLAY {
        out.push_str(&format!(
            "impl JetDisplay for {} {{\n    fn jet_display(&self) -> String {{ <{} as {}>::display(self) }}\n}}\n\n",
            user_type_rust(&i.type_name),
            user_type_rust(&i.type_name),
            Generics::user_trait_rust(crate::Syntax::TRAIT_DISPLAY),
        ));
    }
}

fn emit_trait_method(cx: &Cx, type_name: &str, f: &Func, out: &mut String, indent: usize) {
    // c109 Phase N: the typed IR is the only codegen seam (R7). A trait-impl
    // method always emits at indent 1 inside the `impl Trait for user_<T>` block
    // the caller opened; it lowers + emits through the TIR. A gate-miss is an
    // internal compiler error (I2-class), never an AST fallback.
    debug_assert_eq!(
        indent, 1,
        "trait methods always emit at impl-block indent 1"
    );
    if TIR::tir_covers_trait_method(f, type_name, cx) {
        let tir = TIR::lower_trait_method(f, type_name, cx);
        TIR::emit_tir_func(&tir, cx, out);
        return;
    }
    panic!(
        "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
        f.name
    );
}

fn emit_method(cx: &Cx, type_name: &str, f: &Func, out: &mut String, indent: usize) {
    // c109 Phase N: the typed IR is the only codegen seam (R7). An inherent
    // method always emits at indent 1 inside the `impl` block the caller opened;
    // it lowers + emits through the TIR. A gate-miss is an internal compiler
    // error (I2-class), never an AST fallback.
    debug_assert_eq!(
        indent, 1,
        "inherent methods always emit at impl-block indent 1"
    );
    if TIR::tir_covers_method(f, type_name, cx) {
        let tir = TIR::lower_method(f, type_name, cx);
        TIR::emit_tir_func(&tir, cx, out);
        return;
    }
    panic!(
        "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
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
    // .raw() method: unwrap to the base type.
    out.push_str(&format!(
        "impl user_{} {{\n    pub fn raw(&self) -> {} {{ self.0 }}\n}}\n\n",
        d.name, base_rust
    ));
    // @Numeric: implement Add, Sub, Mul, Div (same-type arithmetic).
    if d.is_numeric {
        for (trait_name, op) in &[("Add", "+"), ("Sub", "-"), ("Mul", "*"), ("Div", "/")] {
            out.push_str(&format!(
                "impl std::ops::{}<user_{n}> for user_{n} {{\n    type Output = user_{n};\n    fn {lc}(self, rhs: user_{n}) -> user_{n} {{ user_{n}(self.0 {op} rhs.0) }}\n}}\n\n",
                trait_name,
                n = d.name,
                lc = trait_name.to_lowercase(),
                op = op
            ));
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
        Expr::Int(n, _, Some((signed, bits))) => {
            let rust = format!("{}{}", if *signed { 'i' } else { 'u' }, bits);
            (format!("{n}{rust}"), rust)
        }
        Expr::Int(n, _, None) => (format!("{}i64", n), "i64".to_string()),
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
    panic!(
        "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
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
            "    if !({cond}) {{ jet_contract_fail({file}, {line}, \"Pre\", &{msg}); }}\n",
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
        let ret_annot = TIR::rust_return_type(cx, &ret_ty, f.is_view_return);
        out.push_str(&format!("    let __jet_result = (|| -> {ret_annot} {{\n"));
        out.push_str(body_only);
        out.push_str("    })();\n");
        for clause in &f.post {
            let cond =
                TIR::render_contract_cond(f, &clause.cond, Some(("__jet_result", &ret_ty)), cx);
            let (_, line, _) = TIR::tir_src_line_at(&cx.src, clause.span.start);
            out.push_str(&format!(
                "    if !({cond}) {{ jet_contract_fail({file}, {line}, \"Post\", &{msg}); }}\n",
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
    panic!(
        "internal compiler error: codegen reached an error-conversion body construct the typed IR does not cover ({} -> {}) — compiler bug (I2/R7)",
        ec.from_ty, ec.to_ty
    );
}

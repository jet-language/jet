use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
enum TypeExpr {
    Named(String),
    Option(Box<TypeExpr>),
    Sequence(Box<TypeExpr>),
    Set(Box<TypeExpr>, bool),
    Map(Box<TypeExpr>, Box<TypeExpr>, bool),
    Boxed(Box<TypeExpr>),
    Arc(Box<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    Array(Box<TypeExpr>, usize),
    StaticStr,
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    ty: TypeExpr,
    public: bool,
}

#[derive(Clone, Debug)]
enum VariantBody {
    Unit,
    Tuple(Vec<TypeExpr>),
    Struct(Vec<Field>),
    Unsupported(String),
}

#[derive(Clone, Debug)]
struct Variant {
    name: String,
    body: VariantBody,
}

#[derive(Clone, Debug)]
enum Body {
    Struct(Vec<Field>),
    Tuple(Vec<Field>),
    Unit,
    Alias(TypeExpr),
    Enum(Vec<Variant>),
}

#[derive(Clone, Debug)]
struct Definition {
    key: String,
    path: String,
    module: String,
    name: String,
    imports: BTreeMap<String, String>,
    body: Body,
    generic: bool,
}

#[derive(Default)]
struct Schema {
    definitions: BTreeMap<String, Definition>,
    aliases: BTreeMap<String, String>,
    wildcards: Vec<(String, String)>,
}

pub fn generate(manifest_dir: &Path, out_dir: &Path) {
    let source_root = manifest_dir.join("src");
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    files.sort();
    let mut schema = Schema::default();
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read MIR image schema {}: {error}", path.display()));
        let relative = path
            .strip_prefix(&source_root)
            .expect("collected schema path is under source root");
        let module = module_path(relative);
        let masked = mask_comments_and_literals(&source);
        let imports = parse_imports(&masked, &module);
        parse_public_uses(&masked, &module, &mut schema);
        let mut definitions = parse_definitions(&masked, &module, &imports)
            .unwrap_or_else(|error| panic!("invalid native MIR image schema {}: {error}", path.display()));
        schema.definitions.extend(definitions.drain(..).map(|definition| {
            (definition.key.clone(), definition)
        }));
        if relative == Path::new("MIR.rs") {
            for name in parse_mir_id_macro_invocations(&masked) {
                let key = format!("{module}::{name}");
                schema.definitions.insert(
                    key.clone(),
                    Definition {
                        key,
                        path: format!("{module}::{name}"),
                        module: module.clone(),
                        name,
                        imports: imports.clone(),
                        body: Body::Tuple(vec![Field {
                            name: "0".to_string(),
                            ty: TypeExpr::Named("u64".to_string()),
                            public: true,
                        }]),
                        generic: false,
                    },
                );
            }
        }
    }
    for (public_module, target_module) in &schema.wildcards.clone() {
        let targets = schema
            .definitions
            .values()
            .filter(|definition| definition.module == *target_module)
            .map(|definition| (format!("{public_module}::{}", definition.name), definition.key.clone()))
            .collect::<Vec<_>>();
        schema.aliases.extend(targets);
    }
    assign_public_paths(&mut schema);
    let reachable = reachable_schema(&schema, "crate::MIR::MirProgram")
        .unwrap_or_else(|error| panic!("native MIR image schema is incomplete: {error}"));
    let output = emit_codec(&schema, &reachable, INTERNED_DEFINITIONS)
        .unwrap_or_else(|error| panic!("cannot generate native MIR image codec: {error}"));
    fs::write(out_dir.join("mir_program_image_codec.rs"), output)
        .expect("write generated native MIR image codec");
}

fn collect_rust_files(root: &Path, output: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read Foundation source directory") {
        let entry = entry.expect("read Foundation source entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

fn module_path(relative: &Path) -> String {
    let mut components = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if components.last().is_some_and(|last| last == "mod.rs" || last == "lib.rs") {
        components.pop();
    } else if let Some(last) = components.last_mut() {
        if let Some(stem) = last.strip_suffix(".rs") {
            *last = stem.to_string();
        }
    }
    if components.is_empty() {
        "crate".to_string()
    } else {
        format!("crate::{}", components.join("::"))
    }
}

fn mask_comments_and_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let start = index;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            blank(&mut masked, start, index);
        } else if bytes[index..].starts_with(b"/*") {
            let start = index;
            index += 2;
            let mut depth = 1usize;
            while index < bytes.len() && depth != 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            blank(&mut masked, start, index);
        } else if let Some(end) = raw_string_end(bytes, index) {
            blank(&mut masked, index, end);
            index = end;
        } else if bytes[index] == b'"' {
            let start = index;
            index = quoted_end(bytes, index, b'"');
            blank(&mut masked, start, index);
        } else if bytes[index] == b'\'' && char_literal_end(bytes, index).is_some() {
            let start = index;
            index = char_literal_end(bytes, index).expect("checked character literal");
            blank(&mut masked, start, index);
        } else {
            index += 1;
        }
    }
    String::from_utf8(masked).expect("masking preserves UTF-8")
}

fn blank(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let mut hashes = 0usize;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes.get(cursor + 1..cursor + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += 1;
    }
    Some(bytes.len())
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
        } else if bytes[cursor] == quote {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    bytes.len()
}

fn char_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'\'') {
        return None;
    }
    let mut cursor = start + 1;
    let first = *bytes.get(cursor)?;
    if first == b'\\' {
        cursor += 1;
        match *bytes.get(cursor)? {
            b'u' => {
                cursor += 1;
                if bytes.get(cursor) != Some(&b'{') {
                    return None;
                }
                cursor += 1;
                let mut digits = 0;
                while let Some(&byte) = bytes.get(cursor) {
                    if byte.is_ascii_hexdigit() {
                        digits += 1;
                    } else if byte != b'_' {
                        break;
                    }
                    cursor += 1;
                }
                if digits == 0 || digits > 6 || bytes.get(cursor) != Some(&b'}') {
                    return None;
                }
                cursor += 1;
            }
            b'x' => {
                cursor += 1;
                if !bytes.get(cursor..cursor + 2)?.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                cursor += 2;
            }
            b'n' | b'r' | b't' | b'0' | b'\\' | b'\'' | b'"' => cursor += 1,
            _ => return None,
        }
    } else {
        let width = match first {
            b'\'' | b'\n' | b'\r' => return None,
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return None,
        };
        std::str::from_utf8(bytes.get(cursor..cursor + width)?).ok()?;
        cursor += width;
    }
    (bytes.get(cursor) == Some(&b'\'')).then_some(cursor + 1)
}

fn parse_imports(source: &str, module: &str) -> BTreeMap<String, String> {
    let mut imports = BTreeMap::new();
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'u' if depth == 0 && token_at(source, cursor, "use") => {
                let start = cursor + 3;
                if let Some(end) = source[start..].find(';') {
                    let statement = source[start..start + end].trim();
                    if !statement.starts_with("=") {
                        insert_use_tree(statement, module, &mut imports);
                    }
                    cursor = start + end + 1;
                    continue;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    imports
}

fn parse_public_uses(source: &str, module: &str, schema: &mut Schema) {
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'p' if depth == 0 && token_at(source, cursor, "pub use") => {
                let start = cursor + "pub use".len();
                if let Some(end) = source[start..].find(';') {
                    let statement = source[start..start + end].trim();
                    insert_public_use_tree(statement, module, schema);
                    cursor = start + end + 1;
                    continue;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
}

fn insert_use_tree(statement: &str, module: &str, imports: &mut BTreeMap<String, String>) {
    if let Some(open) = top_level_byte(statement, b'{') {
        let prefix = statement[..open].trim().trim_end_matches(":");
        let Some(close) = matching_byte(statement.as_bytes(), open, b'{', b'}') else {
            return;
        };
        for item in split_top_level(&statement[open + 1..close], ',') {
            let item = item.trim();
            if item.is_empty() || item == "*" {
                continue;
            }
            let (name, alias) = split_alias(item);
            let path = normalize_module_path(module, &format!("{prefix}::{name}"));
            let local_name = alias.unwrap_or(name.rsplit("::").next().unwrap_or(name));
            imports.insert(local_name.to_string(), path);
        }
    } else {
        let (path, alias) = split_alias(statement.trim());
        let path = normalize_module_path(module, path);
        let name = alias
            .or_else(|| path.rsplit("::").next())
            .unwrap_or(path.as_str());
        imports.insert(name.to_string(), path);
    }
}

fn insert_public_use_tree(statement: &str, module: &str, schema: &mut Schema) {
    if let Some(open) = top_level_byte(statement, b'{') {
        let prefix = statement[..open].trim().trim_end_matches(":");
        let Some(close) = matching_byte(statement.as_bytes(), open, b'{', b'}') else {
            return;
        };
        for item in split_top_level(&statement[open + 1..close], ',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if item == "*" {
                schema
                    .wildcards
                    .push((module.to_string(), normalize_module_path(module, prefix)));
                continue;
            }
            let (name, alias) = split_alias(item);
            let target = normalize_module_path(module, &format!("{prefix}::{name}"));
            let export = format!("{module}::{}", alias.unwrap_or(name.rsplit("::").next().unwrap_or(name)));
            schema.aliases.insert(export, target);
        }
    } else if statement.trim().ends_with("::*") {
        let prefix = statement.trim().trim_end_matches("::*");
        schema
            .wildcards
            .push((module.to_string(), normalize_module_path(module, prefix)));
    } else {
        let (name, alias) = split_alias(statement.trim());
        let target = normalize_module_path(module, name);
        let export = format!(
            "{module}::{}",
            alias.unwrap_or(name.rsplit("::").next().unwrap_or(name))
        );
        schema.aliases.insert(export, target);
    }
}

fn split_alias(item: &str) -> (&str, Option<&str>) {
    let mut parts = item.splitn(2, " as ");
    let name = parts.next().unwrap_or(item).trim();
    let alias = parts.next().map(str::trim);
    (name, alias)
}

fn normalize_module_path(module: &str, path: &str) -> String {
    let path = path.trim();
    if path.starts_with("crate::") || path.starts_with("std::") || path.starts_with("core::") {
        return path.to_string();
    }
    if path == "crate" {
        return path.to_string();
    }
    let mut components = if path.starts_with("self::") {
        let mut components = module.split("::").map(str::to_string).collect::<Vec<_>>();
        components.extend(path[6..].split("::").map(str::to_string));
        components
    } else if path.starts_with("super::") {
        let mut components = module.split("::").map(str::to_string).collect::<Vec<_>>();
        while components.len() > 1 && components.last().is_some_and(|part| part == "super") {
            components.pop();
        }
        components.pop();
        components.extend(path[7..].split("::").map(str::to_string));
        components
    } else {
        let mut components = module.split("::").map(str::to_string).collect::<Vec<_>>();
        components.extend(path.split("::").map(str::to_string));
        components
    };
    if components.first().is_some_and(|part| part != "crate") {
        components.insert(0, "crate".to_string());
    }
    components.join("::")
}

fn parse_definitions(
    source: &str,
    module: &str,
    imports: &BTreeMap<String, String>,
) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();
    let mut cursor = 0usize;
    while let Some((kind, start)) = find_next_definition(source, cursor) {
        let prefix = match kind {
            "struct" => "pub struct",
            "enum" => "pub enum",
            "type" => "pub type",
            _ => unreachable!("unknown native MIR definition kind"),
        };
        let mut position = start + prefix.len();
        skip_space(source, &mut position);
        if source.as_bytes().get(position) == Some(&b'$') {
            cursor = skip_macro_declaration(source, position);
            continue;
        }
        let name = read_identifier(source, &mut position)?;
        skip_space(source, &mut position);
        let generic = source.as_bytes().get(position) == Some(&b'<');
        if generic {
            let end = matching_byte(source.as_bytes(), position, b'<', b'>')
                .ok_or_else(|| format!("unclosed generic parameter list for `{name}`"))?;
            position = end + 1;
            skip_space(source, &mut position);
        }
        let body = if kind == "type" {
            let equals = top_level_byte(&source[position..], b'=')
                .ok_or_else(|| format!("type alias `{name}` has no right-hand side"))?;
            let rhs_start = position + equals + 1;
            let end = top_level_byte(&source[rhs_start..], b';')
                .ok_or_else(|| format!("type alias `{name}` has no terminating semicolon"))?;
            let rhs = source[rhs_start..rhs_start + end].trim();
            position = rhs_start + end + 1;
            Body::Alias(parse_type(rhs)?)
        } else {
            match source.as_bytes().get(position).copied() {
                Some(b'{') => {
                    let end = matching_byte(source.as_bytes(), position, b'{', b'}')
                        .ok_or_else(|| format!("unclosed declaration body for `{name}`"))?;
                    let text = &source[position + 1..end];
                    let body = if kind == "struct" {
                        Body::Struct(parse_fields(text)?)
                    } else {
                        Body::Enum(parse_variants(text)?)
                    };
                    position = end + 1;
                    body
                }
                Some(b'(') if kind == "struct" => {
                    let end = matching_byte(source.as_bytes(), position, b'(', b')')
                        .ok_or_else(|| format!("unclosed tuple struct `{name}`"))?;
                    let fields = split_top_level(&source[position + 1..end], ',')
                        .into_iter()
                        .filter(|field| !field.trim().is_empty())
                        .enumerate()
                        .map(|(index, field)| {
                            let field = field.trim();
                            Ok(Field {
                                name: index.to_string(),
                                public: field.starts_with("pub ") || field.starts_with("pub("),
                                ty: parse_type(strip_visibility(field))?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    position = end + 1;
                    Body::Tuple(fields)
                }
                Some(b';') if kind == "struct" => {
                    position += 1;
                    Body::Unit
                }
                _ => return Err(format!("unsupported body for `{name}`")),
            }
        };
        let key = format!("{module}::{name}");
        definitions.push(Definition {
            key: key.clone(),
            path: key,
            module: module.to_string(),
            name,
            imports: imports.clone(),
            body,
            generic,
        });
        cursor = position;
    }
    Ok(definitions)
}


fn skip_macro_declaration(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut cursor = start;
    let mut depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b'>' if cursor > 0 && bytes[cursor - 1] == b'-' => {}
            b')' | b']' | b'}' | b'>' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return cursor + 1,
            _ => {}
        }
        cursor += 1;
    }
    bytes.len()
}
fn parse_mir_id_macro_invocations(source: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut cursor = 0usize;
    while let Some(index) = source[cursor..].find("mir_id!") {
        let start = cursor + index + "mir_id!".len();
        let Some(open) = source[start..].find('(').map(|offset| start + offset) else {
            break;
        };
        let Some(close) = matching_byte(source.as_bytes(), open, b'(', b')') else {
            break;
        };
        let name = source[open + 1..close].trim();
        if is_identifier(name) {
            output.push(name.to_string());
        }
        cursor = close + 1;
    }
    output
}

fn parse_fields(body: &str) -> Result<Vec<Field>, String> {
    let mut fields = Vec::new();
    for raw in split_top_level(body, ',') {
        let raw = strip_attributes(raw.trim());
        if raw.is_empty() {
            continue;
        }
        let colon = top_level_byte(raw, b':')
            .ok_or_else(|| format!("struct field has no type: `{raw}`"))?;
        let declaration = raw[..colon].trim();
        let name = strip_visibility(declaration)
            .split_whitespace()
            .last()
            .ok_or_else(|| format!("struct field has no name: `{raw}`"))?
            .to_string();
        fields.push(Field {
            name,
            public: declaration.starts_with("pub ") || declaration.starts_with("pub("),
            ty: parse_type(raw[colon + 1..].trim())?,
        });
    }
    Ok(fields)
}

fn variant_suffix_supported(suffix: &str) -> bool {
    let suffix = suffix.trim();
    suffix.is_empty() || suffix.strip_prefix('=').is_some_and(|value| !value.trim().is_empty())
}

fn parse_variants(body: &str) -> Result<Vec<Variant>, String> {
    let mut variants = Vec::new();
    for raw in split_top_level(body, ',') {
        let raw = strip_attributes(raw.trim());
        if raw.is_empty() {
            continue;
        }
        let mut cursor = 0usize;
        let name = read_identifier(raw, &mut cursor)?;
        skip_space(raw, &mut cursor);
        let variant_body = match raw.as_bytes().get(cursor).copied() {
            Some(b'(') => {
                let end = matching_byte(raw.as_bytes(), cursor, b'(', b')')
                    .ok_or_else(|| format!("unclosed tuple variant `{name}`"))?;
                if variant_suffix_supported(&raw[end + 1..]) {
                    VariantBody::Tuple(
                        split_top_level(&raw[cursor + 1..end], ',')
                            .into_iter()
                            .filter(|field| !field.trim().is_empty())
                            .map(|field| parse_type(field.trim()))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                } else {
                    VariantBody::Unsupported(raw[end + 1..].trim().to_string())
                }
            }
            Some(b'{') => {
                let end = matching_byte(raw.as_bytes(), cursor, b'{', b'}')
                    .ok_or_else(|| format!("unclosed struct variant `{name}`"))?;
                if variant_suffix_supported(&raw[end + 1..]) {
                    let mut fields = parse_fields(&raw[cursor + 1..end])?;
                    for field in &mut fields {
                        field.public = true;
                    }
                    VariantBody::Struct(fields)
                } else {
                    VariantBody::Unsupported(raw[end + 1..].trim().to_string())
                }
            }
            None => VariantBody::Unit,
            Some(b'=') if variant_suffix_supported(&raw[cursor..]) => VariantBody::Unit,
            _ => VariantBody::Unsupported(raw[cursor..].trim().to_string()),
        };
        variants.push(Variant { name, body: variant_body });
    }
    Ok(variants)
}

fn strip_attributes(mut input: &str) -> &str {
    loop {
        input = input.trim_start();
        if !input.starts_with("#[") {
            return input;
        }
        let Some(end) = matching_byte(input.as_bytes(), 1, b'[', b']') else {
            return input;
        };
        input = &input[end + 1..];
    }
}

fn strip_visibility(input: &str) -> &str {
    let input = input.trim();
    if let Some(rest) = input.strip_prefix("pub(") {
        return rest
            .find(')')
            .map(|end| rest[end + 1..].trim())
            .unwrap_or(input);
    }
    input.strip_prefix("pub ").unwrap_or(input).trim()
}

fn parse_type(input: &str) -> Result<TypeExpr, String> {
    let input = input.trim();
    if input == "&'static str" || input == "& str" {
        return Ok(TypeExpr::StaticStr);
    }
    if let Some(inner) = generic_inner(input, "Option") {
        return Ok(TypeExpr::Option(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = generic_inner(input, "Vec") {
        return Ok(TypeExpr::Sequence(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = generic_inner(input, "Box") {
        return Ok(TypeExpr::Boxed(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = generic_inner(input, "Arc") {
        return Ok(TypeExpr::Arc(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = generic_inner(input, "BTreeSet") {
        return Ok(TypeExpr::Set(Box::new(parse_type(inner)?), false));
    }
    if let Some(inner) = generic_inner(input, "HashSet") {
        return Ok(TypeExpr::Set(Box::new(parse_type(inner)?), true));
    }
    if let Some(inner) = generic_inner(input, "BTreeMap") {
        return Ok(parse_map(inner, false).unwrap_or_else(|_| TypeExpr::Named(input.to_string())));
    }
    if let Some(inner) = generic_inner(input, "HashMap") {
        return Ok(parse_map(inner, true).unwrap_or_else(|_| TypeExpr::Named(input.to_string())));
    }
    if input.starts_with('(') && input.ends_with(')') {
        let values = split_top_level(&input[1..input.len() - 1], ',')
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| parse_type(value.trim()))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(TypeExpr::Tuple(values));
    }
    if input.starts_with('[') && input.ends_with(']') {
        let parts = split_top_level(&input[1..input.len() - 1], ';');
        if parts.len() == 2 {
            if let Ok(length) = parts[1].trim().parse::<usize>() {
                return Ok(TypeExpr::Array(Box::new(parse_type(parts[0].trim())?), length));
            }
        }
        return Ok(TypeExpr::Named(input.to_string()));
    }
    if input.is_empty() {
        return Err("empty native MIR image field type".to_string());
    }
    Ok(TypeExpr::Named(input.to_string()))
}
fn parse_map(input: &str, hash: bool) -> Result<TypeExpr, String> {
    let parts = split_top_level(input, ',');
    if parts.len() != 2 {
        return Err(format!("native MIR map has invalid parameters `{input}`"));
    }
    Ok(TypeExpr::Map(
        Box::new(parse_type(parts[0].trim())?),
        Box::new(parse_type(parts[1].trim())?),
        hash,
    ))
}

fn generic_inner<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let (outer, arguments) = input.split_once('<')?;
    let outer = outer.rsplit("::").next().unwrap_or(outer);
    if outer != name {
        return None;
    }
    arguments.strip_suffix('>')
}

fn reachable_schema(schema: &Schema, root: &str) -> Result<BTreeSet<String>, String> {
    let mut pending = vec![root.to_string()];
    let mut reachable = BTreeSet::new();
    let mut alias_visiting = Vec::new();
    let mut checked_aliases = BTreeSet::new();
    while let Some(key) = pending.pop() {
        let key = resolve_alias(schema, &key)?;
        if !reachable.insert(key.clone()) {
            continue;
        }
        let definition = schema
            .definitions
            .get(&key)
            .ok_or_else(|| format!("missing declaration for `{key}`"))?;
        if definition.generic {
            if matches!(&definition.body, Body::Alias(_)) {
                return Err(format!(
                    "generic native MIR type alias `{}` needs an explicit instantiated wire schema",
                    definition.key
                ));
            }
            return Err(format!(
                "generic native MIR type `{}` needs an explicit instantiated wire schema",
                definition.key
            ));
        }
        if matches!(&definition.body, Body::Alias(_)) {
            detect_alias_cycle(
                schema,
                &key,
                &mut alias_visiting,
                &mut checked_aliases,
            )?;
        }
        if let Body::Enum(variants) = &definition.body {
            if let Some(variant) = variants
                .iter()
                .find(|variant| matches!(variant.body, VariantBody::Unsupported(_)))
            {
                let VariantBody::Unsupported(reason) = &variant.body else { unreachable!() };
                return Err(format!(
                    "enum `{}` variant `{}` has unsupported wire syntax `{reason}`",
                    definition.key, variant.name
                ));
            }
        }
        if definition.module != "crate::MIR" {
            if let Some(field) = body_fields(&definition.body)
                .into_iter()
                .find(|field| !field.public)
            {
                return Err(format!(
                    "native MIR image cannot access private field `{}` in `{}`",
                    field.name, definition.key
                ));
            }
        }
        let mut names = Vec::new();
        collect_named_types(&definition.body, &mut names);
        for name in names {
            if is_builtin(&name) {
                continue;
            }
            pending.push(resolve_type_name(schema, definition, &name)?);
        }
    }
    Ok(reachable)
}

fn detect_alias_cycle(
    schema: &Schema,
    key: &str,
    visiting: &mut Vec<String>,
    checked: &mut BTreeSet<String>,
) -> Result<(), String> {
    let key = resolve_alias(schema, key)?;
    let Some(definition) = schema.definitions.get(&key) else {
        return Err(format!("missing declaration for `{key}`"));
    };
    if definition.generic {
        let kind = if matches!(&definition.body, Body::Alias(_)) {
            "type alias"
        } else {
            "type"
        };
        return Err(format!(
            "generic native MIR {kind} `{}` needs an explicit instantiated wire schema",
            definition.key
        ));
    }
    if !matches!(&definition.body, Body::Alias(_)) {
        return Ok(());
    }
    if checked.contains(&key) {
        return Ok(());
    }
    if let Some(index) = visiting.iter().position(|item| item == &key) {
        let mut cycle = visiting[index..].to_vec();
        cycle.push(key);
        return Err(format!(
            "cyclic native MIR type aliases: {}",
            cycle.join(" -> ")
        ));
    }
    visiting.push(key.clone());
    let mut names = Vec::new();
    collect_named_types(&definition.body, &mut names);
    for name in names {
        if is_builtin(&name) {
            continue;
        }
        let target = resolve_type_name(schema, definition, &name)?;
        detect_alias_cycle(schema, &target, visiting, checked)?;
    }
    visiting.pop();
    checked.insert(key);
    Ok(())
}

fn collect_named_types(body: &Body, names: &mut Vec<String>) {
    fn collect(ty: &TypeExpr, names: &mut Vec<String>) {
        match ty {
            TypeExpr::Named(name) => names.push(name.clone()),
            TypeExpr::Option(inner)
            | TypeExpr::Sequence(inner)
            | TypeExpr::Set(inner, _)
            | TypeExpr::Boxed(inner)
            | TypeExpr::Arc(inner)
            | TypeExpr::Array(inner, _) => collect(inner, names),
            TypeExpr::Map(key, value, _) => {
                collect(key, names);
                collect(value, names);
            }
            TypeExpr::Tuple(items) => items.iter().for_each(|item| collect(item, names)),
            TypeExpr::StaticStr => {}
        }
    }
    match body {
        Body::Struct(fields) | Body::Tuple(fields) => {
            fields.iter().for_each(|field| collect(&field.ty, names))
        }
        Body::Alias(ty) => collect(ty, names),
        Body::Enum(variants) => {
            for variant in variants {
                match &variant.body {
                    VariantBody::Unit | VariantBody::Unsupported(_) => {}
                    VariantBody::Tuple(fields) => fields.iter().for_each(|field| collect(field, names)),
                    VariantBody::Struct(fields) => fields.iter().for_each(|field| collect(&field.ty, names)),
                }
            }
        }
        Body::Unit => {}
    }
}

fn body_fields(body: &Body) -> Vec<&Field> {
    match body {
        Body::Struct(fields) | Body::Tuple(fields) => fields.iter().collect(),
        Body::Enum(variants) => variants
            .iter()
            .flat_map(|variant| match &variant.body {
                VariantBody::Struct(fields) => fields.iter().collect::<Vec<_>>(),
                VariantBody::Unit | VariantBody::Tuple(_) | VariantBody::Unsupported(_) => Vec::new(),
            })
            .collect(),
        Body::Alias(_) | Body::Unit => Vec::new(),
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "String" | "bool" | "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16"
            | "i32" | "i64" | "isize" | "f32" | "f64" | "char" | "str"
            | "PathBuf" | "std::path::PathBuf"
    )
}
fn generic_type_base(name: &str) -> Option<&str> {
    let open = top_level_byte(name, b'<')?;
    let close = matching_byte(name.as_bytes(), open, b'<', b'>')?;
    if close + 1 != name.len() {
        return None;
    }
    let base = name[..open].trim();
    (!base.is_empty()).then_some(base)
}

fn resolve_type_name(schema: &Schema, definition: &Definition, name: &str) -> Result<String, String> {
    if let Some(base) = generic_type_base(name) {
        if !is_builtin(base) {
            if let Ok(key) = resolve_type_name(schema, definition, base) {
                if let Some(target) = schema.definitions.get(&key) {
                    if target.generic {
                        let kind = if matches!(&target.body, Body::Alias(_)) {
                            "type alias"
                        } else {
                            "type"
                        };
                        return Err(format!(
                            "generic native MIR {kind} `{key}` needs an explicit instantiated wire schema"
                        ));
                    }
                }
            }
        }
        return Err(format!(
            "unsupported generic native MIR type `{name}` referenced by `{}`",
            definition.key
        ));
    }
    if name == "Self" {
        return Ok(definition.key.clone());
    }
    if let Some(local) = schema.definitions.get(&format!("{}::{name}", definition.module)) {
        return Ok(local.key.clone());
    }
    if let Some(imported) = definition.imports.get(name) {
        return resolve_alias(schema, imported);
    }
    let normalized = normalize_module_path(&definition.module, name);
    if schema.definitions.contains_key(&normalized) || schema.aliases.contains_key(&normalized) {
        return resolve_alias(schema, &normalized);
    }
    let candidates = schema
        .definitions
        .values()
        .filter(|candidate| candidate.name == name)
        .map(|candidate| candidate.key.clone())
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [candidate] => Ok(candidate.clone()),
        [] => Err(format!("unknown native MIR type `{name}` referenced by `{}`", definition.key)),
        _ => Err(format!("ambiguous native MIR type `{name}` referenced by `{}`", definition.key)),
    }
}

fn resolve_alias(schema: &Schema, path: &str) -> Result<String, String> {
    let mut current = path.to_string();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return Err(format!("cyclic native MIR type alias `{path}`"));
        }
        if schema.definitions.contains_key(&current) {
            return Ok(current);
        }
        if let Some(next) = schema.aliases.get(&current) {
            current = next.clone();
        } else {
            return Err(format!("unknown native MIR type path `{current}`"));
        }
    }
}

fn assign_public_paths(schema: &mut Schema) {
    let mut paths = schema.definitions.keys()
        .map(|key| (key.clone(), key.clone()))
        .collect::<BTreeMap<_, _>>();
    for export in schema.aliases.keys() {
        if let Ok(key) = resolve_alias(schema, export) {
            let path = paths.get_mut(&key).expect("resolved schema definition");
            if (export.matches("::").count(), export) < (path.matches("::").count(), &*path) {
                *path = export.clone();
            }
        }
    }
    for (key, path) in paths {
        schema.definitions.get_mut(&key).unwrap().path = path;
    }
}

/// Definitions the image stores once in its value table: every occurrence in the
/// payload is a varint index into that table. A definition belongs here when
/// its values are large or repeat across the program. In the stage-zero
/// compiler image, 7,431 distinct checked types stand behind 307 MB of type
/// trees; spans, ownership facts and name references repeat per instruction;
/// value, block and place ids are 64-bit stable hashes used several times each.
const INTERNED_DEFINITIONS: &[&str] = &[
    "crate::MIR::MirType",
    "crate::Diagnostics::Span",
    "crate::MIR::MirOwnership",
    "crate::Names::NameReference",
    "crate::MIR::MirValueId",
    "crate::MIR::MirBlockId",
    "crate::MIR::MirPlaceId",
];

fn emit_codec(schema: &Schema, reachable: &BTreeSet<String>, interned: &[&str]) -> Result<String, String> {
    let mut out = String::new();
    for key in interned {
        if !reachable.contains(*key) {
            return Err(format!("interned native MIR definition `{key}` is outside the codec closure"));
        }
    }
    emit_runtime_codec_primitives(&mut out, schema, interned)?;
    let definitions = reachable
        .iter()
        .map(|key| schema.definitions.get(key).expect("reachable definition").clone())
        .collect::<Vec<_>>();
    for definition in &definitions {
        emit_definition_codec(&mut out, schema, reachable, definition, interned)?;
    }
    let root = schema
        .definitions
        .get("crate::MIR::MirProgram")
        .ok_or_else(|| "missing root MirProgram".to_string())?;
    let encode_root = function_name("encode", &root.key);
    let decode_root = function_name("decode", &root.key);
    writeln!(
        out,
        "pub fn mir_program_image_bytes(value: &MirProgram) -> Result<Vec<u8>, String> {{\n    value.validate().map_err(|error| format!(\"invalid MIR image source: {{error}}\"))?;\n    let mut interner = MirProgramImageInterner::default();\n    let mut writer = MirProgramImageWriter {{ bytes: Vec::new(), interner: &mut interner }};\n    {encode_root}(value, &mut writer)?;\n    let body = writer.finish();\n    mir_image_assemble(interner, body)\n}}\npub fn mir_program_from_image_bytes(bytes: &[u8]) -> Result<MirProgram, String> {{\n    let (tables, cursor) = mir_image_read_tables(bytes)?;\n    let mut reader = MirProgramImageReader {{ bytes, cursor, tables: &tables }};\n    let value = {decode_root}(&mut reader)?;\n    reader.finish()?;\n    value.validate().map_err(|error| format!(\"invalid restored MIR image: {{error}}\"))?;\n    Ok(value)\n}}\n"
    )
    .map_err(|error| error.to_string())?;
    Ok(out)
}

/// The image wire format: unsigned integers, enum tags, counts and lengths are
/// canonical LEB128 varints; signed integers are zigzag varints; floats keep
/// their fixed little-endian bits. The payload opens with the string table and
/// the value table of `INTERNED_DEFINITIONS`, in first-encounter order; strings
/// and interned values in the body are varint indices into those tables. Every
/// collection is encoded in a deterministic order (hash containers by sorted
/// key), so a restored program re-encodes to the identical bytes.
fn emit_runtime_codec_primitives(out: &mut String, schema: &Schema, interned: &[&str]) -> Result<(), String> {
    let mut table_fields = String::new();
    for key in interned {
        let definition = schema
            .definitions
            .get(*key)
            .ok_or_else(|| format!("missing interned native MIR definition `{key}`"))?;
        writeln!(table_fields, "    {}: Vec<{}>,", function_name("interned", key), definition.path)
            .map_err(|error| error.to_string())?;
    }
    let mut decode_entries = String::new();
    for (kind, key) in interned.iter().enumerate() {
        writeln!(
            decode_entries,
            "            {kind} => {{ let value = {}(&mut reader)?; cursor = reader.cursor; tables.{}.push(value); }}",
            function_name("decode_entry", key),
            function_name("interned", key),
        )
        .map_err(|error| error.to_string())?;
    }
    writeln!(
        out,
        "#[derive(Default)]\nstruct MirProgramImageTables {{\n    strings: Vec<String>,\n{table_fields}}}\n#[derive(Default)]\nstruct MirProgramImageInterner {{\n    strings: std::collections::HashMap<String, u64>,\n    string_table: Vec<u8>,\n    entries: std::collections::HashMap<(u32, Vec<u8>), u64>,\n    kind_counts: [u64; {}],\n    entry_table: Vec<u8>,\n    entry_count: u64,\n}}",
        interned.len()
    )
    .map_err(|error| error.to_string())?;
    // Table entries reference only earlier entries (nested values are interned
    // before the value holding them), so one forward pass restores the tables.
    writeln!(
        out,
        "fn mir_image_read_tables(bytes: &[u8]) -> Result<(MirProgramImageTables, usize), String> {{\n    let mut tables = MirProgramImageTables::default();\n    let mut cursor = {{\n        let mut reader = MirProgramImageReader::new(bytes, &tables);\n        let string_count = reader.read_count()?;\n        let mut strings = Vec::new();\n        strings.try_reserve_exact(string_count).map_err(|_| \"native MIR string table is too large\".to_string())?;\n        for _ in 0..string_count {{\n            let length = reader.read_len()?;\n            let text = std::str::from_utf8(reader.read_raw(length)?).map_err(|_| \"native MIR image string is not UTF-8\".to_string())?;\n            strings.push(text.to_string());\n        }}\n        let cursor = reader.cursor;\n        tables.strings = strings;\n        cursor\n    }};\n    let entry_count = {{\n        let mut reader = MirProgramImageReader {{ bytes, cursor, tables: &tables }};\n        let count = reader.read_count()?;\n        cursor = reader.cursor;\n        count\n    }};\n    for _ in 0..entry_count {{\n        let mut reader = MirProgramImageReader {{ bytes, cursor, tables: &tables }};\n        match reader.read_varint()? {{\n{decode_entries}            _ => return Err(\"unknown native MIR image table entry kind\".to_string()),\n        }}\n    }}\n    Ok((tables, cursor))\n}}"
    )
    .map_err(|error| error.to_string())?;
    out.push_str(
        r#"
fn mir_image_push_varint(bytes: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
}
fn mir_image_assemble(interner: MirProgramImageInterner, body: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(interner.string_table.len() + interner.entry_table.len() + body.len() + 20).map_err(|_| "native MIR image is too large to allocate".to_string())?;
    mir_image_push_varint(&mut bytes, u64::try_from(interner.strings.len()).map_err(|_| "native MIR string table exceeds u64".to_string())?);
    bytes.extend_from_slice(&interner.string_table);
    mir_image_push_varint(&mut bytes, interner.entry_count);
    bytes.extend_from_slice(&interner.entry_table);
    bytes.extend_from_slice(&body);
    Ok(bytes)
}
#[allow(dead_code)]
impl MirProgramImageInterner {
    fn intern_string(&mut self, value: &str) -> Result<u64, String> {
        if let Some(index) = self.strings.get(value) { return Ok(*index); }
        let index = u64::try_from(self.strings.len()).map_err(|_| "native MIR string table exceeds u64".to_string())?;
        mir_image_push_varint(&mut self.string_table, u64::try_from(value.len()).map_err(|_| "MIR image string exceeds u64".to_string())?);
        self.string_table.extend_from_slice(value.as_bytes());
        self.strings.insert(value.to_string(), index);
        Ok(index)
    }
    fn intern_entry(&mut self, kind: u32, entry: Vec<u8>) -> u64 {
        let next = self.kind_counts[kind as usize];
        match self.entries.entry((kind, entry)) {
            std::collections::hash_map::Entry::Occupied(found) => *found.get(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                mir_image_push_varint(&mut self.entry_table, u64::from(kind));
                self.entry_table.extend_from_slice(&slot.key().1);
                self.entry_count += 1;
                self.kind_counts[kind as usize] += 1;
                slot.insert(next);
                next
            }
        }
    }
}
struct MirProgramImageWriter<'t> { bytes: Vec<u8>, interner: &'t mut MirProgramImageInterner }
#[allow(dead_code)]
impl<'t> MirProgramImageWriter<'t> {
    fn child(&mut self) -> MirProgramImageWriter<'_> { MirProgramImageWriter { bytes: Vec::new(), interner: &mut *self.interner } }
    fn write_u8(&mut self, value: u8) { self.bytes.push(value); }
    fn write_varint(&mut self, value: u64) { mir_image_push_varint(&mut self.bytes, value); }
    fn write_u16(&mut self, value: u16) { self.write_varint(u64::from(value)); }
    fn write_u32(&mut self, value: u32) { self.write_varint(u64::from(value)); }
    fn write_u64(&mut self, value: u64) { self.write_varint(value); }
    fn write_signed(&mut self, value: i64) { self.write_varint(((value << 1) ^ (value >> 63)) as u64); }
    fn write_fixed_u32(&mut self, value: u32) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    fn write_fixed_u64(&mut self, value: u64) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    fn write_count(&mut self, value: usize) -> Result<(), String> {
        self.write_varint(u64::try_from(value).map_err(|_| "native MIR collection exceeds u64".to_string())?);
        Ok(())
    }
    fn write_bytes(&mut self, value: &[u8]) -> Result<(), String> {
        self.write_count(value.len())?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }
    fn write_string(&mut self, value: &str) -> Result<(), String> {
        let index = self.interner.intern_string(value)?;
        self.write_varint(index);
        Ok(())
    }
    fn write_entry(&mut self, kind: u32, entry: Vec<u8>) {
        let index = self.interner.intern_entry(kind, entry);
        self.write_varint(index);
    }
    fn finish(self) -> Vec<u8> { self.bytes }
}
struct MirProgramImageReader<'a> { bytes: &'a [u8], cursor: usize, tables: &'a MirProgramImageTables }
#[allow(dead_code)]
impl<'a> MirProgramImageReader<'a> {
    fn new(bytes: &'a [u8], tables: &'a MirProgramImageTables) -> Self { Self { bytes, cursor: 0, tables } }
    fn child(&self, bytes: &'a [u8]) -> Self { Self { bytes, cursor: 0, tables: self.tables } }
    fn read_raw(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self.cursor.checked_add(length).ok_or_else(|| "MIR image cursor overflow".to_string())?;
        let value = self.bytes.get(self.cursor..end).ok_or_else(|| "truncated native MIR image".to_string())?;
        self.cursor = end;
        Ok(value)
    }
    fn read_u8(&mut self) -> Result<u8, String> { Ok(self.read_raw(1)?[0]) }
    fn read_varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.read_u8()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 { return Err("native MIR image varint exceeds u64".to_string()); }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                if byte == 0 && shift != 0 { return Err("native MIR image varint is not canonical".to_string()); }
                return Ok(value);
            }
            shift += 7;
            if shift > 63 { return Err("native MIR image varint exceeds u64".to_string()); }
        }
    }
    fn read_u16(&mut self) -> Result<u16, String> { u16::try_from(self.read_varint()?).map_err(|_| "native MIR image u16 out of range".to_string()) }
    fn read_u32(&mut self) -> Result<u32, String> { u32::try_from(self.read_varint()?).map_err(|_| "native MIR image u32 out of range".to_string()) }
    fn read_u64(&mut self) -> Result<u64, String> { self.read_varint() }
    fn read_signed(&mut self) -> Result<i64, String> { let value = self.read_varint()?; Ok(((value >> 1) as i64) ^ -((value & 1) as i64)) }
    fn read_fixed_u32(&mut self) -> Result<u32, String> { Ok(u32::from_le_bytes(self.read_raw(4)?.try_into().map_err(|_| "invalid MIR image u32".to_string())?)) }
    fn read_fixed_u64(&mut self) -> Result<u64, String> { Ok(u64::from_le_bytes(self.read_raw(8)?.try_into().map_err(|_| "invalid MIR image u64".to_string())?)) }
    fn read_count(&mut self) -> Result<usize, String> {
        let count = usize::try_from(self.read_varint()?).map_err(|_| "native MIR image count exceeds host range".to_string())?;
        if count > self.bytes.len().saturating_sub(self.cursor) { return Err("native MIR image collection count exceeds remaining bytes".to_string()); }
        Ok(count)
    }
    fn read_len(&mut self) -> Result<usize, String> { self.read_count() }
    fn read_bytes(&mut self) -> Result<&'a [u8], String> { let length = self.read_len()?; self.read_raw(length) }
    fn read_index(&mut self) -> Result<usize, String> { usize::try_from(self.read_varint()?).map_err(|_| "native MIR image table index exceeds host range".to_string()) }
    fn read_string(&mut self) -> Result<String, String> {
        let index = self.read_index()?;
        self.tables.strings.get(index).cloned().ok_or_else(|| "native MIR image references an unknown string".to_string())
    }
    fn finish(self) -> Result<(), String> { if self.cursor == self.bytes.len() { Ok(()) } else { Err("native MIR image has trailing bytes".to_string()) } }
}
fn mir_image_static_str(value: String) -> Result<&'static str, String> {
    for record in crate::Syntax::CORE_CALLS {
        if record.module == value { return Ok(record.module); }
        if record.member == value { return Ok(record.member); }
        if record.symbol.name() == value { return Ok(record.symbol.name()); }
        if let Some(item) = record.receiver_types.iter().copied().find(|item| *item == value) { return Ok(item); }
        if let Some(item) = record.effect_leaf.filter(|item| *item == value) { return Ok(item); }
        if let Some(item) = record.jit_symbol.filter(|item| *item == value) { return Ok(item); }
        if let Some(marker) = record.marker {
            if marker.member == value { return Ok(marker.member); }
            if marker.since == value { return Ok(marker.since); }
            if marker.replacement == value { return Ok(marker.replacement); }
            if let Some(item) = marker.removed_in.filter(|item| *item == value) { return Ok(item); }
        }
    }
    Err("native MIR image contains an unknown static compiler string".to_string())
}
"#,
    );
    Ok(())
}

fn emit_definition_codec(
    out: &mut String,
    schema: &Schema,
    reachable: &BTreeSet<String>,
    definition: &Definition,
    interned: &[&str],
) -> Result<(), String> {
    // An interned definition's own codec writes its table entry; the public
    // encode/decode pair writes and resolves the table index.
    let interned = interned.iter().position(|key| *key == definition.key);
    let (encoder, decoder) = match interned {
        Some(_) => (function_name("encode_entry", &definition.key), function_name("decode_entry", &definition.key)),
        None => (function_name("encode", &definition.key), function_name("decode", &definition.key)),
    };
    let path = &definition.path;
    if let Some(kind) = interned {
        writeln!(
            out,
            "fn {}(value: &{path}, writer: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{\n    let mut entry = writer.child();\n    {encoder}(value, &mut entry)?;\n    let entry = entry.finish();\n    writer.write_entry({kind}, entry);\n    Ok(())\n}}\nfn {}(reader: &mut MirProgramImageReader<'_>) -> Result<{path}, String> {{\n    let index = reader.read_index()?;\n    reader.tables.{}.get(index).cloned().ok_or_else(|| \"native MIR image references an unknown interned value\".to_string())\n}}\n",
            function_name("encode", &definition.key),
            function_name("decode", &definition.key),
            function_name("interned", &definition.key),
        )
        .map_err(|error| error.to_string())?;
    }
    match &definition.body {
        Body::Struct(fields) => {
            let mut encode = String::new();
            let mut decode = Vec::new();
            for field in fields {
                let expression = encode_expression(&field.ty, &format!("&value.{}", field.name), "writer", schema, reachable, definition)?;
                encode.push_str(&expression);
                decode.push(format!("{}: {},", field.name, decode_expression(&field.ty, "reader", schema, reachable, definition)?));
            }
            writeln!(out, "fn {encoder}(value: &{path}, writer: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{\n{encode}    Ok(())\n}}\nfn {decoder}(reader: &mut MirProgramImageReader<'_>) -> Result<{path}, String> {{\n    Ok({path} {{\n        {}\n    }})\n}}\n", decode.join("\n"))
                .map_err(|error| error.to_string())?;
        }
        Body::Tuple(fields) => {
            let mut encode = String::new();
            let mut decode = Vec::new();
            for (index, field) in fields.iter().enumerate() {
                encode.push_str(&encode_expression(
                    &field.ty,
                    &format!("&value.{index}"),
                    "writer",
                    schema,
                    reachable,
                    definition,
                )?);
                decode.push(decode_expression(
                    &field.ty,
                    "reader",
                    schema,
                    reachable,
                    definition,
                )?);
            }
            let value = format!("{path}({})", decode.join(", "));
            writeln!(
                out,
                "fn {encoder}(value: &{path}, writer: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{\n{encode}    Ok(())\n}}\nfn {decoder}(reader: &mut MirProgramImageReader<'_>) -> Result<{path}, String> {{\n    Ok({value})\n}}\n"
            )
            .map_err(|error| error.to_string())?;
        }
        Body::Unit => {
            writeln!(out, "fn {encoder}(_: &{path}, _: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{ Ok(()) }}\nfn {decoder}(_: &mut MirProgramImageReader<'_>) -> Result<{path}, String> {{ Ok({path}) }}\n")
                .map_err(|error| error.to_string())?;
        }
        Body::Alias(ty) => {
            let encode = encode_expression(ty, "value", "writer", schema, reachable, definition)?;
            let decode = decode_expression(ty, "reader", schema, reachable, definition)?;
            writeln!(
                out,
                "fn {encoder}(value: &{path}, writer: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{\n{encode}    Ok(())\n}}\nfn {decoder}(reader: &mut MirProgramImageReader<'_>) -> Result<{path}, String> {{\n    Ok({decode})\n}}\n"
            )
            .map_err(|error| error.to_string())?;
        }
        Body::Enum(variants) => {
            let mut encode_arms = Vec::new();
            let mut decode_arms = Vec::new();
            for (tag, variant) in variants.iter().enumerate() {
                let tag = u32::try_from(tag).map_err(|_| format!("enum `{}` exceeds native MIR tag range", definition.key))?;
                let (pattern, encode_fields, decoded) = match &variant.body {
                    VariantBody::Unit => (format!("{path}::{}", variant.name), String::new(), Vec::new()),
                    VariantBody::Tuple(fields) => {
                        let bindings = (0..fields.len()).map(|index| format!("__field_{index}")).collect::<Vec<_>>();
                        let mut body = String::new();
                        for (field, binding) in fields.iter().zip(&bindings) {
                            body.push_str(&encode_expression(field, binding, "writer", schema, reachable, definition)?);
                        }
                        let decoded = fields.iter().map(|field| decode_expression(field, "reader", schema, reachable, definition)).collect::<Result<Vec<_>, _>>()?;
                        (format!("{path}::{}({})", variant.name, bindings.join(", ")), body, decoded)
                    }
                    VariantBody::Struct(fields) => {
                        let bindings = fields.iter().enumerate().map(|(index, field)| (field, format!("__field_{index}"))).collect::<Vec<_>>();
                        let mut body = String::new();
                        let mut pattern_fields = Vec::new();
                        let mut decoded = Vec::new();
                        for (field, binding) in bindings {
                            pattern_fields.push(format!("{}: {binding}", field.name));
                            body.push_str(&encode_expression(&field.ty, binding.as_str(), "writer", schema, reachable, definition)?);
                            decoded.push(format!("{}: {}", field.name, decode_expression(&field.ty, "reader", schema, reachable, definition)?));
                        }
                        (format!("{path}::{} {{ {} }}", variant.name, pattern_fields.join(", ")), body, decoded)
                    }
                    VariantBody::Unsupported(reason) => {
                        return Err(format!("unsupported variant wire syntax in `{}`: {reason}", definition.key));
                    }
                };
                encode_arms.push(format!("        {pattern} => {{ writer.write_u32({tag});\n{encode_fields}            Ok(())\n        }},"));
                let constructor = match &variant.body {
                    VariantBody::Unit => format!("{path}::{}", variant.name),
                    VariantBody::Tuple(_) => format!("{path}::{}({})", variant.name, decoded.join(", ")),
                    VariantBody::Struct(_) => format!("{path}::{} {{ {} }}", variant.name, decoded.join(", ")),
                    VariantBody::Unsupported(_) => unreachable!(),
                };
                decode_arms.push(format!("        {tag} => Ok({constructor}),"));
            }
            writeln!(out, "fn {encoder}(value: &{path}, writer: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{\n    match value {{\n{}\n    }}\n}}\nfn {decoder}(reader: &mut MirProgramImageReader<'_>) -> Result<{path}, String> {{\n    match reader.read_u32()? {{\n{}\n        _ => Err(\"unknown native MIR enum tag\".to_string()),\n    }}\n}}\n", encode_arms.join("\n"), decode_arms.join("\n"))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn encode_expression(
    ty: &TypeExpr,
    expression: &str,
    writer: &str,
    schema: &Schema,
    reachable: &BTreeSet<String>,
    context: &Definition,
) -> Result<String, String> {
    let mut out = String::new();
    match ty {
        TypeExpr::Option(inner) => {
            let body = encode_expression(inner, "__item", writer, schema, reachable, context)?;
            writeln!(out, "    match ({expression}).as_ref() {{ Some(__item) => {{ {writer}.write_u8(1);\n{body}    }}, None => {writer}.write_u8(0), }}\n").unwrap();
        }
        TypeExpr::Sequence(inner) => {
            let body = encode_expression(inner, "__item", writer, schema, reachable, context)?;
            writeln!(out, "    {writer}.write_u64(u64::try_from(({expression}).len()).map_err(|_| \"native MIR collection exceeds u64\".to_string())?);\n    for __item in ({expression}).iter() {{\n{body}    }}\n").unwrap();
        }
        TypeExpr::Set(inner, hash) => {
            let body = encode_expression(inner, "__item", "(&mut __item_writer)", schema, reachable, context)?;
            // Hash containers are walked in sorted order: interning assigns table
            // indices on first encounter, so the walk order must be canonical.
            let items = if *hash {
                format!("{{ let mut __sorted = ({expression}).iter().collect::<Vec<_>>(); __sorted.sort(); __sorted }}")
            } else {
                format!("({expression}).iter()")
            };
            writeln!(out, "    {{ let mut __items = Vec::with_capacity(({expression}).len()); for __item in {items} {{ let mut __item_writer = {writer}.child();\n{body}        __items.push(__item_writer.finish()); }} __items.sort(); if __items.windows(2).any(|pair| pair[0] == pair[1]) {{ return Err(\"duplicate native MIR set element encoding\".to_string()); }} {writer}.write_count(__items.len())?; for __item in __items {{ {writer}.write_bytes(&__item)?; }} }}\n").unwrap();
        }
        TypeExpr::Map(key, value, hash) => {
            let key_body = encode_expression(key, "__key", "(&mut __key_writer)", schema, reachable, context)?;
            let value_body = encode_expression(value, "__value", "(&mut __value_writer)", schema, reachable, context)?;
            let entries = if *hash {
                format!("{{ let mut __sorted = ({expression}).iter().collect::<Vec<_>>(); __sorted.sort_by(|left, right| left.0.cmp(right.0)); __sorted }}")
            } else {
                format!("({expression}).iter()")
            };
            writeln!(out, "    {{ let mut __entries = Vec::with_capacity(({expression}).len()); for (__key, __value) in {entries} {{ let __key_bytes = {{ let mut __key_writer = {writer}.child();\n{key_body}        __key_writer.finish() }}; let __value_bytes = {{ let mut __value_writer = {writer}.child();\n{value_body}        __value_writer.finish() }}; __entries.push((__key_bytes, __value_bytes)); }} __entries.sort_by(|left, right| left.0.cmp(&right.0)); if __entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {{ return Err(\"duplicate native MIR map key encoding\".to_string()); }} {writer}.write_count(__entries.len())?; for (__key, __value) in __entries {{ {writer}.write_bytes(&__key)?; {writer}.write_bytes(&__value)?; }} }}\n").unwrap();
        }
        TypeExpr::Array(inner, _) => {
            let body = encode_expression(inner, "__item", writer, schema, reachable, context)?;
            writeln!(out, "    for __item in ({expression}).iter() {{\n{body}    }}\n").unwrap();
        }
        TypeExpr::Boxed(inner) | TypeExpr::Arc(inner) => {
            return encode_expression(inner, &format!("({expression}).as_ref()"), writer, schema, reachable, context);
        }
        TypeExpr::Tuple(items) => {
            for (index, item) in items.iter().enumerate() {
                out.push_str(&encode_expression(item, &format!("&({expression}).{index}"), writer, schema, reachable, context)?);
            }
        }
        TypeExpr::StaticStr => writeln!(out, "    {writer}.write_string({expression})?;\n").unwrap(),
        TypeExpr::Named(name) => emit_named_encode(&mut out, name, expression, writer, schema, reachable, context)?,
    }
    Ok(out)
}

fn emit_named_encode(
    out: &mut String,
    name: &str,
    expression: &str,
    writer: &str,
    schema: &Schema,
    reachable: &BTreeSet<String>,
    context: &Definition,
) -> Result<(), String> {
    match name {
        "String" => writeln!(out, "    {writer}.write_string(({expression}).as_str())?;\n").unwrap(),
        "PathBuf" | "std::path::PathBuf" => writeln!(out, "    {writer}.write_string(({expression}).to_str().ok_or_else(|| \"native MIR path is not valid UTF-8\".to_string())?)?;\n").unwrap(),
        "bool" => writeln!(out, "    {writer}.write_u8(u8::from(*({expression})));\n").unwrap(),
        "u8" => writeln!(out, "    {writer}.write_u8(*({expression}));\n").unwrap(),
        "u16" => writeln!(out, "    {writer}.write_u16(*({expression}));\n").unwrap(),
        "u32" => writeln!(out, "    {writer}.write_u32(*({expression}));\n").unwrap(),
        "u64" => writeln!(out, "    {writer}.write_u64(*({expression}));\n").unwrap(),
        "usize" => writeln!(out, "    {writer}.write_u64(u64::try_from(*({expression})).map_err(|_| \"native MIR usize exceeds u64\".to_string())?);\n").unwrap(),
        "i8" => writeln!(out, "    {writer}.write_u8(*({expression}) as u8);\n").unwrap(),
        "i16" | "i32" | "i64" => writeln!(out, "    {writer}.write_signed(i64::from(*({expression})));\n").unwrap(),
        "isize" => writeln!(out, "    {writer}.write_signed(*({expression}) as i64);\n").unwrap(),
        "f32" => writeln!(out, "    {writer}.write_fixed_u32(({expression}).to_bits());\n").unwrap(),
        "f64" => writeln!(out, "    {writer}.write_fixed_u64(({expression}).to_bits());\n").unwrap(),
        "char" => writeln!(out, "    {writer}.write_u32(*({expression}) as u32);\n").unwrap(),
        _ => {
            let key = resolve_type_name(schema, context, name)?;
            if !reachable.contains(&key) {
                return Err(format!("native MIR type `{key}` is outside generated codec closure"));
            }
            let path = function_name("encode", &key);
            writeln!(out, "    {path}({expression}, &mut *{writer})?;\n").unwrap();
        }
    }
    Ok(())
}

fn decode_expression(
    ty: &TypeExpr,
    reader: &str,
    schema: &Schema,
    reachable: &BTreeSet<String>,
    context: &Definition,
) -> Result<String, String> {
    Ok(match ty {
        TypeExpr::Option(inner) => {
            let value = decode_expression(inner, reader, schema, reachable, context)?;
            format!("match {reader}.read_u8()? {{ 0 => None, 1 => Some({value}), _ => return Err(\"invalid native MIR option tag\".to_string()), }}")
        }
        TypeExpr::Sequence(inner) => {
            let value = decode_expression(inner, reader, schema, reachable, context)?;
            format!("{{ let __count = {reader}.read_count()?; let mut __items = Vec::new(); __items.try_reserve_exact(__count).map_err(|_| \"native MIR sequence is too large\".to_string())?; for _ in 0..__count {{ __items.push({value}); }} __items }}")
        }
        TypeExpr::Set(inner, hash) => {
            let value = decode_expression(inner, "(&mut __item_reader)", schema, reachable, context)?;
            let container = if *hash { "std::collections::HashSet" } else { "std::collections::BTreeSet" };
            format!("{{ let __count = {reader}.read_count()?; let mut __items = Vec::new(); __items.try_reserve_exact(__count).map_err(|_| \"native MIR set is too large\".to_string())?; let mut __seen = BTreeSet::new(); for _ in 0..__count {{ let __bytes = {reader}.read_bytes()?; if !__seen.insert(__bytes.to_vec()) {{ return Err(\"duplicate native MIR set element\".to_string()); }} let mut __item_reader = {reader}.child(__bytes); let __item = {value}; __item_reader.finish()?; __items.push(__item); }} let __output = __items.into_iter().collect::<{container}<_>>(); if __output.len() != __count {{ return Err(\"duplicate native MIR set element\".to_string()); }} __output }}")
        }
        TypeExpr::Map(key, value, hash) => {
            let key_value = decode_expression(key, "(&mut __key_reader)", schema, reachable, context)?;
            let value_value = decode_expression(value, "(&mut __value_reader)", schema, reachable, context)?;
            let container = if *hash { "std::collections::HashMap" } else { "std::collections::BTreeMap" };
            format!("{{ let __count = {reader}.read_count()?; let mut __items = Vec::new(); __items.try_reserve_exact(__count).map_err(|_| \"native MIR map is too large\".to_string())?; let mut __seen = BTreeSet::new(); for _ in 0..__count {{ let __key_bytes = {reader}.read_bytes()?; let mut __key_reader = {reader}.child(__key_bytes); let __key = {key_value}; __key_reader.finish()?; if !__seen.insert(__key_bytes.to_vec()) {{ return Err(\"duplicate native MIR map key\".to_string()); }} let __value_bytes = {reader}.read_bytes()?; let mut __value_reader = {reader}.child(__value_bytes); let __value = {value_value}; __value_reader.finish()?; __items.push((__key, __value)); }} let __output = __items.into_iter().collect::<{container}<_, _>>(); if __output.len() != __count {{ return Err(\"duplicate native MIR map key\".to_string()); }} __output }}")
        }
        TypeExpr::Boxed(inner) => format!("Box::new({})", decode_expression(inner, reader, schema, reachable, context)?),
        TypeExpr::Arc(inner) => format!("std::sync::Arc::new({})", decode_expression(inner, reader, schema, reachable, context)?),
        TypeExpr::Tuple(items) => {
            let values = items.iter().map(|item| decode_expression(item, reader, schema, reachable, context)).collect::<Result<Vec<_>, _>>()?;
            format!("({}{})", values.join(", "), if values.len() == 1 { "," } else { "" })
        }
        TypeExpr::Array(inner, length) => {
            let values = (0..*length)
                .map(|_| decode_expression(inner, reader, schema, reachable, context))
                .collect::<Result<Vec<_>, _>>()?;
            format!("[{}]", values.join(", "))
        }
        TypeExpr::StaticStr => format!("mir_image_static_str({reader}.read_string()?)?"),
        TypeExpr::Named(name) => decode_named(name, reader, schema, reachable, context)?,
    })
}

fn decode_named(
    name: &str,
    reader: &str,
    schema: &Schema,
    reachable: &BTreeSet<String>,
    context: &Definition,
) -> Result<String, String> {
    Ok(match name {
        "String" => format!("{reader}.read_string()?"),
        "PathBuf" | "std::path::PathBuf" => format!("std::path::PathBuf::from({reader}.read_string()?)"),
        "bool" => format!("match {reader}.read_u8()? {{ 0 => false, 1 => true, _ => return Err(\"invalid native MIR boolean\".to_string()), }}"),
        "u8" => format!("{reader}.read_u8()?"),
        "u16" => format!("{reader}.read_u16()?"),
        "u32" => format!("{reader}.read_u32()?"),
        "u64" => format!("{reader}.read_u64()?"),
        "usize" => format!("usize::try_from({reader}.read_u64()?).map_err(|_| \"native MIR usize exceeds host range\".to_string())?"),
        "i8" => format!("{reader}.read_u8()? as i8"),
        "i16" => format!("i16::try_from({reader}.read_signed()?).map_err(|_| \"native MIR i16 out of range\".to_string())?"),
        "i32" => format!("i32::try_from({reader}.read_signed()?).map_err(|_| \"native MIR i32 out of range\".to_string())?"),
        "i64" => format!("{reader}.read_signed()?"),
        "isize" => format!("isize::try_from({reader}.read_signed()?).map_err(|_| \"native MIR isize exceeds host range\".to_string())?"),
        "f32" => format!("f32::from_bits({reader}.read_fixed_u32()?)"),
        "f64" => format!("f64::from_bits({reader}.read_fixed_u64()?)"),
        "char" => format!("char::from_u32({reader}.read_u32()?).ok_or_else(|| \"invalid native MIR character\".to_string())?"),
        _ => {
            let key = resolve_type_name(schema, context, name)?;
            if !reachable.contains(&key) {
                return Err(format!("native MIR type `{key}` is outside generated codec closure"));
            }
            format!("{}(&mut *{reader})?", function_name("decode", &key))
        }
    })
}

fn function_name(prefix: &str, key: &str) -> String {
    let suffix = key
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '_' })
        .collect::<String>();
    format!("__mir_image_{prefix}_{suffix}")
}
fn find_next_definition(source: &str, from: usize) -> Option<(&'static str, usize)> {
    let mut next: Option<(&'static str, usize)> = None;
    for (kind, position) in [
        ("struct", find_keyword(source, "pub struct", from)),
        ("enum", find_keyword(source, "pub enum", from)),
        ("type", find_keyword(source, "pub type", from)),
    ] {
        if let Some(position) = position {
            if next.map_or(true, |(_, current)| position < current) {
                next = Some((kind, position));
            }
        }
    }
    next
}

fn find_keyword(source: &str, keyword: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some(relative) = source[cursor..].find(keyword) {
        let index = cursor + relative;
        if boundary(source.as_bytes().get(index.wrapping_sub(1)).copied())
            && boundary(source.as_bytes().get(index + keyword.len()).copied())
        {
            return Some(index);
        }
        cursor = index + 1;
    }
    None
}

fn token_at(source: &str, index: usize, token: &str) -> bool {
    source[index..].starts_with(token)
        && boundary(source.as_bytes().get(index.wrapping_sub(1)).copied())
        && boundary(source.as_bytes().get(index + token.len()).copied())
}

fn boundary(byte: Option<u8>) -> bool {
    byte.map_or(true, |byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
}

fn read_identifier(input: &str, cursor: &mut usize) -> Result<String, String> {
    let start = *cursor;
    if input.as_bytes().get(*cursor..*cursor + 2) == Some(b"r#") {
        *cursor += 2;
    }
    let identifier_start = *cursor;
    while input.as_bytes().get(*cursor).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
        *cursor += 1;
    }
    if *cursor == identifier_start {
        Err(format!("expected identifier at byte {start}"))
    } else {
        Ok(input[start..*cursor].to_string())
    }
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn skip_space(input: &str, cursor: &mut usize) {
    while input.as_bytes().get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
}

fn matching_byte(input: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in input.iter().enumerate().skip(start) {
        if *byte == open {
            depth += 1;
        } else if *byte == close && !(close == b'>' && index > 0 && input[index - 1] == b'-') {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn top_level_byte(input: &str, wanted: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in input.bytes().enumerate() {
        if byte == wanted && depth == 0 {
            return Some(index);
        }
        match byte {
            b'<' | b'(' | b'[' | b'{' => depth += 1,
            b'>' if index > 0 && input.as_bytes()[index - 1] == b'-' => {}
            b'>' | b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn split_top_level(input: &str, delimiter: char) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in input.char_indices() {
        match character {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' if index > 0 && input.as_bytes()[index - 1] == b'-' => {}
            '>' | ')' | ']' | '}' => depth = depth.saturating_sub(1),
            character if character == delimiter && depth == 0 => {
                pieces.push(&input[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    pieces.push(&input[start..]);
    pieces
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_imports_use_alias_or_final_path_component() {
        let mut imports = BTreeMap::new();
        insert_use_tree(
            "crate::Mir::{Value, Instruction as Op, nested::Block, *}",
            "crate::Image",
            &mut imports,
        );
        assert_eq!(
            imports,
            BTreeMap::from([
                ("Value".to_string(), "crate::Mir::Value".to_string()),
                ("Op".to_string(), "crate::Mir::Instruction".to_string()),
                ("Block".to_string(), "crate::Mir::nested::Block".to_string()),
            ])
        );
    }

    #[test]
    fn character_mask_preserves_lifetimes_and_nested_field_delimiters() {
        let source = "pub struct Rows { pub rows: &'static [(&'static str, Kind)], pub next: u64 }";
        let masked = mask_comments_and_literals(source);
        assert_eq!(masked, source);
        let definitions = parse_definitions(&masked, "crate::Rows", &BTreeMap::new()).unwrap();
        let Body::Struct(fields) = &definitions[0].body else {
            panic!("expected struct");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "rows");
        assert_eq!(fields[1].name, "next");
        assert_eq!(mask_comments_and_literals("&'a str, &'b str"), "&'a str, &'b str");
    }

    #[test]
    fn character_mask_handles_unicode_and_escapes_without_hiding_following_code() {
        for literal in ["'a'", "'é'", "'中'", "'\u{10400}'", r"'\n'", r"'\''", r"'\\'", r"'\x7f'", r"'\u{10400}'"] {
            let source = format!("{literal}, following");
            let expected = format!("{}, following", " ".repeat(literal.len()));
            assert_eq!(mask_comments_and_literals(&source), expected, "{literal}");
        }
    }

    #[test]
    fn function_arrows_do_not_close_generic_delimiters() {
        let ty = "Option<Arc<dyn Fn(u8) -> Result<(u8, u8), String>>>";
        assert_eq!(matching_byte(ty.as_bytes(), 6, b'<', b'>'), Some(ty.len() - 1));
        let fields = format!("pub callback: {ty}, pub next: u64");
        assert_eq!(split_top_level(&fields, ',').len(), 2);
        assert_eq!(parse_fields(&fields).unwrap().len(), 2);
        let suffix = format!("{ty}, following");
        assert_eq!(top_level_byte(&suffix, b','), Some(ty.len()));
    }

    #[test]
    fn recursive_self_type_resolves_to_its_declaring_definition() {
        let definitions = parse_definitions(
            "pub enum Node { Leaf, Child(Box<Self>) }",
            "crate::Graph",
            &BTreeMap::new(),
        ).unwrap();
        let mut schema = Schema::default();
        let definition = definitions[0].clone();
        schema.definitions.insert(definition.key.clone(), definition.clone());
        assert_eq!(resolve_type_name(&schema, &definition, "Self").unwrap(), "crate::Graph::Node");
    }
    #[test]
    fn type_aliases_follow_imported_chains_and_container_wire_schema() {
        let source = mask_comments_and_literals(
            "use crate::Kinds::Kind;
             pub type KindAlias = Kind;
             pub type KindSet = BTreeSet<KindAlias>;
             pub struct MirProgram { pub items: KindSet }",
        );
        let imports = parse_imports(&source, "crate::MIR");
        let definitions = parse_definitions(&source, "crate::MIR", &imports).unwrap();
        let kind_definitions = parse_definitions(
            "pub type Code = u8; pub struct Kind { pub code: Code }",
            "crate::Kinds",
            &BTreeMap::new(),
        )
        .unwrap();
        let mut schema = Schema::default();
        for definition in kind_definitions.into_iter().chain(definitions) {
            schema.definitions.insert(definition.key.clone(), definition);
        }
        assign_public_paths(&mut schema);

        let reachable = reachable_schema(&schema, "crate::MIR::MirProgram").unwrap();
        assert!(reachable.contains("crate::MIR::KindAlias"));
        assert!(reachable.contains("crate::MIR::KindSet"));
        assert!(reachable.contains("crate::Kinds::Kind"));
        assert!(reachable.contains("crate::Kinds::Code"));
        let generated = emit_codec(&schema, &reachable, &[]).unwrap();
        assert!(generated.contains(&format!(
            "fn {}(",
            function_name("encode", "crate::MIR::KindSet")
        )));
        assert!(generated.contains(&format!(
            "fn {}(",
            function_name("decode", "crate::MIR::KindSet")
        )));
        assert!(generated.contains("duplicate native MIR set element encoding"));
    }

    #[test]
    fn reachable_alias_only_cycles_are_rejected_without_recursing_forever() {
        let definitions = parse_definitions(
            "pub type First = Option<Second>;
             pub type Second = Box<First>;
             pub struct MirProgram { pub value: First }",
            "crate::MIR",
            &BTreeMap::new(),
        )
        .unwrap();
        let mut schema = Schema::default();
        for definition in definitions {
            schema.definitions.insert(definition.key.clone(), definition);
        }

        let error = reachable_schema(&schema, "crate::MIR::MirProgram").unwrap_err();
        assert!(error.contains("cyclic native MIR type aliases"));
        assert!(error.contains("crate::MIR::First"));
        assert!(error.contains("crate::MIR::Second"));
    }

    #[test]
    fn reachable_generic_aliases_report_the_unsupported_wire_contract() {
        let definitions = parse_definitions(
            "pub type Generic<T> = Vec<T>;
             pub struct MirProgram { pub value: Generic<u8> }",
            "crate::MIR",
            &BTreeMap::new(),
        )
        .unwrap();
        let mut schema = Schema::default();
        for definition in definitions {
            schema.definitions.insert(definition.key.clone(), definition);
        }

        let error = reachable_schema(&schema, "crate::MIR::MirProgram").unwrap_err();
        assert!(error.contains("generic native MIR type alias `crate::MIR::Generic`"));
        assert!(error.contains("explicit instantiated wire schema"));
    }

    #[test]
    fn public_paths_follow_reexports_instead_of_private_modules() {
        let mut schema = Schema::default();
        for module in ["crate::AST::details", "crate::Syntax::details"] {
            let definition = parse_definitions("pub enum Kind { One }", module, &BTreeMap::new()).unwrap().remove(0);
            schema.definitions.insert(definition.key.clone(), definition);
        }
        parse_public_uses("pub use details::Kind;", "crate::AST", &mut schema);
        parse_public_uses("pub use details::Kind as ExportedKind;", "crate::Syntax", &mut schema);
        parse_public_uses("pub use crate::Syntax::ExportedKind as Kind;", "crate", &mut schema);
        assign_public_paths(&mut schema);
        assert_eq!(schema.definitions["crate::AST::details::Kind"].path, "crate::AST::Kind");
        assert_eq!(schema.definitions["crate::Syntax::details::Kind"].path, "crate::Kind");
    }

    #[test]
    fn explicit_rust_discriminants_preserve_wire_variant_order_and_payloads() {
        let variants = parse_variants("First = 7, Second(u64) = 2, Third { value: u8 } = -1").unwrap();
        assert_eq!(variants.iter().map(|variant| variant.name.as_str()).collect::<Vec<_>>(), ["First", "Second", "Third"]);
        assert!(matches!(variants[0].body, VariantBody::Unit));
        assert!(matches!(&variants[1].body, VariantBody::Tuple(fields) if fields.len() == 1));
        assert!(matches!(&variants[2].body, VariantBody::Struct(fields) if fields.len() == 1 && fields[0].name == "value"));
        assert!(matches!(parse_variants("Broken =").unwrap()[0].body, VariantBody::Unsupported(_)));
    }

    #[test]
    fn generated_nested_containers_and_paths_round_trip() {
        let spelling = "(BTreeMap<std::path::PathBuf, BTreeSet<BTreeMap<Leaf, Leaf>>>, HashMap<String, Leaf>, Vec<(i32, f64, String)>)";
        let ty = parse_type(spelling).unwrap();
        let context = parse_definitions("pub struct Leaf(pub u64);", "crate", &BTreeMap::new()).unwrap().remove(0);
        let mut schema = Schema::default();
        schema.definitions.insert(context.key.clone(), context.clone());
        let reachable = BTreeSet::from([context.key.clone()]);
        let interned = [context.key.as_str()];
        let mut leaf_codec = String::new();
        emit_definition_codec(&mut leaf_codec, &schema, &reachable, &context, &interned).unwrap();
        let encode = encode_expression(&ty, "value", "writer", &schema, &reachable, &context).unwrap();
        let decode = decode_expression(&ty, "reader", &schema, &reachable, &context).unwrap();
        let mut primitives = String::new();
        emit_runtime_codec_primitives(&mut primitives, &schema, &interned).unwrap();
        let primitives = primitives.split("fn mir_image_static_str").next().unwrap();
        let source = format!(r#"
use std::collections::{{BTreeMap, BTreeSet, HashMap}};
type Value = {spelling};
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Leaf(pub u64);
{leaf_codec}
{primitives}
fn encode(value: &Value, writer: &mut MirProgramImageWriter<'_>) -> Result<(), String> {{
{encode}
    Ok(())
}}
fn decode(reader: &mut MirProgramImageReader<'_>) -> Result<Value, String> {{
    Ok({decode})
}}
fn image(value: &Value) -> Result<Vec<u8>, String> {{
    let mut interner = MirProgramImageInterner::default();
    let mut writer = MirProgramImageWriter {{ bytes: Vec::new(), interner: &mut interner }};
    encode(value, &mut writer)?;
    let body = writer.finish();
    mir_image_assemble(interner, body)
}}
fn restore(bytes: &[u8]) -> Result<Value, String> {{
    let (tables, cursor) = mir_image_read_tables(bytes)?;
    let mut reader = MirProgramImageReader {{ bytes, cursor, tables: &tables }};
    let value = decode(&mut reader)?;
    reader.finish()?;
    Ok(value)
}}
fn main() {{
    let leaves = BTreeMap::from([(std::path::PathBuf::from("outer/λ"), BTreeSet::from([
        BTreeMap::from([(Leaf(1), Leaf(7)), (Leaf(2), Leaf(11))]),
        BTreeMap::from([(Leaf(3), Leaf(7))]),
    ]))]);
    let named = (0..64).map(|index| (format!("key{{index}}"), Leaf(index % 3))).collect::<HashMap<_, _>>();
    let rows = vec![(-1, 0.5, "outer/λ".to_string()), (i32::MIN, f64::NAN, "key1".to_string()), (i32::MAX, -0.0, String::new())];
    let value = (leaves, named, rows);
    let bytes = image(&value).unwrap();
    let restored = restore(&bytes).unwrap();
    assert_eq!(restored.0, value.0);
    assert_eq!(restored.1, value.1);
    for (left, right) in restored.2.iter().zip(&value.2) {{
        assert_eq!((left.0, left.1.to_bits(), &left.2), (right.0, right.1.to_bits(), &right.2));
    }}
    // Interning stores each distinct string and Leaf once, and the hash map
    // is walked in key order, so the restored value re-encodes identically.
    assert_eq!(image(&restored).unwrap(), bytes);
    assert_eq!(bytes.windows("outer/λ".len()).filter(|window| *window == "outer/λ".as_bytes()).count(), 1);
    assert!(restore(&bytes[..bytes.len() - 1]).is_err());
    let mut overlong = bytes.clone();
    overlong.splice(0..1, [overlong[0] | 0x80, 0]);
    assert!(restore(&overlong).unwrap_err().contains("not canonical"));
    #[cfg(unix)]
    {{
        use std::os::unix::ffi::OsStringExt;
        let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
        let invalid = (BTreeMap::from([(path, BTreeSet::new())]), HashMap::new(), Vec::new());
        assert!(image(&invalid).unwrap_err().contains("not valid UTF-8"));
    }}
}}
"#);
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let scratch = PathBuf::from(std::env::var_os("HOME").expect("disk-backed test scratch requires HOME"))
            .join(".cache/jet-test-scratch")
            .join(format!("mir-codec-roundtrip-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&scratch).unwrap();
        let source_path = scratch.join("roundtrip.rs");
        let binary = scratch.join("roundtrip");
        fs::write(&source_path, source).unwrap();
        let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let built = std::process::Command::new(compiler)
            .args(["--edition=2024", "-Awarnings", "-Dunused_parens"])
            .arg(&source_path).arg("-o").arg(&binary).output().unwrap();
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        let run = std::process::Command::new(&binary).output().unwrap();
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        fs::remove_dir_all(scratch).unwrap();
    }
}

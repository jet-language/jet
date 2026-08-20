use super::*;

fn frame_text(out: &mut Vec<u8>, text: &str) { super::frame_bytes(out, text.as_bytes()); }

pub(super) fn type_full_key(ty: &Type) -> Vec<u8> {
    fn write(out: &mut Vec<u8>, ty: &Type) {
        use Type::*;
        match ty {
            Int => out.push(1), Float => out.push(2), Bool => out.push(3), String => out.push(4), Char => out.push(5),
            List(inner) => { out.push(6); write(out, inner); }
            Map { key, value, .. } => { out.push(7); write(out, key); write(out, value); }
            Shared(inner) => { out.push(8); write(out, inner); }
            Option(inner) => { out.push(9); write(out, inner); }
            Result { ok, err } => { out.push(10); write(out, ok); write(out, err); }
            Fn { params, ret, .. } => {
                out.push(11); out.extend_from_slice(&(params.len() as u64).to_be_bytes());
                for param in params { write(out, param); }
                match ret { Some(ret) => { out.push(1); write(out, ret); }, None => out.push(0) }
            }
            Named(name) => { out.push(12); frame_text(out, name); }
            Apply { name, args } => {
                out.push(13); frame_text(out, name); out.extend_from_slice(&(args.len() as u64).to_be_bytes());
                for arg in args { write(out, arg); }
            }
            TraitObject(names) => { out.push(14); out.extend_from_slice(&(names.len() as u64).to_be_bytes()); for name in names { frame_text(out, name); } }
            Tuple(fields) => { out.push(15); out.extend_from_slice(&(fields.len() as u64).to_be_bytes()); for (name, ty) in fields { frame_text(out, name); write(out, ty); } }
            FixedList { elem, len, .. } => {
                out.push(16);
                write(out, elem);
                frame_text(out, len.kind());
                frame_text(out, &len.expression());
            }
            IntN { signed, bits } => { out.push(17); out.push(u8::from(*signed)); out.push(*bits); }
            Float32 => out.push(18),
            Tagged { inner, .. } => write(out, inner),
            Union(members) => {
                out.push(19);
                out.extend_from_slice(&(members.len() as u64).to_be_bytes());
                for m in members { write(out, m); }
            }
            Quantity { base, dimension } => {
                out.push(20);
                write(out, base);
                frame_text(out, &dimension.identity());
            }
            Measure(measure) => {
                out.push(21);
                frame_text(out, measure.kind());
                frame_text(out, &measure.expression());
            }
            InlineRange { base, lo, hi } => {
                out.push(22);
                write(out, base);
                out.extend_from_slice(&lo.to_be_bytes());
                out.extend_from_slice(&hi.to_be_bytes());
            }
        }
    }
    let mut out = Vec::new();
    super::frame_bytes(&mut out, b"jet.type.full-key.v1");
    write(&mut out, ty);
    out
}

pub(super) fn parameter_bytes(params: &[ResolvedModuleParam]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(params.len() as u64).to_be_bytes());
    for param in params {
        match param {
            ResolvedModuleParam::Type { name, bound } => {
                out.push(0); frame_text(&mut out, name);
                frame_text(&mut out, bound.as_deref().unwrap_or(""));
            }
            ResolvedModuleParam::Value { name, ty } => {
                out.push(1); frame_text(&mut out, name); super::frame_bytes(&mut out, &type_full_key(ty));
            }
            ResolvedModuleParam::Invalid => out.push(2),
        }
    }
    out
}

pub(super) fn definition_full_key(package_identity: &str, module_path: &str, lexical_path: &str, name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    super::frame_bytes(&mut out, b"jet.genmod.definition.v1");
    frame_text(&mut out, package_identity);
    frame_text(&mut out, module_path);
    frame_text(&mut out, lexical_path);
    frame_text(&mut out, "generic-module");
    frame_text(&mut out, name);
    out
}

fn quoted_field(text: &str, field: &str) -> Option<String> {
    let needle = format!("{field}:");
    text.match_indices(&needle).find_map(|(offset, _)| {
        let boundary = text[..offset].chars().next_back();
        if boundary.is_some_and(|ch| ch.is_alphanumeric() || ch == '_') { return None; }
        let rest = text[offset + needle.len()..].trim_start().strip_prefix('"')?;
        Some(rest.split('"').next()?.to_string())
    })
}

fn canonical_semver(version: &str) -> String {
    let (core_pre, build) = version.split_once('+').map_or((version, None), |(core, build)| (core, Some(build)));
    let (core, pre) = core_pre.split_once('-').map_or((core_pre, None), |(core, pre)| (core, Some(pre)));
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()
        || !part.bytes().all(|byte| byte.is_ascii_digit())
        || (part.len() > 1 && part.starts_with('0')))
    {
        return version.trim().to_string();
    }
    let mut canonical = parts.iter().map(|part| part.parse::<u64>().unwrap_or(0).to_string()).collect::<Vec<_>>().join(".");
    if let Some(pre) = pre { canonical.push('-'); canonical.push_str(pre); }
    if let Some(build) = build { canonical.push('+'); canonical.push_str(build); }
    canonical
}

fn lock_value(line: &str, field: &str) -> Option<String> {
    let (key, value) = line.split_once('=')?;
    (key.trim() == field).then(|| value.trim().trim_matches('"').to_string())
}

fn inline_lock_value(table: &str, field: &str) -> Option<String> {
    let table = table.trim().trim_start_matches('{').trim_end_matches('}');
    table.split(',').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key.trim() == field).then(|| value.trim().trim_matches('"').to_string())
    })
}

fn credential_free_git_url(url: &str) -> String {
    let without_fragment = url.split(['?', '#']).next().unwrap_or(url);
    let Some((scheme, authority_path)) = without_fragment.split_once("://") else {
        return without_fragment.to_string();
    };
    let (authority, path) = authority_path.split_once('/').unwrap_or((authority_path, ""));
    let clean_authority = authority.rsplit_once('@').map_or(authority, |(_, clean)| clean);
    format!("{}://{}{}{}", scheme.to_ascii_lowercase(), clean_authority, if path.is_empty() { "" } else { "/" }, path)
}

fn canonical_lock_source(project_root: &Path, package_root: &Path, dependency_name: Option<&str>, package_name: &str) -> String {
    if dependency_name.is_none() { return "workspace".into(); }
    let raw = std::fs::read_to_string(project_root.join(crate::Syntax::UNIFIED_LOCK_FILE)).unwrap_or_default();
    let wanted = dependency_name.unwrap_or(package_name);
    let mut current = false;
    let mut name = String::new();
    let mut version = String::new();
    let mut source = String::new();
    let mut locked = String::new();
    let mut content_hash = String::new();
    let mut records = Vec::new();
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        if line.starts_with('[') {
            if current { records.push((std::mem::take(&mut name), std::mem::take(&mut version), std::mem::take(&mut source), std::mem::take(&mut locked), std::mem::take(&mut content_hash))); }
            current = line == "[[package]]";
            continue;
        }
        if !current { continue; }
        if let Some(value) = lock_value(line, "name") { name = value; }
        else if let Some(value) = lock_value(line, "version") { version = value; }
        else if let Some(value) = lock_value(line, "source") { source = value; }
        else if let Some(value) = lock_value(line, "locked") { locked = value; }
        else if let Some(value) = lock_value(line, "content-hash") { content_hash = value; }
    }
    if current { records.push((name, version, source, locked, content_hash)); }
    if let Some((_, locked_version, source, locked, content_hash)) = records.into_iter().find(|(name, ..)| name == wanted || name == package_name) {
        if let Some(path) = inline_lock_value(&source, "path") {
            if let Some(registry) = path.strip_prefix("registry:") {
                return format!("registry:{registry}@{}#{content_hash}", canonical_semver(&locked_version));
            }
            let canonical = if Path::new(&path).is_absolute() {
                wanted.to_string()
            } else {
                path.replace('\\', "/").split('/').filter(|part| !part.is_empty() && *part != ".").collect::<Vec<_>>().join("/")
            };
            let content = if !content_hash.is_empty() { content_hash } else { inline_lock_value(&locked, "tree-hash").unwrap_or_else(|| "unlocked".into()) };
            return format!("path:{canonical}#{content}");
        }
        if let Some(url) = inline_lock_value(&source, "git") {
            let rev = inline_lock_value(&locked, "rev").unwrap_or_default();
            let tree = inline_lock_value(&locked, "tree-hash").unwrap_or(content_hash);
            return format!("git:{}@{rev}#{tree}", credential_free_git_url(&url));
        }
    }
    let relative = package_root.strip_prefix(project_root).ok()
        .and_then(|path| path.to_str()).filter(|path| !path.is_empty())
        .map(|path| path.replace('\\', "/"))
        .unwrap_or_else(|| wanted.to_string());
    format!("path:{relative}")
}

pub(in crate::Sema) fn owning_package<'a>(bundle: &'a ProgramBundle, module_path: &Path) -> (&'a Path, Option<&'a str>) {
    bundle.dep_roots.iter()
        .filter(|(_, root)| module_path.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())
        .map(|(name, root)| (root.as_path(), Some(name.as_str())))
        .unwrap_or((bundle.project_root.as_path(), None))
}

pub(in crate::Sema) fn package_identity(bundle: &ProgramBundle, root: &Path, dependency_name: Option<&str>) -> String {
    let manifest_path = [crate::Syntax::PACKAGE_FILE, crate::Syntax::PAYLOAD_FILE]
        .iter().map(|name| root.join(name)).find(|path| path.is_file())
        .unwrap_or_else(|| root.join(crate::Syntax::PAYLOAD_FILE));
    let manifest = std::fs::read_to_string(manifest_path).unwrap_or_default();
    let name = quoted_field(&manifest, "name").or_else(|| dependency_name.map(str::to_string)).unwrap_or_else(|| "workspace".into());
    let version = canonical_semver(&quoted_field(&manifest, "version").unwrap_or_else(|| "0.0.0+workspace".into()));
    let source = canonical_lock_source(&bundle.project_root, root, dependency_name, &name);
    let mut bytes = Vec::new();
    super::frame_bytes(&mut bytes, b"jet.package.identity.v2");
    frame_text(&mut bytes, &name);
    frame_text(&mut bytes, &version);
    frame_text(&mut bytes, &source);
    crate::SHA256::sha256_hex(&bytes)
}

pub(super) fn instance_identity(key: &ModuleInstanceKey, template: &TemplateInfo, alias: &ModuleAliasDef, source_module: &str, args: &[ResolvedModuleArg]) -> crate::AST::ModuleInstanceIdentity {
    let full_key = key.bytes();
    let fingerprint = crate::SHA256::sha256_hex(&full_key);
    crate::AST::ModuleInstanceIdentity {
        fingerprint: fingerprint.clone(), full_key, definition_id: template.definition_id.clone(),
        argument_keys: key.args.clone(), argument_values: args.iter().map(resolved_argument_value).collect(),
        argument_provenance: alias.args.iter().map(|arg| argument_provenance(arg, template)).collect(),
        template_span: template.def.span,
        applications: vec![crate::AST::ModuleInstanceApplication {
            name: alias.name.clone(), source_module: source_module.to_string(),
            semantic_identity: format!("instance:{fingerprint}"), span: alias.name_span,
        }],
    }
}

fn resolved_argument_value(arg: &ResolvedModuleArg) -> String {
    match arg {
        ResolvedModuleArg::Type(ty) => format!("type:{}", type_name(ty)),
        ResolvedModuleArg::Value(value, _) => format!("value:{}", value.jet_show()),
    }
}

fn argument_provenance(arg: &ModuleArg, template: &TemplateInfo) -> Vec<String> {
    let ModuleArg::Value(expr, _) = arg else { return vec!["type argument".to_string()]; };
    let Some(path) = module_expr_path(expr) else { return vec!["closed value expression".to_string()]; };
    if let Some(name) = path.strip_prefix(crate::Syntax::COMPILER_BUILD_FACT_SETTINGS_PREFIX) {
        let mut sources = template.build_facts.setting_provenance.get(name).cloned().unwrap_or_default();
        sources.push(format!("argument:{path}"));
        return sources;
    }
    vec![format!("argument:{path}")]
}

fn module_expr_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => Some(name.clone()),
        Expr::Field(base, member, _) => Some(format!("{}.{}", module_expr_path(base)?, member)),
        Expr::Paren(inner, _) => module_expr_path(inner),
        _ => None,
    }
}

pub(super) fn register_instance_fingerprint(registry: &mut HashMap<String, Vec<u8>>, identity: &crate::AST::ModuleInstanceIdentity, span: Span) {
    if let Some(previous) = registry.get(&identity.fingerprint) {
        if previous != &identity.full_key {
            let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            jet_foundation::ice!(
                Some(span),
                "E0859 generic module instance fingerprint collision: digest={} first-full-key={} second-full-key={}; compilation stopped before codegen",
                identity.fingerprint, hex(previous), hex(&identity.full_key),
            );
        }
    } else {
        registry.insert(identity.fingerprint.clone(), identity.full_key.clone());
    }
}

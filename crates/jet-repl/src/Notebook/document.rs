//! D-NOTEBOOK-DOC1=D — mergeable `.jetnb` source truth, cache, ipynb, `.jet` export.

use super::trust::{quarantine_outputs, MimeBundle, POLICY_VERSION};
use jet_foundation::JSON::{parse_json, JSONValue};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

pub const OUTPUT_CACHE_POLICY: &str = "closure-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Jet,
    Markdown,
}

#[derive(Clone, Debug)]
pub struct CellOutput {
    pub bundle: MimeBundle,
    pub execution_count: Option<u32>,
    pub cache_key: Option<String>,
    pub turn_id: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct NotebookCell {
    pub id: String,
    pub kind: CellKind,
    pub source: String,
    pub output: Option<CellOutput>,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct OutputCacheEntry {
    pub key: String,
    pub bundle: MimeBundle,
    pub execution_count: u32,
}

#[derive(Clone, Debug)]
pub struct JetNotebook {
    pub schema: u32,
    pub environment_hash: String,
    pub cells: Vec<NotebookCell>,
    pub output_cache: BTreeMap<String, OutputCacheEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LossReport {
    pub items: Vec<String>,
}

impl LossReport {
    pub fn push(&mut self, item: impl Into<String>) {
        self.items.push(item.into());
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return "no loss\n".into();
        }
        let mut out = String::from("export loss:\n");
        for item in &self.items {
            out.push_str("  - ");
            out.push_str(item);
            out.push('\n');
        }
        out
    }
}

impl JetNotebook {
    pub fn new(environment_hash: impl Into<String>) -> Self {
        Self {
            schema: 1,
            environment_hash: environment_hash.into(),
            cells: Vec::new(),
            output_cache: BTreeMap::new(),
        }
    }

    pub fn mint_cell_id() -> String {
        let mut bytes = [0u8; 16];
        fill_csprng(&mut bytes);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        )
    }

    pub fn add_cell(&mut self, kind: CellKind, source: impl Into<String>) -> &mut NotebookCell {
        let id = Self::mint_cell_id();
        self.cells.push(NotebookCell {
            id,
            kind,
            source: source.into(),
            output: None,
            depends_on: Vec::new(),
        });
        self.cells.last_mut().unwrap()
    }

    pub fn paste_cell(&mut self, kind: CellKind, source: impl Into<String>) -> &mut NotebookCell {
        self.add_cell(kind, source)
    }

    pub fn cell_index(&self, id: &str) -> Option<usize> {
        self.cells.iter().position(|c| c.id == id)
    }

    pub fn edit_cell(&mut self, cell_id: &str, source: impl Into<String>) -> Result<(), String> {
        let idx = self
            .cell_index(cell_id)
            .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
        let source = source.into();
        if self.cells[idx].source != source {
            self.cells[idx].source = source;
            self.invalidate_from(cell_id);
        }
        Ok(())
    }

    pub fn invalidate_from(&mut self, cell_id: &str) {
        let Some(start) = self.cell_index(cell_id) else {
            return;
        };
        let mut doomed: BTreeSet<String> = BTreeSet::new();
        doomed.insert(cell_id.to_string());
        for cell in self.cells.iter().skip(start) {
            if cell.depends_on.iter().any(|d| doomed.contains(d)) {
                doomed.insert(cell.id.clone());
            }
            if doomed.contains(&cell.id) {
                if let Some(out) = &cell.output {
                    if let Some(key) = &out.cache_key {
                        self.output_cache.remove(key);
                    }
                }
            }
        }
        for cell in &mut self.cells {
            if doomed.contains(&cell.id) {
                cell.output = None;
            }
        }
    }

    pub fn closure_cache_key(&self, cell_id: &str) -> Option<String> {
        let idx = self.cell_index(cell_id)?;
        let cell = &self.cells[idx];
        if cell.kind != CellKind::Jet {
            return None;
        }
        let mut material = String::new();
        material.push_str(OUTPUT_CACHE_POLICY);
        material.push('\0');
        material.push_str(&self.environment_hash);
        material.push('\0');
        material.push_str(POLICY_VERSION);
        material.push('\0');
        let mut seen = BTreeSet::new();
        fn walk(
            nb: &JetNotebook,
            id: &str,
            seen: &mut BTreeSet<String>,
            material: &mut String,
        ) {
            if !seen.insert(id.to_string()) {
                return;
            }
            let Some(c) = nb.cells.iter().find(|c| c.id == id) else {
                return;
            };
            for dep in &c.depends_on {
                walk(nb, dep, seen, material);
            }
            material.push_str(&c.id);
            material.push('\0');
            material.push_str(&c.source);
            material.push('\0');
        }
        for dep in &cell.depends_on {
            walk(self, dep, &mut seen, &mut material);
        }
        material.push_str(&cell.id);
        material.push('\0');
        material.push_str(&cell.source);
        Some(SHA256::sha256_hex(material.as_bytes()))
    }

    pub fn store_output(
        &mut self,
        cell_id: &str,
        bundle: MimeBundle,
        execution_count: u32,
        turn_id: Option<usize>,
    ) -> Result<(), String> {
        let key = self
            .closure_cache_key(cell_id)
            .ok_or_else(|| format!("cell `{cell_id}` is not a Jet cell"))?;
        let idx = self
            .cell_index(cell_id)
            .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
        self.output_cache.insert(
            key.clone(),
            OutputCacheEntry {
                key: key.clone(),
                bundle: bundle.clone(),
                execution_count,
            },
        );
        self.cells[idx].output = Some(CellOutput {
            bundle,
            execution_count: Some(execution_count),
            cache_key: Some(key),
            turn_id,
        });
        Ok(())
    }

    pub fn visible_output(&self, cell_id: &str) -> Option<&CellOutput> {
        let cell = self.cells.iter().find(|c| c.id == cell_id)?;
        let out = cell.output.as_ref()?;
        let live = self.closure_cache_key(cell_id)?;
        if out.cache_key.as_deref() == Some(live.as_str()) {
            Some(out)
        } else {
            None
        }
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, String> {
        let json = CanonicalJson::parse_canonical(bytes)?;
        notebook_from_json(&json)
    }
}

fn fill_csprng(out: &mut [u8]) {
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    for b in out.iter_mut() {
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = (state ^ (state >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        *b = (z ^ (z >> 31)) as u8;
    }
}

pub fn save_jetnb(nb: &JetNotebook, path: &Path) -> Result<(), String> {
    let json = notebook_to_json(nb)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, json.bytes()).map_err(|e| e.to_string())
}

pub fn load_jetnb(path: &Path) -> Result<JetNotebook, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    JetNotebook::from_canonical_bytes(&bytes)
}

fn plain_bundle(text: impl Into<String>) -> MimeBundle {
    MimeBundle {
        text_plain: text.into(),
        mime: Vec::new(),
        quarantined: false,
        widget_id: None,
        requested_origins: Vec::new(),
        requested_messages: Vec::new(),
    }
}

fn notebook_to_json(nb: &JetNotebook) -> Result<CanonicalJson, String> {
    let mut cells = Vec::new();
    for cell in &nb.cells {
        let kind = match cell.kind {
            CellKind::Jet => "jet",
            CellKind::Markdown => "markdown",
        };
        let mut fields = vec![
            ("id".into(), CanonicalJson::String(cell.id.clone())),
            ("kind".into(), CanonicalJson::String(kind.into())),
            ("source".into(), CanonicalJson::String(cell.source.clone())),
            (
                "depends_on".into(),
                CanonicalJson::Array(
                    cell.depends_on
                        .iter()
                        .cloned()
                        .map(CanonicalJson::String)
                        .collect(),
                ),
            ),
        ];
        if let Some(out) = &cell.output {
            fields.push(("output".into(), cell_output_to_json(out)?));
        }
        cells.push(CanonicalJson::object(fields)?);
    }
    let mut cache = BTreeMap::new();
    for (k, v) in &nb.output_cache {
        cache.insert(
            k.clone(),
            CanonicalJson::object([
                ("key".into(), CanonicalJson::String(v.key.clone())),
                (
                    "execution_count".into(),
                    CanonicalJson::integer(v.execution_count.to_string())?,
                ),
                ("bundle".into(), bundle_to_json(&v.bundle)?),
            ])?,
        );
    }
    CanonicalJson::object([
        ("schema".into(), CanonicalJson::integer(nb.schema.to_string())?),
        ("format".into(), CanonicalJson::String("jetnb".into())),
        (
            "environment_hash".into(),
            CanonicalJson::String(nb.environment_hash.clone()),
        ),
        (
            "cache_policy".into(),
            CanonicalJson::String(OUTPUT_CACHE_POLICY.into()),
        ),
        ("cells".into(), CanonicalJson::Array(cells)),
        ("output_cache".into(), CanonicalJson::Object(cache)),
    ])
}

fn cell_output_to_json(out: &CellOutput) -> Result<CanonicalJson, String> {
    let mut fields = vec![
        ("bundle".into(), bundle_to_json(&out.bundle)?),
        (
            "quarantined".into(),
            CanonicalJson::Bool(out.bundle.quarantined),
        ),
    ];
    if let Some(n) = out.execution_count {
        fields.push((
            "execution_count".into(),
            CanonicalJson::integer(n.to_string())?,
        ));
    }
    if let Some(key) = &out.cache_key {
        fields.push(("cache_key".into(), CanonicalJson::String(key.clone())));
    }
    if let Some(turn_id) = out.turn_id {
        fields.push((
            "turn_id".into(),
            CanonicalJson::integer(turn_id.to_string())?,
        ));
    }
    CanonicalJson::object(fields)
}

fn bundle_to_json(bundle: &MimeBundle) -> Result<CanonicalJson, String> {
    let mime = bundle
        .mime
        .iter()
        .map(|(m, d)| {
            CanonicalJson::object([
                ("mime".into(), CanonicalJson::String(m.clone())),
                ("data".into(), CanonicalJson::String(d.clone())),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    CanonicalJson::object([
        (
            "text_plain".into(),
            CanonicalJson::String(bundle.text_plain.clone()),
        ),
        ("mime".into(), CanonicalJson::Array(mime)),
        ("quarantined".into(), CanonicalJson::Bool(bundle.quarantined)),
        (
            "widget_id".into(),
            match &bundle.widget_id {
                Some(id) => CanonicalJson::String(id.clone()),
                None => CanonicalJson::Null,
            },
        ),
    ])
}

fn notebook_from_json(json: &CanonicalJson) -> Result<JetNotebook, String> {
    let CanonicalJson::Object(root) = json else {
        return Err("jetnb root must be an object".into());
    };
    let environment_hash = text_field(root, "environment_hash")?;
    let schema = match root.get("schema") {
        Some(CanonicalJson::Integer(n)) => n.parse::<u32>().unwrap_or(1),
        _ => 1,
    };
    let mut nb = JetNotebook {
        schema,
        environment_hash,
        cells: Vec::new(),
        output_cache: BTreeMap::new(),
    };
    if let Some(CanonicalJson::Array(cells)) = root.get("cells") {
        for cell in cells {
            nb.cells.push(cell_from_json(cell)?);
        }
    }
    if let Some(CanonicalJson::Object(cache)) = root.get("output_cache") {
        for (k, v) in cache {
            let CanonicalJson::Object(entry) = v else {
                continue;
            };
            let key = text_field(entry, "key").unwrap_or_else(|_| k.clone());
            let execution_count = match entry.get("execution_count") {
                Some(CanonicalJson::Integer(n)) => n.parse().unwrap_or(0),
                _ => 0,
            };
            let bundle = match entry.get("bundle") {
                Some(b) => bundle_from_json(b)?,
                None => plain_bundle(""),
            };
            nb.output_cache.insert(
                k.clone(),
                OutputCacheEntry {
                    key,
                    bundle,
                    execution_count,
                },
            );
        }
    }
    Ok(nb)
}

fn cell_from_json(json: &CanonicalJson) -> Result<NotebookCell, String> {
    let CanonicalJson::Object(obj) = json else {
        return Err("cell must be an object".into());
    };
    let id = text_field(obj, "id")?;
    let kind = match text_field(obj, "kind")?.as_str() {
        "markdown" => CellKind::Markdown,
        _ => CellKind::Jet,
    };
    let source = text_field(obj, "source").unwrap_or_default();
    let depends_on = match obj.get("depends_on") {
        Some(CanonicalJson::Array(arr)) => arr
            .iter()
            .filter_map(|v| match v {
                CanonicalJson::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let output = match obj.get("output") {
        Some(o) => Some(output_from_json(o)?),
        None => None,
    };
    Ok(NotebookCell {
        id,
        kind,
        source,
        output,
        depends_on,
    })
}

fn output_from_json(json: &CanonicalJson) -> Result<CellOutput, String> {
    let CanonicalJson::Object(obj) = json else {
        return Err("output must be an object".into());
    };
    let mut bundle = match obj.get("bundle") {
        Some(b) => bundle_from_json(b)?,
        None => plain_bundle(""),
    };
    if matches!(obj.get("quarantined"), Some(CanonicalJson::Bool(true))) {
        quarantine_outputs(&mut bundle);
    }
    let execution_count = match obj.get("execution_count") {
        Some(CanonicalJson::Integer(n)) => Some(n.parse().unwrap_or(0)),
        _ => None,
    };
    let cache_key = match obj.get("cache_key") {
        Some(CanonicalJson::String(s)) => Some(s.clone()),
        _ => None,
    };
    let turn_id = match obj.get("turn_id") {
        Some(CanonicalJson::Integer(value)) => value.parse().ok(),
        _ => None,
    };
    Ok(CellOutput {
        bundle,
        execution_count,
        cache_key,
        turn_id,
    })
}

fn bundle_from_json(json: &CanonicalJson) -> Result<MimeBundle, String> {
    let CanonicalJson::Object(obj) = json else {
        return Err("bundle must be an object".into());
    };
    let text_plain = text_field(obj, "text_plain").unwrap_or_default();
    let mut mime = Vec::new();
    if let Some(CanonicalJson::Array(arr)) = obj.get("mime") {
        for part in arr {
            let CanonicalJson::Object(p) = part else {
                continue;
            };
            let m = text_field(p, "mime")?;
            let d = text_field(p, "data").unwrap_or_default();
            mime.push((m, d));
        }
    }
    let widget_id = match obj.get("widget_id") {
        Some(CanonicalJson::String(s)) => Some(s.clone()),
        _ => None,
    };
    let quarantined = matches!(obj.get("quarantined"), Some(CanonicalJson::Bool(true)));
    Ok(MimeBundle {
        text_plain,
        mime,
        quarantined,
        widget_id,
        requested_origins: Vec::new(),
        requested_messages: Vec::new(),
    })
}

fn text_field(obj: &BTreeMap<String, CanonicalJson>, key: &str) -> Result<String, String> {
    match obj.get(key) {
        Some(CanonicalJson::String(s)) => Ok(s.clone()),
        _ => Err(format!("missing string field `{key}`")),
    }
}

pub fn merge_by_id(base: &JetNotebook, theirs: &JetNotebook) -> JetNotebook {
    let mut out = JetNotebook::new(base.environment_hash.clone());
    out.schema = base.schema.max(theirs.schema);
    let their_map: BTreeMap<&str, &NotebookCell> =
        theirs.cells.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut seen = BTreeSet::new();
    for cell in &base.cells {
        seen.insert(cell.id.clone());
        if let Some(t) = their_map.get(cell.id.as_str()) {
            let mut merged = (*t).clone();
            let base_key = cell.output.as_ref().and_then(|o| o.cache_key.clone());
            let their_key = t.output.as_ref().and_then(|o| o.cache_key.clone());
            if base_key != their_key {
                merged.output = None;
            }
            out.cells.push(merged);
        } else {
            out.cells.push(cell.clone());
        }
    }
    for cell in &theirs.cells {
        if seen.insert(cell.id.clone()) {
            out.cells.push(cell.clone());
        }
    }
    out.output_cache = base.output_cache.clone();
    for (k, v) in &theirs.output_cache {
        out.output_cache.insert(k.clone(), v.clone());
    }
    out
}

pub fn import_ipynb(text: &str) -> Result<(JetNotebook, LossReport), String> {
    let json = CanonicalJson::parse_canonical(text.as_bytes())
        .or_else(|_| parse_json(text).map(json_value_to_canonical).map_err(|_| {
            "ipynb is not valid JSON; export a complete Jupyter notebook".to_string()
        }))?;
    let CanonicalJson::Object(root) = json else {
        return Err("ipynb root must be an object".into());
    };
    let mut loss = LossReport::default();
    let nb_fmt = match root.get("nbformat") {
        Some(CanonicalJson::Integer(n)) => n.parse::<u32>().unwrap_or(4),
        _ => 4,
    };
    if nb_fmt < 4 {
        loss.push(format!("nbformat {nb_fmt} upgraded to 4.5 semantics"));
    }
    let env = SHA256::sha256_hex(b"ipynb-import");
    let mut nb = JetNotebook::new(env);
    let cells = match root.get("cells") {
        Some(CanonicalJson::Array(c)) => c,
        _ => return Err("ipynb missing cells array".into()),
    };
    for cell in cells {
        let CanonicalJson::Object(obj) = cell else {
            loss.push("skipped non-object cell");
            continue;
        };
        let cell_type = text_field(obj, "cell_type").unwrap_or_else(|_| "code".into());
        let kind = if cell_type == "markdown" {
            CellKind::Markdown
        } else {
            CellKind::Jet
        };
        let source = match obj.get("source") {
            Some(CanonicalJson::String(s)) => s.clone(),
            Some(CanonicalJson::Array(lines)) => lines
                .iter()
                .filter_map(|l| match l {
                    CanonicalJson::String(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        };
        let id = match obj.get("id") {
            Some(CanonicalJson::String(s)) if !s.is_empty() => s.clone(),
            _ => {
                loss.push("minted cell id — source cell lacked nbformat 4.5 id");
                JetNotebook::mint_cell_id()
            }
        };
        let mut out = None;
        if let Some(CanonicalJson::Array(outputs)) = obj.get("outputs") {
            if !outputs.is_empty() {
                loss.push(format!(
                    "cell `{id}`: imported outputs quarantined (no ambient execution)"
                ));
                let text = flatten_ipynb_outputs(outputs);
                let mut bundle = plain_bundle(text);
                quarantine_outputs(&mut bundle);
                out = Some(CellOutput {
                    bundle,
                    execution_count: None,
                    cache_key: None,
                    turn_id: None,
                });
            }
        }
        if obj.get("metadata").is_some() {
            loss.push(format!("cell `{id}`: non-Jet metadata dropped on import"));
        }
        nb.cells.push(NotebookCell {
            id,
            kind,
            source,
            output: out,
            depends_on: Vec::new(),
        });
    }
    Ok((nb, loss))
}

fn json_value_to_canonical(value: JSONValue) -> CanonicalJson {
    match value {
        JSONValue::Null => CanonicalJson::Null,
        JSONValue::Bool(value) => CanonicalJson::Bool(value),
        JSONValue::Number(value) => CanonicalJson::Integer(value.to_string()),
        JSONValue::Flt(value) => CanonicalJson::String(value.to_string()),
        JSONValue::String(value) => CanonicalJson::String(value),
        JSONValue::Array(values) => {
            CanonicalJson::Array(values.into_iter().map(json_value_to_canonical).collect())
        }
        JSONValue::Object(values) => CanonicalJson::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, json_value_to_canonical(value)))
                .collect(),
        ),
    }
}

fn flatten_ipynb_outputs(outputs: &[CanonicalJson]) -> String {
    let mut text = String::new();
    for out in outputs {
        let CanonicalJson::Object(obj) = out else {
            continue;
        };
        if let Some(CanonicalJson::Object(data)) = obj.get("data") {
            if let Some(CanonicalJson::String(s)) = data.get("text/plain") {
                text.push_str(s);
                continue;
            }
            if let Some(CanonicalJson::Array(lines)) = data.get("text/plain") {
                for line in lines {
                    if let CanonicalJson::String(s) = line {
                        text.push_str(s);
                    }
                }
                continue;
            }
        }
        if let Some(CanonicalJson::String(s)) = obj.get("text") {
            text.push_str(s);
        }
    }
    text
}

pub fn export_ipynb(nb: &JetNotebook) -> Result<(String, LossReport), String> {
    let mut loss = LossReport::default();
    loss.push("Jet-specific depends_on / closure cache omitted from ipynb");
    let mut cells = Vec::new();
    for cell in &nb.cells {
        let cell_type = match cell.kind {
            CellKind::Jet => "code",
            CellKind::Markdown => "markdown",
        };
        let mut fields = vec![
            ("id".into(), CanonicalJson::String(cell.id.clone())),
            ("cell_type".into(), CanonicalJson::String(cell_type.into())),
            (
                "source".into(),
                CanonicalJson::Array(vec![CanonicalJson::String(cell.source.clone())]),
            ),
            ("metadata".into(), CanonicalJson::object([])?),
        ];
        if cell.kind == CellKind::Jet {
            let mut outputs = Vec::new();
            match nb.visible_output(&cell.id) {
                Some(out) => {
                    let data = CanonicalJson::object([(
                        "text/plain".into(),
                        CanonicalJson::String(out.bundle.text_plain.clone()),
                    )])?;
                    outputs.push(CanonicalJson::object([
                        (
                            "output_type".into(),
                            CanonicalJson::String("execute_result".into()),
                        ),
                        ("data".into(), data),
                        (
                            "execution_count".into(),
                            match out.execution_count {
                                Some(n) => CanonicalJson::integer(n.to_string())?,
                                None => CanonicalJson::Null,
                            },
                        ),
                        ("metadata".into(), CanonicalJson::object([])?),
                    ])?);
                    if out.bundle.quarantined {
                        loss.push(format!(
                            "cell `{}`: quarantined output exported as text only",
                            cell.id
                        ));
                    }
                }
                None => {
                    if cell.output.is_some() {
                        loss.push(format!(
                            "cell `{}`: stale output omitted (closure key mismatch)",
                            cell.id
                        ));
                    }
                }
            }
            fields.push(("outputs".into(), CanonicalJson::Array(outputs)));
            fields.push(("execution_count".into(), CanonicalJson::Null));
        }
        cells.push(CanonicalJson::object(fields)?);
    }
    let root = CanonicalJson::object([
        ("nbformat".into(), CanonicalJson::integer("4")?),
        ("nbformat_minor".into(), CanonicalJson::integer("5")?),
        (
            "metadata".into(),
            CanonicalJson::object([(
                "kernelspec".into(),
                CanonicalJson::object([
                    ("display_name".into(), CanonicalJson::String("Jet".into())),
                    ("language".into(), CanonicalJson::String("jet".into())),
                    ("name".into(), CanonicalJson::String("jet".into())),
                ])?,
            )])?,
        ),
        ("cells".into(), CanonicalJson::Array(cells)),
    ])?;
    Ok((
        String::from_utf8(root.bytes()).map_err(|e| e.to_string())?,
        loss,
    ))
}

pub fn export_jet(nb: &JetNotebook) -> (String, LossReport) {
    let mut loss = LossReport::default();
    loss.push("markdown cells omitted");
    loss.push("outputs / trust / cell ids omitted");
    loss.push("notebook environment identity omitted");
    let mut body =
        String::from("// generated from .jetnb — stated-loss projection (D-NOTEBOOK-DOC1=D)\n");
    for cell in &nb.cells {
        if cell.kind == CellKind::Jet {
            body.push_str(&cell.source);
            if !cell.source.ends_with('\n') {
                body.push('\n');
            }
            body.push('\n');
        }
    }
    (body, loss)
}

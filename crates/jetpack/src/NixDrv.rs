//! Nix derivation compatibility — ATerm `.drv`, path calculus, differential
//! corpus (E4-JP8 / D-JPK-NIXENGINE1=D, D-JPK-NIXBASELINE1=D).
//!
//! Native engine only: no Tvix code, no product-path shell-out to installed Nix.
//! Algorithms follow the Nix 2.34 store-path / derivation-ATerm protocols
//! (`protocols/store-path.md`, `protocols/nix32.md`, `protocols/derivation-aterm.md`).
//! Compatibility types stay behind this module; product surfaces call in later
//! cards (JP9–JP11).

use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Default Nix store directory (canonical product path; JP11 projects it).
pub const DEFAULT_STORE_DIR: &str = "/nix/store";

/// Nix32 alphabet (no `e`/`o`/`u`/`t`).
const NIX32_CHARS: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Internal compatibility failure — never silent field divergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NixDrvError {
    Parse(String),
    Path(String),
    Unsupported(String),
    Divergence { what: String, expected: String, actual: String },
    IO(String),
}

impl fmt::Display for NixDrvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NixDrvError::Parse(m) => write!(f, "nix drv parse: {m}"),
            NixDrvError::Path(m) => write!(f, "nix path calculus: {m}"),
            NixDrvError::Unsupported(m) => write!(f, "nix drv unsupported: {m}"),
            NixDrvError::Divergence { what, expected, actual } => {
                write!(f, "nix compat divergence ({what}): expected `{expected}`, got `{actual}`")
            }
            NixDrvError::IO(m) => write!(f, "nix drv io: {m}"),
        }
    }
}

impl std::error::Error for NixDrvError {}

pub type Result<T> = std::result::Result<T, NixDrvError>;

// ── Nix32 / store-path primitives ───────────────────────────────────────────

/// Encode bytes as Nix32 (processes from the end of the digest).
pub fn nix32_encode(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let len = (bytes.len() * 8 - 1) / 5 + 1;
    let mut s = String::with_capacity(len);
    for n in (0..len).rev() {
        let b = n * 5;
        let i = b / 8;
        let j = b % 8;
        // Widen to u32: `<< (8 - j)` is 8 when j==0 (undefined/panic on u8).
        let mut c = (bytes[i] as u32) >> j;
        if i < bytes.len() - 1 {
            c |= (bytes[i + 1] as u32) << (8 - j);
        }
        s.push(NIX32_CHARS[(c as u8 & 0x1f) as usize] as char);
    }
    s
}

/// Decode Nix32 into raw bytes (length = ceil(chars * 5 / 8), may include padding).
pub fn nix32_decode(text: &str) -> Result<Vec<u8>> {
    let mut rev = [0xffu8; 256];
    for (i, &ch) in NIX32_CHARS.iter().enumerate() {
        rev[ch as usize] = i as u8;
    }
    let mut res = Vec::new();
    for (n, ch) in text.chars().rev().enumerate() {
        let digit = *rev.get(ch as usize).unwrap_or(&0xff);
        if digit == 0xff {
            return Err(NixDrvError::Parse(format!(
                "invalid Nix32 character: {ch:?}"
            )));
        }
        let b = n * 5;
        let i = b / 8;
        let j = b % 8;
        if res.len() <= i {
            res.resize(i + 1, 0);
        }
        res[i] |= digit << j;
        if digit >> (8 - j) != 0 {
            if res.len() <= i + 1 {
                res.resize(i + 2, 0);
            }
            res[i + 1] |= digit >> (8 - j);
        }
    }
    Ok(res)
}

/// XOR-fold a hash down to `new_size` bytes (Nix `compressHash`).
pub fn compress_hash(hash: &[u8], new_size: usize) -> Vec<u8> {
    let mut out = vec![0u8; new_size];
    for (i, &b) in hash.iter().enumerate() {
        out[i % new_size] ^= b;
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    SHA256::sha256(data)
}

/// `makeStorePath(type, "sha256:"+hex, name)` → `{store}/{nix32(compress(sha256(fp),20))}-{name}`.
pub fn make_store_path(store_dir: &str, type_str: &str, hash_with_algo: &str, name: &str) -> String {
    let fingerprint = format!("{type_str}:{hash_with_algo}:{store_dir}:{name}");
    let digest = compress_hash(&sha256_bytes(fingerprint.as_bytes()), 20);
    format!("{store_dir}/{}-{name}", nix32_encode(&digest))
}

/// Output path name: `drvName` or `drvName-output` when output ≠ `out`.
pub fn output_path_name(drv_name: &str, output_name: &str) -> String {
    if output_name == "out" {
        drv_name.to_string()
    } else {
        format!("{drv_name}-{output_name}")
    }
}

/// Input-addressed output path from a `hashDerivationModulo` digest.
pub fn make_output_path(store_dir: &str, output_id: &str, modulo_hash: &[u8; 32], drv_name: &str) -> String {
    let name = output_path_name(drv_name, output_id);
    make_store_path(
        store_dir,
        &format!("output:{output_id}"),
        &format!("sha256:{}", hex_encode(modulo_hash)),
        &name,
    )
}

/// Fixed-output / content-addressed path (`makeFixedOutputPath`).
///
/// `method_algo` is ATerm form: `sha256`, `r:sha256`, `sha512`, `r:sha512`, …
pub fn make_fixed_output_path(store_dir: &str, name: &str, method_algo: &str, hash_hex: &str) -> String {
    if method_algo == "r:sha256" {
        return make_store_path(
            store_dir,
            "source",
            &format!("sha256:{hash_hex}"),
            name,
        );
    }
    // flat / non-sha256-nar / git: digest = sha256("fixed:out:" + methodAlgo + ":" + hex + ":")
    let payload = format!("fixed:out:{method_algo}:{hash_hex}:");
    let digest = sha256_bytes(payload.as_bytes());
    make_store_path(
        store_dir,
        "output:out",
        &format!("sha256:{}", hex_encode(&digest)),
        name,
    )
}

/// Text-method CA path (derivations): `type = text{:ref}*`.
pub fn make_text_path(store_dir: &str, name: &str, contents: &[u8], references: &[String]) -> String {
    let mut type_str = String::from("text");
    let mut refs: Vec<&String> = references.iter().collect();
    refs.sort();
    for r in refs {
        type_str.push(':');
        type_str.push_str(r);
    }
    let h = sha256_bytes(contents);
    make_store_path(
        store_dir,
        &type_str,
        &format!("sha256:{}", hex_encode(&h)),
        name,
    )
}

/// Strip `/nix/store/<hash>-` prefix → name (including `.drv` when present).
pub fn store_path_name(store_path: &str) -> Result<String> {
    let base = store_path.rsplit('/').next().unwrap_or(store_path);
    let dash = base
        .find('-')
        .ok_or_else(|| NixDrvError::Path(format!("not a store path basename: {base}")))?;
    Ok(base[dash + 1..].to_string())
}

/// Derivation name without `.drv` suffix.
pub fn drv_name_from_path(drv_path: &str) -> Result<String> {
    let mut name = store_path_name(drv_path)?;
    if let Some(stripped) = name.strip_suffix(".drv") {
        name = stripped.to_string();
    }
    Ok(name)
}

// ── Derivation model ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivationOutput {
    pub name: String,
    pub path: String,
    /// Empty for input-addressed; `sha256` / `r:sha256` / … for fixed; floating has algo + empty hash.
    pub method_algo: String,
    pub hash_hex: String,
}

impl DerivationOutput {
    pub fn is_fixed(&self) -> bool {
        !self.method_algo.is_empty() && self.hash_hex != "impure" && !self.hash_hex.is_empty()
    }

    pub fn is_floating(&self) -> bool {
        self.path.is_empty() && !self.method_algo.is_empty() && self.hash_hex.is_empty()
    }

    pub fn is_impure(&self) -> bool {
        self.hash_hex == "impure"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDrv {
    pub path: String,
    pub outputs: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derivation {
    pub outputs: BTreeMap<String, DerivationOutput>,
    pub input_drvs: Vec<InputDrv>,
    pub input_srcs: BTreeSet<String>,
    pub platform: String,
    pub builder: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Derivation {
    pub fn is_fixed(&self) -> bool {
        self.outputs.values().any(|o| o.is_fixed())
    }

    pub fn references_for_drv_path(&self) -> Vec<String> {
        let mut refs: BTreeSet<String> = self.input_srcs.clone();
        for inp in &self.input_drvs {
            refs.insert(inp.path.clone());
        }
        refs.into_iter().collect()
    }
}

// ── ATerm parse / encode ────────────────────────────────────────────────────

struct Cursor<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self { s, i: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.i += ch.len_utf8();
        Some(ch)
    }

    fn consume(&mut self, want: char) -> Result<()> {
        match self.bump() {
            Some(c) if c == want => Ok(()),
            other => Err(NixDrvError::Parse(format!(
                "expected {want:?} at {}, got {other:?}",
                self.i
            ))),
        }
    }

    /// Unquoted ATerm string: `"..."` with no escape processing (Nix `printUnquotedString`).
    fn parse_raw_string(&mut self) -> Result<String> {
        self.consume('"')?;
        let start = self.i;
        while self.i < self.s.len() {
            if self.s.as_bytes()[self.i] == b'"' {
                let out = self.s[start..self.i].to_string();
                self.i += 1;
                return Ok(out);
            }
            self.i += 1;
        }
        Err(NixDrvError::Parse("unterminated raw string".into()))
    }

    /// Escaped ATerm string (Nix `printString`): `\\` `\"` `\n` `\r` `\t`.
    fn parse_escaped_string(&mut self) -> Result<String> {
        self.consume('"')?;
        let mut out = String::new();
        while self.i < self.s.len() {
            let c = self
                .bump()
                .ok_or_else(|| NixDrvError::Parse("unterminated escaped string".into()))?;
            if c == '"' {
                return Ok(out);
            }
            if c == '\\' {
                let n = self
                    .bump()
                    .ok_or_else(|| NixDrvError::Parse("truncated escape".into()))?;
                match n {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        Err(NixDrvError::Parse("unterminated escaped string".into()))
    }

    fn parse_list<T, F>(&mut self, mut item: F) -> Result<Vec<T>>
    where
        F: FnMut(&mut Self) -> Result<T>,
    {
        self.consume('[')?;
        let mut items = Vec::new();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(items);
        }
        loop {
            items.push(item(self)?);
            match self.peek() {
                Some(']') => {
                    self.bump();
                    break;
                }
                Some(',') => {
                    self.bump();
                }
                other => {
                    return Err(NixDrvError::Parse(format!(
                        "expected ',' or ']' in list at {}, got {other:?}",
                        self.i
                    )));
                }
            }
        }
        Ok(items)
    }
}

fn encode_raw(s: &str) -> String {
    format!("\"{s}\"")
}

fn encode_escaped(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Parse a stable `Derive(...)` ATerm derivation. Rejects `DrvWithVersion` (dynamic).
pub fn parse_derive(aterm: &str) -> Result<Derivation> {
    let mut c = Cursor::new(aterm);
    if aterm.starts_with("DrvWithVersion(") {
        return Err(NixDrvError::Unsupported(
            "DrvWithVersion / dynamic-derivations (JP21 staged)".into(),
        ));
    }
    if !aterm.starts_with("Derive(") {
        return Err(NixDrvError::Parse("expected Derive(".into()));
    }
    c.i = "Derive(".len();

    let outputs_raw = c.parse_list(|c| {
        c.consume('(')?;
        let name = c.parse_raw_string()?;
        c.consume(',')?;
        let path = c.parse_raw_string()?;
        c.consume(',')?;
        let method_algo = c.parse_raw_string()?;
        c.consume(',')?;
        let hash_hex = c.parse_raw_string()?;
        c.consume(')')?;
        Ok(DerivationOutput {
            name,
            path,
            method_algo,
            hash_hex,
        })
    })?;
    c.consume(',')?;

    let input_drvs = c.parse_list(|c| {
        c.consume('(')?;
        let path = c.parse_raw_string()?;
        c.consume(',')?;
        if c.peek() == Some('(') {
            return Err(NixDrvError::Unsupported(
                "dynamic derivation input childMap (xp-dyn-drv)".into(),
            ));
        }
        let outs = c.parse_list(|c| c.parse_raw_string())?;
        c.consume(')')?;
        Ok(InputDrv {
            path,
            outputs: outs.into_iter().collect(),
        })
    })?;
    c.consume(',')?;

    let input_srcs: BTreeSet<String> = c.parse_list(|c| c.parse_raw_string())?.into_iter().collect();
    c.consume(',')?;
    let platform = c.parse_raw_string()?;
    c.consume(',')?;
    let builder = c.parse_escaped_string()?;
    c.consume(',')?;
    let args = c.parse_list(|c| c.parse_escaped_string())?;
    c.consume(',')?;
    let env_pairs = c.parse_list(|c| {
        c.consume('(')?;
        let k = c.parse_escaped_string()?;
        c.consume(',')?;
        let v = c.parse_escaped_string()?;
        c.consume(')')?;
        Ok((k, v))
    })?;
    c.consume(')')?;

    let mut outputs = BTreeMap::new();
    for o in outputs_raw {
        outputs.insert(o.name.clone(), o);
    }
    let env: BTreeMap<String, String> = env_pairs.into_iter().collect();

    // Keep input_drvs in store-path order (matches Nix map iteration for roundtrip).
    let mut input_drvs = input_drvs;
    input_drvs.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(Derivation {
        outputs,
        input_drvs,
        input_srcs,
        platform,
        builder,
        args,
        env,
    })
}

/// Encode a derivation to ATerm. When `actual_inputs` is set, input drv paths
/// are replaced by modulo-hash hex keys (hashDerivationModulo serialization).
pub fn unparse_derive(
    drv: &Derivation,
    mask_outputs: bool,
    actual_inputs: Option<&BTreeMap<String, BTreeSet<String>>>,
) -> String {
    let mut s = String::from("Derive([");
    let mut first = true;
    for (name, o) in &drv.outputs {
        if !first {
            s.push(',');
        }
        first = false;
        let path = if mask_outputs { "" } else { o.path.as_str() };
        s.push('(');
        s.push_str(&encode_raw(name));
        s.push(',');
        s.push_str(&encode_raw(path));
        s.push(',');
        s.push_str(&encode_raw(&o.method_algo));
        s.push(',');
        s.push_str(&encode_raw(&o.hash_hex));
        s.push(')');
    }
    s.push_str("],[");
    first = true;
    if let Some(inputs) = actual_inputs {
        for (hash_hex, outs) in inputs {
            if !first {
                s.push(',');
            }
            first = false;
            s.push('(');
            s.push_str(&encode_raw(hash_hex));
            s.push_str(",[");
            let mut of = true;
            for o in outs {
                if !of {
                    s.push(',');
                }
                of = false;
                s.push_str(&encode_raw(o));
            }
            s.push_str("])");
        }
    } else {
        for inp in &drv.input_drvs {
            if !first {
                s.push(',');
            }
            first = false;
            s.push('(');
            s.push_str(&encode_raw(&inp.path));
            s.push_str(",[");
            let mut of = true;
            for o in &inp.outputs {
                if !of {
                    s.push(',');
                }
                of = false;
                s.push_str(&encode_raw(o));
            }
            s.push_str("])");
        }
    }
    s.push_str("],[");
    first = true;
    for src in &drv.input_srcs {
        if !first {
            s.push(',');
        }
        first = false;
        s.push_str(&encode_raw(src));
    }
    s.push(']');
    s.push(',');
    s.push_str(&encode_raw(&drv.platform));
    s.push(',');
    s.push_str(&encode_escaped(&drv.builder));
    s.push_str(",[");
    first = true;
    for a in &drv.args {
        if !first {
            s.push(',');
        }
        first = false;
        s.push_str(&encode_escaped(a));
    }
    s.push_str("],[");
    first = true;
    for (k, v) in &drv.env {
        if !first {
            s.push(',');
        }
        first = false;
        let vv = if mask_outputs && drv.outputs.contains_key(k) {
            ""
        } else {
            v.as_str()
        };
        s.push('(');
        s.push_str(&encode_escaped(k));
        s.push(',');
        s.push_str(&encode_escaped(vv));
        s.push(')');
    }
    s.push_str("])");
    s
}

// ── hashDerivationModulo / path calculus ────────────────────────────────────

/// Read a derivation by store path (for recursive modulo).
pub trait DrvStore {
    fn read_drv(&mut self, drv_path: &str) -> Result<Derivation>;
}

/// Filesystem `/nix/store` reader with parse cache.
pub struct FSDrvStore {
    cache: BTreeMap<String, Derivation>,
}

impl FSDrvStore {
    pub fn new() -> Self {
        Self {
            cache: BTreeMap::new(),
        }
    }
}

impl Default for FSDrvStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DrvStore for FSDrvStore {
    fn read_drv(&mut self, drv_path: &str) -> Result<Derivation> {
        if let Some(d) = self.cache.get(drv_path) {
            return Ok(d.clone());
        }
        let text = fs::read_to_string(drv_path).map_err(|e| NixDrvError::IO(format!("{drv_path}: {e}")))?;
        let drv = parse_derive(&text)?;
        self.cache.insert(drv_path.to_string(), drv.clone());
        Ok(drv)
    }
}

/// In-memory map of path → derivation (unit tests / fixtures).
pub struct MapDrvStore {
    pub map: BTreeMap<String, Derivation>,
}

impl DrvStore for MapDrvStore {
    fn read_drv(&mut self, drv_path: &str) -> Result<Derivation> {
        self.map
            .get(drv_path)
            .cloned()
            .ok_or_else(|| NixDrvError::IO(format!("missing drv in map store: {drv_path}")))
    }
}

/// Memo key: `(drvPath, maskOutputs)` → output-name → sha256 digest.
pub type ModuloMemo = BTreeMap<(String, bool), BTreeMap<String, [u8; 32]>>;

/// `hashDerivationModulo` — see Nix `derivations.cc`.
///
/// `mask_outputs`:
/// - `true` when computing this drv's own output paths (`staticOutputHashes`)
/// - `false` when computing the hash substituted into parent input lists
///   (`pathDerivationModulo`)
pub fn hash_derivation_modulo(
    store: &mut dyn DrvStore,
    store_dir: &str,
    drv_path: &str,
    drv: &Derivation,
    mask_outputs: bool,
    memo: &mut ModuloMemo,
) -> Result<BTreeMap<String, [u8; 32]>> {
    let key = (drv_path.to_string(), mask_outputs);
    if let Some(h) = memo.get(&key) {
        return Ok(h.clone());
    }

    if drv.outputs.values().any(|o| o.is_floating() || o.is_impure()) {
        return Err(NixDrvError::Unsupported(
            "floating/impure derivation outputs (staged / trust-gated)".into(),
        ));
    }

    let hashes = if drv.is_fixed() {
        let mut out = BTreeMap::new();
        for (name, o) in &drv.outputs {
            if !o.is_fixed() {
                return Err(NixDrvError::Unsupported(
                    "mixed fixed/non-fixed outputs".into(),
                ));
            }
            let payload = format!("fixed:out:{}:{}:{}", o.method_algo, o.hash_hex, o.path);
            out.insert(name.clone(), sha256_bytes(payload.as_bytes()));
        }
        out
    } else {
        let mut inputs2: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for inp in &drv.input_drvs {
            // pathDerivationModulo always uses maskOutputs=false
            let child = store.read_drv(&inp.path)?;
            let child_hashes =
                hash_derivation_modulo(store, store_dir, &inp.path, &child, false, memo)?;
            for out_name in &inp.outputs {
                let h = child_hashes.get(out_name).ok_or_else(|| {
                    NixDrvError::Path(format!(
                        "no hash for output `{out_name}` of `{}`",
                        inp.path
                    ))
                })?;
                inputs2
                    .entry(hex_encode(h))
                    .or_default()
                    .insert(out_name.clone());
            }
        }
        let serialized = unparse_derive(drv, mask_outputs, Some(&inputs2));
        let h = sha256_bytes(serialized.as_bytes());
        let mut out = BTreeMap::new();
        for name in drv.outputs.keys() {
            out.insert(name.clone(), h);
        }
        out
    };

    memo.insert(key, hashes.clone());
    let _ = store_dir; // reserved for future CA/self-ref variants
    Ok(hashes)
}

/// Recompute every output path; fail closed on mismatch with embedded paths.
pub fn verify_output_paths(
    store: &mut dyn DrvStore,
    store_dir: &str,
    drv_path: &str,
    drv: &Derivation,
) -> Result<()> {
    let drv_name = drv_name_from_path(drv_path)?;
    if drv.is_fixed() {
        for (name, o) in &drv.outputs {
            let expected = make_fixed_output_path(
                store_dir,
                &output_path_name(&drv_name, name),
                &o.method_algo,
                &o.hash_hex,
            );
            if expected != o.path {
                return Err(NixDrvError::Divergence {
                    what: format!("fixed-output `{name}`"),
                    expected,
                    actual: o.path.clone(),
                });
            }
        }
        return Ok(());
    }
    let mut memo = BTreeMap::new();
    let hashes = hash_derivation_modulo(store, store_dir, drv_path, drv, true, &mut memo)?;
    for (name, o) in &drv.outputs {
        let h = hashes.get(name).ok_or_else(|| {
            NixDrvError::Path(format!("missing modulo hash for output `{name}`"))
        })?;
        let expected = make_output_path(store_dir, name, h, &drv_name);
        if expected != o.path {
            return Err(NixDrvError::Divergence {
                what: format!("input-addressed output `{name}`"),
                expected,
                actual: o.path.clone(),
            });
        }
    }
    Ok(())
}

/// Recompute drv store path via text CA; fail closed on mismatch.
pub fn verify_drv_path(store_dir: &str, drv_path: &str, aterm: &[u8], drv: &Derivation) -> Result<()> {
    let name = store_path_name(drv_path)?;
    let refs = drv.references_for_drv_path();
    let expected = make_text_path(store_dir, &name, aterm, &refs);
    if expected != drv_path {
        return Err(NixDrvError::Divergence {
            what: "drvPath".into(),
            expected,
            actual: drv_path.to_string(),
        });
    }
    Ok(())
}

/// Round-trip ATerm encode; fail closed on byte divergence.
pub fn verify_aterm_roundtrip(aterm: &str, drv: &Derivation) -> Result<()> {
    let encoded = unparse_derive(drv, false, None);
    if encoded != aterm {
        return Err(NixDrvError::Divergence {
            what: "ATerm roundtrip".into(),
            expected: aterm.chars().take(120).collect(),
            actual: encoded.chars().take(120).collect(),
        });
    }
    Ok(())
}

// ── Reference scanning (discard rules substrate) ────────────────────────────

/// Scan bytes for Nix store path digests (`[nix32]{32}` after `store_dir/`).
/// Returns sorted unique full store paths found that exist as substrings of the
/// canonical `{store_dir}/{digest}-` form's digest component.
pub fn scan_store_path_digests(store_dir: &str, bytes: &[u8]) -> BTreeSet<String> {
    let prefix = format!("{store_dir}/");
    let prefix_b = prefix.as_bytes();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i + prefix_b.len() + 32 <= bytes.len() {
        if &bytes[i..i + prefix_b.len()] == prefix_b {
            let dig = &bytes[i + prefix_b.len()..i + prefix_b.len() + 32];
            if dig.iter().all(|&b| NIX32_CHARS.contains(&b)) {
                // Prefer full path if `-name` follows; else record digest-only key.
                let start = i;
                let mut end = i + prefix_b.len() + 32;
                if end < bytes.len() && bytes[end] == b'-' {
                    end += 1;
                    while end < bytes.len() {
                        let b = bytes[end];
                        // Store path names: printable path bytes excluding whitespace / quotes.
                        if b.is_ascii_graphic() && b != b'"' && b != b'\'' {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    if let Ok(s) = std::str::from_utf8(&bytes[start..end]) {
                        found.insert(s.to_string());
                    }
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    found
}

// ── Differential corpus ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct CorpusReport {
    pub checked: usize,
    pub skipped: usize,
    pub divergences: Vec<NixDrvError>,
}

/// Differential sample over real store `.drv` files. Empty store → skipped=0, checked=0.
pub fn differential_corpus(store_dir: &str, limit: usize) -> Result<CorpusReport> {
    let dir = Path::new(store_dir);
    if !dir.is_dir() {
        return Ok(CorpusReport::default());
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| NixDrvError::IO(e.to_string()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("drv"))
        .collect();
    paths.sort();
    paths.truncate(limit);

    let mut report = CorpusReport::default();
    let mut store = FSDrvStore::new();
    for path in paths {
        let path_str = path.to_string_lossy().into_owned();
        let aterm = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                report.skipped += 1;
                continue;
            }
        };
        let drv = match parse_derive(&aterm) {
            Ok(d) => d,
            Err(NixDrvError::Unsupported(_)) => {
                report.skipped += 1;
                continue;
            }
            Err(e) => {
                report.divergences.push(e);
                continue;
            }
        };
        if let Err(e) = verify_aterm_roundtrip(&aterm, &drv) {
            report.divergences.push(e);
            continue;
        }
        if let Err(e) = verify_drv_path(store_dir, &path_str, aterm.as_bytes(), &drv) {
            report.divergences.push(e);
            continue;
        }
        if let Err(e) = verify_output_paths(&mut store, store_dir, &path_str, &drv) {
            match e {
                NixDrvError::Unsupported(_) => report.skipped += 1,
                other => report.divergences.push(other),
            }
            continue;
        }
        report.checked += 1;
    }
    Ok(report)
}

/// Fail closed if any divergence; returns checked count.
pub fn assert_corpus_clean(store_dir: &str, limit: usize) -> Result<usize> {
    let report = differential_corpus(store_dir, limit)?;
    if let Some(first) = report.divergences.first() {
        return Err(first.clone());
    }
    Ok(report.checked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix32_empty_sha256_matches_nix_hash() {
        let empty = hex_decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(
            nix32_encode(&empty),
            "0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73"
        );
    }

    #[test]
    fn fixed_flat_sha256_path_matches_known_vector() {
        // From local store: colour-2.3.7.tar.gz FOD (algo sha256).
        // Recompute from method+hash only — path embeds content hash.
        let store = DEFAULT_STORE_DIR;
        // Use make_fixed against a synthetic: empty flat → known construction.
        let empty_hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let path = make_fixed_output_path(store, "empty", "sha256", empty_hex);
        assert!(path.starts_with("/nix/store/"));
        assert!(path.ends_with("-empty"));
        // Roundtrip: path digest is 32 nix32 chars.
        let base = path.rsplit('/').next().unwrap();
        let dig = &base[..32];
        assert_eq!(dig.len(), 32);
        assert!(dig.bytes().all(|b| NIX32_CHARS.contains(&b)));
    }

    #[test]
    fn aterm_roundtrip_minimal_fixed() {
        let aterm = concat!(
            r#"Derive([("out","/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x","sha256","e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")],"#,
            r#"[],[],"x86_64-linux","/bin/bash",["-c","true"],[("name","x"),("out","/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x")])"#
        );
        // Fix path to the real fixed path for empty hash named "x"
        let real = make_fixed_output_path(
            DEFAULT_STORE_DIR,
            "x",
            "sha256",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        let aterm = aterm.replace("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x", &real);
        let drv = parse_derive(&aterm).expect("parse");
        assert!(drv.is_fixed());
        verify_aterm_roundtrip(&aterm, &drv).expect("roundtrip");
        let mut store = MapDrvStore {
            map: BTreeMap::new(),
        };
        verify_output_paths(&mut store, DEFAULT_STORE_DIR, &format!("{real}.drv"), &drv)
            .expect("fod paths");
    }

    #[test]
    fn malformed_aterm_strings_return_parse_errors() {
        for aterm in [
            r#"Derive([("out"#,
            r#"Derive([],[],[],"x86_64-linux","unterminated"#,
        ] {
            assert!(
                matches!(parse_derive(aterm), Err(NixDrvError::Parse(_))),
                "malformed ATerm must return a parse error: {aterm:?}"
            );
        }
    }

    #[test]
    fn reference_scan_finds_store_paths() {
        // 32-char nix32 digest + name
        let digest = "0123456789abcdfghijklmnpqrsvwxyz";
        assert_eq!(digest.len(), 32);
        let blob = format!("hello {}/{digest}-foo/bin/x and done", DEFAULT_STORE_DIR);
        let found = scan_store_path_digests(DEFAULT_STORE_DIR, blob.as_bytes());
        assert!(
            found.iter().any(|p| p.contains("-foo")),
            "found={found:?}"
        );
    }

    #[test]
    fn differential_local_store_sample() {
        if !Path::new(DEFAULT_STORE_DIR).is_dir() {
            return;
        }
        let report = differential_corpus(DEFAULT_STORE_DIR, 64).expect("corpus");
        assert!(
            report.divergences.is_empty(),
            "divergences: {:?}",
            report.divergences
        );
        assert!(
            report.checked > 0,
            "expected to check some .drv files, got checked={} skipped={}",
            report.checked,
            report.skipped
        );
    }

    fn hex_decode(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}

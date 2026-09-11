//! Typed, provenance-checked model outputs carried by the ordinary Package model.
//!
//! A model is not a second package kind or registry.  It is an `outputs:` entry
//! whose kind is `.Model`; this module only turns that checked output payload
//! into typed signatures, identities, limits, and a provider seam.  Providers
//! remain execution adapters.  They cannot change package validation or fetch
//! undeclared bytes.

use crate::Diagnostics::Diagnostic;
use crate::DataTree::DataTree;
use crate::SHA256;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::future::Future;
use std::path::{Component, Path};
use std::sync::Arc;

pub mod provider;
pub use provider::*;

const SIGNATURE_DIAGNOSTIC: &str = "E-MODEL-SIGNATURE";
const PROVENANCE_DIAGNOSTIC: &str = "E-MODEL-PROVENANCE";
const LOAD_DIAGNOSTIC: &str = "E-MODEL-LOAD";

/// Tensor element formats accepted at a model boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TensorDType {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F16,
    BF16,
    F32,
    F64,
}

impl TensorDType {
    /// Parse the one canonical lower-case spelling used in a `.Model` output.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "bool" => Some(Self::Bool),
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "f16" => Some(Self::F16),
            "bf16" => Some(Self::BF16),
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F16 => "f16",
            Self::BF16 => "bf16",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

impl fmt::Display for TensorDType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A static tensor dimension or a bounded symbolic dimension.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TensorDimension {
    Static(u64),
    Dynamic { name: String, min: u64, max: u64 },
}

impl TensorDimension {
    fn accepts(&self, value: u64) -> bool {
        match self {
            Self::Static(expected) => *expected == value,
            Self::Dynamic { min, max, .. } => (*min..=*max).contains(&value),
        }
    }
}

impl fmt::Display for TensorDimension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(value) => value.fmt(formatter),
            Self::Dynamic { name, min, max } => write!(formatter, "{name}[{min}..{max}]"),
        }
    }
}

/// A tensor shape.  Dynamic axes always carry explicit inclusive bounds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TensorShape {
    pub dimensions: Vec<TensorDimension>,
}

impl TensorShape {
    /// Construct a runtime shape whose dimensions are all concrete.
    pub fn runtime<I>(dimensions: I) -> Self
    where
        I: IntoIterator<Item = u64>,
    {
        Self {
            dimensions: dimensions.into_iter().map(TensorDimension::Static).collect(),
        }
    }

    /// Return whether a concrete runtime shape satisfies this declaration.
    pub fn accepts(&self, actual: &TensorShape) -> bool {
        self.dimensions.len() == actual.dimensions.len()
            && self
                .dimensions
                .iter()
                .zip(&actual.dimensions)
                .all(|(expected, actual)| {
                    matches!(actual, TensorDimension::Static(value) if expected.accepts(*value))
                })
    }
}

impl fmt::Display for TensorShape {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[")?;
        for (index, dimension) in self.dimensions.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            dimension.fmt(formatter)?;
        }
        formatter.write_str("]")
    }
}

/// One named input or output tensor at a checked package boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TensorSpec {
    pub name: String,
    pub dtype: TensorDType,
    pub shape: TensorShape,
}

impl TensorSpec {
    /// Construct a concrete runtime tensor signature.
    pub fn runtime(name: impl Into<String>, dtype: TensorDType, shape: impl IntoIterator<Item = u64>) -> Self {
        Self {
            name: name.into(),
            dtype,
            shape: TensorShape::runtime(shape),
        }
    }
}

impl fmt::Display for TensorSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}{}", self.name, self.dtype, self.shape)
    }
}

/// A local package artifact whose bytes are pinned by a lowercase SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelArtifact {
    pub path: String,
    pub sha256: String,
}

/// Full embedding-space identity.  Hashes and semantic choices are all part
/// of this value, so equal dimensions alone can never make an index reusable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelIdentity {
    pub package: String,
    pub package_version: String,
    pub license: String,
    pub graph_sha256: String,
    pub weights_sha256: String,
    pub tokenizer_sha256: String,
    pub adapter_sha256: Option<String>,
    pub preprocessing: String,
    pub pooling: String,
    pub normalization: String,
    pub output_meaning: String,
    pub metric: String,
}

impl ModelIdentity {
    /// Stable identity digest used by embedding indexes.
    ///
    /// Fields are length-delimited before hashing.  Delimiters alone would
    /// make package-controlled metadata ambiguous (`a;b` versus `a` + `b`),
    /// which would let two different embedding spaces share an index tag.
    pub fn digest(&self) -> String {
        let fields = [
            ("package", self.package.as_str()),
            ("version", self.package_version.as_str()),
            ("license", self.license.as_str()),
            ("graph", self.graph_sha256.as_str()),
            ("weights", self.weights_sha256.as_str()),
            ("tokenizer", self.tokenizer_sha256.as_str()),
            ("adapter", self.adapter_sha256.as_deref().unwrap_or("-")),
            ("preprocessing", self.preprocessing.as_str()),
            ("pooling", self.pooling.as_str()),
            ("normalization", self.normalization.as_str()),
            ("output", self.output_meaning.as_str()),
            ("metric", self.metric.as_str()),
        ];
        let mut canonical = String::new();
        for (label, value) in fields {
            let _ = write!(canonical, "{label}:{}:", value.len());
            canonical.push_str(value);
            canonical.push(';');
        }
        SHA256::sha256_hex(canonical.as_bytes())
    }
}

/// Execution contract for a model package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContract {
    pub inputs: Vec<TensorSpec>,
    pub outputs: Vec<TensorSpec>,
    pub provider: String,
    pub custom_operators: bool,
    pub max_context: Option<u64>,
    pub max_batch: Option<u64>,
    pub max_buffer_bytes: Option<u64>,
}

/// The source-owned document-to-vector contract bound to one `.Model` output.
///
/// The compiler supplies this record after resolving ordinary exported Jet
/// declarations.  The model package does not invent a universal `Embedder`
/// type: trait and carrier names come from the package source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSignature {
    pub trait_name: String,
    pub batch_type: String,
    pub space_type: String,
    pub method_name: String,
    pub document_type: String,
    pub result_type: String,
    pub error_type: String,
}

impl ModelSignature {
    /// Build the canonical document embedding method contract for exported
    /// source declarations with package-selected trait and carrier names.
    pub fn document_embedding(
        trait_name: impl Into<String>,
        batch_type: impl Into<String>,
        space_type: impl Into<String>,
    ) -> Self {
        let batch_type = batch_type.into();
        Self {
            trait_name: trait_name.into(),
            batch_type: batch_type.clone(),
            space_type: space_type.into(),
            method_name: "embed".into(),
            document_type: "[String]".into(),
            result_type: batch_type,
            error_type: "Err".into(),
        }
    }

    fn validate(&self, package: &str) -> Result<(), ModelError> {
        for (field, value) in [
            ("trait", self.trait_name.as_str()),
            ("batch carrier", self.batch_type.as_str()),
            ("space carrier", self.space_type.as_str()),
        ] {
            if !valid_qualified_identifier(value) {
                return Err(ModelError::declaration(
                    package,
                    format!("model signature {field} `{value}` is not an exported type name"),
                ));
            }
        }
        if self.method_name != "embed"
            || self.document_type != "[String]"
            || self.result_type != self.batch_type
            || self.error_type != "Err"
        {
            return Err(ModelError::declaration(
                package,
                "model signature must declare `embed(self, documents: [String]) Batch !Err`",
            ));
        }
        Ok(())
    }
}

fn valid_qualified_identifier(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(valid_identifier)
}

/// Export facts collected from one package source module before a model is
/// opened.  Resolution is intentionally explicit so missing, ambiguous, and
/// conflicting declarations fail before provider execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelExportSet {
    pub traits: Vec<ModelSignature>,
    pub batch_types: Vec<String>,
    pub space_types: Vec<String>,
}

/// A resolved source signature and its checked package identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBinding {
    pub output: String,
    pub signature: ModelSignature,
    pub identity: EmbeddingIndexIdentity,
}

/// A model package's runtime descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub package: String,
    pub output: String,
    /// The `.Model` `name` field.  It selects the source trait that binds this
    /// output; it is not part of embedding-space identity.
    pub signature_name: Option<String>,
    pub package_version: String,
    pub license: String,
    pub graph: ModelArtifact,
    pub weights: ModelArtifact,
    pub tokenizer: ModelArtifact,
    pub adapter: Option<ModelArtifact>,
    pub preprocessing: String,
    pub pooling: String,
    pub normalization: String,
    pub output_meaning: String,
    pub metric: String,
    pub contract: ModelContract,
}

/// A typed embedding-space provenance carrier.  Fields are private so callers
/// cannot relabel raw vectors as another model or metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingSpace {
    model_digest: String,
    dimension: u64,
    metric: String,
    normalization: String,
}

impl EmbeddingSpace {
    pub fn model_digest(&self) -> &str {
        &self.model_digest
    }

    pub fn dimension(&self) -> u64 {
        self.dimension
    }

    pub fn metric(&self) -> &str {
        &self.metric
    }

    pub fn normalization(&self) -> &str {
        &self.normalization
    }
}

/// A provider-produced batch of document vectors.  Only this crate's checked
/// provider boundary can construct it; safe consumers can inspect values and
/// identity but cannot retag either.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingBatch {
    values: Vec<Vec<f32>>,
    space: EmbeddingSpace,
}

impl EmbeddingBatch {
    pub fn values(&self) -> &[Vec<f32>] {
        &self.values
    }

    pub fn space(&self) -> &EmbeddingSpace {
        &self.space
    }
}

/// One document's tokenized BERT input.  It remains an ordinary input witness
/// until the provider checks and executes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedDocument {
    pub input_ids: Vec<i64>,
    pub attention_mask: Vec<i64>,
    pub token_type_ids: Vec<i64>,
}

/// Identity recorded by an embedding index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EmbeddingIndexIdentity {
    pub model_digest: String,
    pub dimension: u64,
    pub metric: String,
    pub normalization: String,
}

impl fmt::Display for EmbeddingIndexIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "model={} dimension={} metric={} normalization={}",
            self.model_digest, self.dimension, self.metric, self.normalization
        )
    }
}

/// A fully checked model output selected from host package facts or a runtime
/// descriptor.
///
/// The graph and embedded-weight roles may intentionally share one path when
/// both role hashes are identical; all other artifact paths remain unique.
#[derive(Debug, Clone)]
pub struct ModelPackage {
    pub package: String,
    pub output: String,
    /// The source trait name configured by the `.Model` `name` field.
    pub signature_name: Option<String>,
    pub identity: ModelIdentity,
    pub contract: ModelContract,
    pub artifacts: Vec<ModelArtifact>,
}
/// One exact-search result from a model-bound index.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingHit {
    pub index: usize,
    pub score: f32,
}

/// An exact embedding index whose identity is fixed at construction.
///
/// There is no insertion API for raw arrays: every value must carry the
/// provider-created space witness and match this index's canonical identity.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingIndex {
    identity: EmbeddingIndexIdentity,
    values: Vec<Vec<f32>>,
}

impl EmbeddingIndex {
    pub fn new(package: &ModelPackage) -> Result<Self, ModelError> {
        let identity = package.embedding_index_identity()?;
        if !matches!(identity.metric.as_str(), "cosine" | "dot") {
            return Err(ModelError::declaration(
                &package.package,
                format!("unsupported embedding metric `{}`", identity.metric),
            ));
        }
        Ok(Self {
            identity,
            values: Vec::new(),
        })
    }

    pub fn identity(&self) -> &EmbeddingIndexIdentity {
        &self.identity
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn insert(&mut self, batch: &EmbeddingBatch) -> Result<(), ModelError> {
        self.check_space(batch.space())?;
        let expected = self.identity.dimension as usize;
        for value in batch.values() {
            if value.len() != expected || value.iter().any(|item| !item.is_finite()) {
                return Err(ModelError::signature(
                    "<embedding-index>",
                    format!("{} finite values", expected),
                    format!("{} values", value.len()),
                ));
            }
            self.values.push(value.clone());
        }
        Ok(())
    }

    pub fn nearest(
        &self,
        query: &EmbeddingBatch,
        count: usize,
    ) -> Result<Vec<EmbeddingHit>, ModelError> {
        self.check_space(query.space())?;
        if query.values().len() != 1 {
            return Err(ModelError::declaration(
                "<embedding-index>",
                "nearest requires exactly one query vector",
            ));
        }
        let Some(query) = query.values().first() else {
            return Err(ModelError::declaration(
                "<embedding-index>",
                "nearest requires one query vector",
            ));
        };
        if query.len() != self.identity.dimension as usize
            || query.iter().any(|item| !item.is_finite())
        {
            return Err(ModelError::signature(
                "<embedding-index>",
                format!("{} finite query values", self.identity.dimension),
                format!("{} values", query.len()),
            ));
        }
        let mut hits = self
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| EmbeddingHit {
                index,
                score: self.score(value, query),
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.index.cmp(&right.index))
        });
        hits.truncate(count);
        Ok(hits)
    }

    fn check_space(&self, space: &EmbeddingSpace) -> Result<(), ModelError> {
        let actual = EmbeddingIndexIdentity {
            model_digest: space.model_digest.clone(),
            dimension: space.dimension,
            metric: space.metric.clone(),
            normalization: space.normalization.clone(),
        };
        if actual == self.identity {
            Ok(())
        } else {
            Err(ModelError::signature(
                "<embedding-index>",
                self.identity.to_string(),
                actual.to_string(),
            ))
        }
    }

    fn score(&self, left: &[f32], right: &[f32]) -> f32 {
        match self.identity.metric.as_str() {
            "cosine" | "dot" => left.iter().zip(right).map(|(a, b)| a * b).sum(),
            _ => 0.0,
        }
    }
}
/// A BERT WordPiece tokenizer decoded from the package's pinned tokenizer
/// artifact.  It is deliberately kept beside the model contract so the
/// provider cannot silently pair a different tokenizer with the graph.
pub struct BertWordPieceTokenizer {
    vocabulary: BTreeMap<String, i64>,
    unknown_id: i64,
    cls_id: i64,
    sep_id: i64,
    pad_id: i64,
    lowercase: bool,
    max_input_chars_per_word: usize,
    sequence_length: usize,
    max_batch: Option<u64>,
}

impl fmt::Debug for BertWordPieceTokenizer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BertWordPieceTokenizer")
            .field("vocabulary_size", &self.vocabulary.len())
            .field("lowercase", &self.lowercase)
            .field("max_input_chars_per_word", &self.max_input_chars_per_word)
            .field("sequence_length", &self.sequence_length)
            .field("max_batch", &self.max_batch)
            .finish()
    }
}

/// One tokenized batch, preserving one fixed sequence length for every
/// document so the provider's dynamic-shape witness is unambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedBatch {
    pub documents: Vec<EncodedDocument>,
    pub sequence_length: usize,
}

impl BertWordPieceTokenizer {
    /// Decode the Hugging Face `tokenizer.json` WordPiece form and check that
    /// its special-token and normalization choices agree with the package.
    pub fn from_json(package: &ModelPackage, bytes: &[u8]) -> Result<Self, ModelError> {
        package.check_buffer(bytes.len() as u64)?;
        let text = std::str::from_utf8(bytes).map_err(|_| {
            ModelError::provenance(&package.package, "tokenizer artifact is not UTF-8 JSON")
        })?;
        let root = crate::JSON::parse(text).map_err(|error| {
            ModelError::provenance(
                &package.package,
                format!("tokenizer JSON is invalid: {error}"),
            )
        })?;
        let model = root.get("model").map_err(|error| {
            ModelError::provenance(&package.package, format!("tokenizer JSON {error}"))
        })?;
        let model_type = model
            .get("type")
            .and_then(DataTree::as_str)
            .map_err(|error| {
                ModelError::provenance(&package.package, format!("tokenizer model {error}"))
            })?;
        if model_type != "WordPiece" {
            return Err(ModelError::declaration(
                &package.package,
                format!("tokenizer model `{model_type}` is not WordPiece"),
            ));
        }
        let normalizer = root.get("normalizer").map_err(|error| {
            ModelError::provenance(&package.package, format!("tokenizer JSON {error}"))
        })?;
        let lowercase = normalizer
            .get_opt("lowercase")
            .and_then(|value| match value {
                DataTree::Bool(value) => Some(*value),
                _ => None,
            })
            .unwrap_or(false);
        if package.identity.preprocessing.to_ascii_lowercase().contains("lowercase") && !lowercase {
            return Err(ModelError::signature(
                &package.package,
                "lowercase tokenizer normalization",
                "tokenizer does not lowercase",
            ));
        }
        let vocabulary = model
            .get("vocab")
            .and_then(DataTree::as_object)
            .map_err(|error| {
                ModelError::provenance(&package.package, format!("tokenizer vocabulary {error}"))
            })?
            .iter()
            .map(|(token, value)| {
                let id = tokenizer_integer(value).ok_or_else(|| {
                    ModelError::provenance(
                        &package.package,
                        format!("tokenizer vocabulary id for `{token}` is not a non-negative integer"),
                    )
                })?;
                Ok((token.clone(), id))
            })
            .collect::<Result<BTreeMap<_, _>, ModelError>>()?;
        if vocabulary.is_empty() {
            return Err(ModelError::declaration(
                &package.package,
                "tokenizer vocabulary is empty",
            ));
        }
        let unknown_token = model
            .get_opt("unk_token")
            .and_then(|value| value.as_str().ok())
            .unwrap_or("[UNK]");
        let unknown_id = vocabulary.get(unknown_token).copied().ok_or_else(|| {
            ModelError::declaration(
                &package.package,
                format!("tokenizer vocabulary has no unknown token `{unknown_token}`"),
            )
        })?;
        let cls_id = vocabulary.get("[CLS]").copied().ok_or_else(|| {
            ModelError::declaration(&package.package, "tokenizer vocabulary has no `[CLS]` token")
        })?;
        let sep_id = vocabulary.get("[SEP]").copied().ok_or_else(|| {
            ModelError::declaration(&package.package, "tokenizer vocabulary has no `[SEP]` token")
        })?;
        let pad_id = vocabulary.get("[PAD]").copied().ok_or_else(|| {
            ModelError::declaration(&package.package, "tokenizer vocabulary has no `[PAD]` token")
        })?;
        let max_input_chars_per_word = model
            .get_opt("max_input_chars_per_word")
            .and_then(tokenizer_integer)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(100);
        let sequence_length = package
            .contract
            .max_context
            .or_else(|| {
                package
                    .contract
                    .inputs
                    .first()
                    .and_then(|tensor| tensor.shape.dimensions.get(1))
                    .map(|dimension| match dimension {
                        TensorDimension::Static(value) => *value,
                        TensorDimension::Dynamic { max, .. } => *max,
                    })
            })
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| {
                ModelError::declaration(
                    &package.package,
                    "tokenizer needs a bounded sequence-length contract",
                )
            })?;
        if sequence_length < 2 || max_input_chars_per_word == 0 {
            return Err(ModelError::declaration(
                &package.package,
                "tokenizer sequence and word bounds must be positive",
            ));
        }
        Ok(Self {
            vocabulary,
            unknown_id,
            cls_id,
            sep_id,
            pad_id,
            lowercase,
            max_input_chars_per_word,
            sequence_length,
            max_batch: package.contract.max_batch,
        })
    }

    pub fn sequence_length(&self) -> usize {
        self.sequence_length
    }

    pub fn encode_documents(
        &self,
        package: &ModelPackage,
        documents: &[String],
    ) -> Result<EncodedBatch, ModelError> {
        if documents.is_empty() {
            return Err(ModelError::declaration(
                &package.package,
                "document embedding requires at least one document",
            ));
        }
        if self
            .max_batch
            .is_some_and(|max| documents.len() as u64 > max)
        {
            return Err(ModelError::ResourceLimit {
                package: package.package.clone(),
                reason: format!("batch {} exceeds declared limit", documents.len()),
            });
        }
        let documents = documents
            .iter()
            .map(|document| self.encode(document))
            .collect::<Vec<_>>();
        Ok(EncodedBatch {
            documents,
            sequence_length: self.sequence_length,
        })
    }

    fn encode(&self, text: &str) -> EncodedDocument {
        let mut input_ids = vec![self.cls_id];
        for word in self.words(text) {
            for id in self.wordpiece(&word) {
                if input_ids.len() + 1 >= self.sequence_length {
                    break;
                }
                input_ids.push(id);
            }
            if input_ids.len() + 1 >= self.sequence_length {
                break;
            }
        }
        input_ids.push(self.sep_id);
        let mut attention_mask = vec![1; input_ids.len()];
        input_ids.resize(self.sequence_length, self.pad_id);
        attention_mask.resize(self.sequence_length, 0);
        EncodedDocument {
            input_ids,
            attention_mask,
            token_type_ids: vec![0; self.sequence_length],
        }
    }

    fn words(&self, text: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word = String::new();
        let flush = |word: &mut String, words: &mut Vec<String>| {
            if !word.is_empty() {
                words.push(std::mem::take(word));
            }
        };
        for character in text.chars() {
            if character == '\0' || character == '\u{fffd}' || character.is_control() {
                continue;
            }
            let character = if character.is_whitespace() {
                ' '
            } else if self.lowercase {
                character.to_lowercase().next().unwrap_or(character)
            } else {
                character
            };
            if character == ' ' {
                flush(&mut word, &mut words);
            } else if character.is_ascii_punctuation()
                || (!character.is_alphanumeric() && !character.is_whitespace())
            {
                flush(&mut word, &mut words);
                words.push(character.to_string());
            } else {
                word.push(character);
            }
        }
        flush(&mut word, &mut words);
        words
    }

    fn wordpiece(&self, word: &str) -> Vec<i64> {
        let characters = word.chars().collect::<Vec<_>>();
        if characters.is_empty() || characters.len() > self.max_input_chars_per_word {
            return vec![self.unknown_id];
        }
        let mut pieces = Vec::new();
        let mut start = 0;
        while start < characters.len() {
            let mut end = characters.len();
            let mut found = None;
            while start < end {
                let text = characters[start..end].iter().collect::<String>();
                let token = if start == 0 {
                    text
                } else {
                    format!("##{text}")
                };
                if let Some(id) = self.vocabulary.get(&token) {
                    found = Some(*id);
                    break;
                }
                end -= 1;
            }
            let Some(id) = found else {
                return vec![self.unknown_id];
            };
            pieces.push(id);
            start = end;
        }
        pieces
    }
}

fn tokenizer_integer(value: &DataTree) -> Option<i64> {
    match value {
        DataTree::Int(value) if *value >= 0 => Some(*value),
        DataTree::Number(value) => value.parse::<i64>().ok().filter(|value| *value >= 0),
        _ => None,
    }
}
/// Artifact transport for a filesystem root or an authenticated offline byte
/// map. Package validation checks the same declared artifact closure in either
/// case; browser hosts use `Bytes` because they do not expose a filesystem.
#[derive(Clone, Copy)]
pub enum ModelSource<'a> {
    Directory(&'a Path),
    Bytes(&'a BTreeMap<String, Vec<u8>>),
}

impl<'a> From<&'a Path> for ModelSource<'a> {
    fn from(root: &'a Path) -> Self {
        Self::Directory(root)
    }
}

impl<'a> From<&'a std::path::PathBuf> for ModelSource<'a> {
    fn from(root: &'a std::path::PathBuf) -> Self {
        Self::Directory(root)
    }
}

/// Only ModelPackage can construct this verified, owned artifact closure.
pub struct VerifiedModel<'a> {
    pub(crate) package: &'a ModelPackage,
    pub(crate) artifacts: Vec<Arc<Vec<u8>>>,
}

/// Providers consume checked bytes, never paths or download authorities.
/// Opening may suspend while a browser initializes its foreign runtime.
pub trait ModelProvider {
    type Session;

    fn open(
        &self,
        model: VerifiedModel<'_>,
        cancellation: &provider::CancellationToken,
    ) -> impl Future<Output = Result<Self::Session, ModelError>>;
}

impl ModelPackage {
    /// Construct one model package from canonical facts emitted by a host
    /// package parser.  Identity is derived here rather than accepted from
    /// the transport, and the same artifact/contract validation runs for
    /// native and browser consumers.
    pub fn from_descriptor(descriptor: ModelDescriptor) -> Result<Self, ModelError> {
        let ModelDescriptor {
            package,
            output,
            signature_name,
            package_version,
            license,
            graph,
            weights,
            tokenizer,
            adapter,
            preprocessing,
            pooling,
            normalization,
            output_meaning,
            metric,
            contract,
        } = descriptor;
        if let Some(name) = signature_name.as_deref() {
            validate_descriptor_text(&package, "name", name)?;
            if !valid_qualified_identifier(name) {
                return Err(ModelError::declaration(
                    &package,
                    format!("model signature name `{name}` is not an exported type name"),
                ));
            }
        }
        for (field, value) in [
            ("package", package.as_str()),
            ("output", output.as_str()),
            ("version", package_version.as_str()),
            ("license", license.as_str()),
            ("preprocessing", preprocessing.as_str()),
            ("pooling", pooling.as_str()),
            ("normalization", normalization.as_str()),
            ("output_meaning", output_meaning.as_str()),
            ("metric", metric.as_str()),
            ("provider", contract.provider.as_str()),
        ] {
            validate_descriptor_text(&package, field, value)?;
        }
        for (role, artifact) in [
            ("graph", &graph),
            ("weights", &weights),
            ("tokenizer", &tokenizer),
        ] {
            validate_descriptor_artifact(&package, role, artifact)?;
        }
        if let Some(adapter) = &adapter {
            validate_descriptor_artifact(&package, "adapter", adapter)?;
        }
        let mut artifacts = vec![graph.clone(), weights.clone(), tokenizer.clone()];
        if let Some(adapter) = &adapter {
            artifacts.push(adapter.clone());
        }
        let package_value = Self {
            package: package.clone(),
            output,
            signature_name,
            identity: ModelIdentity {
                package,
                package_version,
                license,
                graph_sha256: graph.sha256.clone(),
                weights_sha256: weights.sha256.clone(),
                tokenizer_sha256: tokenizer.sha256.clone(),
                adapter_sha256: adapter.as_ref().map(|artifact| artifact.sha256.clone()),
                preprocessing,
                pooling,

                normalization,
                output_meaning,
                metric,
            },
            contract,
            artifacts,
        };
        validate_runtime_contract(&package_value.package, &package_value.contract)?;
        validate_artifact_layout(&package_value)?;
        Ok(package_value)
    }
    /// Resolve the package-selected source trait and its referenced carrier
    /// types before any provider is opened.
    pub fn bind_source_exports(
        &self,
        exports: &ModelExportSet,
    ) -> Result<ModelBinding, ModelError> {
        let configured = self.signature_name.as_deref().ok_or_else(|| {
            ModelError::declaration(
                &self.package,
                format!(
                    "model output `{}` does not configure a source signature name",
                    self.output
                ),
            )
        })?;
        let matching = exports
            .traits
            .iter()
            .filter(|signature| signature.trait_name == configured)
            .collect::<Vec<_>>();
        let signature = match matching.as_slice() {
            [] => {
                return Err(ModelError::declaration(
                    &self.package,
                    format!("model signature trait `{configured}` is not exported"),
                ))
            }
            [signature] => (*signature).clone(),
            _ => {
                return Err(ModelError::declaration(
                    &self.package,
                    format!("model signature trait `{configured}` is exported more than once"),
                ))
            }
        };
        signature.validate(&self.package)?;
        let batch_count = exports
            .batch_types
            .iter()
            .filter(|name| *name == &signature.batch_type)
            .count();
        if batch_count == 0 {
            return Err(ModelError::declaration(
                &self.package,
                format!("model signature batch carrier `{}` is not exported", signature.batch_type),
            ));
        }
        if batch_count != 1 {
            return Err(ModelError::declaration(
                &self.package,
                format!(
                    "model signature batch carrier `{}` is exported more than once",
                    signature.batch_type
                ),
            ));
        }
        let space_count = exports
            .space_types
            .iter()
            .filter(|name| *name == &signature.space_type)
            .count();
        if space_count == 0 {
            return Err(ModelError::declaration(
                &self.package,
                format!("model signature space carrier `{}` is not exported", signature.space_type),
            ));
        }
        if space_count != 1 {
            return Err(ModelError::declaration(
                &self.package,
                format!(
                    "model signature space carrier `{}` is exported more than once",
                    signature.space_type
                ),
            ));
        }
        Ok(ModelBinding {
            output: self.output.clone(),
            signature,
            identity: self.embedding_index_identity()?,
        })
    }
    /// Pool and normalize one checked provider output into the private
    /// embedding carrier.  This constructor is crate-visible only so native
    /// and web adapters share exactly one provenance check.
    pub(crate) fn embedding_batch_from_output(
        &self,
        encoded: &EncodedBatch,
        output: &TensorSpec,
        bytes: &[u8],
    ) -> Result<EmbeddingBatch, ModelError> {
        self.check_outputs(std::slice::from_ref(output))?;
        if output.dtype != TensorDType::F32 || output.shape.dimensions.len() != 3 {
            return Err(ModelError::declaration(
                &self.package,
                "document embedding requires one rank-3 f32 hidden-state output",
            ));
        }
        let dimensions = output
            .shape
            .dimensions
            .iter()
            .map(|dimension| match dimension {
                TensorDimension::Static(value) => Ok(*value),
                TensorDimension::Dynamic { .. } => Err(ModelError::signature(
                    &self.package,
                    "concrete output dimensions",
                    output.to_string(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let [batch_value, tokens_value, dimension_value] = dimensions.as_slice() else {
            return Err(ModelError::declaration(
                &self.package,
                "document embedding requires a rank-3 hidden-state output",
            ));
        };
        let batch = usize::try_from(*batch_value).map_err(|_| {
            ModelError::declaration(&self.package, "model output batch exceeds host size")
        })?;
        let tokens = usize::try_from(*tokens_value).map_err(|_| {
            ModelError::declaration(&self.package, "model output context exceeds host size")
        })?;
        let dimension = usize::try_from(*dimension_value).map_err(|_| {
            ModelError::declaration(&self.package, "model output dimension exceeds host size")
        })?;
        if batch != encoded.documents.len()
            || encoded.documents.iter().any(|document| {
                document.attention_mask.len() != tokens
                    || document.input_ids.len() != tokens
                    || document.token_type_ids.len() != tokens
            })
        {
            return Err(ModelError::signature(
                &self.package,
                format!("hidden state shape [{}, {}, {}]", encoded.documents.len(), encoded.sequence_length, dimension),
                format!("hidden state shape [{batch}, {tokens}, {dimension}]"),
            ));
        }
        let expected_bytes = batch
            .checked_mul(tokens)
            .and_then(|value| value.checked_mul(dimension))
            .and_then(|value| value.checked_mul(std::mem::size_of::<f32>()))
            .ok_or_else(|| ModelError::BufferLimit {
                package: self.package.clone(),
                bytes: u64::MAX,
            })?;
        if bytes.len() != expected_bytes {
            return Err(ModelError::signature(
                &self.package,
                format!("{expected_bytes} output bytes"),
                format!("{} output bytes", bytes.len()),
            ));
        }
        if self.identity.pooling != "masked_mean(last_hidden_state, attention_mask)" {
            return Err(ModelError::declaration(
                &self.package,
                format!("unsupported document pooling `{}`", self.identity.pooling),
            ));
        }
        if self.identity.normalization != "l2" {
            return Err(ModelError::declaration(
                &self.package,
                format!("unsupported document normalization `{}`", self.identity.normalization),
            ));
        }
        let mut values = Vec::with_capacity(batch);
        for (batch_index, document) in encoded.documents.iter().enumerate() {
            let active = document
                .attention_mask
                .iter()
                .enumerate()
                .filter_map(|(token_index, mask)| (*mask > 0).then_some(token_index))
                .collect::<Vec<_>>();
            if active.is_empty() {
                return Err(ModelError::declaration(
                    &self.package,
                    "document attention mask has no active tokens",
                ));
            }
            let mut pooled = vec![0.0f32; dimension];
            for token_index in active.iter().copied() {
                let row_start = (batch_index * tokens + token_index) * dimension;
                for (column, slot) in pooled.iter_mut().enumerate() {
                    let offset = (row_start + column) * std::mem::size_of::<f32>();
                    let value_bytes: [u8; 4] = bytes[offset..offset + 4]
                        .try_into()
                        .expect("checked f32 output width");
                    let sample = f32::from_le_bytes(value_bytes);
                    if !sample.is_finite() {
                        return Err(ModelError::declaration(
                            &self.package,
                            "model output contains a non-finite hidden-state value",
                        ));
                    }
                    *slot += sample;
                }
            }
            let count = active.len() as f32;
            for value in &mut pooled {
                *value /= count;
            }
            let norm = pooled
                .iter()
                .map(|value| f64::from(*value) * f64::from(*value))
                .sum::<f64>()
                .sqrt();
            if !norm.is_finite() || norm == 0.0 {
                return Err(ModelError::declaration(
                    &self.package,
                    "model output has a zero or non-finite l2 norm",
                ));
            }
            let norm = norm as f32;
            for value in &mut pooled {
                *value /= norm;
            }
            values.push(pooled);
        }
        let identity = self.embedding_index_identity()?;
        Ok(EmbeddingBatch {
            values,
            space: EmbeddingSpace {
                model_digest: identity.model_digest,
                dimension: identity.dimension,
                metric: identity.metric,
                normalization: identity.normalization,
            },
        })
    }


    /// Export the validated runtime facts without a precomputed identity
    /// digest.  Hosts use this descriptor when transferring the same contract
    /// to a runtime that does not compile package parsing.
    pub fn descriptor(&self) -> Result<ModelDescriptor, ModelError> {
        validate_runtime_contract(&self.package, &self.contract)?;
        validate_artifact_layout(self)?;
        let (Some(graph), Some(weights), Some(tokenizer)) = (
            self.artifacts.first(),
            self.artifacts.get(1),
            self.artifacts.get(2),
        ) else {
            return Err(ModelError::provenance(
                &self.package,
                "model package has no canonical graph, weights, and tokenizer roles",
            ));
        };
        Ok(ModelDescriptor {
            package: self.package.clone(),
            output: self.output.clone(),
            signature_name: self.signature_name.clone(),
            package_version: self.identity.package_version.clone(),
            license: self.identity.license.clone(),
            graph: graph.clone(),
            weights: weights.clone(),
            tokenizer: tokenizer.clone(),
            adapter: self.artifacts.get(3).cloned(),
            preprocessing: self.identity.preprocessing.clone(),
            pooling: self.identity.pooling.clone(),
            normalization: self.identity.normalization.clone(),
            output_meaning: self.identity.output_meaning.clone(),
            metric: self.identity.metric.clone(),
            contract: self.contract.clone(),
        })
    }

    /// Open this package with one provider and one offline artifact source.
    pub async fn open_with<'a, P: ModelProvider>(
        &self,
        source: impl Into<ModelSource<'a>>,
        provider: &P,
        cancellation: &provider::CancellationToken,
    ) -> Result<P::Session, ModelError> {
        let source = source.into();
        validate_artifact_layout(self)?;
        if let ModelSource::Bytes(files) = source {
            let declared_paths = self
                .artifacts
                .iter()
                .map(|artifact| artifact.path.as_str())
                .collect::<BTreeSet<_>>();
            if files.len() != declared_paths.len()
                || files.keys().any(|path| !declared_paths.contains(path.as_str()))
            {
                return Err(ModelError::provenance(
                    &self.package,
                    "supplied model bytes do not match the declared artifact closure",
                ));
            }
        }
        let mut loaded = BTreeMap::<String, Arc<Vec<u8>>>::new();
        let mut artifacts = Vec::with_capacity(self.artifacts.len());
        for artifact in &self.artifacts {
            let bytes = if let Some(bytes) = loaded.get(&artifact.path) {
                bytes.clone()
            } else {
                let bytes = match source {
                    ModelSource::Directory(root) => self.read_artifact(root, artifact)?,
                    ModelSource::Bytes(files) => {
                        let bytes = files.get(&artifact.path).ok_or_else(|| ModelError::Artifact {
                            package: self.package.clone(),
                            path: artifact.path.clone(),
                            reason: "declared model bytes are absent".into(),
                        })?;
                        self.check_artifact_bytes(artifact, bytes)?;
                        bytes.clone()
                    }
                };
                let bytes = Arc::new(bytes);
                loaded.insert(artifact.path.clone(), bytes.clone());
                bytes
            };
            artifacts.push(bytes);
        }
        provider
            .open(VerifiedModel { package: self, artifacts }, cancellation)
            .await
    }
    /// Verify all graph, weights, tokenizer, and optional adapter bytes.
    pub fn verify_artifacts(&self, root: &Path) -> Result<(), ModelError> {
        validate_artifact_layout(self)?;
        let mut checked_paths = BTreeSet::new();
        for artifact in &self.artifacts {
            if checked_paths.insert(artifact.path.as_str()) {
                self.read_artifact(root, artifact)?;
            }
        }
        Ok(())
    }

    /// Read one already-declared artifact without following symlinks and verify
    /// it against the package's exact hash.
    pub fn read_artifact(
        &self,
        root: &Path,
        artifact: &ModelArtifact,
    ) -> Result<Vec<u8>, ModelError> {
        if !self.artifacts.iter().any(|declared| declared == artifact) {
            return Err(ModelError::provenance(
                &self.package,
                format!("artifact `{}` is not declared by this model package", artifact.path),
            ));
        }
        let path = root.join(&artifact.path);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            ModelError::artifact(&self.package, &artifact.path, error.to_string())
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ModelError::artifact(
                &self.package,
                &artifact.path,
                "artifact is not a regular file",
            ));
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            ModelError::artifact(&self.package, &artifact.path, error.to_string())
        })?;
        self.check_artifact_bytes(artifact, &bytes)?;
        Ok(bytes)
    }

    fn check_artifact_bytes(&self, artifact: &ModelArtifact, bytes: &[u8]) -> Result<(), ModelError> {
        let actual = SHA256::sha256_hex(bytes);
        if actual != artifact.sha256 {
            return Err(ModelError::provenance(
                &self.package,
                format!(
                    "artifact `{}` hash is `{actual}`, expected `{}`",
                    artifact.path, artifact.sha256
                ),
            ));
        }
        Ok(())
    }

    /// Check all input tensor names, dtypes, ranks, and bounded dimensions.
    pub fn check_inputs(&self, actual: &[TensorSpec]) -> Result<(), ModelError> {
        check_signatures(&self.package, &self.contract.inputs, actual)
    }

    /// Check all output tensor names, dtypes, ranks, and bounded dimensions.
    pub fn check_outputs(&self, actual: &[TensorSpec]) -> Result<(), ModelError> {
        check_signatures(&self.package, &self.contract.outputs, actual)
    }

    /// Build the exact identity an embedding index must record.
    pub fn embedding_index_identity(&self) -> Result<EmbeddingIndexIdentity, ModelError> {
        if self.contract.outputs.len() != 1 {
            return Err(ModelError::declaration(
                &self.package,
                "embedding identity requires exactly one output tensor",
            ));
        }
        let output = &self.contract.outputs[0];
        let Some(TensorDimension::Static(dimension)) = output.shape.dimensions.last() else {
            return Err(ModelError::declaration(
                &self.package,
                "embedding output's final dimension must be static",
            ));
        };
        Ok(EmbeddingIndexIdentity {
            model_digest: self.identity.digest(),
            dimension: *dimension,
            metric: self.identity.metric.clone(),
            normalization: self.identity.normalization.clone(),
        })
    }

    /// Reject index reuse unless the complete model/space identity matches.
    pub fn check_index_compatibility(
        &self,
        index: &EmbeddingIndexIdentity,
    ) -> Result<(), ModelError> {
        let expected = self.embedding_index_identity()?;
        if expected == *index {
            Ok(())
        } else {
            Err(ModelError::signature(
                &self.package,
                expected.to_string(),
                index.to_string(),
            ))
        }
    }

    /// Reject a provider that did not declare the exact package provider.
    pub fn require_provider(&self, provider: &str) -> Result<(), ModelError> {
        if self.contract.provider == provider {
            Ok(())
        } else {
            Err(ModelError::UnsupportedProvider {
                package: self.package.clone(),
                expected: self.contract.provider.clone(),
                actual: provider.to_string(),
            })
        }
    }

    /// Enforce a declared context bound before execution.
    pub fn check_context(&self, context: u64) -> Result<(), ModelError> {
        if self.contract.max_context.is_some_and(|max| context > max) {
            return Err(ModelError::ResourceLimit {
                package: self.package.clone(),
                reason: format!("context {context} exceeds declared limit"),
            });
        }
        Ok(())
    }

    /// Enforce a declared batch bound before execution.
    pub fn check_batch(&self, batch: u64) -> Result<(), ModelError> {
        if self.contract.max_batch.is_some_and(|max| batch > max) {
            return Err(ModelError::ResourceLimit {
                package: self.package.clone(),
                reason: format!("batch {batch} exceeds declared limit"),
            });
        }
        Ok(())
    }

    /// Enforce a declared byte-buffer bound before execution or transfer.
    pub fn check_buffer(&self, bytes: u64) -> Result<(), ModelError> {
        if self
            .contract
            .max_buffer_bytes
            .is_some_and(|max| bytes > max)
        {
            return Err(ModelError::BufferLimit {
                package: self.package.clone(),
                bytes,
            });
        }
        Ok(())
    }

    /// Custom operators are denied unless an explicit trusted adapter allows them.
    pub fn check_custom_operators(&self, trusted: bool) -> Result<(), ModelError> {
        if self.contract.custom_operators && !trusted {
            return Err(ModelError::CustomOperatorsDenied {
                package: self.package.clone(),
            });
        }
        Ok(())
    }

    /// Produce the checked cancellation error owned by this package session.
    pub fn cancelled(&self) -> ModelError {
        ModelError::Cancelled {
            package: self.package.clone(),
        }
    }
}


fn check_signatures(
    package: &str,
    expected: &[TensorSpec],
    actual: &[TensorSpec],
) -> Result<(), ModelError> {
    if expected.len() != actual.len() {
        return Err(ModelError::signature(
            package,
            format!("{} tensor(s)", expected.len()),
            format!("{} tensor(s)", actual.len()),
        ));
    }
    let mut dynamic_values = BTreeMap::new();
    for (expected, actual) in expected.iter().zip(actual) {
        if expected.name != actual.name
            || expected.dtype != actual.dtype
            || !expected.shape.accepts(&actual.shape)
        {
            return Err(ModelError::signature(
                package,
                expected.to_string(),
                actual.to_string(),
            ));
        }
        for (expected_dimension, actual_dimension) in expected
            .shape
            .dimensions
            .iter()
            .zip(&actual.shape.dimensions)
        {
            if let TensorDimension::Dynamic { name, .. } = expected_dimension {
                let TensorDimension::Static(value) = actual_dimension else {
                    unreachable!("TensorShape::accepts already checked concrete dimensions");
                };
                if let Some(previous) = dynamic_values.insert(name.clone(), *value) {
                    if previous != *value {
                        return Err(ModelError::signature(
                            package,
                            expected.to_string(),
                            actual.to_string(),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
fn validate_descriptor_text(package: &str, field: &str, value: &str) -> Result<(), ModelError> {
    if value.trim().is_empty() || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ModelError::declaration(
            package,
            format!("model descriptor field `{field}` must be non-empty text"),
        ));
    }
    Ok(())
}

fn validate_descriptor_artifact(
    package: &str,
    role: &str,
    artifact: &ModelArtifact,
) -> Result<(), ModelError> {
    validate_artifact_path(package, &artifact.path)?;
    validate_digest(package, &artifact.sha256).map_err(|error| match error {
        ModelError::Provenance { reason, .. } => {
            ModelError::provenance(package, format!("{role} artifact: {reason}"))
        }
        other => other,
    })
}

fn validate_artifact_layout(package: &ModelPackage) -> Result<(), ModelError> {
    if !(3..=4).contains(&package.artifacts.len()) {
        return Err(ModelError::provenance(
            &package.package,
            "model artifact closure must contain graph, weights, tokenizer, and optional adapter",
        ));
    }
    for artifact in &package.artifacts {
        validate_descriptor_artifact(&package.package, "model", artifact)?;
    }
    let required = [
        ("graph", &package.identity.graph_sha256, package.artifacts.first()),
        ("weights", &package.identity.weights_sha256, package.artifacts.get(1)),
        ("tokenizer", &package.identity.tokenizer_sha256, package.artifacts.get(2)),
    ];
    for (role, digest, artifact) in required {
        let Some(artifact) = artifact else {
            return Err(ModelError::provenance(
                &package.package,
                format!("model artifact closure is missing `{role}`"),
            ));
        };
        if &artifact.sha256 != digest {
            return Err(ModelError::provenance(
                &package.package,
                format!("{role} artifact identity differs from the package identity"),
            ));
        }
    }
    match (&package.identity.adapter_sha256, package.artifacts.get(3)) {
        (None, None) | (Some(_), Some(_)) => Ok(()),
        (Some(_), None) | (None, Some(_)) => Err(ModelError::provenance(
            &package.package,
            "optional adapter artifact and identity must agree",
        )),
    }
}

fn validate_runtime_contract(package: &str, contract: &ModelContract) -> Result<(), ModelError> {
    if contract.provider.trim().is_empty() {
        return Err(ModelError::declaration(package, "model provider is empty"));
    }
    if contract.inputs.is_empty() || contract.outputs.is_empty() {
        return Err(ModelError::declaration(
            package,
            "model contract must declare at least one input and one output tensor",
        ));
    }
    for (side, tensors) in [("input", &contract.inputs), ("output", &contract.outputs)] {
        let mut names = BTreeSet::new();
        let mut symbols = BTreeMap::<&str, (u64, u64)>::new();
        for tensor in tensors {
            if tensor.name.trim().is_empty()
                || tensor.name.bytes().any(|byte| byte.is_ascii_control())
                || !names.insert(tensor.name.as_str())
            {
                return Err(ModelError::declaration(
                    package,
                    format!("{side} tensor names must be non-empty and unique"),
                ));
            }
            for dimension in &tensor.shape.dimensions {
                if let TensorDimension::Dynamic { name, min, max } = dimension {
                    if !valid_identifier(name)
                        || min > max
                        || symbols
                            .insert(name, (*min, *max))
                            .is_some_and(|bounds| bounds != (*min, *max))
                    {
                        return Err(ModelError::declaration(
                            package,
                            format!("{side} tensor `{}` has invalid or conflicting dynamic axes", tensor.name),
                        ));
                    }
                }
            }
        }
    }
    for (field, value) in [
        ("max_context", contract.max_context),
        ("max_batch", contract.max_batch),
        ("max_buffer_bytes", contract.max_buffer_bytes),
    ] {
        if value == Some(0) {
            return Err(ModelError::declaration(
                package,
                format!("model limit `{field}` must be positive"),
            ));
        }
    }
    Ok(())
}

fn validate_digest(package: &str, value: &str) -> Result<(), ModelError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(ModelError::provenance(
            package,
            format!("model artifact digest `{value}` is not lowercase SHA-256"),
        ));
    }
    Ok(())
}
fn validate_artifact_path(package: &str, value: &str) -> Result<(), ModelError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.bytes().any(|byte| byte.is_ascii_control())
        || value.contains("://")
        || value.contains('\\')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
    {
        return Err(ModelError::provenance(
            package,
            format!("model artifact path `{value}` must be a local package-relative file"),
        ));
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// Failure at the model-package contract or trust boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    MissingPackage { root: String },
    MissingOutput { package: String, output: String },
    WrongKind { package: String, output: String },
    Declaration { package: String, reason: String },
    Provenance { package: String, reason: String },
    Artifact { package: String, path: String, reason: String },
    SignatureMismatch { package: String, expected: String, actual: String },
    UnsupportedProvider { package: String, expected: String, actual: String },
    Cancelled { package: String },
    CustomOperatorsDenied { package: String },
    ResourceLimit { package: String, reason: String },
    BufferLimit { package: String, bytes: u64 },
}

impl ModelError {
    pub fn declaration(package: &str, reason: impl Into<String>) -> Self {
        Self::Declaration {
            package: package.to_string(),
            reason: reason.into(),
        }
    }

    pub fn provenance(package: &str, reason: impl Into<String>) -> Self {
        Self::Provenance {
            package: package.to_string(),
            reason: reason.into(),
        }
    }

    pub fn artifact(package: &str, path: &str, reason: impl Into<String>) -> Self {
        Self::Artifact {
            package: package.to_string(),
            path: path.to_string(),
            reason: reason.into(),
        }
    }

    pub fn load(package: &str, reason: impl Into<String>) -> Self {
        Self::Artifact {
            package: package.to_string(),
            path: "<package>".to_string(),
            reason: reason.into(),
        }
    }

    pub fn signature(package: &str, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::SignatureMismatch {
            package: package.to_string(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }


    /// Return the registered diagnostic code for this failure.
    pub fn code(&self) -> &'static str {
        match self {
            Self::SignatureMismatch { .. } => SIGNATURE_DIAGNOSTIC,
            Self::Provenance { .. } => PROVENANCE_DIAGNOSTIC,
            Self::MissingPackage { .. }
            | Self::MissingOutput { .. }
            | Self::WrongKind { .. }
            | Self::Declaration { .. }
            | Self::Artifact { .. }
            | Self::UnsupportedProvider { .. }
            | Self::Cancelled { .. }
            | Self::CustomOperatorsDenied { .. }
            | Self::ResourceLimit { .. }
            | Self::BufferLimit { .. } => LOAD_DIAGNOSTIC,
        }
    }

    /// Render this typed failure through the registered diagnostic row.
    pub fn diagnostic(&self) -> Diagnostic {
        let package = self.package_name();
        match self {
            Self::SignatureMismatch {
                expected, actual, ..
            } => Diagnostic::from_row(
                SIGNATURE_DIAGNOSTIC,
                &[("package", package), ("expected", expected), ("actual", actual)],
                None,
            ),
            Self::Provenance { .. } => {
                Diagnostic::from_row(PROVENANCE_DIAGNOSTIC, &[("package", package)], None)
            }
            _ => Diagnostic::from_row(
                LOAD_DIAGNOSTIC,
                &[("package", package), ("reason", &self.to_string())],
                None,
            ),
        }
    }

    fn package_name(&self) -> &str {
        match self {
            Self::MissingPackage { root } => root,
            Self::MissingOutput { package, .. }
            | Self::WrongKind { package, .. }
            | Self::Declaration { package, .. }
            | Self::Provenance { package, .. }
            | Self::Artifact { package, .. }
            | Self::SignatureMismatch { package, .. }
            | Self::UnsupportedProvider { package, .. }
            | Self::Cancelled { package }
            | Self::CustomOperatorsDenied { package }
            | Self::ResourceLimit { package, .. }
            | Self::BufferLimit { package, .. } => package,
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPackage { root } => write!(formatter, "model package is missing at `{root}`"),
            Self::MissingOutput { package, output } => {
                write!(formatter, "package `{package}` has no model output `{output}`")
            }
            Self::WrongKind { package, output } => {
                write!(formatter, "package `{package}` output `{output}` is not a model")
            }
            Self::Declaration { package, reason } => {
                write!(formatter, "model package `{package}` declaration is invalid: {reason}")
            }
            Self::Provenance { package, reason } => {
                write!(formatter, "model package `{package}` provenance is invalid: {reason}")
            }
            Self::Artifact {
                package,
                path,
                reason,
            } => write!(formatter, "model package `{package}` cannot load `{path}`: {reason}"),
            Self::SignatureMismatch {
                package,
                expected,
                actual,
            } => write!(
                formatter,
                "model package `{package}` signature mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::UnsupportedProvider {
                package,
                expected,
                actual,
            } => write!(
                formatter,
                "model package `{package}` requires provider `{expected}`, not `{actual}`"
            ),
            Self::Cancelled { package } => write!(formatter, "model package `{package}` execution cancelled"),
            Self::CustomOperatorsDenied { package } => {
                write!(formatter, "model package `{package}` uses untrusted custom operators")
            }
            Self::ResourceLimit { package, reason } => {
                write!(formatter, "model package `{package}` resource limit exceeded: {reason}")
            }
            Self::BufferLimit { package, bytes } => write!(
                formatter,
                "model package `{package}` buffer of {bytes} bytes exceeds its declared limit"
            ),
        }
    }
}

impl std::error::Error for ModelError {}


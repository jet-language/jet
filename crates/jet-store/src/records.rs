//! #2517: persisted compiler record kinds.
//!
//! Every record uses the one binary codec in `jet_foundation::RecordCodec`.
//! Action kinds are addressed by a caller-computed key (the package check
//! key, the program key, or the compiled-output key). Blob kinds are content
//! addressed: their key is the lowercase SHA-256 hex of their bytes, so a
//! check record names its interface, diagnostics, and item graph by digest.
//!
//! Both directions verify that the bytes decode as a record of the kind's
//! schema. A stored record that no longer decodes is a miss, never an error.

use super::{ActionHandle, Digest, Store, StoreError};
use jet_foundation::RecordCodec::RecordReader;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RecordKind {
    /// Action record under the package check key.
    PkgCheck,
    /// Package interface record (content addressed).
    Iface,
    /// Typed diagnostics of one package check (content addressed).
    Diags,
    /// Item graph of one package check (content addressed).
    Items,
    /// Lowered bodies for build lenses (content addressed).
    Body,
    /// Action record under the program key.
    Program,
    /// Action record for one package's compiled output.
    PkgObject,
}

impl RecordKind {
    pub const ALL: [RecordKind; 7] = [
        Self::PkgCheck,
        Self::Iface,
        Self::Diags,
        Self::Items,
        Self::Body,
        Self::Program,
        Self::PkgObject,
    ];

    pub fn schema(self) -> &'static str {
        match self {
            Self::PkgCheck => "jet.pkg-check/v1",
            Self::Iface => "jet.iface/v1",
            Self::Diags => "jet.diags/v1",
            Self::Items => "jet.items/v1",
            Self::Body => "jet.body/v1",
            Self::Program => "jet.program/v1",
            Self::PkgObject => "jet.pkg-object/v1",
        }
    }

    pub fn from_schema(schema: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.schema() == schema)
    }

    /// True for kinds addressed by a caller key; false for content-addressed
    /// blobs.
    pub fn is_action(self) -> bool {
        matches!(self, Self::PkgCheck | Self::Program | Self::PkgObject)
    }

    fn action(self, key: &str) -> ActionHandle {
        let mut framed = Vec::with_capacity(self.schema().len() + 1 + key.len());
        framed.extend_from_slice(self.schema().as_bytes());
        framed.push(0);
        framed.extend_from_slice(key.as_bytes());
        ActionHandle::from_bytes(&framed)
    }
}

impl Store {
    /// Publish one record. For a blob kind `key` must be the SHA-256 hex of
    /// `bytes`. An action key that already holds different bytes is a
    /// `Conflict`: the same inputs recomputed to a different record.
    pub fn put_record(&self, kind: RecordKind, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        RecordReader::open_schema(bytes, kind.schema()).map_err(|error| {
            StoreError::Config(format!("{} record is not publishable: {error}", kind.schema()))
        })?;
        if kind.is_action() {
            return self.publish_action(kind.action(key), bytes);
        }
        let object = self.publish_blob(bytes)?;
        if object.key() != key {
            return Err(StoreError::Config(format!(
                "{} record key `{key}` is not its content digest `{}`",
                kind.schema(),
                object.key()
            )));
        }
        Ok(())
    }

    /// Load one record. A missing, corrupt, or undecodable record is `None`.
    pub fn record(&self, kind: RecordKind, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let bytes = if kind.is_action() {
            self.get_action(&kind.action(key))?
        } else {
            let Ok(digest) = Digest::from_hex(key) else {
                return Ok(None);
            };
            self.blob_by_digest(&digest)?
        };
        Ok(bytes.filter(|bytes| RecordReader::open_schema(bytes, kind.schema()).is_ok()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::RecordCodec::{encode_record, RecordSection, RecordValue};

    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "jet-store-records-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn records_round_trip_by_kind_and_key() {
        let root = scratch("round-trip");
        let store = Store::new(&root).unwrap();
        let iface = encode_record(
            RecordKind::Iface.schema(),
            &[RecordSection::new(1, vec![RecordValue::str("pkg:.")])],
        )
        .unwrap();
        let digest = jet_foundation::SHA256::sha256_hex(&iface);
        store.put_record(RecordKind::Iface, &digest, &iface).unwrap();
        assert_eq!(store.record(RecordKind::Iface, &digest).unwrap(), Some(iface.clone()));
        // A blob is only reachable under its own kind.
        assert_eq!(store.record(RecordKind::Diags, &digest).unwrap(), None);
        assert!(store.put_record(RecordKind::Iface, &"0".repeat(64), &iface).is_err());

        let check = encode_record(
            RecordKind::PkgCheck.schema(),
            &[RecordSection::new(1, vec![RecordValue::str(digest.clone())])],
        )
        .unwrap();
        store.put_record(RecordKind::PkgCheck, "key-a", &check).unwrap();
        assert_eq!(store.record(RecordKind::PkgCheck, "key-a").unwrap(), Some(check.clone()));
        assert_eq!(store.record(RecordKind::Program, "key-a").unwrap(), None);
        assert_eq!(store.record(RecordKind::PkgCheck, "key-b").unwrap(), None);
        // Wrong schema for the kind is refused.
        assert!(store.put_record(RecordKind::Program, "key-a", &check).is_err());
        assert_eq!(RecordKind::from_schema("jet.items/v1"), Some(RecordKind::Items));
        let _ = std::fs::remove_dir_all(&root);
    }
}

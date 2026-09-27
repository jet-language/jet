//! Native MIR bridge for the canonical Prelude iterator cursor kernel.
//!
//! This module owns only value marshalling and the typed carrier boundary.
//! Cursor stepping, current-item consumption, and exhaustion remain in the
//! shared `Prelude/Core/LoopCursor.rs` fragment used by AOT and evaluator
//! collection paths. Host-owned iterators must provide the existing `Send`
//! runtime contract; thread-affine resources require an owner-side adapter.

use jet_foundation::MIR::{
    MirConstKey, MirHandleId, MirLoopSourceKind, MirNativeCursor, MirNativeCursorState,
    MirNativeCursorError, MirRuntimeValue,
};

#[allow(dead_code)]
mod kernel {
    fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
        panic!("{message}");
    }

    include!("../Prelude/Core/LoopCursor.rs");
}

pub type NativeIter =
    Box<dyn Iterator<Item = Result<MirRuntimeValue, MirNativeCursorError>> + Send>;

/// Result of acquiring a native resource iterator.
///
/// `consumed` is committed as soon as a by-value source slot has been
/// detached. It is preserved even when iterator construction or cursor
/// installation fails, so the caller can retire the exact source receipt
/// without probing the arena or using a side channel.
pub struct NativeLoopResourceOutcome<T> {
    pub consumed: Option<NativeLoopResourceKey>,
    pub result: Result<T, MirNativeCursorError>,
}
type NativeCursor =
    kernel::JetLoopIterCursor<Result<MirRuntimeValue, MirNativeCursorError>, NativeIter>;

struct NativeCursorState {
    cursor: NativeCursor,
}

impl MirNativeCursorState for NativeCursorState {
    fn has_next(&mut self) -> Result<bool, MirNativeCursorError> {
        kernel::jet_loop_iter_typed_has_next(&self.cursor)
    }

    fn value(&mut self) -> Result<MirRuntimeValue, MirNativeCursorError> {
        kernel::jet_loop_iter_typed_value(&mut self.cursor)
    }

    fn advance(&mut self) -> Result<(), MirNativeCursorError> {
        kernel::jet_loop_iter_typed_advance(&mut self.cursor)
    }
}

fn cursor_from_iter(
    iter: NativeIter,
    step_value: i64,
    has_step: bool,
) -> Result<MirRuntimeValue, MirNativeCursorError> {
    let cursor = kernel::jet_loop_iter_init_typed(iter, step_value, has_step)
        .map_err(|message| MirNativeCursorError::internal(message))?;
    Ok(MirRuntimeValue::NativeCursor(MirNativeCursor::new(
        NativeCursorState { cursor },
    )))
}

/// Checked identity supplied by the scoped native handle bridge. Resource
/// owners validate all three fields before returning an iterator; `raw` is
/// never interpreted without the handle generation and source-kind facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLoopResourceKey {
    pub handle: MirHandleId,
    pub raw: i64,
    pub source_kind: MirLoopSourceKind,
}

/// Checked producers for the native resource source forms. Implementations
/// resolve the scoped resource key, retain its lease in the returned iterator,
/// and convert each item to `Ok(MirRuntimeValue)` or its original typed
/// `MirNativeCursorError`. A by-value producer reports its committed source
/// key in `NativeLoopResourceOutcome`, including when later setup fails; this
/// trait owns no resource table and cannot be used for UserIterable.
pub trait NativeLoopResourceFactory: Send + Sync {
    fn plain_stream(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter>;
    fn lines_file(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter>;
    fn lines_stdin(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter>;
    fn lines_process_stream(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter>;
    fn channel_receiver(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter>;
    fn encoding_reader(
        &self,
        key: &NativeLoopResourceKey,
        reader_type: &str,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter>;
}

/// Select one checked resource producer and install its iterator in the shared
/// cursor kernel. The factory validates the complete scoped resource key and
/// returns any committed by-value source receipt alongside success or failure;
/// no raw value is interpreted as an item or persisted by this adapter.
pub fn init_from_resource_factory(
    factory: &dyn NativeLoopResourceFactory,
    key: &NativeLoopResourceKey,
    step_value: i64,
    has_step: bool,
    by_value: bool,
) -> NativeLoopResourceOutcome<MirRuntimeValue> {
    let acquisition = match &key.source_kind {
        MirLoopSourceKind::Plain => factory.plain_stream(key, by_value),
        MirLoopSourceKind::Chars => {
            return NativeLoopResourceOutcome {
                consumed: None,
                result: Err(MirNativeCursorError::internal(
                    "character cursor requires the canonical String data carrier",
                )),
            }
        }
        MirLoopSourceKind::LinesFile => factory.lines_file(key, by_value),
        MirLoopSourceKind::LinesStdin => factory.lines_stdin(key, by_value),
        MirLoopSourceKind::LinesProcessStream => {
            factory.lines_process_stream(key, by_value)
        }
        MirLoopSourceKind::ChannelReceiver => factory.channel_receiver(key, by_value),
        MirLoopSourceKind::EncodingReader { reader_type } => {
            factory.encoding_reader(key, reader_type, by_value)
        }
        MirLoopSourceKind::Iterable { .. } => {
            return NativeLoopResourceOutcome {
                consumed: None,
                result: Err(MirNativeCursorError::internal(
                    "UserIterable cursor remains Source-owned",
                )),
            }
        }
    };
    let NativeLoopResourceOutcome { consumed, result } = acquisition;
    let result = match result {
        Ok(iter) => init_from_resource(iter, step_value, has_step, key.source_kind.clone()),
        Err(error) => Err(error),
    };
    NativeLoopResourceOutcome { consumed, result }
}

/// Initialize a cursor from a checked native resource producer. The producer
/// must already retain the source-specific lease and implement the checked
/// by-value/borrow decision; this function only rejects UserIterable, whose
/// protocol remains Source-owned, then installs the shared cursor kernel.
pub(crate) fn init_from_resource(
    iter: NativeIter,
    step_value: i64,
    has_step: bool,
    source_kind: MirLoopSourceKind,
) -> Result<MirRuntimeValue, MirNativeCursorError> {
    if matches!(source_kind, MirLoopSourceKind::Iterable { .. }) {
        return Err(MirNativeCursorError::internal(
            "UserIterable cursor remains Source-owned",
        ));
    }
    cursor_from_iter(iter, step_value, has_step)
}

/// Resource-backed source kinds use `init_from_resource`; their host owner
/// retains the checked lease and borrow/move meaning without serialization.
pub(crate) fn init_from_value(
    collection: MirRuntimeValue,
    step_value: i64,
    has_step: bool,
    by_value: bool,
    source_wire: &str,
) -> Result<MirRuntimeValue, MirNativeCursorError> {
    let source_kind = MirLoopSourceKind::from_wire(source_wire).ok_or_else(|| {
        MirNativeCursorError::internal("invalid canonical loop source wire")
    })?;
    let iter = value_iterator(collection, &source_kind, by_value)?;
    cursor_from_iter(iter, step_value, has_step)
}

fn value_iterator(
    collection: MirRuntimeValue,
    source_kind: &MirLoopSourceKind,
    by_value: bool,
) -> Result<NativeIter, MirNativeCursorError> {
    match (source_kind, collection) {
        (MirLoopSourceKind::Plain, MirRuntimeValue::List(values)) => {
            let values = if by_value { values } else { values.clone() };
            Ok(Box::new(values.into_iter().map(Ok)))
        }
        (MirLoopSourceKind::Plain, MirRuntimeValue::Map(entries)) => {
            let entries = if by_value { entries } else { entries.clone() };
            let values = entries
                .into_iter()
                .map(|(key, value)| {
                    MirRuntimeValue::Struct {
                        type_name: "Tuple".to_string(),
                        fields: vec![
                            ("key".to_string(), const_key_value(key)),
                            ("value".to_string(), value),
                        ],
                    }
                })
                .collect::<Vec<_>>();
            Ok(Box::new(values.into_iter().map(Ok)))
        }
        (MirLoopSourceKind::Plain, MirRuntimeValue::Bytes(bytes)) => {
            let bytes = if by_value { bytes } else { bytes.clone() };
            Ok(Box::new(
                bytes
                    .into_iter()
                    .map(|value| MirRuntimeValue::Int(i64::from(value)))
                    .map(Ok),
            ))
        }
        (MirLoopSourceKind::Plain | MirLoopSourceKind::Chars, MirRuntimeValue::String(text)) => {
            let text = if by_value { text } else { text.clone() };
            Ok(Box::new(
                kernel::JetStringChars::new(text)
                    .map(MirRuntimeValue::Char)
                    .map(Ok),
            ))
        }
        (MirLoopSourceKind::LinesFile
        | MirLoopSourceKind::LinesStdin
        | MirLoopSourceKind::LinesProcessStream
        | MirLoopSourceKind::ChannelReceiver
        | MirLoopSourceKind::EncodingReader { .. }
        | MirLoopSourceKind::Iterable { .. }, _) => Err(
            MirNativeCursorError::internal(
                "resource-backed loop source requires its host-owned iterator carrier",
            ),
        ),
        (source_kind, _) => Err(MirNativeCursorError::internal(format!(
            "loop source kind `{source_kind:?}` does not match its native MIR collection carrier"
        ))),
    }
}

fn const_key_value(key: MirConstKey) -> MirRuntimeValue {
    match key {
        MirConstKey::Int(value) => MirRuntimeValue::Int(value),
        MirConstKey::String(value) => MirRuntimeValue::String(value),
        MirConstKey::Bool(value) => MirRuntimeValue::Bool(value),
        MirConstKey::Char(value) => MirRuntimeValue::Char(value),
        MirConstKey::Tuple(fields) => MirRuntimeValue::Struct {
            type_name: "Tuple".to_string(),
            fields: fields
                .into_iter()
                .map(|(name, value)| (name, const_key_value(value)))
                .collect(),
        },
        MirConstKey::Struct { type_name, fields } => MirRuntimeValue::Struct {
            type_name,
            fields: fields
                .into_iter()
                .map(|(name, value)| (name, const_key_value(value)))
                .collect(),
        },
        MirConstKey::Enum { type_name, variant } => MirRuntimeValue::Enum {
            type_name,
            variant,
            args: Vec::new(),
        },
    }
}

pub(crate) fn has_next(
    value: &MirRuntimeValue,
) -> Result<bool, MirNativeCursorError> {
    let MirRuntimeValue::NativeCursor(cursor) = value else {
        return Err(MirNativeCursorError::internal(
            "native Prelude iterator operation requires its cursor carrier",
        ));
    };
    cursor.has_next()
}

pub(crate) fn value(
    value: &MirRuntimeValue,
) -> Result<MirRuntimeValue, MirNativeCursorError> {
    let MirRuntimeValue::NativeCursor(cursor) = value else {
        return Err(MirNativeCursorError::internal(
            "native Prelude iterator operation requires its cursor carrier",
        ));
    };
    cursor.value()
}

pub(crate) fn advance(
    value: &MirRuntimeValue,
) -> Result<(), MirNativeCursorError> {
    let MirRuntimeValue::NativeCursor(cursor) = value else {
        return Err(MirNativeCursorError::internal(
            "native Prelude iterator operation requires its cursor carrier",
        ));
    };
    cursor.advance()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::MIR::MirNativeOwned;

    fn list(values: &[i64]) -> MirRuntimeValue {
        MirRuntimeValue::List(
            values
                .iter()
                .copied()
                .map(MirRuntimeValue::Int)
                .collect(),
        )
    }

    #[test]
    fn plain_cursor_steps_and_exhausts_through_shared_kernel() {
        let cursor = init_from_value(list(&[1, 2, 3, 4]), 2, true, true, "plain")
            .expect("plain source initializes");
        assert!(has_next(&cursor).expect("has_next succeeds"));
        assert_eq!(value(&cursor), Ok(MirRuntimeValue::Int(1)));
        advance(&cursor).expect("advance succeeds");
        assert_eq!(value(&cursor), Ok(MirRuntimeValue::Int(3)));
        advance(&cursor).expect("advance succeeds");
        assert!(!has_next(&cursor).expect("has_next succeeds"));
        assert_eq!(
            value(&cursor),
            Err(MirNativeCursorError::internal(
                "iterator loop value requested after exhaustion"
            ))
        );
    }

    #[test]
    fn clone_preserves_identity_and_cursor_lifetime() {
        let cursor = init_from_value(list(&[7, 8]), 0, false, false, "plain")
            .expect("plain source initializes");
        let clone = cursor.clone();
        let (MirRuntimeValue::NativeCursor(left), MirRuntimeValue::NativeCursor(right)) =
            (&cursor, &clone)
        else {
            panic!("initializer must return the typed cursor carrier");
        };
        assert_eq!(left, right);
        assert_eq!(value(&clone), Ok(MirRuntimeValue::Int(7)));
        advance(&cursor).expect("shared alias advances");
        assert_eq!(value(&clone), Ok(MirRuntimeValue::Int(8)));
    }

    #[test]
    fn chars_and_host_owned_iterators_use_the_same_cursor_operations() {
        let chars = init_from_value(
            MirRuntimeValue::String("aβ".to_string()),
            1,
            true,
            true,
            "chars",
        )
        .expect("character source initializes");
        assert_eq!(value(&chars), Ok(MirRuntimeValue::Char('a')));
        advance(&chars).expect("advance succeeds");
        assert_eq!(value(&chars), Ok(MirRuntimeValue::Char('β')));

        let host = init_from_resource(
            Box::new(
                [10_i64, 20_i64]
                    .into_iter()
                    .map(MirRuntimeValue::Int)
                    .map(Ok),
            ),
            1,
            true,
            MirLoopSourceKind::ChannelReceiver,
        )
        .expect("host-owned source initializes");

        assert_eq!(value(&host), Ok(MirRuntimeValue::Int(10)));
        advance(&host).expect("advance succeeds");
        assert_eq!(value(&host), Ok(MirRuntimeValue::Int(20)));
    }

    #[test]
    fn native_owned_payloads_cross_the_cursor_without_serialization() {
        #[derive(Debug, PartialEq, Eq)]
        struct Payload(i64);

        let owner = MirNativeOwned::new(Payload(17));
        let alias = owner.clone();
        assert_eq!(owner.identity(), alias.identity());
        let carrier = MirRuntimeValue::NativeOwned(owner.clone());
        let cursor = init_from_resource(
            Box::new([carrier].into_iter().map(Ok)),
            1,
            true,
            MirLoopSourceKind::ChannelReceiver,
        )
        .expect("native-owned source initializes");
        let MirRuntimeValue::NativeOwned(returned) =
            value(&cursor).expect("native-owned value crosses cursor")
        else {
            panic!("cursor must preserve the native-owned carrier");
        };
        assert_eq!(returned, owner);
        assert_eq!(returned.downcast_ref::<Payload>(), Some(&Payload(17)));
    }

    #[test]
    fn host_iterator_errors_cross_cursor_without_panicking_or_stringifying() {
        let error = MirNativeCursorError::from_runtime_value(MirRuntimeValue::Struct {
            type_name: "ReaderError".to_string(),
            fields: vec![
                (
                    "operation".to_string(),
                    MirRuntimeValue::String("read".to_string()),
                ),
                (
                    "record".to_string(),
                    MirRuntimeValue::Int(2),
                ),
                (
                    "reason".to_string(),
                    MirRuntimeValue::String("reader failed".to_string()),
                ),
            ],
        });
        assert!(error.as_runtime_value().is_some());
        assert!(!error.is_internal());
        let internal = MirNativeCursorError::internal("cursor setup failed");
        assert!(internal.as_runtime_value().is_none());
        assert!(internal.is_internal());
        let items = vec![
            Ok(MirRuntimeValue::Int(1)),
            Err(error.clone()),
        ];
        let cursor = init_from_resource(
            Box::new(items.into_iter()),
            1,
            true,
            MirLoopSourceKind::LinesFile,
        )
        .expect("host-owned source initializes");
        assert_eq!(value(&cursor), Ok(MirRuntimeValue::Int(1)));
        advance(&cursor).expect("advance to the error succeeds");
        assert_eq!(has_next(&cursor), Err(error.clone()));
        assert_eq!(value(&cursor), Err(error));
    }

    #[test]
    fn cursor_and_runtime_value_retain_thread_contracts() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MirNativeCursor>();
        assert_send_sync::<MirNativeOwned>();
        assert_send_sync::<MirRuntimeValue>();
    }

}

//! D-WEBAPP1 / c-devserver / D-FLAGSHIP-WEBAPI1: resident-JIT web host.
//! Thin opaque-handle adapters over Prelude/App.rs + DevServer.rs.
//! Source-owned `core.web` behavior stays in CoreLib; this module only carries
//! real host boundaries.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

pub(crate) mod web_rt {
    /// No evaluator run carries a web devtools policy, so the Prelude hooks
    /// report devtools and history as disabled.
    pub(crate) fn jet_web_runtime_devtools_enabled() -> bool {
        false
    }

    fn jet_web_runtime_history_enabled() -> bool {
        false
    }

    fn jet_runtime_stop(_code: &'static str, _file: &str, _line: u32, message: &str) -> ! {
        panic!("{message}");
    }

    use jet_foundation::Devtools::jet_devtools_publish_event;
    use crate::JetShow;
    
    pub(crate) use crate::net_http_rt::{jet_app_http_assets, jet_app_http_mount,
        jet_app_http_mux_new, jet_app_http_page_response, jet_app_http_reload,
        jet_app_http_request_url, jet_app_http_serve, jet_std,
        jet_web_server_fn_register_http, jet_web_server_fn_typed, JetHTTPMux, JetHTTPRouter, JetWebServerFnError, JetWebServerFnHandler,
        JetWebServerFunction
    };
    // The keyed live-query entries WebTable.rs names and the query-panel
    // projections App.rs names live with the resident reactive host, which
    // includes the one LiveQuery.rs / WebQuery.rs source.
    pub(crate) use crate::Reactive::{
        jet_live_publish_transport,
        jet_app_invalidate_key, jet_app_live_keyed, jet_web_query_dev_panel,
        jet_web_query_facts_json
    };
    // EncodingTraits.rs' map codec, inline-range refinement, and CSV rows are
    // the same kernels the collection, runtime, and encoding hosts compile.
    pub(crate) use crate::Collections::collection_semantics::JetMap;
    pub(crate) use crate::Encoding::inline_range_rt::jet_inline_range_from_int;
    fn jet_ring_csv_parse(
        text: &str,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> Result<Vec<Vec<String>>, String> {
        crate::Encoding::csv_parse(text, delimiter, header, skip_blank)
            .map(|rows| rows.into_iter().map(|row| row.fields).collect())
    }
    // EncodingTraits.rs names these unqualified; the canonical definitions
    // are the JIT modules reexported through the net_http_rt jet_std shim.
    #[allow(unused_imports)]
    pub(crate) use jet_std::{
        jet_codec_date_decode, jet_codec_date_encode, jet_codec_datetime_decode,
        jet_codec_datetime_encode, jet_codec_decimal_decode_int, jet_codec_decimal_decode_text,
        jet_codec_decimal_encode, jet_codec_duration_decode, jet_codec_duration_encode,
        jet_codec_local_time_decode, jet_codec_local_time_encode, JetDate, JetDateTime,
        JetDecimal, JetLocalTime
    };
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/EncodingTraits.rs");
    const MAX_SYNC_TEXT: usize = 1024 * 1024;
    const MAX_SYNC_REPLICAS: usize = 4096;
    const MAX_SYNC_ENTRIES: usize = 100_000;
    const MAX_SYNC_SESSION: usize = 256;
    const MAX_SYNC_DOCUMENT: usize = 4 * 1024 * 1024;
    fn jet_sync_counter_total(counts: &[(String, u64, u64)]) -> Option<i64> {
        let mut total = 0i128;
        for (_, positive, negative) in counts {
            total = total
                .checked_add(i128::from(*positive))?
                .checked_sub(i128::from(*negative))?;
        }
        i64::try_from(total).ok()
    }
    pub(crate) fn jet_sync_token_is_valid(value: &str) -> bool {
        !value.trim().is_empty()
            && value.len() <= MAX_SYNC_TEXT
            && !value.chars().any(char::is_control)
    }
    include!("../../jet-codegen/src/Prelude/CoreLib/SyncPublish.rs");
    mod jet_app_middleware {
        include!("../../jet-foundation/src/AppMiddleware.rs");
    }
    include!("../../jet-codegen/src/Prelude/App.rs");
    // Browser storage has no native AOT row, so App.rs does not re-export it;
    // the resident host marshals these entries directly. `jet_web_page` is a
    // canonical Core row and reaches every host through the App.rs root.
    
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/WebPending.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebRouter.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/OpenAPI.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebForms.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebTable.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebVirtual.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebStore.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/CoreSlice7WebRaw.rs");
    include!("../../jet-codegen/src/Prelude/DevServer.rs");
}

#[cfg(test)]
mod tests {
    use super::web_rt::__jet_Decode;
    use jet_foundation::{Dyn::Dyn, Numeric::JetInt};

    #[test]
    fn store_nullary_transaction_stages_captured_signal_writes_and_publishes_once() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

        let store = super::web_rt::JetWebStore::new("nullary-store".to_string(), 0_i64, 8);
        let signal = store.state_signal();
        let notifications = Arc::new(AtomicUsize::new(0));
        let observed = notifications.clone();
        let _subscription = store.subscribe(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
        });
        let baseline = notifications.load(Ordering::SeqCst);
        let transaction = store.transact("edit".to_string(), vec!["value".to_string()], || {
            signal.set(1);
            assert_eq!(signal.get(), 1);
            signal.set(2);
            assert_eq!(store.value(), 2);
            assert_eq!(notifications.load(Ordering::SeqCst), baseline);
        });
        assert_eq!(transaction.before, 0);
        assert_eq!(transaction.after, 2);
        assert_eq!(store.value(), 2);
        assert_eq!(notifications.load(Ordering::SeqCst), baseline + 1);
    }

    #[test]
    fn web_decode_preserves_exact_integers_and_fixed_bounds() {
        for expected in [i64::MIN, i64::MAX] {
            let tree = Dyn::Number(expected.to_string());
            assert_eq!(<i64 as __jet_Decode>::jet_decode(&tree).unwrap(), expected);
        }
        for text in ["-9223372036854775809", "9223372036854775808"] {
            let tree = Dyn::Number(text.to_string());
            assert!(<i64 as __jet_Decode>::jet_decode(&tree).is_err());
            assert_eq!(
                <JetInt as __jet_Decode>::jet_decode(&tree).unwrap().to_string(),
                text,
            );
        }
        let tree = Dyn::Number(u64::MAX.to_string());
        assert_eq!(<u64 as __jet_Decode>::jet_decode(&tree).unwrap(), u64::MAX);
        for text in ["-1", "18446744073709551616"] {
            assert!(
                <u64 as __jet_Decode>::jet_decode(&Dyn::Number(text.to_string()))
                    .is_err()
            );
        }
        let text = "12345678901234567890123456789012345678901234567890";
        assert_eq!(
            <JetInt as __jet_Decode>::jet_decode(&Dyn::Number(text.to_string()))
                .unwrap()
                .to_string(),
            text,
        );
        assert!(<JetInt as __jet_Decode>::jet_decode(&Dyn::Number("1.5".to_string())).is_err());
        assert!(<JetInt as __jet_Decode>::jet_decode(&Dyn::Text("12".to_string())).is_err());
    }
}


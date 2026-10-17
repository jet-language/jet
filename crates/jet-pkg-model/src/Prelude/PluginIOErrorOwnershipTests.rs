//! Private generated-runtime test: raw component bytes arrive on stdin from
//! the qualified canonical writer. No filesystem refusal or new wire codec.

use super::*;
use std::io::Read;

const MAX_COMPONENT_BYTES: u64 = 16 * 1024 * 1024;
const RESOURCE: &str = "io11-original-resource-positive9";
const CAUSE: &str = "io11-original-cause-positive9";

#[test]
#[ignore = "requires qualified canonical writer component bytes on bounded stdin"]
fn guest_already_exists_positive_context_buffers_release_once() {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_COMPONENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .expect("read bounded raw canonical component bytes");
    assert!(!bytes.is_empty() && bytes.len() as u64 <= MAX_COMPONENT_BYTES);
    println!(
        "io11 component input bytes={} sha256={}",
        bytes.len(),
        jet_foundation::SHA256::sha256_hex(&bytes),
    );
    observe_positive9_component(&bytes);
}

fn observe_positive9_component(bytes: &[u8]) {
    let engine = plugin_engine().expect("existing canonical plugin engine");
    let component = Component::new(&engine, bytes).expect("actual raw component/WIT validates");
    let mut linker: Linker<PluginHostState> = Linker::new(&engine);
    let component_type = component.component_type();
    let mut imports = component_type.imports(&engine);
    let (name, item) = imports.next().expect("the actual registered host import");
    assert!(imports.next().is_none(), "one actual registered fixture host import");
    assert_eq!(name, "host-read");
    let ComponentItem::ComponentFunc(function) = item else {
        panic!("canonical fixture has a typed direct function import");
    };
    let fact = plugin_import_fact(name, &function).expect("actual component-derived import fact");
    assert_eq!(fact.required_grant(), "FS.Read");
    let authority = CanonicalAuthority::from_rights(vec!["FS.Read".to_string()]);
    let decision = plugin_load_import_fact().decision(&authority);
    assert_eq!(decision.verdict, Verdict::Allowed);
    let mut declared_imports = BTreeMap::new();
    declared_imports.insert(fact.id.clone(), fact.clone());
    linker
        .root()
        .func_new(name, move |_store, params, results| {
            assert!(matches!(params, [Val::String(value)] if value == "ownership-fixture"));
            let error = std::io::Error::new(std::io::ErrorKind::AlreadyExists, CAUSE);
            assert_eq!(error.raw_os_error(), None);
            assert_eq!(error.to_string(), CAUSE);
            plugin_set_io_result_error(&fact, results, "read", Some(RESOURCE), &error)
                .map_err(wasmtime::Error::msg)?;
            // This first case is the real std producer's Some(cause) domain.
            // Structural None(cause) is not silently substituted or credited.
            let [Val::Result(Err(Some(value)))] = results else {
                panic!("actual canonical result error carrier");
            };
            let Val::Variant(kind, Some(context)) = &**value else {
                panic!("actual canonical known-kind IO error");
            };
            assert_eq!(kind, "already-exists");
            let Val::Record(fields) = &**context else {
                panic!("actual declared IOContext record");
            };
            let [(operation_key, operation), (resource_key, resource), (code_key, code), (cause_key, cause)] =
                fields.as_slice()
            else {
                panic!("all four canonical IOContext fields in declared order");
            };
            assert_eq!(operation_key, "operation");
            assert!(matches!(operation, Val::Enum(value) if value == "read"));
            assert_eq!(resource_key, "resource");
            assert!(matches!(resource, Val::Option(Some(value))
                if matches!(&**value, Val::String(text) if text == RESOURCE)));
            assert_eq!(code_key, "os-code");
            assert!(matches!(code, Val::Option(None)));
            assert_eq!(cause_key, "cause");
            assert!(matches!(cause, Val::Option(Some(value))
                if matches!(&**value, Val::String(text) if text == CAUSE)));
            Ok(())
        })
        .expect("existing dynamic Wasmtime linker registers typed producer callback");
    let mut store = Store::new(
        &engine,
        PluginHostState {
            limits: plugin_limits(),
            authority,
            authority_decision: decision,
            imports: declared_imports,
            disabled_adapter: None,
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(1_000_000_000);
    store.epoch_deadline_trap();
    store.set_fuel(PLUGIN_MAX_FUEL).expect("existing canonical fuel limit");
    let pre_instance = linker.instantiate_pre(&component).expect("actual WIT/import pairing");
    let instance = pre_instance.instantiate(&mut store).expect("actual guest instantiation");
    let function = instance.get_func(&mut store, "probe").expect("the one registered guest export");
    let mut results = [Val::String(String::new())];
    function
        .call(&mut store, &[Val::String("ownership-fixture".to_string())], &mut results)
        .expect("actual typed lower plus generated guest decode/free and allocation witness");
    let [result] = results;
    let Val::String(output) = result else {
        panic!("actual owned host Text result");
    };
    function
        .post_return(&mut store)
        .expect("actual post-return cleanup and allocation witness finish");
    let (guest, event_text) = output
        .rsplit_once("|ownership-events=")
        .expect("actual guest allocation-generation diagnostics");
    assert_eq!(guest, "AlreadyExists|original-context-preserved");
    let events = parse_observed_io_releases(event_text);
    for ((role, expected_size), event) in [(1, RESOURCE.len()), (2, CAUSE.len())]
        .into_iter()
        .zip(events)
    {
        assert_eq!(event[0], role);
        assert_ne!(event[1], 0, "original allocation generation");
        assert_ne!(event[2], 0, "original canonical-lowering pointer");
        assert_eq!(event[3], expected_size as u64, "actual original allocation layout size");
        assert_eq!(event[4], 1, "actual string allocation layout alignment");
        assert_eq!(event[5], 1, "actual deallocation once, not balance or copied-value inference");
        println!(
            "io11 release role={} generation={} pointer={} size={} alignment={} releases={}",
            event[0], event[1], event[2], event[3], event[4], event[5],
        );
    }
    assert_ne!(events[0][1], events[1][1], "distinct original allocation generations");
    assert_ne!(events[0][2], events[1][2], "distinct simultaneously lowered allocations");
}

fn parse_observed_io_releases(text: &str) -> [[u64; 6]; 2] {
    // Test-only diagnostics, not a component transport or production codec.
    let text = text
        .strip_prefix("[(")
        .and_then(|text| text.strip_suffix(")]") )
        .expect("two exact allocation event tuples");
    let (left, right) = text.split_once("), (").expect("two distinct allocation roles");
    let mut events = [[0; 6]; 2];
    for (part, event) in [left, right].into_iter().zip(&mut events) {
        let mut fields = part.split(", ");
        for field in event {
            *field = fields
                .next()
                .expect("all six observed event fields")
                .parse::<u64>()
                .expect("actual observed unsigned event value");
        }
        assert!(fields.next().is_none(), "no additional event fields");
    }
    events
}

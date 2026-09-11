// D-TEST-HISTORY1=A: typed operation histories share the Foundation runner,
// comparison record, and shrinker with host-side evidence.

type JetHistoryCaptureReader<'a> = dyn Fn() -> Result<jet_std::DataTree, String> + 'a;
type JetHistorySendCaptureReader<'a> = dyn Fn() -> Result<jet_std::DataTree, String> + Send + Sync + 'a;
type JetHistoryWeakReader = crate::jet_testing_history_foundation::HistoryCallbackReader<Result<jet_std::DataTree, String>>;
type JetHistoryWeakSendReader = crate::jet_testing_history_foundation::HistorySendCallbackReader<Result<jet_std::DataTree, String>>;

// A generic call carries the codecs of its checked type arguments. Factories
// retain this dictionary with their environment, so invocation after return
// still encodes the actual instantiation. Any is only a checked downcast; its
// address, TypeId and Rust type name never become provenance.
#[derive(Clone)]
struct JetHistoryTypeCodec {
    checked_type: String,
    encode: std::sync::Arc<dyn Fn(&dyn std::any::Any, usize) -> Result<jet_std::DataTree, String> + Send + Sync>,
}
type JetHistoryTypes = std::collections::BTreeMap<String, JetHistoryTypeCodec>;
thread_local! {
    static JET_HISTORY_TYPES: std::cell::RefCell<JetHistoryTypes> = std::cell::RefCell::new(JetHistoryTypes::new());
}
fn jet_history_current_types() -> JetHistoryTypes {
    JET_HISTORY_TYPES.with(|types| types.borrow().clone())
}
fn jet_history_with_types<R>(types: JetHistoryTypes, body: impl FnOnce() -> R) -> R {
    struct Scope(JetHistoryTypes);
    impl Drop for Scope {
        fn drop(&mut self) {
            JET_HISTORY_TYPES.with(|types| *types.borrow_mut() = std::mem::take(&mut self.0));
        }
    }
    let _scope = Scope(JET_HISTORY_TYPES.with(|current| current.replace(types)));
    body()
}
fn jet_history_type_codec<T: 'static>(
    checked_type: String,
    encode: impl Fn(&T, usize) -> Result<jet_std::DataTree, String> + Send + Sync + 'static,
) -> JetHistoryTypeCodec {
    JetHistoryTypeCodec { checked_type, encode: std::sync::Arc::new(move |value, depth| {
        let value = value.downcast_ref::<T>().ok_or_else(|| "history capture type does not match its checked instantiation".to_string())?;
        encode(value, depth)
    }) }
}
fn jet_history_encode_type<T: 'static>(
    types: &JetHistoryTypes, name: &str, value: &T, depth: usize,
) -> Result<jet_std::DataTree, String> {
    if depth >= 64 { return Err("history capture exceeds the nesting bound".to_string()); }
    (types.get(name).ok_or_else(|| "history capture type is unresolved".to_string())?.encode)(value, depth)
}

// Ordinary boxed callables stay callable through the Fn supertrait. Cloning
// their generated shared environment also preserves its live metadata reader.
// Rust host boundaries can upcast this private trait to their existing dyn Fn.
// A mode-bearing instantiation keeps borrowed inputs late-bound, so a callback
// can borrow each host argument for one invocation without owning its referent.
macro_rules! jet_history_callable_argument {
    (owned, $arg:ident, $lifetime:lifetime) => { $arg };
    (read, $arg:ident, $lifetime:lifetime) => { &$lifetime $arg };
    (write, $arg:ident, $lifetime:lifetime) => { &$lifetime mut $arg };
}

macro_rules! jet_history_callable {
    ($name:ident $(, $arg:ident)*) => {
        jet_history_callable!($name; $($arg => owned),*);
    };
    ($name:ident; $($arg:ident => $mode:ident),* $(,)?) => {
        trait $name<'capture, $($arg,)* R>:
            for<'__jet_history_arg>
                Fn($(jet_history_callable_argument!($mode, $arg, '__jet_history_arg)),*) -> R
                + 'capture
        {
            fn clone_box(&self) -> Box<dyn $name<'capture, $($arg,)* R> + 'capture>;
        }
        impl<'capture, F, $($arg,)* R> $name<'capture, $($arg,)* R> for F
        where
            F: for<'__jet_history_arg>
                    Fn($(jet_history_callable_argument!($mode, $arg, '__jet_history_arg)),*) -> R
                + Clone
                + 'capture,
        {
            fn clone_box(&self) -> Box<dyn $name<'capture, $($arg,)* R> + 'capture> {
                let (cloned, life) = jet_history_clone_callable(self);
                let key = self as *const F as *const () as usize;
                let reader = JET_HISTORY_CALLBACKS.with(|callbacks| {
                    callbacks.borrow().get(&key).and_then(|(life, reader)| {
                        life.upgrade().map(|_| reader.clone())
                    })
                });
                let callback: Box<dyn $name<'capture, $($arg,)* R> + 'capture> = Box::new(cloned);
                match (reader, life) {
                    // The clone shares the exact typed environment/reader and
                    // owns its new ticket. No referent lifetime is extended.
                    (Some(reader), Some(life)) => jet_history_register_callback_reader(callback, reader, &life),
                    _ => callback,
                }
            }
        }
        impl<'capture, $($arg,)* R> Clone
            for Box<dyn $name<'capture, $($arg,)* R> + 'capture>
        where
            $($arg: 'capture,)*
            R: 'capture,
        {
            fn clone(&self) -> Self { self.as_ref().clone_box() }
        }
        impl<'capture, $($arg,)* R> std::fmt::Debug
            for dyn $name<'capture, $($arg,)* R> + 'capture
        {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("Function").finish_non_exhaustive()
            }
        }
    };
}

jet_history_callable!(JetHistoryFn1, A0);
jet_history_callable!(JetHistoryFn3, A0, A1, A2);
jet_history_callable!(JetHistoryFn0);

#[derive(Clone)]
struct JetHistoryStrategy<Command: 'static> {
    generate: Box<dyn JetHistoryFn3<'static, JetHistoryRng, usize, usize,
        JetOutcome<crate::jet_testing_history_foundation::TypedHistoryCase<Command>, JetAbsent>>>,
    rebuild: Box<dyn JetHistoryFn1<'static, crate::jet_testing_history_foundation::HistoryCase,
        JetOutcome<crate::jet_testing_history_foundation::TypedHistoryCase<Command>, JetAbsent>>>,
    valid: Box<dyn JetHistoryFn1<'static, crate::jet_testing_history_foundation::TypedHistoryCase<Command>, bool>>,
    bounds: crate::jet_testing_history_foundation::HistoryBounds,
    distributions: Vec<crate::jet_testing_history_foundation::HistoryDistribution>,
}

// A scoped owner of the actual Foundation RNG. All source-level copies share
// this cell. Closing the callback returns that same RNG to the runner and
// invalidates escaped handles; no second draw implementation or state snapshot.
#[derive(Clone)]
struct JetHistoryRng(std::rc::Rc<std::cell::RefCell<Option<crate::jet_testing_history_foundation::HistoryRng>>>);

struct JetHistoryRngLoan<'a> {
    destination: &'a mut crate::jet_testing_history_foundation::HistoryRng,
    value: JetHistoryRng,
}

impl<'a> JetHistoryRngLoan<'a> {
    fn new(destination: &'a mut crate::jet_testing_history_foundation::HistoryRng) -> Self {
        let rng = std::mem::replace(destination, crate::jet_testing_history_foundation::HistoryRng::new(0));
        Self { destination, value: JetHistoryRng(std::rc::Rc::new(std::cell::RefCell::new(Some(rng)))) }
    }
}

impl Drop for JetHistoryRngLoan<'_> {
    fn drop(&mut self) {
        if let Some(rng) = self.value.0.borrow_mut().take() {
            *self.destination = rng;
        }
    }
}

#[derive(Default)]
struct JetHistoryLocalTicket(std::rc::Rc<()>);
impl Clone for JetHistoryLocalTicket {
    fn clone(&self) -> Self {
        let ticket = Self::default();
        JET_HISTORY_CLONED_LIFE.with(|life| *life.borrow_mut() = Some(ticket.0.clone()));
        ticket
    }
}

// Cloning an erased Fn keeps its concrete closure type. The generated closure
// owns one ticket; capture environments are shared, so cloning them cannot
// register a nested callable's ticket as this allocation's lifetime.
fn jet_history_clone_callable<F: Clone>(value: &F) -> (F, Option<std::rc::Rc<()>>) {
    struct Scope(Option<std::rc::Rc<()>>);
    impl Drop for Scope {
        fn drop(&mut self) {
            JET_HISTORY_CLONED_LIFE.with(|life| *life.borrow_mut() = self.0.take());
        }
    }
    let _scope = Scope(JET_HISTORY_CLONED_LIFE.with(|life| life.borrow_mut().take()));
    let value = value.clone();
    let life = JET_HISTORY_CLONED_LIFE.with(|life| life.borrow_mut().take());
    (value, life)
}
#[derive(Default)]
struct JetHistorySendTicket(std::sync::Arc<()>);
impl Clone for JetHistorySendTicket {
    fn clone(&self) -> Self { Self::default() }
}

thread_local! {
    static JET_HISTORY_CLONED_LIFE: std::cell::RefCell<Option<std::rc::Rc<()>>> = const { std::cell::RefCell::new(None) };
    static JET_HISTORY_CALLBACKS: std::cell::RefCell<std::collections::BTreeMap<usize, (std::rc::Weak<()>, JetHistoryWeakReader)>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    static JET_HISTORY_CAPTURE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
static JET_HISTORY_SEND_CALLBACKS: std::sync::Mutex<std::collections::BTreeMap<usize, (std::sync::Weak<()>, JetHistoryWeakSendReader)>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

// The callable owns its reader and environment. These weak indexes retain no
// captures and never use an address as artifact identity. Moving a Box or
// cloning a SendFn Arc leaves its allocation (and this lookup) intact.
// ponytail: prune dead weak rows at registration; use drop tickets if peak
// simultaneous callable count makes the index scan material.
fn jet_history_register_callback<'a, F: ?Sized + 'a>(
    callback: Box<F>,
    reader: &std::rc::Rc<JetHistoryCaptureReader<'a>>,
    life: &std::rc::Rc<()>,
) -> Box<F> {
    // SAFETY: generated callbacks own this typed reader and their allocation
    // ticket. Lookup borrows the registered callable before invoking a reader.
    let reader = unsafe { JetHistoryWeakReader::new(reader) };
    jet_history_register_callback_reader(callback, reader, life)
}

fn jet_history_register_callback_reader<F: ?Sized>(
    callback: Box<F>,
    reader: JetHistoryWeakReader,
    life: &std::rc::Rc<()>,
) -> Box<F> {
    let key = callback.as_ref() as *const F as *const () as usize;
    JET_HISTORY_CALLBACKS.with(|callbacks| {
        let mut callbacks = callbacks.borrow_mut();
        callbacks.retain(|_, (life, _)| life.strong_count() != 0);
        callbacks.insert(key, (std::rc::Rc::downgrade(life), reader));
    });
    callback
}

fn jet_history_register_send_callback<'a, F: ?Sized + 'a>(
    callback: std::sync::Arc<F>,
    reader: &std::sync::Arc<JetHistorySendCaptureReader<'a>>,
    life: &std::sync::Arc<()>,
) -> std::sync::Arc<F> {
    let key = callback.as_ref() as *const F as *const () as usize;
    let mut callbacks = JET_HISTORY_SEND_CALLBACKS.lock().unwrap_or_else(|error| error.into_inner());
    callbacks.retain(|_, (life, _)| life.strong_count() != 0);
    // SAFETY: the SendFn owns its typed, Send + Sync reader. A lookup borrows
    // that same Arc allocation until encoding completes.
    callbacks.insert(key, (std::sync::Arc::downgrade(life), unsafe { JetHistoryWeakSendReader::new(reader) }));
    callback
}

fn jet_history_callback_captures<F: ?Sized>(callback: &F) -> Result<jet_std::DataTree, String> {
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            JET_HISTORY_CAPTURE_DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
    }
    JET_HISTORY_CAPTURE_DEPTH.with(|depth| {
        if depth.get() >= 64 {
            return Err("history callback captures are cyclic or exceed the nesting bound".to_string());
        }
        depth.set(depth.get() + 1);
        Ok(())
    })?;
    let _depth = Depth;
    let key = callback as *const F as *const () as usize;
    let local = JET_HISTORY_CALLBACKS.with(|callbacks| {
        callbacks.borrow().get(&key).and_then(|(life, reader)| {
                        life.upgrade().map(|_| reader.clone())
                    })
    });
    if let Some(reader) = local {
        // SAFETY: callback is borrowed for this entire call, and its ticket
        // identifies the exact live allocation that owns the typed reader.
        return unsafe { reader.read() }.ok_or_else(|| "history callback has no live checked metadata".to_string())?;
    }
    let reader = JET_HISTORY_SEND_CALLBACKS.lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key).and_then(|(life, reader)| life.upgrade().map(|_| reader.clone()));
    let reader = reader.ok_or_else(|| "history callback has no live checked metadata".to_string())?;
    // SAFETY: the borrowed SendFn keeps its typed environment and reader alive.
    unsafe { reader.read() }.ok_or_else(|| "history callback has no live checked metadata".to_string())?
}

fn jet_history_callback_fingerprint<F: ?Sized>(callback: &F) -> Result<String, String> {
    let metadata = jet_history_callback_captures(callback)?;
    let identity = metadata.get("function_identity")?.as_str()?;
    let captures = metadata.get("captures")?;
    let bytes = jet_std::render_datatree_json(captures, false, 0).into_bytes();
    crate::jet_testing_history_foundation::history_callback_value_fingerprint(identity, &bytes)
        .map_err(|error| error.to_string())
}

fn jet_history_data_tree_capture(value: &jet_std::DataTree, depth: usize) -> Result<jet_std::DataTree, String> {
    use jet_std::DataTree as Tree;
    if depth >= 64 {
        return Err("history capture exceeds the nesting bound".to_string());
    }
    let (kind, value) = match value {
        Tree::Null => ("null", Tree::Null),
        Tree::Bool(value) => ("bool", Tree::Bool(*value)),
        Tree::Int(value) => ("int", Tree::Text(value.to_string())),
        Tree::Float(value) => ("float", Tree::Text(value.to_bits().to_string())),
        Tree::Number(value) => ("number", Tree::Text(value.clone())),
        Tree::Text(value) => ("text", Tree::Text(value.clone())),
        Tree::TypedText(value) => ("typed_text", Tree::Text(value.clone())),
        Tree::Bytes(value) => ("bytes", Tree::Array(value.iter().map(|value| Tree::Int(i64::from(*value))).collect())),
        Tree::Array(values) => ("array", Tree::Array(values.iter().map(|value| {
            jet_history_data_tree_capture(value, depth + 1)
        }).collect::<Result<_, _>>()?)),
        Tree::Object(fields) => ("object", Tree::Object(fields.iter().map(|(name, value)| {
            Ok((name.clone(), jet_history_data_tree_capture(value, depth + 1)?))
        }).collect::<Result<_, String>>()?)),
    };
    Ok(Tree::Array(vec![Tree::Text(kind.to_string()), value]))
}

fn jet_testing_history_observation(
    value: &jet_std::DataTree,
) -> crate::jet_testing_comparison_foundation::ComparisonObservation {
    crate::jet_testing_comparison_foundation::ComparisonObservation::value(
        jet_std::render_datatree_json(value, false, 0),
    )
}

fn jet_testing_history_apply<Input>(
    callback: &dyn Fn(Input) -> jet_std::DataTree,
    input: Input,
    role: &str,
) -> Result<jet_std::DataTree, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(input))).map_err(|_| {
        format!("{role} callback crashed while replaying a typed history")
    })
}

pub(crate) fn jet_testing_history_rng_next_u64(
    rng: &JetHistoryRng,
) -> u64 {
    rng.0.borrow_mut().as_mut().expect("history RNG scope has ended").next_u64()
}

pub(crate) fn jet_testing_history_rng_below(
    rng: &JetHistoryRng,
    bound: u64,
) -> u64 {
    rng.0.borrow_mut().as_mut().expect("history RNG scope has ended").below(bound)
}

fn jet_testing_history_execute<Command>(
    model: &dyn Fn(Vec<Command>) -> jet_std::DataTree,
    actual: &dyn Fn(Vec<Command>) -> jet_std::DataTree,
    observe: &dyn Fn(jet_std::DataTree) -> jet_std::DataTree,
    commands: &[Command],
) -> Result<
    (
        jet_std::DataTree,
        jet_std::DataTree,
        crate::jet_testing_comparison_foundation::ComparisonObservation,
        crate::jet_testing_comparison_foundation::ComparisonObservation,
    ),
    String,
>
where
    Command: Clone,
{
    let reference_value =
        jet_testing_history_apply(model, commands.to_vec(), "model")?;
    let candidate_value =
        jet_testing_history_apply(actual, commands.to_vec(), "actual")?;
    let reference_observed =
        jet_testing_history_apply(observe, reference_value.clone(), "observe")
            .map(|value| jet_testing_history_observation(&value))?;
    let candidate_observed =
        jet_testing_history_apply(observe, candidate_value.clone(), "observe")
            .map(|value| jet_testing_history_observation(&value))?;
    Ok((
        reference_value,
        candidate_value,
        reference_observed,
        candidate_observed,
    ))
}
fn jet_testing_history_record(
    record: crate::jet_testing_comparison_foundation::ComparisonRecord,
    inputs: Vec<jet_std::DataTree>,
    reference: Vec<jet_std::DataTree>,
    candidate: Vec<jet_std::DataTree>,
    seed: Option<i64>,
    provenance: Option<&crate::jet_testing_history_foundation::HistoryProvenance>,
) -> jet_std::JetTestComparison {
    let (source, tool, target) = provenance
        .map(|provenance| {
            (
                provenance.source.clone(),
                provenance.tool.clone(),
                provenance.target.clone(),
            )
        })
        .unwrap_or_else(|| (String::new(), String::new(), String::new()));
    jet_std::JetTestComparison {
        status: record.status.as_str().to_string(),
        relation: record.relation.as_str().to_string(),
        source,
        tool,
        target,
        seed,
        case_ids: record
            .samples
            .iter()
            .map(|sample| sample.identity.case_id.clone())
            .collect(),
        inputs,
        reference,
        candidate,
        first_difference: record
            .first_difference
            .map(|index| index as i64)
            .unwrap_or(-1),
        reason: record
            .reason
            .unwrap_or_else(|| "history comparison has no reason".to_string()),
        universal_proof: record.universal_proof,
    }
}
pub(crate) fn jet_testing_histories<Command>(
    seed: i64,
    cases: i64,
    strategy: Option<JetHistoryStrategy<Command>>,
    model: Box<dyn Fn(Vec<Command>) -> jet_std::DataTree>,
    actual: Box<dyn Fn(Vec<Command>) -> jet_std::DataTree>,
    observe: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>,
) -> Result<jet_std::JetTestComparison, String>
where
    Command: crate::jet_testing_history_foundation::HistoryCommand + 'static,
{
    jet_testing_histories_with_provenance(
        seed,
        cases,
        strategy,
        model,
        actual,
        observe,
        None,
    )
}

/// Compiler-private entrypoint used when the checked caller carries selected
/// artifact/callback provenance outside the six Jet-visible history values.
pub(crate) fn jet_testing_histories_with_provenance<Command>(
    seed: i64,
    cases: i64,
    strategy: Option<JetHistoryStrategy<Command>>,
    model: Box<dyn Fn(Vec<Command>) -> jet_std::DataTree>,
    actual: Box<dyn Fn(Vec<Command>) -> jet_std::DataTree>,
    observe: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>,
    provenance: Option<crate::jet_testing_history_foundation::HistoryProvenance>,
) -> Result<jet_std::JetTestComparison, String>
where
    Command: crate::jet_testing_history_foundation::HistoryCommand + 'static,
{
    let provenance = provenance.and_then(|base| {
        let bind = || -> Result<crate::jet_testing_history_foundation::HistoryProvenance, String> {
            let mut callbacks = vec![
                ("model", jet_history_callback_fingerprint(model.as_ref())?),
                ("actual", jet_history_callback_fingerprint(actual.as_ref())?),
                ("observe", jet_history_callback_fingerprint(observe.as_ref())?),
            ];
            if let Some(strategy) = strategy.as_ref() {
                callbacks.extend([
                    ("generate", jet_history_callback_fingerprint(strategy.generate.as_ref())?),
                    ("rebuild", jet_history_callback_fingerprint(strategy.rebuild.as_ref())?),
                    ("valid", jet_history_callback_fingerprint(strategy.valid.as_ref())?),
                ]);
            } else {
                let identity = format!("derived:{}", Command::history_command_type());
                callbacks.push(("strategy",
                    crate::jet_testing_history_foundation::history_callback_value_fingerprint(
                        &identity, b"[]",
                    ).map_err(|error| error.to_string())?,
                ));
            }
            let callbacks = callbacks.iter().map(|(role, identity)| (*role, identity.as_str()))
                .collect::<Vec<_>>();
            base.bind_callbacks(&callbacks).map_err(|error| error.to_string())
        };
        bind().ok()
    });
    match strategy {
        Some(strategy) => {
            let JetHistoryStrategy { generate, rebuild, valid, bounds, distributions } = strategy;
            let strategy = crate::jet_testing_history_foundation::HistoryStrategy {
                generate: Box::new(move |rng, index, steps| {
                    let loan = JetHistoryRngLoan::new(rng);
                    generate(loan.value.clone(), index, steps).ok()
                }),
                rebuild: Box::new(move |case| rebuild(case.clone()).ok()),
                valid: Box::new(move |case| valid(case.clone())),
                bounds,
                distributions,
            };
            jet_testing_histories_result_with_strategy(
                seed, cases, model, actual, observe, strategy, provenance,
            )
        }
        None => {
            let strategy =
                <Command as crate::jet_testing_history_foundation::HistoryCommand>::history_strategy();
            jet_testing_histories_result_with_strategy(
                seed, cases, model, actual, observe, strategy, provenance,
            )
        }
    }
}

pub(crate) fn jet_testing_histories_schema(
    seed: i64,
    cases: i64,
    model: Box<dyn Fn(Vec<jet_std::DataTree>) -> jet_std::DataTree>,
    actual: Box<dyn Fn(Vec<jet_std::DataTree>) -> jet_std::DataTree>,
    observe: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>,
    _command_type: &str,
) -> Result<jet_std::JetTestComparison, String> {
    jet_testing_histories::<jet_std::DataTree>(seed, cases, None, model, actual, observe)
}

fn jet_testing_histories_result_with_strategy<Command, Strategy>(
    seed: i64,
    cases: i64,
    model: Box<dyn Fn(Vec<Command>) -> jet_std::DataTree>,
    actual: Box<dyn Fn(Vec<Command>) -> jet_std::DataTree>,
    observe: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>,
    strategy: Strategy,
    provenance: Option<crate::jet_testing_history_foundation::HistoryProvenance>,
) -> Result<jet_std::JetTestComparison, String>
where
    Command: crate::jet_testing_history_foundation::HistoryCommand + 'static,
    Strategy: crate::jet_testing_history_foundation::HistoryStrategyBehavior<Command>,
{
    let cases = usize::try_from(cases).map_err(|_| "history case bound is invalid".to_string())?;
    let seed = u64::try_from(seed).map_err(|_| "history seed is invalid".to_string())?;
    let bounds = strategy.bounds();
    let relation =
        crate::jet_testing_comparison_foundation::ObservationRelation::TypedEquality;
    let Some(provenance) = provenance else {
        let record = crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
            relation,
            crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable,
            "history provenance is unavailable",
        );
        return Ok(jet_testing_history_record(
            record,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(seed as i64),
            None,
        ));
    };
    if let Some(reason) = strategy.unsupported_reason() {
        let record = crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
            relation.clone(),
            crate::jet_testing_comparison_foundation::ComparisonStatus::Unsupported,
            reason,
        );
        return Ok(jet_testing_history_record(
            record,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(seed as i64),
            Some(&provenance),
        ));
    }
    let config = crate::jet_testing_history_foundation::HistoryConfig::new(
        seed,
        cases,
        bounds,
        provenance.source.clone(),
        provenance.tool.clone(),
        provenance.target.clone(),
        relation.clone(),
        crate::jet_testing_history_foundation::OracleDeclaration::independent(
            "testing.histories.model",
        ),
    )
    .with_command_type(strategy.command_type());
    let config = strategy
        .distributions()
        .into_iter()
        .fold(config, |config, distribution| config.with_distribution(distribution));
    let runner =
        crate::jet_testing_history_foundation::HistoryRunner::new(config).map_err(|error| error.to_string())?;

    let mut inputs = Vec::new();
    let mut reference_values = Vec::new();
    let mut candidate_values = Vec::new();
    let mut samples = Vec::new();
    let run = runner
        .run_strategy::<Command, _, _>(&strategy, |typed| {
            let case = &typed.case;
            let input = match jet_std::parse_json_datatree(&case.json()) {
                Ok(input) => input,
                Err(error) => {
                    return crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
                        relation.clone(),
                        crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable,
                        format!("history input could not be decoded: {}", error.reason),
                    );
                }
            };
            let (
                reference_value,
                candidate_value,
                reference_observed,
                candidate_observed,
            ) = match jet_testing_history_execute(
                model.as_ref(),
                actual.as_ref(),
                observe.as_ref(),
                &typed.commands,
            ) {
                Ok(values) => values,
                Err(reason) => {
                    return crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
                        relation.clone(),
                        crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable,
                        reason,
                    );
                }
            };
            let (
                _reference_replay_value,
                _candidate_replay_value,
                reference_replay,
                candidate_replay,
            ) = match jet_testing_history_execute(
                model.as_ref(),
                actual.as_ref(),
                observe.as_ref(),
                &typed.commands,
            ) {
                Ok(values) => values,
                Err(reason) => {
                    return crate::jet_testing_comparison_foundation::ComparisonRecord::terminal(
                        relation.clone(),
                        crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable,
                        reason,
                    );
                }
            };
            let mut identity = crate::jet_testing_comparison_foundation::ComparisonIdentity::new(
                case.case_id.clone(),
                case.input_id(),
                provenance.source.clone(),
                provenance.tool.clone(),
                provenance.target.clone(),
            );
            identity.seed = Some(case.seed);
            let sample = crate::jet_testing_comparison_foundation::ComparisonSample::new(
                identity,
                reference_observed,
                candidate_observed,
            )
            .with_replays(reference_replay, candidate_replay);
            inputs.push(input);
            reference_values.push(reference_value);
            candidate_values.push(candidate_value);
            samples.push(sample.clone());
            crate::jet_testing_comparison_foundation::compare_samples(relation.clone(), [sample])
        })
        .map_err(|error| error.to_string())?;
    if let Some(artifact) = run.failure {
        return Err(format!("history-artifact {}", artifact.json()));
    }
    let mut record = crate::jet_testing_comparison_foundation::compare_samples_with_discarded(
        relation,
        samples,
        run.discarded_cases,
    );
    if run.explored_cases == 0 {
        record.status =
            crate::jet_testing_comparison_foundation::ComparisonStatus::Unavailable;
        record.reason = Some("no history case produced an observation".to_string());
    }
    Ok(jet_testing_history_record(
        record,
        inputs,
        reference_values,
        candidate_values,
        Some(seed as i64),
        Some(&provenance),
    ))
}
#[cfg(target_arch = "wasm32")]
mod jet_testing_history_web {
    use super::{jet_testing_history_observation, jet_testing_history_record};
    use crate::jet_std;
    use crate::jet_testing_history_foundation::{
        HistoryBounds, HistoryCase, HistoryDistribution, HistoryOperation, HistoryPrecondition,
        HistoryProvenance, HistoryRng, HistoryScheduleChoice, HistoryStrategyBehavior, HistoryValue,
        TypedHistoryCase,
    };
    use crate::jet_testing_comparison_foundation::{
        compare_samples, compare_samples_with_discarded, ComparisonIdentity,
        ComparisonRecord, ComparisonSample, ComparisonStatus, ObservationRelation,
    };
    use std::cell::RefCell;

    type Tree = jet_std::DataTree;
    const MAX_WIRE_BYTES: usize = 8 * 1024 * 1024;

    thread_local! {
        static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static RNGS: RefCell<Vec<Option<HistoryRng>>> = const { RefCell::new(Vec::new()) };
        static STRATEGIES: RefCell<Vec<(&'static str, fn(&Tree) -> Result<jet_std::JetTestComparison, String>)>> =
            const { RefCell::new(Vec::new()) };
    }

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn jet_testing_history_web_callback(callback: u32, pointer: u32, length: u32) -> u64;
    }

    fn history_rng_slot(handle: u64) -> Option<usize> {
        let slot = handle.checked_sub(1)?;
        usize::try_from(slot).ok()
    }

    fn with_history_rng<R>(handle: u64, operation: impl FnOnce(&mut HistoryRng) -> R) -> Option<R> {
        let slot = history_rng_slot(handle)?;
        RNGS.with(|cell| {
            let mut rngs = cell.borrow_mut();
            let rng = rngs.get_mut(slot)?.as_mut()?;
            Some(operation(rng))
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_rng_alloc(state: u64) -> u64 {
        RNGS.with(|cell| {
            let mut rngs = cell.borrow_mut();
            if let Some((index, slot)) = rngs.iter_mut().enumerate().find(|(_, slot)| slot.is_none()) {
                *slot = Some(HistoryRng::new(state));
                return u64::try_from(index + 1).unwrap_or(0);
            }
            rngs.push(Some(HistoryRng::new(state)));
            u64::try_from(rngs.len()).unwrap_or(0)
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_rng_release(handle: u64) {
        let Some(slot) = history_rng_slot(handle) else {
            return;
        };
        RNGS.with(|cell| {
            if let Some(rng) = cell.borrow_mut().get_mut(slot) {
                *rng = None;
            }
        });
    }
    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_rng_state(handle: u64) -> u64 {
        with_history_rng(handle, |rng| rng.state()).unwrap_or(0)
    }


    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_rng_next_u64(handle: u64) -> u64 {
        with_history_rng(handle, HistoryRng::next_u64).unwrap_or(0)
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_rng_below(handle: u64, bound: u64) -> u64 {
        with_history_rng(handle, |rng| rng.below(bound)).unwrap_or(0)
    }

    fn field<'a>(value: &'a Tree, name: &str) -> Option<&'a Tree> {
        value.get_opt(name)
    }

    fn field_any<'a>(value: &'a Tree, names: &[&str]) -> Option<&'a Tree> {
        names.iter().find_map(|name| field(value, name))
    }

    fn integer(value: &Tree, name: &str) -> Result<i64, String> {
        match value {
            Tree::Int(value) => Ok(*value),
            Tree::Number(value) => value
                .parse::<i64>()
                .map_err(|_| format!("{name} must be an exact integer")),
            _ => Err(format!("{name} must be an exact integer")),
        }
    }

    fn unsigned(value: &Tree, name: &str) -> Result<u64, String> {
        match value {
            Tree::Int(value) => u64::try_from(*value)
                .map_err(|_| format!("{name} must be a non-negative exact integer")),
            Tree::Number(value) => value
                .parse::<u64>()
                .map_err(|_| format!("{name} must be a non-negative exact integer")),
            _ => Err(format!("{name} must be an exact integer")),
        }
    }

    fn required_integer(root: &Tree, name: &str) -> Result<i64, String> {
        integer(
            field(root, name).ok_or_else(|| format!("history wire field `{name}` is missing"))?,
            name,
        )
    }

    fn required_unsigned(root: &Tree, name: &str) -> Result<u64, String> {
        unsigned(
            field(root, name).ok_or_else(|| format!("history wire field `{name}` is missing"))?,
            name,
        )
    }

    fn required_text(root: &Tree, name: &str) -> Result<String, String> {
        match field(root, name) {
            Some(Tree::Text(value) | Tree::TypedText(value)) => Ok(value.clone()),
            Some(_) => Err(format!("{name} must be text")),
            None => Err(format!("history wire field `{name}` is missing")),
        }
    }

    fn required_array<'a>(root: &'a Tree, name: &str) -> Result<&'a [Tree], String> {
        match field(root, name) {
            Some(Tree::Array(values)) => Ok(values),
            Some(_) => Err(format!("{name} must be an array")),
            None => Err(format!("history wire field `{name}` is missing")),
        }
    }

    fn callback_id(root: &Tree, name: &str) -> Result<u32, String> {
        u32::try_from(required_integer(root, name)?)
            .map_err(|_| format!("{name} is not a valid callback id"))
    }
    fn object(entries: Vec<(&str, Tree)>) -> Tree {
        Tree::Object(
            entries
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        )
    }

    fn integer_tree(value: i64) -> Tree {
        Tree::Int(value)
    }

    fn unsigned_tree(value: u64) -> Tree {
        Tree::Number(value.to_string())
    }

    fn id_tree(value: u32) -> Tree {
        object(vec![("value", integer_tree(i64::from(value)))])
    }

    fn option_id_tree(value: Option<u32>) -> Tree {
        match value {
            Some(value) => object(vec![("tag", Tree::Text("Some".to_string())), ("values", Tree::Array(vec![id_tree(value)]))]),
            None => object(vec![("tag", Tree::Text("None".to_string())), ("values", Tree::Array(Vec::new()))]),
        }
    }

    fn history_value_tree(value: &HistoryValue) -> Tree {
        let (tag, values) = match value {
            HistoryValue::Integer(value) => ("Integer", vec![integer_tree(*value)]),
            HistoryValue::Boolean(value) => ("Boolean", vec![Tree::Bool(*value)]),
            HistoryValue::Text(value) => ("Text", vec![Tree::Text(value.clone())]),
            HistoryValue::Handle(value) => ("Handle", vec![id_tree(value.value)]),
            HistoryValue::Redacted(value) => ("Redacted", vec![Tree::Text(value.clone())]),
        };
        object(vec![("tag", Tree::Text(tag.to_string())), ("values", Tree::Array(values))])
    }

    fn history_precondition_tree(value: &HistoryPrecondition) -> Tree {
        let (tag, values) = match value {
            HistoryPrecondition::HandleLive(handle) => ("HandleLive", vec![id_tree(handle.value)]),
            HistoryPrecondition::HandleState { handle, state } => (
                "HandleState",
                vec![id_tree(handle.value), Tree::Text(state.clone())],
            ),
            HistoryPrecondition::TaskCompleted(task) => ("TaskCompleted", vec![id_tree(task.value)]),
            HistoryPrecondition::EventAvailable(event) => ("EventAvailable", vec![id_tree(event.value)]),
        };
        object(vec![("tag", Tree::Text(tag.to_string())), ("values", Tree::Array(values))])
    }

    fn history_operation_tree(operation: &HistoryOperation) -> Tree {
        object(vec![
            ("index", unsigned_tree(u64::from(operation.index))),
            ("name", Tree::Text(operation.name.clone())),
            (
                "arguments",
                Tree::Array(operation.arguments.iter().map(history_value_tree).collect()),
            ),
            (
                "creates",
                Tree::Array(operation.creates.iter().map(|value| id_tree(value.value)).collect()),
            ),
            (
                "consumes",
                Tree::Array(operation.consumes.iter().map(|value| id_tree(value.value)).collect()),
            ),
            (
                "preconditions",
                Tree::Array(
                    operation
                        .preconditions
                        .iter()
                        .map(history_precondition_tree)
                        .collect(),
                ),
            ),
            (
                "depends_on",
                Tree::Array(
                    operation
                        .depends_on
                        .iter()
                        .map(|value| unsigned_tree(u64::from(*value)))
                        .collect(),
                ),
            ),
            ("task", option_id_tree(operation.task.map(|value| value.value))),
            ("event", option_id_tree(operation.event.map(|value| value.value))),
        ])
    }

    fn history_schedule_tree(choice: &HistoryScheduleChoice) -> Tree {
        object(vec![
            ("operation", unsigned_tree(u64::from(choice.operation))),
            ("task", option_id_tree(choice.task.map(|value| value.value))),
            ("event", option_id_tree(choice.event.map(|value| value.value))),
            ("choice", Tree::Text(choice.choice.clone())),
        ])
    }

    fn history_case_tree(case: &HistoryCase) -> Tree {
        object(vec![
            ("case_id", Tree::Text(case.case_id.clone())),
            ("seed", unsigned_tree(case.seed)),
            (
                "operations",
                Tree::Array(case.operations.iter().map(history_operation_tree).collect()),
            ),
            (
                "schedule",
                Tree::Array(case.schedule.iter().map(history_schedule_tree).collect()),
            ),
        ])
    }

    fn typed_history_case_tree(case: &TypedHistoryCase<Tree>) -> Tree {
        object(vec![
            ("case", history_case_tree(&case.case)),
            ("commands", Tree::Array(case.commands.clone())),
        ])
    }

    fn required_any<'a>(root: &'a Tree, names: &[&str], label: &str) -> Result<&'a Tree, String> {
        field_any(root, names).ok_or_else(|| format!("{label} is missing"))
    }

    fn required_text_any(root: &Tree, names: &[&str], label: &str) -> Result<String, String> {
        match required_any(root, names, label)? {
            Tree::Text(value) | Tree::TypedText(value) => Ok(value.clone()),
            _ => Err(format!("{label} must be text")),
        }
    }

    fn unsigned_any(root: &Tree, names: &[&str], label: &str) -> Result<u64, String> {
        unsigned(required_any(root, names, label)?, label)
    }

    fn u32_any(root: &Tree, names: &[&str], label: &str) -> Result<u32, String> {
        u32::try_from(unsigned_any(root, names, label)?)
            .map_err(|_| format!("{label} is outside the u32 range"))
    }

    fn array_any<'a>(root: &'a Tree, names: &[&str], label: &str) -> Result<&'a [Tree], String> {
        match required_any(root, names, label)? {
            Tree::Array(values) => Ok(values),
            _ => Err(format!("{label} must be an array")),
        }
    }

    fn history_id(value: &Tree, label: &str) -> Result<u32, String> {
        match value {
            Tree::Object(_) if field(value, "value").is_some() => u32_any(value, &["value"], label),
            _ => u32::try_from(unsigned(value, label)?)
                .map_err(|_| format!("{label} is outside the u32 range")),
        }
    }

    fn tagged<'a>(value: &'a Tree, label: &str) -> Result<(&'a str, &'a [Tree]), String> {
        let tag = match field(value, "tag") {
            Some(Tree::Text(tag) | Tree::TypedText(tag)) => tag.as_str(),
            _ => return Err(format!("{label}.tag must be text")),
        };
        let values = match required_any(value, &["values"], label)? {
            Tree::Array(values) => values.as_slice(),
            _ => return Err(format!("{label}.values must be an array")),
        };
        Ok((tag, values))
    }

    fn history_value(value: &Tree, label: &str) -> Result<HistoryValue, String> {
        let Tree::Object(_) = value else {
            return match value {
                Tree::Int(value) => Ok(HistoryValue::Integer(*value)),
                Tree::Number(value) => value
                    .parse::<i64>()
                    .map(HistoryValue::Integer)
                    .map_err(|_| format!("{label} integer is out of range")),
                Tree::Bool(value) => Ok(HistoryValue::Boolean(*value)),
                Tree::Text(value) | Tree::TypedText(value) => Ok(HistoryValue::Text(value.clone())),
                _ => Err(format!("{label} is not a HistoryValue")),
            };
        };
        let (tag, values) = tagged(value, label)?;
        match (tag, values) {
            ("Integer", [value]) => Ok(HistoryValue::Integer(integer(value, label)?)),
            ("Boolean", [Tree::Bool(value)]) => Ok(HistoryValue::Boolean(*value)),
            ("Text", [Tree::Text(value) | Tree::TypedText(value)]) => Ok(HistoryValue::Text(value.clone())),
            ("Handle", [value]) => Ok(HistoryValue::Handle(crate::jet_testing_history_foundation::HandleId {
                value: history_id(value, label)?,
            })),
            ("Redacted", [Tree::Text(value) | Tree::TypedText(value)]) => Ok(HistoryValue::Redacted(value.clone())),
            _ => Err(format!("{label} has an invalid HistoryValue variant")),
        }
    }

    fn option_id(value: &Tree, label: &str) -> Result<Option<u32>, String> {
        if matches!(value, Tree::Null) {
            return Ok(None);
        }
        if let Tree::Object(_) = value {
            if let Some(Tree::Text(tag) | Tree::TypedText(tag)) = field(value, "tag") {
                let values = match required_any(value, &["values"], label)? {
                    Tree::Array(values) => values,
                    _ => return Err(format!("{label}.values must be an array")),
                };
                return match (tag.as_str(), values.as_slice()) {
                    ("None", []) => Ok(None),
                    ("Some", [value]) => history_id(value, label).map(Some),
                    _ => Err(format!("{label} has an invalid option")),
                };
            }
        }
        history_id(value, label).map(Some)
    }

    fn history_precondition(value: &Tree, label: &str) -> Result<HistoryPrecondition, String> {
        if let Tree::Object(_) = value {
            if let Some(Tree::Text(kind) | Tree::TypedText(kind)) = field(value, "kind") {
                return match kind.as_str() {
                    "handle_live" => Ok(HistoryPrecondition::HandleLive(
                        crate::jet_testing_history_foundation::HandleId {
                            value: u32_any(value, &["handle"], label)?,
                        },
                    )),
                    "handle_state" => Ok(HistoryPrecondition::HandleState {
                        handle: crate::jet_testing_history_foundation::HandleId {
                            value: u32_any(value, &["handle"], label)?,
                        },
                        state: required_text_any(value, &["state"], label)?,
                    }),
                    "task_completed" => Ok(HistoryPrecondition::TaskCompleted(
                        crate::jet_testing_history_foundation::TaskId {
                            value: u32_any(value, &["task"], label)?,
                        },
                    )),
                    "event_available" => Ok(HistoryPrecondition::EventAvailable(
                        crate::jet_testing_history_foundation::EventId {
                            value: u32_any(value, &["event"], label)?,
                        },
                    )),
                    _ => Err(format!("{label} has an unknown precondition kind")),
                };
            }
        }
        let (tag, values) = tagged(value, label)?;
        match (tag, values) {
            ("HandleLive", [value]) => Ok(HistoryPrecondition::HandleLive(
                crate::jet_testing_history_foundation::HandleId {
                    value: history_id(value, label)?,
                },
            )),
            ("HandleState", [handle, Tree::Text(state) | Tree::TypedText(state)]) => {
                Ok(HistoryPrecondition::HandleState {
                    handle: crate::jet_testing_history_foundation::HandleId {
                        value: history_id(handle, label)?,
                    },
                    state: state.clone(),
                })
            }
            ("TaskCompleted", [value]) => Ok(HistoryPrecondition::TaskCompleted(
                crate::jet_testing_history_foundation::TaskId {
                    value: history_id(value, label)?,
                },
            )),
            ("EventAvailable", [value]) => Ok(HistoryPrecondition::EventAvailable(
                crate::jet_testing_history_foundation::EventId {
                    value: history_id(value, label)?,
                },
            )),
            _ => Err(format!("{label} has an invalid precondition variant")),
        }
    }

    fn history_operation(value: &Tree, label: &str) -> Result<HistoryOperation, String> {
        let mut operation = HistoryOperation::new(
            u32_any(value, &["index"], &format!("{label}.index"))?,
            required_text_any(value, &["name"], &format!("{label}.name"))?,
        );
        operation.arguments = array_any(value, &["arguments"], &format!("{label}.arguments"))?
            .iter()
            .enumerate()
            .map(|(index, value)| history_value(value, &format!("{label}.arguments[{index}]")))
            .collect::<Result<_, _>>()?;
        operation.creates = array_any(value, &["creates"], &format!("{label}.creates"))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                history_id(value, &format!("{label}.creates[{index}]")).map(
                    |value| crate::jet_testing_history_foundation::HandleId { value },
                )
            })
            .collect::<Result<_, _>>()?;
        operation.consumes = array_any(value, &["consumes"], &format!("{label}.consumes"))?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                history_id(value, &format!("{label}.consumes[{index}]")).map(
                    |value| crate::jet_testing_history_foundation::HandleId { value },
                )
            })
            .collect::<Result<_, _>>()?;
        operation.preconditions = array_any(value, &["preconditions"], &format!("{label}.preconditions"))?
            .iter()
            .enumerate()
            .map(|(index, value)| history_precondition(value, &format!("{label}.preconditions[{index}]")))
            .collect::<Result<_, _>>()?;
        operation.depends_on = array_any(value, &["depends_on", "dependsOn"], &format!("{label}.depends_on"))?
            .iter()
            .enumerate()
            .map(|(index, value)| u32::try_from(unsigned(value, &format!("{label}.depends_on[{index}]"))?)
                .map_err(|_| format!("{label}.depends_on[{index}] is outside the u32 range")))
            .collect::<Result<_, _>>()?;
        operation.task = option_id(
            required_any(value, &["task"], &format!("{label}.task"))?,
            &format!("{label}.task"),
        )?
        .map(|value| crate::jet_testing_history_foundation::TaskId { value });
        operation.event = option_id(
            required_any(value, &["event"], &format!("{label}.event"))?,
            &format!("{label}.event"),
        )?
        .map(|value| crate::jet_testing_history_foundation::EventId { value });
        Ok(operation)
    }

    fn history_schedule(value: &Tree, label: &str) -> Result<HistoryScheduleChoice, String> {
        let mut choice = HistoryScheduleChoice::new(
            u32_any(value, &["operation"], &format!("{label}.operation"))?,
            required_text_any(value, &["choice"], &format!("{label}.choice"))?,
        );
        choice.task = option_id(
            required_any(value, &["task"], &format!("{label}.task"))?,
            &format!("{label}.task"),
        )?
        .map(|value| crate::jet_testing_history_foundation::TaskId { value });
        choice.event = option_id(
            required_any(value, &["event"], &format!("{label}.event"))?,
            &format!("{label}.event"),
        )?
        .map(|value| crate::jet_testing_history_foundation::EventId { value });
        Ok(choice)
    }

    fn history_case(value: &Tree, label: &str) -> Result<HistoryCase, String> {
        let mut case = HistoryCase::new(
            required_text_any(value, &["case_id", "caseId"], &format!("{label}.case_id"))?,
            unsigned_any(value, &["seed"], &format!("{label}.seed"))?,
        );
        case.operations = array_any(value, &["operations"], &format!("{label}.operations"))?
            .iter()
            .enumerate()
            .map(|(index, value)| history_operation(value, &format!("{label}.operations[{index}]")))
            .collect::<Result<_, _>>()?;
        case.schedule = array_any(value, &["schedule"], &format!("{label}.schedule"))?
            .iter()
            .enumerate()
            .map(|(index, value)| history_schedule(value, &format!("{label}.schedule[{index}]")))
            .collect::<Result<_, _>>()?;
        Ok(case)
    }

    fn typed_history_case(value: &Tree, label: &str) -> Result<TypedHistoryCase<Tree>, String> {
        let case = history_case(
            required_any(value, &["case"], &format!("{label}.case"))?,
            &format!("{label}.case"),
        )?;
        let commands = array_any(value, &["commands"], &format!("{label}.commands"))?.to_vec();
        TypedHistoryCase::new(case, commands)
            .ok_or_else(|| format!("{label} command count does not match operation count"))
    }

    fn optional_typed_history_case(
        value: &Tree,
        label: &str,
    ) -> Result<Option<TypedHistoryCase<Tree>>, String> {
        if matches!(value, Tree::Null) {
            return Ok(None);
        }
        if let Tree::Object(_) = value {
            if let Some(Tree::Text(tag) | Tree::TypedText(tag)) = field(value, "tag") {
                let values = match required_any(value, &["values"], label)? {
                    Tree::Array(values) => values,
                    _ => return Err(format!("{label}.values must be an array")),
                };
                return match (tag.as_str(), values.as_slice()) {
                    ("None", []) => Ok(None),
                    ("Some", [value]) => typed_history_case(value, label).map(Some),
                    _ => Err(format!("{label} has an invalid option")),
                };
            }
        }
        typed_history_case(value, label).map(Some)
    }

    fn history_bounds(value: &Tree) -> Result<HistoryBounds, String> {
        Ok(HistoryBounds {
            max_steps: usize::try_from(unsigned_any(value, &["max_steps", "maxSteps"], "strategy.bounds.max_steps")?)
                .map_err(|_| "strategy.bounds.max_steps is outside usize range".to_string())?,
            max_resources: usize::try_from(unsigned_any(value, &["max_resources", "maxResources"], "strategy.bounds.max_resources")?)
                .map_err(|_| "strategy.bounds.max_resources is outside usize range".to_string())?,
            max_shrink_attempts: usize::try_from(unsigned_any(
                value,
                &["max_shrink_attempts", "maxShrinkAttempts"],
                "strategy.bounds.max_shrink_attempts",
            )?)
            .map_err(|_| "strategy.bounds.max_shrink_attempts is outside usize range".to_string())?,
            max_discarded_cases: usize::try_from(unsigned_any(
                value,
                &["max_discarded_cases", "maxDiscardedCases"],
                "strategy.bounds.max_discarded_cases",
            )?)
            .map_err(|_| "strategy.bounds.max_discarded_cases is outside usize range".to_string())?,
        })
    }

    fn history_distributions(value: &Tree) -> Result<Vec<HistoryDistribution>, String> {
        array_any(value, &["distributions"], "strategy.distributions")?
            .iter()
            .enumerate()
            .map(|(index, value)| {
                Ok(HistoryDistribution {
                    operation: required_text_any(
                        value,
                        &["operation"],
                        &format!("strategy.distributions[{index}].operation"),
                    )?,
                    weight: usize::try_from(unsigned_any(
                        value,
                        &["weight"],
                        &format!("strategy.distributions[{index}].weight"),
                    )?)
                    .map_err(|_| format!("strategy.distributions[{index}].weight is outside usize range"))?,
                })
            })
            .collect()
    }

    struct WebHistoryStrategy {
        command_type: String,
        generate: u32,
        rebuild: u32,
        valid: u32,
        bounds: HistoryBounds,
        distributions: Vec<HistoryDistribution>,
    }

    impl WebHistoryStrategy {
        fn from_wire(command_type: String, value: &Tree) -> Result<Self, String> {
            let bounds = history_bounds(required_any(value, &["bounds"], "strategy.bounds")?)?;
            let distributions = history_distributions(value)?;
            Ok(Self {
                command_type,
                generate: callback_id(value, "generate")?,
                rebuild: callback_id(value, "rebuild")?,
                valid: callback_id(value, "valid")?,
                bounds,
                distributions,
            })
        }
    }

    impl HistoryStrategyBehavior<Tree> for WebHistoryStrategy {
        fn command_type(&self) -> &str {
            &self.command_type
        }

        fn bounds(&self) -> HistoryBounds {
            self.bounds
        }

        fn distributions(&self) -> Vec<HistoryDistribution> {
            self.distributions.clone()
        }

        fn generate(
            &self,
            rng: &mut HistoryRng,
            case_index: usize,
            max_steps: usize,
        ) -> Option<TypedHistoryCase<Tree>> {
            let handle = jet_testing_history_web_rng_alloc(rng.state());
            if handle == 0 {
                return None;
            }
            let input = object(vec![
                ("rng_handle", unsigned_tree(handle)),
                ("case_index", unsigned_tree(case_index as u64)),
                ("max_steps", unsigned_tree(max_steps as u64)),
            ]);
            let response = callback(self.generate, &input, "strategy.generate");
            let state = jet_testing_history_web_rng_state(handle);
            jet_testing_history_web_rng_release(handle);
            rng.replace_state(state);
            let response = response.ok()?;
            optional_typed_history_case(&response, "strategy.generate.value")
                .ok()
                .flatten()
        }

        fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<Tree>> {
            let response = callback(
                self.rebuild,
                &history_case_tree(case),
                "strategy.rebuild",
            )
            .ok()?;
            optional_typed_history_case(&response, "strategy.rebuild.value")
                .ok()
                .flatten()
        }

        fn valid(&self, case: &TypedHistoryCase<Tree>) -> bool {
            let response = callback(
                self.valid,
                &typed_history_case_tree(case),
                "strategy.valid",
            )
            .ok();
            matches!(response, Some(Tree::Bool(true)))
        }

        fn commands_to_data_tree(&self, commands: &[Tree]) -> Option<Tree> {
            Some(Tree::Array(commands.to_vec()))
        }
    }


    fn set_output(value: Tree) {
        OUTPUT.with(|cell| {
            *cell.borrow_mut() = jet_std::render_datatree_json(&value, false, 0).into_bytes();
        });
    }

    fn result_tree(result: Result<jet_std::JetTestComparison, String>) -> Tree {
        match result {
            Ok(value) => Tree::Object(vec![
                ("tag".to_string(), Tree::Text("Ok".to_string())),
                (
                    "values".to_string(),
                    Tree::Array(vec![comparison_tree(value)]),
                ),
            ]),
            Err(reason) => Tree::Object(vec![
                ("tag".to_string(), Tree::Text("Err".to_string())),
                ("values".to_string(), Tree::Array(vec![Tree::Text(reason)])),
            ]),
        }
    }

    fn option_integer(value: Option<i64>) -> Tree {
        match value {
            Some(value) => Tree::Object(vec![
                ("tag".to_string(), Tree::Text("Some".to_string())),
                (
                    "values".to_string(),
                    Tree::Array(vec![Tree::Int(value)]),
                ),
            ]),
            None => Tree::Object(vec![
                ("tag".to_string(), Tree::Text("None".to_string())),
                ("values".to_string(), Tree::Array(Vec::new())),
            ]),
        }
    }

    fn text_array(values: Vec<String>) -> Tree {
        Tree::Array(values.into_iter().map(Tree::Text).collect())
    }

    fn comparison_tree(value: jet_std::JetTestComparison) -> Tree {
        Tree::Object(vec![
            ("status".to_string(), Tree::Text(value.status)),
            ("relation".to_string(), Tree::Text(value.relation)),
            ("source".to_string(), Tree::Text(value.source)),
            ("tool".to_string(), Tree::Text(value.tool)),
            ("target".to_string(), Tree::Text(value.target)),
            ("seed".to_string(), option_integer(value.seed)),
            ("case_ids".to_string(), text_array(value.case_ids)),
            ("inputs".to_string(), Tree::Array(value.inputs)),
            ("reference".to_string(), Tree::Array(value.reference)),
            ("candidate".to_string(), Tree::Array(value.candidate)),
            (
                "first_difference".to_string(),
                Tree::Int(value.first_difference),
            ),
            ("reason".to_string(), Tree::Text(value.reason)),
            (
                "universal_proof".to_string(),
                Tree::Bool(value.universal_proof),
            ),
        ])
    }

    fn output_is_current(pointer: u32, length: u32) -> bool {
        OUTPUT.with(|cell| {
            let output = cell.borrow();
            pointer != 0
                && pointer as usize == output.as_ptr() as usize
                && usize::try_from(length)
                    .ok()
                    .is_some_and(|length| length <= output.len())
        })
    }

    fn callback(
        callback: u32,
        input: &Tree,
        role: &str,
    ) -> Result<Tree, String> {
        let encoded = jet_std::render_datatree_json(input, false, 0).into_bytes();
        if encoded.len() > MAX_WIRE_BYTES {
            return Err(format!("{role} history callback input exceeds the Web limit"));
        }
        let pointer = u32::try_from(encoded.as_ptr() as usize)
            .map_err(|_| format!("{role} history callback input pointer is invalid"))?;
        let packed = unsafe {
            jet_testing_history_web_callback(
                callback,
                pointer,
                u32::try_from(encoded.len())
                    .map_err(|_| format!("{role} history callback input is too large"))?,
            )
        };
        if packed == 0 {
            return Err(format!("{role} history callback returned no response"));
        }
        let output_pointer = (packed >> 32) as u32;
        let output_length = packed as u32;
        if !output_is_current(output_pointer, output_length) {
            OUTPUT.with(|cell| cell.borrow_mut().clear());
            return Err(format!("{role} history callback returned an invalid response"));
        }
        let parsed = OUTPUT.with(|cell| {
            let output = cell.borrow();
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    output_pointer as usize as *const u8,
                    output_length as usize,
                )
            };
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|value| jet_std::parse_json_typed_datatree(value).ok())
        });
        OUTPUT.with(|cell| cell.borrow_mut().clear());
        let parsed = parsed.ok_or_else(|| format!("{role} callback returned invalid JSON"))?;
        let ok = match field(&parsed, "ok") {
            Some(Tree::Bool(value)) => *value,
            _ => return Err(format!("{role} callback returned an invalid response envelope")),
        };
        if ok {
            field(&parsed, "value")
                .cloned()
                .ok_or_else(|| format!("{role} callback omitted its value"))
        } else {
            match field(&parsed, "reason") {
                Some(Tree::Text(reason) | Tree::TypedText(reason)) => Err(format!(
                    "{role} callback failed: {reason}"
                )),
                _ => Err(format!("{role} callback failed without a reason")),
            }
        }
    }

    fn wire_provenance(root: &Tree) -> Result<Option<HistoryProvenance>, String> {
        let Some(value) = field(root, "provenance") else {
            return Ok(None);
        };
        if matches!(value, Tree::Null) {
            return Ok(None);
        }
        let base = HistoryProvenance {
            source: required_text(value, "source")?,
            tool: required_text(value, "tool")?,
            target: required_text(value, "target")?,
        };
        let rows = field(root, "callback_provenance")
            .ok_or_else(|| "history callback provenance is missing".to_string())?
            .as_array()?;
        let explicit = field(root, "strategy").is_some_and(|value| !matches!(value, Tree::Null));
        let expected: &[&str] = if explicit {
            &["model", "actual", "observe", "generate", "rebuild", "valid"]
        } else {
            &["model", "actual", "observe", "strategy"]
        };
        if rows.len() != expected.len() {
            return Err("history callback provenance has an invalid role count".to_string());
        }
        let mut callbacks = Vec::with_capacity(rows.len());
        for row in rows {
            let role = required_text(row, "role")?;
            if !expected.contains(&role.as_str()) {
                return Err("history callback provenance has an unknown role".to_string());
            }
            let identity = required_text(row, "function_identity")?;
            let captures = field(row, "captures")
                .ok_or_else(|| "history callback captures are missing".to_string())?;
            captures.as_array()?;
            if role == "strategy" && (
                identity != format!("derived:{}", required_text(root, "command_type")?)
                || !captures.as_array()?.is_empty()
            ) {
                return Err("history derived strategy provenance does not match its descriptor".to_string());
            }
            // The typed JSON parser retains exact integer lexemes and ordered
            // nested DataTree values. The emitter's capture type tags travel
            // inside this existing wire tree, never through an f64 conversion.
            let bytes = jet_std::render_datatree_json(captures, false, 0).into_bytes();
            let fingerprint = crate::jet_testing_history_foundation::history_callback_value_fingerprint(
                &identity, &bytes,
            ).map_err(|error| error.to_string())?;
            callbacks.push((role, fingerprint));
        }
        let callbacks = callbacks.iter().map(|(role, identity)| (role.as_str(), identity.as_str()))
            .collect::<Vec<_>>();
        base.bind_callbacks(&callbacks).map(Some).map_err(|error| error.to_string())
    }

    fn command_type_matches(descriptor: &str, name: &str) -> bool {
        descriptor == name
            || descriptor
                .strip_prefix(name)
                .is_some_and(|suffix| suffix.starts_with(':'))
    }

    pub(crate) fn register_strategy(
        command_type: &'static str,
        dispatch: fn(&Tree) -> Result<jet_std::JetTestComparison, String>,
    ) {
        STRATEGIES.with(|strategies| {
            let mut strategies = strategies.borrow_mut();
            if let Some(row) = strategies
                .iter_mut()
                .find(|(name, _)| *name == command_type)
            {
                row.1 = dispatch;
            } else {
                strategies.push((command_type, dispatch));
            }
        });
    }

    fn execute<Command, Strategy>(
        model: u32,
        actual: u32,
        observe: u32,
        strategy: &Strategy,
        commands: &[Command],
    ) -> Result<
        (
            Tree,
            Tree,
            crate::jet_testing_comparison_foundation::ComparisonObservation,
            crate::jet_testing_comparison_foundation::ComparisonObservation,
        ),
        String,
    >
    where
        Command: crate::jet_testing_history_foundation::HistoryCommand + 'static,
        Strategy: crate::jet_testing_history_foundation::HistoryStrategyBehavior<Command>,
    {
        let input = strategy
            .commands_to_data_tree(commands)
            .ok_or_else(|| "history command codec is unavailable on Web".to_string())?;
        let reference_value = callback(model, &input, "model")?;
        let candidate_value = callback(actual, &input, "actual")?;
        let reference_observed = callback(observe, &reference_value, "observe")
            .map(|value| jet_testing_history_observation(&value))?;
        let candidate_observed = callback(observe, &candidate_value, "observe")
            .map(|value| jet_testing_history_observation(&value))?;
        Ok((
            reference_value,
            candidate_value,
            reference_observed,
            candidate_observed,
        ))
    }


    fn run_wire(root: &Tree) -> Result<jet_std::JetTestComparison, String> {
        let command_type = required_text(root, "command_type")?;
        if let Some(strategy) = field(root, "strategy") {
            if !matches!(strategy, Tree::Null) {
                let strategy = WebHistoryStrategy::from_wire(command_type.clone(), strategy)?;
                return run_wire_with_strategy::<Tree, WebHistoryStrategy>(root, strategy);
            }
        }
        let dispatch = STRATEGIES.with(|strategies| {
            strategies
                .borrow()
                .iter()
                .find(|(name, _)| command_type_matches(&command_type, name))
                .map(|(_, dispatch)| *dispatch)
        });
        if let Some(dispatch) = dispatch {
            return dispatch(root);
        }
        if command_type_matches(&command_type, "DataTree") {
            return run_wire_with_strategy::<
                jet_std::DataTree,
                crate::jet_testing_history_foundation::HistorySchemaStrategy,
            >(
                root,
                <jet_std::DataTree as crate::jet_testing_history_foundation::HistoryCommand>::history_strategy(),
            );
        }
        Err(crate::jet_testing_history_foundation::history_unsupported_type_reason(
            &command_type,
        ))
    }

    pub(crate) fn run_wire_with_strategy<Command, Strategy>(
        root: &Tree,
        strategy: Strategy,
    ) -> Result<jet_std::JetTestComparison, String>
    where
        Command: crate::jet_testing_history_foundation::HistoryCommand + 'static,
        Strategy: crate::jet_testing_history_foundation::HistoryStrategyBehavior<Command>,
    {
        let seed = u64::try_from(required_integer(root, "seed")?)
            .map_err(|_| "history seed is invalid".to_string())?;
        let cases = usize::try_from(required_integer(root, "cases")?)
            .map_err(|_| "history case bound is invalid".to_string())?;
        let command_type = required_text(root, "command_type")?;
        if !command_type_matches(&command_type, strategy.command_type()) {
            return Err(format!(
                "history command descriptor `{command_type}` does not match generated strategy `{}`",
                strategy.command_type(),
            ));
        }
        let relation = ObservationRelation::TypedEquality;
        let Some(provenance) = wire_provenance(root)? else {
            let record = ComparisonRecord::terminal(
                relation,
                ComparisonStatus::Unavailable,
                "history provenance is unavailable",
            );
            return Ok(jet_testing_history_record(
                record,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(seed as i64),
                None,
            ));
        };
        let model = callback_id(root, "model")?;
        let actual = callback_id(root, "actual")?;
        let observe = callback_id(root, "observe")?;
        let bounds = strategy.bounds();
        if let Some(reason) = strategy.unsupported_reason() {
            let record = ComparisonRecord::terminal(
                relation.clone(),
                ComparisonStatus::Unsupported,
                reason,
            );
            return Ok(jet_testing_history_record(
                record,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(seed as i64),
                Some(&provenance),
            ));
        }
        let config = crate::jet_testing_history_foundation::HistoryConfig::new(
            seed,
            cases,
            bounds,
            provenance.source.clone(),
            provenance.tool.clone(),
            provenance.target.clone(),
            relation.clone(),
            crate::jet_testing_history_foundation::OracleDeclaration::independent(
                "testing.histories.model",
            ),
        )
        .with_command_type(strategy.command_type());
        let config = strategy
            .distributions()
            .into_iter()
            .fold(config, |config, distribution| config.with_distribution(distribution));
        let runner = crate::jet_testing_history_foundation::HistoryRunner::new(config)
            .map_err(|error| error.to_string())?;
        let mut inputs = Vec::new();
        let mut reference_values = Vec::new();
        let mut candidate_values = Vec::new();
        let mut samples = Vec::new();
        let run = runner
            .run_strategy::<Command, _, _>(&strategy, |typed| {
                let case = &typed.case;
                let input = match jet_std::parse_json_typed_datatree(&case.json()) {
                    Ok(input) => input,
                    Err(error) => {
                        return ComparisonRecord::terminal(
                            relation.clone(),
                            ComparisonStatus::Unavailable,
                            format!("history input could not be decoded: {}", error.reason),
                        );
                    }
                };
                let (
                    reference_value,
                    candidate_value,
                    reference_observed,
                    candidate_observed,
                ) = match execute(model, actual, observe, &strategy, &typed.commands) {
                    Ok(values) => values,
                    Err(reason) => {
                        return ComparisonRecord::terminal(
                            relation.clone(),
                            ComparisonStatus::Unavailable,
                            reason,
                        );
                    }
                };
                let (
                    _reference_replay_value,
                    _candidate_replay_value,
                    reference_replay,
                    candidate_replay,
                ) = match execute(model, actual, observe, &strategy, &typed.commands) {
                    Ok(values) => values,
                    Err(reason) => {
                        return ComparisonRecord::terminal(
                            relation.clone(),
                            ComparisonStatus::Unavailable,
                            reason,
                        );
                    }
                };
                let mut identity = ComparisonIdentity::new(
                    case.case_id.clone(),
                    case.input_id(),
                    provenance.source.clone(),
                    provenance.tool.clone(),
                    provenance.target.clone(),
                );
                identity.seed = Some(case.seed);
                let sample = ComparisonSample::new(
                    identity,
                    reference_observed,
                    candidate_observed,
                )
                .with_replays(reference_replay, candidate_replay);
                inputs.push(input);
                reference_values.push(reference_value);
                candidate_values.push(candidate_value);
                samples.push(sample.clone());
                compare_samples(relation.clone(), [sample])
            })
            .map_err(|error| error.to_string())?;
        if let Some(artifact) = run.failure {
            return Err(format!("history-artifact {}", artifact.json()));
        }
        let mut record =
            compare_samples_with_discarded(relation, samples, run.discarded_cases);
        if run.explored_cases == 0 {
            record.status = ComparisonStatus::Unavailable;
            record.reason = Some("no history case produced an observation".to_string());
        }
        Ok(jet_testing_history_record(
            record,
            inputs,
            reference_values,
            candidate_values,
            Some(seed as i64),
            Some(&provenance),
        ))
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_input_alloc(length: u32) -> u32 {
        let Ok(length) = usize::try_from(length) else {
            return 0;
        };
        if length == 0 || length > MAX_WIRE_BYTES {
            return 0;
        }
        INPUT.with(|cell| {
            let mut input = cell.borrow_mut();
            input.resize(length, 0);
            u32::try_from(input.as_mut_ptr() as usize).unwrap_or(0)
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_input_free(_pointer: u32) {
        INPUT.with(|cell| cell.borrow_mut().clear());
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_output_alloc(length: u32) -> u32 {
        let Ok(length) = usize::try_from(length) else {
            return 0;
        };
        if length == 0 || length > MAX_WIRE_BYTES {
            return 0;
        }
        OUTPUT.with(|cell| {
            let mut output = cell.borrow_mut();
            output.resize(length, 0);
            u32::try_from(output.as_mut_ptr() as usize).unwrap_or(0)
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_output_ptr() -> u32 {
        OUTPUT.with(|cell| u32::try_from(cell.borrow().as_ptr() as usize).unwrap_or(0))
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_output_len() -> u32 {
        OUTPUT.with(|cell| u32::try_from(cell.borrow().len()).unwrap_or(u32::MAX))
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_output_clear() {
        OUTPUT.with(|cell| cell.borrow_mut().clear());
    }

    #[no_mangle]
    pub extern "C" fn jet_testing_history_web_call(pointer: u32, length: u32) -> i32 {
        OUTPUT.with(|cell| cell.borrow_mut().clear());
        let input = INPUT.with(|cell| {
            let input = cell.borrow();
            let valid = pointer != 0
                && pointer as usize == input.as_ptr() as usize
                && usize::try_from(length)
                    .ok()
                    .is_some_and(|length| length <= input.len());
            if !valid {
                return None;
            }
            let bytes = unsafe {
                std::slice::from_raw_parts(pointer as usize as *const u8, length as usize)
            };
            Some(bytes.to_vec())
        });
        let result = input
            .and_then(|bytes| std::str::from_utf8(&bytes).ok().map(str::to_string))
            .and_then(|wire| jet_std::parse_json_typed_datatree(&wire).ok())
            .map_or_else(
                || Err("invalid testing history WebWasm wire JSON".to_string()),
                |wire| run_wire(&wire),
            );
        set_output(result_tree(result));
        0
    }
}

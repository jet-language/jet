use super::*;

/// Checked metadata passed by the MIR evaluator to the comptime history
/// adapter. Derived strategies use the scalar schema rows; explicit strategies
/// retain their lossless typed command values and use only provenance here.
#[derive(Clone, Debug)]
pub struct HistoryCommandSchema {
    pub command_type: String,
    pub variants: Vec<HistoryCommandVariant>,
    pub unsupported_reason: Option<String>,
    pub provenance: Option<HistoryProvenance>,
}

#[derive(Clone, Debug)]
pub struct HistoryCommandVariant {
    pub operation: String,
    pub variant: String,
    pub fields: Vec<(Option<String>, jet_foundation::TestingHistory::HistorySchemaField)>,
}
/// Recover the checked ordinary-enum strategy from the MIR type table.  This
/// is the comptime counterpart of MIRRust's emitted `HistoryCommand` impl, so
/// both tiers use the same variant names and scalar bounds.
pub fn history_command_schema_from_mir(
    program: &jet_foundation::MIR::MirProgram,
    command_type: &jet_foundation::MIR::MirType,
    provenance: Option<HistoryProvenance>,
) -> Option<HistoryCommandSchema> {
    let command_name = command_type
        .nominal_name()
        .map(str::to_string)
        .unwrap_or_else(|| command_type.identity_key());
    let Some(definition) = command_type
        .identity
        .and_then(|identity| program.types.iter().find(|definition| definition.id == identity))
        .or_else(|| {
            program
                .types
                .iter()
                .find(|definition| definition.name == command_name || definition.key == command_name)
        }) else {
        return Some(HistoryCommandSchema {
            command_type: command_name.clone(),
            variants: Vec::new(),
            unsupported_reason: Some(
                jet_foundation::TestingHistory::history_unsupported_type_reason(&command_name),
            ),
            provenance,
        });
    };
    let jet_foundation::MIR::MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
        return Some(HistoryCommandSchema {
            command_type: command_name.clone(),
            variants: Vec::new(),
            unsupported_reason: Some(
                jet_foundation::TestingHistory::history_unsupported_type_reason(&command_name),
            ),
            provenance,
        });
    };
    if !definition.generic_params.is_empty() {
        return Some(HistoryCommandSchema {
            command_type: command_name.clone(),
            variants: Vec::new(),
            unsupported_reason: Some(
                jet_foundation::TestingHistory::history_unsupported_type_reason(&command_name),
            ),
            provenance,
        });
    }
    let mut supported = Vec::new();
    let mut unsupported_variant = None;
    for variant in variants {
        let fields = match &variant.payload {
            jet_foundation::MIR::MirVariantPayload::Unit => Some(Vec::new()),
            jet_foundation::MIR::MirVariantPayload::Single(ty) => {
                history_schema_field(ty).map(|field| vec![(None, field)])
            }
            jet_foundation::MIR::MirVariantPayload::Named(fields) => fields
                .iter()
                .map(|field| {
                    history_schema_field(&field.ty)
                        .map(|field_type| (Some(field.name.clone()), field_type))
                })
                .collect::<Option<Vec<_>>>(),
        };
        let Some(fields) = fields else {
            if unsupported_variant.is_none() {
                unsupported_variant = Some(variant.name.clone());
            }
            continue;
        };
        supported.push(HistoryCommandVariant {
            operation: variant.wire_name.clone(),
            variant: variant.name.clone(),
            fields,
        });
    }
    Some(HistoryCommandSchema {
        command_type: command_name.clone(),
        variants: supported,
        unsupported_reason: unsupported_variant.map(|variant| {
            jet_foundation::TestingHistory::history_unsupported_variant_reason(
                &command_name,
                &variant,
            )
        }),
        provenance,
    })
}

pub(super) fn history_schema_field(
    ty: &jet_foundation::MIR::MirType,
) -> Option<jet_foundation::TestingHistory::HistorySchemaField> {
    use jet_foundation::MIR::MirTypeKind;
    use jet_foundation::TestingHistory::HistorySchemaField;

    match ty.kind() {
        MirTypeKind::Int => Some(HistorySchemaField::Integer { lo: -8, hi: 8 }),
        MirTypeKind::Bool => Some(HistorySchemaField::Boolean),
        MirTypeKind::String => Some(HistorySchemaField::Text),
        MirTypeKind::Char => Some(HistorySchemaField::Char),
        MirTypeKind::IntN { signed, .. } => Some(HistorySchemaField::Integer {
            lo: if *signed { -8 } else { 0 },
            hi: if *signed { 8 } else { 16 },
        }),
        MirTypeKind::InlineRange { base, lo, hi } if lo <= hi => {
            match history_schema_field(base)? {
                HistorySchemaField::Integer { .. } => {
                    Some(HistorySchemaField::Integer { lo: *lo, hi: *hi })
                }
                _ => None,
            }
        }
        MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => {
            history_schema_field(inner)
        }
        _ => None,
    }
}

pub(super) fn comptime_history_data_tree_command(
    value: &jet_foundation::DataTree::DataTree,
) -> Result<CtValue, String> {
    let json = jet_foundation::EncodingJson::render_json(value, false, 0);
    crate::Comptime::JSONInterp::parse_json(&json)
        .map_err(|error| format!("history command could not be encoded: {}", error.message))
}

pub(super) fn comptime_history_enum_command(
    operation: &jet_foundation::TestingHistory::HistoryOperation,
    schema: &HistoryCommandSchema,
) -> Result<CtValue, String> {
    if !operation.creates.is_empty() || !operation.consumes.is_empty() {
        return Err(format!(
            "history operation `{}` carries resources without an executable strategy",
            operation.name
        ));
    }
    let variant = schema
        .variants
        .iter()
        .find(|variant| variant.operation == operation.name)
        .ok_or_else(|| format!("history operation `{}` is not in the checked command schema", operation.name))?;
    if operation.arguments.len() != variant.fields.len() {
        return Err(format!(
            "history operation `{}` has {} argument(s), expected {}",
            operation.name,
            operation.arguments.len(),
            variant.fields.len()
        ));
    }
    let args = variant
        .fields
        .iter()
        .zip(&operation.arguments)
        .map(|((label, field), value)| {
            let value = match (field, value) {
                (
                    jet_foundation::TestingHistory::HistorySchemaField::Integer { lo, hi },
                    jet_foundation::TestingHistory::HistoryValue::Integer(value),
                ) if lo <= value && value <= hi => CtValue::Int(*value),
                (
                    jet_foundation::TestingHistory::HistorySchemaField::Boolean,
                    jet_foundation::TestingHistory::HistoryValue::Boolean(value),
                ) => CtValue::Bool(*value),
                (
                    jet_foundation::TestingHistory::HistorySchemaField::Text,
                    jet_foundation::TestingHistory::HistoryValue::Text(value),
                ) => CtValue::Str(value.clone()),
                (
                    jet_foundation::TestingHistory::HistorySchemaField::Char,
                    jet_foundation::TestingHistory::HistoryValue::Text(value),
                ) => {
                    let mut chars = value.chars();
                    let character = chars
                        .next()
                        .filter(|_| chars.next().is_none())
                        .ok_or_else(|| format!("history operation `{}` has an invalid Char argument", operation.name))?;
                    CtValue::Char(character)
                }
                _ => {
                    return Err(format!(
                        "history operation `{}` has an argument with the wrong scalar kind",
                        operation.name
                    ))
                }
            };
            Ok((label.clone(), value))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(CtValue::Enum {
        type_name: schema.command_type.clone(),
        variant: variant.variant.clone(),
        args,
    })
}
pub(super) fn comptime_history_terminal(
    seed: u64,
    status: jet_foundation::TestingComparison::ComparisonStatus,
    reason: impl Into<String>,
) -> CtValue {
    CtValue::Present(Box::new(CtValue::Struct {
        type_name: "TestComparison".to_string(),
        fields: vec![
            (
                "status".to_string(),
                CtValue::Str(status.as_str().to_string()),
            ),
            (
                "relation".to_string(),
                CtValue::Str(
                    jet_foundation::TestingComparison::ObservationRelation::TypedEquality
                        .as_str()
                        .to_string(),
                ),
            ),
            ("source".to_string(), CtValue::Str("core.testing".to_string())),
            ("tool".to_string(), CtValue::Str("interpreter".to_string())),
            ("target".to_string(), CtValue::Str("comptime".to_string())),
            ("seed".to_string(), history_count_to_ct(seed)),
            ("case_ids".to_string(), CtValue::List(Vec::new())),
            ("inputs".to_string(), CtValue::List(Vec::new())),
            ("reference".to_string(), CtValue::List(Vec::new())),
            ("candidate".to_string(), CtValue::List(Vec::new())),
            ("first_difference".to_string(), CtValue::Int(-1)),
            ("reason".to_string(), CtValue::Str(reason.into())),
            ("universal_proof".to_string(), CtValue::Bool(false)),
        ],
    }))
}
/// Ephemeral borrowed state passed to explicit history generator callbacks.
/// The carrier never enters a history record or replay artifact; it only
/// keeps the Foundation runner's RNG state alive while one callback executes.
#[derive(Clone, Copy)]
pub(super) struct HistoryRngBorrow(*mut jet_foundation::TestingHistory::HistoryRng);

// The pointer is valid only for the synchronous callback invocation that owns
// the Foundation runner borrow. CtValue requires its opaque payload to be
// Send + Sync so the same carrier can cross the MIR callback seam.
unsafe impl Send for HistoryRngBorrow {}
unsafe impl Sync for HistoryRngBorrow {}

pub(super) fn history_rng_carrier(
    rng: &mut jet_foundation::TestingHistory::HistoryRng,
) -> CtValue {
    CtValue::Closure(std::sync::Arc::new(crate::AST::ClosureData {
        lambda: crate::AST::Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            result_type: None,
            error_type: None,
            effects: None,
            body: crate::AST::LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: crate::AST::LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None,
        opaque: Some(crate::AST::CtOpaque::new(HistoryRngBorrow(rng as *mut _))),
    }))
}

pub(super) fn history_rng_pointer(value: &CtValue) -> Option<*mut jet_foundation::TestingHistory::HistoryRng> {
    let CtValue::Closure(data) = value else {
        return None;
    };
    data.opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<HistoryRngBorrow>())
        .map(|borrow| borrow.0)
}

/// Execute one explicit HistoryRng method against the runner-owned borrowed
/// state. MIR's callback bridge uses this same carrier operation.
pub fn apply_history_rng_method(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(method, "next_u64" | "below" | "coin") {
        return None;
    }
    let Some(pointer) = history_rng_pointer(receiver) else {
        return None;
    };
    let Some(rng) = (unsafe { pointer.as_mut() }) else {
        return Some(Err(unsupported("invalid HistoryRng callback carrier", span)));
    };
    match method {
        "next_u64" if args.is_empty() => Some(Ok(exact_int_value(
            jet_foundation::Numeric::CtBigInt::from_u64(rng.next_u64()),
        ))),
        "next_u64" => Some(Err(unsupported(
            "HistoryRng.next_u64() expects no arguments",
            span,
        ))),
        "below" if args.len() == 1 => {
            let bound = match history_u64_from_ct(&args[0]) {
                Ok(bound) => bound,
                Err(reason) => return Some(Err(unsupported(&reason, span))),
            };
            Some(Ok(exact_int_value(
                jet_foundation::Numeric::CtBigInt::from_u64(rng.below(bound)),
            )))
        }
        "below" => Some(Err(unsupported(
            "HistoryRng.below() expects one argument",
            span,
        ))),
        "coin" if args.is_empty() => Some(Ok(CtValue::Bool(rng.coin()))),
        "coin" => Some(Err(unsupported(
            "HistoryRng.coin() expects no arguments",
            span,
        ))),
        _ => None,
    }
}

pub(super) fn history_u64_from_ct(value: &CtValue) -> Result<u64, String> {
    let number = exact_big(value)
        .ok_or_else(|| "history count must be an exact non-negative Int".to_string())?;
    let signed = number
        .try_i128()
        .ok_or_else(|| "history count is outside the supported integer range".to_string())?;
    u64::try_from(signed)
        .map_err(|_| "history count must be a non-negative Int".to_string())
}

pub(super) fn history_positive_count_from_ct(value: &CtValue) -> Result<usize, String> {
    let count = history_u64_from_ct(value)?;
    if count == 0 {
        return Err("history cases must be a positive Int".to_string());
    }
    usize::try_from(count)
        .map_err(|_| "history cases exceed the supported host count range".to_string())
}

pub(super) fn history_u32_from_ct(value: &CtValue, label: &str) -> Result<u32, String> {
    let count = history_u64_from_ct(value)
        .map_err(|reason| format!("{label} must be a non-negative Count: {reason}"))?;
    u32::try_from(count).map_err(|_| format!("{label} exceeds the u32 identity range"))
}

pub(super) fn history_i64_from_ct(value: &CtValue, label: &str) -> Result<i64, String> {
    exact_big(value)
        .and_then(|number| number.try_i64())
        .ok_or_else(|| format!("{label} must fit in an Int"))
}

pub(super) fn history_string_from_ct(value: &CtValue, label: &str) -> Result<String, String> {
    match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(format!("{label} must be a String")),
    }
}

pub(super) fn history_option_value(value: &CtValue) -> Result<Option<CtValue>, String> {
    match value {
        CtValue::Present(value) => Ok(Some((**value).clone())),
        CtValue::Failed(CtReport::Clean(_)) => Ok(None),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name.is_empty() && variant == "Val" => args
            .first()
            .map(|(_, value)| Some(value.clone()))
            .ok_or_else(|| "malformed optional history value".to_string()),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name.is_empty() && variant == "None" && args.is_empty() => Ok(None),
        CtValue::Failed(CtReport::Told(reason)) => Err(format!(
            "optional history value carries failure: {}",
            reason.debug_rust()
        )),
        _ => Err("history strategy field must be an optional value".to_string()),
    }
}

pub(super) fn history_field<'a>(value: &'a CtValue, name: &str) -> Result<&'a CtValue, String> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(format!("history value is not a record with `{name}`"));
    };
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| format!("history record is missing `{name}`"))
}

pub(super) fn history_list<'a>(value: &'a CtValue, label: &str) -> Result<&'a Vec<CtValue>, String> {
    match value {
        CtValue::List(values) => Ok(values),
        _ => Err(format!("{label} must be a List")),
    }
}

#[derive(Clone, Debug)]
pub(super) struct ComptimeHistoryCommand {
    value: CtValue,
}

#[derive(Clone, Debug)]
pub(super) struct ComptimeHistoryDerivedStrategy;

impl HistoryCommand for ComptimeHistoryCommand {
    type Strategy = ComptimeHistoryDerivedStrategy;

    fn history_strategy() -> Self::Strategy {
        ComptimeHistoryDerivedStrategy
    }

    fn history_command_type() -> &'static str {
        "comptime-history-command"
    }
}

impl HistoryStrategyBehavior<ComptimeHistoryCommand> for ComptimeHistoryDerivedStrategy {
    fn command_type(&self) -> &str {
        "comptime-history-command"
    }

    fn distributions(&self) -> Vec<HistoryDistribution> {
        Vec::new()
    }

    fn generate(
        &self,
        _rng: &mut HistoryRng,
        _case_index: usize,
        _max_steps: usize,
    ) -> Option<TypedHistoryCase<ComptimeHistoryCommand>> {
        None
    }

    fn rebuild(&self, _case: &HistoryCase) -> Option<TypedHistoryCase<ComptimeHistoryCommand>> {
        None
    }

    fn valid(&self, _case: &TypedHistoryCase<ComptimeHistoryCommand>) -> bool {
        false
    }
}

pub(super) fn history_enum_payload<'a>(
    args: &'a [(Option<String>, CtValue)],
    label: &str,
) -> Result<&'a CtValue, String> {
    args.first()
        .map(|(_, value)| value)
        .ok_or_else(|| format!("{label} is missing its payload"))
}

pub(super) fn history_handle_from_ct(value: &CtValue) -> Result<HandleId, String> {
    Ok(HandleId {
        value: history_u32_from_ct(
            history_field(value, "value")?,
            "HandleId.value",
        )?,
    })
}

pub(super) fn history_task_from_ct(value: &CtValue) -> Result<TaskId, String> {
    Ok(TaskId {
        value: history_u32_from_ct(history_field(value, "value")?, "TaskId.value")?,
    })
}

pub(super) fn history_event_from_ct(value: &CtValue) -> Result<EventId, String> {
    Ok(EventId {
        value: history_u32_from_ct(history_field(value, "value")?, "EventId.value")?,
    })
}

pub(super) fn history_value_from_ct(value: &CtValue) -> Result<HistoryValue, String> {
    let CtValue::Enum { variant, args, .. } = value else {
        return Err("HistoryValue must be an enum value".to_string());
    };
    let payload = history_enum_payload(args, "HistoryValue")?;
    match variant.as_str() {
        "Integer" => history_i64_from_ct(payload, "HistoryValue.Integer").map(HistoryValue::Integer),
        "Boolean" => match payload {
            CtValue::Bool(value) => Ok(HistoryValue::Boolean(*value)),
            _ => Err("HistoryValue.Boolean payload must be Bool".to_string()),
        },
        "Text" => history_string_from_ct(payload, "HistoryValue.Text").map(HistoryValue::Text),
        "Handle" => history_handle_from_ct(payload).map(HistoryValue::Handle),
        "Redacted" => history_string_from_ct(payload, "HistoryValue.Redacted")
            .map(HistoryValue::Redacted),
        _ => Err(format!("unknown HistoryValue variant `{variant}`")),
    }
}

pub(super) fn history_precondition_from_ct(value: &CtValue) -> Result<HistoryPrecondition, String> {
    let CtValue::Enum { variant, args, .. } = value else {
        return Err("HistoryPrecondition must be an enum value".to_string());
    };
    match variant.as_str() {
        "HandleLive" => Ok(HistoryPrecondition::HandleLive(history_handle_from_ct(
            history_enum_payload(args, "HistoryPrecondition.HandleLive")?,
        )?)),
        "HandleState" => Ok(HistoryPrecondition::HandleState {
            handle: history_handle_from_ct(
                args.iter()
                    .find_map(|(name, value)| (name.as_deref() == Some("handle")).then_some(value))
                    .ok_or_else(|| {
                        "HistoryPrecondition.HandleState is missing `handle`".to_string()
                    })?,
            )?,
            state: history_string_from_ct(
                args.iter()
                    .find_map(|(name, value)| (name.as_deref() == Some("state")).then_some(value))
                    .ok_or_else(|| {
                        "HistoryPrecondition.HandleState is missing `state`".to_string()
                    })?,
                "HistoryPrecondition.HandleState.state",
            )?,
        }),
        "TaskCompleted" => Ok(HistoryPrecondition::TaskCompleted(history_task_from_ct(
            history_enum_payload(args, "HistoryPrecondition.TaskCompleted")?,
        )?)),
        "EventAvailable" => Ok(HistoryPrecondition::EventAvailable(history_event_from_ct(
            history_enum_payload(args, "HistoryPrecondition.EventAvailable")?,
        )?)),
        _ => Err(format!("unknown HistoryPrecondition variant `{variant}`")),
    }
}

pub(super) fn history_operation_from_ct(value: &CtValue) -> Result<HistoryOperation, String> {
    let arguments = history_list(history_field(value, "arguments")?, "HistoryOperation.arguments")?
        .iter()
        .map(history_value_from_ct)
        .collect::<Result<Vec<_>, _>>()?;
    let creates = history_list(history_field(value, "creates")?, "HistoryOperation.creates")?
        .iter()
        .map(history_handle_from_ct)
        .collect::<Result<Vec<_>, _>>()?;
    let consumes = history_list(history_field(value, "consumes")?, "HistoryOperation.consumes")?
        .iter()
        .map(history_handle_from_ct)
        .collect::<Result<Vec<_>, _>>()?;
    let preconditions = history_list(
        history_field(value, "preconditions")?,
        "HistoryOperation.preconditions",
    )?
    .iter()
    .map(history_precondition_from_ct)
    .collect::<Result<Vec<_>, _>>()?;
    let depends_on = history_list(
        history_field(value, "depends_on")?,
        "HistoryOperation.depends_on",
    )?
    .iter()
    .map(|value| history_u32_from_ct(value, "HistoryOperation.depends_on"))
    .collect::<Result<Vec<_>, _>>()?;
    let task = match history_option_value(history_field(value, "task")?)? {
        Some(value) => Some(history_task_from_ct(&value)?),
        None => None,
    };
    let event = match history_option_value(history_field(value, "event")?)? {
        Some(value) => Some(history_event_from_ct(&value)?),
        None => None,
    };
    Ok(HistoryOperation {
        index: history_u32_from_ct(
            history_field(value, "index")?,
            "HistoryOperation.index",
        )?,
        name: history_string_from_ct(
            history_field(value, "name")?,
            "HistoryOperation.name",
        )?,
        arguments,
        creates,
        consumes,
        preconditions,
        depends_on,
        task,
        event,
    })
}

pub(super) fn history_schedule_from_ct(value: &CtValue) -> Result<HistoryScheduleChoice, String> {
    Ok(HistoryScheduleChoice {
        operation: history_u32_from_ct(
            history_field(value, "operation")?,
            "HistoryScheduleChoice.operation",
        )?,
        task: match history_option_value(history_field(value, "task")?)? {
            Some(value) => Some(history_task_from_ct(&value)?),
            None => None,
        },
        event: match history_option_value(history_field(value, "event")?)? {
            Some(value) => Some(history_event_from_ct(&value)?),
            None => None,
        },
        choice: history_string_from_ct(
            history_field(value, "choice")?,
            "HistoryScheduleChoice.choice",
        )?,
    })
}

pub(super) fn history_case_from_ct(value: &CtValue) -> Result<HistoryCase, String> {
    let operations = history_list(
        history_field(value, "operations")?,
        "HistoryCase.operations",
    )?
    .iter()
    .map(history_operation_from_ct)
    .collect::<Result<Vec<_>, _>>()?;
    let schedule = history_list(history_field(value, "schedule")?, "HistoryCase.schedule")?
        .iter()
        .map(history_schedule_from_ct)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HistoryCase {
        case_id: history_string_from_ct(history_field(value, "case_id")?, "HistoryCase.case_id")?,
        seed: history_u64_from_ct(history_field(value, "seed")?)?,
        operations,
        schedule,
    })
}

pub(super) fn history_typed_case_from_ct(
    value: &CtValue,
) -> Result<TypedHistoryCase<ComptimeHistoryCommand>, String> {
    let case = history_case_from_ct(history_field(value, "case")?)?;
    let commands = history_list(
        history_field(value, "commands")?,
        "TypedHistoryCase.commands",
    )?
    .iter()
    .cloned()
    .map(|value| ComptimeHistoryCommand { value })
    .collect::<Vec<_>>();
    TypedHistoryCase::new(case, commands)
        .ok_or_else(|| "TypedHistoryCase command count does not match operations".to_string())
}

pub(super) fn history_count_to_ct(value: u64) -> CtValue {
    exact_int_value(jet_foundation::Numeric::CtBigInt::from_u64(value))
}

pub(super) fn history_optional_to_ct(value: Option<CtValue>, type_name: &str) -> CtValue {
    value.map_or_else(
        || CtValue::absent(Type::Named(type_name.to_string())),
        |value| CtValue::Present(Box::new(value)),
    )
}

pub(super) fn history_id_to_ct(type_name: &str, value: u32) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("value".to_string(), history_count_to_ct(u64::from(value)))],
    }
}

pub(super) fn history_value_to_ct(value: &HistoryValue) -> CtValue {
    let (variant, payload) = match value {
        HistoryValue::Integer(value) => ("Integer", CtValue::Int(*value)),
        HistoryValue::Boolean(value) => ("Boolean", CtValue::Bool(*value)),
        HistoryValue::Text(value) => ("Text", CtValue::Str(value.clone())),
        HistoryValue::Handle(value) => ("Handle", history_id_to_ct("HandleId", value.value)),
        HistoryValue::Redacted(value) => ("Redacted", CtValue::Str(value.clone())),
    };
    CtValue::Enum {
        type_name: "HistoryValue".to_string(),
        variant: variant.to_string(),
        args: vec![(None, payload)],
    }
}

pub(super) fn history_usize_from_ct(value: &CtValue, label: &str) -> Result<usize, String> {
    usize::try_from(history_u64_from_ct(value)?)
        .map_err(|_| format!("{label} exceeds the supported host count range"))
}

pub(super) fn history_precondition_to_ct(value: &HistoryPrecondition) -> CtValue {
    match value {
        HistoryPrecondition::HandleLive(handle) => CtValue::Enum {
            type_name: "HistoryPrecondition".to_string(),
            variant: "HandleLive".to_string(),
            args: vec![(None, history_id_to_ct("HandleId", handle.value))],
        },
        HistoryPrecondition::HandleState { handle, state } => CtValue::Enum {
            type_name: "HistoryPrecondition".to_string(),
            variant: "HandleState".to_string(),
            args: vec![
                (
                    Some("handle".to_string()),
                    history_id_to_ct("HandleId", handle.value),
                ),
                (Some("state".to_string()), CtValue::Str(state.clone())),
            ],
        },
        HistoryPrecondition::TaskCompleted(task) => CtValue::Enum {
            type_name: "HistoryPrecondition".to_string(),
            variant: "TaskCompleted".to_string(),
            args: vec![(None, history_id_to_ct("TaskId", task.value))],
        },
        HistoryPrecondition::EventAvailable(event) => CtValue::Enum {
            type_name: "HistoryPrecondition".to_string(),
            variant: "EventAvailable".to_string(),
            args: vec![(None, history_id_to_ct("EventId", event.value))],
        },
    }
}

pub(super) fn history_operation_to_ct(value: &HistoryOperation) -> CtValue {
    CtValue::Struct {
        type_name: "HistoryOperation".to_string(),
        fields: vec![
            ("index".to_string(), history_count_to_ct(u64::from(value.index))),
            ("name".to_string(), CtValue::Str(value.name.clone())),
            (
                "arguments".to_string(),
                CtValue::List(value.arguments.iter().map(history_value_to_ct).collect()),
            ),
            (
                "creates".to_string(),
                CtValue::List(
                    value
                        .creates
                        .iter()
                        .map(|handle| history_id_to_ct("HandleId", handle.value))
                        .collect(),
                ),
            ),
            (
                "consumes".to_string(),
                CtValue::List(
                    value
                        .consumes
                        .iter()
                        .map(|handle| history_id_to_ct("HandleId", handle.value))
                        .collect(),
                ),
            ),
            (
                "preconditions".to_string(),
                CtValue::List(
                    value
                        .preconditions
                        .iter()
                        .map(history_precondition_to_ct)
                        .collect(),
                ),
            ),
            (
                "depends_on".to_string(),
                CtValue::List(
                    value
                        .depends_on
                        .iter()
                        .map(|index| history_count_to_ct(u64::from(*index)))
                        .collect(),
                ),
            ),
            (
                "task".to_string(),
                history_optional_to_ct(
                    value
                        .task
                        .map(|task| history_id_to_ct("TaskId", task.value)),
                    "TaskId",
                ),
            ),
            (
                "event".to_string(),
                history_optional_to_ct(
                    value
                        .event
                        .map(|event| history_id_to_ct("EventId", event.value)),
                    "EventId",
                ),
            ),
        ],
    }
}

pub(super) fn history_schedule_to_ct(value: &HistoryScheduleChoice) -> CtValue {
    CtValue::Struct {
        type_name: "HistoryScheduleChoice".to_string(),
        fields: vec![
            (
                "operation".to_string(),
                history_count_to_ct(u64::from(value.operation)),
            ),
            (
                "task".to_string(),
                history_optional_to_ct(
                    value
                        .task
                        .map(|task| history_id_to_ct("TaskId", task.value)),
                    "TaskId",
                ),
            ),
            (
                "event".to_string(),
                history_optional_to_ct(
                    value
                        .event
                        .map(|event| history_id_to_ct("EventId", event.value)),
                    "EventId",
                ),
            ),
            ("choice".to_string(), CtValue::Str(value.choice.clone())),
        ],
    }
}

pub(super) fn history_case_to_ct(value: &HistoryCase) -> CtValue {
    CtValue::Struct {
        type_name: "HistoryCase".to_string(),
        fields: vec![
            ("case_id".to_string(), CtValue::Str(value.case_id.clone())),
            ("seed".to_string(), history_count_to_ct(value.seed)),
            (
                "operations".to_string(),
                CtValue::List(value.operations.iter().map(history_operation_to_ct).collect()),
            ),
            (
                "schedule".to_string(),
                CtValue::List(value.schedule.iter().map(history_schedule_to_ct).collect()),
            ),
        ],
    }
}

pub(super) fn history_typed_case_to_ct(
    value: &TypedHistoryCase<ComptimeHistoryCommand>,
) -> CtValue {
    CtValue::Struct {
        type_name: "TypedHistoryCase".to_string(),
        fields: vec![
            ("case".to_string(), history_case_to_ct(&value.case)),
            (
                "commands".to_string(),
                CtValue::List(
                    value
                        .commands
                        .iter()
                        .map(|command| command.value.clone())
                        .collect(),
                ),
            ),
        ],
    }
}

pub(super) fn history_bounds_from_ct(value: &CtValue) -> Result<HistoryBounds, String> {
    Ok(HistoryBounds {
        max_steps: history_usize_from_ct(
            history_field(value, "max_steps")?,
            "HistoryBounds.max_steps",
        )?,
        max_resources: history_usize_from_ct(
            history_field(value, "max_resources")?,
            "HistoryBounds.max_resources",
        )?,
        max_shrink_attempts: history_usize_from_ct(
            history_field(value, "max_shrink_attempts")?,
            "HistoryBounds.max_shrink_attempts",
        )?,
        max_discarded_cases: history_usize_from_ct(
            history_field(value, "max_discarded_cases")?,
            "HistoryBounds.max_discarded_cases",
        )?,
    })
}

pub(super) fn history_distribution_from_ct(
    value: &CtValue,
) -> Result<HistoryDistribution, String> {
    let operation = history_string_from_ct(
        history_field(value, "operation")?,
        "HistoryDistribution.operation",
    )?;
    let weight = history_usize_from_ct(
        history_field(value, "weight")?,
        "HistoryDistribution.weight",
    )?;
    HistoryDistribution::new(operation, weight)
        .map_err(|error| format!("invalid HistoryDistribution: {error}"))
}

pub(super) struct ComptimeHistoryStrategy {
    command_type: String,
    generate: CtValue,
    rebuild: CtValue,
    valid: CtValue,
    bounds: HistoryBounds,
    distributions: Vec<HistoryDistribution>,
    span: Span,
    callback_error: Rc<RefCell<Option<String>>>,
}

impl ComptimeHistoryStrategy {
    fn note_error(&self, reason: impl Into<String>) {
        let mut error = self.callback_error.borrow_mut();
        error.get_or_insert_with(|| reason.into());
    }

    fn invoke(
        &self,
        callback: &CtValue,
        args: Vec<CtValue>,
        role: &str,
    ) -> Result<CtValue, String> {
        let result = crate::Comptime::try_ambient_standalone_closure(
            callback,
            args.clone(),
            self.span,
        )
        .unwrap_or_else(|| super::invoke_standalone_closure(callback, args, self.span));
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                let reason = format!("{role} callback failed: {}", error.what);
                self.note_error(reason.clone());
                return Err(reason);
            }
        };
        match value {
            CtValue::Failed(CtReport::Told(reason)) => {
                let reason = format!("{role} callback returned failure: {}", reason.debug_rust());
                self.note_error(reason.clone());
                Err(reason)
            }
            CtValue::Failed(CtReport::Clean(_)) => {
                let reason = format!("{role} callback returned an absent value");
                self.note_error(reason.clone());
                Err(reason)
            }
            value => Ok(value),
        }
    }

    fn invoke_mut(
        &self,
        callback: &CtValue,
        args: &mut Vec<CtValue>,
        role: &str,
    ) -> Result<CtValue, String> {
        let result = crate::Comptime::try_ambient_standalone_closure_mut(
            callback,
            args,
            self.span,
        )
        .unwrap_or_else(|| super::invoke_standalone_closure_mut_args(callback, args, self.span));
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                let reason = format!("{role} callback failed: {}", error.what);
                self.note_error(reason.clone());
                return Err(reason);
            }
        };
        match value {
            CtValue::Failed(CtReport::Told(reason)) => {
                let reason = format!("{role} callback returned failure: {}", reason.debug_rust());
                self.note_error(reason.clone());
                Err(reason)
            }
            CtValue::Failed(CtReport::Clean(_)) => {
                let reason = format!("{role} callback returned an absent value");
                self.note_error(reason.clone());
                Err(reason)
            }
            value => Ok(value),
        }
    }

    fn callback_error(&self) -> Option<String> {
        self.callback_error.borrow().clone()
    }
}

impl HistoryStrategyBehavior<ComptimeHistoryCommand> for ComptimeHistoryStrategy {
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
    ) -> Option<TypedHistoryCase<ComptimeHistoryCommand>> {
        let mut args = vec![
            history_rng_carrier(rng),
            history_count_to_ct(u64::try_from(case_index).unwrap_or(u64::MAX)),
            history_count_to_ct(u64::try_from(max_steps).unwrap_or(u64::MAX)),
        ];
        let value = match self.invoke_mut(&self.generate, &mut args, "history generate") {
            Ok(value) => value,
            Err(reason) => {
                self.note_error(reason);
                return None;
            }
        };
        let value = match history_option_value(&value) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(reason) => {
                self.note_error(format!("history generate callback: {reason}"));
                return None;
            }
        };
        match history_typed_case_from_ct(&value) {
            Ok(value) => Some(value),
            Err(reason) => {
                self.note_error(format!("history generate callback: {reason}"));
                None
            }
        }
    }

    fn rebuild(&self, case: &HistoryCase) -> Option<TypedHistoryCase<ComptimeHistoryCommand>> {
        let value = match self.invoke(
            &self.rebuild,
            vec![history_case_to_ct(case)],
            "history rebuild",
        ) {
            Ok(value) => value,
            Err(reason) => {
                self.note_error(reason);
                return None;
            }
        };
        let value = match history_option_value(&value) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(reason) => {
                self.note_error(format!("history rebuild callback: {reason}"));
                return None;
            }
        };

        match history_typed_case_from_ct(&value) {
            Ok(value) => Some(value),
            Err(reason) => {
                self.note_error(format!("history rebuild callback: {reason}"));
                None
            }
        }
    }

    fn valid(&self, case: &TypedHistoryCase<ComptimeHistoryCommand>) -> bool {
        let value = match self.invoke(
            &self.valid,
            vec![history_typed_case_to_ct(case)],
            "history valid",
        ) {
            Ok(value) => value,
            Err(reason) => {
                self.note_error(reason);
                return false;
            }
        };
        match value {
            CtValue::Bool(value) => value,
            _ => {
                self.note_error("history valid callback must return Bool");
                false
            }
        }
    }
}
pub(super) fn history_capture_u64(payload: &mut Vec<u8>, value: u64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn history_capture_bytes(payload: &mut Vec<u8>, value: &[u8]) {
    history_capture_u64(payload, u64::try_from(value.len()).unwrap_or(u64::MAX));
    payload.extend_from_slice(value);
}

pub(super) fn history_capture_text(payload: &mut Vec<u8>, value: &str) {
    history_capture_bytes(payload, value.as_bytes());
}

pub(super) fn history_capture_value(value: &CtValue, payload: &mut Vec<u8>) -> Result<(), String> {
    match value {
        CtValue::Int(value) => {
            payload.push(b'i');
            payload.extend_from_slice(&value.to_le_bytes());
        }
        CtValue::Float(CtFloat::F32(value)) => {
            payload.push(b'f');
            payload.push(32);
            payload.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        CtValue::Float(CtFloat::F64(value)) => {
            payload.push(b'f');
            payload.push(64);
            payload.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        CtValue::Bool(value) => {
            payload.extend_from_slice(if *value { b"bt" } else { b"bf" });
        }
        CtValue::Char(value) => {
            payload.push(b'c');
            payload.extend_from_slice(&(*value as u32).to_le_bytes());
        }
        CtValue::Str(value) => {
            payload.push(b's');
            history_capture_text(payload, value);
        }
        CtValue::BigInt(value) => {
            payload.push(b'n');
            history_capture_text(payload, &value.to_string_rep());
        }
        CtValue::Bytes(value) => {
            payload.push(b'b');
            history_capture_bytes(payload, value);
        }
        CtValue::List(values) => {
            payload.push(b'l');
            history_capture_u64(payload, u64::try_from(values.len()).unwrap_or(u64::MAX));
            for value in values {
                history_capture_value(value, payload)?;
            }
        }
        CtValue::Map(values) => {
            payload.push(b'm');
            history_capture_u64(payload, u64::try_from(values.len()).unwrap_or(u64::MAX));
            for (key, value) in values {
                history_capture_value(&key.to_value(), payload)?;
                history_capture_value(value, payload)?;
            }
        }
        CtValue::Struct { type_name, fields } => {
            payload.push(b'r');
            history_capture_text(payload, type_name);
            let fields = fields
                .iter()
                .filter(|(name, _)| !crate::Syntax::is_memo_storage_name(name))
                .collect::<Vec<_>>();
            history_capture_u64(payload, u64::try_from(fields.len()).unwrap_or(u64::MAX));
            for (name, value) in fields {
                history_capture_text(payload, name);
                history_capture_value(value, payload)?;
            }
        }
        CtValue::Enum {
            type_name,
            variant,
            args,
        } => {
            payload.push(b'e');
            history_capture_text(payload, type_name);
            history_capture_text(payload, variant);
            history_capture_u64(payload, u64::try_from(args.len()).unwrap_or(u64::MAX));
            for (label, value) in args {
                match label {
                    Some(label) => {
                        payload.push(1);
                        history_capture_text(payload, label);
                    }
                    None => payload.push(0),
                }
                history_capture_value(value, payload)?;
            }
        }
        CtValue::Present(value) => {
            payload.push(b'o');
            history_capture_value(value, payload)?;
        }
        CtValue::Failed(CtReport::Clean(ty)) => {
            payload.push(b'a');
            history_capture_text(payload, &ty.identity_key());
        }
        CtValue::Failed(CtReport::Told(value)) => {
            payload.push(b'x');
            history_capture_value(value, payload)?;
        }
        CtValue::Unit => payload.push(b'u'),
        CtValue::Closure(_) => {
            payload.push(b'f');
            let identity = history_callback_fingerprint_from_ct(value)?;
            history_capture_text(payload, &identity);
        }
    }
    Ok(())
}

/// Fingerprint one checked callback and its relevant captures without exposing
/// capture bytes.  The function identity is supplied by the checked closure
/// owner; values use a width/type-preserving canonical codec.
pub fn history_callback_fingerprint(
    function_identity: &str,
    captures: &[CtValue],
) -> Result<String, String> {
    if function_identity.is_empty() {
        return Err("history callback has no checked function identity".to_string());
    }
    let mut payload = Vec::new();
    history_capture_text(&mut payload, "jet.history.callback.v1");
    history_capture_text(&mut payload, function_identity);
    history_capture_u64(
        &mut payload,
        u64::try_from(captures.len()).unwrap_or(u64::MAX),
    );
    for capture in captures {
        history_capture_value(capture, &mut payload)?;
    }
    Ok(jet_foundation::SHA256::sha256_hex(&payload))
}

thread_local! {
    static HISTORY_CALLBACK_FINGERPRINT_STACK: RefCell<Vec<usize>> =
        const { RefCell::new(Vec::new()) };
}

pub(super) struct HistoryCallbackFingerprintGuard(usize);

impl Drop for HistoryCallbackFingerprintGuard {
    fn drop(&mut self) {
        HISTORY_CALLBACK_FINGERPRINT_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(index) = stack.iter().rposition(|key| *key == self.0) {
                stack.remove(index);
            }
        });
    }
}

pub(super) fn enter_history_callback_fingerprint(
    closure: &std::sync::Arc<crate::AST::ClosureData>,
) -> Result<HistoryCallbackFingerprintGuard, String> {
    let key = std::sync::Arc::as_ptr(closure) as usize;
    HISTORY_CALLBACK_FINGERPRINT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if stack.contains(&key) {
            return Err("history callback capture contains a cycle".to_string());
        }
        stack.push(key);
        Ok(HistoryCallbackFingerprintGuard(key))
    })
}


pub(super) fn history_callback_fingerprint_from_ct(value: &CtValue) -> Result<String, String> {
    let CtValue::Closure(closure) = value else {
        return Err("history callback is not a closure".to_string());
    };
    let _guard = enter_history_callback_fingerprint(closure)?;
    if let Some(opaque) = closure.opaque.as_ref() {
        if let Some(host) = opaque.downcast_ref::<crate::Comptime::AmbientStandaloneClosure>() {
            return host.history_callback_identity();
        }
        return Err("history callback has an unknown opaque owner".to_string());
    }
    Err(
        "history callback checked source identity is unavailable for a non-MIR closure"
            .to_string(),
    )
}

pub(super) fn history_derived_strategy_fingerprint(
    schema: Option<&HistoryCommandSchema>,
    strategy: &jet_foundation::TestingHistory::HistorySchemaStrategy,
    bounds: HistoryBounds,
) -> Result<String, String> {
    let schema = schema.ok_or_else(|| "history checked schema is unavailable".to_string())?;
    let mut payload = Vec::new();
    history_capture_text(&mut payload, "jet.history.derived-strategy.v1");
    history_capture_text(&mut payload, &schema.command_type);
    history_capture_u64(
        &mut payload,
        u64::try_from(schema.variants.len()).unwrap_or(u64::MAX),
    );
    for variant in &schema.variants {
        history_capture_text(&mut payload, &variant.operation);
        history_capture_text(&mut payload, &variant.variant);
        history_capture_u64(
            &mut payload,
            u64::try_from(variant.fields.len()).unwrap_or(u64::MAX),
        );
        for (label, field) in &variant.fields {
            match label {
                Some(label) => {
                    payload.push(1);
                    history_capture_text(&mut payload, label);
                }
                None => payload.push(0),
            }
            match field {
                jet_foundation::TestingHistory::HistorySchemaField::Integer { lo, hi } => {
                    payload.push(b'i');
                    payload.extend_from_slice(&lo.to_le_bytes());
                    payload.extend_from_slice(&hi.to_le_bytes());
                }
                jet_foundation::TestingHistory::HistorySchemaField::Boolean => payload.push(b'b'),
                jet_foundation::TestingHistory::HistorySchemaField::Text => payload.push(b't'),
                jet_foundation::TestingHistory::HistorySchemaField::Char => payload.push(b'c'),
            }
        }
    }
    match schema.unsupported_reason.as_deref() {
        Some(reason) => {
            payload.push(1);
            history_capture_text(&mut payload, reason);
        }
        None => payload.push(0),
    }
    for value in [
        bounds.max_steps,
        bounds.max_resources,
        bounds.max_shrink_attempts,
        bounds.max_discarded_cases,
    ] {
        history_capture_u64(&mut payload, u64::try_from(value).unwrap_or(u64::MAX));
    }
    for distribution in strategy.distributions() {
        history_capture_text(&mut payload, &distribution.operation);
        history_capture_u64(
            &mut payload,
            u64::try_from(distribution.weight).unwrap_or(u64::MAX),
        );
    }
    Ok(jet_foundation::SHA256::sha256_hex(&payload))
}

pub(super) fn history_provenance_for_callbacks(
    schema: Option<&HistoryCommandSchema>,
    callbacks: &[(&str, &CtValue)],
    strategy_identity: Option<String>,
) -> Result<HistoryProvenance, String> {
    let provenance = schema
        .and_then(|schema| schema.provenance.clone())
        .ok_or_else(|| "history checked provenance is unavailable".to_string())?;
    let mut fingerprints = callbacks
        .iter()
        .map(|(role, callback)| {
            history_callback_fingerprint_from_ct(callback)
                .map(|identity| (*role, identity))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(identity) = strategy_identity {
        fingerprints.push(("strategy", identity));
    }
    fingerprints.sort_unstable_by(|left, right| left.0.cmp(right.0));
    provenance
        .bind_callbacks(
            &fingerprints
                .iter()
                .map(|(role, identity)| (*role, identity.as_str()))
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())
}

pub(super) fn history_strategy_from_ct(
    value: &CtValue,
    command_type: &str,
    span: Span,
) -> Result<ComptimeHistoryStrategy, String> {
    let bounds = history_bounds_from_ct(history_field(value, "bounds")?)?;
    let distributions = history_list(
        history_field(value, "distributions")?,
        "HistoryStrategy.distributions",
    )?
    .iter()
    .map(history_distribution_from_ct)
    .collect::<Result<Vec<_>, _>>()?;
    Ok(ComptimeHistoryStrategy {
        command_type: command_type.to_string(),
        generate: history_field(value, "generate")?.clone(),
        rebuild: history_field(value, "rebuild")?.clone(),
        valid: history_field(value, "valid")?.clone(),
        bounds,
        distributions,
        span,
        callback_error: Rc::new(RefCell::new(None)),
    })
}

pub(crate) fn apply_testing_histories_explicit(
    seed: u64,
    cases: usize,
    strategy_value: CtValue,
    model: CtValue,
    actual: CtValue,
    observe: CtValue,
    schema: Option<&HistoryCommandSchema>,
    command_type: &str,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let strategy = match history_strategy_from_ct(&strategy_value, command_type, span) {
        Ok(strategy) => strategy,
        Err(reason) => return Ok(CtValue::failed(Box::new(CtValue::Str(reason)))),
    };
    let provenance = match history_provenance_for_callbacks(
        schema,
        &[
            ("generate", &strategy.generate),
            ("rebuild", &strategy.rebuild),
            ("valid", &strategy.valid),
            ("model", &model),
            ("actual", &actual),
            ("observe", &observe),
        ],
        None,
    ) {
        Ok(provenance) => provenance,
        Err(reason) => return Ok(CtValue::failed(Box::new(CtValue::Str(reason)))),
    };
    let relation = jet_foundation::TestingComparison::ObservationRelation::TypedEquality;
    let config = jet_foundation::TestingHistory::HistoryConfig::new(
        seed,
        cases,
        strategy.bounds,
        provenance.source.clone(),
        provenance.tool.clone(),
        provenance.target.clone(),
        relation.clone(),
        jet_foundation::TestingHistory::OracleDeclaration::independent(
            "testing.histories.model",
        ),
    )
    .with_provenance(provenance.clone())
    .with_command_type(command_type.to_string());
    let config = strategy
        .distributions
        .iter()
        .cloned()
        .fold(config, |config, distribution| config.with_distribution(distribution));
    let runner = match jet_foundation::TestingHistory::HistoryRunner::new(config) {
        Ok(runner) => runner,
        Err(error) => return Ok(CtValue::failed(Box::new(CtValue::Str(error.to_string())))),
    };
    let invoke = |callback: &CtValue, input: CtValue, role: &str| {
        strategy.invoke(callback, vec![input], role)
    };
    let mut inputs = Vec::new();
    let mut reference_values = Vec::new();
    let mut candidate_values = Vec::new();
    let mut samples = Vec::new();
    let run = match runner.run_strategy::<ComptimeHistoryCommand, _, _>(
        &strategy,
        |typed| {
            let unavailable = |reason: String| {
                jet_foundation::TestingComparison::ComparisonRecord::terminal(
                    relation.clone(),
                    jet_foundation::TestingComparison::ComparisonStatus::Unavailable,
                    reason,
                )
            };
            let input = CtValue::List(
                typed
                    .commands
                    .iter()
                    .map(|command| command.value.clone())
                    .collect(),
            );
            let observe_value = |value: CtValue| {
                invoke(&observe, value, "observe").map(|value| {
                    jet_foundation::TestingComparison::ComparisonObservation::value(
                        crate::Comptime::JSONInterp::render_ordered_datatree(&value, false, 0),
                    )
                })
            };
            let reference_value = match invoke(&model, input.clone(), "model") {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let candidate_value = match invoke(&actual, input.clone(), "actual") {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let reference_observed = match observe_value(reference_value.clone()) {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let candidate_observed = match observe_value(candidate_value.clone()) {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let reference_replay_value = match invoke(&model, input.clone(), "model replay") {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let candidate_replay_value = match invoke(&actual, input.clone(), "actual replay") {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let reference_replay = match observe_value(reference_replay_value) {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let candidate_replay = match observe_value(candidate_replay_value) {
                Ok(value) => value,
                Err(reason) => return unavailable(reason),
            };
            let mut identity = jet_foundation::TestingComparison::ComparisonIdentity::new(
                typed.case.case_id.clone(),
                typed.case.input_id(),
                provenance.source.clone(),
                provenance.tool.clone(),
                provenance.target.clone(),
            );
            identity.seed = Some(typed.case.seed);
            let sample = jet_foundation::TestingComparison::ComparisonSample::new(
                identity,
                reference_observed,
                candidate_observed,
            )
            .with_replays(reference_replay, candidate_replay);
            inputs.push(input.clone());
            reference_values.push(reference_value);
            candidate_values.push(candidate_value);
            samples.push(sample.clone());
            jet_foundation::TestingComparison::compare_samples(relation.clone(), [sample])
        },
    ) {
        Ok(run) => run,
        Err(error) => {
            let reason = strategy
                .callback_error()
                .unwrap_or_else(|| error.to_string());
            return Ok(CtValue::failed(Box::new(CtValue::Str(reason))));
        }
    };
    if let Some(reason) = strategy.callback_error() {
        return Ok(CtValue::failed(Box::new(CtValue::Str(reason))));
    }
    if let Some(artifact) = run.failure {
        return Ok(CtValue::failed(Box::new(CtValue::Str(format!(
            "history-artifact {}",
            artifact.json()
        )))));
    }
    let explored_cases = run.explored_cases;
    let mut record = jet_foundation::TestingComparison::compare_samples_with_discarded(
        relation,
        samples,
        run.discarded_cases,
    );
    if explored_cases == 0 {
        record.status = jet_foundation::TestingComparison::ComparisonStatus::Unavailable;
        record.reason = Some("no history case produced an observation".to_string());
    }
    let case_ids = record
        .samples
        .iter()
        .map(|sample| CtValue::Str(sample.identity.case_id.clone()))
        .collect();
    Ok(CtValue::Present(Box::new(CtValue::Struct {
        type_name: "TestComparison".to_string(),
        fields: vec![
            (
                "status".to_string(),
                CtValue::Str(record.status.as_str().to_string()),
            ),
            (
                "relation".to_string(),
                CtValue::Str(record.relation.as_str().to_string()),
            ),
            (
                "source".to_string(),
                CtValue::Str(provenance.source.clone()),
            ),
            ("tool".to_string(), CtValue::Str(provenance.tool.clone())),
            ("target".to_string(), CtValue::Str(provenance.target.clone())),
            ("seed".to_string(), history_count_to_ct(seed)),
            ("case_ids".to_string(), CtValue::List(case_ids)),
            ("inputs".to_string(), CtValue::List(inputs)),
            (
                "reference".to_string(),
                CtValue::List(reference_values),
            ),
            (
                "candidate".to_string(),
                CtValue::List(candidate_values),
            ),
            (
                "first_difference".to_string(),
                CtValue::Int(record.first_difference.map(|index| index as i64).unwrap_or(-1)),
            ),
            (
                "reason".to_string(),
                CtValue::Str(record.reason.unwrap_or_else(|| {
                    format!("{explored_cases} observed history case(s) matched")
                })),
            ),
            ("universal_proof".to_string(), CtValue::Bool(false)),
        ],
    })))
}

pub(crate) fn apply_testing_histories(
    args: Vec<CtValue>,
    type_args: &[Type],
    span: Span,
    schema: Option<&HistoryCommandSchema>,
) -> Result<CtValue, Diagnostic> {
    if args.len() != 6 {
        return Err(unsupported(
            "testing.histories expects seed, cases, optional strategy, model, actual, and observe",
            span,
        ));
    }
    let Some(command_type) = type_args.first() else {
        return Ok(CtValue::failed(Box::new(CtValue::Str(
            "core.testing.histories needs one declared Command type".to_string(),
        ))));
    };
    if type_args.len() != 1 {
        return Ok(CtValue::failed(Box::new(CtValue::Str(
            "core.testing.histories needs exactly one declared Command type".to_string(),
        ))));
    }
    let command_key = command_type.identity_key();
    let data_tree_command = command_key == "DataTree" || command_key.starts_with("DataTree|");
    let seed = match history_u64_from_ct(&args[0]) {
        Ok(value) => value,
        Err(reason) => {
            return Ok(CtValue::failed(Box::new(CtValue::Str(reason))));
        }
    };
    let cases = match history_positive_count_from_ct(&args[1]) {
        Ok(value) => value,
        Err(reason) => {
            return Ok(CtValue::failed(Box::new(CtValue::Str(reason))));
        }
    };
    let strategy_value = match history_option_value(&args[2]) {
        Ok(value) => value,
        Err(reason) => {
            return Ok(CtValue::failed(Box::new(CtValue::Str(reason))));
        }
    };
    if let Some(strategy_value) = strategy_value {
        return apply_testing_histories_explicit(
            seed,
            cases,
            strategy_value,
            args[3].clone(),
            args[4].clone(),
            args[5].clone(),
            schema,
            &command_key,
            span,
        );
    }
    let model = args[3].clone();
    let actual = args[4].clone();
    let observe = args[5].clone();
    let unsupported_reason = if data_tree_command {
        None
    } else {
        schema
            .and_then(|schema| schema.unsupported_reason.as_deref())
            .map(str::to_string)
            .or_else(|| {
                schema
                    .filter(|schema| schema.variants.is_empty())
                    .map(|_| {
                        jet_foundation::TestingHistory::history_unsupported_type_reason(
                            &command_key,
                        )
                    })
            })
            .or_else(|| {
                schema.is_none().then(|| {
                    jet_foundation::TestingHistory::history_unsupported_type_reason(&command_key)
                })
            })
    };
    if let Some(reason) = unsupported_reason {
        return Ok(comptime_history_terminal(
            seed,
            jet_foundation::TestingComparison::ComparisonStatus::Unsupported,
            reason,
        ));
    }
    let bounds = jet_foundation::TestingHistory::HistoryBounds {
        max_steps: 8,
        max_resources: 32,
        max_shrink_attempts: 10_000,
        max_discarded_cases: 100,
    };
    let relation = jet_foundation::TestingComparison::ObservationRelation::TypedEquality;
    let strategy = if let Some(schema) = schema {
        let variants = schema
            .variants
            .iter()
            .map(|variant| {
                jet_foundation::TestingHistory::HistorySchemaVariant::new(
                    variant.operation.clone(),
                    variant.variant.clone(),
                    variant
                        .fields
                        .iter()
                        .map(|(_, field)| field.clone())
                        .collect(),
                )
            })
            .collect::<Option<Vec<_>>>();
        let Some(variants) = variants.and_then(|variants| {
            jet_foundation::TestingHistory::HistorySchemaStrategy::with_variants(
                schema.command_type.clone(),
                variants,
            )
        }) else {
            return Ok(comptime_history_terminal(
                seed,
                jet_foundation::TestingComparison::ComparisonStatus::Unsupported,
                jet_foundation::TestingHistory::history_unsupported_type_reason(
                    &schema.command_type,
                ),
            ));
        };
        variants
    } else {
        jet_foundation::TestingHistory::HistorySchemaStrategy::new("DataTree")
    };
    let strategy_identity = match history_derived_strategy_fingerprint(schema, &strategy, bounds) {
        Ok(identity) => identity,
        Err(reason) => return Ok(CtValue::failed(Box::new(CtValue::Str(reason)))),
    };
    let provenance = match history_provenance_for_callbacks(
        schema,
        &[("model", &model), ("actual", &actual), ("observe", &observe)],
        Some(strategy_identity),
    ) {
        Ok(provenance) => provenance,
        Err(reason) => return Ok(CtValue::failed(Box::new(CtValue::Str(reason)))),
    };
    let config = jet_foundation::TestingHistory::HistoryConfig::new(
        seed,
        cases,
        bounds,
        provenance.source.clone(),
        provenance.tool.clone(),
        provenance.target.clone(),
        relation.clone(),
        jet_foundation::TestingHistory::OracleDeclaration::independent(
            "testing.histories.model",
        ),
    )
    .with_provenance(provenance.clone())
    .with_command_type(strategy.command_type());
    let config = strategy
        .distributions()
        .into_iter()
        .fold(config, |config, distribution| config.with_distribution(distribution));
    let runner = match jet_foundation::TestingHistory::HistoryRunner::new(config) {
        Ok(runner) => runner,
        Err(error) => return Ok(CtValue::failed(Box::new(CtValue::Str(error.to_string())))),
    };
    let invoke = |callback: &CtValue, input: CtValue, role: &str| -> Result<CtValue, String> {
        let result = crate::Comptime::try_ambient_standalone_closure(
            callback,
            vec![input.clone()],
            span,
        )
        .unwrap_or_else(|| super::invoke_standalone_closure(callback, vec![input], span));
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(format!("{role} callback failed: {}", error.what)),
        }
    };
    let mut inputs = Vec::new();
    let mut reference_values = Vec::new();
    let mut candidate_values = Vec::new();
    let mut samples = Vec::new();
    let run = match runner
        .run_strategy::<jet_foundation::DataTree::DataTree, _, _>(
            &strategy,
            |typed| {
                let unavailable = |reason: String| {
                    jet_foundation::TestingComparison::ComparisonRecord::terminal(
                        relation.clone(),
                        jet_foundation::TestingComparison::ComparisonStatus::Unavailable,
                        reason,
                    )
                };
                let command_values = match schema {
                    Some(schema) => typed
                        .case
                        .operations
                        .iter()
                        .map(|operation| comptime_history_enum_command(operation, schema))
                        .collect::<Result<Vec<_>, _>>(),
                    None => typed
                        .commands
                        .iter()
                        .map(comptime_history_data_tree_command)
                        .collect::<Result<Vec<_>, _>>(),
                };
                let command_values = match command_values {
                    Ok(values) => values,
                    Err(reason) => return unavailable(reason),
                };
                let input = CtValue::List(command_values);
                let observe_value = |value: CtValue| {
                    invoke(&observe, value, "observe").map(|value| {
                        jet_foundation::TestingComparison::ComparisonObservation::value(
                            crate::Comptime::JSONInterp::render_ordered_datatree(
                                &value, false, 0,
                            ),
                        )
                    })
                };
                let reference_value = match invoke(&model, input.clone(), "model") {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let candidate_value = match invoke(&actual, input.clone(), "actual") {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let reference_observed = match observe_value(reference_value.clone()) {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let candidate_observed = match observe_value(candidate_value.clone()) {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let reference_replay_value = match invoke(&model, input.clone(), "model replay") {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let candidate_replay_value = match invoke(&actual, input.clone(), "actual replay") {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let reference_replay = match observe_value(reference_replay_value) {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let candidate_replay = match observe_value(candidate_replay_value) {
                    Ok(value) => value,
                    Err(reason) => return unavailable(reason),
                };
                let mut identity = jet_foundation::TestingComparison::ComparisonIdentity::new(
                    typed.case.case_id.clone(),
                    typed.case.input_id(),
                    provenance.source.clone(),
                    provenance.tool.clone(),
                    provenance.target.clone(),
                );
                identity.seed = Some(typed.case.seed);
                let sample = jet_foundation::TestingComparison::ComparisonSample::new(
                    identity,
                    reference_observed,
                    candidate_observed,
                )
                .with_replays(reference_replay, candidate_replay);
                inputs.push(input.clone());
                reference_values.push(reference_value);
                candidate_values.push(candidate_value);
                samples.push(sample.clone());
                jet_foundation::TestingComparison::compare_samples(relation.clone(), [sample])
            },
        )
    {
        Ok(run) => run,
        Err(error) => return Ok(CtValue::failed(Box::new(CtValue::Str(error.to_string())))),
    };
    let explored_cases = run.explored_cases;
    if let Some(artifact) = run.failure {
        return Ok(CtValue::failed(Box::new(CtValue::Str(format!(
            "history-artifact {}",
            artifact.json()
        )))));
    }
    let mut record = jet_foundation::TestingComparison::compare_samples_with_discarded(
        relation,
        samples,
        run.discarded_cases,
    );
    if explored_cases == 0 {
        record.status = jet_foundation::TestingComparison::ComparisonStatus::Unavailable;
        record.reason = Some("no history case produced an observation".to_string());
    }
    let case_ids = record
        .samples
        .iter()
        .map(|sample| CtValue::Str(sample.identity.case_id.clone()))
        .collect();
    Ok(CtValue::Present(Box::new(CtValue::Struct {
        type_name: "TestComparison".to_string(),
        fields: vec![
            (
                "status".to_string(),
                CtValue::Str(record.status.as_str().to_string()),
            ),
            (
                "relation".to_string(),
                CtValue::Str(record.relation.as_str().to_string()),
            ),
            (
                "source".to_string(),
                CtValue::Str(provenance.source.clone()),
            ),
            ("tool".to_string(), CtValue::Str(provenance.tool.clone())),
            ("target".to_string(), CtValue::Str(provenance.target.clone())),
            ("seed".to_string(), history_count_to_ct(seed)),
            ("case_ids".to_string(), CtValue::List(case_ids)),
            ("inputs".to_string(), CtValue::List(inputs)),
            (
                "reference".to_string(),
                CtValue::List(reference_values),
            ),
            (
                "candidate".to_string(),
                CtValue::List(candidate_values),
            ),
            (
                "first_difference".to_string(),
                CtValue::Int(record.first_difference.map(|index| index as i64).unwrap_or(-1)),
            ),
            (
                "reason".to_string(),
                CtValue::Str(record.reason.unwrap_or_else(|| {
                    format!("{explored_cases} observed history case(s) matched")
                })),
            ),
            ("universal_proof".to_string(), CtValue::Bool(false)),
        ],
    })))
}


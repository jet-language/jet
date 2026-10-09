//! Physical read loans: walk the evaluator's existing homes, never a marshalled clone.
//! Seed/verification seam, dies with the Rust evaluator at cutover.
use super::*;

#[derive(Clone, Copy)]
enum Home<'a> {
    Runtime(&'a RuntimeValue),
    Data(&'a MirEvalValue),
    Ambient(&'a CtValue),
    OptionalData(&'a MirEvalValue),
    OptionalAmbient(&'a CtValue),
}

// A stack-linked path avoids allocating/copying projection lists when an SSA
// alias or address leads through another frame's place.
#[derive(Clone, Copy)]
enum HomeSteps<'a> {
    Place(usize, &'a [MirProjection]),
    Alias(&'a [MoveStep]),
    One(usize, &'a MirProjection),
    EnumPayload(&'a str, usize),
}
struct HomePath<'a> {
    steps: HomeSteps<'a>,
    tail: Option<&'a HomePath<'a>>,
}

impl Machine<'_, '_> {
    pub(super) fn eval_string_home_call(&self, frame: usize, id: MirCoreCallId, route: MirPreludeCallId, args: &[jet_foundation::MIR::MirCallArg], span: Span) -> Result<Option<RuntimeValue>, Diagnostic> {
        let row = self.program.core_calls.iter().find(|row| row.id == id)
            .ok_or_else(|| mir_error_at("MIR String loan Core row is absent", span))?;
        if row.module != "core.mem" || !matches!(row.member.as_str(), "__string_data" | "__string_byte_len") {
            return Ok(None);
        }
        self.ensure_core_route(id, route, span)?;
        let [arg] = args else { return Err(mir_error_at("MIR String loan requires one read argument", span)); };
        if arg.access != MirAccess::Read || arg.spread {
            return Err(mir_error_at("MIR String loan requires a non-spread read argument", span));
        }
        let (pointer, length) = match arg.place {
            Some(place) => self.string_home_place(frame, place, None, span)?,
            None => self.string_home_value(frame, arg.value, None, span)?,
        };
        let word = if row.member == "__string_data" { pointer as i64 } else {
            i64::try_from(length).map_err(|_| mir_error_at("MIR String byte length exceeds the host range", span))?
        };
        Ok(Some(RuntimeValue::Data(MirEvalValue::Int(word))))
    }

    fn string_home_value(&self, frame: usize, id: MirValueId, tail: Option<&HomePath<'_>>, span: Span) -> Result<(u64, usize), Diagnostic> {
        let function = program_function(self.program, self.frames[frame].function)?;
        let operation = function.blocks.iter().flat_map(|block| &block.instructions)
            .find(|instruction| instruction.result == Some(id)).map(|instruction| &instruction.operation);
        if let Some(MirOperation::Global { name }) = operation {
            if let Some(home) = crate::Comptime::ForeignMemory::physical_global_string_home(
                name, |value| self.string_home_walk(Home::Ambient(value), tail, span),
            ) {
                return home;
            }
        }
        if let Some(MirOperation::Parameter { index, .. }) = operation {
            if function.params.get(*index).is_some_and(|parameter| parameter.access == MirAccess::Read) {
                if let Some((caller, block, ip)) = self.frames[frame].caller_site {
                    let caller_function = program_function(self.program, self.frames[caller].function)?;
                    let instruction = caller_function.blocks.iter().find(|row| row.id == block)
                        .and_then(|row| row.instructions.get(ip));
                    let args = instruction.and_then(|instruction| match &instruction.operation {
                        MirOperation::Call { args, .. } | MirOperation::IndirectCall { args, .. } => Some(args),
                        _ => None,
                    });
                    if let Some(arg) = args.and_then(|args| args.get(*index)).filter(|arg| arg.access == MirAccess::Read && !arg.spread) {
                        return match arg.place {
                            Some(place) => self.string_home_place(caller, place, tail, span),
                            None => self.string_home_value(caller, arg.value, tail, span),
                        };
                    }
                }
            }
        }
        if let Some(alias) = self.frames[frame].value_aliases.get(&id) {
            let path = HomePath { steps: HomeSteps::Alias(&alias.steps), tail };
            return self.string_home_place(alias.frame, alias.place, Some(&path), span);
        }
        if let Some(MirOperation::EnumPayload { subject, variant, index, .. }) = operation {
            let path = HomePath { steps: HomeSteps::EnumPayload(variant, *index), tail };
            return self.string_home_value(frame, *subject, Some(&path), span);
        }
        let projection = match operation {
            Some(MirOperation::Field { base, field }) => Some((*base, MirProjection::Field { field: *field, span })),
            Some(MirOperation::OptionValue { subject }) => Some((*subject, MirProjection::Payload { kind: MirPayloadKind::Option, span })),
            Some(MirOperation::ResultValue { subject, ok }) => Some((*subject, MirProjection::Payload { kind: MirPayloadKind::Result { ok: *ok }, span })),
            Some(MirOperation::Index { base, index, kind, call, location, .. }) => Some((*base, MirProjection::Index { index: *index, kind: *kind, call: *call, write_call: None, location: location.clone(), context: None, span })),
            _ => None,
        };
        if let Some((base, projection)) = projection {
            let path = HomePath { steps: HomeSteps::One(frame, &projection), tail };
            return self.string_home_value(frame, base, Some(&path), span);
        }
        let value = self.frames[frame].values.get(&id)
            .ok_or_else(|| mir_error_at("MIR String loan value is unavailable", span))?;
        self.string_home_walk(Home::Runtime(value), tail, span)
    }

    fn string_home_place(&self, frame: usize, id: MirPlaceId, tail: Option<&HomePath<'_>>, span: Span) -> Result<(u64, usize), Diagnostic> {
        if let Some(value) = self.frames[frame].place_overrides.get(&id) {
            return self.string_home_walk(Home::Runtime(value), tail, span);
        }
        let place = program_function(self.program, self.frames[frame].function)?.places.iter()
            .find(|place| place.id == id).ok_or_else(|| mir_error_at("MIR String loan place is unavailable", span))?;
        let path = HomePath { steps: HomeSteps::Place(frame, &place.projections), tail };
        match &place.base {
            MirPlaceBase::Local(local) => {
                let value = self.frames[frame].locals.get(local)
                    .ok_or_else(|| mir_error_at("MIR String loan local is unavailable", span))?;
                self.string_home_walk(Home::Runtime(value), Some(&path), span)
            }
            MirPlaceBase::Parameter(id) | MirPlaceBase::Capture(id) | MirPlaceBase::Temporary(id) => {
                if let Some(alias) = self.frames[frame].value_aliases.get(id) {
                    if alias.frame != frame || alias.place != place.id {
                        let alias_path = HomePath { steps: HomeSteps::Alias(&alias.steps), tail: Some(&path) };
                        return self.string_home_place(alias.frame, alias.place, Some(&alias_path), span);
                    }
                }
                if matches!(place.base, MirPlaceBase::Parameter(_))
                    || !self.frames[frame].value_aliases.contains_key(id)
                {
                    return self.string_home_value(frame, *id, Some(&path), span);
                }
                // A place with a Temporary base is itself an SSA alias; do not
                // follow that same alias again (ReadPlace would form a cycle).
                let value = self.frames[frame].values.get(id)
                    .ok_or_else(|| mir_error_at("MIR String loan base is unavailable", span))?;
                self.string_home_walk(Home::Runtime(value), Some(&path), span)
            }
            MirPlaceBase::Static(name) => {
                let values = self.static_values.borrow();
                let value = values.get(name).ok_or_else(|| mir_error_at("MIR String loan static is unavailable", span))?;
                self.string_home_walk(Home::Runtime(value), Some(&path), span)
            }
        }
    }

    fn string_home_walk(&self, mut home: Home<'_>, path: Option<&HomePath<'_>>, span: Span) -> Result<(u64, usize), Diagnostic> {
        match home {
            Home::Runtime(RuntimeValue::Address(address)) => {
                require_address_access(address, MirAccess::Read, span)?;
                return self.string_home_place(address.frame, address.place, path, span);
            }
            Home::Runtime(RuntimeValue::Data(data)) => home = Home::Data(data),
            Home::Runtime(RuntimeValue::Ambient(data)) => home = Home::Ambient(data),
            _ => {}
        }
        let Some(path) = path else {
            let text = match home {
                Home::Data(MirEvalValue::String(text)) | Home::Ambient(CtValue::Str(text)) => text,
                _ => return Err(mir_error_at("MIR String loan does not name a live String home", span)),
            };
            return Ok((text.as_ptr() as usize as u64, text.len()));
        };
        let (step, rest) = match path.steps {
            HomeSteps::Place(frame, projections) => {
                let Some((projection, rest)) = projections.split_first() else {
                    return self.string_home_walk(home, path.tail, span);
                };
                (HomeStep::Place(frame, projection), HomeSteps::Place(frame, rest))
            }
            HomeSteps::Alias(steps) => {
                let Some((step, rest)) = steps.split_first() else {
                    return self.string_home_walk(home, path.tail, span);
                };
                (HomeStep::Alias(step), HomeSteps::Alias(rest))
            }
            HomeSteps::One(frame, projection) => (HomeStep::Place(frame, projection), HomeSteps::Place(frame, &[])),
            HomeSteps::EnumPayload(variant, index) => {
                return self.string_home_walk(home_enum(home, variant, index, span)?, path.tail, span);
            }
        };
        let rest = HomePath { steps: rest, tail: path.tail };
        let child = match step {
            HomeStep::Place(_, MirProjection::Deref { .. }) | HomeStep::Alias(MoveStep::Deref) => home,
            HomeStep::Place(_, MirProjection::Field { field, .. }) | HomeStep::Alias(MoveStep::Field { field, .. }) => {
                let name = self.string_home_field_name(*field, span)?;
                match home {
                    Home::Runtime(RuntimeValue::Aggregate(fields)) => fields.iter().find(|(id, _)| id == field).map(|(_, value)| Home::Runtime(value)),
                    Home::Data(MirEvalValue::Struct { fields, .. }) => fields.iter().find(|(key, _)| key == name).map(|(_, value)| Home::Data(value)),
                    Home::Ambient(CtValue::Struct { fields, .. }) => fields.iter().find(|(key, _)| key == name).map(|(_, value)| Home::Ambient(value)),
                    Home::Data(MirEvalValue::Enum { args, .. }) => args.iter().find(|(key, _)| key.as_deref() == Some(name)).map(|(_, value)| Home::Data(value)),
                    Home::Ambient(CtValue::Enum { args, .. }) => args.iter().find(|(key, _)| key.as_deref() == Some(name)).map(|(_, value)| Home::Ambient(value)),
                    _ => None,
                }.ok_or_else(|| mir_error_at("MIR String loan field is absent", span))?
            }
            HomeStep::Place(frame, MirProjection::Index { index, .. }) => {
                let value = self.frames[frame].values.get(index).ok_or_else(|| mir_error_at("MIR String loan index is absent", span))?;
                let RuntimeValue::Data(value) = value else { return Err(mir_error_at("MIR String loan index is not data", span)); };
                home_index(home, &value, span)?
            }
            HomeStep::Alias(MoveStep::Index { value, .. }) => home_index(home, value, span)?,
            HomeStep::Place(frame, MirProjection::MapLookup { lookup, optional, .. }) => {
                let value = self.frames[frame].values.get(lookup)
                    .ok_or_else(|| mir_error_at("MIR String loan map lookup is absent", span))?;
                let RuntimeValue::Result { ok: true, value } = value else {
                    return Err(mir_error_at("MIR String loan map entry is absent", span));
                };
                let RuntimeValue::Data(MirEvalValue::Int(index)) = value.as_ref() else {
                    return Err(mir_error_at("MIR String loan map lookup is not an index", span));
                };
                let index = usize::try_from(*index)
                    .map_err(|_| mir_error_at("MIR String loan map lookup is out of bounds", span))?;
                match home {
                    Home::Data(MirEvalValue::Map(entries)) => entries.iter().nth(index).map(|(_, value)| {
                        if *optional { Home::OptionalData(value) } else { Home::Data(value) }
                    }),
                    Home::Ambient(CtValue::Map(entries)) => entries.iter().nth(index).map(|(_, value)| {
                        if *optional { Home::OptionalAmbient(value) } else { Home::Ambient(value) }
                    }),
                    _ => None,
                }.ok_or_else(|| mir_error_at("MIR String loan map lookup outlived its window", span))?
            }
            HomeStep::Place(_, MirProjection::Payload { kind, .. }) | HomeStep::Alias(MoveStep::Payload(kind)) => {
                match (home, kind) {
                    (Home::Runtime(RuntimeValue::Result { ok, value }), kind) if payload_result_side(kind) == Some(*ok) => Some(Home::Runtime(value)),
                    (Home::Data(MirEvalValue::Present(value)), MirPayloadKind::Option | MirPayloadKind::Result { ok: true }) => Some(Home::Data(value)),
                    (Home::Data(MirEvalValue::FailedTold(value)), MirPayloadKind::Result { ok: false }) => Some(Home::Data(value)),
                    (home, MirPayloadKind::Enum { variant, index, .. }) => Some(home_enum(home, variant, *index, span)?),
                    (Home::Ambient(CtValue::Present(value)), MirPayloadKind::Option | MirPayloadKind::Result { ok: true }) => Some(Home::Ambient(value)),
                    (Home::OptionalData(value), MirPayloadKind::Option) => Some(Home::Data(value)),
                    (Home::OptionalAmbient(value), MirPayloadKind::Option) => Some(Home::Ambient(value)),
                    _ => None,
                }.ok_or_else(|| mir_error_at("MIR String loan payload does not match its variant", span))?
            }
            HomeStep::Place(_, MirProjection::Range { .. }) | HomeStep::Alias(MoveStep::Range { .. }) => {
                return Err(mir_error_at("MIR String loan range must be indexed to a String home", span));
            }
        };
        self.string_home_walk(child, Some(&rest), span)
    }

    fn string_home_field_name(&self, id: MirFieldId, span: Span) -> Result<&str, Diagnostic> {
        for ty in &self.program.types {
            match &ty.kind {
                MirTypeDefKind::Struct { fields, .. } => if let Some(field) = fields.iter().find(|field| field.id == id) { return Ok(&field.name); },
                MirTypeDefKind::Enum { variants, .. } => for variant in variants {
                    if let MirVariantPayload::Named(fields) = &variant.payload {
                        if let Some(field) = fields.iter().find(|field| field.id == id) { return Ok(&field.name); }
                    }
                },
                _ => {}
            }
        }
        self.program.fields.iter().find(|row| row.id == id).map(|row| row.field.name.as_str())
            .ok_or_else(|| mir_error_at("MIR String loan field ID is absent", span))
    }
}

enum HomeStep<'a> { Place(usize, &'a MirProjection), Alias(&'a MoveStep) }

fn home_index<'a>(home: Home<'a>, index: &MirEvalValue, span: Span) -> Result<Home<'a>, Diagnostic> {
    let child = match (home, index) {
        (Home::Data(MirEvalValue::List(values)), MirEvalValue::Int(index)) => usize::try_from(*index).ok().and_then(|index| values.get(index)).map(Home::Data),
        (Home::Ambient(CtValue::List(values)), MirEvalValue::Int(index)) => usize::try_from(*index).ok().and_then(|index| values.get(index)).map(Home::Ambient),
        (Home::Data(MirEvalValue::Map(values)), index) => values.iter().find(|(key, _)| match (key, index) {
            (MirConstKey::String(a), MirEvalValue::String(b)) => a == b,
            (MirConstKey::Int(a), MirEvalValue::Int(b)) => a == b,
            _ => false,
        }).map(|(_, value)| Home::Data(value)),
        (Home::Ambient(CtValue::Map(values)), index) => values.iter().find(|(key, _)| match (key, index) {
            (crate::AST::CtKey::Str(a), MirEvalValue::String(b)) => a == b,
            (crate::AST::CtKey::Int(a), MirEvalValue::Int(b)) => a == b,
            _ => false,
        }).map(|(_, value)| Home::Ambient(value)),
        _ => None,
    };
    child.ok_or_else(|| mir_error_at("MIR String loan index is out of bounds", span))
}

fn home_enum<'a>(home: Home<'a>, variant: &str, index: usize, span: Span) -> Result<Home<'a>, Diagnostic> {
    let child = match home {
        Home::Data(MirEvalValue::Enum { variant: actual, args, .. }) if variant_name_matches(actual, variant) => args.get(index).map(|(_, value)| Home::Data(value)),
        Home::Ambient(CtValue::Enum { variant: actual, args, .. }) if variant_name_matches(actual, variant) => args.get(index).map(|(_, value)| Home::Ambient(value)),
        _ => None,
    };
    child.ok_or_else(|| mir_error_at("MIR String loan payload does not match its variant", span))
}

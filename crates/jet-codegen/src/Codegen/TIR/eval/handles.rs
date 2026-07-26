//! Exhaustive THandleOp dispatch (#777).
use crate::Comptime::Builtins::{apply_method, apply_mutating};
use crate::Comptime::CtValue;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Codegen::TIR::THandleOp;
use super::unsupported;

pub(super) fn eval_handle(
    op: &THandleOp,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match op {
        THandleOp::DurationNew { unit, float } => duration_new(recv, unit, *float, span),
        THandleOp::ClockNow => apply_method(recv, "now", args.to_vec(), span),
        THandleOp::ClockTick => apply_mutating(recv, "tick", args.to_vec(), span),
        THandleOp::ClockAdvance => apply_mutating(recv, "advance", args.to_vec(), span),
        THandleOp::ClockWait => apply_mutating(recv, "wait", args.to_vec(), span),
        THandleOp::RngInt => apply_mutating(recv, "int", args.to_vec(), span),
        THandleOp::RngFloat => apply_mutating(recv, "float", args.to_vec(), span),
        THandleOp::RngFloatRange => apply_mutating(recv, "float_range", args.to_vec(), span),
        THandleOp::RngBool => apply_mutating(recv, "bool", args.to_vec(), span),
        THandleOp::RngBoolP => apply_mutating(recv, "bool", args.to_vec(), span),
        THandleOp::RngNormal => apply_mutating(recv, "normal", args.to_vec(), span),
        THandleOp::RngExponential => apply_mutating(recv, "exponential", args.to_vec(), span),
        THandleOp::RngBytes => apply_mutating(recv, "bytes", args.to_vec(), span),
        THandleOp::RngSplit => apply_mutating(recv, "split", args.to_vec(), span),
        THandleOp::RngPick => apply_mutating(recv, "pick", args.to_vec(), span),
        THandleOp::RngWeightedPick => apply_mutating(recv, "weighted_pick", args.to_vec(), span),
        THandleOp::RngSample => apply_mutating(recv, "sample", args.to_vec(), span),
        THandleOp::RngShuffle => {
            let mut state = match recv {
                CtValue::Struct { type_name, fields }
                    if type_name == crate::Syntax::RNG_TYPE =>
                {
                    fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("state", CtValue::Int(state)) => Some(*state as u64),
                            _ => None,
                        })
                        .unwrap_or(0)
                }
                _ => {
                    return Err(unsupported("Rng.shuffle receiver", span));
                }
            };
            let value =
                crate::Comptime::apply_seeded_rng_method(&mut state, "shuffle", args, span)?;
            *recv = CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(state as i64))],
            };
            Ok(value)
        }
        THandleOp::SolverNew => {
            let seed = match recv {
                CtValue::Int(n) => *n,
                _ => {
                    return Err(unsupported("Solver.new expects an Int seed", span));
                }
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::SOLVER_TYPE.to_string(),
                fields: vec![
                    ("seed".to_string(), CtValue::Int(seed)),
                    ("checked".to_string(), CtValue::Int(0)),
                    ("failures".to_string(), CtValue::Int(0)),
                ],
            })
        }
        THandleOp::SolverRequire => apply_mutating(recv, "require", args.to_vec(), span),
        THandleOp::SolverFailureCount => apply_method(recv, "failure_count", args.to_vec(), span),
        THandleOp::SolverStatus => apply_method(recv, "status", args.to_vec(), span),
        THandleOp::MeasurementMethod { method } => {
            apply_method(recv, method, args.to_vec(), span)
        }
        THandleOp::CivilTimeMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::PreciseMethod { type_name, method } => {
            apply_method(recv, method, args.to_vec(), span).or_else(|_| {
                Err(unsupported(
                    &format!("precise `{type_name}.{method}`"),
                    span,
                ))
            })
        }
        THandleOp::DurationIn { .. } => apply_method(recv, "in", args.to_vec(), span),
        THandleOp::FileReaderReadLine => Err(unsupported("handle `FileReaderReadLine`", span)),
        THandleOp::FileWriterWriteLine => Err(unsupported("handle `FileWriterWriteLine`", span)),
        THandleOp::FileWriterFlush => Err(unsupported("handle `FileWriterFlush`", span)),
        THandleOp::JSONReaderNext => Err(unsupported("handle `JSONReaderNext`", span)),
        THandleOp::JSONWriterWrite => Err(unsupported("handle `JSONWriterWrite`", span)),
        THandleOp::JSONWriterFlush => Err(unsupported("handle `JSONWriterFlush`", span)),
        THandleOp::JSONWriterFinish => Err(unsupported("handle `JSONWriterFinish`", span)),
        THandleOp::JSONLReaderNext => Err(unsupported("handle `JSONLReaderNext`", span)),
        THandleOp::JSONLWriterWrite => Err(unsupported("handle `JSONLWriterWrite`", span)),
        THandleOp::JSONLWriterFlush => Err(unsupported("handle `JSONLWriterFlush`", span)),
        THandleOp::JSONLWriterFinish => Err(unsupported("handle `JSONLWriterFinish`", span)),
        THandleOp::CSVReaderNext => Err(unsupported("handle `CSVReaderNext`", span)),
        THandleOp::DataStreamNext => Err(unsupported("handle `DataStreamNext`", span)),
        THandleOp::XMLReaderNext => Err(unsupported("handle `XMLReaderNext`", span)),
        THandleOp::XMLWriterWrite => Err(unsupported("handle `XMLWriterWrite`", span)),
        THandleOp::XMLWriterFlush => Err(unsupported("handle `XMLWriterFlush`", span)),
        THandleOp::XMLWriterFinish => Err(unsupported("handle `XMLWriterFinish`", span)),
        THandleOp::CSVWriterWrite => Err(unsupported("handle `CSVWriterWrite`", span)),
        THandleOp::CSVWriterFlush => Err(unsupported("handle `CSVWriterFlush`", span)),
        THandleOp::CSVWriterFinish => Err(unsupported("handle `CSVWriterFinish`", span)),
        THandleOp::CBORReaderNext => Err(unsupported("handle `CBORReaderNext`", span)),
        THandleOp::CBORWriterWrite => Err(unsupported("handle `CBORWriterWrite`", span)),
        THandleOp::CBORWriterFlush => Err(unsupported("handle `CBORWriterFlush`", span)),
        THandleOp::CBORWriterFinish => Err(unsupported("handle `CBORWriterFinish`", span)),
        THandleOp::StdinReadLine => Err(unsupported("handle `StdinReadLine`", span)),
        THandleOp::StdoutWrite => Err(unsupported("handle `StdoutWrite`", span)),
        THandleOp::StdoutWriteLine => Err(unsupported("handle `StdoutWriteLine`", span)),
        THandleOp::StdoutWriteBytes => Err(unsupported("handle `StdoutWriteBytes`", span)),
        THandleOp::StdoutFlush => Err(unsupported("handle `StdoutFlush`", span)),
        THandleOp::StdoutIsTty => Err(unsupported("handle `StdoutIsTty`", span)),
        THandleOp::StderrWrite => Err(unsupported("handle `StderrWrite`", span)),
        THandleOp::StderrWriteLine => Err(unsupported("handle `StderrWriteLine`", span)),
        THandleOp::StderrWriteBytes => Err(unsupported("handle `StderrWriteBytes`", span)),
        THandleOp::StderrFlush => Err(unsupported("handle `StderrFlush`", span)),
        THandleOp::StderrIsTty => Err(unsupported("handle `StderrIsTty`", span)),
        THandleOp::StopwatchElapsedMillis => {
            Err(unsupported("handle `StopwatchElapsedMillis`", span))
        }
        THandleOp::GameSceneNew => Err(unsupported("handle `GameSceneNew`", span)),
        THandleOp::GameReplayRecord => Err(unsupported("handle `GameReplayRecord`", span)),
        THandleOp::GameBackendHeadless => Err(unsupported("handle `GameBackendHeadless`", span)),
        THandleOp::GameSceneOnFrame => Err(unsupported("handle `GameSceneOnFrame`", span)),
        THandleOp::GameSceneComponent => Err(unsupported("handle `GameSceneComponent`", span)),
        THandleOp::GameSceneQuery => Err(unsupported("handle `GameSceneQuery`", span)),
        THandleOp::GameAssetsImage => Err(unsupported("handle `GameAssetsImage`", span)),
        THandleOp::GameAssetsSound => Err(unsupported("handle `GameAssetsSound`", span)),
        THandleOp::GameInputBind => Err(unsupported("handle `GameInputBind`", span)),
        THandleOp::GameInputPressed => Err(unsupported("handle `GameInputPressed`", span)),
        THandleOp::TcpListenerAccept => Err(unsupported("handle `TcpListenerAccept`", span)),
        THandleOp::TcpListenerLocalAddr => Err(unsupported("handle `TcpListenerLocalAddr`", span)),
        THandleOp::TcpStreamRead => Err(unsupported("handle `TcpStreamRead`", span)),
        THandleOp::TcpStreamWrite => Err(unsupported("handle `TcpStreamWrite`", span)),
        THandleOp::TcpStreamPeerAddr => Err(unsupported("handle `TcpStreamPeerAddr`", span)),
        THandleOp::TcpStreamLocalAddr => Err(unsupported("handle `TcpStreamLocalAddr`", span)),
        THandleOp::TcpStreamClose => Err(unsupported("handle `TcpStreamClose`", span)),
        THandleOp::TcpStreamReadBytes => Err(unsupported("handle `TcpStreamReadBytes`", span)),
        THandleOp::TcpStreamReadText => Err(unsupported("handle `TcpStreamReadText`", span)),
        THandleOp::TcpStreamWriteBytes => Err(unsupported("handle `TcpStreamWriteBytes`", span)),
        THandleOp::TcpStreamWriteAllBytes => {
            Err(unsupported("handle `TcpStreamWriteAllBytes`", span))
        }
        THandleOp::TcpStreamWriteText => Err(unsupported("handle `TcpStreamWriteText`", span)),
        THandleOp::TcpStreamShutdown => Err(unsupported("handle `TcpStreamShutdown`", span)),
        THandleOp::TcpStreamReady => Err(unsupported("handle `TcpStreamReady`", span)),
        THandleOp::UdpSocketReady => Err(unsupported("handle `UdpSocketReady`", span)),
        THandleOp::UdpSocketClose => Err(unsupported("handle `UdpSocketClose`", span)),
        THandleOp::UdpSocketReceiveDeadline => {
            Err(unsupported("handle `UdpSocketReceiveDeadline`", span))
        }
        THandleOp::UdpSocketSendToDeadline => {
            Err(unsupported("handle `UdpSocketSendToDeadline`", span))
        }
        THandleOp::UnixListenerAcceptDeadline => {
            Err(unsupported("handle `UnixListenerAcceptDeadline`", span))
        }
        THandleOp::UnixStreamReadDeadline => {
            Err(unsupported("handle `UnixStreamReadDeadline`", span))
        }
        THandleOp::UnixStreamWriteAllDeadline => {
            Err(unsupported("handle `UnixStreamWriteAllDeadline`", span))
        }
        THandleOp::UnixStreamReady => Err(unsupported("handle `UnixStreamReady`", span)),
        THandleOp::UnixStreamClose => Err(unsupported("handle `UnixStreamClose`", span)),
        THandleOp::UnixStreamSetTimeout => Err(unsupported("handle `UnixStreamSetTimeout`", span)),
        THandleOp::TlsStreamReadDeadline => {
            Err(unsupported("handle `TlsStreamReadDeadline`", span))
        }
        THandleOp::TlsStreamWriteAllDeadline => {
            Err(unsupported("handle `TlsStreamWriteAllDeadline`", span))
        }
        THandleOp::TlsStreamReady => Err(unsupported("handle `TlsStreamReady`", span)),
        THandleOp::TlsStreamClose => Err(unsupported("handle `TlsStreamClose`", span)),
        THandleOp::TlsStreamCloseWrite => Err(unsupported("handle `TlsStreamCloseWrite`", span)),
        THandleOp::TlsStreamPeerIdentity => {
            Err(unsupported("handle `TlsStreamPeerIdentity`", span))
        }
        THandleOp::TlsClientConfigDefault => {
            Err(unsupported("handle `TlsClientConfigDefault`", span))
        }
        THandleOp::TlsClientConfigWithAlpn => {
            Err(unsupported("handle `TlsClientConfigWithAlpn`", span))
        }
        THandleOp::TlsRootCertificatesFromPem => {
            Err(unsupported("handle `TlsRootCertificatesFromPem`", span))
        }
        THandleOp::TlsClientIdentityFromPem => {
            Err(unsupported("handle `TlsClientIdentityFromPem`", span))
        }
        THandleOp::TlsClientConfigWithTrust => {
            Err(unsupported("handle `TlsClientConfigWithTrust`", span))
        }
        THandleOp::TlsClientConfigWithIdentity => {
            Err(unsupported("handle `TlsClientConfigWithIdentity`", span))
        }
        THandleOp::TlsClientConfigWithVersionBounds => {
            Err(unsupported("handle `TlsClientConfigWithVersionBounds`", span))
        }
        THandleOp::HttpClientNew => Err(unsupported("handle `HttpClientNew`", span)),
        THandleOp::AllocAlloc => Err(unsupported("handle `AllocAlloc`", span)),
        THandleOp::AllocReset => Err(unsupported("handle `AllocReset`", span)),
        THandleOp::HttpReqField(_) => Err(unsupported("handle `HttpReqField`", span)),
        THandleOp::HttpReqHeader => Err(unsupported("handle `HttpReqHeader`", span)),
        THandleOp::HttpReqParam => Err(unsupported("handle `HttpReqParam`", span)),
        THandleOp::HttpReqTrailers => Err(unsupported("handle `HttpReqTrailers`", span)),
        THandleOp::HttpRespField(_) => Err(unsupported("handle `HttpRespField`", span)),
        THandleOp::HttpRespHeader => Err(unsupported("handle `HttpRespHeader`", span)),
        THandleOp::HttpRespTrailers => Err(unsupported("handle `HttpRespTrailers`", span)),
        THandleOp::ArgsSpecFlag => Err(unsupported("handle `ArgsSpecFlag`", span)),
        THandleOp::ArgsSpecFlagShort => Err(unsupported("handle `ArgsSpecFlagShort`", span)),
        THandleOp::ArgsSpecOption => Err(unsupported("handle `ArgsSpecOption`", span)),
        THandleOp::ArgsSpecOptionShort => Err(unsupported("handle `ArgsSpecOptionShort`", span)),
        THandleOp::ArgsSpecOptionDefault => {
            Err(unsupported("handle `ArgsSpecOptionDefault`", span))
        }
        THandleOp::ArgsSpecOptionEnv => Err(unsupported("handle `ArgsSpecOptionEnv`", span)),
        THandleOp::ArgsSpecOptionInt => Err(unsupported("handle `ArgsSpecOptionInt`", span)),
        THandleOp::ArgsSpecOptionFloat => Err(unsupported("handle `ArgsSpecOptionFloat`", span)),
        THandleOp::ArgsSpecOptionChoice => Err(unsupported("handle `ArgsSpecOptionChoice`", span)),
        THandleOp::ArgsSpecRepeat => Err(unsupported("handle `ArgsSpecRepeat`", span)),
        THandleOp::ArgsSpecRequiredOption => {
            Err(unsupported("handle `ArgsSpecRequiredOption`", span))
        }
        THandleOp::ArgsSpecPositional => Err(unsupported("handle `ArgsSpecPositional`", span)),
        THandleOp::ArgsSpecSubcommand => Err(unsupported("handle `ArgsSpecSubcommand`", span)),
        THandleOp::ArgsSpecVersion => Err(unsupported("handle `ArgsSpecVersion`", span)),
        THandleOp::ArgsSpecCompletion => Err(unsupported("handle `ArgsSpecCompletion`", span)),
        THandleOp::ArgsSpecHelp => Err(unsupported("handle `ArgsSpecHelp`", span)),
        THandleOp::ArgsSpecParse => Err(unsupported("handle `ArgsSpecParse`", span)),
        THandleOp::ParsedArgsFlag => Err(unsupported("handle `ParsedArgsFlag`", span)),
        THandleOp::ParsedArgsOption => Err(unsupported("handle `ParsedArgsOption`", span)),
        THandleOp::ParsedArgsOptionInt => Err(unsupported("handle `ParsedArgsOptionInt`", span)),
        THandleOp::ParsedArgsOptionFloat => {
            Err(unsupported("handle `ParsedArgsOptionFloat`", span))
        }
        THandleOp::ParsedArgsOptions => Err(unsupported("handle `ParsedArgsOptions`", span)),
        THandleOp::ParsedArgsSubcommand => Err(unsupported("handle `ParsedArgsSubcommand`", span)),
        THandleOp::ParsedArgsPositional => Err(unsupported("handle `ParsedArgsPositional`", span)),
        THandleOp::ProcessSpecMethod { .. } => Err(unsupported("handle `ProcessSpecMethod`", span)),
        THandleOp::ProcessChildMethod { .. } => {
            Err(unsupported("handle `ProcessChildMethod`", span))
        }
        THandleOp::ProcessStdinWrite => Err(unsupported("handle `ProcessStdinWrite`", span)),
        THandleOp::ReflectValueTypeName => Err(unsupported("handle `ReflectValueTypeName`", span)),
        THandleOp::ReflectValueDisplay => Err(unsupported("handle `ReflectValueDisplay`", span)),
        THandleOp::ReflectValueFields => Err(unsupported("handle `ReflectValueFields`", span)),
        THandleOp::ReflectFieldName => Err(unsupported("handle `ReflectFieldName`", span)),
        THandleOp::ReflectFieldValue => Err(unsupported("handle `ReflectFieldValue`", span)),
        THandleOp::TaskJoin => Err(unsupported("handle `TaskJoin`", span)),
        THandleOp::TaskDetach => Err(unsupported("handle `TaskDetach`", span)),
        THandleOp::TaskPause => Err(unsupported("handle `TaskPause`", span)),
        THandleOp::TaskResume => Err(unsupported("handle `TaskResume`", span)),
        THandleOp::TaskCancel => Err(unsupported("handle `TaskCancel`", span)),
        THandleOp::TaskTrace => Err(unsupported("handle `TaskTrace`", span)),
        THandleOp::ChannelReceive => Err(unsupported("handle `ChannelReceive`", span)),
        THandleOp::SenderSend => Err(unsupported("handle `SenderSend`", span)),
        THandleOp::HttpRouterRegister { .. } => {
            Err(unsupported("handle `HttpRouterRegister`", span))
        }
        THandleOp::MathMethod {
            method,
            reduce_op,
            ..
        } => {
            let mut argv = args.to_vec();
            if let Some(op) = reduce_op {
                // Lowering resolves the `ReduceOp` value into `reduce_op` and drops
                // the source arg — restore it for MathLayout::apply_method.
                argv.insert(0, CtValue::Str(op.clone()));
            }
            apply_method(recv, method, argv, span)
        }
        THandleOp::ReactiveGet => Err(unsupported("handle `ReactiveGet`", span)),
        THandleOp::ReactiveSet => Err(unsupported("handle `ReactiveSet`", span)),
        THandleOp::ReactiveEffectMethod { .. } => {
            Err(unsupported("handle `ReactiveEffectMethod`", span))
        }
        THandleOp::EventMethod { .. } => Err(unsupported("handle `EventMethod`", span)),
        THandleOp::WatchMethod { .. } => Err(unsupported("handle `WatchMethod`", span)),
        THandleOp::LayoutMethod { .. } => Err(unsupported("handle `LayoutMethod`", span)),
        THandleOp::LoadableMethod { method } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::ExpiringMethod { .. } => Err(unsupported("handle `ExpiringMethod`", span)),
        THandleOp::SketchMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::UrlMimeMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::EmailMethod { method } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::RegexMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::HttpClientMethod { .. } => Err(unsupported("handle `HttpClientMethod`", span)),
        THandleOp::HttpServerMethod { .. } => Err(unsupported("handle `HttpServerMethod`", span)),
        THandleOp::DataTreeField => Err(unsupported("handle `DataTreeField`", span)),
        THandleOp::DataTreeAt => Err(unsupported("handle `DataTreeAt`", span)),
        THandleOp::DataTreeInt => Err(unsupported("handle `DataTreeInt`", span)),
        THandleOp::DataTreeText => Err(unsupported("handle `DataTreeText`", span)),
        THandleOp::DataTreeBool => Err(unsupported("handle `DataTreeBool`", span)),
        THandleOp::DataTreeFloat => Err(unsupported("handle `DataTreeFloat`", span)),
        THandleOp::DataTreeDecode(_) => Err(unsupported("handle `DataTreeDecode`", span)),
        THandleOp::SerdeEncode => Err(unsupported("handle `SerdeEncode`", span)),
        THandleOp::JsonField => Err(unsupported("handle `JsonField`", span)),
        THandleOp::JsonAt => Err(unsupported("handle `JsonAt`", span)),
        THandleOp::JsonInt => Err(unsupported("handle `JsonInt`", span)),
        THandleOp::JsonText => Err(unsupported("handle `JsonText`", span)),
        THandleOp::JsonBool => Err(unsupported("handle `JsonBool`", span)),
        THandleOp::JsonFloat => Err(unsupported("handle `JsonFloat`", span)),
        THandleOp::PathFrom => Err(unsupported("handle `PathFrom`", span)),
        THandleOp::PathJoin => Err(unsupported("handle `PathJoin`", span)),
        THandleOp::PathParent => Err(unsupported("handle `PathParent`", span)),
        THandleOp::PathExtension => Err(unsupported("handle `PathExtension`", span)),
        THandleOp::PathStem => Err(unsupported("handle `PathStem`", span)),
        THandleOp::PathToString => Err(unsupported("handle `PathToString`", span)),
        THandleOp::PathWriteAtomic => Err(unsupported("handle `PathWriteAtomic`", span)),
        THandleOp::PathWalk => Err(unsupported("handle `PathWalk`", span)),
        THandleOp::UiBackendMethod { .. } => Err(unsupported("handle `UiBackendMethod`", span)),
        THandleOp::DevServerMethod { .. } => Err(unsupported("handle `DevServerMethod`", span)),
        THandleOp::WebAppMethod { .. } => Err(unsupported("handle `WebAppMethod`", span)),
        THandleOp::DbQuery => Err(unsupported("handle `DbQuery`", span)),
        THandleOp::DbQueryOne => Err(unsupported("handle `DbQueryOne`", span)),
        THandleOp::DbExecute => Err(unsupported("handle `DbExecute`", span)),
        THandleOp::DbBegin => Err(unsupported("handle `DbBegin`", span)),
        THandleOp::DbCommit => Err(unsupported("handle `DbCommit`", span)),
        THandleOp::DbRollback => Err(unsupported("handle `DbRollback`", span)),
        THandleOp::DbClose => Err(unsupported("handle `DbClose`", span)),
        THandleOp::DbValueInt => Err(unsupported("handle `DbValueInt`", span)),
        THandleOp::DbValueFloat => Err(unsupported("handle `DbValueFloat`", span)),
        THandleOp::DbValueText => Err(unsupported("handle `DbValueText`", span)),
        THandleOp::DbValueBool => Err(unsupported("handle `DbValueBool`", span)),
        THandleOp::DbValueIsNull => Err(unsupported("handle `DbValueIsNull`", span)),
        THandleOp::PluginCall => Err(unsupported("handle `PluginCall`", span)),
        THandleOp::PluginCallInt => Err(unsupported("handle `PluginCallInt`", span)),
        THandleOp::ReaderOver => Err(unsupported("handle `ReaderOver`", span)),
        THandleOp::ReaderReadU8 => Err(unsupported("handle `ReaderReadU8`", span)),
        THandleOp::ReaderReadU16Le => Err(unsupported("handle `ReaderReadU16Le`", span)),
        THandleOp::ReaderReadU16Be => Err(unsupported("handle `ReaderReadU16Be`", span)),
        THandleOp::ReaderReadU32Le => Err(unsupported("handle `ReaderReadU32Le`", span)),
        THandleOp::ReaderReadU32Be => Err(unsupported("handle `ReaderReadU32Be`", span)),
        THandleOp::ReaderReadU64Le => Err(unsupported("handle `ReaderReadU64Le`", span)),
        THandleOp::ReaderReadU64Be => Err(unsupported("handle `ReaderReadU64Be`", span)),
        THandleOp::ReaderTake => Err(unsupported("handle `ReaderTake`", span)),
        THandleOp::ReaderRemaining => Err(unsupported("handle `ReaderRemaining`", span)),
        THandleOp::ReaderAtEnd => Err(unsupported("handle `ReaderAtEnd`", span)),
        THandleOp::CursorOver => Err(unsupported("handle `CursorOver`", span)),
        THandleOp::CursorTakeUntil => Err(unsupported("handle `CursorTakeUntil`", span)),
        THandleOp::CursorSkipWs => Err(unsupported("handle `CursorSkipWs`", span)),
        THandleOp::CursorTakePattern { .. } => {
            Err(unsupported("handle `CursorTakePattern`", span))
        }
        THandleOp::ReaderTakePattern { .. } => {
            Err(unsupported("handle `ReaderTakePattern`", span))
        }
    }
}

fn duration_new(
    recv: &CtValue,
    unit: &str,
    float: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let scale = match unit {
        "Milliseconds" => 1_i64,
        "Seconds" => 1_000,
        "Minutes" => 60_000,
        "Hours" => 3_600_000,
        _ => return Err(unsupported(&format!("Duration unit `{unit}`"), span)),
    };
    let ms = if float {
        let n = match recv {
            CtValue::Float(n) => n.as_f64(),
            CtValue::Int(n) => *n as f64,
            _ => {
                return Err(unsupported(
                    "Duration constructor expects a numeric value",
                    span,
                ));
            }
        };
        let scaled = n * scale as f64;
        (scaled.is_finite()
            && scaled >= i64::MIN as f64
            && scaled < 9_223_372_036_854_775_808.0)
            .then_some(scaled.trunc() as i64)
    } else {
        match recv {
            CtValue::Int(n) => n.checked_mul(scale),
            _ => None,
        }
    };
    Ok(match ms {
        Some(ms) => CtValue::ResOk(Box::new(CtValue::Struct {
            type_name: crate::Syntax::DURATION_TYPE.to_string(),
            fields: vec![("ms".to_string(), CtValue::Int(ms))],
        })),
        None => CtValue::ResErr(Box::new(CtValue::Struct {
            type_name: crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
            fields: vec![(
                "reason".to_string(),
                CtValue::Str(
                    "duration must be finite and inside the supported range".to_string(),
                ),
            )],
        })),
    })
}

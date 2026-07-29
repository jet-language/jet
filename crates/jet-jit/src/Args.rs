//! `core.args` hosts (#729) — `include!` canonical Args.rs runtime.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

#[allow(dead_code, unused_imports, clippy::all)]
mod runtime {
    use super::Concurrency;

    trait JetShow {
        fn jet_show(&self) -> String;
    }
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Args.rs");

    #[derive(Clone)]
    pub(crate) struct Spec(JetArgsSpec);
    #[derive(Clone)]
    pub(crate) struct Parsed(JetParsedArgs);

    fn clone_str(id: i64) -> String {
        Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
    }

    fn alloc_str(s: String) -> i64 {
        Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
    }

    fn list_of_strings(list: i64) -> Vec<String> {
        Concurrency::with_runtime_mut(|rt| {
            let len = rt.heap.list_len(list).unwrap_or(0);
            let mut out = Vec::with_capacity(len as usize);
            for i in 0..len {
                let sid = rt.heap.list_get_int(list, i).unwrap_or(0);
                out.push(rt.heap.clone_string(sid).unwrap_or_default());
            }
            out
        })
    }

    fn list_from_strings(items: Vec<String>) -> i64 {
        Concurrency::with_runtime_mut(|rt| {
            let list = rt.heap.alloc_empty_list();
            for s in items {
                let sid = rt.heap.alloc_string(s);
                rt.heap.list_push_int(list, sid).expect("jit args list");
            }
            list
        })
    }

    fn push_spec(spec: JetArgsSpec) -> i64 {
        Concurrency::with_runtime_mut(|rt| {
            rt.args_specs.push(Spec(spec));
            rt.args_specs.len() as i64
        })
    }

    fn take_spec(handle: i64) -> JetArgsSpec {
        Concurrency::with_runtime_mut(|rt| -> Option<JetArgsSpec> {
            Some(
                rt.args_specs
                    .get(handle.saturating_sub(1) as usize)
                    .cloned()
                    .expect("jit args spec: bad handle")
                    .0,
            )
        })
        .expect("jit args: no active runtime")
    }

    fn replace_spec(handle: i64, spec: JetArgsSpec) -> i64 {
        Concurrency::with_runtime_mut(|rt| {
            let slot = rt
                .args_specs
                .get_mut(handle.saturating_sub(1) as usize)
                .expect("jit args spec: bad handle");
            *slot = Spec(spec);
            handle
        })
    }

    fn push_parsed(parsed: JetParsedArgs) -> i64 {
        Concurrency::with_runtime_mut(|rt| {
            rt.args_parsed.push(Parsed(parsed));
            rt.args_parsed.len() as i64
        })
    }

    fn with_parsed<R: Default>(handle: i64, f: impl FnOnce(&JetParsedArgs) -> R) -> R {
        Concurrency::with_runtime_mut(|rt| {
            let p = rt
                .args_parsed
                .get(handle.saturating_sub(1) as usize)
                .expect("jit args parsed: bad handle");
            f(&p.0)
        })
    }

    fn result_ok(bits: u64) -> i64 {
        Concurrency::with_runtime_mut(|rt| {
            rt.results.push(crate::JitResultValue { ok: true, bits });
            rt.results.len() as i64
        })
    }

    fn result_err(msg: &str) -> i64 {
        Concurrency::with_runtime_mut(|rt| {
            let sid = rt.heap.alloc_string(msg.to_string());
            rt.results.push(crate::JitResultValue {
                ok: false,
                bits: sid as u64,
            });
            rt.results.len() as i64
        })
    }

    fn pack_option_str(opt: Option<String>) -> i64 {
        match opt {
            Some(s) => alloc_str(s).wrapping_add(1),
            None => 0,
        }
    }

    fn pack_option_i64(opt: Option<i64>) -> i64 {
        match opt {
            Some(v) => v.wrapping_add(1),
            None => 0,
        }
    }

    pub(super) extern "C" fn jet_jit_args_spec() -> i64 {
        push_spec(jet_args_spec())
    }

    pub(super) extern "C" fn jet_jit_args_flag(h: i64, name: i64, help: i64) -> i64 {
        let spec = jet_args_flag(take_spec(h), &clone_str(name), &clone_str(help));
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_flag_short(
        h: i64,
        name: i64,
        short: i64,
        help: i64,
    ) -> i64 {
        let spec = jet_args_flag_short(
            take_spec(h),
            &clone_str(name),
            &clone_str(short),
            &clone_str(help),
        );
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_option(
        h: i64,
        name: i64,
        help: i64,
        meta: i64,
    ) -> i64 {
        let spec = jet_args_option(
            take_spec(h),
            &clone_str(name),
            &clone_str(help),
            &clone_str(meta),
        );
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_option_default(
        h: i64,
        name: i64,
        help: i64,
        meta: i64,
        default: i64,
    ) -> i64 {
        let spec = jet_args_option_default(
            take_spec(h),
            &clone_str(name),
            &clone_str(help),
            &clone_str(meta),
            &clone_str(default),
        );
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_option_int(
        h: i64,
        name: i64,
        help: i64,
        meta: i64,
    ) -> i64 {
        let spec = jet_args_option_int(
            take_spec(h),
            &clone_str(name),
            &clone_str(help),
            &clone_str(meta),
        );
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_option_choice(
        h: i64,
        name: i64,
        help: i64,
        meta: i64,
        choices: i64,
    ) -> i64 {
        let spec = jet_args_option_choice(
            take_spec(h),
            &clone_str(name),
            &clone_str(help),
            &clone_str(meta),
            &clone_str(choices),
        );
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_repeat(
        h: i64,
        name: i64,
        help: i64,
        meta: i64,
    ) -> i64 {
        let spec = jet_args_repeat(
            take_spec(h),
            &clone_str(name),
            &clone_str(help),
            &clone_str(meta),
        );
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_positional(
        h: i64,
        name: i64,
        help: i64,
    ) -> i64 {
        let spec = jet_args_positional(take_spec(h), &clone_str(name), &clone_str(help));
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_subcommand(
        h: i64,
        name: i64,
        help: i64,
        sub: i64,
    ) -> i64 {
        let nested = take_spec(sub);
        let spec = jet_args_subcommand(take_spec(h), &clone_str(name), &clone_str(help), nested);
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_version(h: i64, version: i64) -> i64 {
        let spec = jet_args_version(take_spec(h), &clone_str(version));
        replace_spec(h, spec)
    }

    pub(super) extern "C" fn jet_jit_args_help(h: i64) -> i64 {
        alloc_str(take_spec(h).help())
    }

    pub(super) extern "C" fn jet_jit_args_completion(h: i64, shell: i64) -> i64 {
        alloc_str(jet_args_completion(&take_spec(h), &clone_str(shell)))
    }

    pub(super) extern "C" fn jet_jit_args_parse(h: i64, argv: i64) -> i64 {
        match jet_args_parse(&take_spec(h), &list_of_strings(argv)) {
            Ok(parsed) => result_ok(push_parsed(parsed) as u64),
            Err(msg) => result_err(&msg),
        }
    }

    pub(super) extern "C" fn jet_jit_args_parse_or_exit(h: i64, argv: i64) -> i64 {
        push_parsed(jet_args_parse_or_exit(
            &take_spec(h),
            &list_of_strings(argv),
        ))
    }

    pub(super) extern "C" fn jet_jit_parsed_flag(h: i64, name: i64) -> i8 {
        with_parsed(h, |p| i8::from(jet_parsed_flag(p, &clone_str(name))))
    }

    pub(super) extern "C" fn jet_jit_parsed_option(h: i64, name: i64) -> i64 {
        with_parsed(h, |p| pack_option_str(jet_parsed_option(p, &clone_str(name))))
    }

    pub(super) extern "C" fn jet_jit_parsed_option_int(h: i64, name: i64) -> i64 {
        with_parsed(h, |p| pack_option_i64(jet_parsed_option_int(p, &clone_str(name))))
    }

    pub(super) extern "C" fn jet_jit_parsed_option_float_opt(h: i64, name: i64) -> i64 {
        with_parsed(h, |p| match jet_parsed_option_float(p, &clone_str(name)) {
            Some(v) => (v.to_bits() as i64).wrapping_add(1),
            None => 0,
        })
    }

    pub(super) extern "C" fn jet_jit_parsed_options(h: i64, name: i64) -> i64 {
        with_parsed(h, |p| list_from_strings(jet_parsed_options(p, &clone_str(name))))
    }

    pub(super) extern "C" fn jet_jit_parsed_positional(h: i64, idx: i64) -> i64 {
        with_parsed(h, |p| pack_option_str(jet_parsed_positional(p, idx)))
    }

    pub(super) extern "C" fn jet_jit_parsed_subcommand(h: i64) -> i64 {
        with_parsed(h, |p| pack_option_str(jet_parsed_subcommand(p)))
    }
}

pub(crate) type ArgsSpec = runtime::Spec;
pub(crate) type ParsedArgs = runtime::Parsed;

pub(crate) struct ArgsHostFns {
    pub spec: FuncId,
    pub flag: FuncId,
    pub flag_short: FuncId,
    pub option: FuncId,
    pub option_default: FuncId,
    pub option_int: FuncId,
    pub option_choice: FuncId,
    pub repeat: FuncId,
    pub positional: FuncId,
    pub subcommand: FuncId,
    pub version: FuncId,
    pub help: FuncId,
    pub completion: FuncId,
    pub parse: FuncId,
    pub parse_or_exit: FuncId,
    pub parsed_flag: FuncId,
    pub parsed_option: FuncId,
    pub parsed_option_int: FuncId,
    pub parsed_option_float: FuncId,
    pub parsed_options: FuncId,
    pub parsed_positional: FuncId,
    pub parsed_subcommand: FuncId,
}

pub(crate) fn register_args_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_args_spec", runtime::jet_jit_args_spec as *const u8);
    builder.symbol("jet_jit_args_flag", runtime::jet_jit_args_flag as *const u8);
    builder.symbol(
        "jet_jit_args_flag_short",
        runtime::jet_jit_args_flag_short as *const u8,
    );
    builder.symbol("jet_jit_args_option", runtime::jet_jit_args_option as *const u8);
    builder.symbol(
        "jet_jit_args_option_default",
        runtime::jet_jit_args_option_default as *const u8,
    );
    builder.symbol(
        "jet_jit_args_option_int",
        runtime::jet_jit_args_option_int as *const u8,
    );
    builder.symbol(
        "jet_jit_args_option_choice",
        runtime::jet_jit_args_option_choice as *const u8,
    );
    builder.symbol("jet_jit_args_repeat", runtime::jet_jit_args_repeat as *const u8);
    builder.symbol(
        "jet_jit_args_positional",
        runtime::jet_jit_args_positional as *const u8,
    );
    builder.symbol(
        "jet_jit_args_subcommand",
        runtime::jet_jit_args_subcommand as *const u8,
    );
    builder.symbol("jet_jit_args_version", runtime::jet_jit_args_version as *const u8);
    builder.symbol("jet_jit_args_help", runtime::jet_jit_args_help as *const u8);
    builder.symbol(
        "jet_jit_args_completion",
        runtime::jet_jit_args_completion as *const u8,
    );
    builder.symbol("jet_jit_args_parse", runtime::jet_jit_args_parse as *const u8);
    builder.symbol(
        "jet_jit_args_parse_or_exit",
        runtime::jet_jit_args_parse_or_exit as *const u8,
    );
    builder.symbol("jet_jit_parsed_flag", runtime::jet_jit_parsed_flag as *const u8);
    builder.symbol(
        "jet_jit_parsed_option",
        runtime::jet_jit_parsed_option as *const u8,
    );
    builder.symbol(
        "jet_jit_parsed_option_int",
        runtime::jet_jit_parsed_option_int as *const u8,
    );
    builder.symbol(
        "jet_jit_parsed_option_float_opt",
        runtime::jet_jit_parsed_option_float_opt as *const u8,
    );
    builder.symbol(
        "jet_jit_parsed_options",
        runtime::jet_jit_parsed_options as *const u8,
    );
    builder.symbol(
        "jet_jit_parsed_positional",
        runtime::jet_jit_parsed_positional as *const u8,
    );
    builder.symbol(
        "jet_jit_parsed_subcommand",
        runtime::jet_jit_parsed_subcommand as *const u8,
    );
}

pub(crate) fn declare_args_host_fns(module: &mut JITModule) -> Result<ArgsHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut nullary = Signature::new(cc);
    nullary.returns.push(AbiParam::new(types::I64));
    let mut unary = Signature::new(cc);
    unary.params.push(AbiParam::new(types::I64));
    unary.returns.push(AbiParam::new(types::I64));
    let mut binary = Signature::new(cc);
    binary.params.push(AbiParam::new(types::I64));
    binary.params.push(AbiParam::new(types::I64));
    binary.returns.push(AbiParam::new(types::I64));
    let mut binary_i8 = Signature::new(cc);
    binary_i8.params.push(AbiParam::new(types::I64));
    binary_i8.params.push(AbiParam::new(types::I64));
    binary_i8.returns.push(AbiParam::new(types::I8));
    let mut ternary = Signature::new(cc);
    for _ in 0..3 {
        ternary.params.push(AbiParam::new(types::I64));
    }
    ternary.returns.push(AbiParam::new(types::I64));
    let mut quaternary = Signature::new(cc);
    for _ in 0..4 {
        quaternary.params.push(AbiParam::new(types::I64));
    }
    quaternary.returns.push(AbiParam::new(types::I64));
    let mut quinary = Signature::new(cc);
    for _ in 0..5 {
        quinary.params.push(AbiParam::new(types::I64));
    }
    quinary.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(ArgsHostFns {
        spec: import("jet_jit_args_spec", &nullary)?,
        flag: import("jet_jit_args_flag", &ternary)?,
        flag_short: import("jet_jit_args_flag_short", &quaternary)?,
        option: import("jet_jit_args_option", &quaternary)?,
        option_default: import("jet_jit_args_option_default", &quinary)?,
        option_int: import("jet_jit_args_option_int", &quaternary)?,
        option_choice: import("jet_jit_args_option_choice", &quinary)?,
        repeat: import("jet_jit_args_repeat", &quaternary)?,
        positional: import("jet_jit_args_positional", &ternary)?,
        subcommand: import("jet_jit_args_subcommand", &quaternary)?,
        version: import("jet_jit_args_version", &binary)?,
        help: import("jet_jit_args_help", &unary)?,
        completion: import("jet_jit_args_completion", &binary)?,
        parse: import("jet_jit_args_parse", &binary)?,
        parse_or_exit: import("jet_jit_args_parse_or_exit", &binary)?,
        parsed_flag: import("jet_jit_parsed_flag", &binary_i8)?,
        parsed_option: import("jet_jit_parsed_option", &binary)?,
        parsed_option_int: import("jet_jit_parsed_option_int", &binary)?,
        parsed_option_float: import("jet_jit_parsed_option_float_opt", &binary)?,
        parsed_options: import("jet_jit_parsed_options", &binary)?,
        parsed_positional: import("jet_jit_parsed_positional", &binary)?,
        parsed_subcommand: import("jet_jit_parsed_subcommand", &unary)?,
    })
}

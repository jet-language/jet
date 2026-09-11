//! Host-owned remote-builder bindings (D-JPK-REMOTE1).

use crate::OutputMode;
use jet::Comptime::Build::RemoteBuildBinding;
use jet::ExitCodes;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};

pub(crate) fn run_remote(args: &[String], mode: OutputMode) -> ! {
    match args.get(1).map(String::as_str) {
        Some("bind") => bind(args, mode),
        Some("list") => list(mode),
        Some("remove") => remove(args),
        Some("help") | None => {
            println!(
                "jet remote — manage host-owned remote builders\n\n  jet remote bind <name> --root <path> --credential-file <path> --trust-domain <name> [--execute] [--cache-read] [--cache-write] [--fallback-local]\n  jet remote list\n  jet remote remove <name>"
            );
            std::process::exit(ExitCodes::OK);
        }
        Some(other) => fail(&format!("unknown remote subcommand `{other}`")),
    }
}

fn bind(args: &[String], mode: OutputMode) -> ! {
    let Some(builder) = args.get(2).filter(|value| !value.starts_with('-')) else {
        fail("jet remote bind needs a builder name");
    };
    let root = value_flag(args, "--root");
    let credential_file = value_flag(args, "--credential-file");
    let trust_domain = value_flag(args, "--trust-domain");
    let Some(root) = root else {
        fail("jet remote bind needs `--root <absolute-path>`")
    };
    let Some(credential_file) = credential_file else {
        fail("jet remote bind needs `--credential-file <path>`")
    };
    let Some(trust_domain) = trust_domain else {
        fail("jet remote bind needs `--trust-domain <name>`")
    };
    let worker_id = value_flag(args, "--worker-id").unwrap_or_else(|| "worker".to_string());
    let platform = value_flag(args, "--platform")
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH));
    let abi = value_flag(args, "--abi").unwrap_or_else(|| "native".to_string());
    let timeout_ms = value_flag(args, "--timeout-ms")
        .map(|value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|_| fail("`--timeout-ms` must be a positive integer"))
        })
        .unwrap_or(30_000);
    if timeout_ms == 0 {
        fail("`--timeout-ms` must be a positive integer");
    }
    let cache_read = has_flag(args, "--cache-read");
    let cache_write = has_flag(args, "--cache-write");
    let execute = has_flag(args, "--execute");
    let fallback_local = has_flag(args, "--fallback-local");
    let known = [
        "--root",
        "--credential-file",
        "--trust-domain",
        "--worker-id",
        "--platform",
        "--abi",
        "--timeout-ms",
        "--cache-read",
        "--cache-write",
        "--execute",
        "--fallback-local",
        jet::CLI::MACHINE_OUTPUT_FLAG,
    ];
    for argument in args.iter().skip(3) {
        let head = argument
            .split_once('=')
            .map(|(head, _)| head)
            .unwrap_or(argument);
        if argument.starts_with('-') && !known.contains(&head) {
            fail(&format!("unknown remote bind flag `{argument}`"));
        }
    }
    let binding = RemoteBuildBinding::bind_host(
        builder.clone(),
        root,
        credential_file,
        trust_domain,
        worker_id,
        platform,
        abi,
        cache_read,
        cache_write,
        execute,
        fallback_local,
        timeout_ms,
    )
    .unwrap_or_else(|error| fail(&error));
    if mode.json {
        let fields = StatusFields::new()
            .with("builder", binding.builder.clone())
            .with("root", binding.root.to_string_lossy().into_owned())
            .with("trust_domain", binding.trust_domain.clone())
            .with("worker_id", binding.worker_id.clone())
            .with("platform", binding.platform.clone())
            .with("abi", binding.abi.clone());
        println!(
            "{}",
            StatusEnvelope::new("remote.bind", true)
                .with_fields(fields)
                .json()
        );
    } else {
        println!("bound remote builder `{}`", binding.builder);
    }
    std::process::exit(ExitCodes::OK);
}

fn list(mode: OutputMode) -> ! {
    let names = RemoteBuildBinding::list_host().unwrap_or_else(|error| fail(&error));
    if mode.json {
        let builders = StatusValue::array(names.iter().filter_map(|name| {
            RemoteBuildBinding::load_host(name).ok().map(|binding| {
                StatusValue::object(
                    StatusFields::new()
                        .with("name", binding.builder)
                        .with("root", binding.root.to_string_lossy().into_owned())
                        .with("trust_domain", binding.trust_domain)
                        .with("worker_id", binding.worker_id)
                        .with("platform", binding.platform)
                        .with("abi", binding.abi)
                        .with("cache_read", binding.cache_read)
                        .with("cache_write", binding.cache_write)
                        .with("execute", binding.execute)
                        .with("fallback_local", binding.fallback_local),
                )
            })
        }));
        println!(
            "{}",
            StatusEnvelope::new("remote.list", true)
                .with_field("builders", builders)
                .json()
        );
    } else if names.is_empty() {
        println!("no remote builders are bound");
    } else {
        for name in names {
            match RemoteBuildBinding::load_host(&name) {
                Ok(binding) => println!(
                    "{}\t{}\t{}\t{}",
                    binding.builder,
                    binding.root.display(),
                    binding.trust_domain,
                    capabilities(&binding),
                ),
                Err(error) => eprintln!("{name}: {error}"),
            }
        }
    }
    std::process::exit(ExitCodes::OK);
}

fn remove(args: &[String]) -> ! {
    let Some(builder) = args.get(2).filter(|value| !value.starts_with('-')) else {
        fail("jet remote remove needs a builder name");
    };
    RemoteBuildBinding::remove_host(builder).unwrap_or_else(|error| fail(&error));
    println!("removed remote builder `{builder}`");
    std::process::exit(ExitCodes::OK);
}

fn capabilities(binding: &RemoteBuildBinding) -> String {
    [
        binding.cache_read.then_some("cache-read"),
        binding.cache_write.then_some("cache-write"),
        binding.execute.then_some("execute"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(",")
}

fn value_flag(args: &[String], name: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(index, argument)| {
        argument
            .strip_prefix(&format!("{name}="))
            .map(str::to_string)
            .or_else(|| {
                (argument == name)
                    .then(|| args.get(index + 1).cloned())
                    .flatten()
            })
    })
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter()
        .any(|argument| argument == name || argument == &format!("{name}=true"))
}


fn fail(message: &str) -> ! {
    crate::cli_error!(@fix "E2104", message, "run `jet remote help`");
    std::process::exit(ExitCodes::USAGE);
}

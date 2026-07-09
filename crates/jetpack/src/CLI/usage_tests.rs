fn usage() -> String {
    let bin = Syntax::JETPACK_BINARY_NAME;
    let pack = Syntax::ENV_FILE;
    // Bold section headers on a TTY only; the text is identical when piped.
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let h = |s: &str| {
        if color {
            format!("\x1b[1m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    };
    format!(
        "\
{title}

{envs}
  {bin} enter                          enter the project shell described by ./{pack}
  {bin} enter -- cmd                   run a command in the project shell, then exit
  {bin} enter -p <pkg>...              add ad-hoc nixpkgs packages, undeclared
  {bin} enter --flake                  force a foreign flake.nix/devenv.nix shell
  {bin} run   <source>:<package>       enter a temporary shell with that package
  {bin} run   <source>:<package> -- cmd run a command in that environment, then exit
  {bin} run                            enter the shell described by ./{pack}
  {bin} dev                            realize the env, then run the project's fn dev()

{manifest}
  {bin} add    <source>:<package>      add a package to ./{pack}
  {bin} add    <Component>             copy a starter component into ./components
  {bin} remove <source>:<package>      remove a package from ./{pack}
  {bin} bridge flake                   print an env.* shim translated from ./flake.nix

{store}
  {bin} build [<source>:<package>]     realize a package/environment, don't enter
  {bin} list                           show realized packages
  {bin} hangar du                      honest per-object hangar disk usage
  {bin} vendor [<dir>]                 write vendored + hash-pinned sources
  {bin} audit                          read build provenance (runs nothing)
  {bin} clean                          collect stale hangar objects + optimize
  {bin} search <query>                 search the local offline package index
  {bin} info <source>.<package>         show local offline package metadata
  {bin} explain <ref>                  show resolution path and latest build status
  {bin} logs <pkg> --json              show persisted per-step build logs
  {bin} override draft <ref> --patch <file>
                                      draft reviewed workspace overlay policy

{machines}
  jet os check <host>                  validate ./config.jet system.<host>
  jet os plan <host> --json            print checked plan/proof input without building
  jet os proof <host> --json           print latest generation proof/provenance facts
  jet os build <host>                  build a named jetos generation
  jet os switch <host> [--name <name>] build + activate a named generation
  jet os generations [<host>]          list generations newest first
  jet os rollback <host> [<name>]      activate a previous generation
  jet os init <host> [--manual <path>] write starter ./config.jet
  jet os lift <host> [<root>]          draft ./config.jet from a host root
  jet os import <flake-or-dir> --host <host>
                                      import NixOS/flake-parts/Home Manager facts
  jet os image <host> [--manual <path>] write jetos hybrid ISO media/proof
  jet os vm prove <host> --disk <path> boot installer, install, reboot, prove
  jet os vm test <vmtest> --disk <path> run declared VM scenario proof
  jetos studio [path] --host <host>    open installed jetos Studio app
  jetos studio [path] --serve 127.0.0.1:7417 serve browser/edit fallback
  {bin} push <fleet>                   validate a fleet's hosts (deploy is gated)
  {bin} services up   [<name>]         start dev services declared under env.*
  {bin} services down [<name>]         stop them
  {bin} services health [<name>]       one-shot readiness check
  {bin} services logs <name>           print a service's captured stdout/stderr
  {bin} image <name>                   build a declared `.Oci` image into a native OCI layout
  {bin} image <name> --push <ref>      (gated on TLS support, E1268 — not yet)

{trust}
  {bin} trust list                    show package/build/env/service/image/fleet/jetos grants
  {bin} trust explain [<grant>]        explain exact authority and revocation key
  {bin} trust grant <grant>            add a reviewed local grant
  {bin} trust revoke <grant>           drop a grant; next risky action asks again
  {bin} config trust add <pattern>     pre-authorize matching project paths
  {bin} config trust list              show trusted hashes and patterns
  {bin} config trust remove <pattern>  drop a trusted pattern
  {bin} config sandbox require         refuse unsandboxed build fallback
  {bin} config sandbox allow           allow fallback with L0205 warning

{refs}
  nixpkgs:fastfetch                    a package from nixpkgs
  github:owner/repo                    a Jet pack repo (or a flake fallback)
  path:./my-env                        a local pack/flake directory

{components}
  Button, Label, Input, Container      starter kit — ownable, editable .jet source

{flags}
  --no-color                           disable colored output (also: NO_COLOR)
  --offline                            resolve from fixtures only, never network
  -y, --yes                            apply a mutation plan without prompting
  --shell-on-fail                      after a failed build, open a shell in preserved scratch
  --fixtures <dir>                     read provider output from captured fixtures
  --trust                              skip the trust prompt for this one run
  --scope <user|repo>                  (trust grant) where the grant applies
  -p <pkg>...                          (enter) ad-hoc nixpkgs packages, not declared anywhere
  --flake                              (enter) force the foreign flake.nix/devenv.nix fallback
  --pure                               (enter) isolate the shell from the host environment
  --push <ref>                         (image) push after building — gated on TLS, E1268
  --name <name>                        (os switch) override generation name
  --manual <path>                      (os init/image) record manual disk path
  --disk <path>                        (os vm prove) target qcow2/raw disk image
  --headless                           (jetos studio) print app path without opening
  --serve <addr>                       (jetos studio) run local projection service
  --host <host>                        (os import/studio) select system host
",
        title = h(&format!("{bin} — Jet's package manager (Phase 1)")),
        envs = h("environments:"),
        manifest = h("manifest:"),
        store = h("store:"),
        machines = h("machines:"),
        trust = h("trust:"),
        refs = h("refs:"),
        components = h("components:"),
        flags = h("flags:"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_trailing_command() {
        let args: Vec<String> = ["nixpkgs:jq", "--", "jq", "--version"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.positional, vec!["nixpkgs:jq"]);
        assert_eq!(p.command, Some(vec!["jq".into(), "--version".into()]));
    }

    #[test]
    fn parses_flags() {
        let fixtures = std::env::temp_dir().join("fx");
        let fixtures_arg = fixtures.to_string_lossy().to_string();
        let args: Vec<String> = ["--no-color", "--fixtures", &fixtures_arg, "-y", "nixpkgs:jq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert!(p.flags.no_color);
        assert!(p.flags.assume_yes);
        assert_eq!(p.flags.fixtures, Some(fixtures));
        assert_eq!(p.positional, vec!["nixpkgs:jq"]);
    }

    #[test]
    fn parses_long_yes_flag() {
        let args: Vec<String> = ["--yes", "nixpkgs:jq"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert!(p.flags.assume_yes);
        assert_eq!(p.positional, vec!["nixpkgs:jq"]);
    }

    // ── U16: -p / --flake / --pure ──

    #[test]
    fn dash_p_collects_packages_until_dash_dash() {
        let args: Vec<String> = ["-p", "nodejs", "ripgrep", "--", "some-command"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.flags.packages, vec!["nodejs", "ripgrep"]);
        assert_eq!(p.command, Some(vec!["some-command".to_string()]));
        assert!(p.positional.is_empty());
    }

    #[test]
    fn dash_p_stops_at_next_flag() {
        let args: Vec<String> = ["-p", "nodejs", "--no-color"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.flags.packages, vec!["nodejs"]);
        assert!(p.flags.no_color);
    }

    #[test]
    fn repeated_dash_p_groups_accumulate() {
        let args: Vec<String> = ["-p", "nodejs", "-p", "ripgrep", "fd"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert_eq!(p.flags.packages, vec!["nodejs", "ripgrep", "fd"]);
    }

    #[test]
    fn parses_flake_and_pure_flags() {
        let args: Vec<String> = ["--flake", "--pure"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_args(&args);
        assert!(p.flags.flake);
        assert!(p.flags.pure);
    }

    // ── U16: foreign-flake detection ordering ──

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jetpack_cli_u16_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn foreign_flake_not_detected_without_a_flake_file() {
        let dir = scratch("no_flake");
        assert_eq!(foreign_flake_path(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn foreign_flake_prefers_flake_nix_over_devenv() {
        let dir = scratch("both");
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        std::fs::write(dir.join("devenv.nix"), "{ }").unwrap();
        assert_eq!(foreign_flake_path(&dir), Some(dir.join("flake.nix")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_declares_env_false_with_no_env_file() {
        let dir = scratch("no_env");
        assert!(!project_declares_env(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_declares_env_true_for_typed_module_with_packages() {
        let dir = scratch("typed_env");
        std::fs::write(
            dir.join(Syntax::ENV_FILE),
            "module env.dev { packages: [ripgrep] }\n",
        )
        .unwrap();
        assert!(project_declares_env(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_declares_env_true_for_phase1_directive_surface() {
        let dir = scratch("phase1_env");
        std::fs::write(
            dir.join(Syntax::ENV_FILE),
            "use jetpack as pkg;\npub fn shell() -> [JSON] {\n    return [pkg.packages([\"ripgrep\"])];\n}\n",
        )
        .unwrap();
        assert!(project_declares_env(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn foreign_flake_detection_ordering_only_when_no_env_declared() {
        // The core U16 ordering rule: a project that already declares env.*
        // is never silently swapped for a foreign flake, even if one exists —
        // only `--flake` can force that.
        let dir = scratch("ordering");
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        std::fs::write(
            dir.join(Syntax::ENV_FILE),
            "module env.dev { packages: [ripgrep] }\n",
        )
        .unwrap();
        let has_foreign = foreign_flake_path(&dir).is_some();
        let declares_env = project_declares_env(&dir);
        assert!(has_foreign);
        assert!(declares_env);
        // Auto-detection condition from `cmd_enter`: foreign.is_some() &&
        // !project_declares_env(..) — false here, so the project's own env
        // wins unless `--flake` is passed explicitly.
        assert!(!(has_foreign && !declares_env));
        std::fs::remove_dir_all(&dir).ok();
    }
}

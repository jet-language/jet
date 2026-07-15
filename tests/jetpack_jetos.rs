//! JetOS generation/proof tests (Tower card #367 slice 6 split).
//!
//! Covers `jet os` import/build/switch/generations, cachyos/systemd/gnome
//! host config requirements, and the VM prove/test/image proof chain. Split
//! out of the former `tests/jetpack.rs`; see `tests/jetpack_studio.rs` for
//! the JetOS Studio slice and `tests/support/jetpack_fixtures.rs` for shared
//! helpers.

use std::fs;
use std::process::Command;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

#[test]
fn os_import_writes_semantic_nixos_facts_with_audit() {
    let src = Scratch::new("os-import-src");
    fs::write(
        src.join("jetos-import-facts.json"),
        r#"{
  "host": "halcyon",
  "target": "linux.x64",
  "nixpkgs": "github@NixOS/nixpkgs/nixos-24.05",
  "packages": ["git", "ripgrep", "jetbrains.idea-ultimate"],
  "services": ["openssh", "pipewire"],
  "options": {
    "network.hostName": "halcyon",
    "services.openssh.enable": true,
    "boot.loader": ".Limine"
  },
  "flakePartsModules": ["./nix/hosts/halcyon.nix"],
  "homeManagerModules": ["./home/nate.nix"],
  "users": [
    {
      "name": "nate",
      "home": "/home/nate",
      "groups": ["wheel"],
      "packages": ["neovim", "ghostty"],
      "homeManager": true
    }
  ],
  "omissions": ["programs.firefox.profiles need profile-specific Canvas editing"]
}"#,
    )
    .unwrap();
    let out_dir = Scratch::new("os-import-out");
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--user",
            "nate",
            "--write",
            "--out",
            out_dir.path.to_str().unwrap(),
            "--no-color",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = fs::read_to_string(out_dir.join("config.jet")).unwrap();
    assert!(config.contains("system.halcyon"), "{config}");
    assert!(config.contains("nixpkgs: github@NixOS/nixpkgs/nixos-24.05"), "{config}");
    assert!(config.contains("packages: [nixpkgs.[git, ripgrep]]"), "{config}");
    assert!(config.contains("openssh: { enable: true"), "{config}");
    assert!(config.contains("user.nate.packages: [nixpkgs.[neovim, ghostty]]"), "{config}");
    assert!(config.contains("user.nate.homeManager: true"), "{config}");
    let audit = fs::read_to_string(out_dir.join("jetos-import-audit.json")).unwrap();
    assert!(audit.contains("\"mode\":\"semantic-facts\""), "{audit}");
    assert!(audit.contains("jetbrains.idea-ultimate"), "{audit}");
    assert!(audit.contains("programs.firefox.profiles"), "{audit}");
}


#[test]
fn os_import_live_recovers_package_provenance_from_flake_inputs() {
    let src = Scratch::new("os-import-provenance-src");
    let tools = Scratch::new("os-import-provenance-tools");
    fs::write(
        src.join("flake.nix"),
        "{\n  outputs = { ... }: { nixosConfigurations.halcyon = { }; };\n}\n",
    )
    .unwrap();
    fs::write(
        src.join("flake.lock"),
        r#"{
  "nodes": {
    "nixpkgs": {"locked": {"owner": "NixOS", "repo": "nixpkgs", "rev": "fef9403a3e4d31b0a23f0bacebbec52c248fbb51"}},
    "zen-beta": {"locked": {"owner": "0xc000022070", "repo": "zen-browser-flake", "rev": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
  }
}"#,
    )
    .unwrap();
    // Live extractor returns an external package; nixpkgs probe reports it
    // unresolvable; zen-beta probe reports it resolved (empty unresolvable list).
    let live = r#"{"host":"halcyon","stateVersion":"26.05","tz":"UTC","locale":"en_US.UTF-8","keyboard":"us","desktopGnome":false,"desktopPlasma":false,"dmGdm":false,"dmSddm":false,"loaderLimine":true,"loaderSystemdBoot":false,"efiTouch":false,"kernelName":"linux","kernelParams":[],"sysctl":{},"firewallTcp":[],"firewallUdp":[],"nameservers":[],"networkmanager":false,"zramEnable":false,"zramPercent":0,"svcOpenssh":false,"svcPipewire":false,"svcRtkit":false,"svcTailscale":false,"svcLibvirtd":false,"svcDocker":false,"svcFlatpak":false,"svcSteam":false,"svcGamemode":false,"svcPcscd":false,"svcBluetooth":false,"packages":["git","zen-browser"],"users":[],"hm":[]}"#;
    let stub = format!(
        r#"#!/bin/sh
# Package resolvability probe uses getFlake + resolves; live extractor uses --apply.
case " $* " in
  *'getFlake'*'0xc000022070'*|*'getFlake'*'zen-browser-flake'*)
    printf '%s\n' '[]'
    exit 0
    ;;
  *'getFlake'*)
    printf '%s\n' '["zen-browser"]'
    exit 0
    ;;
  *'--apply'*)
    printf '%s\n' '{live}'
    exit 0
    ;;
esac
exit 0
"#
    );
    fs::create_dir_all(&tools.path).unwrap();
    write_executable(&tools.join("nix"), &stub);
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = String::from_utf8_lossy(&out.stdout);
    assert!(
        config.contains("zen_beta: github@0xc000022070/zen-browser-flake/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "extra source must be pinned from flake.lock:\n{config}"
    );
    assert!(
        config.contains("zen_beta.[zen-browser]")
            || config.contains("packages: [nixpkgs.[git], zen_beta.[zen-browser]]"),
        "recovered package must be sourced from zen_beta:\n{config}"
    );
    assert!(
        !config.contains("package-provenance import will recover it"),
        "recovered packages must not stay as deferred omissions:\n{config}"
    );
}


#[test]
fn os_import_live_semantic_eval_maps_real_options() {
    let src = Scratch::new("os-import-live-src");
    let tools = Scratch::new("os-import-live-tools");
    write_live_import_fixture(&src.path, &tools.path, Some(LIVE_IMPORT_EVAL_JSON));
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = String::from_utf8_lossy(&out.stdout);
    assert!(config.contains("nixpkgs: github@NixOS/nixpkgs/fef9403a3e4d31b0a23f0bacebbec52c248fbb51"), "{config}");
    assert!(config.contains("network.hostName: \"halcyon\""), "{config}");
    assert!(config.contains("network.networkmanager.enable: true"), "{config}");
    assert!(config.contains("network.firewall.allowedTcpPorts: [22, 443]"), "{config}");
    assert!(config.contains("filesystem.timeZone: \"America/New_York\""), "{config}");
    assert!(config.contains("boot.loader: .Limine"), "{config}");
    assert!(config.contains("boot.kernel: .CachyOS"), "{config}");
    assert!(config.contains("services.desktop.plasma.enable: true"), "{config}");
    assert!(config.contains("services.displayManager: \"sddm\""), "{config}");
    assert!(config.contains("services.audio.pipewire.enable: true"), "{config}");
    assert!(config.contains("services.virtualization.libvirtd.enable: true"), "{config}");
    assert!(config.contains("services.gaming.steam.enable: true"), "{config}");
    assert!(config.contains("performance.sysctl.vm.swappiness: 10"), "{config}");
    assert!(config.contains("performance.zram.memoryPercent: 25"), "{config}");
    assert!(config.contains("users.nate.shell: nixpkgs.fish"), "{config}");
    assert!(config.contains("user.nate.homeManager: true"), "{config}");
    assert!(config.contains("apps.program.git.enable: true"), "{config}");
    assert!(config.contains("apps.program.starship.enable: true"), "{config}");
    assert!(
        config.contains("services.virtualization.docker.enable: true"),
        "{config}"
    );
    assert!(config.contains("packages: [nixpkgs.[git, ripgrep]]"), "{config}");
    assert!(config.contains("openssh: { enable: true"), "{config}");
    assert!(config.contains("tailscale: { enable: true"), "{config}");
}


#[test]
fn os_import_live_semantic_eval_reports_omissions() {
    let src = Scratch::new("os-import-live-audit-src");
    let tools = Scratch::new("os-import-live-audit-tools");
    write_live_import_fixture(&src.path, &tools.path, Some(LIVE_IMPORT_EVAL_JSON));
    let out_dir = Scratch::new("os-import-live-audit-out");
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--write",
            "--out",
            out_dir.path.to_str().unwrap(),
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let audit = fs::read_to_string(out_dir.join("jetos-import-audit.json")).unwrap();
    assert!(audit.contains("\"mode\":\"semantic-eval\""), "{audit}");
    assert!(audit.contains("jetbrains.idea-ultimate"), "{audit}");
    assert!(
        audit.contains("no `nix-cachyos-kernel` pin"),
        "{audit}"
    );
    assert!(
        audit.contains("stylix theming is enabled upstream"),
        "{audit}"
    );
    assert!(
        !audit.contains("virtualisation.docker.enable has no jetos option"),
        "docker must map to services.virtualization.docker.enable, not omit: {audit}"
    );
    assert!(
        !audit.contains("Home Manager program `starship`"),
        "known HM programs must map to apps.program.*, not omit: {audit}"
    );
}


#[test]
fn os_import_live_eval_failure_is_loud() {
    let src = Scratch::new("os-import-live-fail-src");
    let tools = Scratch::new("os-import-live-fail-tools");
    write_live_import_fixture(&src.path, &tools.path, None);
    let out = jet()
        .args([
            "os",
            "import",
            src.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--no-color",
        ])
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1289"), "{stderr}");
    assert!(stderr.contains("attribute missing"), "{stderr}");
    assert!(stderr.contains("--facts-only"), "{stderr}");
}


#[test]
fn os_import_missing_source_has_snapshot() {
    let out = jet()
        .args([
            "os",
            "import",
            "/definitely/not/here",
            "--host",
            "halcyon",
            "--no-color",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_jetos_stderr_snapshot(
        "import_missing_source",
        &String::from_utf8_lossy(&out.stderr),
    );
}


#[test]
fn os_lift_is_audited_facts_only_import_draft() {
    let src = Scratch::new("os-lift-src");
    fs::write(
        src.join("flake.nix"),
        r#"{
  inputs.flake-parts.url = "github:hercules-ci/flake-parts";
  inputs.home-manager.url = "github:nix-community/home-manager";
  outputs = { self, nixpkgs, ... }: {
    nixosConfigurations = { laptop = nixpkgs.lib.nixosSystem {}; };
  };
}"#,
    )
    .unwrap();
    let out = jet()
        .args([
            "os",
            "lift",
            "laptop",
            src.path.to_str().unwrap(),
            "--no-color",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("Generated by `jet os import`"), "{stdout}");
    assert!(stdout.contains("system.laptop"), "{stdout}");
    assert!(stderr.contains("facts-only"), "{stderr}");
}


#[test]
fn os_build_realizes_selected_system_offline() {
    // I5/D-JPK-OSVERB1/D-JPK-OSHOST1: `jet os build <host>` loads config.jet
    // from the current repo, selects system.<host>, and realizes its packages
    // into a named generation.
    // System named <host>, and realizes its packages into a system generation —
    // fully offline (the packages come from a first-party `core` source repo, so
    // no nix). The store lives under a scratch JETPACK_ROOT.
    let root = Scratch::new("os-build-root");
    let run = || {
        jet()
            .args([
                "os",
                "build",
                "halcyon",
                "--name",
                "fixture-source-built",
                "--no-color",
                "--offline",
            ])
            .current_dir(config_example_dir())
            .env("JETPACK_ROOT", &root.path)
            .env("PATH", "/usr/bin:/bin") // no nix on PATH
            .output()
            .unwrap()
    };
    let out = run();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.lines().any(|line| {
            line.contains("building system")
                && line.contains(" -> hello · resolving")
                && !line.contains('\u{1b}')
        }),
        "plain jetos build must project its real package edge without ANSI: {stderr}"
    );
    for pkg in ["hello", "btop"] {
        assert!(stderr.contains(pkg), "expected `{pkg}` in output: {stderr}");
    }
    assert!(stderr.contains("halcyon"), "stderr: {stderr}");
    assert!(stderr.contains("generation"), "stderr: {stderr}");
    let cached = run();
    assert!(
        cached.status.success(),
        "cached generation stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&cached.stderr).contains("resolving"),
        "an exact published retry must validate in place without rebuilding: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    // A generation directory was assembled under the managed system store.
    assert!(
        root.join("systems").is_dir(),
        "expected a systems dir under the root"
    );
    let generation = root.join("systems/generations/fixture-source-built");
    let root_proof = fs::read_to_string(generation.join("generation-root.json")).unwrap();
    assert!(
        root_proof.contains("\"kind\":\"jetos.generation-root.v1\"")
            && root_proof.contains("\"source_proof_sha256\":\"")
            && root_proof.contains("\"files_proof_sha256\":\"")
            && root_proof.contains("\"output_digests\":[\"sha256-"),
        "generation root must bind complete source/files proof and Hangar digests: {root_proof}"
    );
    assert!(
        !fs::read(generation.join("generation-files.proof"))
            .unwrap()
            .is_empty(),
        "generation files proof must be durable before ledger publication"
    );
    assert_eq!(
        fs::read_to_string(root.join("systems/generations.log"))
            .unwrap()
            .lines()
            .filter(|line| line.contains("\tfixture-source-built\t"))
            .count(),
        1,
        "exact retry must not duplicate a generation ledger row"
    );
    let journal = root.join("hangar/lifecycle-db/journal");
    let lifecycle = fs::read_dir(&journal)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect::<String>();
    assert!(
        lifecycle.contains("external-consumer") && lifecycle.contains("commit"),
        "generation must own a committed typed ExternalConsumer root: {lifecycle}"
    );
    let kernel = fs::read_to_string(generation.join("boot/kernel")).unwrap();
    let initrd = fs::read_to_string(generation.join("boot/initrd")).unwrap();
    assert!(
        kernel.contains("fixture-built cachyos kernel"),
        "kernel should come from source/build.sh: {kernel}"
    );
    assert!(
        initrd.contains("fixture-built cachyos initrd"),
        "initrd should come from source/build.sh: {initrd}"
    );
    assert_no_ephemeral_links(&generation);
    let hello = Command::new(generation.join("sw/bin/hello")).output().unwrap();
    assert!(hello.status.success());
    assert!(
        String::from_utf8_lossy(&hello.stdout).contains("hello"),
        "generation-owned executable must survive lease close and FD reuse"
    );
}

#[test]
fn os_generation_recovers_prepared_root_after_durable_ledger_crash_window() {
    let root = Scratch::new("os-generation-root-recovery");
    let project = Scratch::new("os-generation-root-project");
    copy_dir_recursive(&config_example_dir(), &project.path);
    let run = |failpoint: Option<&str>| {
        let mut command = jet();
        command
            .args([
                "os",
                "build",
                "halcyon",
                "--name",
                "root-recovery",
                "--no-color",
                "--offline",
            ])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("PATH", "/usr/bin:/bin");
        if let Some(failpoint) = failpoint {
            command.env("JET_TEST_GENERATION_FAILPOINT", failpoint);
        }
        command.output().unwrap()
    };

    for failpoint in [
        "after-root-prepare",
        "after-ledger-partial",
        "after-ledger-append",
        "after-ledger-durable",
        "after-ledger",
        "after-commit",
    ] {
        let interrupted = run(Some(failpoint));
        assert_eq!(
            interrupted.status.code(),
            Some(2),
            "failpoint {failpoint} did not stop publication: {}",
            String::from_utf8_lossy(&interrupted.stderr)
        );
        let generation = root.join("systems/generations/root-recovery");
        assert!(
            generation.join("generation-root.json").is_file(),
            "{failpoint} exposed a ledger/root before the immutable directory"
        );
    }

    let recovered = run(None);
    assert!(
        recovered.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let ledger = fs::read_to_string(root.join("systems/generations.log")).unwrap();
    assert_eq!(ledger.lines().count(), 1, "recovery must reuse exact ledger row");
    assert_eq!(
        ledger.split('\t').count(),
        5,
        "ledger row must bind the immutable root witness: {ledger}"
    );
    let journal = root.join("hangar/lifecycle-db/journal");
    let lifecycle = fs::read_dir(journal)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect::<String>();
    assert!(
        lifecycle.contains("commit"),
        "retry must commit the exact prepared root: {lifecycle}"
    );

    let source = project.join("config.jet");
    let original_source = fs::read(&source).unwrap();
    let mut changed_source = original_source.clone();
    changed_source.extend_from_slice(b"\n// changed after immutable publication\n");
    fs::write(&source, changed_source).unwrap();
    let changed = run(None);
    assert_eq!(changed.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&changed.stderr).contains("already published"),
        "changed source must not replace a retained generation: {}",
        String::from_utf8_lossy(&changed.stderr)
    );
    fs::write(&source, original_source).unwrap();

    let generation = root.join("systems/generations/root-recovery");
    let sealed = generation.join("proof.txt");
    let original_sealed = fs::read(&sealed).unwrap();
    fs::write(&sealed, b"tampered\n").unwrap();
    let tampered = run(None);
    assert_eq!(tampered.status.code(), Some(2));
    assert_eq!(fs::read(&sealed).unwrap(), b"tampered\n");
    fs::write(&sealed, original_sealed).unwrap();

    let extra = generation.join("unproved-extra");
    fs::write(&extra, b"extra").unwrap();
    assert_eq!(run(None).status.code(), Some(2));
    fs::remove_file(extra).unwrap();

    let files_proof = generation.join("generation-files.proof");
    let original_files_proof = fs::read(&files_proof).unwrap();
    fs::write(&files_proof, b"tampered-proof").unwrap();
    assert_eq!(run(None).status.code(), Some(2));
    fs::write(files_proof, original_files_proof).unwrap();

    let committed_retry = run(None);
    assert!(
        committed_retry.status.success(),
        "exact committed-name retry must validate without mutation: {}",
        String::from_utf8_lossy(&committed_retry.stderr)
    );
}


#[test]
fn os_switch_activates_and_sets_current() {
    // U15: `switch` builds the generation, then activates it — flips a `current`
    // pointer (and a boot `default`). The internal mechanic is a symlink in the
    // managed system store; the user sees a clear "activated" line.
    let root = Scratch::new("os-switch-root");
    let out = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "known-good",
            "--no-color",
            "--yes",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("activated"), "stderr: {stderr}");
    assert!(stderr.contains("known-good"), "stderr: {stderr}");
    // The `current` pointer now exists.
    let current = root.join("systems").join("current");
    assert!(
        current.exists(),
        "expected a `current` generation pointer at {}",
        current.display()
    );
    let generation = root.join("systems/generations/known-good");
    assert!(
        generation
            .join("etc/systemd/system/openssh.service")
            .is_file(),
        "expected generated systemd unit"
    );
    assert!(
        generation.join("sw/bin/hello").exists(),
        "expected hello in the system package closure"
    );
    assert!(
        generation.join("sw/bin/btop").exists(),
        "expected btop in the system package closure"
    );
    assert!(
        generation.join("sw/bin/systemd").exists(),
        "expected systemd in the system package closure"
    );
    assert!(
        generation.join("sw/bin/gdm").exists()
            && generation.join("sw/bin/gnome-session").exists()
            && generation.join("sw/bin/gnome-shell").exists(),
        "expected GNOME desktop commands in the system package closure"
    );
    assert_eq!(
        fs::read_to_string(generation.join("etc/hostname")).unwrap(),
        "halcyon\n"
    );
    assert_eq!(
        fs::read_to_string(generation.join("etc/timezone")).unwrap(),
        "Europe/London\n"
    );
    let fstab = fs::read_to_string(generation.join("etc/fstab")).unwrap();
    assert!(fstab.contains("jetos-root"), "fstab: {fstab}");
    assert!(
        fstab.contains("/dev/disk/by-label/swap\tnone\tswap\tpri=5"),
        "fstab: {fstab}"
    );
    let diff = fs::read_to_string(generation.join("activation-diff.txt")).unwrap();
    assert!(diff.contains("packages: 7"), "diff: {diff}");
    assert!(diff.contains("services: 3"), "diff: {diff}");
    let health = fs::read_to_string(generation.join("health-checks.txt")).unwrap();
    assert!(health.contains("openssh"), "health: {health}");
    assert!(health.contains("backup"), "health: {health}");
    assert!(health.contains("metrics"), "health: {health}");
    let provenance = fs::read_to_string(generation.join("provenance.json")).unwrap();
    assert!(provenance.contains("\"hello\""), "provenance: {provenance}");
    assert!(
        provenance.contains("\"cachyos-kernel\""),
        "provenance: {provenance}"
    );
    assert!(
        provenance.contains("core-source"),
        "provenance: {provenance}"
    );
    assert!(
        provenance.contains("packages.overlay.nixpkgs"),
        "provenance should expose compatibility escape hatches: {provenance}"
    );
    assert!(
        provenance.contains("\"bootstrap\":\"source-built\""),
        "provenance should record source-built CachyOS bootstrap: {provenance}"
    );
    let passwd = fs::read_to_string(generation.join("etc/passwd")).unwrap();
    assert!(passwd.contains("nate:x:1000"), "passwd: {passwd}");
    assert!(
        passwd.contains("/run/current-system/sw/bin/hello"),
        "passwd: {passwd}"
    );
    let group = fs::read_to_string(generation.join("etc/group")).unwrap();
    assert!(group.contains("wheel:x:2000:nate"), "group: {group}");
    let sysusers = fs::read_to_string(generation.join("etc/sysusers.d/jetos.conf")).unwrap();
    assert!(sysusers.contains("u nate 1000"), "sysusers: {sysusers}");
    assert!(sysusers.contains("g wheel 2000"), "sysusers: {sysusers}");
    let shells = fs::read_to_string(generation.join("etc/shells")).unwrap();
    assert!(
        shells.contains("/run/current-system/sw/bin/hello"),
        "shells: {shells}"
    );
    let profile = fs::read_to_string(generation.join("etc/profile")).unwrap();
    assert!(
        profile.contains("/run/current-system/sw/bin")
            && profile.contains("export JETOS_BRAND=JetOS")
            && profile.contains("export JETOS_PROMPT='JetOS halcyon'")
            && profile.contains("\\033[1;36m\\]JetOS"),
        "profile: {profile}"
    );
    let issue = fs::read_to_string(generation.join("etc/issue")).unwrap();
    assert!(
        issue.contains("JetOS halcyon") && issue.contains("proof-backed system shell"),
        "issue: {issue}"
    );
    let motd = fs::read_to_string(generation.join("etc/motd")).unwrap();
    assert!(
        motd.contains("JetOS halcyon") && motd.contains("source-owned, proof-backed"),
        "motd: {motd}"
    );
    let terminal = fs::read_to_string(generation.join("terminal/facts.json")).unwrap();
    assert!(
        terminal.contains("\"login_user\":\"nate\"")
            && terminal.contains("\"serial_tty\":\"ttyS0\"")
            && terminal.contains("\"prompt\":\"JetOS halcyon $ \"")
            && terminal.contains("terminal-login-ready"),
        "terminal: {terminal}"
    );
    assert!(
        generation
            .join("etc/systemd/system/serial-getty@ttyS0.service")
            .is_file(),
        "expected serial getty unit"
    );
    assert!(
        generation
            .join("etc/systemd/system/getty.target.wants/serial-getty@ttyS0.service")
            .exists(),
        "expected serial getty enabled"
    );
    let boot = fs::read_to_string(generation.join("boot/facts.json")).unwrap();
    assert!(boot.contains("\"loader\":\"Limine\""), "boot: {boot}");
    assert!(boot.contains("\"kernel\":\"CachyOS\""), "boot: {boot}");
    assert!(
        boot.contains("\"kernel_package\""),
        "boot facts should name the realized kernel package: {boot}"
    );
    assert!(
        boot.contains("\"output_hash\""),
        "boot facts should carry kernel provenance hash: {boot}"
    );
    assert!(
        boot.contains("\"source_recipe\""),
        "boot facts should carry source recipe provenance: {boot}"
    );
    assert!(
        boot.contains("\"sha256\""),
        "boot facts should hash kernel recipe inputs: {boot}"
    );
    let kernel = fs::read_to_string(generation.join("boot/kernel")).unwrap();
    assert!(
        kernel.contains("MZ fixture-built cachyos kernel") && kernel.contains("HdrS"),
        "kernel artifact: {kernel}"
    );
    assert!(
        generation.join("boot/limine.conf").is_file(),
        "expected Limine config"
    );
    let network = fs::read_to_string(generation.join("network/facts.json")).unwrap();
    assert!(
        network.contains("\"interface\":\"enp0s1\""),
        "network: {network}"
    );
    assert!(
        network.contains("\"firewall_allowed_tcp_ports\":[\"22\",\"443\"]"),
        "network: {network}"
    );
    let networkd =
        fs::read_to_string(generation.join("etc/systemd/network/10-jetos.network")).unwrap();
    assert!(
        networkd.contains("Address=192.0.2.10/24"),
        "networkd: {networkd}"
    );
    let nft = fs::read_to_string(generation.join("etc/nftables/jetos-firewall.nft")).unwrap();
    assert!(nft.contains("tcp dport { 22, 443 } accept"), "nft: {nft}");
    let init = fs::read_to_string(generation.join("init/systemd.json")).unwrap();
    assert!(init.contains("graphical.target"), "init: {init}");
    assert!(init.contains("\"systemd\""), "init: {init}");
    assert!(
        generation.join("sbin/init").exists(),
        "expected bootable /sbin/init projection"
    );
    assert!(
        generation
            .join("usr/lib/systemd/system/graphical.target")
            .exists()
            && generation
                .join("systemd/lib/systemd/system/graphical.target")
                .exists()
            && generation
                .join("etc/systemd/system/graphical.target")
                .exists()
            && generation
                .join("usr/lib/systemd/system/rescue.target")
                .exists()
            && generation.join("etc/systemd/system/default.target").exists(),
        "expected base systemd target units in bootable generation"
    );
    assert!(
        generation.join("root/etc/hostname").is_file(),
        "expected root-shaped /etc projection"
    );
    assert!(
        generation.join("root/boot/kernel").is_file(),
        "expected root-shaped /boot projection"
    );
    assert!(
        generation.join("root/sbin/init").exists(),
        "expected root-shaped /sbin/init projection"
    );
    assert!(
        generation
            .join("root/run/current-system/etc/systemd/system/openssh.service")
            .is_file(),
        "expected current-system projection inside root"
    );
    assert!(
        generation.join("root/home/nate/.profile").is_file(),
        "expected user home profile in installed root"
    );
    let user_profile = fs::read_to_string(generation.join("users/nate/profile.json")).unwrap();
    assert!(
        user_profile.contains("\"kind\":\"jetos.user-generation\"")
            && user_profile.contains("\"user\":\"nate\"")
            && user_profile.contains("\"syncthing\""),
        "user profile: {user_profile}"
    );
    assert!(
        generation
            .join("etc/systemd/user/jetos-user-nate.service")
            .is_file(),
        "expected user environment unit"
    );
    assert!(
        generation.join("sw/bin/jetos-user-apply").is_file(),
        "expected standalone user apply helper"
    );
    let user_home = root.path.join("applied-home");
    let user_apply = Command::new(generation.join("sw/bin/jetos-user-apply"))
        .arg("nate")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_USER_HOME", &user_home)
        .output()
        .unwrap();
    assert!(
        user_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&user_apply.stdout),
        String::from_utf8_lossy(&user_apply.stderr)
    );
    let ghostty = fs::read_to_string(user_home.join(".config/ghostty/config")).unwrap();
    assert!(
        ghostty.contains("managed-by=jetos") && ghostty.contains("home/ghostty/config"),
        "ghostty: {ghostty}"
    );
    assert!(
        user_home.join(".jetos/profile/bin/hello").exists(),
        "expected per-user package profile link"
    );
    assert!(
        user_home
            .join(".config/systemd/user/syncthing.service")
            .is_file(),
        "expected per-user service unit"
    );
    let user_apply_proof =
        fs::read_to_string(user_home.join(".jetos/proof/user-nate.json")).unwrap();
    assert!(
        user_apply_proof.contains("\"state\":\"applied\""),
        "user_apply_proof: {user_apply_proof}"
    );
    assert!(
        generation
            .join("root/run/current-system/terminal/facts.json")
            .is_file(),
        "expected terminal proof facts in current-system projection"
    );
    assert!(
        generation.join("etc/systemd/system/backup.timer").is_file(),
        "expected backup timer"
    );
    assert!(
        generation
            .join("etc/systemd/system/timers.target.wants/backup.timer")
            .exists(),
        "expected enabled backup timer"
    );
    assert!(
        generation
            .join("etc/systemd/system/metrics.socket")
            .is_file(),
        "expected metrics socket"
    );
    assert!(
        generation
            .join("etc/systemd/system/sockets.target.wants/metrics.socket")
            .exists(),
        "expected enabled metrics socket"
    );
    assert!(
        generation
            .join("etc/systemd/system/display-manager.service")
            .is_file(),
        "expected display manager unit"
    );
    assert!(
        generation
            .join("etc/systemd/system/graphical.target.wants/display-manager.service")
            .exists(),
        "expected enabled display manager"
    );
    assert!(
        generation
            .join("etc/systemd/system/multi-user.target.wants/openssh.service")
            .exists(),
        "expected enabled openssh service"
    );
    let hardware = fs::read_to_string(generation.join("hardware/facts.json")).unwrap();
    assert!(hardware.contains("iwlwifi"), "hardware: {hardware}");
    assert!(hardware.contains("amdgpu"), "hardware: {hardware}");
    assert!(
        hardware.contains("framework-13-amd"),
        "hardware: {hardware}"
    );
    assert!(
        hardware.contains("jetos.hardware") && hardware.contains("jetos-hardware-doctor"),
        "hardware: {hardware}"
    );
    assert!(
        generation.join("hardware/halcyon.jet").is_file()
            && generation
                .join("hardware/profile-framework-13-amd.json")
                .is_file()
            && generation
                .join("boot/specialisations/plasmaBeta.conf")
                .is_file()
            && generation.join("sw/bin/jetos-hardware-scan").is_file()
            && generation.join("sw/bin/jetos-hardware-doctor").is_file(),
        "expected hardware scan/profile/specialisation artifacts"
    );
    let specialisation = fs::read_to_string(
        generation.join("boot/specialisations/plasmaBeta.conf"),
    )
    .unwrap();
    assert!(
        specialisation.contains("title jetos 26.10 (Apex) — halcyon (plasmaBeta)"),
        "specialisation title: {specialisation}"
    );
    let hardware_root = root.path.join("fake-hardware");
    fs::create_dir_all(hardware_root.join("proc")).unwrap();
    fs::create_dir_all(hardware_root.join("sys/class/block/nvme0n1")).unwrap();
    fs::create_dir_all(hardware_root.join("sys/class/drm/card0")).unwrap();
    fs::write(
        hardware_root.join("proc/modules"),
        "amdgpu 1 0 - Live 0\nnvme 1 0 - Live 0\n",
    )
    .unwrap();
    let scan_out = root.path.join("halcyon-scan.jet");
    let scan = Command::new(generation.join("sw/bin/jetos-hardware-scan"))
        .arg("halcyon")
        .env("JETOS_HW_ROOT", &hardware_root)
        .env("JETOS_HARDWARE_OUT", &scan_out)
        .output()
        .unwrap();
    assert!(
        scan.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );
    let scanned_source = fs::read_to_string(&scan_out).unwrap();
    assert!(
        scanned_source.contains("hardware.halcyon.scan.modules")
            && scanned_source.contains("amdgpu,nvme")
            && scanned_source.contains("nvme0n1"),
        "scanned_source: {scanned_source}"
    );
    let doctor = Command::new(generation.join("sw/bin/jetos-hardware-doctor"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_HW_ROOT", &hardware_root)
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        doctor.contains("\"state\":\"match\"") && doctor.contains("hardware-drift-checked"),
        "doctor: {doctor}"
    );
    let performance = fs::read_to_string(generation.join("performance/facts.json")).unwrap();
    assert!(
        performance.contains("\"profile\":\"Gaming\"")
            && performance.contains("\"kernel_profile\":\"CachyOSLatest\"")
            && performance.contains("vm.swappiness")
            && performance.contains("performance/initrd.json")
            && performance.contains("performance/bootloader.json"),
        "performance: {performance}"
    );
    let perf_profile = fs::read_to_string(generation.join("performance/profile.json")).unwrap();
    assert!(
        perf_profile.contains("kernel-tuning-profile-ready")
            && perf_profile.contains("CachyOSLatest"),
        "perf_profile: {perf_profile}"
    );
    let scheduler = fs::read_to_string(generation.join("performance/scheduler.json")).unwrap();
    assert!(
        scheduler.contains("ScxLavd") && scheduler.contains("sched-ext-service-ready"),
        "scheduler: {scheduler}"
    );
    assert!(
        generation
            .join("etc/systemd/system/jetos-performance-scheduler.service")
            .is_file()
            && generation
                .join("etc/systemd/system/multi-user.target.wants/jetos-performance-scheduler.service")
                .exists()
            && generation.join("sw/bin/jetos-performance-scheduler").is_file(),
        "expected scheduler service"
    );
    let scheduler_bin = root.path.join("fake-scheduler");
    let scheduler_log = root.path.join("scheduler.log");
    write_executable(
        &scheduler_bin,
        "#!/bin/sh\nprintf '%s\\n' scheduler >> \"$JETOS_SCHEDULER_LOG\"\n",
    );
    let scheduler_run = Command::new(generation.join("sw/bin/jetos-performance-scheduler"))
        .env("JETOS_SCHEDULER_BIN", &scheduler_bin)
        .env("JETOS_SCHEDULER_LOG", &scheduler_log)
        .output()
        .unwrap();
    assert!(
        scheduler_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&scheduler_run.stdout),
        String::from_utf8_lossy(&scheduler_run.stderr)
    );
    assert!(
        fs::read_to_string(&scheduler_log)
            .unwrap()
            .contains("scheduler"),
        "expected scheduler log"
    );
    let initrd = fs::read_to_string(generation.join("performance/initrd.json")).unwrap();
    assert!(
        initrd.contains("\"systemd\":true") && initrd.contains("\"verbosity\":\"quiet\""),
        "initrd: {initrd}"
    );
    let bootloader = fs::read_to_string(generation.join("performance/bootloader.json")).unwrap();
    assert!(
        bootloader.contains("\"limine_max_generations\":\"7\"")
            && bootloader.contains("\"efi_can_touch_variables\":false"),
        "bootloader: {bootloader}"
    );
    assert!(
        generation
            .join("etc/sysctl.d/90-jetos-performance.conf")
            .is_file(),
        "expected sysctl projection"
    );
    assert!(
        generation
            .join("etc/systemd/zram-generator.conf.d/jetos.conf")
            .is_file(),
        "expected zram projection"
    );
    let storage = fs::read_to_string(generation.join("storage/facts.json")).unwrap();
    assert!(
        storage.contains("jetos.storage-tree")
            && storage.contains("disk.main.device")
            && storage.contains("jetos-storage-apply")
            && storage.contains("\"ephemeral_root\":true"),
        "storage: {storage}"
    );
    let storage_plan = fs::read_to_string(generation.join("storage/plan.json")).unwrap();
    assert!(
        storage_plan.contains("\"root_fs\":\"Btrfs\"")
            && storage_plan.contains("/var/lib")
            && storage_plan.contains("requires --manual plus --execute"),
        "storage_plan: {storage_plan}"
    );
    assert!(
        generation.join("storage/mounts.fstab").is_file()
            && generation.join("sw/bin/jetos-storage-plan").is_file()
            && generation.join("sw/bin/jetos-storage-apply").is_file()
            && generation.join("sw/bin/jetos-persist-activate").is_file(),
        "expected storage scripts and mounts"
    );
    let storage_apply_log = root.path.join("storage-apply.sh");
    let storage_proofs = root.path.join("storage-proofs");
    let storage_apply = Command::new(generation.join("sw/bin/jetos-storage-apply"))
        .arg("--manual")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_STORAGE_LOG", &storage_apply_log)
        .env("JETOS_STORAGE_PROOF_DIR", &storage_proofs)
        .output()
        .unwrap();
    assert!(
        storage_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&storage_apply.stdout),
        String::from_utf8_lossy(&storage_apply.stderr)
    );
    let storage_apply_log = fs::read_to_string(&storage_apply_log).unwrap();
    assert!(
        storage_apply_log.contains("sfdisk --wipe always /dev/sda")
            && storage_apply_log.contains("mkfs.btrfs -L jetos-root"),
        "storage_apply_log: {storage_apply_log}"
    );
    let storage_apply_proof = fs::read_to_string(storage_proofs.join("apply-proof.json")).unwrap();
    assert!(
        storage_apply_proof.contains("\"executed\":false")
            && storage_apply_proof.contains("manual-storage-plan-reviewed"),
        "storage_apply_proof: {storage_apply_proof}"
    );
    let persist_root = root.path.join("persist-root");
    let ephemeral_root = root.path.join("ephemeral-root");
    let persist = Command::new(generation.join("sw/bin/jetos-persist-activate"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_PERSIST_ROOT", &persist_root)
        .env("JETOS_EPHEMERAL_ROOT", &ephemeral_root)
        .env("JETOS_STORAGE_PROOF_DIR", &storage_proofs)
        .output()
        .unwrap();
    assert!(
        persist.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&persist.stdout),
        String::from_utf8_lossy(&persist.stderr)
    );
    assert!(
        persist_root.join("home/nate").is_dir()
            && persist_root.join("var/lib").is_dir()
            && ephemeral_root.join("home/nate").is_dir(),
        "expected persisted paths"
    );
    let persist_proof = fs::read_to_string(storage_proofs.join("persistence-proof.json")).unwrap();
    assert!(
        persist_proof.contains("\"state\":\"activated\"")
            && persist_proof.contains("impermanence-persist-ready"),
        "persist_proof: {persist_proof}"
    );
    let module_explain = fs::read_to_string(generation.join("module-system/explain.json")).unwrap();
    assert!(
        module_explain.contains("\"key\":\"services.displayManager\"")
            && module_explain.contains("\"value\":\"gdm\"")
            && module_explain.contains("\"value\":\"sddm\"")
            && module_explain.contains("\"winner\":true")
            && module_explain.contains("Force")
            && module_explain.contains("stylix.kmscon"),
        "module explain: {module_explain}"
    );
    let disabled_modules =
        fs::read_to_string(generation.join("module-system/disabled-modules.manifest")).unwrap();
    assert!(
        disabled_modules.contains("stylix.kmscon"),
        "disabled_modules: {disabled_modules}"
    );
    let theme = fs::read_to_string(generation.join("theme/facts.json")).unwrap();
    assert!(
        theme.contains("\"name\":\"halcyon\"") && theme.contains("theme-projected"),
        "theme: {theme}"
    );
    assert!(
        generation
            .join("share/themes/jetos/gtk-4.0/gtk.css")
            .is_file(),
        "expected theme projection"
    );
    for themed in [
        "share/qt6ct/colors/jetos.conf",
        "share/terminal/theme.toml",
        "share/editor/theme.json",
        "share/display-manager/theme.conf",
        "studio/theme-preview.json",
    ] {
        assert!(
            generation.join(themed).is_file(),
            "expected theme projection {themed}"
        );
    }
    let studio_theme = fs::read_to_string(generation.join("studio/theme-preview.json")).unwrap();
    assert!(
        studio_theme.contains("jetos.theme-preview") && studio_theme.contains("#7aa2f7"),
        "studio_theme: {studio_theme}"
    );
    let flatpak = fs::read_to_string(generation.join("flatpak/plan.json")).unwrap();
    assert!(
        flatpak.contains("com.discordapp.Discord")
            && flatpak.contains("flatpak-reconcile-planned")
            && flatpak.contains("obsidian"),
        "flatpak: {flatpak}"
    );
    let appimage = fs::read_to_string(generation.join("appimage/plan.json")).unwrap();
    assert!(
        appimage.contains("appimage-runtime-integrated")
            && appimage.contains("/opt/apps/Obsidian.AppImage"),
        "appimage: {appimage}"
    );
    assert!(
        generation.join("sw/bin/jetos-flatpak-reconcile").is_file()
            && generation.join("sw/bin/jetos-appimage-run").is_file()
            && generation.join("appimage/obsidian.desktop").is_file(),
        "expected foreign app helpers"
    );
    let flatpak_bin = root.path.join("fake-flatpak");
    let flatpak_log = root.path.join("flatpak.log");
    write_executable(
        &flatpak_bin,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$JETOS_FLATPAK_LOG\"\nif [ \"$1\" = list ]; then\n  printf '%s\\n' com.discordapp.Discord com.spotify.Client\nfi\n",
    );
    let flatpak_run = Command::new(generation.join("sw/bin/jetos-flatpak-reconcile"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_FLATPAK_BIN", &flatpak_bin)
        .env("JETOS_FLATPAK_LOG", &flatpak_log)
        .output()
        .unwrap();
    assert!(
        flatpak_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&flatpak_run.stdout),
        String::from_utf8_lossy(&flatpak_run.stderr)
    );
    let flatpak_steps = fs::read_to_string(&flatpak_log).unwrap();
    assert!(
        flatpak_steps.contains("remote-add --if-not-exists flathub")
            && flatpak_steps.contains("install -y flathub com.discordapp.Discord")
            && flatpak_steps.contains("override com.discordapp.Discord --filesystem=Downloads")
            && flatpak_steps.contains("uninstall -y com.spotify.Client")
            && flatpak_steps.contains("update -y"),
        "flatpak_steps: {flatpak_steps}"
    );
    let appimage_print = Command::new(generation.join("sw/bin/jetos-appimage-run"))
        .args(["obsidian", "--print"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        appimage_print.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&appimage_print.stdout),
        String::from_utf8_lossy(&appimage_print.stderr)
    );
    assert!(
        String::from_utf8_lossy(&appimage_print.stdout).contains("/opt/apps/Obsidian.AppImage"),
        "appimage_print: {}",
        String::from_utf8_lossy(&appimage_print.stdout)
    );
    let workloads = fs::read_to_string(generation.join("workloads/facts.json")).unwrap();
    assert!(
        workloads.contains("\"name\":\"web\"")
            && workloads.contains("\"backend\":\"Container\"")
            && workloads.contains("\"name\":\"sandbox\"")
            && workloads.contains("\"backend\":\"MicroVM\"")
            && workloads.contains("web-token"),
        "workloads: {workloads}"
    );
    let web_plan = fs::read_to_string(generation.join("workloads/web.plan.json")).unwrap();
    assert!(
        web_plan.contains("/srv/web:/srv/web:ro")
            && web_plan.contains("\"memory\":\"512M\"")
            && web_plan.contains("\"rollback_keep\":\"3\"")
            && web_plan.contains("workload-proof-ready"),
        "web_plan: {web_plan}"
    );
    let sandbox_plan = fs::read_to_string(generation.join("workloads/sandbox.plan.json")).unwrap();
    assert!(
        sandbox_plan.contains("qemu-system-x86_64")
            && sandbox_plan.contains("-m 2048M")
            && sandbox_plan.contains("\"backend\":\"MicroVM\""),
        "sandbox_plan: {sandbox_plan}"
    );
    assert!(
        generation
            .join("etc/systemd/system/workload-web.service")
            .is_file(),
        "expected workload systemd unit"
    );
    assert!(
        generation.join("workloads/web.rollback.manifest").is_file()
            && generation.join("workloads/health-web.sh").is_file()
            && generation
                .join("workloads/sandbox.rollback.manifest")
                .is_file(),
        "expected workload health/rollback artifacts"
    );
    let workload_log = root.path.join("workload.log");
    let workload_run = Command::new(generation.join("sw/bin/jetos-workload-run"))
        .arg("web")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_WORKLOAD_LOG", &workload_log)
        .output()
        .unwrap();
    assert!(
        workload_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&workload_run.stdout),
        String::from_utf8_lossy(&workload_run.stderr)
    );
    let workload_steps = fs::read_to_string(&workload_log).unwrap();
    assert!(
        workload_steps.contains("workload"),
        "workload: {workload_steps}"
    );
    let fleet = fs::read_to_string(generation.join("fleet/deploy-plan.json")).unwrap();
    assert!(
        fleet.contains("staged-proof-gated-rollback-stop") && fleet.contains("\"fleet\": \"home\""),
        "fleet: {fleet}"
    );
    assert!(
        generation.join("sw/bin/jetos-fleet-deploy").is_file(),
        "expected fleet deploy launcher"
    );
    let deploy_log = root.path.join("deploy.log");
    let deploy_proofs = root.path.join("deploy-proofs");
    fs::create_dir_all(&deploy_proofs).unwrap();
    let deploy = Command::new(generation.join("sw/bin/jetos-fleet-deploy"))
        .arg("halcyon")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_DEPLOY_PROOF_DIR", &deploy_proofs)
        .env("JETOS_DEPLOY_LOG", &deploy_log)
        .output()
        .unwrap();
    assert!(
        deploy.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&deploy.stdout),
        String::from_utf8_lossy(&deploy.stderr)
    );
    let deploy_steps = fs::read_to_string(&deploy_log).unwrap();
    assert!(
        deploy_steps.contains("push")
            && deploy_steps.contains("proof")
            && deploy_steps.contains("switch")
            && deploy_steps.contains("health"),
        "deploy_steps: {deploy_steps}"
    );
    let deploy_proof = fs::read_to_string(deploy_proofs.join("home-halcyon.json")).unwrap();
    assert!(
        deploy_proof.contains("\"state\":\"deployed\"")
            && deploy_proof.contains("remote-proof-before-switch"),
        "deploy_proof: {deploy_proof}"
    );
    let options_ref = fs::read_to_string(generation.join("options/reference.json")).unwrap();
    assert!(
        options_ref.contains("apps.flatpak.app.discord.ref")
            && options_ref.contains("performance.sysctl.vm.swappiness")
            && options_ref.contains("\"type\":")
            && options_ref.contains("\"doc\":")
            && options_ref.contains("\"tier\":"),
        "options reference: {options_ref}"
    );
    assert!(
        generation.join("sw/bin/jetos-options-search").is_file(),
        "expected options search helper"
    );
    let option_exact = Command::new(generation.join("sw/bin/jetos-options-search"))
        .args(["--exact", "services.displayManager"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        option_exact.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&option_exact.stdout),
        String::from_utf8_lossy(&option_exact.stderr)
    );
    let option_exact = String::from_utf8_lossy(&option_exact.stdout);
    assert!(
        option_exact.contains("services.displayManager") && option_exact.contains("gdm"),
        "option_exact: {option_exact}"
    );
    let option_explain = Command::new(generation.join("sw/bin/jetos-options-search"))
        .args(["--explain", "services.displayManager"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        option_explain.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&option_explain.stdout),
        String::from_utf8_lossy(&option_explain.stderr)
    );
    let option_explain = String::from_utf8_lossy(&option_explain.stdout);
    assert!(
        option_explain.contains("services.displayManager") && option_explain.contains("winner"),
        "option_explain: {option_explain}"
    );
    let images = fs::read_to_string(generation.join("image-variants/matrix.json")).unwrap();
    assert!(
        images.contains("\"name\": \"installer\"") && images.contains("image-variant-plan-ready"),
        "image variants: {images}"
    );
    let lifecycle = fs::read_to_string(generation.join("lifecycle/policy.json")).unwrap();
    assert!(
        lifecycle.contains("gc")
            && lifecycle.contains("rollback_window")
            && lifecycle.contains("\"auto_upgrade\":true"),
        "lifecycle: {lifecycle}"
    );
    let auto_upgrade = fs::read_to_string(generation.join("lifecycle/auto-upgrade.json")).unwrap();
    assert!(
        auto_upgrade.contains("auto-upgrade-proof-gated")
            && auto_upgrade.contains("rollback-on-fail"),
        "auto_upgrade: {auto_upgrade}"
    );
    let channel = fs::read_to_string(generation.join("lifecycle/channel.json")).unwrap();
    assert!(
        channel.contains("\"channel\":\"stable\"") && channel.contains("channel-policy-ready"),
        "channel: {channel}"
    );
    assert!(
        generation.join("sw/bin/jetos-lifecycle-gc").is_file()
            && generation.join("sw/bin/jetos-channel-update").is_file()
            && generation.join("sw/bin/jetos-auto-upgrade").is_file()
            && generation
                .join("etc/systemd/system/jetos-auto-upgrade.timer")
                .is_file(),
        "expected lifecycle launchers"
    );
    let gc_systems = root.path.join("gc-systems");
    let old = gc_systems.join("generations/old");
    let mid = gc_systems.join("generations/mid");
    let new = gc_systems.join("generations/new");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&mid).unwrap();
    fs::create_dir_all(&new).unwrap();
    fs::write(
        gc_systems.join("generations.log"),
        format!(
            "1\thalcyon\told\t{}\n2\thalcyon\tmid\t{}\n3\thalcyon\tnew\t{}\n",
            old.display(),
            mid.display(),
            new.display()
        ),
    )
    .unwrap();
    let gc = Command::new(generation.join("sw/bin/jetos-lifecycle-gc"))
        .arg("--apply")
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_SYSTEMS_DIR", &gc_systems)
        .output()
        .unwrap();
    assert!(
        gc.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&gc.stdout),
        String::from_utf8_lossy(&gc.stderr)
    );
    assert!(!old.exists(), "old generation should be deleted by GC");
    assert!(
        mid.exists() && new.exists(),
        "newer generations should be kept"
    );
    let gc_plan = fs::read_to_string(generation.join("lifecycle/gc-plan.txt")).unwrap();
    assert!(
        gc_plan.contains("reason=older-than-retention")
            && gc_plan.contains("reason=within-retention"),
        "gc_plan: {gc_plan}"
    );
    let lifecycle_log = root.path.join("lifecycle.log");
    let lifecycle_log_q = test_shell_quote(&lifecycle_log);
    let lifecycle_proofs = root.path.join("lifecycle-proofs");
    let upgrade = Command::new(generation.join("sw/bin/jetos-auto-upgrade"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_LIFECYCLE_PROOF_DIR", &lifecycle_proofs)
        .env(
            "JETOS_UPGRADE_FETCH_CMD",
            format!("echo fetch >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_BUILD_CMD",
            format!("echo build >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_PROOF_CMD",
            format!("echo proof >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_SWITCH_CMD",
            format!("echo switch >> {lifecycle_log_q}"),
        )
        .env(
            "JETOS_UPGRADE_HEALTH_CMD",
            format!("echo health >> {lifecycle_log_q}"),
        )
        .output()
        .unwrap();
    assert!(
        upgrade.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&upgrade.stdout),
        String::from_utf8_lossy(&upgrade.stderr)
    );
    let lifecycle_steps = fs::read_to_string(&lifecycle_log).unwrap();
    assert!(
        lifecycle_steps.contains("fetch")
            && lifecycle_steps.contains("build")
            && lifecycle_steps.contains("proof")
            && lifecycle_steps.contains("switch")
            && lifecycle_steps.contains("health"),
        "lifecycle_steps: {lifecycle_steps}"
    );
    let upgrade_proof =
        fs::read_to_string(lifecycle_proofs.join("auto-upgrade-proof.json")).unwrap();
    assert!(
        upgrade_proof.contains("\"state\":\"switched\"") && upgrade_proof.contains("health-passed"),
        "upgrade_proof: {upgrade_proof}"
    );
    let rollback_log = root.path.join("lifecycle-rollback.log");
    let rollback_log_q = test_shell_quote(&rollback_log);
    let rollback_proofs = root.path.join("lifecycle-rollback-proofs");
    let rollback = Command::new(generation.join("sw/bin/jetos-auto-upgrade"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_LIFECYCLE_PROOF_DIR", &rollback_proofs)
        .env("JETOS_UPGRADE_FETCH_CMD", "true")
        .env("JETOS_UPGRADE_BUILD_CMD", "true")
        .env("JETOS_UPGRADE_PROOF_CMD", "true")
        .env("JETOS_UPGRADE_SWITCH_CMD", "true")
        .env("JETOS_UPGRADE_HEALTH_CMD", "false")
        .env(
            "JETOS_UPGRADE_ROLLBACK_CMD",
            format!("echo rollback >> {rollback_log_q}"),
        )
        .output()
        .unwrap();
    assert!(
        !rollback.status.success(),
        "rollback path should fail after health failure"
    );
    assert!(
        fs::read_to_string(&rollback_log)
            .unwrap()
            .contains("rollback"),
        "expected rollback log"
    );
    let rollback_proof =
        fs::read_to_string(rollback_proofs.join("auto-upgrade-proof.json")).unwrap();
    assert!(
        rollback_proof.contains("\"state\":\"rolled-back\"")
            && rollback_proof.contains("health-failed"),
        "rollback_proof: {rollback_proof}"
    );
    let services = fs::read_to_string(generation.join("service-manager/facts.json")).unwrap();
    assert!(
        services.contains("tmpfiles")
            && services.contains("hardening")
            && services.contains("journal"),
        "service depth: {services}"
    );
    assert!(
        generation.join("etc/tmpfiles.d/backup.conf").is_file(),
        "expected tmpfiles projection"
    );
    assert!(
        generation.join("sw/bin/jetos-service-logs").is_file(),
        "expected service log helper"
    );
    let journal_bin = root.path.join("fake-journalctl");
    let journal_log = root.path.join("journalctl.log");
    write_executable(
        &journal_bin,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$JETOS_JOURNAL_LOG\"\n",
    );
    let service_logs = Command::new(generation.join("sw/bin/jetos-service-logs"))
        .args(["openssh", "--since", "1 hour ago"])
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_JOURNALCTL_BIN", &journal_bin)
        .env("JETOS_JOURNAL_LOG", &journal_log)
        .output()
        .unwrap();
    assert!(
        service_logs.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&service_logs.stdout),
        String::from_utf8_lossy(&service_logs.stderr)
    );
    let journal_args = fs::read_to_string(&journal_log).unwrap();
    assert!(
        journal_args.contains("-u openssh --since 1 hour ago"),
        "journal_args: {journal_args}"
    );
    let app_modules = fs::read_to_string(generation.join("apps/modules.json")).unwrap();
    assert!(
        app_modules.contains("app-module-library")
            && app_modules.contains("ghosttyConfig")
            && app_modules.contains("\"name\":\"git\"")
            && app_modules.contains("\"name\":\"vscode\"")
            && app_modules.contains("jetos-app-module-apply"),
        "app modules: {app_modules}"
    );
    assert!(
        generation.join("apps/programs/git/module.json").is_file()
            && generation.join("apps/programs/vscode/config").is_file()
            && generation
                .join("apps/programs/discord/module.json")
                .is_file()
            && generation.join("apps/coverage.manifest").is_file()
            && generation.join("apps/gap-cards.manifest").is_file()
            && generation.join("sw/bin/jetos-app-module-apply").is_file(),
        "expected app module library artifacts"
    );
    let git_config = fs::read_to_string(generation.join("apps/programs/git/config")).unwrap();
    assert!(
        git_config.contains("user.name = Nate") && git_config.contains("user.email"),
        "git_config: {git_config}"
    );
    let app_home = root.path.join("app-home");
    let app_apply = Command::new(generation.join("sw/bin/jetos-app-module-apply"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_USER_HOME", &app_home)
        .output()
        .unwrap();
    assert!(
        app_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&app_apply.stdout),
        String::from_utf8_lossy(&app_apply.stderr)
    );
    assert!(
        app_home.join(".config/git/config").is_file()
            && app_home.join(".config/Code/User/settings.json").is_file()
            && app_home.join(".jetos/proof/app-modules.json").is_file(),
        "expected app module apply output"
    );
    let acceptance =
        fs::read_to_string(generation.join("acceptance/jetos-host-coverage.json")).unwrap();
    assert!(
        acceptance.contains("jetos.host-coverage")
            && acceptance.contains("\"state\": \"covered\"")
            && acceptance.contains("jetos-host-covered")
            && acceptance.contains("\"omissions\":[]"),
        "acceptance: {acceptance}"
    );
    let coverage = fs::read_to_string(generation.join("acceptance/coverage-matrix.tsv")).unwrap();
    assert!(
        coverage.contains("desktop-audio-locale-fonts-virt-gaming-smartcard\tcovered")
            && coverage.contains("flatpak-appimage\tcovered")
            && coverage.contains("lifecycle-gc-auto-upgrade\tcovered")
            && !coverage.contains("\tmissing\t"),
        "coverage: {coverage}"
    );
    let vm_gates = fs::read_to_string(generation.join("acceptance/vm-gates.json")).unwrap();
    assert!(
        vm_gates.contains("desktop-session-ready")
            && vm_gates.contains("app-modules-present")
            && vm_gates.contains("vm-acceptance-required"),
        "vm_gates: {vm_gates}"
    );
    let os_release = fs::read_to_string(generation.join("etc/os-release")).unwrap();
    let expected_os_release = "NAME=jetos\nID=jetos\nVERSION=\"26.10 (Apex)\"\nVERSION_ID=26.10\nVERSION_CODENAME=apex\nPRETTY_NAME=\"jetos 26.10 (Apex)\"\nHOME_URL=\"https://jet.dev/jetos\"\n";
    assert_eq!(os_release, expected_os_release);
    assert_eq!(
        fs::read_to_string(generation.join("usr/lib/os-release")).unwrap(),
        expected_os_release
    );
    let installed_limine = fs::read_to_string(generation.join("boot/limine.conf")).unwrap();
    assert!(
        installed_limine.contains("/jetos 26.10 (Apex) — halcyon"),
        "installed Limine title: {installed_limine}"
    );
    let wallpaper = fs::read_to_string(
        generation.join("share/backgrounds/jetos/apex.svg"),
    )
    .unwrap();
    assert!(
        wallpaper.starts_with("<svg ")
            && wallpaper.contains("jetos 26.10 Apex")
            && wallpaper.contains("linearGradient")
            && wallpaper.len() > 1_000,
        "baseline wallpaper must contain the real committed SVG bytes"
    );
    for (surface, text) in [
        ("etc/os-release", os_release.as_str()),
        ("usr/lib/os-release", expected_os_release),
        ("boot/limine.conf", installed_limine.as_str()),
        ("boot specialisation", specialisation.as_str()),
        ("wallpaper", wallpaper.as_str()),
    ] {
        assert!(
            !text.contains("NixOS") && !text.contains("Yarara"),
            "upstream identity leaked through {surface}: {text}"
        );
    }
    assert!(
        generation.join("acceptance/owner-jetos-coverage.md").is_file()
            && generation.join("sw/bin/jetos-acceptance-prove").is_file(),
        "expected acceptance artifacts"
    );
    assert!(
        !generation.join("acceptance/nixos-parity.json").exists()
            && !generation.join("acceptance/owner-nixos-diff.md").exists(),
        "legacy NixOS-named JetOS artifacts must not be generated"
    );
    let acceptance_proofs = root.path.join("acceptance-proofs");
    let acceptance_run = Command::new(generation.join("sw/bin/jetos-acceptance-prove"))
        .env("JETOS_SYSTEM_ROOT", &generation)
        .env("JETOS_ACCEPTANCE_PROOF_DIR", &acceptance_proofs)
        .output()
        .unwrap();
    assert!(
        acceptance_run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&acceptance_run.stdout),
        String::from_utf8_lossy(&acceptance_run.stderr)
    );
    let acceptance_proof =
        fs::read_to_string(acceptance_proofs.join("acceptance-proof.json")).unwrap();
    assert!(
        acceptance_proof.contains("\"state\":\"passed\"")
            && acceptance_proof.contains("jetos-host-covered"),
        "acceptance_proof: {acceptance_proof}"
    );
    let desktop = fs::read_to_string(generation.join("desktop/facts.json")).unwrap();
    assert!(
        desktop.contains("\"session\":\"gnome-wayland\""),
        "desktop: {desktop}"
    );
    assert!(
        desktop.contains("\"display_manager\":\"gdm\"")
            && desktop.contains("\"terminal_fallback\":\"ttyS0+tty1\"")
            && desktop.contains("desktop-session-ready"),
        "desktop: {desktop}"
    );
    assert!(
        generation.join("sw/bin/jetos-desktop-session").is_file(),
        "expected desktop session launcher"
    );
    let desktop_breadth = fs::read_to_string(generation.join("desktop/breadth.json")).unwrap();
    assert!(
        desktop_breadth.contains("desktop-module-breadth-ready")
            && desktop_breadth.contains("\"audio\":true")
            && desktop_breadth.contains("plasma-wayland")
            && desktop_breadth.contains("libvirtd")
            && desktop_breadth.contains("gamemode")
            && desktop_breadth.contains("Inter"),
        "desktop_breadth: {desktop_breadth}"
    );
    for desktop_path in [
        "share/wayland-sessions/jetos-plasma.desktop",
        "etc/pipewire/jetos.conf",
        "etc/security/limits.d/99-jetos-rtkit.conf",
        "etc/locale.conf",
        "etc/vconsole.conf",
        "etc/fonts/local.conf",
        "share/applications/mimeapps.list",
        "etc/systemd/system/libvirtd.service",
        "etc/systemd/system/gamemoded.service",
        "etc/systemd/system/pcscd.service",
        "etc/binfmt.d/appimage.conf",
    ] {
        assert!(
            generation.join(desktop_path).is_file(),
            "expected desktop breadth artifact {desktop_path}"
        );
    }
    let desktop_session =
        fs::read_to_string(generation.join("sw/bin/jetos-desktop-session")).unwrap();
    assert!(
        desktop_session.contains("--jetos-proof")
            && desktop_session.contains("desktop session command gnome-session"),
        "desktop session launcher should expose proof mode: {desktop_session}"
    );
    let display_manager =
        fs::read_to_string(generation.join("sw/bin/jetos-display-manager")).unwrap();
    assert!(
        display_manager.contains("--jetos-proof")
            && display_manager.contains("display manager command gdm"),
        "display manager launcher should expose proof mode: {display_manager}"
    );
    assert!(
        generation.join("sw/bin/gdm").is_file()
            && generation.join("sw/bin/gnome-session").is_file()
            && generation.join("sw/bin/gnome-shell").is_file(),
        "expected default GNOME profile commands in system closure"
    );
    assert!(
        generation.join("sw/bin/jetos-terminal-fallback").is_file(),
        "expected terminal fallback launcher"
    );
    let terminal_fallback =
        fs::read_to_string(generation.join("sw/bin/jetos-terminal-fallback")).unwrap();
    assert!(
        terminal_fallback.contains("cat /etc/motd")
            && terminal_fallback.contains("ttyS0 and tty1 remain available"),
        "terminal_fallback: {terminal_fallback}"
    );
    assert!(
        generation
            .join("share/wayland-sessions/jetos-gnome.desktop")
            .is_file(),
        "expected GNOME Wayland session entry"
    );
    let cache = fs::read_to_string(generation.join("store/cache.json")).unwrap();
    assert!(cache.contains("jetpack-hangar"), "cache: {cache}");
    let compat = fs::read_to_string(generation.join("compat/escape-hatches.json")).unwrap();
    assert!(
        compat.contains("\"studio_visible\": \"true\""),
        "compat: {compat}"
    );
    assert!(
        generation.join("sw/bin/jetos-studio").is_file(),
        "expected installed jetos Studio launcher"
    );
    assert!(
        generation
            .join("share/applications/jetos-studio.desktop")
            .is_file(),
        "expected desktop app entry"
    );
    let studio = fs::read_to_string(generation.join("studio/app.json")).unwrap();
    assert!(
        studio.contains("\"runtime\": \"jetos-system-app\""),
        "studio: {studio}"
    );
    assert!(
        studio.contains("\"browser_fallback\": \"true\""),
        "studio: {studio}"
    );
    assert!(
        !studio.contains("Canvas"),
        "studio app projection must stay separate from Canvas: {studio}"
    );
    let studio_data = fs::read_to_string(generation.join("studio/data.json")).unwrap();
    assert!(
        studio_data.contains("\"kind\":\"jetos-studio-projection\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"artifacts\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"dashboard\"") && studio_data.contains("\"selected_host\":\"halcyon\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"page_registry\"") && studio_data.contains("\"id\":\"changeset\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"controller\":\"studio-actions\"")
            && studio_data.contains("\"model_contract\":")
            && studio_data.contains("\"read_only\":"),
        "Studio page registry must own render and action contracts: {studio_data}"
    );
    assert!(
        studio_data.contains("\"apply_gate\":\"single-source-transaction\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"secret_policy\"")
            && studio_data.contains("\"plaintext_in_projection\":false"),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"fleet\"") && studio_data.contains("\"mode\":\"adaptive\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"canvas_bridge\"")
            && studio_data.contains("\"mode\":\"separate-app-deeplink\""),
        "studio data: {studio_data}"
    );
    assert!(
        studio_data.contains("\"first_boot\"")
            && studio_data.contains("\"role\":\"os-control-center\"")
            && studio_data.contains("\"canvas_first_surface\":false"),
        "first-boot control center must own Studio, not Canvas: {studio_data}"
    );
    assert!(
        studio_data.contains("\"openssh\""),
        "studio data: {studio_data}"
    );
    assert!(
        generation
            .join("studio/first-boot.json")
            .is_file(),
        "expected Studio first-boot control-center projection"
    );
    let first_boot = fs::read_to_string(generation.join("studio/first-boot.json")).unwrap();
    assert!(
        first_boot.contains("\"role\":\"os-control-center\"")
            && first_boot.contains("\"proof\":\"first-boot-control-center-ready\"")
            && first_boot.contains("\"first_surface\":false"),
        "first-boot: {first_boot}"
    );
    assert!(
        generation
            .join("share/xdg/autostart/jetos-studio-first-boot.desktop")
            .is_file(),
        "expected first-boot Studio autostart desktop entry"
    );
    assert!(
        generation.join("sw/bin/jetos-studio-first-boot").is_file(),
        "expected first-boot Studio launcher"
    );
    assert!(
        generation.join("studio/first-boot.pending").is_file(),
        "expected first-boot pending marker"
    );
    assert!(
        generation
            .join("root/run/current-system/studio/app.json")
            .is_file(),
        "expected Studio app in root current-system projection"
    );
    assert!(
        generation
            .join("root/run/current-system/studio/data.json")
            .is_file(),
        "expected Studio data in root current-system projection"
    );
    let studio_html = fs::read_to_string(generation.join("studio/index.html")).unwrap();
    assert!(
        studio_html.contains("jetos Studio"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-registry=\"studio-pages\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"dashboard\"")
            && studio_html.contains("Service configuration")
            && studio_html.contains("Proof/rollback status"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"settings\"")
            && studio_html.contains("data-stage-setting=\"network.hostName\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"changeset\"")
            && studio_html.contains("data-apply-gate=\"single-source-transaction\"")
            && studio_html.contains("data-changeset-action=\"apply\"")
            && studio_html.contains("data-changeset-action=\"discard\"")
            && studio_html.contains("Impact ledger")
            && studio_html.contains("Build only"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-changeset-tray=\"true\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-secret-policy=\"no-plaintext\"")
            && studio_html.contains("plaintext: never projected"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-page-kind=\"fleet\"")
            && studio_html.contains("data-fleet-mode=\"adaptive\"")
            && studio_html.contains("proof-before-switch"),
        "studio: {studio_html}"
    );
    assert!(studio_html.contains("openssh"), "studio: {studio_html}");
    assert!(
        studio_html.contains("network.hostName"),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-stage-source=\"true\"")
            && studio_html.contains("data-pipeline=\"build-switch\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("data-run=\"proof\""),
        "studio: {studio_html}"
    );
    assert!(
        studio_html.contains("const PAGE_CONTROLLERS")
            && studio_html.contains("synthetic-registered-page")
            && studio_html.contains("resolvePageBinding")
            && !studio_html.contains("renderMissing"),
        "synthetic registry entry must resolve renderer, controller, and model contract: {studio_html}"
    );
    assert!(
        studio_html.contains("data-open-canvas=\"source\"")
            && studio_html.contains("Open Canvas")
            && studio_html.contains("jetos Studio"),
        "Studio may deep-link to Canvas while remaining a separate app: {studio_html}"
    );
    let secrets = fs::read_to_string(generation.join("secrets.tmpfs.manifest")).unwrap();
    assert!(
        secrets.contains("wifi\tsecrets/wifi.age"),
        "secrets: {secrets}"
    );
    let vm_proof = generation.join("vm-proof.txt");
    let vm_text = fs::read_to_string(&vm_proof).expect("risk switch writes VM proof");
    assert!(vm_text.contains("plan-sha256:"), "vm proof: {vm_text}");
    assert!(
        vm_text.contains("service-artifacts: pass"),
        "vm proof: {vm_text}"
    );
    let proof = jet()
        .args(["os", "proof", "halcyon", "--json", "--no-color"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        proof.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&proof.stderr)
    );
    let proof_json = String::from_utf8_lossy(&proof.stdout);
    assert!(
        proof_json.contains("\"host\":\"halcyon\""),
        "proof: {proof_json}"
    );
    assert!(proof_json.contains("\"boot\":"), "proof: {proof_json}");
    assert!(
        proof_json.contains("\"provenance\":"),
        "proof: {proof_json}"
    );
    assert!(proof_json.contains("\"vm_proof\":"), "proof: {proof_json}");
}


#[test]
fn os_plan_prints_checked_system_contract_without_building() {
    let root = Scratch::new("os-plan-root");
    let out = jet()
        .args(["os", "plan", "halcyon", "--json", "--no-color", "--offline"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = String::from_utf8_lossy(&out.stdout);
    assert!(json.contains("\"host\":\"halcyon\""), "plan: {json}");
    assert!(json.contains("\"loader\":\"Limine\""), "plan: {json}");
    assert!(json.contains("\"kernel\":\"CachyOS\""), "plan: {json}");
    assert!(
        json.contains("\"key\": \"users.nate.normal\""),
        "plan: {json}"
    );
    assert!(
        !root.join("systems/generations").exists(),
        "plan must not create a generation"
    );
}


#[test]
fn jetos_user_commands_use_same_generation_engine() {
    let root = Scratch::new("jetos-user-root");
    let plan = jetos()
        .args(["user", "plan", "nate", "--json", "--no-color"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let stdout = String::from_utf8_lossy(&plan.stdout);
    assert!(
        stdout.contains("\"kind\":\"jetos.user-generation\"")
            && stdout.contains("\"user\":\"nate\""),
        "plan: {stdout}"
    );

    let build = jetos()
        .args([
            "user",
            "build",
            "nate",
            "--name",
            "user-gen",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(
        root.path
            .join("systems/generations/user-gen/users/nate/profile.json")
            .is_file(),
        "expected user profile artifact"
    );

    let proof = jetos()
        .args(["user", "prove", "nate", "--json", "--no-color"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        proof.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&proof.stderr)
    );
    let stdout = String::from_utf8_lossy(&proof.stdout);
    assert!(
        stdout.contains("\"user\":\"nate\"") && stdout.contains("user-gen"),
        "proof: {stdout}"
    );
}


#[test]
fn os_build_bare_host_uses_current_repo_config() {
    // D-JPK-OSHOST1=C: bare host discovers system.<host> in ./config.jet.
    let proj = Scratch::new("os-repo");
    let root = Scratch::new("os-default-root");
    // A minimal self-contained system (no packages → realizes trivially offline).
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&pkg);
    write_cachyos_source_recipe(&pkg);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("box"), "stderr: {stderr}");
}


#[test]
fn os_cachyos_kernel_source_recipe_builds_boot_artifacts() {
    let proj = Scratch::new("os-kernel-source-build");
    let root = Scratch::new("os-kernel-source-build-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_cachyos_source_recipe(&pkg);
    write_cachyos_source_builder(
        &pkg,
        "#!/bin/sh\nset -eu\nprintf 'MZ built cachyos kernel\\nHdrS\\n' > \"$JETOS_KERNEL_OUT/vmlinuz-cachyos\"\nprintf '070701 built cachyos initrd\\n' > \"$JETOS_KERNEL_OUT/initrd-cachyos\"\n",
    );
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args([
            "os",
            "build",
            "box",
            "--name",
            "kernel-source-built",
            "--no-color",
            "--offline",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let kernel = fs::read_to_string(
        root.path
            .join("systems/generations/kernel-source-built/boot/kernel"),
    )
    .unwrap();
    assert!(kernel.contains("built cachyos kernel"), "kernel: {kernel}");
    let boot = fs::read_to_string(
        root.path
            .join("systems/generations/kernel-source-built/boot/facts.json"),
    )
    .unwrap();
    assert!(
        boot.contains("\"bootstrap\":\"source-built\""),
        "boot: {boot}"
    );
}


#[test]
fn os_cachyos_kernel_source_builder_failure_is_diagnostic() {
    let proj = Scratch::new("os-kernel-source-build-fail");
    let root = Scratch::new("os-kernel-source-build-fail-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_cachyos_source_recipe(&pkg);
    write_cachyos_source_builder(&pkg, "#!/bin/sh\necho compiler missing >&2\nexit 7\n");
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1286]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("cachyos_source_build_failed", diagnostic);
}


#[test]
fn os_cachyos_kernel_requires_first_party_source() {
    let proj = Scratch::new("os-missing-kernel");
    let root = Scratch::new("os-missing-kernel-root");
    fs::write(
        proj.join("config.jet"),
        "module box {\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("missing_cachyos_kernel", &stderr);
}


#[test]
fn os_systemd_init_requires_first_party_source() {
    let proj = Scratch::new("os-missing-systemd");
    let root = Scratch::new("os-missing-systemd-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&pkg);
    write_cachyos_source_recipe(&pkg);
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1281]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("missing_systemd_init", diagnostic);
}


#[test]
fn os_default_gnome_desktop_requires_first_party_packages() {
    let proj = Scratch::new("os-missing-gnome-desktop");
    let root = Scratch::new("os-missing-gnome-desktop-root");
    let kernel = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(kernel.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        kernel.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&kernel);
    write_cachyos_source_recipe(&kernel);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    write_executable(&systemd.join("bin/systemd"), "#!/bin/sh\nexit 0\n");
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64, options: [ services.desktop.profile: .Default ] }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1288]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("missing_gnome_desktop", diagnostic);
}


#[test]
fn os_cachyos_kernel_requires_boot_artifacts() {
    let proj = Scratch::new("os-missing-kernel-artifacts");
    let root = Scratch::new("os-missing-kernel-artifacts-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(&pkg).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1282]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot_trimmed("missing_cachyos_boot_artifacts", diagnostic);
}


#[test]
fn os_cachyos_kernel_rejects_text_boot_artifacts() {
    let proj = Scratch::new("os-text-kernel-artifacts");
    let root = Scratch::new("os-text-kernel-artifacts-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    fs::write(pkg.join("boot/vmlinuz-cachyos"), "not a kernel\n").unwrap();
    fs::write(pkg.join("boot/initrd-cachyos"), "not an initrd\n").unwrap();
    write_cachyos_source_recipe(&pkg);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1282]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot_trimmed("missing_cachyos_boot_artifacts", diagnostic);
}


#[test]
fn os_cachyos_kernel_requires_source_recipe() {
    let proj = Scratch::new("os-missing-kernel-source");
    let root = Scratch::new("os-missing-kernel-source-root");
    let pkg = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(pkg.join("boot")).unwrap();
    fs::create_dir_all(systemd.join("bin")).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        pkg.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&pkg);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(systemd.join("bin/systemd"), "test systemd\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1284]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot_trimmed("missing_cachyos_source_recipe", diagnostic);
}


#[test]
fn os_systemd_init_requires_init_artifact() {
    let proj = Scratch::new("os-missing-systemd-artifact");
    let root = Scratch::new("os-missing-systemd-artifact-root");
    let kernel = proj.join("jet-pkgs/pkgs/cachyos-kernel");
    let systemd = proj.join("jet-pkgs/pkgs/systemd");
    fs::create_dir_all(kernel.join("boot")).unwrap();
    fs::create_dir_all(&systemd).unwrap();
    fs::write(
        proj.join("jet-pkgs/pkg.jet"),
        "payload: { name: \"jet-pkgs\", version: \"0.1.0\" }\npackages: { cachyos-kernel: library, systemd: executable }\n",
    )
    .unwrap();
    fs::write(
        kernel.join("cachyos-kernel.jet"),
        "module cachyos-kernel { }\n",
    )
    .unwrap();
    write_bootlike_cachyos_artifacts(&kernel);
    write_cachyos_source_recipe(&kernel);
    fs::write(systemd.join("systemd.jet"), "module systemd { }\n").unwrap();
    fs::write(
        proj.join("config.jet"),
        "module box {\n    sources: { mine: path@./jet-pkgs }\n    system.box: { target: linux.x64 }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let diagnostic = stderr
        .find("\n  error[E1283]")
        .map(|idx| &stderr[idx..])
        .unwrap_or(&stderr);
    assert_jetos_stderr_snapshot("missing_systemd_init_artifact", diagnostic);
}


#[test]
fn os_missing_host_is_friendly_and_exits_2() {
    let root = Scratch::new("os-no-host");
    let out = jet()
        .args(["os", "build", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("missing_host", &stderr);
}


#[test]
fn os_unknown_host_lists_available_systems() {
    let root = Scratch::new("os-bad-host");
    let out = jet()
        .args(["os", "build", "nope", "--no-color", "--offline"])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("unknown_host", &stderr);
}


#[test]
fn os_missing_config_file_is_friendly() {
    let root = Scratch::new("os-no-config");
    let out = jet()
        .args(["os", "build", "/definitely/not/here@box", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("missing_config", &stderr);
}


#[test]
fn os_retired_option_namespace_is_snapshot_pinned() {
    let proj = Scratch::new("os-bad-namespace");
    let root = Scratch::new("os-bad-namespace-root");
    fs::write(
        proj.join("config.jet"),
        "module box {\n    system.box: { target: linux.x64, options: [net.hostName: \"box\"] }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args(["os", "build", "box", "--no-color", "--offline"])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("retired_namespace", &stderr);
}


#[test]
fn os_generations_are_newest_first_and_rollback_activates_prior() {
    let root = Scratch::new("os-gens-root");
    for name in ["first", "second"] {
        let out = jet()
            .args([
                "os",
                "switch",
                "halcyon",
                "--name",
                name,
                "--no-color",
                "--offline",
            ])
            .current_dir(config_example_dir())
            .env("JETPACK_ROOT", &root.path)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(
        !root
            .join("systems/generations/second/activation-proof.txt")
            .exists(),
        "activation must never mutate a sealed generation"
    );
    let sealed_retry = jet()
        .args([
            "os",
            "build",
            "halcyon",
            "--name",
            "second",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        sealed_retry.status.success(),
        "activation left the sealed proof stale: {}",
        String::from_utf8_lossy(&sealed_retry.stderr)
    );
    for name in ["first", "second"] {
        let proof = fs::read_to_string(
            root.path
                .join("systems/generations")
                .join(name)
                .join("generation-root.json"),
        )
        .unwrap();
        assert!(
            proof.contains("\"kind\":\"jetos.generation-root.v1\"")
                && proof.contains("\"output_digests\":[\"sha256-"),
            "retained generation {name} must keep its typed Hangar root proof: {proof}"
        );
    }
    let journal = root.join("hangar/lifecycle-db/journal");
    let read_journal = || {
        let mut entries = fs::read_dir(&journal)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| (entry.file_name(), fs::read(entry.path()).unwrap()))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    };
    let lifecycle_before_rollback = read_journal();
    let lifecycle_text = lifecycle_before_rollback
        .iter()
        .map(|(_, bytes)| String::from_utf8_lossy(bytes))
        .collect::<String>();
    assert!(
        lifecycle_text.matches("external-consumer").count() >= 2,
        "both retained generations must remain lifecycle roots: {lifecycle_text}"
    );
    let list = jet()
        .args(["os", "generations", "halcyon", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    let second = stdout.find("second").unwrap();
    let first = stdout.find("first").unwrap();
    assert!(
        second < first,
        "generations should be newest-first: {stdout}"
    );
    let proof = fs::read_to_string(
        root.path
            .join("systems/activation-proofs/halcyon/second.txt"),
    )
    .unwrap();
    assert!(proof.contains("service-risk"), "proof: {proof}");
    assert!(
        proof.contains("rollback-proof: pass previous=first"),
        "proof: {proof}"
    );

    let rollback = jet()
        .args(["os", "rollback", "halcyon", "first", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    assert!(
        String::from_utf8_lossy(&rollback.stderr).contains("rolled back"),
        "stderr: {}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    assert_eq!(
        read_journal(),
        lifecycle_before_rollback,
        "rollback changes activation pointers, never lifecycle roots"
    );
    let same = jet()
        .args(["os", "rollback", "halcyon", "first", "--no-color"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert_eq!(same.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&same.stderr);
    assert!(
        stderr.contains("no generation is available"),
        "stderr: {stderr}"
    );
}


#[test]
fn os_vm_run_real_tier_requires_nixpkgs_pin() {
    // E1291 (D-JOS-NIXBACKEND1=C): the hidden real-tier NixOS backend
    // refuses to generate when it can't map every declaration — here
    // there is no `sources:` entry that resolves to a nixpkgs pin, so
    // `map_system_to_nixos` reports it as unmapped instead of silently
    // dropping it. `jet os vm run` hits this before any tool/media check
    // because a fresh disk always routes through `cmd_vm_run_or_build`.
    let proj = Scratch::new("os-vm-real-no-nixpkgs-pin");
    let root = Scratch::new("os-vm-real-no-nixpkgs-pin-root");
    fs::write(
        proj.join("config.jet"),
        "module halcyon {\n    sources: {}\n    system.halcyon: {\n        target: linux.x64,\n        packages: [],\n        services: {},\n        options: [\n            network.hostName: halcyon,\n        ],\n    }\n}\n",
    )
    .unwrap();
    let out = jet()
        .args([
            "os", "vm", "run", "halcyon", "--disk", "halcyon.qcow2", "--no-color", "--offline",
        ])
        .current_dir(&proj.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("real_tier_no_nixpkgs_pin", &stderr);
}


#[test]
fn os_vm_prove_requires_pinned_media_tools() {
    let root = Scratch::new("os-vm-tools-root");
    let tools = Scratch::new("os-vm-tools-empty");
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot_trimmed("vm_tools_missing", &stderr);
    assert!(
        !root.join("systems/vm-proofs").exists(),
        "missing tools must not write VM proof artifacts"
    );
}


#[test]
fn os_vm_run_requires_proved_installed_disk() {
    let root = Scratch::new("os-vm-run-unproven-root");
    let tools = Scratch::new("os-vm-run-unproven-tools");
    write_fake_vm_tools(&tools.path, false);
    let out = jet()
        .args([
            "os",
            "vm",
            "run",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot("vm_run_unproven", &stderr);
}


#[test]
fn os_vm_prove_real_tier_rejects_fake_toolchain() {
    let root = Scratch::new("os-vm-real-root");
    let tools = Scratch::new("os-vm-real-tools");
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--real",
            "--name",
            "vm-real",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_jetos_stderr_snapshot_normalized(
        "vm_real_fake_tools",
        &stderr,
        &[(tools.path.to_str().unwrap(), "<tools>")],
    );
    assert!(
        !root.join("systems/vm-proofs").exists(),
        "real tier must fail before writing replacement proof with fake tools"
    );
}


#[test]
fn os_vm_prove_writes_media_bound_harness() {
    let root = Scratch::new("os-vm-proof-root");
    let tools = Scratch::new("os-vm-proof-tools");
    write_fake_vm_tools(&tools.path, false);
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--name",
            "vm-proof",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[E1285]"),
        "stderr should refuse harness-only proof: {stderr}"
    );
    let proof_dir = root.path.join("systems/vm-proofs");
    let proof = fs::read_dir(&proof_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .expect("vm proof json");
    let data = fs::read_to_string(&proof).unwrap();
    assert!(
        data.contains("\"state\":\"harness-ready\""),
        "proof should not claim guest pass before QEMU run: {data}"
    );
    assert!(
        data.contains("\"proof_tier\":\"plumbing\""),
        "fake-tool harness should be labeled plumbing tier: {data}"
    );
    assert!(data.contains("\"disk\":\"halcyon.qcow2\""), "proof: {data}");
    assert!(
        data.contains("\"media_proof\":"),
        "proof should bind installer media: {data}"
    );
    assert!(
        data.contains("\"expected_guest_proof\":"),
        "proof should name the guest proof artifact path: {data}"
    );
    assert!(
        data.contains("\"sha256\":"),
        "proof should hash tools: {data}"
    );
    assert!(
        data.contains("rollback-generation-bootable"),
        "proof should name guest assertions: {data}"
    );
    assert!(
        data.contains("terminal-login-ready")
            && data.contains("desktop-session-ready")
            && data.contains("graphical-console-ready")
            && data.contains("desktop-launchers-run"),
        "proof should require terminal and desktop readiness: {data}"
    );
    assert!(
        data.contains("\"phase\":\"boot-installer\""),
        "proof should record QEMU boot phase: {data}"
    );
    assert!(
        data.contains("\"phase\":\"boot-installed-disk\""),
        "proof should record reboot phase: {data}"
    );
    assert!(
        data.contains("\"phase\":\"boot-graphical-desktop\""),
        "proof should record graphical desktop phase: {data}"
    );
    assert!(
        data.contains("\"-cdrom\"") && data.contains("jetos-installer-halcyon.iso"),
        "installer phase should boot the ISO media: {data}"
    );
    let installed_phase = data
        .split("\"phase\":\"boot-installed-disk\"")
        .nth(1)
        .and_then(|rest| rest.split("\"phase\":\"boot-graphical-desktop\"").next())
        .expect("installed-disk command in proof");
    assert!(
        installed_phase.contains("\"-boot\",\"c\"")
            && installed_phase.contains("file=halcyon.qcow2,format=qcow2,if=ide")
            && !installed_phase.contains("\"-kernel\"")
            && !installed_phase.contains("\"-initrd\"")
            && !installed_phase.contains("\"-append\""),
        "installed-disk phase should boot firmware/disk, not direct kernel: {installed_phase}"
    );
    assert!(
        data.contains("\"-kernel\"") && data.contains("/boot/kernel"),
        "graphical proof should direct-boot the generation kernel: {data}"
    );
    assert!(
        data.contains("\"-initrd\"") && data.contains("/boot/initrd"),
        "graphical proof should direct-boot the generation initrd: {data}"
    );
    assert!(
        data.contains("jetos.mode=desktop-verify")
            && data.contains("\"-display\"")
            && data.contains("vnc=127.0.0.1:0")
            && data.contains("\"-vga\"")
            && data.contains("\"std\""),
        "graphical proof should expose a fixed VNC-backed stdvga display: {data}"
    );
    assert!(
        data.contains("qemu-xhci,id=xhci")
            && data.contains("usb-kbd,bus=xhci.0")
            && data.contains("usb-tablet,bus=xhci.0"),
        "graphical proof should expose explicit USB input devices for VNC use: {data}"
    );
    assert!(
        data.contains("rdinit=/jetos/init"),
        "graphical QEMU proof should boot the JetOS verifier overlay script: {data}"
    );
    assert!(
        data.contains("jetos.generation=vm-proof"),
        "QEMU proof should bind guest boot to the generation name: {data}"
    );
    assert!(
        data.contains("console=ttyS0"),
        "QEMU proof needs serial output for guest proof marker: {data}"
    );
    assert!(
        proof
            .with_file_name("halcyon-vm-proof-vm-proof.run")
            .join("boot-graphical-desktop.stdout")
            .is_file(),
        "vm prove should run the recorded QEMU phases"
    );
    assert!(
        root.path
            .join("systems/images/jetos-installer-halcyon.iso.proof.json")
            .is_file(),
        "vm prove should build media proof first"
    );
}


#[test]
fn os_vm_prove_runs_qemu_and_records_guest_proof() {
    let root = Scratch::new("os-vm-run-root");
    let tools = Scratch::new("os-vm-run-tools");
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "vm",
            "prove",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--name",
            "vm-live",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-live-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-live-vm-proof-guest-proof.json");
    let final_proof = fs::read_to_string(&proof).unwrap();
    assert!(
        final_proof.contains("\"state\":\"guest-passed\""),
        "proof: {final_proof}"
    );
    assert!(
        final_proof.contains("\"proof_tier\":\"plumbing\""),
        "regular VM proof is harness plumbing until --real passes: {final_proof}"
    );
    assert!(
        final_proof.contains("\"guest_proof_sha256\""),
        "proof: {final_proof}"
    );
    let guest_proof = fs::read_to_string(&guest).unwrap();
    assert!(
        guest_proof.contains("\"serial_report\""),
        "guest proof: {guest_proof}"
    );
    assert!(
        guest_proof.contains("halcyon") && guest_proof.contains("vm-live"),
        "guest serial report should bind host and generation: {guest_proof}"
    );
    assert!(
        guest_proof.contains("\"qemu-system-x86_64\""),
        "guest proof should bind the runner toolchain: {guest_proof}"
    );
    let boot_log = root
        .path
        .join("systems/vm-proofs/halcyon-vm-live-vm-proof.run/boot-graphical-desktop.stdout");
    assert!(
        fs::read_to_string(&boot_log)
            .unwrap()
            .contains("JETOS_GUEST_PROOF"),
        "boot log should carry guest proof marker"
    );

    let run = jet()
        .args([
            "os",
            "vm",
            "run",
            "halcyon",
            "--disk",
            "halcyon.qcow2",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("booting jetos VM halcyon generation vm-live"),
        "stderr: {stderr}"
    );
    assert!(
        stdout.contains("JETOS_GUEST_PROOF"),
        "interactive run should launch QEMU with the proved disk: {stdout}"
    );
}


#[test]
fn os_vm_test_runs_declared_scenario_and_records_proof() {
    let project = Scratch::new("os-vmtest-project");
    copy_dir_recursive(&config_example_dir(), &project.path);
    let mut config = fs::read_to_string(project.path.join("config.jet")).unwrap();
    config.push_str(
        r#"

module vmtest.ssh-smoke {
    hosts: { halcyon: system.halcyon }
    run: test {
        halcyon.wait_for_boot()
        halcyon.assert_unit_active(.openssh)
        halcyon.assert_port_open(22)
    }
}
"#,
    );
    fs::write(project.path.join("config.jet"), config).unwrap();
    let root = Scratch::new("os-vmtest-root");
    let tools = Scratch::new("os-vmtest-tools");
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "vm",
            "test",
            "ssh-smoke",
            "--disk",
            "ssh-smoke.qcow2",
            "--name",
            "vmtest-proof",
            "--no-color",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proof = root
        .path
        .join("systems/vm-tests/ssh-smoke-vmtest-proof.json");
    let data = fs::read_to_string(&proof).unwrap();
    assert!(
        data.contains("\"kind\":\"jetos.vmtest.proof\"")
            && data.contains("\"state\":\"passed\"")
            && data.contains("\"name\":\"ssh-smoke\""),
        "proof: {data}"
    );
    assert!(
        data.contains("\"name\": \"halcyon\"")
            && data.contains("\"system\": \"halcyon\"")
            && data.contains("\"disk\": \"ssh-smoke.qcow2\""),
        "proof: {data}"
    );
    assert!(
        data.contains("\"wait_for_boot\"")
            && data.contains("\"assert_unit_active\"")
            && data.contains("\"assert_port_open\""),
        "proof should capture typed assertion methods: {data}"
    );
    assert!(
        data.contains("halcyon.assert_unit_active(.openssh)"),
        "proof should carry the source test body for replay: {data}"
    );
    assert!(
        root.path
            .join("systems/vm-proofs/halcyon-vmtest-proof-vm-proof.json")
            .is_file(),
        "vmtest should reuse the install/reboot VM proof harness"
    );
}


#[test]
fn os_vm_prove_accepts_matching_guest_proof() {
    let root = Scratch::new("os-vm-guest-proof-root");
    let tools = Scratch::new("os-vm-guest-proof-tools");
    write_fake_vm_tools(&tools.path, false);
    let args = [
        "os",
        "vm",
        "prove",
        "halcyon",
        "--disk",
        "halcyon.qcow2",
        "--name",
        "vm-proof",
        "--no-color",
        "--offline",
    ];
    let first = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof-guest-proof.json");
    let media_proof = root
        .path
        .join("systems/images/jetos-installer-halcyon.iso.proof.json");
    let harness = fs::read_to_string(&proof).unwrap();
    fs::write(
        &guest,
        format!(
            "{{\"state\":\"guest-passed\",\"host\":\"halcyon\",\"generation\":\"vm-proof\",\"disk\":\"halcyon.qcow2\",\"media_proof\":\"{}\",\"media_proof_sha256\":\"{}\",\"installer_iso_fingerprint\":\"{}\",\"assertions\":[\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\",\"rollback-generation-bootable\",\"terminal-login-ready\",\"desktop-session-ready\",\"graphical-console-ready\",\"desktop-launchers-run\"],\"toolchain\":\"{}\"}}\n",
            test_json_escape(&media_proof.display().to_string()),
            harness_json_field(&harness, "media_proof_sha256"),
            harness_json_field(&harness, "installer_iso_fingerprint"),
            test_json_escape(&harness)
        ),
    )
    .unwrap();

    let second = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let final_proof = fs::read_to_string(&proof).unwrap();
    assert!(
        final_proof.contains("\"state\":\"guest-passed\""),
        "proof: {final_proof}"
    );
    assert!(
        final_proof.contains("\"guest_proof_sha256\""),
        "proof: {final_proof}"
    );
}


#[test]
fn os_vm_prove_rejects_incomplete_guest_proof() {
    let root = Scratch::new("os-vm-stale-guest-proof-root");
    let tools = Scratch::new("os-vm-stale-guest-proof-tools");
    write_fake_vm_tools(&tools.path, false);
    let args = [
        "os",
        "vm",
        "prove",
        "halcyon",
        "--disk",
        "halcyon.qcow2",
        "--name",
        "vm-proof",
        "--no-color",
        "--offline",
    ];
    let first = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof-guest-proof.json");
    let media_proof = root
        .path
        .join("systems/images/jetos-installer-halcyon.iso.proof.json");
    let harness = fs::read_to_string(&proof).unwrap();
    fs::write(
        &guest,
        format!(
            "{{\"state\":\"guest-passed\",\"host\":\"halcyon\",\"generation\":\"vm-proof\",\"disk\":\"halcyon.qcow2\",\"media_proof\":\"{}\",\"media_proof_sha256\":\"{}\",\"installer_iso_fingerprint\":\"{}\",\"assertions\":[\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\"],\"toolchain\":\"{}\"}}\n",
            test_json_escape(&media_proof.display().to_string()),
            harness_json_field(&harness, "media_proof_sha256"),
            harness_json_field(&harness, "installer_iso_fingerprint"),
            test_json_escape(&harness)
        ),
    )
    .unwrap();

    let second = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("guest assertions did not match"),
        "stderr: {stderr}"
    );
    let final_proof = fs::read_to_string(&proof).unwrap();
    assert!(
        final_proof.contains("\"state\":\"harness-ready\""),
        "proof should remain unpromoted: {final_proof}"
    );
}


#[test]
fn os_vm_prove_rejects_stale_guest_generation() {
    let root = Scratch::new("os-vm-stale-generation-root");
    let tools = Scratch::new("os-vm-stale-generation-tools");
    write_fake_vm_tools(&tools.path, false);
    let args = [
        "os",
        "vm",
        "prove",
        "halcyon",
        "--disk",
        "halcyon.qcow2",
        "--name",
        "vm-proof",
        "--no-color",
        "--offline",
    ];
    let first = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    let proof = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof.json");
    let guest = root
        .path
        .join("systems/vm-proofs/halcyon-vm-proof-vm-proof-guest-proof.json");
    let media_proof = root
        .path
        .join("systems/images/jetos-installer-halcyon.iso.proof.json");
    let harness = fs::read_to_string(&proof).unwrap();
    fs::write(
        &guest,
        format!(
            "{{\"state\":\"guest-passed\",\"host\":\"halcyon\",\"generation\":\"older-generation\",\"disk\":\"halcyon.qcow2\",\"media_proof\":\"{}\",\"assertions\":[\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\",\"rollback-generation-bootable\",\"terminal-login-ready\",\"desktop-session-ready\",\"graphical-console-ready\",\"desktop-launchers-run\"],\"toolchain\":\"{}\"}}\n",
            test_json_escape(&media_proof.display().to_string()),
            test_json_escape(&harness)
        ),
    )
    .unwrap();

    let second = jet()
        .args(args)
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("`generation` expected `vm-proof`, found `older-generation`"),
        "stderr: {stderr}"
    );
}


#[test]
fn os_image_writes_jetos_installer_media_proof() {
    let root = Scratch::new("os-image-root");
    let tools = Scratch::new("os-image-tools");
    let boot = Scratch::new("os-image-boot");
    fs::write(boot.join("kernel"), "MZ test kernel\nHdrS\n").unwrap();
    fs::write(
        boot.join("initrd"),
        b"070701 test initrd with embedded zstd magic \x28\xb5\x2f\xfd\n",
    )
    .unwrap();
    write_fake_vm_tools(&tools.path, true);
    let out = jet()
        .args([
            "os",
            "image",
            "halcyon",
            "--manual",
            "/dev/sda",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", &tools.path)
        .env("JETOS_CACHYOS_KERNEL", boot.join("kernel"))
        .env("JETOS_CACHYOS_INITRD", boot.join("initrd"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let proof = root
        .path
        .join("systems/images")
        .join("jetos-installer-halcyon.iso.proof.json");
    let data = fs::read_to_string(&proof).unwrap();
    assert!(data.contains("\"brand\":\"jetos\""), "data: {data}");
    assert!(data.contains("\"kind\":\"hybrid-iso\""), "data: {data}");
    assert!(data.contains("\"state\":\"built\""), "data: {data}");
    assert!(data.contains("\"sha256\":"), "data: {data}");
    assert!(
        root.path
            .join("systems/images/jetos-installer-halcyon.iso")
            .is_file(),
        "expected real hybrid ISO artifact"
    );
    let variant_proof = root
        .path
        .join("systems/images")
        .join("jetos-image-variants-halcyon.proof.json");
    let variants = fs::read_to_string(&variant_proof).unwrap();
    assert!(
        variants.contains("\"kind\":\"jetos.image-variants\"")
            && (variants.contains("\"proof\":\"image-variants-smoke-proved\"")
                || variants.contains("\"proof\":\"image-variants-staged\""))
            && variants.contains("\"kind\": \"qcow2\"")
            && variants.contains("\"kind\": \"sd\"")
            && variants.contains("\"kind\": \"netboot-ipxe\""),
        "variants: {variants}"
    );
    // D-JOS-IMAGEPROOF1=C: sparse raw/sd markers must never claim built.
    assert!(
        variants.contains("\"kind\": \"raw\"") && variants.contains("\"state\": \"staged\""),
        "raw sparse marker must be staged: {variants}"
    );
    assert!(
        variants.contains("\"kind\": \"sd\"")
            && variants
                .split("\"kind\": \"sd\"")
                .nth(1)
                .map(|rest| rest.contains("\"state\": \"staged\""))
                .unwrap_or(false),
        "sd sparse marker must be staged: {variants}"
    );
    for artifact in [
        "jetos-halcyon.qcow2",
        "jetos-halcyon.raw",
        "jetos-halcyon-sd.img",
        "jetos-halcyon-netboot/vmlinuz",
        "jetos-halcyon-netboot/initrd",
        "jetos-halcyon-netboot/ipxe.conf",
    ] {
        assert!(
            root.path.join("systems/images").join(artifact).is_file(),
            "expected image variant artifact {artifact}"
        );
    }
    let ipxe = fs::read_to_string(
        root.path
            .join("systems/images/jetos-halcyon-netboot/ipxe.conf"),
    )
    .unwrap();
    assert!(
        ipxe.contains("kernel vmlinuz")
            && ipxe.contains("initrd initrd")
            && ipxe.contains("jetos.mode=run"),
        "ipxe: {ipxe}"
    );
    let staging = root
        .path
        .join("systems/images")
        .join("jetos-installer-halcyon.iso.d");
    let transaction = fs::read_to_string(staging.join("install/transaction.json")).unwrap();
    assert!(
        transaction.contains("\"disk\":\"/dev/sda\""),
        "tx: {transaction}"
    );
    assert!(
        transaction.contains("\"partition-gpt\"")
            && transaction.contains("\"mkfs.vfat-esp\"")
            && transaction.contains("\"install-limine-esp\""),
        "tx: {transaction}"
    );
    let install = fs::read_to_string(staging.join("install/install.sh")).unwrap();
    assert!(
        install.contains("sfdisk --wipe always"),
        "install: {install}"
    );
    assert!(
        install.contains("blockdev --rereadpt"),
        "install: {install}"
    );
    assert!(install.contains("mkfs.vfat -F 32"), "install: {install}");
    assert!(install.contains("mkfs.ext4"), "install: {install}");
    assert!(
        install.contains("EFI/BOOT/BOOTX64.EFI") && install.contains("installed-limine.conf"),
        "install: {install}"
    );
    assert!(install.contains("install-proof.json"), "install: {install}");
    let verify = fs::read_to_string(staging.join("install/guest-verify.sh")).unwrap();
    assert!(
        verify.contains("system=\"$root/var/lib/jetos/generations/")
            && verify.contains("need \"$system/plan.json\""),
        "verify: {verify}"
    );
    assert!(
        verify.contains("jetos verifier: missing $path"),
        "verify: {verify}"
    );
    assert!(
        verify.contains("LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda"),
        "verify should probe partitioned installed disks: {verify}"
    );
    assert!(
        verify.contains("terminal/facts.json")
            && verify.contains("serial-getty@ttyS0.service")
            && verify.contains("desktop/facts.json")
            && verify.contains("sw/bin/gdm")
            && verify.contains("sw/bin/gnome-session")
            && verify.contains("sw/bin/gnome-shell")
            && verify.contains("jetos-desktop-session")
            && verify.contains("jetos-studio")
            && verify.contains("--jetos-proof")
            && verify.contains("desktop-launchers-run"),
        "verify: {verify}"
    );
    assert!(
        verify.contains("for svc in openssh backup metrics"),
        "verify: {verify}"
    );
    assert!(verify.contains("\"rollback\""), "verify: {verify}");
    let initrd_bytes = fs::read(staging.join("boot/initrd")).unwrap();
    assert!(
        initrd_bytes.starts_with(b"070701"),
        "raw newc initrd with embedded compressed payload bytes must stay a raw cpio archive"
    );
    let initrd = String::from_utf8_lossy(&initrd_bytes);
    assert!(initrd.contains("jetos.mode=install"), "initrd: {initrd}");
    assert!(initrd.contains("jetos.mode=verify"), "initrd: {initrd}");
    assert!(
        initrd.contains("jetos.mode=desktop-verify"),
        "initrd: {initrd}"
    );
    assert!(
        initrd.contains("jetos/init"),
        "initrd should carry the JetOS init dispatcher: {initrd}"
    );
    assert!(
        initrd.contains("mount -t proc proc /proc")
            && initrd.contains("mount -t devtmpfs devtmpfs /dev"),
        "initrd dispatcher should prepare proc/dev before reading cmdline: {initrd}"
    );
    assert!(
        initrd.contains("LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda"),
        "initrd should probe partitioned installed disks before installer fallback: {initrd}"
    );
    assert!(
        initrd.contains("use_system_nix")
            && initrd.contains("ln -s \"$system_nix\" /nix"),
        "initrd should expose the installed generation nix store when no initrd /nix exists: {initrd}"
    );
    assert!(
        initrd.contains("jetos/tools/bin/sh")
            && initrd.contains("jetos/tools/bin/sfdisk")
            && initrd.contains("jetos/tools/bin/blockdev"),
        "initrd should carry installer partition tools: {initrd}"
    );
    assert!(
        initrd.contains("exec chroot /sysroot /run/current-system/sbin/init")
            && initrd.contains("SYSTEMD_UNIT_PATH=/etc/systemd/system")
            && initrd.contains("for top in etc sbin sw share studio init systemd lib usr network")
            && initrd.contains("ln -s \"$generation_target/$top\" \"/sysroot/$top\""),
        "initrd run mode should hand off to installed current-system, not fallback shell: {initrd}"
    );
    assert!(
        initrd.contains("jetos/modules/atkbd.ko.xz")
            && initrd.contains("jetos/modules/usbhid.ko.xz")
            && initrd.contains("jetos/modules/xhci-hcd.ko.xz"),
        "initrd should carry keyboard and USB HID modules for VNC input: {initrd}"
    );
    assert!(
        initrd.contains("JETOS_GUEST_PROOF"),
        "initrd should carry guest proof reporter: {initrd}"
    );
    let limine = fs::read_to_string(staging.join("boot/limine.conf")).unwrap();
    assert!(
        limine.contains("/Install jetos 26.10 (Apex) — halcyon")
            && limine.contains("cmdline: console=ttyS0 rdinit=/jetos/init")
            && limine.contains("jetos.disk=/dev/sda"),
        "limine: {limine}"
    );
    let installed_limine =
        fs::read_to_string(staging.join("boot/installed-limine.conf")).unwrap();
    assert!(
        installed_limine.contains("/jetos 26.10 (Apex) — halcyon verify"),
        "installed limine: {installed_limine}"
    );
    let installer_os_release = fs::read_to_string(
        staging.join("jetos/current-system/etc/os-release"),
    )
    .unwrap();
    let installer_usr_os_release = fs::read_to_string(
        staging.join("jetos/current-system/usr/lib/os-release"),
    )
    .unwrap();
    assert_eq!(installer_os_release, installer_usr_os_release);
    assert!(
        installer_os_release.contains("PRETTY_NAME=\"jetos 26.10 (Apex)\"")
            && installer_os_release.contains("VERSION_CODENAME=apex")
    );
    let installer_wallpaper = fs::read_to_string(
        staging.join("jetos/current-system/share/backgrounds/jetos/apex.svg"),
    )
    .unwrap();
    assert!(
        installer_wallpaper.contains("jetos 26.10 Apex")
            && installer_wallpaper.contains("linearGradient")
            && installer_wallpaper.len() > 1_000,
        "installer must contain projected wallpaper bytes"
    );
    for text in [
        limine.as_str(),
        installed_limine.as_str(),
        installer_os_release.as_str(),
        installer_wallpaper.as_str(),
    ] {
        assert!(!text.contains("NixOS") && !text.contains("Yarara"));
    }
    assert!(
        staging.join("boot/efiboot.img").is_file(),
        "installer media should carry a UEFI FAT ESP boot image"
    );
    assert!(
        staging.join("EFI/BOOT/BOOTX64.EFI").is_file(),
        "installer media should expose the EFI loader for target ESP install"
    );
    assert!(
        staging.join("boot/installed-limine.conf").is_file(),
        "installer media should carry installed-disk Limine config"
    );
    assert_eq!(
        fs::read_to_string(staging.join("limine.conf")).unwrap(),
        limine,
        "Limine config should be available at the ISO root and /boot"
    );
    assert!(staging.join("jetos/provenance.json").is_file());
    assert!(
        staging
            .join("jetos/current-system/etc/systemd/system/openssh.service")
            .is_file(),
        "installer media should carry the full generation"
    );
    assert!(
        !fs::symlink_metadata(staging.join("jetos/current-system/plan.json"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "installer media must be self-contained"
    );
    assert!(
        !fs::symlink_metadata(staging.join("jetos/current-system/sbin/init"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "installer media must not point back to the host root"
    );
}


#[test]
fn os_init_writes_guided_ext4_config() {
    let proj = Scratch::new("os-init");
    let out = jet()
        .args(["os", "init", "laptop", "--no-color"])
        .current_dir(&proj.path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = fs::read_to_string(proj.join("config.jet")).unwrap();
    assert!(config.contains("system.laptop"), "config: {config}");
    assert!(config.contains("filesystem.layout"), "config: {config}");
    assert!(config.contains("network.hostName"), "config: {config}");
}

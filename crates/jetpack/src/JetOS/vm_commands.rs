fn cmd_vm(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some((action, rest)) = args.split_first().map(|(a, r)| (a.as_str(), r)) else {
        theme.error(
            "vm needs an action",
            "D-JOS-VMCOMMAND1=A, D-JOS-VMRUN1=A, and D-JOS-VMTEST1=A: the active VM actions are `prove`, `run`, and `test`.",
            "run `jet os vm prove <host> --disk <path>`, `jet os vm run <host> --disk <path>`, or `jet os vm test <scenario> --disk <path>`.",
        );
        return 2;
    };
    if action != Syntax::OS_VM_ACTION_PROVE
        && action != Syntax::OS_VM_ACTION_RUN
        && action != Syntax::OS_VM_ACTION_TEST
    {
        theme.error(
            &format!("`{action}` is not a jetos VM action"),
            "D-JOS-VMCOMMAND1=A, D-JOS-VMRUN1=A, and D-JOS-VMTEST1=A: the active VM actions are `prove`, `run`, and `test`.",
            "run `jet os vm prove <host> --disk <path>`, `jet os vm run <host> --disk <path>`, or `jet os vm test <scenario> --disk <path>`.",
        );
        return 2;
    }
    let real_guest = rest.iter().any(|arg| arg == Syntax::OS_VM_FLAG_REAL);
    let action_args = rest
        .iter()
        .filter(|arg| arg.as_str() != Syntax::OS_VM_FLAG_REAL)
        .cloned()
        .collect::<Vec<_>>();
    let rest = action_args.as_slice();
    if action == Syntax::OS_VM_ACTION_TEST {
        let Some(target) = parse_target_or_report(theme, rest.first().map(String::as_str)) else {
            return 2;
        };
        let disk = flags
            .disk
            .as_deref()
            .or(flags.manual_disk.as_deref())
            .unwrap_or("");
        if disk.is_empty() {
            theme.error(
                "vm test needs a target disk",
                "`jet os vm test` installs each declared host into a proved virtual disk and records scenario proof facts.",
                "pass `--disk ./scenario.qcow2`.",
            );
            return 2;
        }
        return cmd_vm_test(theme, &target, disk, flags);
    }
    let Some(target) = parse_target_or_report(theme, rest.first().map(String::as_str)) else {
        return 2;
    };
    let disk = flags
        .disk
        .as_deref()
        .or(flags.manual_disk.as_deref())
        .unwrap_or("");
    if disk.is_empty() {
        theme.error(
            "vm needs a target disk",
            "`jet os vm prove` installs into a virtual disk; `jet os vm run` opens that proved disk for human use.",
            "pass `--disk ./host.qcow2`.",
        );
        return 2;
    }
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    if action == Syntax::OS_VM_ACTION_RUN {
        return cmd_vm_run(theme, &plan, &system, disk, flags);
    }
    if real_guest {
        // The real tier realizes the disk through the hidden system backend,
        // so it needs a real QEMU and `nix` — not the installer-media
        // toolchain the plumbing tier stages ISOs with.
        if let Err(e) = require_real_vm_tools() {
            theme.error_coded(
                "E1290",
                "jetos real VM proof needs real tools",
                &e,
                "rerun without `--real` for plumbing tests, or put real QEMU/image/media tools on PATH before claiming replacement proof.",
            );
            return 2;
        }
    } else {
        let missing = missing_vm_tools();
        if !missing.is_empty() {
            theme.error_coded(
                "E1279",
                "jetos VM proof tools are missing",
                &format!(
                "D-JOS-VMDEPS1=A requires pinned VM/media tools before installer proof can run; missing: {}.",
                    missing.join(", ")
                ),
                "realize or expose qemu-system-x86_64, qemu-img, xorriso, limine, sfdisk, blockdev, mkfs.ext4, mkfs.vfat, mmd, mcopy, and zstd, then rerun `jet os vm prove`.",
            );
            return 2;
        }
    }
    let mut flags = flags.clone();
    flags.real_tier = real_guest;
    let flags = &flags;
    let Some(gen) = build_generation(theme, &plan, &system, flags) else {
        return 2;
    };
    if real_guest {
        return cmd_vm_prove_real(theme, &gen, &plan.table, &system, disk, flags);
    }
    let media = match write_installer_media(&gen, &system, "guided-ext4") {
        Ok(path) => path,
        Err(e) => {
            theme.error(
                "could not write the jetos installer media",
                &format!("writing installer media artifacts failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            return 2;
        }
    };
    let installer_iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    if !installer_iso.is_file() {
        theme.error(
            "jetos installer ISO was not built",
            &format!(
                "VM proof needs `{}`; media staging did not produce a bootable ISO.",
                installer_iso.display()
            ),
            "inspect the media staging `iso-error.txt`, fix the ISO build failure, then rerun `jet os vm prove`.",
        );
        return 2;
    }
    match write_vm_install_plan(&gen, &system, disk, &media, real_guest) {
        Ok(path) => match prove_vm_guest(&gen, &system, disk, &media, &path, real_guest) {
            Ok(Some(final_path)) => {
                theme.ok(&format!(
                    "proved jetos VM install/reboot {}",
                    final_path.display()
                ));
                0
            }
            Ok(None) => {
                theme.error_coded(
                    "E1285",
                    "jetos VM guest proof has not run",
                    &format!(
                        "the QEMU install/reboot harness was written to `{}`, but no guest boot proof was recorded.",
                        path.display()
                    ),
                    "inspect the VM run logs, fix the boot/install path, then rerun `jet os vm prove` to capture a guest proof marker.",
                );
                2
            }
            Err(e) => {
                theme.error_coded(
                    "E1285",
                    "jetos VM guest proof has not run",
                    &format!(
                        "the guest proof for `{}` is stale or incomplete: {e}.",
                        path.display()
                    ),
                    "rerun the recorded QEMU install/reboot phases and write a matching guest proof artifact.",
                );
                2
            }
        },
        Err(e) => {
            theme.error(
                "could not write the jetos VM proof plan",
                &format!("writing VM proof artifacts failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            2
        }
    }
}

fn cmd_vm_test(theme: &Theme, target: &Target, disk: &str, flags: &OsFlags) -> i32 {
    let Some(plan) = load_plan(theme, target) else {
        return 2;
    };
    let Some(vmtest) = plan.vmtests.iter().find(|t| t.name == target.host).cloned() else {
        let mut names: Vec<String> = plan.vmtests.iter().map(|t| t.name.clone()).collect();
        names.sort();
        let known = if names.is_empty() {
            "this config defines no vmtests".to_string()
        } else {
            format!("available vmtests: {}", names.join(", "))
        };
        theme.error_coded(
            "E0980",
            &format!("`{}` is not a vmtest in this config", target.host),
            &known,
            "define `module vmtest.<name> { hosts: { node: system.<host> }, run: test { ... } }`, or select one of the vmtests above.",
        );
        return 2;
    };
    let missing = missing_vm_tools();
    if !missing.is_empty() {
        theme.error_coded(
            "E1279",
            "jetos VM proof tools are missing",
            &format!(
                "D-JOS-VMTEST1=A runs the same pinned VM/media harness as `jet os vm prove`; missing: {}.",
                missing.join(", ")
            ),
            "realize or expose qemu-system-x86_64, qemu-img, xorriso, limine, sfdisk, blockdev, mkfs.ext4, mkfs.vfat, mmd, mcopy, and zstd, then rerun `jet os vm test`.",
        );
        return 2;
    }
    match run_vmtest(theme, &plan, &vmtest, disk, flags) {
        Ok(path) => {
            theme.ok(&format!("proved jetos VM test {}", path.display()));
            0
        }
        Err(e) => {
            theme.error_coded(
                "E1285",
                "jetos VM test proof has not run",
                &format!("the VM test `{}` did not produce a passing proof: {e}.", vmtest.name),
                "inspect the VM test artifacts, fix the failing host/assertion, then rerun `jet os vm test`.",
            );
            2
        }
    }
}

fn cmd_vm_run(theme: &Theme, plan: &EnvPlan, system: &SystemPlan, disk: &str, flags: &OsFlags) -> i32 {
    // Running a VM is never gated on a proof (owner decree, card #363,
    // 2026-07-09): a missing disk is built by the hidden backend, an existing
    // real-tier disk just boots. Only legacy plumbing disks (no backend
    // artifacts, no real marker) keep the old harness contract below.
    let real_marker = real_tier_proof_marker_path(disk);
    let backend_flake = nixos_backend_dir(&system.name).join("flake.nix");
    if !Path::new(disk).is_file() || real_marker.is_file() || backend_flake.is_file() {
        return cmd_vm_run_or_build(theme, &plan.table, system, disk, flags);
    }
    let missing = missing_vm_tools();
    let missing = missing
        .into_iter()
        .filter(|tool| tool == "qemu-system-x86_64")
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        theme.error_coded(
            "E1279",
            "jetos VM proof tools are missing",
            &format!(
                "D-JOS-VMDEPS1=A requires pinned VM/media tools before a VM can run; missing: {}.",
                missing.join(", ")
            ),
            "realize or expose qemu-system-x86_64, then rerun `jet os vm run`.",
        );
        return 2;
    }
    let Some(gen) = latest_generation_for(&system.name) else {
        theme.error_coded(
            "E1287",
            "jetos VM run needs a proved installed disk",
            &format!(
                "no built generation exists for `{}`; VM launch follows the latest proven generation.",
                system.name
            ),
            "run `jet os vm prove <host> --disk <path>` first.",
        );
        return 2;
    };
    let proof = systems_dir()
        .join("vm-proofs")
        .join(format!("{}-{}-vm-proof.json", system.name, gen.name));
    let media_proof = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.proof.json", system.name));
    match require_vm_run_proof(&gen, system, disk, &media_proof, &proof) {
        Ok(()) => {}
        Err(e) => {
            theme.error_coded(
                "E1287",
                "jetos VM run needs a proved installed disk",
                &format!("the installed disk `{disk}` is not tied to a passing VM proof: {e}."),
                "run `jet os vm prove <host> --disk <path>` first, then rerun `jet os vm run`.",
            );
            return 2;
        }
    }
    let boot_dir = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.d/boot", system.name));
    let boot_dir = if boot_dir.join("initrd").is_file() {
        boot_dir
    } else {
        gen.path.join("boot")
    };
    let command = qemu_interactive_run_command(&boot_dir, disk, &system.name, &gen.name);
    theme.ok(&format!(
        "booting jetos VM {} generation {}",
        theme.bold(&system.name),
        theme.bold(&gen.name)
    ));
    if qemu_has_local_display() {
        theme.detail("graphical console is open in a local QEMU window");
    } else {
        theme.detail(&format!(
            "graphical console is exposed over VNC at {}; serial output is attached here",
            qemu_vnc_endpoint()
        ));
    }
    match run_interactive_vm_command(&command) {
        Ok(code) => code,
        Err(e) => {
            theme.error(
                "could not run the jetos VM",
                &format!("starting interactive QEMU failed: {e}"),
                "check the VM proof artifacts and rerun `jet os vm prove` if the disk changed.",
            );
            2
        }
    }
}

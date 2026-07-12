use super::options_rendering::{
    boot_profile, clean_bool_json, clean_symbol, collect_names, option_rows_json, option_value,
    package_path_or_literal, parse_list_items, prefixed_options, render_user_profile_json_parts,
    safe_filename, shell_single_quote, user_names,
};
use super::root_projection::enable_unit;
use super::studio_projection::make_executable;
use crate::ModuleEval::SystemPlan;
use crate::JSON;
use std::fs;
use std::path::Path;

pub(super) fn write_user_environment_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let users_dir = dir.join("users");
    let unit_dir = dir.join("etc/systemd/user");
    fs::create_dir_all(&users_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let names = user_names(system);
    let mut index = Vec::new();
    for name in names {
        let profile_dir = users_dir.join(&name);
        fs::create_dir_all(profile_dir.join("files"))?;
        let home = option_value(
            system,
            &[&format!("user.{name}.home"), &format!("users.{name}.home")],
        )
        .unwrap_or_else(|| format!("/home/{name}"));
        let shell = option_value(
            system,
            &[
                &format!("user.{name}.shell"),
                &format!("users.{name}.shell"),
            ],
        )
        .map(|s| package_path_or_literal(&s))
        .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
        let packages = option_value(
            system,
            &[
                &format!("user.{name}.packages"),
                &format!("users.{name}.packages"),
            ],
        )
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
        let services = option_value(
            system,
            &[
                &format!("user.{name}.services"),
                &format!("users.{name}.services"),
            ],
        )
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
        let files = prefixed_options(system, &format!("user.{name}.files."));
        let files_json = files
            .iter()
            .map(|(key, value)| {
                JSON::object_of(&[("path", &user_file_target(key, value)), ("source", value)])
            })
            .collect::<Vec<_>>()
            .join(",");
        fs::write(profile_dir.join("home.txt"), format!("{home}\n"))?;
        fs::write(profile_dir.join("shell.txt"), format!("{shell}\n"))?;
        fs::write(
            profile_dir.join("packages.manifest"),
            manifest_lines(&packages),
        )?;
        fs::write(
            profile_dir.join("services.manifest"),
            manifest_lines(&services),
        )?;
        for (rel, source) in &files {
            let target = user_file_target(rel, source);
            let safe = target
                .trim_start_matches('/')
                .trim_start_matches('.')
                .replace('/', "__");
            fs::write(
                profile_dir.join("files").join(safe),
                format!("source={source}\ntarget={target}\n"),
            )?;
        }
        let packages_json = packages
            .iter()
            .map(|p| JSON::quote(p))
            .collect::<Vec<_>>()
            .join(",");
        let services_json = services
            .iter()
            .map(|s| JSON::quote(s))
            .collect::<Vec<_>>()
            .join(",");
        let facts = render_user_profile_json_parts(
            &name,
            &home,
            &shell,
            &packages_json,
            &services_json,
            &files_json,
        );
        fs::write(profile_dir.join("profile.json"), &facts)?;
        fs::write(
            profile_dir.join("proof.txt"),
            format!("user {name}: pass\n"),
        )?;
        fs::write(
            unit_dir.join(format!("jetos-user-{name}.service")),
            format!(
                "[Unit]\nDescription=jetos user environment for {name}\n\n[Service]\nType=oneshot\nExecStart=/run/current-system/sw/bin/jetos-user-apply {name}\n\n[Install]\nWantedBy=default.target\n"
            ),
        )?;
        index.push(facts);
    }
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&bin_dir)?;
    let apply = "#!/usr/bin/env sh\nset -eu\nuser=${1:-}\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nif [ -z \"$user\" ]; then\n  echo 'usage: jetos-user-apply <user>' >&2\n  exit 2\nfi\nprofile_dir=\"$root/users/$user\"\nprofile=\"$profile_dir/profile.json\"\nif [ ! -f \"$profile\" ]; then\n  echo \"jetos user: no profile for $user\" >&2\n  exit 2\nfi\nhome=${JETOS_USER_HOME:-}\nif [ -z \"$home\" ] && [ -f \"$profile_dir/home.txt\" ]; then\n  home=$(sed -n '1p' \"$profile_dir/home.txt\")\nfi\nif [ -z \"$home\" ]; then\n  home=\"$HOME\"\nfi\nmkdir -p \"$home/.jetos/profile/bin\" \"$home/.jetos/proof\" \"$home/.config/systemd/user\"\nfor entry in \"$profile_dir\"/files/*; do\n  [ -f \"$entry\" ] || continue\n  target=$(sed -n 's/^target=//p' \"$entry\")\n  source=$(sed -n 's/^source=//p' \"$entry\")\n  [ -n \"$target\" ] || continue\n  case \"$target\" in\n    /*) dest=\"$target\" ;;\n    *) dest=\"$home/$target\" ;;\n  esac\n  dir=${dest%/*}\n  [ \"$dir\" = \"$dest\" ] || mkdir -p \"$dir\"\n  if [ -f \"$root/$source\" ]; then\n    cp \"$root/$source\" \"$dest\"\n  else\n    printf 'managed-by=jetos\\nuser=%s\\nsource=%s\\n' \"$user\" \"$source\" > \"$dest\"\n  fi\ndone\nif [ -f \"$profile_dir/packages.manifest\" ]; then\n  while IFS= read -r package; do\n    [ -n \"$package\" ] || continue\n    name=${package##*.}\n    src=\"$root/sw/bin/$name\"\n    if [ -e \"$src\" ]; then\n      ln -sfn \"$src\" \"$home/.jetos/profile/bin/$name\"\n    fi\n  done < \"$profile_dir/packages.manifest\"\nfi\nif [ -f \"$profile_dir/services.manifest\" ]; then\n  while IFS= read -r service; do\n    [ -n \"$service\" ] || continue\n    unit=\"$home/.config/systemd/user/$service.service\"\n    printf '[Unit]\\nDescription=jetos user service %s\\n\\n[Service]\\nExecStart=/run/current-system/sw/bin/%s\\n\\n[Install]\\nWantedBy=default.target\\n' \"$service\" \"$service\" > \"$unit\"\n  done < \"$profile_dir/services.manifest\"\nfi\nprintf '{\"state\":\"applied\",\"user\":\"%s\",\"home\":\"%s\",\"profile\":\"%s\"}\\n' \"$user\" \"$home\" \"$profile\" > \"$home/.jetos/proof/user-$user.json\"\ncat \"$home/.jetos/proof/user-$user.json\"\n";
    let apply_path = bin_dir.join("jetos-user-apply");
    fs::write(&apply_path, apply)?;
    make_executable(&apply_path)?;
    fs::write(
        users_dir.join("index.json"),
        format!(
            "{{\"kind\":\"jetos.user-index\",\"host\":{},\"profiles\":[{}]}}",
            JSON::quote(&system.name),
            index.join(",")
        ),
    )
}

fn user_file_target(key: &str, source: &str) -> String {
    if key.starts_with('.') || key.starts_with('/') || key.contains('/') {
        return key.to_string();
    }
    if let Some(rest) = source.strip_prefix("home/") {
        return format!(".config/{rest}");
    }
    format!(".config/{key}")
}

pub(super) fn manifest_lines(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("{}\n", items.join("\n"))
    }
}

pub(super) fn write_flatpak_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let flatpak_dir = dir.join("flatpak");
    let appimage_dir = dir.join("appimage");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&flatpak_dir)?;
    fs::create_dir_all(&appimage_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let options = prefixed_options(system, "apps.flatpak.");
    let remotes = prefixed_options(system, "apps.flatpak.remotes.");
    let apps = collect_names(system, "apps.flatpak.app");
    let appimages = collect_names(system, "apps.appimage.app");
    let apps_json = apps
        .iter()
        .map(|name| {
            let ref_id = option_value(system, &[&format!("apps.flatpak.app.{name}.ref")])
                .unwrap_or_else(|| name.clone());
            let pin = option_value(system, &[&format!("apps.flatpak.app.{name}.pin")])
                .unwrap_or_else(|| "tracking".to_string());
            JSON::object_of(&[("name", name), ("ref", &ref_id), ("pin", &pin)])
        })
        .collect::<Vec<_>>()
        .join(",");
    let appimages_json = appimages
        .iter()
        .map(|name| {
            let path = option_value(system, &[&format!("apps.appimage.app.{name}.path")])
                .unwrap_or_else(|| name.clone());
            let integrate = option_value(system, &[&format!("apps.appimage.app.{name}.integrate")])
                .unwrap_or_else(|| "true".to_string());
            JSON::object_of(&[
                ("name", name),
                ("path", &path),
                ("integrate", clean_bool_json(&integrate)),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let reconcile = option_value(system, &["apps.flatpak.reconcile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Exact".to_string());
    fs::write(
        flatpak_dir.join("plan.json"),
        format!(
            "{{\"kind\":\"jetos.flatpak-plan\",\"reconcile\":{},\"apps\":[{}],\"appimages\":[{}],\"options\":[{}],\"proof\":\"flatpak-reconcile-planned\"}}",
            JSON::quote(&reconcile),
            apps_json,
            appimages_json,
            option_rows_json(&options)
        ),
    )?;
    fs::write(
        appimage_dir.join("plan.json"),
        format!(
            "{{\"kind\":\"jetos.appimage-plan\",\"apps\":[{}],\"runner\":\"sw/bin/jetos-appimage-run\",\"proof\":\"appimage-runtime-integrated\"}}",
            appimages_json
        ),
    )?;
    fs::write(
        flatpak_dir.join("permissions.manifest"),
        options
            .iter()
            .filter(|(key, _)| key.contains(".permissions."))
            .map(|(key, value)| format!("{key}\t{value}\n"))
            .collect::<String>(),
    )?;
    let mut script = String::from(
        "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nflatpak=${JETOS_FLATPAK_BIN:-flatpak}\nproof_dir=${JETOS_FLATPAK_PROOF_DIR:-$root/flatpak}\nmkdir -p \"$proof_dir\"\nproof=\"$proof_dir/reconcile-proof.json\"\nrun() {\n  printf '%s\\n' \"jetos flatpak: $*\"\n  \"$flatpak\" \"$@\"\n}\n",
    );
    let mut declared_refs = Vec::new();
    for (remote, url) in remotes {
        script.push_str(&format!(
            "run remote-add --if-not-exists {} {}\n",
            shell_single_quote(&remote),
            shell_single_quote(&url)
        ));
    }
    for app in &apps {
        let ref_id = option_value(system, &[&format!("apps.flatpak.app.{app}.ref")])
            .unwrap_or_else(|| app.clone());
        declared_refs.push(ref_id.clone());
        let remote = option_value(system, &[&format!("apps.flatpak.app.{app}.remote")])
            .unwrap_or_else(|| "flathub".to_string());
        script.push_str(&format!(
            "run install -y {} {}\n",
            shell_single_quote(&remote),
            shell_single_quote(&ref_id)
        ));
        for (key, value) in
            prefixed_options(system, &format!("apps.flatpak.app.{app}.permissions."))
        {
            let flag = key.replace('.', "-");
            script.push_str(&format!(
                "run override {} --{}={}\n",
                shell_single_quote(&ref_id),
                flag,
                shell_single_quote(&clean_symbol(&value))
            ));
        }
    }
    if reconcile == "Exact" {
        script.push_str(&format!(
            "declared={}\ninstalled=$(\"$flatpak\" list --app --columns=application 2>/dev/null || true)\nfor app in $installed; do\n  case \" $declared \" in\n    *\" $app \"*) ;;\n    *) run uninstall -y \"$app\" ;;\n  esac\ndone\n",
            shell_single_quote(&declared_refs.join(" "))
        ));
        script.push_str("run update -y\n");
    }
    script.push_str(
        "printf '{\"state\":\"reconciled\",\"proofs\":[\"remotes\",\"apps\",\"permissions\"]}\\n' > \"$proof\"\ncat \"$proof\"\n",
    );
    let reconcile_path = bin_dir.join("jetos-flatpak-reconcile");
    fs::write(&reconcile_path, script)?;
    make_executable(&reconcile_path)?;
    for name in &appimages {
        let path = option_value(system, &[&format!("apps.appimage.app.{name}.path")])
            .unwrap_or_else(|| name.clone());
        fs::write(
            appimage_dir.join(format!("{}.desktop", safe_filename(name))),
            format!(
                "[Desktop Entry]\nName={name}\nType=Application\nExec=/run/current-system/sw/bin/jetos-appimage-run {name}\n"
            ),
        )?;
        fs::write(
            appimage_dir.join(format!("{}.path", safe_filename(name))),
            format!("{path}\n"),
        )?;
    }
    let appimage_runner = "#!/usr/bin/env sh\nset -eu\nname=${1:-}\nif [ -z \"$name\" ]; then\n  echo 'usage: jetos-appimage-run <name> [--print]' >&2\n  exit 2\nfi\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\npath_file=\"$root/appimage/$name.path\"\nif [ ! -f \"$path_file\" ]; then\n  echo \"jetos appimage: no app named $name\" >&2\n  exit 2\nfi\napp=$(sed -n '1p' \"$path_file\")\nif [ \"${2:-}\" = '--print' ]; then\n  printf '%s\\n' \"$app\"\n  exit 0\nfi\nexec \"$app\"\n";
    let appimage_path = bin_dir.join("jetos-appimage-run");
    fs::write(&appimage_path, appimage_runner)?;
    make_executable(&appimage_path)
}

pub(super) fn write_performance_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let perf_dir = dir.join("performance");
    let bin_dir = dir.join("sw/bin");
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&perf_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let profile = option_value(system, &["performance.profile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Safe".to_string());
    let kernel_profile = option_value(
        system,
        &["boot.kernel.profile", "performance.kernel.profile"],
    )
    .map(|s| clean_symbol(&s))
    .unwrap_or_else(|| boot_profile(system).kernel);
    let sysctls = prefixed_options(system, "performance.sysctl.");
    let mut sysctl_conf = String::new();
    for (key, value) in &sysctls {
        sysctl_conf.push_str(&format!("{key} = {value}\n"));
    }
    if !sysctl_conf.is_empty() {
        let sysctl_dir = dir.join("etc/sysctl.d");
        fs::create_dir_all(&sysctl_dir)?;
        fs::write(sysctl_dir.join("90-jetos-performance.conf"), sysctl_conf)?;
    }
    if let Some(percent) = option_value(system, &["performance.zram.memoryPercent"]) {
        let zram_dir = dir.join("etc/systemd/zram-generator.conf.d");
        fs::create_dir_all(&zram_dir)?;
        fs::write(
            zram_dir.join("jetos.conf"),
            format!("[zram0]\nzram-size = ram * {percent} / 100\n"),
        )?;
    }
    let scheduler = option_value(system, &["performance.scheduler"]).map(|s| clean_symbol(&s));
    if let Some(scheduler) = &scheduler {
        let scheduler_bin = match scheduler.as_str() {
            "ScxLavd" => "scx_lavd".to_string(),
            _ => scheduler.to_ascii_lowercase(),
        };
        let launcher = bin_dir.join("jetos-performance-scheduler");
        fs::write(
            &launcher,
            format!(
                "#!/usr/bin/env sh\nset -eu\nscheduler=${{JETOS_SCHEDULER_BIN:-{}}}\nexec \"$scheduler\" \"$@\"\n",
                shell_single_quote(&scheduler_bin)
            ),
        )?;
        make_executable(&launcher)?;
        fs::write(
            unit_dir.join("jetos-performance-scheduler.service"),
            "[Unit]\nDescription=jetos sched-ext scheduler\nAfter=multi-user.target\n\n[Service]\nExecStart=/run/current-system/sw/bin/jetos-performance-scheduler\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
        )?;
        enable_unit(
            &unit_dir,
            "multi-user.target",
            "jetos-performance-scheduler.service",
        )?;
    }
    let params = option_value(system, &["boot.kernel.params", "performance.kernel.params"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let params_json = params
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let initrd_systemd =
        option_value(system, &["boot.initrd.systemd"]).unwrap_or_else(|| "false".to_string());
    let initrd_verbosity =
        option_value(system, &["boot.initrd.verbosity"]).unwrap_or_else(|| "normal".to_string());
    let limine_max = option_value(system, &["boot.loader.limine.maxGenerations"])
        .unwrap_or_else(|| "10".to_string());
    let efi_vars = option_value(system, &["boot.loader.efi.canTouchVariables"])
        .unwrap_or_else(|| "false".to_string());
    fs::write(
        perf_dir.join("profile.json"),
        format!(
            "{{\"kind\":\"jetos.performance-profile\",\"profile\":{},\"kernel_profile\":{},\"kernel_params\":[{}],\"proof\":\"kernel-tuning-profile-ready\"}}",
            JSON::quote(&profile),
            JSON::quote(&kernel_profile),
            params_json
        ),
    )?;
    fs::write(
        perf_dir.join("bootloader.json"),
        format!(
            "{{\"kind\":\"jetos.bootloader-tuning\",\"limine_max_generations\":{},\"efi_can_touch_variables\":{},\"proof\":\"bootloader-tuning-ready\"}}",
            JSON::quote(&limine_max),
            clean_bool_json(&efi_vars)
        ),
    )?;
    fs::write(
        perf_dir.join("initrd.json"),
        format!(
            "{{\"kind\":\"jetos.initrd-tuning\",\"systemd\":{},\"verbosity\":{},\"proof\":\"initrd-tuning-ready\"}}",
            clean_bool_json(&initrd_systemd),
            JSON::quote(&initrd_verbosity)
        ),
    )?;
    fs::write(
        perf_dir.join("scheduler.json"),
        format!(
            "{{\"kind\":\"jetos.scheduler\",\"scheduler\":{},\"unit\":{},\"proof\":\"sched-ext-service-ready\"}}",
            scheduler
                .as_ref()
                .map(|s| JSON::quote(s))
                .unwrap_or_else(|| "null".to_string()),
            if scheduler.is_some() {
                JSON::quote("etc/systemd/system/jetos-performance-scheduler.service")
            } else {
                "null".to_string()
            }
        ),
    )?;
    fs::write(
        perf_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.performance\",\"profile\":{},\"kernel_profile\":{},\"scheduler\":{},\"kernel_params\":[{}],\"sysctl\":[{}],\"zram\":{},\"initrd\":\"performance/initrd.json\",\"bootloader\":\"performance/bootloader.json\",\"risk\":\"explicit-overrides-proof-visible\"}}",
            JSON::quote(&profile),
            JSON::quote(&kernel_profile),
            scheduler
                .as_ref()
                .map(|s| JSON::quote(s))
                .unwrap_or_else(|| "null".to_string()),
            params_json,
            option_rows_json(&sysctls),
            option_value(system, &["performance.zram.memoryPercent"])
                .map(|p| JSON::quote(&p))
                .unwrap_or_else(|| "null".to_string())
        ),
    )
}

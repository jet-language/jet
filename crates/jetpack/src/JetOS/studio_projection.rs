pub(super) fn write_studio_app_projection(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let studio_dir = dir.join("studio");
    let bin_dir = dir.join("sw/bin");
    let desktop_dir = dir.join("share/applications");
    let autostart_dir = dir.join("share/xdg/autostart");
    fs::create_dir_all(&studio_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&desktop_dir)?;
    fs::create_dir_all(&autostart_dir)?;

    let app = JSON::object_of(&[
        ("kind", "jetos-studio-app"),
        ("host", &system.name),
        ("runtime", "jetos-system-app"),
        ("protocol", "local-projection-service"),
        ("source_truth", "jet-source-transactions"),
        ("semantic_state", "jet-source-only"),
        ("browser_fallback", "true"),
        ("canvas_coupled", "false"),
        ("first_boot_role", "os-control-center"),
    ]);
    fs::write(studio_dir.join("app.json"), app)?;
    fs::write(studio_dir.join("data.json"), studio_data_json(dir, system))?;
    fs::write(studio_dir.join("index.html"), studio_index_html(dir, system))?;
    fs::write(
        studio_dir.join("first-boot.json"),
        studio_first_boot_json(dir, system),
    )?;
    fs::write(studio_dir.join("first-boot.pending"), "1\n")?;

    let launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_STUDIO_ROOT:-/run/current-system}\npage=\"$root/studio/index.html\"\nif command -v jetos >/dev/null 2>&1; then\n  exec jetos studio \"$@\"\nfi\nif command -v xdg-open >/dev/null 2>&1; then\n  exec xdg-open \"$page\"\nfi\nprintf '%s\\n' \"$page\"\n";
    let launcher_path = bin_dir.join("jetos-studio");
    fs::write(&launcher_path, launcher)?;
    make_executable(&launcher_path)?;

    // D-JOS-FIRSTBOOT1=D: first graphical session opens Studio as the OS
    // control center. Canvas stays a deep-link from source spans only.
    let first_boot = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_STUDIO_ROOT:-/run/current-system}\nmarker=${JETOS_FIRST_BOOT_MARKER:-$root/studio/first-boot.pending}\nif [ ! -f \"$marker\" ]; then\n  exit 0\nfi\nif command -v jetos-studio >/dev/null 2>&1; then\n  jetos-studio --first-boot || true\nelif [ -x \"$root/sw/bin/jetos-studio\" ]; then\n  \"$root/sw/bin/jetos-studio\" --first-boot || true\nfi\nrm -f \"$marker\"\n";
    let first_boot_path = bin_dir.join("jetos-studio-first-boot");
    fs::write(&first_boot_path, first_boot)?;
    make_executable(&first_boot_path)?;

    fs::write(
        desktop_dir.join("jetos-studio.desktop"),
        "[Desktop Entry]\nName=jetos Studio\nComment=Edit jetos system source\nExec=/run/current-system/sw/bin/jetos-studio\nType=Application\nCategories=System;Settings;\n",
    )?;
    fs::write(
        autostart_dir.join("jetos-studio-first-boot.desktop"),
        "[Desktop Entry]\nName=jetos Studio First Boot\nComment=Open the jetos control center on first boot\nExec=/run/current-system/sw/bin/jetos-studio-first-boot\nType=Application\nX-GNOME-Autostart-enabled=true\nX-KDE-autostart-condition=true\nOnlyShowIn=GNOME;KDE;XFCE;Hyprland;niri;\n",
    )
}

fn studio_first_boot_json(dir: &Path, system: &SystemPlan) -> String {
    let generation = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let proof_present = dir.join("proof.txt").is_file();
    let health_present = dir.join("health").exists() || dir.join("acceptance").exists();
    format!(
        "{{\"kind\":\"jetos.studio.first-boot\",\"role\":\"os-control-center\",\"host\":{},\"generation\":{},\"surfaces\":[\"host\",\"generation\",\"source\",\"proof\",\"update\",\"rollback\",\"health\"],\"source_truth\":\"jet-source-only\",\"proof_visible\":{},\"health_visible\":{},\"update_path\":\"jet os switch\",\"rollback_path\":\"jet os rollback\",\"canvas\":{{\"mode\":\"separate-app-deeplink\",\"path\":\"/canvas\",\"from\":\"source-spans-only\",\"first_surface\":false}},\"autostart\":\"share/xdg/autostart/jetos-studio-first-boot.desktop\",\"proof\":\"first-boot-control-center-ready\"}}",
        JSON::quote(&system.name),
        JSON::quote(generation),
        if proof_present { "true" } else { "false" },
        if health_present { "true" } else { "false" }
    )
}

const STUDIO_PAGES: &[(&str, &str, &str, &str, &str, &str, bool)] = &[
    ("dashboard", "Dashboard", "operations", "dashboard,services,artifacts", "read-only", "system_plan,proof_state", true),
    ("settings", "Settings", "settings", "options,source", "studio-actions", "system_plan.options,changeset", false),
    ("monitoring", "Monitoring", "operations", "services,artifacts", "read-only", "system_plan.services,proof_state", true),
    ("services", "Services", "operations", "services", "read-only", "system_plan.services", true),
    ("packages", "Packages", "inventory", "packages", "read-only", "system_plan.packages", true),
    ("secrets", "Secrets", "settings", "options", "read-only", "system_plan.options", true),
    ("fleet", "Fleet", "operations", "fleet,proof", "read-only", "proof_state", true),
    ("generations", "Generations", "operations", "artifacts", "studio-actions", "generations,changeset", false),
    ("changeset", "Changeset", "review", "source,diff,impact", "studio-actions", "changeset", false),
    (
        "proof-provenance",
        "Proof/Provenance",
        "audit",
        "artifacts,run",
        "studio-actions",
        "system_plan,proof_state,generations",
        false,
    ),
];

fn studio_data_json(dir: &Path, system: &SystemPlan) -> String {
    let pages = studio_pages_json();
    let packages = system
        .packages
        .iter()
        .map(|pkg| {
            JSON::object_of(&[
                ("name", &pkg.name),
                (
                    "source",
                    if pkg.source.is_empty() {
                        "default"
                    } else {
                        &pkg.source
                    },
                ),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let services = system
        .services
        .iter()
        .map(|svc| {
            let fields = svc
                .extra
                .iter()
                .map(|(key, value)| JSON::object_of(&[("key", key), ("value", value)]))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"name\":{},\"enable\":{},\"fields\":[{}]}}",
                JSON::quote(&svc.name),
                if svc.enable { "true" } else { "false" },
                fields
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let enabled_services = system.services.iter().filter(|svc| svc.enable).count();
    let options = system
        .options
        .iter()
        .map(|opt| JSON::object_of(&[("key", &opt.key), ("value", &opt.value)]))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"jetos-studio-projection\",\"host\":{},\"target\":{},\"dashboard\":{{\"selected_host\":{},\"generation\":{},\"service_configuration\":\"{enabled}/{total} enabled\",\"runtime_health\":\"not-projected\",\"alerts\":[],\"last_run\":\"generation-built\",\"rollback_status\":\"not-evaluated\"}},\"page_registry\":[{}],\"packages\":[{}],\"services\":[{}],\"options\":[{}],\"changeset\":{{\"state\":\"empty\",\"apply_gate\":\"single-source-transaction\",\"proof_required\":[\"check\",\"plan\",\"build\",\"proof\"]}},\"secret_policy\":{{\"plaintext_in_projection\":false,\"visible_fields\":[\"name\",\"source\",\"runtime_path\",\"provenance\"],\"transaction\":\"audited-secret-rekey\"}},\"fleet\":{{\"mode\":\"adaptive\",\"single_host_default\":true,\"rollout_gate\":\"proof-before-switch\"}},\"first_boot\":{{\"role\":\"os-control-center\",\"autostart\":true,\"surfaces\":[\"host\",\"generation\",\"source\",\"proof\",\"update\",\"rollback\",\"health\"],\"canvas_first_surface\":false}},\"canvas_bridge\":{{\"mode\":\"separate-app-deeplink\",\"target\":\"/canvas\",\"shared_state\":\"source-only\",\"from\":\"source-spans-only\"}},\"artifacts\":{{\"plan\":{},\"proof\":{},\"provenance\":{},\"vm_proof\":{}}}}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        JSON::quote(&system.name),
        JSON::quote(
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unrecorded")
        ),
        pages,
        packages,
        services,
        options,
        studio_artifact_json(dir, "plan.json"),
        studio_artifact_json(dir, "proof.txt"),
        studio_artifact_json(dir, "provenance.json"),
        studio_artifact_json(dir, "vm-proof.txt"),
        enabled = enabled_services,
        total = system.services.len()
    )
}

fn studio_artifact_json(dir: &Path, name: &str) -> String {
    format!(
        "{{\"path\":{},\"present\":{}}}",
        JSON::quote(&format!("../{name}")),
        if dir.join(name).is_file() { "true" } else { "false" }
    )
}

pub(crate) fn studio_pages_json() -> String {
    STUDIO_PAGES
        .iter()
        .map(|(id, title, group, needs, controller, contract, read_only)| {
            format!("{{\"id\":{},\"title\":{},\"group\":{},\"renderer\":{},\"controller\":{},\"model_contract\":{},\"read_only\":{},\"data_needs\":{}}}",
                JSON::quote(id),
                JSON::quote(title),
                JSON::quote(group),
                JSON::quote(id),
                JSON::quote(controller),
                JSON::quote(contract),
                if *read_only { "true" } else { "false" },
                JSON::quote(needs),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn studio_index_html(dir: &Path, system: &SystemPlan) -> String {
    let enabled_services = system.services.iter().filter(|svc| svc.enable).count();
    let generation = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unrecorded");
    let proof_status = if dir.join("proof.txt").is_file() {
        "proof artifact present"
    } else {
        "proof not run"
    };
    let nav = STUDIO_PAGES.iter().map(|(id, title, _, _, _, _, _)| {
        format!(
            "<a href=\"#{id}\" data-page=\"{id}\">{}</a>",
            html_escape(title)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let packages = system
        .packages
        .iter()
        .map(|pkg| {
            let source = if pkg.source.is_empty() {
                "default"
            } else {
                &pkg.source
            };
            format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                html_escape(&pkg.name),
                html_escape(source)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let services = system
        .services
        .iter()
        .map(|svc| {
            let fields = if svc.extra.is_empty() {
                String::new()
            } else {
                svc.extra
                    .iter()
                    .map(|(key, value)| format!("{key}: {value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&svc.name),
                if svc.enable { "enabled" } else { "disabled" },
                html_escape(&fields)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let options = system
        .options
        .iter()
        .map(|opt| {
            format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                html_escape(&opt.key),
                html_escape(&opt.value)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let settings_controls = system
        .options
        .iter()
        .map(|opt| {
            format!(
                "<div class=\"setting-control\"><label><span>{}</span><input data-setting-key=\"{}\" value=\"{}\"></label><button type=\"button\" data-stage-setting=\"{}\">Stage change</button></div>",
                html_escape(&opt.key),
                html_escape(&opt.key),
                html_escape(&opt.value),
                html_escape(&opt.key),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<!doctype html>
<html>
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>jetos Studio - {host}</title>
<style>
:root {{ --paper: #eef3f6; --panel: #fbfdfe; --ink: #142430; --muted: #607482; --nav: #132b3a; --accent: #32769a; --ok: #1b7f70; --warn: #b56f1c; --line: #c8d4dc; font-family: \"IBM Plex Sans\", Inter, ui-sans-serif, system-ui, sans-serif; background: var(--paper); color: var(--ink); }}
body {{ margin: 0; min-height: 100vh; background: var(--paper); }}
main {{ display: grid; grid-template-columns: 248px 1fr; min-height: 100vh; }}
aside {{ border-right: 1px solid #0d202b; padding: 22px 16px; background: var(--nav); color: #f3f8fa; }}
section {{ padding: 28px; }}
h1, h2 {{ margin: 0; font-weight: 650; }}
h1 {{ font-size: 22px; letter-spacing: -0.025em; }}
h2 {{ font-size: 15px; margin-bottom: 12px; }}
.nav {{ display: grid; gap: 4px; margin-top: 24px; }}
.nav a {{ color: #b9cbd4; text-decoration: none; border-left: 2px solid transparent; border-radius: 3px; padding: 8px 10px; font-size: 14px; }}
.nav a:hover, .nav a.active {{ background: #1d3b4d; border-left-color: #67b6d5; color: #ffffff; }}
.host {{ display: grid; gap: 6px; margin-top: 20px; }}
.host .pill {{ border-color: #466374; background: #193748; color: #ffffff; }}
.host > span:last-child {{ color: #a9bec9; font: 12px \"JetBrains Mono\", ui-monospace, monospace; }}
.pill {{ border: 1px solid var(--line); border-radius: 999px; padding: 6px 10px; width: max-content; background: var(--panel); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; }}
.panel {{ border: 1px solid var(--line); border-radius: 7px; background: var(--panel); overflow: hidden; box-shadow: 0 5px 18px rgba(20, 36, 48, .05); }}
.panel header {{ padding: 14px 16px; border-bottom: 1px solid var(--line); }}
.hero {{ display: grid; gap: 14px; margin-bottom: 18px; }}
.metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 10px; }}
.metric {{ border: 1px solid var(--line); border-radius: 7px; background: var(--panel); padding: 14px; color: var(--muted); }}
.metric strong {{ display: block; font-size: 22px; margin-top: 4px; }}
.lineage {{ display: grid; grid-template-columns: repeat(4, 1fr); border: 1px solid var(--line); border-radius: 7px; background: #dfe9ee; overflow: hidden; }}
.lineage span {{ position: relative; padding: 11px 14px; color: #294554; font: 12px \"JetBrains Mono\", ui-monospace, monospace; }}
.lineage span + span {{ border-left: 1px solid #b5c7d1; }}
.lineage small {{ display: block; margin-bottom: 3px; color: var(--accent); font: 600 10px \"IBM Plex Sans\", ui-sans-serif, sans-serif; letter-spacing: .09em; text-transform: uppercase; }}
.page {{ display: none; }}
.page.active {{ display: block; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
td {{ padding: 10px 16px; border-top: 1px solid #e7ebf0; vertical-align: top; }}
td:first-child {{ color: #245d7a; font-weight: 600; white-space: nowrap; }}
.status {{ display: flex; flex-wrap: wrap; gap: 8px; margin: 18px 0 24px; }}
.empty {{ color: var(--muted); padding: 16px; }}
.form {{ display: grid; grid-template-columns: 1fr 1fr; gap: 10px; padding: 16px; }}
.setting-control {{ display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; align-items: end; }}
label {{ display: grid; gap: 6px; font-size: 12px; color: var(--muted); }}
input {{ min-width: 0; border: 1px solid var(--line); border-radius: 5px; padding: 9px 10px; background: #ffffff; color: var(--ink); }}
.actions {{ display: flex; flex-wrap: wrap; gap: 8px; padding: 0 16px 16px; }}
button {{ border: 1px solid #9eb1bc; border-radius: 5px; padding: 8px 11px; background: #e6eef2; color: var(--ink); cursor: pointer; }}
button:hover {{ border-color: var(--accent); background: #dce9ef; }}
a:focus-visible, button:focus-visible, input:focus-visible {{ outline: 3px solid rgba(50, 118, 154, .32); outline-offset: 2px; }}
pre {{ margin: 0; padding: 16px; min-height: 96px; max-height: 300px; overflow: auto; border-top: 1px solid var(--line); color: #22313f; background: #f4f8fa; font: 12px/1.5 \"JetBrains Mono\", ui-monospace, monospace; }}
.changeset-tray {{ position: sticky; bottom: 0; z-index: 2; border-top: 1px solid #91aab7; background: #f8fbfc; padding: 12px 16px; display: flex; gap: 10px; align-items: center; justify-content: space-between; box-shadow: 0 -8px 24px rgba(20, 36, 48, .08); }}
@media (max-width: 720px) {{ main {{ grid-template-columns: 1fr; }} aside {{ border-right: 0; border-bottom: 1px solid #0d202b; }} section {{ padding: 18px; }} .form, .lineage {{ grid-template-columns: 1fr; }} .lineage span + span {{ border-left: 0; border-top: 1px solid #b5c7d1; }} }}
</style>
</head>
<body>
<main id=\"studio\" data-host=\"{host}\" data-protocol=\"local-projection-service\" data-source-truth=\"jet-source-transactions\">
<aside>
<h1>jetos Studio</h1>
<div class=\"host\">
<span class=\"pill\">{host}</span>
<span>{target}</span>
</div>
<nav class=\"nav\" data-page-registry=\"studio-pages\">{nav}</nav>
</aside>
<section>
<div id=\"dashboard\" class=\"page active\" data-page-kind=\"dashboard\">
<div class=\"hero\">
<h2>Dashboard</h2>
<div class=\"lineage\" aria-label=\"Source to built generation\">
<span><small>Source</small>config.jet</span>
<span><small>Plan</small>plan.json</span>
<span><small>Proof</small>proof.txt</span>
<span><small>Generation</small>{generation}</span>
</div>
<div class=\"status\">
<span class=\"pill\">selected host: {host}</span>
<span class=\"pill\">generation: {generation}</span>
<span class=\"pill\">rollback: not evaluated</span>
</div>
<div class=\"metrics\">
<div class=\"metric\">Services configured<strong data-live-count=\"services\">{enabled_services}/{service_total}</strong></div>
<div class=\"metric\">Packages<strong data-live-count=\"packages\">{package_total}</strong></div>
<div class=\"metric\">Options<strong data-live-count=\"options\">{option_total}</strong></div>
<div class=\"metric\">Projection alerts<strong>0</strong></div>
</div>
</div>
<div class=\"grid\">
<article class=\"panel\"><header><h2>Service configuration</h2></header><div class=\"empty\">Runtime health is not projected yet.</div><table data-live-services>{services}</table></article>
<article class=\"panel\"><header><h2>Proof/rollback status</h2></header><pre data-live-proof>plan.json proof.txt provenance.json vm-proof.txt
last run: generation built
status: {proof_status}
rollback: not evaluated
proof required: check, plan, build, proof</pre></article>
</div>
</div>
<div id=\"settings\" class=\"page\" data-page-kind=\"settings\">
<div class=\"grid\">
<article class=\"panel\"><header><h2>Settings</h2></header><div id=\"settings-controls\" class=\"form\">{settings_controls}</div></article>
<article class=\"panel\"><header><h2>Exact Jet diff</h2></header><pre id=\"tx-output\"></pre></article>
</div>
</div>
<div id=\"monitoring\" class=\"page\" data-page-kind=\"monitoring\"><div class=\"grid\"><article class=\"panel\"><header><h2>Monitoring</h2></header><table data-live-services>{services}</table></article><article class=\"panel\"><header><h2>Artifacts</h2></header><pre data-live-proof>provenance: ../provenance.json
proof: ../proof.txt
vm proof: ../vm-proof.txt</pre></article></div></div>
<div id=\"services\" class=\"page\" data-page-kind=\"services\"><article class=\"panel\"><header><h2>Services</h2></header><table data-live-services>{services}</table></article></div>
<div id=\"packages\" class=\"page\" data-page-kind=\"packages\"><article class=\"panel\"><header><h2>Packages</h2></header><table data-live-packages>{packages}</table></article></div>
<div id=\"secrets\" class=\"page\" data-page-kind=\"secrets\" data-secret-policy=\"no-plaintext\"><article class=\"panel\"><header><h2>Secrets</h2></header><pre>plaintext: never projected
runtime path: /run/jetos-secrets/*
transactions: audited rekey/add only</pre><table>{options}</table></article></div>
<div id=\"fleet\" class=\"page\" data-page-kind=\"fleet\" data-fleet-mode=\"adaptive\"><article class=\"panel\"><header><h2>Fleet</h2></header><pre>single host default: true
rollout gate: proof-before-switch
rollback on failed health window</pre></article></div>
<div id=\"generations\" class=\"page\" data-page-kind=\"generations\"><article class=\"panel\"><header><h2>Generations</h2></header><pre data-live-generations>current generation: {generation}
Rollback restores source through the same Changeset review and Apply gate.</pre><div class=\"actions\"><button data-changeset-action=\"stage-rollback\">Stage last source rollback</button><button data-run=\"generations\">List generations</button></div></article></div>
<div id=\"changeset\" class=\"page\" data-page-kind=\"changeset\" data-apply-gate=\"single-source-transaction\">
<div class=\"grid\">
<article class=\"panel\"><header><h2>Changeset</h2></header><pre id=\"changeset-diff\">No staged changes.</pre></article>
<article class=\"panel\"><header><h2>Impact ledger</h2></header><pre id=\"changeset-impact\">generation delta: current -> candidate
proof requirements: check, plan, build, proof
source transaction: config.jet only</pre>
<div class=\"actions\">
<button data-changeset-action=\"apply\">Apply to config.jet</button>
<button data-run=\"build\">Build only</button>
<button data-pipeline=\"build-switch\">Build and switch</button>
<button data-changeset-action=\"discard\">Discard</button>
</div></article>
</div>
</div>
<div id=\"proof-provenance\" class=\"page\" data-page-kind=\"proof-provenance\">
<div class=\"grid\">
<article class=\"panel\"><header><h2>Packages</h2></header><table data-live-packages>{packages}</table></article>
<article class=\"panel\"><header><h2>Services</h2></header><table data-live-services>{services}</table></article>
<article class=\"panel\"><header><h2>Options</h2></header><table data-live-options>{options}</table></article>
<article class=\"panel\"><header><h2>Source</h2></header>
<div class=\"form\">
<label>Option<input id=\"tx-key\" value=\"network.hostName\"></label>
<label>Value<input id=\"tx-value\" value=\"{host}\"></label>
</div>
<div class=\"actions\">
<button data-stage-source=\"true\">Stage edit</button>
<a href=\"/canvas\" data-open-canvas=\"source\">Open Canvas</a>
</div>
<pre id=\"source-tx-output\"></pre>
</article>
<article class=\"panel\"><header><h2>Module</h2></header><pre id=\"source-output\"></pre></article>
<article class=\"panel\"><header><h2>Proof</h2></header>
<div class=\"actions\">
<button data-run=\"check\">Check</button>
<button data-run=\"plan\">Plan</button>
<button data-run=\"build\">Build</button>
<button data-run=\"proof\">Proof</button>
<button data-run=\"generations\">List generations</button>
</div>
<pre id=\"run-output\">plan.json proof.txt provenance.json vm-proof.txt</pre>
</article>
</div>
</div>
<div class=\"changeset-tray\" data-changeset-tray=\"true\"><span id=\"changeset-summary\">0 staged changes</span><button data-open-page=\"changeset\">Review Changeset</button></div>
</section>
</main>
<script>
function showPage(id) {{
  for (const page of document.querySelectorAll('.page')) page.classList.toggle('active', page.id === id);
  for (const link of document.querySelectorAll('[data-page]')) link.classList.toggle('active', link.dataset.page === id);
}}
function wirePageLink(link) {{
  link.addEventListener('click', (event) => {{
    event.preventDefault();
    showPage(link.dataset.page);
  }});
}}
for (const link of document.querySelectorAll('[data-page]')) wirePageLink(link);
for (const button of document.querySelectorAll('[data-open-page]')) {{
  button.addEventListener('click', () => showPage(button.dataset.openPage));
}}
async function refreshSource() {{
  const res = await fetch('/studio/source');
  document.getElementById('source-output').textContent = await res.text();
}}
async function refreshProjection() {{
  const res = await fetch('/studio/data.json');
  const projection = await res.json();
  if (!res.ok) throw new Error(projection.error || 'Studio projection failed');
  installPageRegistry(projection.page_registry || [], projection);
  return projection;
}}
function installPageRegistry(registry, projection) {{
  const nav = document.querySelector('[data-page-registry=\"studio-pages\"]');
  nav.replaceChildren();
  for (const entry of registry) {{
    let page = document.getElementById(entry.id);
    if (!page) {{
      page = document.createElement('div');
      page.id = entry.id;
      page.className = 'page';
      page.dataset.pageKind = entry.id;
      const panel = document.createElement('article');
      panel.className = 'panel';
      const header = document.createElement('header');
      const title = document.createElement('h2');
      title.textContent = entry.title;
      header.append(title);
      const detail = document.createElement('div');
      detail.className = 'empty';
      detail.textContent = `Live data: ${{entry.data_needs || 'none'}}`;
      panel.append(header, detail);
      page.append(panel);
      document.querySelector('section').insertBefore(page, document.querySelector('.changeset-tray'));
    }}
    const link = document.createElement('a');
    link.href = `#${{entry.id}}`;
    link.dataset.page = entry.id;
    link.textContent = entry.title;
    wirePageLink(link);
    nav.append(link);
    const binding = resolvePageBinding(entry);
    binding.renderer(page, projection, entry);
    binding.controller(page, projection, entry);
  }}
}}
function replaceRows(selector, rows, fields) {{
  for (const table of document.querySelectorAll(selector)) {{
    table.replaceChildren();
    for (const row of rows) {{
      const tr = document.createElement('tr');
      for (const field of fields) {{
        const td = document.createElement('td');
        td.textContent = String(field(row) ?? '');
        tr.append(td);
      }}
      table.append(tr);
    }}
  }}
}}
function renderDashboard(_page, projection) {{
  const plan = projection.system_plan || projection;
  const services = plan.services || [];
  const enabled = services.filter(service => service.enable === true || service.enable === 'true').length;
  const serviceCount = document.querySelector('[data-live-count=services]');
  const packageCount = document.querySelector('[data-live-count=packages]');
  const optionCount = document.querySelector('[data-live-count=options]');
  if (serviceCount) serviceCount.textContent = `${{enabled}}/${{services.length}}`;
  if (packageCount) packageCount.textContent = String((plan.packages || []).length);
  if (optionCount) optionCount.textContent = String((plan.options || []).length);
}}
function renderSettings(_page, projection) {{
  const plan = projection.system_plan || projection;
  const controls = document.getElementById('settings-controls');
  controls.replaceChildren();
  for (const option of plan.options || []) {{
    const row = document.createElement('div');
    row.className = 'setting-control';
    const label = document.createElement('label');
    const name = document.createElement('span');
    name.textContent = option.key;
    const input = document.createElement('input');
    input.dataset.settingKey = option.key;
    input.value = option.value;
    label.append(name, input);
    const stage = document.createElement('button');
    stage.type = 'button';
    stage.textContent = 'Stage change';
    stage.dataset.stageSetting = option.key;
    stage.addEventListener('click', () => stageSetting(option.key, input.value));
    row.append(label, stage);
    controls.append(row);
  }}
}}
function renderServices(_page, projection) {{
  const plan = projection.system_plan || projection;
  replaceRows('[data-live-services]', plan.services || [], [row => row.name, row => row.enable, row => row.fields || '']);
}}
function renderPackages(_page, projection) {{
  const plan = projection.system_plan || projection;
  replaceRows('[data-live-packages]', plan.packages || [], [row => row.name, row => row.version || row.source || row.ref]);
}}
function renderOptions(_page, projection) {{
  const plan = projection.system_plan || projection;
  replaceRows('[data-live-options]', plan.options || [], [row => row.key, row => row.value]);
}}
function renderProof(_page, projection) {{
  for (const output of document.querySelectorAll('[data-live-proof]')) {{
    const state = projection.proof_state || {{ state: 'unproved', source_revision: null }};
    output.textContent = `proof: ${{state.state}}\nsource revision: ${{state.source_revision || 'none'}}`;
  }}
}}
function renderGenerations(_page, projection) {{
  const output = document.querySelector('[data-live-generations]');
  if (output) output.textContent = projection.generations || 'No generations recorded.';
}}
function renderComposite(page, projection) {{
  renderPackages(page, projection);
  renderServices(page, projection);
  renderOptions(page, projection);
  renderProof(page, projection);
}}
function renderBound(page, projection, entry) {{
  page.dataset.modelRevision = projection.proof_state?.source_revision || 'live';
  page.dataset.dataNeeds = entry.data_needs || '';
}}
const PAGE_RENDERERS = {{
  bound: renderBound,
  dashboard: (page, model, entry) => {{ renderBound(page, model, entry); renderDashboard(page, model); renderServices(page, model); renderProof(page, model); }},
  settings: (page, model, entry) => {{ renderBound(page, model, entry); renderSettings(page, model); }},
  monitoring: (page, model, entry) => {{ renderBound(page, model, entry); renderServices(page, model); renderProof(page, model); }},
  services: (page, model, entry) => {{ renderBound(page, model, entry); renderServices(page, model); }},
  packages: (page, model, entry) => {{ renderBound(page, model, entry); renderPackages(page, model); }},
  secrets: (page, model, entry) => {{ renderBound(page, model, entry); renderOptions(page, model); }},
  fleet: renderBound,
  generations: (page, model, entry) => {{ renderBound(page, model, entry); renderGenerations(page, model); }},
  changeset: renderBound,
  'proof-provenance': (page, model, entry) => {{ renderBound(page, model, entry); renderComposite(page, model); renderGenerations(page, model); }}
}};
function wireOnce(control, key, handler) {{
  const marker = `studio${{key}}Wired`;
  if (control.dataset[marker]) return;
  control.dataset[marker] = 'true';
  control.addEventListener('click', handler);
}}
function controllerReadOnly(page, _projection, entry) {{
  page.dataset.controller = 'read-only';
  page.dataset.modelContract = entry.model_contract;
}}
function controllerStudioActions(page, _projection, entry) {{
  page.dataset.controller = 'studio-actions';
  page.dataset.modelContract = entry.model_contract;
  for (const button of page.querySelectorAll('[data-stage-source]')) {{
    wireOnce(button, 'stageSource', async () => {{
      await stageSetting(document.getElementById('tx-key').value, document.getElementById('tx-value').value);
    }});
  }}
  for (const button of page.querySelectorAll('[data-stage-setting]')) {{
    wireOnce(button, 'stageSetting', async () => {{
      const input = page.querySelector('[data-setting-key=\"' + button.dataset.stageSetting + '\"]');
      await stageSetting(button.dataset.stageSetting, input ? input.value : '');
    }});
  }}
  for (const button of page.querySelectorAll('[data-changeset-action]')) {{
    wireOnce(button, 'changeset', async () => {{
      const action = button.dataset.changesetAction;
      const result = await studioPost('/studio/transaction', {{ op: action, session_id: studioSessionId, token: studioChangesetToken, base_revision: studioChangesetBaseRevision }});
      renderChangeset(result);
      if (action === 'apply' && !result.error) {{
        await refreshSource();
        await refreshProjection();
      }} else if (action === 'stage-rollback' && !result.error) {{
        showPage('changeset');
      }}
    }});
  }}
  for (const button of page.querySelectorAll('[data-run]')) {{
    wireOnce(button, 'run', async () => {{
      if (studioChangesetState === 'staged' && (button.dataset.run === 'build' || button.dataset.run === 'proof')) {{
        document.getElementById('run-output').textContent = 'Apply Changeset before building or proving this source.';
        showPage('changeset');
        return;
      }}
      await runStudioAction(button.dataset.run);
      await refreshProjection();
    }});
  }}
  for (const button of page.querySelectorAll('[data-pipeline=\"build-switch\"]')) {{
    wireOnce(button, 'pipeline', async () => {{
      if (studioChangesetState === 'staged') return;
      const build = await runStudioAction('build');
      if (!build.success) return;
      const proof = await runStudioAction('proof');
      if (!proof.success) return;
      await runStudioAction('switch');
      await refreshProjection();
    }});
  }}
}}
const PAGE_CONTROLLERS = {{
  'read-only': controllerReadOnly,
  'studio-actions': controllerStudioActions
}};
function resolvePageBinding(entry) {{
  if (!entry.model_contract) throw new Error(`Studio page ${{entry.id}} has no model contract`);
  const renderer = PAGE_RENDERERS[entry.renderer];
  if (!renderer) throw new Error(`Studio page ${{entry.id}} has no registered renderer ${{entry.renderer}}`);
  const controllerName = entry.controller || (entry.read_only === true ? 'read-only' : '');
  const controller = PAGE_CONTROLLERS[controllerName];
  if (!controller) throw new Error(`Studio page ${{entry.id}} has no registered controller`);
  if (entry.read_only !== true && controllerName === 'read-only') throw new Error(`Interactive Studio page ${{entry.id}} cannot use read-only controller`);
  return {{ renderer, controller }};
}}
const STUDIO_REGISTRY_CONTRACT_PROBE = resolvePageBinding({{
  id: 'synthetic-registered-page',
  renderer: 'bound',
  controller: 'read-only',
  read_only: true,
  model_contract: 'system_plan'
}});
async function studioPost(path, payload) {{
  const res = await fetch(path, {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify(payload) }});
  return await res.json();
}}
let studioChangesetState = 'empty';
let studioSessionId = null;
let studioChangesetToken = null;
let studioChangesetBaseRevision = null;
function renderChangeset(result) {{
  studioChangesetState = result.state || studioChangesetState;
  if (Object.prototype.hasOwnProperty.call(result, 'token')) studioChangesetToken = result.token;
  if (Object.prototype.hasOwnProperty.call(result, 'base_revision')) studioChangesetBaseRevision = result.base_revision;
  const diff = result.diff || result.error || 'No staged changes.';
  document.getElementById('tx-output').textContent = diff;
  document.getElementById('changeset-diff').textContent = diff;
  const count = result.staged_count || 0;
  document.getElementById('changeset-summary').textContent = count === 1 ? '1 staged source transaction' : `${{count}} staged source transactions`;
}}
async function runStudioAction(action) {{
  const result = await studioPost('/studio/run', {{ action }});
  document.getElementById('run-output').textContent = result.stdout || result.stderr || result.error || JSON.stringify(result, null, 2);
  return result;
}}
async function stageSetting(key, value) {{
  const result = await studioPost('/studio/transaction', {{ op: 'set-option', key, value, session_id: studioSessionId, token: studioChangesetToken, base_revision: studioChangesetBaseRevision }});
  renderChangeset(result);
  showPage('changeset');
  return result;
}}
studioPost('/studio/transaction', {{ op: 'session' }}).then(session => {{
  studioSessionId = session.session_id;
  return Promise.all([
    refreshSource(),
    refreshProjection(),
    studioPost('/studio/transaction', {{ op: 'status', session_id: studioSessionId }}).then(renderChangeset)
  ]);
}});
</script>
</body>
</html>
",
        host = html_escape(&system.name),
        target = html_escape(&system.target),
        generation = html_escape(generation),
        nav = nav,
        enabled_services = enabled_services,
        service_total = system.services.len(),
        package_total = system.packages.len(),
        option_total = system.options.len(),
        proof_status = html_escape(proof_status),
        settings_controls = if settings_controls.is_empty() {
            "<div class=\"empty\">No settings projected.</div>".to_string()
        } else {
            settings_controls
        },
        packages = if packages.is_empty() {
            "<tr><td>none</td><td></td></tr>".to_string()
        } else {
            packages
        },
        services = if services.is_empty() {
            "<tr><td>none</td><td></td><td></td></tr>".to_string()
        } else {
            services
        },
        options = if options.is_empty() {
            "<tr><td>none</td><td></td></tr>".to_string()
        } else {
            options
        },
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(unix)]
pub(super) fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub(super) fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

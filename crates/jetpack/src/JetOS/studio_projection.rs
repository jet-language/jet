fn write_studio_app_projection(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let studio_dir = dir.join("studio");
    let bin_dir = dir.join("sw/bin");
    let desktop_dir = dir.join("share/applications");
    fs::create_dir_all(&studio_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&desktop_dir)?;

    let app = JSON::object_of(&[
        ("kind", "jetos-studio-app"),
        ("host", &system.name),
        ("runtime", "jetos-system-app"),
        ("protocol", "local-projection-service"),
        ("source_truth", "jet-source-transactions"),
        ("semantic_state", "none"),
        ("browser_fallback", "true"),
        ("canvas_coupled", "false"),
    ]);
    fs::write(studio_dir.join("app.json"), app)?;
    fs::write(studio_dir.join("data.json"), studio_data_json(system))?;
    fs::write(studio_dir.join("index.html"), studio_index_html(system))?;

    let launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_STUDIO_ROOT:-/run/current-system}\npage=\"$root/studio/index.html\"\nif command -v jetos >/dev/null 2>&1; then\n  exec jetos studio \"$@\"\nfi\nif command -v xdg-open >/dev/null 2>&1; then\n  exec xdg-open \"$page\"\nfi\nprintf '%s\\n' \"$page\"\n";
    let launcher_path = bin_dir.join("jetos-studio");
    fs::write(&launcher_path, launcher)?;
    make_executable(&launcher_path)?;

    fs::write(
        desktop_dir.join("jetos-studio.desktop"),
        "[Desktop Entry]\nName=jetos Studio\nComment=Edit jetos system source\nExec=/run/current-system/sw/bin/jetos-studio\nType=Application\nCategories=System;Settings;\n",
    )
}

fn studio_data_json(system: &SystemPlan) -> String {
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
    let disabled_services = system.services.len().saturating_sub(enabled_services);
    let options = system
        .options
        .iter()
        .map(|opt| JSON::object_of(&[("key", &opt.key), ("value", &opt.value)]))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"jetos-studio-projection\",\"host\":{},\"target\":{},\"dashboard\":{{\"selected_host\":{},\"generation\":\"current\",\"service_health\":\"{enabled}/{total} enabled\",\"alerts\":{},\"last_run\":\"ready\",\"rollback_status\":\"available\"}},\"page_registry\":[{}],\"packages\":[{}],\"services\":[{}],\"options\":[{}],\"changeset\":{{\"state\":\"empty\",\"apply_gate\":\"single-source-transaction\",\"proof_required\":[\"check\",\"plan\",\"build\",\"proof\"]}},\"secret_policy\":{{\"plaintext_in_projection\":false,\"visible_fields\":[\"name\",\"source\",\"runtime_path\",\"provenance\"],\"transaction\":\"audited-secret-rekey\"}},\"fleet\":{{\"mode\":\"adaptive\",\"single_host_default\":true,\"rollout_gate\":\"proof-before-switch\"}},\"canvas_bridge\":{{\"mode\":\"separate-app-deeplink\",\"target\":\"/canvas\",\"shared_state\":\"source-only\"}},\"artifacts\":{{\"plan\":\"../plan.json\",\"proof\":\"../proof.txt\",\"provenance\":\"../provenance.json\",\"vm_proof\":\"../vm-proof.txt\"}}}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        JSON::quote(&system.name),
        if disabled_services == 0 {
            "[]"
        } else {
            "[\"disabled-services\"]"
        },
        pages,
        packages,
        services,
        options,
        enabled = enabled_services,
        total = system.services.len()
    )
}

fn studio_pages_json() -> String {
    let pages = [
        ("dashboard", "Dashboard", "operations", "dashboard,services,artifacts"),
        ("settings", "Settings", "settings", "options,source"),
        ("monitoring", "Monitoring", "operations", "services,artifacts"),
        ("services", "Services", "operations", "services"),
        ("packages", "Packages", "inventory", "packages"),
        ("secrets", "Secrets", "settings", "options"),
        ("fleet", "Fleet", "operations", "fleet,proof"),
        ("generations", "Generations", "operations", "artifacts"),
        ("changeset", "Changeset", "review", "source,diff,impact"),
        (
            "proof-provenance",
            "Proof/Provenance",
            "audit",
            "artifacts,run",
        ),
    ];
    pages
        .iter()
        .map(|(id, title, group, needs)| {
            JSON::object_of(&[
                ("id", id),
                ("title", title),
                ("group", group),
                ("data_needs", needs),
            ])
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn studio_index_html(system: &SystemPlan) -> String {
    let enabled_services = system.services.iter().filter(|svc| svc.enable).count();
    let disabled_services = system.services.len().saturating_sub(enabled_services);
    let alerts = if disabled_services == 0 {
        "No active alerts".to_string()
    } else {
        format!("{disabled_services} disabled service review")
    };
    let nav = [
        ("dashboard", "Dashboard"),
        ("settings", "Settings"),
        ("monitoring", "Monitoring"),
        ("services", "Services"),
        ("packages", "Packages"),
        ("secrets", "Secrets"),
        ("fleet", "Fleet"),
        ("generations", "Generations"),
        ("changeset", "Changeset"),
        ("proof-provenance", "Proof/Provenance"),
    ]
    .iter()
    .map(|(id, title)| {
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
                "<label><span>{}</span><input data-setting-key=\"{}\" value=\"{}\"></label>",
                html_escape(&opt.key),
                html_escape(&opt.key),
                html_escape(&opt.value)
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
:root {{ color-scheme: light dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #f5f7fa; color: #16202a; }}
body {{ margin: 0; min-height: 100vh; background: #f5f7fa; }}
main {{ display: grid; grid-template-columns: 248px 1fr; min-height: 100vh; }}
aside {{ border-right: 1px solid #d8dee8; padding: 20px 16px; background: #ffffff; }}
section {{ padding: 24px; }}
h1, h2 {{ margin: 0; font-weight: 650; }}
h1 {{ font-size: 22px; }}
h2 {{ font-size: 15px; margin-bottom: 12px; }}
.nav {{ display: grid; gap: 4px; margin-top: 24px; }}
.nav a {{ color: #44515f; text-decoration: none; border-radius: 6px; padding: 8px 10px; font-size: 14px; }}
.nav a:hover, .nav a.active {{ background: #e8eef5; color: #15202b; }}
.host {{ display: grid; gap: 6px; margin-top: 20px; }}
.pill {{ border: 1px solid #cbd5e1; border-radius: 999px; padding: 6px 10px; width: max-content; background: #ffffff; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; }}
.panel {{ border: 1px solid #d8dee8; border-radius: 8px; background: #ffffff; overflow: hidden; }}
.panel header {{ padding: 14px 16px; border-bottom: 1px solid #d8dee8; }}
.hero {{ display: grid; gap: 14px; margin-bottom: 18px; }}
.metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 10px; }}
.metric {{ border: 1px solid #d8dee8; border-radius: 8px; background: #ffffff; padding: 14px; }}
.metric strong {{ display: block; font-size: 22px; margin-top: 4px; }}
.page {{ display: none; }}
.page.active {{ display: block; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
td {{ padding: 10px 16px; border-top: 1px solid #e7ebf0; vertical-align: top; }}
td:first-child {{ color: #245d7a; font-weight: 600; white-space: nowrap; }}
.status {{ display: flex; flex-wrap: wrap; gap: 8px; margin: 18px 0 24px; }}
.empty {{ color: #647383; padding: 16px; }}
.form {{ display: grid; grid-template-columns: 1fr 1fr; gap: 10px; padding: 16px; }}
label {{ display: grid; gap: 6px; font-size: 12px; color: #647383; }}
input {{ min-width: 0; border: 1px solid #cbd5e1; border-radius: 6px; padding: 9px 10px; background: #ffffff; color: #16202a; }}
.actions {{ display: flex; flex-wrap: wrap; gap: 8px; padding: 0 16px 16px; }}
button {{ border: 1px solid #b8c4d1; border-radius: 6px; padding: 8px 11px; background: #eef3f8; color: #16202a; cursor: pointer; }}
button:hover {{ border-color: #245d7a; }}
pre {{ margin: 0; padding: 16px; min-height: 96px; max-height: 300px; overflow: auto; border-top: 1px solid #d8dee8; color: #22313f; background: #f8fafc; font-size: 12px; }}
.changeset-tray {{ position: sticky; bottom: 0; border-top: 1px solid #cbd5e1; background: #ffffff; padding: 12px 16px; display: flex; gap: 10px; align-items: center; justify-content: space-between; }}
@media (max-width: 720px) {{ main {{ grid-template-columns: 1fr; }} aside {{ border-right: 0; border-bottom: 1px solid #d8dee8; }} .form {{ grid-template-columns: 1fr; }} }}
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
<div class=\"status\">
<span class=\"pill\">selected host: {host}</span>
<span class=\"pill\">generation: current</span>
<span class=\"pill\">rollback: available</span>
</div>
<div class=\"metrics\">
<div class=\"metric\">Services<strong>{enabled_services}/{service_total}</strong></div>
<div class=\"metric\">Packages<strong>{package_total}</strong></div>
<div class=\"metric\">Options<strong>{option_total}</strong></div>
<div class=\"metric\">Alerts<strong>{alerts}</strong></div>
</div>
</div>
<div class=\"grid\">
<article class=\"panel\"><header><h2>Service Health</h2></header><table>{services}</table></article>
<article class=\"panel\"><header><h2>Proof/rollback status</h2></header><pre>plan.json proof.txt provenance.json vm-proof.txt
last run: ready
proof required: check, plan, build, proof</pre></article>
</div>
</div>
<div id=\"settings\" class=\"page\" data-page-kind=\"settings\">
<div class=\"grid\">
<article class=\"panel\"><header><h2>Settings</h2></header><div class=\"form\">{settings_controls}</div>
<div class=\"actions\"><button data-stage-setting=\"network.hostName\">Stage host name</button></div></article>
<article class=\"panel\"><header><h2>Exact Jet diff</h2></header><pre id=\"tx-output\"></pre></article>
</div>
</div>
<div id=\"monitoring\" class=\"page\" data-page-kind=\"monitoring\"><div class=\"grid\"><article class=\"panel\"><header><h2>Monitoring</h2></header><table>{services}</table></article><article class=\"panel\"><header><h2>Artifacts</h2></header><pre>provenance: ../provenance.json
proof: ../proof.txt
vm proof: ../vm-proof.txt</pre></article></div></div>
<div id=\"services\" class=\"page\" data-page-kind=\"services\"><article class=\"panel\"><header><h2>Services</h2></header><table>{services}</table></article></div>
<div id=\"packages\" class=\"page\" data-page-kind=\"packages\"><article class=\"panel\"><header><h2>Packages</h2></header><table>{packages}</table></article></div>
<div id=\"secrets\" class=\"page\" data-page-kind=\"secrets\" data-secret-policy=\"no-plaintext\"><article class=\"panel\"><header><h2>Secrets</h2></header><pre>plaintext: never projected
runtime path: /run/jetos-secrets/*
transactions: audited rekey/add only</pre><table>{options}</table></article></div>
<div id=\"fleet\" class=\"page\" data-page-kind=\"fleet\" data-fleet-mode=\"adaptive\"><article class=\"panel\"><header><h2>Fleet</h2></header><pre>single host default: true
rollout gate: proof-before-switch
rollback on failed health window</pre></article></div>
<div id=\"generations\" class=\"page\" data-page-kind=\"generations\"><article class=\"panel\"><header><h2>Generations</h2></header><pre>current generation: {host}
rollback action stages an inverse changeset through the same apply gate.</pre></article></div>
<div id=\"changeset\" class=\"page\" data-page-kind=\"changeset\" data-apply-gate=\"single-source-transaction\">
<div class=\"grid\">
<article class=\"panel\"><header><h2>Changeset</h2></header><pre id=\"changeset-diff\">No staged changes.</pre></article>
<article class=\"panel\"><header><h2>Impact ledger</h2></header><pre id=\"changeset-impact\">generation delta: current -> candidate
proof requirements: check, plan, build, proof
source transaction: config.jet only</pre>
<div class=\"actions\">
<button data-run=\"build\">Build only</button>
<button data-run=\"proof\">Build and proof</button>
<button data-discard=\"changeset\">Discard</button>
</div></article>
</div>
</div>
<div id=\"proof-provenance\" class=\"page\" data-page-kind=\"proof-provenance\">
<div class=\"grid\">
<article class=\"panel\"><header><h2>Packages</h2></header><table>{packages}</table></article>
<article class=\"panel\"><header><h2>Services</h2></header><table>{services}</table></article>
<article class=\"panel\"><header><h2>Options</h2></header><table>{options}</table></article>
<article class=\"panel\"><header><h2>Source</h2></header>
<div class=\"form\">
<label>Option<input id=\"tx-key\" value=\"network.hostName\"></label>
<label>Value<input id=\"tx-value\" value=\"{host}\"></label>
</div>
<div class=\"actions\">
<button data-tx=\"preview\">Preview</button>
<button data-tx=\"write\">Save</button>
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
<button data-run=\"generations\">Rollback</button>
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
for (const link of document.querySelectorAll('[data-page]')) {{
  link.addEventListener('click', (event) => {{
    event.preventDefault();
    showPage(link.dataset.page);
  }});
}}
for (const button of document.querySelectorAll('[data-open-page]')) {{
  button.addEventListener('click', () => showPage(button.dataset.openPage));
}}
async function refreshSource() {{
  const res = await fetch('/studio/source');
  document.getElementById('source-output').textContent = await res.text();
}}
async function studioPost(path, payload) {{
  const res = await fetch(path, {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify(payload) }});
  return await res.json();
}}
for (const button of document.querySelectorAll('[data-tx]')) {{
  button.addEventListener('click', async () => {{
    const write = button.dataset.tx === 'write';
    const result = await studioPost('/studio/transaction', {{
      op: 'set-option',
      key: document.getElementById('tx-key').value,
      value: document.getElementById('tx-value').value,
      write
    }});
    document.getElementById('tx-output').textContent = result.diff || result.error || JSON.stringify(result, null, 2);
    document.getElementById('changeset-diff').textContent = result.diff || 'No staged changes.';
    document.getElementById('changeset-summary').textContent = result.changed ? '1 staged source transaction' : '0 staged changes';
    if (write && !result.error) await refreshSource();
  }});
}}
for (const button of document.querySelectorAll('[data-stage-setting]')) {{
  button.addEventListener('click', async () => {{
    const input = document.querySelector('[data-setting-key=\"' + button.dataset.stageSetting + '\"]');
    const result = await studioPost('/studio/transaction', {{
      op: 'set-option',
      key: button.dataset.stageSetting,
      value: input ? input.value : '',
      write: false
    }});
    document.getElementById('tx-output').textContent = result.diff || result.error || JSON.stringify(result, null, 2);
    document.getElementById('changeset-diff').textContent = result.diff || 'No staged changes.';
    document.getElementById('changeset-summary').textContent = result.changed ? '1 staged source transaction' : '0 staged changes';
    showPage('changeset');
  }});
}}
for (const button of document.querySelectorAll('[data-run]')) {{
  button.addEventListener('click', async () => {{
    const result = await studioPost('/studio/run', {{ action: button.dataset.run }});
    document.getElementById('run-output').textContent = result.stdout || result.stderr || result.error || JSON.stringify(result, null, 2);
  }});
}}
refreshSource();
</script>
</body>
</html>
",
        host = html_escape(&system.name),
        target = html_escape(&system.target),
        nav = nav,
        enabled_services = enabled_services,
        service_total = system.services.len(),
        package_total = system.packages.len(),
        option_total = system.options.len(),
        alerts = html_escape(&alerts),
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
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

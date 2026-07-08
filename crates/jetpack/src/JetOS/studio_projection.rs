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
    let options = system
        .options
        .iter()
        .map(|opt| JSON::object_of(&[("key", &opt.key), ("value", &opt.value)]))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"jetos-studio-projection\",\"host\":{},\"target\":{},\"packages\":[{}],\"services\":[{}],\"options\":[{}],\"artifacts\":{{\"plan\":\"../plan.json\",\"proof\":\"../proof.txt\",\"provenance\":\"../provenance.json\",\"vm_proof\":\"../vm-proof.txt\"}}}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        packages,
        services,
        options
    )
}

fn studio_index_html(system: &SystemPlan) -> String {
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
    format!(
        "<!doctype html>
<html>
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>jetos Studio - {host}</title>
<style>
:root {{ color-scheme: light dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #101418; color: #edf2f7; }}
body {{ margin: 0; min-height: 100vh; background: #101418; }}
main {{ display: grid; grid-template-columns: 260px 1fr; min-height: 100vh; }}
aside {{ border-right: 1px solid #2d3742; padding: 20px; background: #151b21; }}
section {{ padding: 24px; }}
h1, h2 {{ margin: 0; font-weight: 650; }}
h1 {{ font-size: 22px; }}
h2 {{ font-size: 15px; margin-bottom: 12px; }}
.host {{ display: grid; gap: 6px; margin-top: 20px; }}
.pill {{ border: 1px solid #3b4652; border-radius: 999px; padding: 6px 10px; width: max-content; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; }}
.panel {{ border: 1px solid #2d3742; border-radius: 8px; background: #151b21; overflow: hidden; }}
.panel header {{ padding: 14px 16px; border-bottom: 1px solid #2d3742; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
td {{ padding: 10px 16px; border-top: 1px solid #202832; vertical-align: top; }}
td:first-child {{ color: #9ccfd8; font-weight: 600; white-space: nowrap; }}
.status {{ display: flex; flex-wrap: wrap; gap: 8px; margin: 18px 0 24px; }}
.empty {{ color: #9aa7b2; padding: 16px; }}
.form {{ display: grid; grid-template-columns: 1fr 1fr; gap: 10px; padding: 16px; }}
label {{ display: grid; gap: 6px; font-size: 12px; color: #9aa7b2; }}
input {{ min-width: 0; border: 1px solid #3b4652; border-radius: 6px; padding: 9px 10px; background: #101418; color: #edf2f7; }}
.actions {{ display: flex; flex-wrap: wrap; gap: 8px; padding: 0 16px 16px; }}
button {{ border: 1px solid #3b4652; border-radius: 6px; padding: 8px 11px; background: #1d2730; color: #edf2f7; cursor: pointer; }}
button:hover {{ border-color: #9ccfd8; }}
pre {{ margin: 0; padding: 16px; min-height: 96px; max-height: 300px; overflow: auto; border-top: 1px solid #2d3742; color: #c9d1d9; background: #0c1014; font-size: 12px; }}
@media (max-width: 720px) {{ main {{ grid-template-columns: 1fr; }} aside {{ border-right: 0; border-bottom: 1px solid #2d3742; }} }}
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
</aside>
<section>
<div class=\"status\">
<span class=\"pill\">Source</span>
<span class=\"pill\">Proof</span>
<span class=\"pill\">Local</span>
</div>
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
</div>
<pre id=\"tx-output\"></pre>
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
</section>
</main>
<script>
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
    if (write && !result.error) await refreshSource();
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

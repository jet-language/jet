use jet_foundation::JSON::json_escape;

pub fn canvas_html() -> String {
    canvas_html_for("/canvas")
}

pub fn canvas_html_for(base: &str) -> String {
    canvas_html_document(&format!(
        r#"<script>window.__JET_CANVAS_BASE__ = "{}";</script>
<script src="{}/app.js?canvas_ui=blueprint23"></script>"#,
        json_escape(base),
        json_escape(base)
    ))
}

pub fn canvas_html_query() -> String {
    canvas_html_document(
        r#"<script>window.__JET_CANVAS_BASE__ = ""; window.__JET_CANVAS_GRAPH__ = "/?jet_panel_graph=1"; window.__JET_CANVAS_TX__ = "/canvas/transaction"; window.__JET_CANVAS_QUERY__ = "/canvas/query"; window.__JET_CANVAS_SCM__ = "/canvas/source-control"; window.__JET_CANVAS_PROOF__ = "/canvas/proof"; window.__JET_CANVAS_COMMAND__ = "/canvas/command";</script>
<script src="/?jet_panel_app=1&canvas_ui=blueprint23"></script>"#,
    )
}

fn canvas_html_document(bootstrap: &str) -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'%3E%3Crect width='16' height='16' rx='3' fill='%230b1017'/%3E%3Cpath d='M3 2h10v3H6v2h6v3H6v4H3z' fill='%2335c2ff'/%3E%3C/svg%3E">
<title>Jet Canvas</title>
<style>
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; overflow: hidden; background: #101318; color: #d7e4f7; font: 13px "Inter", "Segoe UI", Roboto, system-ui, sans-serif; }
body:not(.is-dev-mode) .dev-only { display: none !important; }
button, input, select { font: inherit; }
button { color: #d7e4f7; border: 1px solid #31445d; background: linear-gradient(#18202b, #111821); min-height: 30px; padding: 0 10px; cursor: pointer; border-radius: 4px; box-shadow: inset 0 1px 0 rgba(255,255,255,.04), 0 1px 0 rgba(0,0,0,.4); white-space: nowrap; }
button:hover, button:focus-visible { border-color: #35c2ff; background: #1d2b3a; outline: none; box-shadow: 0 0 0 2px rgba(53,194,255,.18); }
button.primary { background: #123d45; border-color: #22d3ee; color: #e7fbff; }
button.is-active { border-color: #f6d365; background: #352c17; color: #fff4bd; }
button:disabled { opacity: .42; cursor: default; box-shadow: none; }
input, select { color: #e7eefb; border: 1px solid #31445d; background: #0b1118; min-height: 30px; padding: 0 8px; border-radius: 4px; }
input:focus-visible, select:focus-visible { border-color: #35c2ff; outline: none; box-shadow: 0 0 0 2px rgba(53,194,255,.18); }
select { min-width: 180px; }
#shell { height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr) 28px; min-width: 0; }
#topbar { display: flex; align-items: center; gap: 8px; flex-wrap: nowrap; min-width: 0; min-height: 48px; padding: 8px 10px; border-bottom: 1px solid #25364b; background: linear-gradient(#111923, #0c1119 62%, #080c12); box-shadow: 0 1px 0 rgba(255,255,255,.04) inset, 0 14px 40px rgba(0,0,0,.25); }
#brand { display: flex; flex-direction: column; gap: 1px; flex: 0 1 164px; min-width: 124px; padding-left: 8px; border-left: 3px solid #35c2ff; }
#brand strong { font-size: 14px; letter-spacing: .08em; text-transform: uppercase; color: #f8fbff; }
#brand span { color: #9db4d2; font-size: 11px; }
.toolbar-group { display: flex; align-items: center; gap: 5px; padding: 0 7px; border-left: 1px solid #25364b; min-width: 0; flex: 0 0 auto; }
.toolbar-group:first-of-type { border-left: 0; }
.toolbar-spacer { flex: 1 1 auto; min-width: 10px; }
.icon-button { width: 32px; min-width: 32px; padding: 0; display: inline-grid; place-items: center; }
.icon-button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.toolbar-menu { position: relative; flex: 0 0 auto; }
.toolbar-menu summary { list-style: none; }
.toolbar-menu summary::-webkit-details-marker { display: none; }
.toolbar-menu summary.icon-button { color: #d7e4f7; border: 1px solid #31445d; background: linear-gradient(#18202b, #111821); min-height: 30px; cursor: pointer; border-radius: 4px; box-shadow: inset 0 1px 0 rgba(255,255,255,.04), 0 1px 0 rgba(0,0,0,.4); }
.toolbar-menu summary.icon-button:hover, .toolbar-menu summary.icon-button:focus-visible { border-color: #35c2ff; background: #1d2b3a; outline: none; box-shadow: 0 0 0 2px rgba(53,194,255,.18); }
.toolbar-menu[open] summary { border-color: #35c2ff; background: #1d2b3a; }
.toolbar-popover { position: absolute; right: 0; top: calc(100% + 6px); z-index: 40; min-width: 220px; display: grid; gap: 6px; padding: 8px; border: 1px solid #344b68; background: #0b1118; border-radius: 6px; box-shadow: 0 18px 48px rgba(0,0,0,.55); }
.toolbar-popover button { width: 100%; justify-content: start; text-align: left; }
.toolbar-popover .detail-toggles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); }
.zoom-readout { min-width: 44px; color: #9db4d2; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; text-align: center; }
#graph-select { display: none; border-color: #4b6685; background: #0c1420; color: #eaf5ff; }
body.is-dev-mode #graph-select { display: block; }
#topbar > select { flex: 1 1 190px; min-width: 140px; max-width: 360px; }
#topbar > button { flex: 0 0 auto; padding-inline: 8px; }
.lens-switch { display: flex; gap: 3px; padding: 3px; border: 1px solid #2c4058; background: #080d13; border-radius: 6px; }
.lens-switch button { min-height: 26px; padding-inline: 9px; border-radius: 4px; background: transparent; border-color: transparent; box-shadow: none; }
.lens-switch button.is-active { border-color: #d4d4d8; background: #242931; color: #fafafa; }
.detail-toggles { display: flex; align-items: center; gap: 5px; flex-wrap: wrap; padding: 3px 5px; border: 1px solid #25364b; background: #080d13; border-radius: 6px; }
.detail-toggle { display: flex; align-items: center; gap: 4px; color: #9db4d2; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; text-transform: uppercase; }
.detail-toggle input { width: 13px; height: 13px; min-height: 0; accent-color: #d4d4d8; }
.type-detail, .diagnostic-detail, .effect-detail, .debug-detail, .package-detail { display: none !important; }
body.detail-types .type-detail, body.detail-diagnostics .diagnostic-detail, body.detail-effects .effect-detail, body.detail-debug .debug-detail, body.detail-package .package-detail { display: revert !important; }
body.detail-package .project-section.package-detail, body.detail-diagnostics .project-section.diagnostic-detail { display: grid !important; }
#jump { flex: 1 1 150px; min-width: 94px; color: #9db4d2; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
body:not(.is-dev-mode) #jump { display: none; }
.debug-controls { display: none; align-items: center; gap: 5px; min-width: 0; justify-content: flex-end; flex-wrap: wrap; }
body.is-debug-active .debug-controls { display: flex; }
body:not(.is-debug-active) #debug-menu[open] .debug-controls { display: grid; }
body:not(.is-debug-active) .debug-controls > :not(#debug-start) { display: none; }
.debug-controls select { flex: 1 1 130px; min-width: 112px; max-width: 220px; }
.debug-controls button { min-width: 30px; padding: 0 7px; }
#workbench { min-height: 0; min-width: 0; position: relative; display: grid; grid-template-columns: minmax(156px, 15vw) minmax(0, 1fr) minmax(238px, 20vw); grid-template-rows: auto minmax(0, 1fr); background: #05070b; }
#workbench-header { grid-column: 1 / -1; min-width: 0; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 14px; padding: 7px 12px; border-bottom: 1px solid #23344a; background: linear-gradient(180deg, #101a27, #0b121c); box-shadow: inset 0 1px 0 rgba(255,255,255,.03); }
.workbench-heading { display: grid; gap: 1px; min-width: 142px; }
.workbench-heading strong { color: #f8fbff; font-size: 13px; letter-spacing: .04em; }
.workbench-heading span { color: #7895b8; font: 9px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .1em; text-transform: uppercase; }
.workbench-map { min-width: 0; display: flex; align-items: center; gap: 6px; overflow: auto hidden; scrollbar-width: thin; }
.workbench-map span, .workbench-session { display: flex; align-items: baseline; gap: 6px; min-width: max-content; padding: 5px 8px; border: 1px solid #29415d; border-radius: 4px; background: rgba(7,14,23,.72); }
.workbench-map b, .workbench-session > b { color: #8fb2dc; font: 9px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .1em; text-transform: uppercase; }
.workbench-map span span { color: #d7e4f7; font-size: 11px; letter-spacing: 0; text-transform: none; }
.workbench-session { justify-content: end; gap: 8px; border-color: #365a7f; }
#workbench-header #session-identity { max-width: 220px; overflow: hidden; color: #cfe9ff; text-overflow: ellipsis; white-space: nowrap; }
#workbench-header #run-hud { position: static; top: auto; right: auto; z-index: auto; display: block; min-width: 132px; margin: 0; padding: 0; border: 0; background: transparent; color: #9fb9d8; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; white-space: nowrap; }
#workbench-header #run-hud.is-running { display: block; color: #fef3c7; }
.side { min-width: 0; overflow: hidden auto; background: #0b1017; border-right: 1px solid #23344a; box-shadow: inset -1px 0 0 rgba(255,255,255,.03); }
.right { border-right: 0; border-left: 1px solid #23344a; box-shadow: inset 1px 0 0 rgba(255,255,255,.03); display: grid; grid-template-rows: auto minmax(0, 1fr) auto auto; overflow: hidden; }
#right-drawer #details { min-height: 0; overflow: auto; }
#right-drawer #preview-panel { min-height: 0; }
.panel { border-bottom: 1px solid #23344a; padding: clamp(9px, 1.2vw, 13px); }
.panel h2 { margin: 0 0 10px; color: #eaf5ff; font-size: 11px; letter-spacing: .12em; text-transform: uppercase; }
.component-tree-head { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; margin-bottom: 10px; }
.component-tree-head h2 { margin: 0; }
.component-tree-section { display: grid; gap: 7px; padding-top: 9px; border-top: 1px solid #22364d; }
.component-tree-section + .component-tree-section { margin-top: 9px; }
.component-tree-section details { gap: 7px; }
.component-tree-section summary { min-height: 27px; }
.component-tree-section summary > .component-tree-section-actions { display: inline-flex; align-items: center; justify-content: end; gap: 6px; }
.component-tree-section-actions button { min-height: 22px; padding: 2px 7px; border-radius: 4px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .04em; }
.component-tree-section [role="treeitem"] { position: relative; }
.component-tree-section [role="treeitem"]:focus-visible { outline: 2px solid #35c2ff; outline-offset: 1px; }
.panel details { display: grid; gap: 8px; }
.panel summary { list-style: none; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 30px; color: #eaf5ff; font-size: 11px; letter-spacing: .12em; text-transform: uppercase; cursor: pointer; }
.panel summary::-webkit-details-marker { display: none; }
.panel summary::before { content: "▸"; color: #35c2ff; font-size: 12px; transform: rotate(0deg); transition: transform .12s ease; }
.panel details[open] summary::before { transform: rotate(90deg); }
.panel summary .count { justify-self: end; }
.panel details > :not(summary) { margin-top: 8px; }
.graph-list, .search-results, .project-list, .variable-list { display: grid; gap: 6px; }
#project-panel details > #project-rail { max-height: min(26vh, 250px); overflow: auto; padding-right: 3px; scrollbar-width: thin; }
#output-panel details > #output-list { max-height: min(18vh, 170px); overflow: auto; padding-right: 3px; scrollbar-width: thin; }
.project-section { display: grid; gap: 7px; margin-top: 10px; padding-top: 10px; border-top: 1px solid #22364d; }
.project-section h3 { margin: 0; color: #8fb2dc; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .09em; text-transform: uppercase; }
.library-panel { gap: 8px; }
.library-search { width: 100%; }
.library-list { display: grid; gap: 7px; }
.library-module { display: grid; gap: 7px; padding: 7px; border: 1px solid #29415d; background: #0c1724; border-radius: 5px; }
.library-module summary { min-height: 25px; }
.library-module-body { display: grid; gap: 6px; }
.library-module-summary { margin: 0; color: #91a8c5; font-size: 10px; line-height: 1.4; }
.library-entry-row { display: grid; gap: 3px; }
.library-entry { width: 100%; display: grid; gap: 3px; min-width: 0; padding: 7px 8px; text-align: left; border-color: #2d4a68; background: #101d2b; }
.library-entry:hover, .library-entry:focus-visible { border-color: #35c2ff; background: #142b40; }
.library-entry:disabled { opacity: .58; cursor: not-allowed; }
.library-entry-name { overflow: hidden; color: #eaf5ff; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
.library-entry-signature { overflow: hidden; color: #9fc1e5; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }
.library-entry-meta { display: flex; gap: 6px; flex-wrap: wrap; color: #7895b8; font: 9px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.library-entry-meta small { color: #7895b8; overflow-wrap: anywhere; }
.library-entry-state { color: #65d4ff; text-transform: uppercase; letter-spacing: .06em; }
.library-entry-reason { color: #f6c177 !important; }
.library-packages { display: grid; gap: 6px; padding-top: 7px; border-top: 1px solid #22364d; }
.library-packages h4 { margin: 0; color: #8fb2dc; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .09em; text-transform: uppercase; }
.library-package { display: grid; gap: 2px; padding: 6px 7px; border: 1px solid #263b54; background: #0f1a27; border-radius: 4px; }
.library-package b { color: #eaf5ff; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.library-package small { color: #87a4c8; overflow-wrap: anywhere; }
.status-grid { display: grid; gap: 6px; }
.status-card { border: 1px solid #263850; border-radius: 6px; background: #0d1520; padding: 8px; display: grid; gap: 4px; }
.status-card b { color: #eef7ff; font-size: 12px; }
.status-card small { color: #8fa7c6; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow-wrap: anywhere; }
.proof-rail { display: grid; gap: 6px; }
.proof-row { display: grid; grid-template-columns: 96px minmax(0, 1fr); gap: 8px; padding: 7px 8px; border: 1px solid #21344b; background: rgba(7,13,22,.72); border-radius: 4px; }
.proof-row b { color: #8fb2dc; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .08em; text-transform: uppercase; }
.proof-row span { min-width: 0; overflow-wrap: anywhere; }
.proof-row.is-missing span { color: #f8c76a; }
.graph-item { width: 100%; display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 8px; text-align: left; border-color: #283b52; background: #101821; min-height: 38px; }
.graph-item.is-active { border-color: #35c2ff; background: #102437; box-shadow: inset 3px 0 0 #35c2ff; }
.variable-item { width: 100%; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 8px; text-align: left; border-color: #283b52; background: #101821; min-height: 34px; }
.variable-item.is-active { border-color: #35c2ff; background: #102437; box-shadow: inset 3px 0 0 #35c2ff; }
.variable-dot { width: 10px; height: 10px; border-radius: 50%; background: currentColor; box-shadow: 0 0 12px currentColor; }
.variable-name { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.project-card { border: 1px solid #263850; border-radius: 6px; background: #0d1520; padding: 8px; display: grid; gap: 5px; }
button.project-card { width: 100%; text-align: left; cursor: pointer; color: inherit; }
.project-card.is-active { border-color: #35c2ff; background: #102437; box-shadow: inset 3px 0 0 #35c2ff; }
.project-card b { color: #eef7ff; font-size: 12px; overflow-wrap: anywhere; }
.project-card small, .project-card code { color: #8fa7c6; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow-wrap: anywhere; }
.project-card .tag { justify-self: start; }
.search-item { width: 100%; text-align: left; border-color: #34285e; background: #151229; }
.search-item.is-active { border-color: #d58cff; background: #21133a; }
.search-item small { color: #9aa8c5; display: block; margin-top: 3px; overflow-wrap: anywhere; }
.count, .tag { color: #8fb2dc; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.search { width: 100%; margin-bottom: 8px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
#stage { position: relative; min-width: 0; min-height: 0; overflow: hidden; background: #101318; }
#jet-canvas-view { width: 100%; height: 100%; display: block; background: #101318; }
#canvas-dock { position: absolute; left: 10px; top: 10px; z-index: 26; display: none; gap: 6px; padding: 5px; border: 1px solid #365a7f; background: rgba(8,17,29,.92); box-shadow: 0 12px 34px rgba(0,0,0,.42); border-radius: 6px; }
#canvas-dock button { min-height: 28px; padding: 0 8px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#canvas-dock button.is-active { border-color: #35c2ff; color: #e7fbff; background: #102437; }
#graph-strip { position: absolute; left: 12px; right: 12px; top: 10px; z-index: 13; display: flex; gap: 8px; overflow-x: auto; scrollbar-width: thin; padding: 6px; border: 1px solid rgba(53,194,255,.45); background: linear-gradient(180deg, rgba(9,19,32,.94), rgba(5,11,19,.82)); box-shadow: 0 16px 44px rgba(0,0,0,.42), inset 0 1px 0 rgba(255,255,255,.05); border-radius: 6px; max-width: min(860px, calc(100% - 24px)); }
body:not(.is-dev-mode) #graph-strip { display: none; }
body:not(.is-dev-mode) #wire-status { display: none; }
.graph-tab { position: relative; flex: 0 0 auto; min-width: 116px; min-height: 36px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 7px; align-items: center; border-color: #2e4966; background: linear-gradient(180deg, rgba(18,31,46,.96), rgba(8,17,29,.96)); font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; box-shadow: inset 0 -1px 0 rgba(255,255,255,.04); }
.graph-tab::before { content: ""; width: 8px; height: 18px; border-radius: 2px; background: #46617d; box-shadow: 0 0 14px rgba(70,97,125,.32); }
.graph-tab.is-active { border-color: #35c2ff; background: linear-gradient(180deg, #12324a, #0b1d2d); color: #eaf8ff; box-shadow: inset 0 -3px 0 #35c2ff, 0 0 28px rgba(53,194,255,.24); }
.graph-tab.is-active::before { background: #35c2ff; box-shadow: 0 0 18px rgba(53,194,255,.72); }
.graph-tab-title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 700; color: #f2f8ff; }
.graph-tab-kind { color: #77a7d7; font-size: 9px; letter-spacing: .12em; text-transform: uppercase; }
.graph-tab-count { color: #8fb2dc; border: 1px solid #2b4a67; background: rgba(2,8,15,.42); border-radius: 999px; padding: 2px 6px; }
#wire-status { position: absolute; left: 10px; top: 58px; z-index: 13; display: grid; grid-template-columns: auto auto minmax(0, 1fr); align-items: center; gap: 7px; max-width: min(470px, calc(100% - 24px)); min-height: 34px; padding: 7px 10px; border: 1px solid rgba(54,90,127,.78); background: rgba(7,16,28,.82); box-shadow: 0 14px 34px rgba(0,0,0,.34); border-radius: 6px; color: #93b3d7; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; pointer-events: none; }
#wire-status-dot { width: 9px; height: 9px; border-radius: 50%; background: var(--wire-color, #7dd3fc); box-shadow: 0 0 16px var(--wire-color, #7dd3fc); }
#wire-status b { color: #eaf5ff; font-weight: 700; }
#wire-status span:last-child { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#graph-overview { display: none; }
.graph-overview-title { display: grid; gap: 2px; min-width: 0; }
.graph-overview-title b { color: #f8fbff; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.graph-overview-title code { color: #fde68a; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.graph-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 6px; }
.graph-stat { display: grid; gap: 2px; padding: 6px; border: 1px solid #243852; background: rgba(8,16,27,.74); border-radius: 4px; min-width: 0; }
.graph-stat b { color: #eaf5ff; font-size: 13px; }
.graph-stat span { color: #84a8cf; font: 9px ui-monospace, "SFMono-Regular", Consolas, monospace; text-transform: uppercase; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#source-view, #source-editor { position: absolute; inset: 0; display: none; margin: 0; padding: 20px 24px 84px; overflow: auto; color: #dbeafe; background: #07101a; border: 0; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; line-height: 1.6; white-space: pre; tab-size: 4; }
#source-editor { resize: none; outline: none; }
#review-view { position: absolute; inset: 0; display: none; grid-template-rows: auto minmax(0, 1fr); background: #0a1018; color: #dbeafe; overflow: hidden; }
#stage.is-review #review-view { display: grid; }
#stage.is-review #jet-canvas-view, #stage.is-review #source-view, #stage.is-review #source-editor, #stage.is-review #minimap, #stage.is-review #graph-strip, #stage.is-review #wire-status, #stage.is-review #graph-overview, #stage.is-review #hud { display: none; }
#review-head { display: flex; align-items: start; justify-content: space-between; gap: 16px; padding: 16px 18px 14px; border-bottom: 1px solid #263b55; background: linear-gradient(135deg, #111d2b, #0b131e 70%); box-shadow: 0 10px 28px rgba(0,0,0,.26); }
.review-eyebrow { color: #65d4ff; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .16em; text-transform: uppercase; }
#review-head h1 { margin: 3px 0 4px; color: #f8fbff; font: 700 21px "Inter", "Segoe UI", system-ui, sans-serif; letter-spacing: -.02em; }
#review-head p { margin: 0; max-width: 620px; color: #94abc7; line-height: 1.45; }
.review-head-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; justify-content: end; }
.review-summary { display: flex; gap: 6px; flex-wrap: wrap; justify-content: end; }
.review-stat { display: grid; gap: 2px; min-width: 68px; padding: 6px 8px; border: 1px solid #29425f; background: rgba(7,16,28,.7); border-radius: 5px; }
.review-stat b { color: #f2f8ff; font: 700 14px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-stat span { color: #86a5c8; font: 9px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .08em; text-transform: uppercase; }
#review-refresh { min-height: 34px; }
#review-dev-facts { display: none; color: #708bad; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow-wrap: anywhere; }
body.is-dev-mode #review-dev-facts { display: block; }
#review-body { display: grid; grid-template-columns: minmax(180px, 23%) minmax(0, 1fr); min-height: 0; }
#review-files { min-width: 0; overflow: auto; padding: 12px; border-right: 1px solid #263b55; background: #0b131e; }
.review-file-list { display: grid; gap: 7px; }
.review-file { width: 100%; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 8px; align-items: center; padding: 9px; border: 1px solid #243b56; background: #0f1a27; border-radius: 5px; text-align: left; }
.review-file:hover, .review-file:focus-visible { border-color: #65d4ff; background: #12283a; }
.review-file.is-active { border-color: #65d4ff; background: #123044; box-shadow: inset 3px 0 0 #65d4ff; }
.review-file-mark { width: 8px; height: 8px; border-radius: 50%; background: #f6d365; box-shadow: 0 0 11px rgba(246,211,101,.58); }
.review-file-name { min-width: 0; overflow: hidden; color: #eef7ff; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }
.review-file-meta { color: #88a4c5; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-file-status { color: #f6d365; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; text-transform: uppercase; }
#review-pane { min-width: 0; min-height: 0; overflow: auto; padding: 16px 18px 36px; }
.review-file-head { display: flex; align-items: start; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
.review-file-title { min-width: 0; }
.review-file-title h2 { margin: 0 0 4px; color: #f8fbff; font: 700 17px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow-wrap: anywhere; }
.review-file-title p { margin: 0; color: #8fa8c6; }
.review-file-facts { display: grid; gap: 3px; min-width: 150px; text-align: right; color: #88a4c5; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-hunks { display: grid; gap: 14px; }
.review-hunk { overflow: hidden; border: 1px solid #2b4663; background: #09131f; border-radius: 6px; box-shadow: 0 12px 30px rgba(0,0,0,.18); }
.review-hunk.is-active { border-color: #65d4ff; box-shadow: 0 0 0 1px rgba(101,212,255,.24), 0 14px 34px rgba(0,0,0,.24); }
.review-hunk-head { display: flex; align-items: center; justify-content: space-between; gap: 9px; padding: 8px 10px; border-bottom: 1px solid #213853; background: #101e2d; }
.review-hunk-title { display: flex; align-items: center; gap: 8px; min-width: 0; color: #c6d8ed; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-hunk-title code { color: #8db3da; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.review-hunk-actions { display: flex; gap: 6px; flex: 0 0 auto; }
.review-hunk-actions button { min-height: 26px; padding-inline: 8px; font-size: 11px; }
.review-status { display: inline-flex; align-items: center; min-height: 20px; padding: 2px 7px; border: 1px solid currentColor; border-radius: 999px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .05em; text-transform: uppercase; }
.review-status-added { color: #5eead4; background: rgba(45,212,191,.1); }
.review-status-modified { color: #f6d365; background: rgba(246,211,101,.1); }
.review-status-deleted { color: #fb7185; background: rgba(251,113,133,.1); }
.review-status-unprojectable { color: #c084fc; background: rgba(192,132,252,.1); }
.review-status-clean { color: #8fa8c6; background: rgba(143,168,198,.08); }
.review-lines { display: grid; padding: 5px 0; font: 12px/1.55 ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-line { display: grid; grid-template-columns: 44px 44px 20px minmax(0, 1fr) auto; align-items: baseline; min-width: 0; padding: 1px 10px 1px 0; }
.review-line.is-add { background: rgba(45,212,191,.09); }
.review-line.is-delete { background: rgba(251,113,133,.09); }
.review-line.is-context { color: #9eb3cc; }
.review-line-number { color: #5f7693; text-align: right; user-select: none; }
.review-line-sign { color: #7089a7; text-align: center; user-select: none; }
.review-line.is-add .review-line-sign { color: #5eead4; }
.review-line.is-delete .review-line-sign { color: #fb7185; }
.review-line-text { min-width: 0; overflow-x: auto; color: #dceafa; white-space: pre; }
.review-line.is-delete .review-line-text { color: #fda4af; }
.review-line-note { margin-left: 9px; color: #85a4c7; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; white-space: nowrap; }
.review-map { padding: 8px 10px; border-top: 1px solid #213853; color: #94abc7; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-map strong { color: #eaf5ff; }
.review-empty { display: grid; gap: 8px; place-content: center; min-height: 280px; max-width: 620px; margin: 30px auto; padding: 24px; border: 1px dashed #365a7f; background: rgba(12,24,39,.78); border-radius: 8px; text-align: center; }
.review-empty h2 { margin: 0; color: #f8fbff; font-size: 18px; }
.review-empty p { margin: 0; color: #9bb2cf; line-height: 1.55; }
.review-legend { display: flex; flex-wrap: wrap; gap: 7px 12px; margin-bottom: 12px; color: #8fa8c6; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.review-legend span { display: inline-flex; align-items: center; gap: 5px; }
.review-legend i { width: 8px; height: 8px; display: inline-block; border-radius: 50%; background: currentColor; box-shadow: 0 0 8px currentColor; }
body:not(.is-dev-mode) .review-dev { display: none !important; }
#stage.is-code #source-view { display: block; }
#stage.is-source-edit #source-view { display: none; }
#stage.is-source-edit #source-editor { display: block; }
#stage.is-split #source-view { display: block; right: 50%; border-right: 1px solid #25364b; box-shadow: 12px 0 30px rgba(0,0,0,.28); }
#stage.is-split.is-source-edit #source-editor { display: block; right: 50%; border-right: 1px solid #25364b; box-shadow: 12px 0 30px rgba(0,0,0,.28); }
#stage.is-split #jet-canvas-view { position: absolute; right: 0; top: 0; width: 50%; height: 100%; border-left: 1px solid #25364b; }
#stage.is-split #graph-strip { left: calc(50% + 12px); }
#stage.is-split #wire-status { left: calc(50% + 10px); }
#stage.is-split #hud { left: calc(50% + 16px); }
#stage.is-split #minimap { width: min(180px, 18vw); }
#minimap { position: absolute; right: clamp(8px, 1.4vw, 16px); bottom: clamp(8px, 1.4vw, 16px); width: min(210px, 22vw); height: min(132px, 15vw); min-width: 120px; min-height: 78px; border: 1px solid #365a7f; background: rgba(7,16,28,.9); box-shadow: 0 14px 42px rgba(0,0,0,.42); border-radius: 6px; }
#hud { position: absolute; left: clamp(8px, 1.4vw, 16px); bottom: clamp(8px, 1.4vw, 16px); display: flex; gap: 8px; color: #9bb4d3; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: calc(100% - 32px); flex-wrap: wrap; }
#hud span { border: 1px solid #263b59; background: rgba(8,17,29,.88); padding: 5px 8px; border-radius: 4px; }
body:not(.is-dev-mode) #graph-meta { display: none; }
#details { min-height: 0; overflow: auto; --node-accent: #35c2ff; padding-bottom: 18px; }
.details-empty { display: grid; gap: 8px; padding: 12px; border: 1px dashed #31445d; background: #0d1520; color: #90a5c4; border-radius: 4px; }
.details-hero { position: relative; display: grid; gap: 10px; padding: 12px; border: 1px solid color-mix(in srgb, var(--node-accent) 56%, #21334a); background: linear-gradient(180deg, color-mix(in srgb, var(--node-accent) 18%, #101926), #09111a); border-radius: 6px; box-shadow: inset 4px 0 0 var(--node-accent), 0 14px 36px rgba(0,0,0,.26); }
.details-titleline { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 9px; align-items: center; }
.node-glyph { display: grid; place-items: center; width: 36px; height: 30px; color: var(--node-accent); border: 1px solid color-mix(in srgb, var(--node-accent) 64%, #1d2d42); background: color-mix(in srgb, var(--node-accent) 13%, #08111b); border-radius: 4px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; box-shadow: 0 0 18px color-mix(in srgb, var(--node-accent) 28%, transparent); }
.details-title { min-width: 0; }
#details .title { color: #f8fbff; font-size: 17px; font-weight: 700; margin: 0 0 2px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
#details .kind { color: var(--node-accent); font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; text-transform: uppercase; letter-spacing: .08em; }
.details-chips { display: flex; flex-wrap: wrap; gap: 6px; }
.details-chip { color: #bfd4ee; border: 1px solid #2b405a; background: rgba(6,14,24,.56); border-radius: 999px; padding: 3px 7px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.quick-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 7px; }
.quick-actions button { min-width: 0; }
.quick-actions .wide { grid-column: 1 / -1; }
#details dl { display: grid; grid-template-columns: 72px 1fr; gap: 6px 9px; margin: 10px 0 0; padding-top: 9px; border-top: 1px solid #21344b; }
#details dt { color: #8096b5; font-size: 11px; }
#details dd { margin: 0; color: #d8e7fb; overflow-wrap: anywhere; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.pin-list, .inline-list { display: grid; gap: 7px; margin-top: 8px; }
.pin-row, .inline-row { border: 1px solid #2d4056; background: #101821; padding: 8px; border-radius: 4px; }
.pin-row { display: grid; gap: 6px; }
.pin-row b, .inline-row b { color: #f2f7ff; }
.pin-card { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 8px; border: 1px solid color-mix(in srgb, var(--pin-color) 42%, #253852); background: linear-gradient(180deg, color-mix(in srgb, var(--pin-color) 10%, #101821), #0a121b); padding: 8px; border-radius: 4px; box-shadow: inset 3px 0 0 var(--pin-color); }
.pin-card-title { min-width: 0; }
.pin-card-title b { display: block; color: #f2f7ff; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pin-card-title small { display: block; margin-top: 2px; color: #91a7c4; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pin-card button { min-height: 26px; padding-inline: 8px; }
.inline-row code { display: block; color: #fde68a; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; margin-top: 4px; white-space: pre-wrap; }
.edit-grid { display: grid; gap: 8px; margin-top: 10px; }
.edit-grid label { display: grid; gap: 4px; color: #90a5c4; }
.signature-board { display: grid; gap: 10px; margin: 10px 0 14px; padding: 10px; border: 1px solid #385a7e; background: linear-gradient(180deg, #0d1825, #09111b); border-radius: 6px; box-shadow: inset 0 1px 0 rgba(255,255,255,.04); }
.signature-head { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: 8px; padding-bottom: 8px; border-bottom: 1px solid #223954; }
.signature-head b { display: block; color: #f8fbff; font-size: 14px; overflow-wrap: anywhere; }
.signature-head code, .signature-source code { display: block; color: #fde68a; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; }
.sig-eyebrow, .lane-meta { color: #84a8cf; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .09em; text-transform: uppercase; }
.signature-actions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 7px; }
.signature-actions .primary { grid-column: 1 / -1; }
.signature-source { display: grid; gap: 7px; padding: 8px; border: 1px solid #263c55; background: #0a121c; border-radius: 4px; }
.signature-source input { width: 100%; }
.rename-strip { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 7px; }
.pin-lane { display: grid; gap: 7px; padding: 8px; border: 1px solid #243852; background: rgba(8,16,27,.78); border-radius: 4px; }
.lane-head { display: flex; align-items: center; gap: 8px; min-width: 0; flex-wrap: wrap; }
.lane-head b { color: #eaf5ff; letter-spacing: .06em; text-transform: uppercase; font-size: 11px; }
.lane-head .lane-meta { margin-left: auto; white-space: nowrap; }
.lane-head button { min-height: 26px; padding-inline: 8px; flex: 0 0 auto; }
.pin-editor-row { display: grid; gap: 7px; padding: 8px; border: 1px solid #2c4868; background: #0e1722; border-radius: 4px; }
.pin-editor-title { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; min-width: 0; }
.pin-editor-title b { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.pin-port { width: 12px; height: 12px; border-radius: 50%; display: inline-block; box-shadow: 0 0 0 3px rgba(255,255,255,.07), 0 0 16px currentColor; background: currentColor; }
.pin-port.is-exec { width: 14px; height: 14px; border-radius: 2px; clip-path: polygon(0 0, 100% 50%, 0 100%); }
.pin-port.is-fallible { border-radius: 2px; transform: rotate(45deg); }
.type-chip { color: #cfe9ff; border: 1px solid currentColor; background: rgba(255,255,255,.04); border-radius: 999px; padding: 2px 7px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: 98px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pin-empty { color: #7890ad; border: 1px dashed #31445d; padding: 8px; border-radius: 4px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.pin-tools { display: grid; grid-template-columns: minmax(0, 1fr) 68px 32px; gap: 6px; margin-top: 2px; }
.pin-tools.output-pin-tools { grid-template-columns: minmax(0, 1fr) 68px 46px; }
.pin-tools [data-param-default] { grid-column: 1 / 3; }
.pin-tools input { min-width: 0; }
#context-menu { position: fixed; z-index: 30; display: none; min-width: 340px; max-width: min(470px, calc(100vw - 20px)); border: 1px solid #344b68; background: #101318; box-shadow: 0 18px 48px rgba(0,0,0,.55); padding: 8px; border-radius: 6px; }
#context-menu.is-open { display: grid; gap: 6px; }
#context-menu button { width: 100%; text-align: left; }
#context-menu .menu-title { color: #dbeafe; font-size: 11px; letter-spacing: .1em; text-transform: uppercase; padding: 4px 6px; }
.action-palette-head { display: grid; gap: 6px; padding: 4px; border-bottom: 1px solid #203954; }
.action-palette-head input { width: 100%; border-color: #3b5f89; background: #07111f; }
.action-context { color: #8fb2dc; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; display: flex; gap: 7px; align-items: center; min-width: 0; }
.action-context .pin-port { width: 9px; height: 9px; box-shadow: 0 0 0 2px rgba(255,255,255,.07), 0 0 12px currentColor; }
.action-results { display: grid; gap: 5px; max-height: min(360px, calc(100vh - 170px)); overflow: auto; padding: 2px; }
.action-category { display: grid; gap: 4px; }
.action-category h3 { margin: 8px 4px 2px; color: #8fb2dc; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .12em; text-transform: uppercase; }
.action-category h4 { margin: 6px 4px 1px; color: #6f8fb3; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.action-result { --action-color: #8a8f98; border-color: color-mix(in srgb, var(--action-color) 48%, #284866); background: #151922; min-height: 44px; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 9px; align-items: center; }
.action-glyph { display: grid; place-items: center; width: 22px; height: 22px; color: var(--action-color); border: 1px solid color-mix(in srgb, var(--action-color) 62%, #162131); background: color-mix(in srgb, var(--action-color) 13%, #101318); border-radius: 4px; font-weight: 700; }
.action-glyph { font: 700 10px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; }
.action-result.is-favorite { border-color: #facc15; box-shadow: inset 3px 0 0 #facc15; }
.action-result:hover, .action-result:focus-visible { border-color: var(--action-color); background: #1d2129; }
.action-result.is-disabled { opacity: .58; filter: grayscale(.35); cursor: not-allowed; }
.action-result.is-disabled:hover, .action-result.is-disabled:focus-visible { border-color: #3f4856; background: #151922; }
.action-result small { color: #9aaecb; display: block; margin-top: 2px; overflow-wrap: anywhere; font-family: "JetBrains Mono", ui-monospace, "SFMono-Regular", Consolas, monospace; }
.action-empty { color: #8da4c2; padding: 9px; border: 1px dashed #31445d; border-radius: 4px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.action-empty[data-action-empty] { display: grid; gap: 8px; }
.action-empty[data-action-empty] b { color: #e4efff; font: 600 12px system-ui, sans-serif; }
.action-empty[data-action-empty] span { line-height: 1.45; }
.action-empty[data-action-empty] button { justify-self: start; }
#keyboard-cheat-sheet { width: min(760px, calc(100vw - 32px)); max-height: min(82vh, 760px); padding: 0; border: 1px solid #46698f; border-radius: 8px; background: #0b1118; color: #d7e4f7; box-shadow: 0 26px 90px rgba(0,0,0,.68); }
#keyboard-cheat-sheet:not([open]) { display: none; }
#keyboard-cheat-sheet[open] { display: grid; grid-template-rows: auto auto minmax(0, 1fr) auto; overflow: hidden; }
#keyboard-cheat-sheet::backdrop { background: rgba(2,7,13,.74); }
.keyboard-cheat-sheet-head { display: flex; align-items: start; justify-content: space-between; gap: 16px; padding: 16px 18px 12px; border-bottom: 1px solid #263b55; background: linear-gradient(135deg, #111d2b, #0b131e 70%); }
.keyboard-cheat-sheet-head h2 { margin: 0 0 4px; color: #f8fbff; font-size: 18px; }
.keyboard-cheat-sheet-head p, .keyboard-cheat-sheet-note { margin: 0; color: #94abc7; line-height: 1.45; }
.keyboard-cheat-sheet-note { padding: 12px 18px 0; }
.keyboard-shortcuts { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; padding: 12px 18px 18px; overflow: auto; }
.keyboard-shortcut-group { display: grid; align-content: start; gap: 5px; padding: 9px; border: 1px solid #263b55; border-radius: 6px; background: #0f1823; }
.keyboard-shortcut-group h3 { margin: 0 0 3px; color: #65d4ff; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .1em; text-transform: uppercase; }
.keyboard-shortcut-row { display: grid; grid-template-columns: minmax(112px, auto) minmax(0, 1fr); gap: 10px; align-items: baseline; color: #c9dcf2; line-height: 1.35; }
.keyboard-shortcut-row kbd { color: #f8fbff; border: 1px solid #46698f; border-bottom-width: 2px; border-radius: 4px; background: #172435; padding: 2px 5px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; text-align: center; white-space: nowrap; }
.keyboard-cheat-sheet-foot { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 18px 14px; border-top: 1px solid #263b55; color: #8fa7c6; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.keyboard-cheat-sheet-foot kbd { color: #eaf5ff; }
@media (max-width: 640px) {
  .keyboard-shortcuts { grid-template-columns: 1fr; }
  .keyboard-shortcut-row { grid-template-columns: minmax(104px, auto) minmax(0, 1fr); }
}
#first-run-tour { position: fixed; inset: auto 18px 42px auto; z-index: 29; width: min(340px, calc(100vw - 36px)); display: none; gap: 10px; padding: 12px; border: 1px solid #365a7f; border-radius: 6px; background: rgba(7,16,28,.95); box-shadow: 0 22px 70px rgba(0,0,0,.5); color: #c9dcf2; pointer-events: none; }
#first-run-tour.is-open { display: grid; }
#first-run-tour b { color: #f8fbff; }
#first-run-tour button { pointer-events: auto; }
.canvas-tour-target { outline: 2px solid #f6d365; outline-offset: 3px; box-shadow: 0 0 0 5px rgba(246,211,101,.14), 0 0 26px rgba(246,211,101,.24); }
#first-run-tour button:focus-visible { outline: 2px solid #f6d365; outline-offset: 2px; }
#run-hud { position: absolute; right: 12px; top: 100px; z-index: 12; display: none; min-width: 190px; padding: 8px 10px; border: 1px solid #31506d; border-radius: 6px; background: rgba(8,16,27,.88); color: #9fb9d8; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#run-hud.is-running { display: block; border-color: #facc15; color: #fef3c7; }
.lod-node { opacity: .88; }
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: .001ms !important; transition-duration: .001ms !important; scroll-behavior: auto !important; }
}
#statusbar { display: flex; align-items: center; gap: 14px; padding: 0 12px; border-top: 1px solid #23344a; background: #070b10; color: #8096b5; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#toast { position: fixed; right: 12px; bottom: 34px; z-index: 50; max-width: min(760px, calc(100vw - 24px)); max-height: min(50vh, 420px); overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; color: #a7f3d0; border: 1px solid #2b5948; background: rgba(7,16,22,.96); border-radius: 6px; padding: 8px 10px; box-shadow: 0 16px 44px rgba(0,0,0,.42); cursor: pointer; }
#toast:empty { display: none; }
#toast.is-error { color: #fecaca; border-color: #ef4444; background: rgba(31,9,12,.97); }
#toast button { margin-left: 10px; border-color: #ef4444; color: #fee2e2; }
.problem-list { display: grid; gap: 8px; }
.problem-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 8px; align-items: start; padding: 8px; border: 1px solid #334155; border-radius: 6px; background: #10151e; text-align: left; color: #dbeafe; }
.problem-row[data-severity="error"] { border-color: #ef4444; box-shadow: inset 3px 0 0 #ef4444; }
.problem-row[data-severity="warning"] { border-color: #f59e0b; box-shadow: inset 3px 0 0 #f59e0b; }
.problem-row b { color: #f8fbff; font-family: "JetBrains Mono", ui-monospace, "SFMono-Regular", Consolas, monospace; }
.problem-row small { display: block; margin-top: 2px; color: #9fb9d8; overflow-wrap: anywhere; }
.problem-row button { padding: 3px 7px; }
.problem-detail { grid-column: 1 / 4; margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; color: #fecaca; font: 10px "JetBrains Mono", ui-monospace, "SFMono-Regular", Consolas, monospace; }
.problem-empty { color: #8fb2dc; border: 1px dashed #334155; border-radius: 6px; padding: 8px; font: 11px "JetBrains Mono", ui-monospace, "SFMono-Regular", Consolas, monospace; }
@media (max-width: 1120px) {
  #workbench { grid-template-columns: minmax(142px, 18vw) minmax(0, 1fr) minmax(200px, 23vw); }
  #workbench-header { gap: 8px; }
  #brand span { display: none; }
  #topbar { gap: 6px; }
  .debug-controls { flex-basis: 280px; }
  .debug-controls button, #topbar > button { padding-inline: 6px; }
}
@media (max-width: 900px) {
  #workbench { grid-template-columns: minmax(0, 1fr); }
  #workbench-header { grid-template-columns: auto minmax(0, 1fr); }
  .workbench-session { justify-self: end; }
  .workbench-map { grid-column: 1 / -1; grid-row: 2; }
  #canvas-dock { display: flex; }
  #graph-strip { top: 56px; left: 10px; right: 10px; max-width: calc(100% - 20px); }
  #wire-status { top: 104px; left: 10px; right: 10px; max-width: none; }
  #graph-overview { display: none; }
  .side { display: none; position: absolute; top: 0; bottom: 0; left: 0; width: min(326px, calc(100vw - 54px)); z-index: 22; border-right: 1px solid #35c2ff; background: rgba(9,15,23,.98); box-shadow: 18px 0 52px rgba(0,0,0,.58); }
  .right { left: auto; right: 0; border-left: 1px solid #35c2ff; border-right: 0; box-shadow: -18px 0 52px rgba(0,0,0,.58); }
  .side.is-drawer-open { display: block; }
  #stage { grid-column: 1; }
  #jump { display: none; }
}
@media (max-width: 640px) {
  #workbench { grid-template-columns: 1fr; }
  #workbench-header { grid-template-columns: 1fr; gap: 5px; }
  .workbench-session { justify-self: stretch; justify-content: start; }
  .workbench-map { grid-column: auto; grid-row: auto; }
  .side { width: min(310px, calc(100vw - 42px)); }
  .side:not(.is-drawer-open) { display: none; }
  #minimap { display: none; }
  #graph-overview { display: none; }
  #topbar > button, .debug-controls button { min-height: 28px; padding-inline: 6px; }
}
</style>
</head>
<body>
<div id="shell">
  <header id="topbar">
    <div id="brand"><strong>Jet Canvas</strong><span>Source-backed editor</span></div>
    <div class="toolbar-group" aria-label="View controls">
      <button id="graph-back" class="icon-button" title="Back" aria-label="Back"><svg viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6"/></svg></button>
      <button id="graph-forward" class="icon-button" title="Forward" aria-label="Forward"><svg viewBox="0 0 24 24"><path d="M9 6l6 6-6 6"/></svg></button>
      <button id="fit" class="icon-button" title="Fit graph" aria-label="Fit graph"><svg viewBox="0 0 24 24"><path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M16 3h3a2 2 0 0 1 2 2v3"/><path d="M8 21H5a2 2 0 0 1-2-2v-3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/></svg></button>
      <span class="zoom-readout" id="toolbar-zoom">100%</span>
    </div>
    <div id="lens-switch" class="lens-switch toolbar-group" role="group" aria-label="Canvas lens"><button id="view-code" data-view-mode="code">Code</button><button id="view-split" data-view-mode="split">Split</button><button id="view-graph" data-view-mode="graph">Graph</button><button id="review-view-button" data-view-mode="review">Review</button></div>
    <div class="toolbar-group" aria-label="Edit controls">
      <button id="undo-edit" class="icon-button" title="Undo" aria-label="Undo"><svg viewBox="0 0 24 24"><path d="M9 14l-5-5 5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-2"/></svg></button>
      <button id="redo-edit" class="icon-button" title="Redo" aria-label="Redo"><svg viewBox="0 0 24 24"><path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h2"/></svg></button>
      <button id="org-align" class="icon-button" title="Align selected nodes" aria-label="Align selected nodes"><svg viewBox="0 0 24 24"><path d="M6 4v16"/><path d="M10 7h8"/><path d="M10 12h6"/><path d="M10 17h10"/></svg></button>
      <button id="org-distribute" class="icon-button" title="Distribute selected nodes" aria-label="Distribute selected nodes"><svg viewBox="0 0 24 24"><path d="M4 4v16"/><path d="M20 4v16"/><path d="M8 9h3v6H8z"/><path d="M13 9h3v6h-3z"/></svg></button>
      <button id="org-tidy" class="icon-button" title="Tidy graph" aria-label="Tidy graph"><svg viewBox="0 0 24 24"><path d="M4 7h6v6H4z"/><path d="M14 4h6v6h-6z"/><path d="M14 14h6v6h-6z"/><path d="M10 10l4-3"/><path d="M10 13l4 4"/></svg></button>
    </div>
    <div class="toolbar-group" aria-label="Run controls">
      <button id="check-current" title="Check current source" aria-label="Check current source">Check</button>
      <button id="run-current" class="primary" title="Run current entry" aria-label="Run current entry" data-capability="runtime_output" hidden>Run</button>
      <details id="debug-menu" class="toolbar-menu" data-capability="runtime_output" hidden><summary class="icon-button" title="Debug controls" aria-label="Debug controls"><svg viewBox="0 0 24 24"><path d="M8 2v4"/><path d="M16 2v4"/><path d="M7 10h10"/><path d="M12 6v14"/><path d="M5 14h14"/><path d="M8 22h8"/><path d="M4 18l4-4"/><path d="M20 18l-4-4"/></svg></summary><div class="toolbar-popover debug-controls">
      <select id="debug-session" aria-label="Debug session"><option value="none">No live session</option></select>
      <span id="debug-liveness" class="tag" role="status">Runtime idle</span>
      <button id="debug-start" type="button" aria-label="Start debug">Start debug</button>
      <button id="debug-break">Break</button>
      <button id="debug-watch">Watch</button>
      <button id="debug-step">Step</button>
      <button id="debug-next">Next</button>
      <button id="debug-continue">Continue</button>
      <button id="debug-stop">Stop</button>
      </div></details>
    </div>
    <div class="toolbar-spacer"></div>
    <div class="toolbar-group" aria-label="Source and navigation">
      <details class="toolbar-menu"><summary id="more-tools-toggle" class="icon-button" title="More tools" aria-label="More tools"><svg viewBox="0 0 24 24"><path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h16"/></svg></summary><div class="toolbar-popover">
        <select id="graph-select" aria-label="Graph"></select>
        <button id="reload">Reload</button>
        <button id="source-diff">Diff</button>
        <button id="edit-source">Edit Source</button>
        <button id="apply-source-edit">Apply Source</button>
        <button id="cancel-source-edit">Cancel</button>
        <button id="view-toggle">Code</button>
        <button id="bookmark-add">Mark</button>
        <button id="bookmark-jump">Go</button>
        <button id="core-catalog">Core</button>
        <button id="favorite-action">Fav</button>
        <button id="keyboard-help" type="button" aria-haspopup="dialog" aria-controls="keyboard-cheat-sheet">Keyboard</button>
      </div></details>
      <button id="toolbar-search" class="icon-button" title="Search" aria-label="Search"><svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg></button>
      <button id="developer-mode" class="icon-button" title="Developer details" aria-label="Developer details"><svg viewBox="0 0 24 24"><path d="M16 18l6-6-6-6"/><path d="M8 6l-6 6 6 6"/><path d="M14 4l-4 16"/></svg></button>
      <details class="toolbar-menu dev-only"><summary class="icon-button" title="Detail filters" aria-label="Detail filters"><svg viewBox="0 0 24 24"><path d="M4 7h16"/><path d="M7 12h10"/><path d="M10 17h4"/></svg></summary><div id="detail-toggles" class="detail-toggles toolbar-popover" aria-label="Detail toggles"><label class="detail-toggle"><input id="toggle-types" data-detail-toggle="types" type="checkbox">Types</label><label class="detail-toggle"><input id="toggle-diagnostics" data-detail-toggle="diagnostics" type="checkbox">Diagnostics</label><label class="detail-toggle"><input id="toggle-effects" data-detail-toggle="effects" type="checkbox">Effects</label><label class="detail-toggle"><input id="toggle-debug" data-detail-toggle="debug" type="checkbox">Debug</label><label class="detail-toggle"><input id="toggle-package" data-detail-toggle="package" type="checkbox">Package</label></div></details>
    </div>
    <span id="jump">loading graph</span>
  </header>
  <main id="workbench">
    <header id="workbench-header" aria-label="Jet project workbench">
      <div class="workbench-heading"><span>Project IDE</span><strong>Canvas Workbench</strong></div>
      <div class="workbench-map" aria-label="Workbench regions">
        <span><b>Project</b><span>Files · Outputs</span></span>
        <span><b>Editor</b><span>Code · Graph</span></span>
        <span><b>Runtime</b><span>Preview · Problems</span></span>
      </div>
      <div class="workbench-session"><b>Session</b><span id="session-identity" class="tag" data-session-id="">starting session</span><div id="run-hud" aria-live="polite" data-capability="runtime_output" hidden>run idle</div></div>
    </header>
    <aside id="left-drawer" class="side">
      <section id="canvas-panel" class="panel">
        <div id="canvas-component-tree" data-canvas-component-tree role="tree" aria-label="My Canvas">
          <div class="component-tree-head"><h2>My Canvas</h2><span class="tag">source tree</span></div>
          <section id="project-panel" class="component-tree-section" data-canvas-panel="project">
            <details open>
              <summary><span>Files</span><span id="project-mode" class="count">file</span></summary>
              <div id="project-rail" class="project-list"></div>
            </details>
          </section>
          <section id="output-panel" class="component-tree-section" data-canvas-panel="output" hidden>
            <details open>
              <summary><span>Outputs</span><span id="output-count" class="count">0</span></summary>
              <div id="output-list" class="project-list" aria-label="Project outputs"></div>
            </details>
          </section>
          <section id="graphs-panel" class="component-tree-section" data-canvas-panel="graphs">
            <details open>
              <summary><span>Functions</span><span class="component-tree-section-actions"><button id="canvas-new-function" type="button" title="Create a source-backed function">+ New</button><button id="canvas-new-callback" type="button" title="Create a source-backed callback handler">+ Callback</button><span id="graph-count" class="count">0</span></span></summary>
              <div id="graph-list" class="graph-list"></div>
            </details>
          </section>
          <section id="variables-panel" class="component-tree-section" data-canvas-panel="variables">
            <details open>
              <summary><span>Variables</span><span class="component-tree-section-actions"><button id="canvas-add-variable" type="button" title="Promote a selected value to a source-backed variable">+ Add</button><span id="variable-count" class="count">0</span></span></summary>
              <div id="variables-list" class="variable-list"></div>
            </details>
          </section>
        </div>
      </section>
      <section id="status-panel" class="panel" data-canvas-panel="status"><details open><summary><span>Status</span><span id="status-count" class="count">clean</span></summary><div id="status-summary" class="status-grid"></div><div id="package-summary" class="project-section dev-only package-detail"></div><div id="dependency-summary" class="project-section dev-only package-detail"></div><div id="dev-summary" class="project-section dev-only package-detail"></div><div id="diagnostics-summary" class="project-section dev-only diagnostic-detail"></div><div id="trust-summary" class="project-section dev-only package-detail"></div><div id="session-views" class="project-section" aria-label="Session views"><div data-session-view="text"></div><div data-session-view="graph"></div><div data-session-view="designer" data-capability="designer" hidden></div><div data-session-view="preview" data-capability="preview" hidden></div><div data-session-view="terminal" data-capability="terminal" hidden></div><div data-session-view="debugger" data-capability="runtime_output" hidden></div><div data-session-view="tests" data-capability="diagnostics" hidden></div><div data-session-view="custom servers" data-capability="service" hidden></div></div></details></section>
      <section id="search-panel" class="panel" data-canvas-panel="search"><details><summary><span>Search</span></summary><input id="canvas-search" class="search" placeholder="Find in graph"><div id="search-results" class="search-results"></div></details></section>
    </aside>
    <section id="stage">
      <div id="canvas-dock" aria-label="Canvas panels"><button id="dock-graphs">Graphs</button><button id="dock-details">Inspector</button></div>
      <div id="graph-strip" aria-label="Graph tabs"></div>
      <div id="wire-status" role="status" aria-label="Canvas hover details" aria-live="polite"><span id="wire-status-dot"></span><b>Ready</b><span>Hover a node or pin for details</span></div>
      <div id="graph-overview" aria-label="Graph overview"></div>
      <section id="review-view" aria-label="Git text review">
        <header id="review-head">
          <div>
            <div class="review-eyebrow">Canvas M3 · source truth</div>
            <h1>Review changes</h1>
            <p>Read the Git text diff first. Select a hunk to see its current source span and graph projection.</p>
            <div id="review-dev-facts" class="review-dev" aria-label="Review developer facts"></div>
          </div>
          <div class="review-head-actions">
            <div id="review-summary" class="review-summary" aria-label="Review summary"></div>
            <button id="review-refresh" type="button">Refresh</button>
          </div>
        </header>
        <div id="review-body">
          <aside id="review-files" aria-label="Changed files"><div id="review-file-list" class="review-file-list"></div></aside>
          <section id="review-pane" aria-live="polite"><div id="review-content"></div></section>
        </div>
      </section>
      <canvas id="jet-canvas-view" width="1400" height="900" tabindex="0" aria-label="Jet source graph. Press question mark for keyboard shortcuts."></canvas>
      <pre id="source-view" aria-label="Jet source"></pre>
      <textarea id="source-editor" aria-label="Editable Jet source"></textarea>
      <canvas id="minimap" width="190" height="124"></canvas>
      <div id="hud"><span id="zoom-label">100%</span><span id="graph-meta">0 nodes</span></div>
    </section>
    <aside id="right-drawer" class="side right">
      <section id="problems-panel" class="panel" data-canvas-panel="problems"><details open><summary><span>Problems</span><span id="problems-count" class="count">0</span></summary><div id="problems-list" class="problem-list"></div></details></section>
      <section id="details" class="panel" data-canvas-panel="details"></section>
      <section id="proof-panel" class="panel" data-canvas-panel="proof"><details open><summary><span>Proof</span><span id="proof-state" class="count">unknown</span></summary><div id="proof-rail" class="proof-rail"></div></details></section>
      <section id="preview-panel" class="panel" data-canvas-panel="preview" data-capability="preview" hidden><details open><summary><span>App preview</span><span class="count">last good</span></summary><div id="preview-status" class="tag">waiting for session</div><a id="preview-link" href="/" target="_blank" rel="noreferrer">Preview is starting</a></details></section>
    </aside>
  </main>
  <footer id="statusbar"><span id="source-id">source</span><span id="revision">revision</span><span id="session-id">session</span><span id="schema">canvas v1</span><span id="scm-state">git</span><span id="toast"></span></footer>
</div>
<div id="context-menu" role="menu"></div>
<dialog id="keyboard-cheat-sheet" role="dialog" aria-modal="true" aria-hidden="true" aria-labelledby="keyboard-cheat-sheet-title" aria-describedby="keyboard-cheat-sheet-note">
  <header class="keyboard-cheat-sheet-head">
    <div>
      <h2 id="keyboard-cheat-sheet-title">Canvas keyboard shortcuts</h2>
      <p>Use these commands while graph focus is active. Text fields keep their normal editing keys.</p>
    </div>
    <button id="keyboard-cheat-sheet-close" type="button" aria-label="Close keyboard shortcuts">Close</button>
  </header>
  <p id="keyboard-cheat-sheet-note" class="keyboard-cheat-sheet-note">Jet source stays canonical. Layout, selection, and shortcut help are local Canvas state.</p>
  <div class="keyboard-shortcuts">
    <section class="keyboard-shortcut-group" aria-labelledby="keyboard-shortcuts-view">
      <h3 id="keyboard-shortcuts-view">View and search</h3>
      <div class="keyboard-shortcut-row"><kbd>?</kbd><span>Open keyboard shortcuts</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + `</kbd><span>Toggle graph and Code</span></div>
      <div class="keyboard-shortcut-row"><kbd>/</kbd><span>Focus Canvas search</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + F</kbd><span>Find in Canvas</span></div>
      <div class="keyboard-shortcut-row"><kbd>F</kbd><span>Fit graph</span></div>
      <div class="keyboard-shortcut-row"><kbd>Space + drag</kbd><span>Pan graph</span></div>
    </section>
    <section class="keyboard-shortcut-group" aria-labelledby="keyboard-shortcuts-actions">
      <h3 id="keyboard-shortcuts-actions">Actions</h3>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + K or P</kbd><span>Open action palette</span></div>
      <div class="keyboard-shortcut-row" data-capability="runtime_output" hidden><kbd>Alt + Enter</kbd><span>Prepare Run</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + Z</kbd><span>Undo source edit</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + Y</kbd><span>Redo source edit</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + Shift + Z</kbd><span>Redo source edit</span></div>
      <div class="keyboard-shortcut-row"><kbd>Esc</kbd><span>Close transient UI or clear selection</span></div>
    </section>
    <section class="keyboard-shortcut-group" aria-labelledby="keyboard-shortcuts-layout">
      <h3 id="keyboard-shortcuts-layout">Arrange and navigate</h3>
      <div class="keyboard-shortcut-row"><kbd>Arrow keys</kbd><span>Nudge selected nodes</span></div>
      <div class="keyboard-shortcut-row"><kbd>Shift + Arrow</kbd><span>Nudge farther</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + A</kbd><span>Align top</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + Shift + A</kbd><span>Align left</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + D</kbd><span>Distribute horizontally</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + Shift + D</kbd><span>Distribute vertically</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + T</kbd><span>Tidy graph</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + B / G</kbd><span>Save / go to bookmark</span></div>
    </section>
    <section class="keyboard-shortcut-group" aria-labelledby="keyboard-shortcuts-source">
      <h3 id="keyboard-shortcuts-source">Source authoring</h3>
      <div class="keyboard-shortcut-row"><kbd>C</kbd><span>Add source-backed comment</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + C</kbd><span>Copy selection</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + V</kbd><span>Paste selection</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + Shift + V</kbd><span>Paste as staged</span></div>
      <div class="keyboard-shortcut-row"><kbd>Ctrl/Cmd + D</kbd><span>Duplicate selection</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + C</kbd><span>Collapse selected source</span></div>
      <div class="keyboard-shortcut-row"><kbd>Alt + Shift + C</kbd><span>Expand selected source</span></div>
    </section>
  </div>
  <footer class="keyboard-cheat-sheet-foot"><span>Press <kbd>?</kbd> again after closing to reopen.</span></footer>
</dialog>
<div id="first-run-tour" role="dialog" aria-modal="false" aria-label="Canvas first run" aria-describedby="tour-detail">
  <b>Canvas edits Jet source.</b>
  <span>Use Code, Split, or Graph, then right-click or drag from a pin and release on empty space.</span>
  <button id="tour-dismiss">Dismiss</button>
</div>
__JET_CANVAS_BOOTSTRAP__
</body>
</html>
"#
    .replace("__JET_CANVAS_BOOTSTRAP__", bootstrap)
}

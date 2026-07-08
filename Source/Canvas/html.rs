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
        r#"<script>window.__JET_CANVAS_BASE__ = ""; window.__JET_CANVAS_GRAPH__ = "/?jet_panel_graph=1"; window.__JET_CANVAS_TX__ = "/canvas/transaction"; window.__JET_CANVAS_QUERY__ = "/canvas/query"; window.__JET_CANVAS_SCM__ = "/canvas/source-control";</script>
<script src="/?jet_panel_app=1&canvas_ui=blueprint23"></script>"#,
    )
}

fn canvas_html_document(bootstrap: &str) -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Jet Canvas</title>
<style>
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; overflow: hidden; background: #05070b; color: #d7e4f7; font: 13px "Inter", "Segoe UI", system-ui, sans-serif; }
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
#topbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-width: 0; min-height: 54px; padding: 8px 10px; border-bottom: 1px solid #25364b; background: linear-gradient(#111923, #0c1119 62%, #080c12); box-shadow: 0 1px 0 rgba(255,255,255,.04) inset, 0 14px 40px rgba(0,0,0,.25); }
#brand { display: flex; flex-direction: column; gap: 1px; flex: 0 1 164px; min-width: 124px; padding-left: 8px; border-left: 3px solid #35c2ff; }
#brand strong { font-size: 14px; letter-spacing: .08em; text-transform: uppercase; color: #f8fbff; }
#brand span { color: #9db4d2; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#graph-select { display: none; border-color: #4b6685; background: #0c1420; color: #eaf5ff; }
body.is-dev-mode #graph-select { display: block; }
#topbar > select { flex: 1 1 190px; min-width: 140px; max-width: 360px; }
#topbar > button { flex: 0 0 auto; padding-inline: 8px; }
#jump { flex: 1 1 150px; min-width: 94px; color: #9db4d2; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
body:not(.is-dev-mode) #jump { display: none; }
.debug-controls { margin-left: auto; display: flex; align-items: center; gap: 5px; flex: 1 1 380px; min-width: 0; justify-content: flex-end; flex-wrap: wrap; }
body:not(.is-dev-mode) .debug-controls { display: none; }
.debug-controls select { flex: 1 1 130px; min-width: 112px; max-width: 220px; }
.debug-controls button { min-width: 30px; padding: 0 7px; }
#workbench { min-height: 0; min-width: 0; position: relative; display: grid; grid-template-columns: minmax(156px, 15vw) minmax(0, 1fr) minmax(238px, 20vw); background: #05070b; }
.side { min-width: 0; overflow: hidden auto; background: #0b1017; border-right: 1px solid #23344a; box-shadow: inset -1px 0 0 rgba(255,255,255,.03); }
.right { border-right: 0; border-left: 1px solid #23344a; box-shadow: inset 1px 0 0 rgba(255,255,255,.03); }
.panel { border-bottom: 1px solid #23344a; padding: clamp(9px, 1.2vw, 13px); }
.panel h2 { margin: 0 0 10px; color: #eaf5ff; font-size: 11px; letter-spacing: .12em; text-transform: uppercase; }
.panel details { display: grid; gap: 8px; }
.panel summary { list-style: none; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 30px; color: #eaf5ff; font-size: 11px; letter-spacing: .12em; text-transform: uppercase; cursor: pointer; }
.panel summary::-webkit-details-marker { display: none; }
.panel summary::before { content: "▸"; color: #35c2ff; font-size: 12px; transform: rotate(0deg); transition: transform .12s ease; }
.panel details[open] summary::before { transform: rotate(90deg); }
.panel summary .count { justify-self: end; }
.panel details > :not(summary) { margin-top: 8px; }
.graph-list, .search-results, .project-list { display: grid; gap: 6px; }
.graph-item { width: 100%; display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 8px; text-align: left; border-color: #283b52; background: #101821; min-height: 38px; }
.graph-item.is-active { border-color: #35c2ff; background: #102437; box-shadow: inset 3px 0 0 #35c2ff; }
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
#stage { position: relative; min-width: 0; min-height: 0; overflow: hidden; background: #05070b; }
#jet-canvas-view { width: 100%; height: 100%; display: block; background: #05070b; }
#canvas-dock { position: absolute; left: 10px; top: 10px; z-index: 26; display: none; gap: 6px; padding: 5px; border: 1px solid #365a7f; background: rgba(8,17,29,.92); box-shadow: 0 12px 34px rgba(0,0,0,.42); border-radius: 6px; }
#canvas-dock button { min-height: 28px; padding: 0 8px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#canvas-dock button.is-active { border-color: #35c2ff; color: #e7fbff; background: #102437; }
#graph-strip { position: absolute; left: 12px; right: 12px; top: 10px; z-index: 13; display: flex; gap: 8px; overflow-x: auto; scrollbar-width: thin; padding: 6px; border: 1px solid rgba(53,194,255,.45); background: linear-gradient(180deg, rgba(9,19,32,.94), rgba(5,11,19,.82)); box-shadow: 0 16px 44px rgba(0,0,0,.42), inset 0 1px 0 rgba(255,255,255,.05); border-radius: 6px; max-width: min(860px, calc(100% - 24px)); }
body:not(.is-dev-mode) #graph-strip { display: none; }
.graph-tab { position: relative; flex: 0 0 auto; min-width: 116px; min-height: 36px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 7px; align-items: center; border-color: #2e4966; background: linear-gradient(180deg, rgba(18,31,46,.96), rgba(8,17,29,.96)); font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; box-shadow: inset 0 -1px 0 rgba(255,255,255,.04); }
.graph-tab::before { content: ""; width: 8px; height: 18px; border-radius: 2px; background: #46617d; box-shadow: 0 0 14px rgba(70,97,125,.32); }
.graph-tab.is-active { border-color: #35c2ff; background: linear-gradient(180deg, #12324a, #0b1d2d); color: #eaf8ff; box-shadow: inset 0 -3px 0 #35c2ff, 0 0 28px rgba(53,194,255,.24); }
.graph-tab.is-active::before { background: #35c2ff; box-shadow: 0 0 18px rgba(53,194,255,.72); }
.graph-tab-title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 700; color: #f2f8ff; }
.graph-tab-kind { color: #77a7d7; font-size: 9px; letter-spacing: .12em; text-transform: uppercase; }
.graph-tab-count { color: #8fb2dc; border: 1px solid #2b4a67; background: rgba(2,8,15,.42); border-radius: 999px; padding: 2px 6px; }
#wire-status { position: absolute; left: 10px; top: 58px; z-index: 13; display: grid; grid-template-columns: auto auto minmax(0, 1fr); align-items: center; gap: 7px; max-width: min(470px, calc(100% - 24px)); min-height: 34px; padding: 7px 10px; border: 1px solid rgba(54,90,127,.78); background: rgba(7,16,28,.82); box-shadow: 0 14px 34px rgba(0,0,0,.34); border-radius: 6px; color: #93b3d7; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; pointer-events: none; }
body:not(.is-dev-mode) #wire-status { display: none; }
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
#source-view { position: absolute; inset: 0; display: none; margin: 0; padding: 20px 24px 84px; overflow: auto; color: #dbeafe; background: #07101a; border: 0; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; line-height: 1.6; white-space: pre; tab-size: 4; }
#stage.is-source #jet-canvas-view, #stage.is-source #minimap, #stage.is-source #graph-strip, #stage.is-source #wire-status, #stage.is-source #graph-overview { display: none; }
#stage.is-source #source-view { display: block; }
#minimap { position: absolute; right: clamp(8px, 1.4vw, 16px); bottom: clamp(8px, 1.4vw, 16px); width: min(210px, 22vw); height: min(132px, 15vw); min-width: 120px; min-height: 78px; border: 1px solid #365a7f; background: rgba(7,16,28,.9); box-shadow: 0 14px 42px rgba(0,0,0,.42); border-radius: 6px; }
#hud { position: absolute; left: clamp(8px, 1.4vw, 16px); bottom: clamp(8px, 1.4vw, 16px); display: flex; gap: 8px; color: #9bb4d3; font: 12px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: calc(100% - 32px); flex-wrap: wrap; }
#hud span { border: 1px solid #263b59; background: rgba(8,17,29,.88); padding: 5px 8px; border-radius: 4px; }
body:not(.is-dev-mode) #graph-meta { display: none; }
#details { height: 100%; overflow: auto; --node-accent: #35c2ff; }
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
.type-chip { color: #cfe9ff; border: 1px solid currentColor; background: rgba(255,255,255,.04); border-radius: 999px; padding: 2px 7px; font: 10px ui-monospace, "SFMono-Regular", Consolas, monospace; max-width: 98px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pin-empty { color: #7890ad; border: 1px dashed #31445d; padding: 8px; border-radius: 4px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
.pin-tools { display: grid; grid-template-columns: minmax(0, 1fr) 68px 32px; gap: 6px; margin-top: 2px; }
.pin-tools.output-pin-tools { grid-template-columns: minmax(0, 1fr) 68px 46px; }
.pin-tools [data-param-default] { grid-column: 1 / 3; }
.pin-tools input { min-width: 0; }
#context-menu { position: fixed; z-index: 30; display: none; min-width: 320px; max-width: min(430px, calc(100vw - 20px)); border: 1px solid #3b5f89; background: #091525; box-shadow: 0 18px 48px rgba(0,0,0,.55); padding: 8px; border-radius: 6px; }
#context-menu.is-open { display: grid; gap: 6px; }
#context-menu button { width: 100%; text-align: left; display: grid; grid-template-columns: 1fr auto; gap: 10px; }
#context-menu .menu-title { color: #dbeafe; font-size: 11px; letter-spacing: .1em; text-transform: uppercase; padding: 4px 6px; }
.action-palette-head { display: grid; gap: 6px; padding: 4px; border-bottom: 1px solid #203954; }
.action-palette-head input { width: 100%; border-color: #3b5f89; background: #07111f; }
.action-context { color: #8fb2dc; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; display: flex; gap: 7px; align-items: center; min-width: 0; }
.action-context .pin-port { width: 9px; height: 9px; box-shadow: 0 0 0 2px rgba(255,255,255,.07), 0 0 12px currentColor; }
.action-results { display: grid; gap: 5px; max-height: min(360px, calc(100vh - 170px)); overflow: auto; padding: 2px; }
.action-result { border-color: #284866; background: #0d1826; min-height: 44px; }
.action-result.is-favorite { border-color: #facc15; box-shadow: inset 3px 0 0 #facc15; }
.action-result:hover, .action-result:focus-visible { border-color: #35c2ff; background: #102942; }
.action-result small { color: #9aaecb; display: block; margin-top: 2px; overflow-wrap: anywhere; }
.action-empty { color: #8da4c2; padding: 9px; border: 1px dashed #31445d; border-radius: 4px; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#first-run-tour { position: fixed; inset: auto 18px 42px auto; z-index: 29; width: min(340px, calc(100vw - 36px)); display: none; gap: 10px; padding: 12px; border: 1px solid #365a7f; border-radius: 6px; background: rgba(7,16,28,.95); box-shadow: 0 22px 70px rgba(0,0,0,.5); color: #c9dcf2; }
#first-run-tour.is-open { display: grid; }
#first-run-tour b { color: #f8fbff; }
#run-hud { position: absolute; right: 12px; top: 100px; z-index: 12; display: none; min-width: 190px; padding: 8px 10px; border: 1px solid #31506d; border-radius: 6px; background: rgba(8,16,27,.88); color: #9fb9d8; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#run-hud.is-running { display: block; border-color: #facc15; color: #fef3c7; }
.lod-node { opacity: .88; }
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: .001ms !important; transition-duration: .001ms !important; scroll-behavior: auto !important; }
}
#statusbar { display: flex; align-items: center; gap: 14px; padding: 0 12px; border-top: 1px solid #23344a; background: #070b10; color: #8096b5; font: 11px ui-monospace, "SFMono-Regular", Consolas, monospace; }
#toast { margin-left: auto; color: #a7f3d0; }
@media (max-width: 1120px) {
  #workbench { grid-template-columns: minmax(142px, 18vw) minmax(0, 1fr) minmax(200px, 23vw); }
  #brand span { display: none; }
  #topbar { gap: 6px; }
  .debug-controls { flex-basis: 280px; }
  .debug-controls button, #topbar > button { padding-inline: 6px; }
}
@media (max-width: 900px) {
  #workbench { grid-template-columns: minmax(0, 1fr); }
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
    <div id="brand"><strong>Jet Canvas</strong><span>source-backed blueprint</span></div>
    <button id="graph-back" title="Back graph">‹</button>
    <button id="graph-forward" title="Forward graph">›</button>
    <select id="graph-select" aria-label="Graph"></select>
    <button id="fit">Fit</button>
    <button id="reload">Reload</button>
    <button id="source-diff">Diff</button>
    <button id="view-toggle">Code</button>
    <button id="developer-mode" title="Show Canvas internals">Developer</button>
    <button id="undo-edit">Undo</button>
    <button id="redo-edit">Redo</button>
    <button id="org-align" title="Align selected nodes">Align</button>
    <button id="org-tidy" title="Tidy visible graph">Tidy</button>
    <button id="bookmark-add" title="Bookmark graph">Mark</button>
    <button id="bookmark-jump" title="Jump to bookmark">Go</button>
    <button id="favorite-action" title="Pin first compatible action">Fav</button>
    <button id="run-current" title="Run current entry">Run</button>
    <span id="jump">loading graph</span>
    <div class="debug-controls">
      <select id="debug-session" aria-label="Debug session"><option>local debug</option></select>
      <button id="debug-break">Break</button>
      <button id="debug-watch">Watch</button>
      <button id="debug-step">Step</button>
      <button id="debug-next">Next</button>
      <button id="debug-continue">Continue</button>
      <button id="debug-stop">Stop</button>
    </div>
  </header>
  <main id="workbench">
    <aside id="left-drawer" class="side">
      <section id="project-panel" class="panel"><details open><summary><span>Project</span><span id="project-mode" class="count">file</span></summary><div id="project-rail" class="project-list"></div></details></section>
      <section id="graphs-panel" class="panel"><details open><summary><span>Functions</span><span id="graph-count" class="count">0</span></summary><div id="graph-list" class="graph-list"></div></details></section>
      <section id="search-panel" class="panel"><details><summary><span>Search</span></summary><input id="canvas-search" class="search" placeholder="Find in graph"><div id="search-results" class="search-results"></div></details></section>
    </aside>
    <section id="stage">
      <div id="canvas-dock" aria-label="Canvas panels"><button id="dock-graphs">Graphs</button><button id="dock-details">Inspector</button></div>
      <div id="graph-strip" aria-label="Graph tabs"></div>
      <div id="wire-status" aria-live="polite"><span id="wire-status-dot"></span><b>Ready</b><span>Drag from a socket or right-click the canvas</span></div>
      <div id="graph-overview" aria-label="Graph overview"></div>
      <div id="run-hud" aria-live="polite">run idle</div>
      <canvas id="jet-canvas-view" width="1400" height="900"></canvas>
      <pre id="source-view" aria-label="Jet source"></pre>
      <canvas id="minimap" width="190" height="124"></canvas>
      <div id="hud"><span id="zoom-label">100%</span><span id="graph-meta">0 nodes</span></div>
    </section>
    <aside id="right-drawer" class="side right">
      <section id="details" class="panel"></section>
    </aside>
  </main>
  <footer id="statusbar"><span id="source-id">source</span><span id="revision">revision</span><span id="schema">canvas v1</span><span id="scm-state">git</span><span id="toast"></span></footer>
</div>
<div id="context-menu" role="menu"></div>
<div id="first-run-tour" role="dialog" aria-label="Canvas first run">
  <b>Canvas uses source as truth.</b>
  <span>Use search, right-click actions, pin menus, graph bookmarks, and Run without creating graph assets.</span>
  <button id="tour-dismiss">Dismiss</button>
</div>
__JET_CANVAS_BOOTSTRAP__
</body>
</html>
"#
    .replace("__JET_CANVAS_BOOTSTRAP__", bootstrap)
}

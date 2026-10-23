const { readFileSync, writeFileSync } = await import("node:fs");
// The Rust harness appended this export line to the generated app before importing it.
writeFileSync("./.jet/build/app_stop.mjs", readFileSync("./.jet/build/app.js", "utf8") + "\nexport { jet_runtime_stop, JET_RUNTIME_STOP_METADATA };\n");
run.mjs

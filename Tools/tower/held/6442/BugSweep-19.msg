BugSweep 19 (compiler, JetDriver): the inline-Package stock carrier is `run.jet` / `src/run.jet`, not the retired `main.jet`

Base: 348c3649f1 (cut on 215fbac8cd, re-cut on df84747616, rebased by hand onto 348c3649f1 on 2026-11-04: the import now joins Loader.jet's `use { … }` block). No wave/8/in patch touches Loader.jet. BugSweep 20 (L1) and 21 (L4) also edit Loader.jet in other hunks, and the three compose in either order.

Root: `jet_driver_root_inline_carrier` (Compiler/JetDriver/Source/Driver/Loader.jet) looks for the entry root's inline `package { … }` carrier. It checks the entry itself and then the stock entry. The Jet port spelled the stock entry as `{"main.jet", "src/main.jet"}`. Rust `checked_inline_package_entry` (jet-driver Loader.rs:3228 at c9129d174d~1) scans `DEFAULT_ENTRY_FILE` = "run.jet" and `src/run.jet` (jet-foundation Syntax/package_files.rs:34). The Jet registry agrees: Jetpack Foundation Syntax.jet `DEFAULT_ENTRY_FILE :: "run.jet"`, and Compiler/JetCli Entry.jet `JET_CLI_STOCK_ENTRIES`. So `jet check other.jet` in a project with both a package.jet and a run.jet that carries `package { }` never saw the carrier and reported no E1363.

Fix: `stock_entries :: [String]{DEFAULT_ENTRY_FILE, "src/{DEFAULT_ENTRY_FILE}"}`, imported from jetpack_foundation as JetDriver ComptimePackageViews.jet already does. The comment was updated to match. `main.jet` does not occur anywhere else in the driver.

Case made green: tests/cases/inline_package/inline_package_conflicts_with_manifest_for_every_entry.case (`jet check other.jet` → exit nonzero, stderr has E1363). The case is unchanged.

Evidence:

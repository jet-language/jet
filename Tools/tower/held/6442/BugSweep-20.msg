BugSweep 20 (compiler, JetDriver + JetSema): a command home's or named entry's own command function no longer collides (E0105) with a sibling's same name

Base: 348c3649f1 (cut on 215fbac8cd, re-cut on df84747616, rebased by hand onto 348c3649f1 on 2026-11-04: dev had reworded the collision comment, "unless it imports that very function", which the new comment keeps). Main decided option (a) for L1. No wave/8/in patch touches these files. Loader.jet hunks are disjoint from BugSweep 19 and 21.

Root:
- `jet_driver_bind_package_namespaces` (Loader.jet) keeps a package-root command home's command function out of the package namespace. The command homes are `run.jet` → `run`, `build.jet`, `dev.jet` and `test.jet`. A manifest-named entry's `run` is kept out too (owner ruling B). Neither is in E0624 or `namespace_names`.
- Semantic registration did not know about the exemption. `sema_registration_namespace_collisions` (JetSema Registration/Items.jet) refuses every own declaration whose name has a namespace owner.
- So a package with `run.jet` `fn run() {…}` and `explicit.jet` `fn run() {…}` failed with E0105 "defined twice" at run.jet. explicit.jet's `run` is a namespace name; run.jet's `run` is the home's own.

Fix: one mechanism covers both exemptions.
- The loader records each member's kept-to-itself names in a new `namespace_private: [String]` on JetDriverLoadedModule (Identity.jet).
- Pipeline.jet hands it to SemaRegistrationModuleInput, and Items.jet copies it to SemaRegistrationModule.
- The collision check skips an own symbol whose name is private.
- Namespace binding and lookup already prefer a module's own symbols, so run.jet's `run` stays its own. Siblings that write `run` still reach the namespace owner.

Case made green: tests/cases/cli_parts_runtime/command_role_home_overrides_stock_and_show_default_reports_stock.case. The case is unchanged.

Evidence:

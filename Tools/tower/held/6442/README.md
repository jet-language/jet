# #6442 held patches (BugSweep lane)

The BugSweep lane stopped on 2026-10-31. Prep rebuilt its patches on
2026-11-04 from the lane transcripts, after the scratch cleanup. Each rebuilt
patch matches the lane file's size and stat. See #6442 for the recovery proof.

Every patch below applies with `git apply -p1` to dev 348c3649f1 and to dev
1b79163228. 19, 20 and 21 also compose, in that order. None of them has had a
chain, lane-check or targeted-case run on either commit.

| Item | File | Base | State | Next step |
|---|---|---|---|---|
| BS15n | `BugSweep-15n.patch` | 45280941cf (applies to 348c3649f1) | Jetpack/CLI Tests/WhyCommand.jet only: the report sits under the envelope's `why` field, and its ints are read with `foundation_json_integer`. The lane's ReportJSON.jet half is superseded by 6e912387a9. No msg | Run the Jetpack CLI tests (`WhyCommand`) and lane-check, then write the msg and land |
| BS19 | `BugSweep-19.patch`, `BugSweep-19.msg` | 348c3649f1 (rebased by hand from df84747616) | The inline-Package stock carrier is `run.jet` / `src/run.jet` (`DEFAULT_ENTRY_FILE`), not `main.jet` | Run `tests/cases/inline_package/inline_package_conflicts_with_manifest_for_every_entry` and lane-check ×2, add the Evidence section, land |
| BS20 | `BugSweep-20.patch`, `BugSweep-20.msg` | 348c3649f1 (rebased by hand from df84747616) | A command home's or a named entry's own command function is `namespace_private` and no longer collides (E0105) with a sibling's same name | Run `cli_parts_runtime/command_role_home_overrides_stock_and_show_default_reports_stock` and the `check_fix_and_format` determinism case (fixture `cli_parts_runtime/fixtures/determinism`; on dev it reports E0105 "`run` is defined twice"), then lane-check ×2, add the Evidence section, land |
| BS21 | `BugSweep-21.patch` | df84747616 (applies to 348c3649f1) | A project configuration file (package.jet, workspace.jet, env.jet) at any depth is never package source. New `jet_driver_is_project_configuration`, used by the source-catalog skip and `jet_driver_unit_is_namespace_member`. No msg | Write the msg. Run the two `tests/cases/jetpack_tool/test_*` cases, `jet check package.jet`, compiler_api_package, the workspace.jet examples and lane-check ×2 |
| BS23 | `BugSweep-23.patch` | df84747616 (applies to 348c3649f1) | Every catalog service stopped at E1317. The fix is Jetpack/Services ServiceCatalog.jet `pkg_ref` `@nixpkgs` → `@jetpack` ×41, plus the typed-catalog `packages` list in Services Tests/Services.jet and jetpack-epoch5.md:395-396. No msg | Write the msg (root: `cli_typed_plan` feeds `services_catalog_pkg_ref` into `cli_classify_all`, which refuses `@nixpkgs`, E1317 D-JPK-SNIXREUSE1). Run the Services area claims, the CLI catalog/adminer rows and lane-check ×2 |

Open #6442 rows with no patch:

- The user-facing hints that still suggest `@nixpkgs` were never started:
  - CLI AddRemoveCommands.jet:13/182, BrowserCommand.jet:91,
    DiscoveryCommands.jet:116/146, UsagePage.jet:118 and
    ToolCommand.jet:329/445;
  - Migration/Importers.jet:170, which generates `{raw}@nixpkgs`.
- The Engine area rerun. Its blocker, #6441, is fixed.
- The BuildHTTPTransport loopback port test. tests.tsv still maps
  `comptime_http_fetch_rejects_loopback_before_connecting` as `missing`.

Superseded rows (BS18, BS22, SHORTROOTS, the ReportJSON half of BS15n, and the
Foundation fails, now 92/92 at 6e912387a9) are listed in `../README.md`.

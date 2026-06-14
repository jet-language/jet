# Versioning policy

Jet follows [semantic versioning](https://semver.org/) after **v1.0.0**.

## Before 1.0

While Jet is pre-1.0, minor releases may include syntax or stdlib changes.
Examples and the language tour are the executable spec — if they still pass,
the release is consistent. Breaking changes should still be called out in
release notes.

## After 1.0

| Change kind | Version bump | Migration |
|-------------|--------------|-----------|
| Bug fix (no syntax change) | PATCH | None |
| New feature (additive syntax/stdlib) | MINOR | Optional — old programs keep compiling |
| Breaking syntax or type rule | MAJOR | Required `jet fmt` migration |

### Syntax changes require `jet fmt`

Post-1.0, any change that would break existing source must ship with a
formatter migration. **`jet fmt` is the upgrade tool** — run it after
upgrading the compiler and commit the diff.

The formatter owns layout and, when needed, mechanical syntax updates (new
keywords, renamed forms, etc.). If `jet fmt` cannot rewrite your program,
the compiler prints a diagnostic with a manual fix.

### Standard library

- Adding functions or modules: MINOR
- Changing fallible signatures or error types: MAJOR (or a new function name)
- Removing or renaming exported items: MAJOR

### Error codes

Diagnostic codes (`E0102`, etc.) are **stable forever** once published.
Wording may improve; codes are never reused or renumbered (see
[04-diagnostics](admin/04-diagnostics.md)).

### Toolchain pin

Projects may pin a Jet version in `jet.toml` under `[package].jet`. The
compiler rejects incompatible toolchains with **E1208**.

## Release artifacts

Tagged releases (`v*.*.*`) publish prebuilt `jet` binaries for common
platforms via GitHub Actions. See `.github/workflows/release.yml`.

To check for a newer release manually:

```bash
jet upgrade   # prints the latest GitHub release URL (no self-install in v1)
```

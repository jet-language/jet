# Mobile native UI and store delivery — Tower #480

**Status:** ready; all owner gates ratified 2026-07-13. D-NATIVEUI3=A ratifies
tier-1 iOS and Android targets plus first-party local signing and direct store
submission. D-NATIVEUI-ANDROID1=C ratifies one Android protocol with a direct
JNI View default and a first-party Compose adapter. D-NATIVEUI-DEV1=C ratifies
one resident native host for full applications and component previews.

## Binding frame

- D-NATIVEUI1=A: thin Core adapters call platform widget APIs. Jet does not
  introduce a second mobile UI language or own-pixels renderer.
- D-NATIVEUI2=B: iOS and Android implement the same `JetBackend` seam and the
  same View, Style, component, motion, event, and accessibility semantics.
- D-NATIVEUI3=A: native AOT targets produce installable `.ipa` and `.aab`
  artifacts. `jet ship` signs locally and talks directly to store APIs through
  a separately versioned jetpack engine.
- D-FFI-SWIFT1=A: UIKit bindings use the ratified generated Swift projection
  over the C ABI. Non-Apple hosts report the Apple SDK boundary before build.
- D-FFI-JVM1=A: Kotlin and Java libraries use the single `java.*` projection
  over JNI. Android attaches to its existing runtime; it never starts another
  JVM.
- D-BUILDTOOLCHAIN1=A: SDKs, NDKs, signing identities, store credentials, and
  generated tools are typed, hash-recorded jetpack inputs.
- D-PERSIST1: native `jet dev` preserves only compatible `@Persist` bindings
  by module path and name. No alternative state annotation is added.
- D-NATIVEUI-ANDROID1=C: `AndroidViewBackend` is the default. A selectable
  `AndroidComposeBackend` consumes the identical checked tree and event
  protocol. Both use `java.*` for Kotlin and Java library calls.
- D-NATIVEUI-DEV1=C: full-app and preview roots run in one `NativeDevHost` with
  one checked swap, activation, reconciliation, state, and audit protocol.

Neither mobile follow-up decision adds language syntax.

## Delivery order

### 1. Target and artifact contracts

Start with failing CLI, manifest, and artifact-schema tests. Extend the existing
cross-target model with `ios` and `android`; do not create a mobile-only target
registry. Resolve host, target triple, SDK, minimum OS, architecture, and output
kind before code generation. Semantic checking remains target-aware and rustc
remains a hidden verifier.

Android lowers checked TIR through the existing native backend to
`aarch64-linux-android`, links against the jetpack-pinned NDK, and packages one
deterministic `.aab`. iOS lowers through the same path to
`aarch64-apple-ios`, links UIKit through D-FFI-SWIFT1, and packages one
deterministic `.ipa`. iOS linkage runs only on Apple hardware with the licensed
SDK. Unsupported hosts receive a Jet-owned coded diagnostic with what, why,
and fix text plus a UI snapshot.

Acceptance evidence:

- deterministic artifact manifests bind source, checked TIR, target, SDK,
  tool hashes, entitlements or permissions, and package identity;
- Android emulator installs and launches the `.aab`-derived package;
- iOS simulator installs and launches the `.ipa` on a macOS runner;
- malformed target, missing SDK, wrong architecture, and foreign-tool failures
  are Jet diagnostics; raw rustc, clang, swiftc, or NDK output never escapes.

### 2. Shared mobile lifecycle contract

Specify one checked lifecycle input to the existing UI backend: create,
foreground, background, suspend, resume, memory pressure, configuration change,
and destroy. Map Android Activity and iOS application or scene callbacks into
that contract. Route navigation, safe areas, keyboard insets, restoration,
permissions, deep links, and back behavior through typed Core values rather
than platform conditionals in application code.

Build lifecycle fixtures before platform adapters. A deterministic NullBackend
trace proves event order, cancellation, restoration, and teardown. Device tests
then require byte-equivalent semantic traces after removing platform metadata.

### 3. Platform widget backends

Implement the iOS backend through D-FFI-SWIFT1 and UIKit. Implement Android's
direct JNI View backend as the default, then its first-party Compose adapter
over the same `AndroidBackend` protocol. Both consume the same checked View tree
and implement the same measure, layout, paint, event, focus, motion-clock, and
accessibility contract already used by GTK, DOM, TUI, and NullBackend.

The conformance matrix covers text, button, input, list, image, scroll,
navigation, modal, focus, keyboard, screen rotation, dynamic text size, color
scheme, reduced motion, screen-reader labels and actions, and state restoration.
Every mismatch is either fixed or recorded as a typed capability fact with an
honest diagnostic. No silent fallback and no platform-specific Jet component
surface.

### 4. Native development loop

Implement one `NativeDevHost` using the checked TIR/JIT seam, never source
rewriting or a second interpreter. Full-app mode is the default. Preview mode
selects a component root and explicit device, theme, locale, lifecycle, and
accessibility inputs. Validate the entire changed dependency closure before
activation. ABI or layout incompatibility keeps the last good app running and
offers a controlled restart. Compatible swaps are atomic, reconcile stable
widget identities, and migrate D-PERSIST1 state.

Device transport authenticates the development host, signs swap bundles, binds
receipts to source and TIR hashes, and grants only declared development
capabilities. Preview and full-app modes use production backends and the same
swap protocol. Tests cover bad edits, partial transfer,
disconnect, rollback, state migration, widget reuse, lifecycle transitions,
and accessibility environment changes.

### 5. Local signing and deterministic packaging

Implement signing in jetpack libraries behind the `jet ship` command seam.
Private keys remain local and are represented by opaque handles. Logs and JSON
output never contain key bytes, passwords, session tokens, or full credential
paths. First use explains backup and rotation before creating an Android
keystore. iOS certificate and provisioning-profile selection is deterministic
and auditable.

Test unsigned, debug-signed, release-signed, expired, revoked, wrong-team,
wrong-bundle, lost-keystore, rotated-key, and reproducible-resign cases. Verify
signatures independently with platform validators in CI. Secret scanning of
captured stdout, stderr, JSON, receipts, and artifacts is an exit gate.

### 6. Direct store submission

Use App Store Connect and Play Developer APIs directly from jetpack. Submission
starts from a signed artifact manifest and records an idempotency key, remote
application identity, release track, build number, uploaded hash, API response
hash, and final remote state. Retries resume an existing upload and never create
an accidental duplicate release.

The default ships to TestFlight or Play internal testing. Production promotion
requires an explicit channel selection and shows the exact remote change before
write. Expert policy controls account, team, track, phased rollout, review
metadata, regional availability, scheduling, and audit output without changing
the beginner command.

Contract tests run against hostile local API fakes: rate limits, expired auth,
resumable-upload loss, duplicate build numbers, policy rejection, server error,
eventual consistency, and cancellation. Final acceptance also uploads real
canary apps to both stores' non-production tracks and reads them back by hash.

### 7. End-to-end flagship proof

One source application must run unchanged on GTK, web, TUI, iOS, and Android.
Mobile proof includes install, first launch, input, background/resume, rotation,
state restoration, screen-reader tree, hot reload or preview per the ratified
model, release signing, upload, store-side readback, and clean install of the
downloaded test-track artifact.

Finish only when targeted suites and `scripts/agent/verify-full.sh` pass, docs
match runtime behavior, every diagnostic is registered and snapshot-pinned, and
the example's expected behavior is golden-tested. Schema or proof-text fixtures
alone do not satisfy device, signing, or store acceptance.

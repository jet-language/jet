# FFI decision dossier

Five full ballots accompanying [the report](ffi-owned-source-and-boundaries.md). All passed independent beginner and adversarial review after repairs and are ready in Tower. The owner permits fresh reviewers from the same model family. Readiness is not ratification; implementation of these proposed surfaces awaits the owner's choices.

## D-FFI-CONTRACT1 — One foreign-call contract with evidence for every guarantee

Should every binding share one contract that explains which safety and effect claims the actual foreign artifact supports?

A binding translates a call between languages. Its contract says who owns each value, how long it lives, what the call may do, and how it fails. A wrapper can check callers while foreign code still breaks those promises. Proof, runtime enforcement, isolation, and trust must stay distinct.

A binding is generated glue between languages. A facade is the callable interface that glue presents. An artifact is a compiled library or generated file. ABI means the machine-level calling and data-layout rules. Ownership decides who releases a value; lifetime decides when access is valid. Effects are operations such as file access. Isolation separates execution so foreign memory faults cannot reach protected host memory.

Native trust means relying on identified code to obey an audited promise. It is not proof. A runtime is the host machinery for executing code and managing objects. Runtime attachment prepares a foreign thread to enter that machinery. A component model is a common interface of values and resource handles shared by several languages.

Maya maintains a C byte-summing module in a CMake repository. Jet calls it today; C++ later calls a Jet replacement. She needs one ownership contract and honest protection claims. She cannot replace the company build system. Both options keep one canonical descriptor and both call directions; they differ in the granularity of accepted evidence.

CXX checks paired C++ and Rust declarations, but its unsafe C++ declaration still asserts implementation safety. RLBox adds isolation and requires validation of returned values. These are complementary forms of evidence. Sources: https://cxx.rs/extern-c%2B%2B.html and https://rlbox.dev/.

### Current experience

Current pointer/count shape for the same byte-sum fixture; no fresh runtime claim.

```
// sum_bytes(const uint8_t *bytes, size_t count) -> uint64_t
bytes := [U8]{10, 20, 30}
print(sums.sum_bytes(&bytes, bytes.len())) // 60
```

### Comparison: Rust / CXX

Illustrative CXX bridge for the same sum; its unsafe declaration asserts implementation obligations.

```
// CXX bridge syntax: https://cxx.rs/extern-c%2B%2B.html
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("sum.hpp");
        fn sum_bytes(bytes: &[u8]) -> u64;
    }
}
// C++ implementation must obey the declared slice contract.
```

### A. One contract, evidence per obligation — recommended

Extend the existing Jet descriptor. Each ownership, lifetime, effect, and failure fact carries its basis and artifact coverage. Generate both language facades from those facts. Source analysis and compiler integration remain optional inputs to the same binding generator. A missing fact stays unknown; native trust needs its existing audited boundary.

```
use "sum.h" as sums
fn run() {
    bytes := [U8]{10, 20, 30}
    print(sums.sum_bytes(&bytes))
}
// PROPOSED; output 60 when the selected evidence is accepted.
// Inspection shows each obligation and its evidence.
```

### B. One contract, whole-module acceptance

Keep the same shared descriptor, but accept implementation evidence only for a complete module profile. Every exported operation must satisfy that profile before safe projection. A module-wide audited profile is still trust, not proof. This gives one uniform admission result but withholds covered operations when another operation lacks evidence.

```
use "sum.h" as sums
fn run() {
    bytes := [U8]{10, 20, 30}
    print(sums.sum_bytes(&bytes))
}
// PROPOSED; output 60 when the selected evidence is accepted.
// Inspection admits or rejects the complete module profile.
```

### Exact behavior and edge cases

Preserve D-FFI-UNIFY1, D-FACT-LAW1, D-ONCE-LAW1, I1, I3, and I9. Clarify D-MEM-GUARANTEE1 using the residual foreign-pointer limit already recorded by D-HARDENED1. Claims bind to headers, source, compiler options, target, generator, loaded artifacts, and reachable native edges. Source beside a binary is not evidence that the binary implements that source. Build sandboxes are not runtime effect sandboxes. A process boundary protects memory only within its stated threat model. No dependency or execution-tier exception is approved by this choice.

The same program in both options sums three bytes and prints 60. Its C declaration is uint64_t sum_bytes(const uint8_t *bytes, size_t count). The complete supported implementation reads indices below count, retains nothing, and returns the unsigned sum. Empty input returns zero. Both options preserve that program; Option B additionally requires the rest of the module profile to pass.

Admission outcomes, PROPOSED:

**Checked proof or complete runtime enforcement**. Emit the ordinary facade for the covered operation.

**Isolation with established API equivalence**. Emit the facade with the selected isolated failure contract.

**Already vetted standard-library implementation**. Use its existing approved trust scope.

**Accepted native contract without implementation proof**. Keep the generated binding directly callable and label implementation coverage TRUSTED outside hardening. Never label that trust as proven.

**Unknown retention, validity, thread, or required authority fact**. Reject the safe projection before execution.

**Source file beside an unmatched binary**. Reject reuse of implementation evidence; matching filenames are insufficient.


The generator may keep native arity only when the remaining safety obligations already pass. It does not run an unsafe call to discover whether the contract was true.

Source-backed inspection can report: count rule proved; pointer lifetime proved; artifact matched; native placement unchanged. Binary-only inspection can report: count rule proved relative to the supplied contract; implementation retention TRUSTED; direct generated call permitted outside hardening, but not claimed as implementation-safe. A proof of translation is not a proof of the binary.

A missing-fact diagnostic says: Cannot borrow bytes for this call because the foreign implementation may retain the pointer. Supply a checked retention contract, select a valid owning transfer, or use an explicit audited native boundary. An unknown effect remains unknown foreign behavior; it never becomes pure.

Reverse direction uses the same obligation. A generated C++ sums::sum_bytes facade calls an eligible Jet export, checks native inputs before constructing Jet values, and preserves the sum. It does not make unrelated C++ host code safe. A frozen project rejects an unmatched replacement artifact before loading it.

Both choices preserve D-FFI-UNIFY1, D-FACT-LAW1, and D-ONCE-LAW1. Option B is a different evidence-admission policy on the same contract, not permission for duplicated semantic authorities.

Generated compiler-vetted bindings remain directly callable under D-FFI-UNIFY1 and D-FFI-CAP1. An asserted native contract remains visibly TRUSTED; it is not implementation proof. Raw symbols outside bindings and unsupported capability escapes still require user-written #Unsafe. Under D-MEM-GUARANTEE1 and D-HARDENED1, hardening cannot leave a dependency TRUSTED; missing required containment rejects that profile.

Preserve D-EFF4/D-EFF5 and each binder's language leaf. Every reached foreign call retains FFI.<Lang>, even when its native body is proved pure. The descriptor also includes established reachable callback and cleanup effects. Unknown native behavior remains marked as foreign and unverified; a declared row alone does not enforce native authority.

Existing guarantee labels remain the public vocabulary. A discharged static obligation is proven; a checked operation is watched; an established containment boundary is fenced. An asserted implementation obligation is TRUSTED. An operation may carry several labels for different obligations; one proven count never upgrades an unchecked native body. Unknown descriptor facts still block unsupported safe projections; an accepted explicit contract may establish a TRUSTED assumption.

### Recommendation

One contract preserves Jet semantics across both directions while making the foreign implementation boundary explicit.

- One semantic contract across languages and call directions.
- Inspect each guarantee without mistaking a wrapper for whole-library safety.
- Use stronger evidence without taking over existing builds.

**Why not B:** Whole-module admission withholds a covered operation because an unrelated operation lacks evidence. Per-obligation evidence still composes over every reachable edge.

**Remaining limit:** Opaque or uncovered native operations can remain unresolved. Without an accepted contract, proof, enforcement, or valid isolation, the missing obligation has no established basis.

Accepted native assertions remain TRUSTED outside hardening; unresolved obligations still block projections that need them.

### Review status

Fresh agent: /root/ffi_beginner_review. Skill: rli5. The reader verified the TRUSTED dial, hardening, raw escapes, durable FFI leaves, and callback or cleanup effects.

Author model family: OpenAI. Adversarial model family: OpenAI. Fresh agent: /root/ffi_sol_review. Verified the matched byte-sum workload, proven/watched/fenced/TRUSTED coverage, and preserved foreign, callback, and cleanup effects.

## D-FFI-AUTO1 — Automatic API improvements with inspect, native shape, freeze, and refusal

What evidence permits a simpler generated call, and how can a team inspect or freeze that choice?

C often passes a buffer and its length separately. A generator can supply the length, but it must know its unit, range, and meaning. A shorter call is correct only if it preserves the original operation. Tests show examples; an established adaptation rule explains why the translation preserves behavior. The same rule can lift an established resource protocol into a method; unrecognized protocols stay low-level.

A binding is generated glue between languages. A facade is the callable interface that glue presents. An artifact is a compiled library or generated file. ABI means the machine-level calling and data-layout rules. Ownership decides who releases a value; lifetime decides when access is valid. Effects are operations such as file access. Isolation separates execution so foreign memory faults cannot reach protected host memory.

Native trust means relying on identified code to obey an audited promise. It is not proof. A runtime is the host machinery for executing code and managing objects. Runtime attachment prepares a foreign thread to enter that machinery. A component model is a common interface of values and resource handles shared by several languages.

Ravi imports a C function that sums bytes. He wants the length supplied automatically. A specialist needs the native argument list. Their CI must reject an unreviewed binding change. Both options keep automatic generation and the existing build system; they differ in what evidence allows shortening the call.

Swift safe C++ interop uses bounds and lifetime information to offer safer wrappers, including external annotations. Some wrapper facilities remain experimental. Such annotations describe a contract; they do not verify arbitrary C++ bodies. Source: https://www.swift.org/documentation/cxx-interop/safe-interop/.

### Current experience

Source-based current behavior or current approved shape; no fresh runtime claim.

```
// Current-shaped pointer/count call.
bytes := [U8]{10, 20, 30}
print(sums.sum_bytes(&bytes, bytes.len()))
```

### Comparison: C++

The same sum operation can express its extent as a native C++ span. The function body still owes that contract.

```
#include <span>
#include <cstdint>
uint64_t sum_bytes(std::span<const uint8_t> bytes);
// Callers supply one bounded view.
```

### A. Prove the adaptation, check its conditions — recommended

Use an established rule for each API adaptation. Derive the count only when the contract fixes its unit and meaning; check native width and buffer extent. Keep implementation safety evidence separate. Document missing facts and preserve a safe native shape when possible. Pin the selected plan. No unsupported copying, retry, placement change, or error remapping. Established error translations may be automatic.

```
// PROPOSED default; output 60.
bytes := [U8]{10, 20, 30}
print(sums.sum_bytes(&bytes))
// Expert commands, PROPOSED:
// jet inspect bind sums --explain
// jet bind sums --shape native
// jet bind sums --freeze
// jet bind --policy frozen
```

### B. Require complete body proof before reshaping

Generate safe native-shaped bindings first. Shorten the same call automatically only after proving the entire reachable implementation obeys its contract. Provide the same inspection and frozen controls. This uses one stronger threshold for implementation safety and API adaptation, even when only the latter needs a local proof.

```
// ALTERNATIVE until complete body proof; output 60.
bytes := [U8]{10, 20, 30}
print(sums.sum_bytes(&bytes, bytes.len()))
// The count disappears only after whole-implementation proof.
// Existing binary APIs usually retain native arity.
```

### Exact behavior and edge cases

This choice preserves D-FFI-CAP1: a borrowed buffer is exclusive for the call and cannot be retained. Raw C declarations retain D-CABI-RESULT1; only an ordinary generated Jet wrapper maps established native failures.

Inspection stays read-only. Mutating bind commands write package.jet and the existing .jet/lock system. --shape native selects one public shape. --freeze records the resolved plan. --policy frozen persists a project rule that rejects drift on ordinary builds. Changes require explicit update and review. A solver timeout cannot weaken guarantees or select another runtime.

Frozen inputs include the target and actual artifact. Initial discovery may use existing package metadata; ambiguity requires an explicit library selection.

Proposed explicit discovery: jet bind sums --header sum.h --library build/libsum.a --compile-commands build/compile_commands.json. This records a pairing, not a source-to-binary proof. The existing resolver supplies ordinary package identities. Frozen updates must be explicit and reviewable.

Evidence threshold: prove each adaptation relative to named assumptions, then require an accepted basis for those assumptions. Compiler-checked facts or complete runtime checks can discharge them. Vendor annotations can state a contract but remain trust until verified or enforced. Names, signatures, signatures on packages, and passing tests do not establish the native body. A signature on a package proves origin, not behavior.

A lying binary that retains a borrowed pointer is not stopped by count removal. Without implementation coverage or approved trust, safe borrowing is rejected. Generated compiler-vetted bindings remain directly callable under D-FFI-UNIFY1 and D-FFI-CAP1. An asserted native contract remains visibly TRUSTED; it is not implementation proof. Raw symbols outside bindings and unsupported capability escapes still require user-written #Unsafe. Under D-MEM-GUARANTEE1 and D-HARDENED1, hardening cannot leave a dependency TRUSTED; missing required containment rejects that profile.

Exact update lifecycle, all commands PROPOSED:

```sh
jet inspect bind sums --explain
jet bind sums --shape native
jet bind sums --freeze
jet bind --policy frozen
jet bind sums --update --preview
jet bind sums --update --accept <candidate-digest>
```

Preview prints a semantic diff and candidate digest, using the existing generated cache without changing project files. Accept rechecks unchanged inputs and atomically publishes the matching package.jet configuration and .jet/lock plan. Failure leaves the prior pair valid. Exit 0 means success, 1 means contract rejection or frozen drift, and 101 remains an internal compiler error.

The diff names API shape, count units, ownership, failure mapping, copies, placement, effects, cleanup, and artifact evidence. Frozen drift says: sums changed after its plan was frozen; the old bounds claim no longer applies; run jet bind sums --update --preview. No build implicitly accepts that candidate.

**Empty sum buffer**. Pass count zero; return zero, without a dereference.

**Sum buffer length exceeds native size_t**. Reject before entering C; no truncation.

**Integer might count bytes or elements**. Reject count removal; retain a safe native shape only if all safety obligations still pass.

**Native shape explicitly passes count 2 for a three-byte buffer**. Read the valid prefix and return 30.

**Explicit count exceeds the permitted view**. Reject before the call.

**Artifact changes under frozen policy**. Exit 1 without executing or rewriting the plan.


For the prefix example, the expert writes sums.sum_bytes(&bytes, 2) after selecting native shape. The default sums.sum_bytes(&bytes) means the entire supplied view, never an inferred shorter request.

Error mapping is automatic only when success states, failure states, initialized outputs, native error payloads, partial mutation, and cleanup order are established. The zlib wrapper preserves short successful reads and the distinct native error. Native shape retains original status/out parameters. A destructor cannot swallow a fallible close result. Raw C declarations still obey D-CABI-RESULT1.

The sum facade returns U64, not an extra fallible wrapper. A statically known count violation is a compile-time Jet diagnostic. A dynamic width or extent violation follows Jet's existing checked-boundary panic policy before entering C. It is a caller-contract violation, not a native library error. Production must register its diagnostic and snapshot. Valid calls preserve the U64 result; established native failures use their separately declared fallible mapping.

Protocol lifting is included in this choice. A supported operation sequence may generate an owning constructor or method when ordering, status, partial work, output initialization, and cleanup are established. An external PNG binding package may propose png.Image.open(path, format: .RGBA). This is a generated operation, not a libpng symbol. Unrecognized protocols remain lower-level operations; names and AI guesses cannot establish them.

The generated PNG operation preserves decoder initialization, explicit RGBA selection, output allocation, decode completion, native failure details, and decoder cleanup on every specified path. It returns an owning image or the corresponding typed failure. It does not silently change formats or replace libpng's algorithm. Source/body evidence remains separate from proof of the protocol translation.

The four-arrangement UX adds these proposed binding inputs: jet bind frames --source native/frame --language cpp --std c++20, and jet bind png --package c@nixpkgs:libpng. The first lets Jet build selected owned source. The second uses the existing resolver and lock without requiring source ownership. Both record the same canonical contract and inspectable plan. No implicit build takeover occurs.

Preserve D-EFF4/D-EFF5 and each binder's language leaf. Every reached foreign call retains FFI.<Lang>, even when its native body is proved pure. The descriptor also includes established reachable callback and cleanup effects. Unknown native behavior remains marked as foreign and unverified; a declared row alone does not enforce native authority.

The following package.jet fields are PROPOSED public configuration under this ballot. They extend the existing package model and D-TEAMPOLICY1; no second plan store is introduced.

```jet
bindings: .{
    sums: .{ shape: .Native, frozen: true }
}
policy: .{ bindings: .Frozen }
```

The shape field is .Automatic by default; .Native requests native arity. A binding defaults to frozen: false. The project binding policy is .Automatic by default; .Frozen requires recorded plans for every reached binding. Omitted fields use those defaults. These fields merge into the existing package records rather than replacing other policy entries.

The --shape native command writes bindings.sums.shape; --freeze writes bindings.sums.frozen and the resolved .jet/lock plan. The --policy frozen command writes policy.bindings. Manual edits and commands are equivalent and use the same validator. The lock contains resolved evidence, not another user policy.

This explicitly extends D-PACKAGE-POLICY-SCOPE1: binding-plan controls are package/artifact facts, with no lexical #Policy mirror. Single-file use keeps the same lock and needs no manifest:

```sh
jet bind script.jet --freeze
jet run script.jet --bind-policy frozen
```

Here bind records that script's reached plans in .jet/lock without creating package.jet. The run flag is invocation-only; missing plans or drift fail before execution. Project and single-file forms use the same plan and refusal law.

The png.Image.open spelling is an illustrative generated package API, not a universal reserved name. This ballot approves the established-protocol lifting rule and its guarantees, not one global image API.

### Recommendation

Prove the transformation that actually occurs, enforce its conditions, and report implementation safety separately.

- Established counts and resource protocols become ordinary calls.
- Experts can preserve native shape without weakening memory checks.
- Frozen projects reject unreviewed changes.

**Why not B:** Whole-body proof couples two different questions and blocks established local improvements without strengthening the local translation rule.

**Remaining limit:** Ambiguous APIs need an additional contract fact. The generator cannot know whether an integer counts bytes, elements, or a requested prefix without evidence.

An ambiguous API still needs evidence. Automatic generation may not guess units, ownership, retention, or error meaning.

### Review status

Fresh agent: /root/ffi_beginner_review. Skill: rli5. The reader verified exact configuration, lock ownership, single-file refusal, protocol lifting, updates, and failure behavior.

Author model family: OpenAI. Adversarial model family: OpenAI. Fresh agent: /root/ffi_sol_review. Verified count and protocol equivalence, low-level fallback, exact inspection, native shape, freezing, updates, and package-policy controls.

## D-FFI-GUEST1 — Native host facades from one Jet export contract

Should generated host APIs preserve native language types through adapters, or use one component-shaped interface everywhere?

When C++ or Python calls Jet, Jet is the guest. The host still owns its build and runtime. A generated facade translates Jet exports into familiar host types. One shared contract can drive several native facades, or every host can use a narrower common component type model.

A binding is generated glue between languages. A facade is the callable interface that glue presents. An artifact is a compiled library or generated file. ABI means the machine-level calling and data-layout rules. Ownership decides who releases a value; lifetime decides when access is valid. Effects are operations such as file access. Isolation separates execution so foreign memory faults cannot reach protected host memory.

Native trust means relying on identified code to obey an audited promise. It is not proof. A runtime is the host machinery for executing code and managing objects. Runtime attachment prepares a foreign thread to enter that machinery. A component model is a common interface of values and resource handles shared by several languages.

Maya replaces a C++ document module with Jet. Native callers own bytes, keep a checked view, replace storage, detect expiration, and close. Both choices preserve those observations; they differ in native facades versus a universal component-shaped interface.

CXX preserves selected native C++ and Rust types through generated glue. WIT defines a common component interface with owned and borrowed resources. The choice concerns which model is universal, not whether components can be supported. Sources: https://cxx.rs/ and https://component-model.bytecodealliance.org/design/wit.html.

### Current experience

Illustrative current workaround: maintain this host facade by hand over C exports; Jet does not yet generate this rich interface.

```
#include "manual_rules.hpp" // Handwritten owner/view glue.
#include <cassert>
int main() {
    auto d = rules::Document::open({10,20,30}).value();
    auto v = d.bytes();
    assert(v.at(1).value() == 20);
    d.replace({40,50}).value();
    assert(v.at(1).error() == rules::Error::ExpiredView);
    assert(d.bytes().at(1).value() == 50);
    d.close().value();
}
```

### Comparison: WebAssembly component model / WIT

Illustrative WIT contract for the same document, invalidating view, and close protocol.

```
// WIT resource syntax: https://component-model.bytecodealliance.org/design/wit.html
package demo:rules;
interface rules {
  resource document;
  resource view;
  variant error { allocation, bounds, expired-view, closed }
  open: func(bytes: list<u8>) -> result<document, error>;
  bytes: func(doc: borrow<document>) -> view;
  replace: func(doc: borrow<document>, bytes: list<u8>) -> result<_, error>;
  at: func(v: borrow<view>, index: u64) -> result<u8, error>;
  close: func(doc: document) -> result<_, error>;
}
```

### A. Host-native facades over stable bridges — recommended

Extend the existing Library output and export rows. Generate C/C++/Rust/Zig/Go/Python/JavaScript facades over selected C-compatible shims and supported host-runtime interfaces. A component projection remains available where it fits. Preserve identity and lifetime where expressible; require explicit adaptation otherwise.

```
#include "rules.hpp"
#include <cassert>
int main() {
    auto d = rules::Document::open({10,20,30}).value();
    auto v = d.bytes();
    assert(v.at(1).value() == 20);
    d.replace({40,50}).value();
    assert(v.at(1).error() == rules::Error::ExpiredView);
    assert(d.bytes().at(1).value() == 50);
    d.close().value();
}
```

PROPOSED host test bodies for the same resource protocol follow. Generated package imports and the host test function supply their normal context. Every fixture observes 20, ExpiredView, then 50, and closes the owner. All generated names here remain proposal API.

**C**

```c
uint8_t input[] = {10,20,30}, next[] = {40,50}, out;
RulesDocument d; RulesView v, fresh;
assert(rules_document_open(input,3,&d) == RULES_OK);
assert(rules_document_bytes(&d,&v) == RULES_OK);
assert(rules_view_at(&v,1,&out) == RULES_OK && out == 20);
assert(rules_document_replace(&d,next,2) == RULES_OK);
assert(rules_view_at(&v,1,&out) == RULES_EXPIRED_VIEW);
assert(rules_document_bytes(&d,&fresh) == RULES_OK);
assert(rules_view_at(&fresh,1,&out) == RULES_OK && out == 50);
assert(rules_document_close(&d) == RULES_OK);
```

**C++**

```cpp
#include "rules.hpp"
#include <cassert>
int main() {
    auto d = rules::Document::open({10,20,30}).value();
    auto v = d.bytes();
    assert(v.at(1).value() == 20);
    d.replace({40,50}).value();
    assert(v.at(1).error() == rules::Error::ExpiredView);
    assert(d.bytes().at(1).value() == 50);
    d.close().value();
}
```

**Rust**

```rust
let mut d = rules::Document::open([10,20,30])?;
let v = d.bytes();
assert_eq!(v.at(1)?, 20);
d.replace([40,50])?;
assert_eq!(v.at(1), Err(rules::Error::ExpiredView));
assert_eq!(d.bytes().at(1)?, 50);
d.close()?;
```

**Zig**

```zig
var d = try rules.Document.open(&.{10,20,30});
const v = d.bytes();
try std.testing.expectEqual(@as(u8,20), try v.at(1));
try d.replace(&.{40,50});
try std.testing.expectError(error.ExpiredView, v.at(1));
try std.testing.expectEqual(@as(u8,50), try d.bytes().at(1));
try d.close();
```

**Go**

```go
d, err := rules.OpenDocument([]byte{10,20,30}); if err != nil { panic(err) }
v := d.Bytes()
x, err := v.At(1); if err != nil || x != 20 { panic("first view") }
if err = d.Replace([]byte{40,50}); err != nil { panic(err) }
_, err = v.At(1); if !errors.Is(err, rules.ErrExpiredView) { panic("expiry") }
x, err = d.Bytes().At(1); if err != nil || x != 50 { panic("fresh view") }
if err = d.Close(); err != nil { panic(err) }
```

**Python**

```python
with rules.Document.open(bytes([10,20,30])) as d:
    v = d.bytes()
    assert v.at(1) == 20
    d.replace(bytes([40,50]))
    try:
        v.at(1)
    except rules.ExpiredView:
        pass
    else:
        raise AssertionError("view should expire")
    assert d.bytes().at(1) == 50
# Context exit closes exactly once.
```

**Node**

```js
const d = await rules.Document.open(Buffer.from([10,20,30]));
try {
  const v = d.bytes();
  assert.equal(await v.at(1), 20);
  await d.replace(Buffer.from([40,50]));
  await assert.rejects(v.at(1), rules.ExpiredView);
  assert.equal(await d.bytes().at(1), 50);
} finally {
  await d.close();
}
```

These fixtures select generation-checked view handles, not raw pointers. Replace or close changes the owner generation; stale access returns ExpiredView before touching freed storage. Views do not own foreign storage and need no separate foreign close. Host-managed handle bookkeeping is released by each facade; C view tokens are non-owning values.

Rust may instead select a true borrow projection that rejects replace while the borrow remains live. That stronger static shape is explicit because it changes which source programs compile. C, C++, Zig, Go, Python, and Node use the stated runtime invalidation checks in this fixture. Go and Python close reuse is checked; Rust ownership can reject consumed-owner reuse statically. Node shows an explicitly selected asynchronous facade; scheduling is recorded, never silently changed.

### B. Component model for every facade

Project every export through one shared component-shaped type system with records, variants, lists, and resources. Generate each host facade from that interface. The sum example remains simple. Native identity-bearing views and runtime-specific types need explicit adapters into that common model.

```
#include "rules_component.hpp"
#include <cassert>
int main() {
    auto d = rules::open_document({10,20,30}).value();
    auto v = rules::open_view(d);
    assert(rules::view_at(v,1).value() == 20);
    rules::replace(d,{40,50}).value();
    assert(rules::view_at(v,1).error() == rules::Error::ExpiredView);
    assert(rules::view_at(rules::open_view(d),1).value() == 50);
    rules::close_document(d).value();
}
```

### Exact behavior and edge cases

Extend D-ADOPT-GUEST1 and D-LIB-EXPORT1 without reviving retired #Extern syntax. Preserve D-FFI-CPP1, D-FFI-PY1, D-FFI-JS1, and D-FFI-GO1 runtime defaults. Rust native ABI is not assumed stable; use selected monomorphized bridge operations. Python and Node packages must root objects and preserve error distinctions. Go follows cgo pointer rules. Add real Zig discovery, target handling, packaging, and conformance. Existing #1345, #1347, and #1348 retain packaging, host-build, and mixed-repo DX ownership. A safe Jet guest does not make arbitrary in-process host code memory-safe. Browser or embedded restrictions need explicit applicability decisions.

An eligible export has a lossless mapping for the selected host, or an explicitly selected representation change. If one requested projection lacks a mapping, generation reports that projection and symbol as unsupported. A build requiring all selected outputs fails atomically. It never silently copies an identity-bearing value or publishes a partial valid-looking package.

The richer fixture owns bytes, lends a view, replaces its storage, reports expired views, and closes. Both alternatives assert the same observations: 20, an expired-view error, and 50; they print nothing. Opening or replacing can return AllocationFailure; indexed access can return BoundsFailure or ExpiredView. Close succeeds once and invalidates every view. Reusing a consumed owner is rejected or checked by the host facade.

Option A, proposed C++ facade:

The complete code appears in the corresponding option above.

Option B, proposed component-shaped resource API:

The complete code appears in the corresponding option above.

Both use an owner and invalidation identity. Option B represents the view as a resource operation protocol, not an untracked list snapshot. A component projection may preserve lifetime through handles; it is not inherently incapable of safe views. Option A can additionally integrate compatible native views directly. Neither choice earns a zero-copy claim without checking its actual bridge.

**C**. rules.h plus static/shared library. Selected C ABI; Make, CMake, or existing linker. int64_t and explicit tagged result records.

**C++**. rules.hpp plus selected shim. C-compatible shim; existing CMake/compiler. int64_t, RAII resources, checked result.

**Rust**. game_rules crate. Selected C-compatible operations; Cargo build script. i64, owned/borrowed wrappers, Result.

**Zig**. rules.zig module. Selected C ABI; existing build.zig. i64, checked resource operations, error union.

**Go**. rules Go package. cgo and permitted handles; go build. int64, values plus error.

**Python**. game_rules package, extension, and stubs. Supported CPython interface; existing wheel/build backend. Checked Python int and mapped exception classes.

**Node**. game-rules package, addon, and declarations. Node-API; existing package scripts. bigint for the I64 smoke and a lossless exact-Int facade, typed errors or explicit Promise outcomes.


These artifact names and host APIs are proposed public projections. CMake still invokes the canonical Jet build path through jet_library. Cargo's build.rs invokes that same path and declares generated output dependencies. Python's existing build backend packages the generated extension. Node scripts build the addon. Go and Zig host builds consume their generated modules and selected library. Jet does not take over those build graphs.

The CMake baseline uses three build declarations:

```cmake
find_package(Jet REQUIRED)
jet_library(rules ENTRY library.jet OUTPUT core LIBRARY rules LOADABLE)
target_link_libraries(game PRIVATE rules)
```

 The host owns its runtime, scheduler, test runner, and release. Runtime failures map through the selected facade; an unrelated native host memory fault is outside an in-process Jet guest guarantee.

The same contract supports four project arrangements. Jet applications may use owned foreign source or external packages. Foreign projects may host a Jet guest or select Jet's compiler or build driver. Build ownership is independent of whether native facades or component-shaped facades are selected.

The compiler route uses the existing jet-cc and jet-c++ driver concept with the host build unchanged. Stronger source checks and evidence are proposed additions. The driver uses the relevant native front end; it does not claim to reinterpret arbitrary C++ as Jet.

Full build import is a separate owner choice, D-FFI-BUILD1 on #2961. This ballot does not approve its commands or graph-ownership policy.


The scalar smoke export is PROPOSED #Export(c) pub fn on_tick(dt: I64) I64 -> dt + 1, with input 41 and output 42. C and C++ use int64_t; Rust and Zig use i64; Go uses int64. Python int and Node bigint are checked against I64 before entry. I64 overflow follows Jet's existing checked arithmetic and native-boundary panic policy; it never wraps or unwinds through an incompatible ABI.

Exact Jet Int is not I64. A general Int export needs a lossless owning BigInt/JetInt facade or is rejected for that projection. An explicitly selected fixed-width narrowing adapter must return a tagged/fallible result for input or result overflow. A plain fixed-width return cannot silently truncate an exact Int.

### Recommendation

Use the existing export model while preserving native integration where it is valid. Offer components as a projection of the same contract.

- Incremental migration preserves native host types and tools.
- One eligible export contract drives every requested projection.
- Components remain an option without limiting every native call.

**Why not B:** A universal component-shaped boundary narrows native integration before the program requires that tradeoff.

**Remaining limit:** Some native types lack a lossless counterpart in the selected host. A checked handle or owned copy can change identity, invalidation, or failure observations; it cannot silently replace a true native borrow.

A host cannot express every foreign lifetime or runtime rule. Generate checked handles or require an explicit owned conversion when necessary.

### Review status

Fresh agent: /root/ffi_beginner_review. Skill: rli5. The reader verified the seven-host resource protocol and the distinction between I64 and lossless or fallible exact Int projections.

Author model family: OpenAI. Adversarial model family: OpenAI. Fresh agent: /root/ffi_sol_review. Verified the seven-host document protocol, checked handles versus Rust borrows, I64 mapping, and rejection of lossy exact-Int projections.

## D-FFI-CALLBACK2 — Captured and asynchronous callbacks with explicit registration lifetimes

How should generated bindings support callbacks that retain state beyond a single native call?

A callback lets foreign code call Jet later. A closure also carries captured values. Those values must remain alive until every callback finishes. Cancellation and unregistration need a contract: stopping future callbacks does not automatically stop one already running or undo its effects.

A binding is generated glue between languages. A facade is the callable interface that glue presents. An artifact is a compiled library or generated file. ABI means the machine-level calling and data-layout rules. Ownership decides who releases a value; lifetime decides when access is valid. Effects are operations such as file access. Isolation separates execution so foreign memory faults cannot reach protected host memory.

Native trust means relying on identified code to obey an audited promise. It is not proof. A runtime is the host machinery for executing code and managing objects. Runtime attachment prepares a foreign thread to enter that machinery. A component model is a common interface of values and resource handles shared by several languages.

Sam subscribes to a native byte source and prints a prefix with every event. The source retains the callback. Sam then unsubscribes and releases the captured state. The program must print each accepted event once and never call released state. The existing pure, capture-free C callback rule cannot express this directly.

Rust FFI guidance distinguishes synchronous callbacks from asynchronous ones and requires preventing calls after the target is destroyed. The generator must enforce that lifecycle, including foreign threads. Source: https://doc.rust-lang.org/nomicon/ffi.html.

### Current experience

Current supported subset cannot generate this retained captured notification; a native trampoline needs handwritten lifetime glue.

```
// Same byte-notification workload, current limitation:
prefix :: "byte"
// A retained callback capturing prefix and printing needs
// native context ownership and deregistration glue today.
// It is outside the approved pure capture-free callback subset.
// Desired output: byte: 7, then stopped.
```

### Comparison: C

Native registration makes retained context and its stop operation explicit. The implementation must define whether stop drains active calls.

```
typedef void (*on_byte)(void *ctx, int value);
subscription *subscribe(on_byte callback, void *ctx);
void emit(int value);
void unsubscribe(subscription *s);
```

### A. Managed registration with explicit completion — recommended

Generate an owning registration and a borrowed event for each invocation. The event can request stop without waiting on itself. Unsubscribe consumes the registration and returns a task that completes after native shutdown acknowledgment and all accepted calls finish. Captures remain alive until then. Validate thread, reentrancy, effect, and failure obligations before exposing this facade.

```
// PROPOSED; output byte: 7, then stopped.
prefix :: "byte"
sub :: source.on_data(event -> {
    print("{prefix}: {event.value}")
    event.stop()
})
emitted :: source.emit_async(7)
(emitted.join() ?? panic("task failed")) ?? panic("emission failed")
stopped :: source.unsubscribe(^sub)
(stopped.join() ?? panic("task failed")) ?? panic("unsubscribe failed")
print("stopped")
```

### B. Owned messages for retained notifications

Generate owned queued notifications for sources with an established backpressure contract. This fixture has capacity one, FIFO order, and an explicit full-queue error. It never blocks the synchronous producer or silently drops events. Stop closes admission and acknowledges native shutdown; accepted events remain drainable. Delivery placement changes, so this is an explicit API choice.

```
// ALTERNATIVE queue fixture; output byte: 7, then stopped.
prefix :: "byte"
events :: source.notifications(capacity: 1)
source.emit(7) ?? panic("queue full")
value :: events.next() ?? panic("source closed")
print("{prefix}: {value}")
stopped :: source.stop(^events)
stopped.join() ?? panic("stop failed")
print("stopped")
```

### Exact behavior and edge cases

Explicitly amend D-CABI-CALLBACK1. Pure capture-free callbacks remain the current approved subset until acceptance. Generated event, registration, and completion APIs reuse Jet ownership, effects, and tasks. No new TaskFailure variant is introduced. No new language keyword or dependency is approved.

The fixture is a byte source that can acknowledge deregistration and support producer backpressure. In Option A, emit_async completes after its one accepted callback finishes. In Option B, emit enqueues a value or returns QueueFull. Both examples print byte: 7 and then stopped. Queue projection is rejected for a native source that cannot support its declared policy.

**Active**. Call admission atomically checks this state and acquires an in-flight reference before invoking user code.

**Stopping**. No new user invocation is admitted. Previously accepted calls may finish with captures retained.

**event.stop()**. Requests Stopping without waiting; valid only within the borrowed event's invocation.

**unsubscribe(^sub)**. Consumes the owner, requests stop, and returns a task. An earlier event stop does not invalidate this final owner action.

**Stop completion**. Native acknowledgment prevents future trampoline entries and no accepted invocation remains active. Release captures and trampoline storage only then.

**Stop failure**. Report the failure and retain state needed to prevent invalid access. Do not pretend release succeeded.

**Self-dependent join**. The borrowed event exposes only nonblocking stop; it cannot reach the owning registration or its completion tasks. Sema rejects capturing or publishing those handles into that callback's reachable state. Ordinary unrelated task cycles retain existing task law.


Self-stop is shown in Option A: event.stop requests cancellation from inside its own callback. The owning subscription remains outside and is consumed exactly once by unsubscribe. No owning handle is lost on a self-stop error, because requesting stop does not consume that handle.

An accepted call has acquired its in-flight reference while Active. Draining means waiting until all such calls finish. Native deregistration acknowledgment is separate: the foreign source must promise no future trampoline entry before its storage can be freed. A wrapper cannot infer that promise from a stop function's name.

**Stop wins before admission**. No user callback runs; the emission reports Stopped.

**Admission wins before stop**. The callback may run once; its completed effects remain.

**Callback requests stop**. It returns normally; shutdown completes after that invocation exits.

**Native thread enters**. Vetted runtime attachment precedes user code; incompatible thread-affine captures are rejected.

**Native source permits concurrent calls**. Captures must satisfy Jet's concurrent access law; otherwise reject that registration.

**Callback returns a supported failure**. For a source with an established failure channel, stop admission and surface that failure through completion.

**Panic or incompatible unwind**. Never unwind across an incompatible boundary; use only the approved panic policy, not an invented recoverable exception.

**Native source cannot acknowledge stop**. Reject the safe registration contract, unless the selected protected runtime provides another established lifetime boundary.


If an accepted implementation violates its shutdown contract at runtime, report an enforcement or boundary failure according to that implementation's protection. A wrapper-only audited path cannot claim to repair arbitrary stale native calls. Returning a task does not manufacture protection.

Option B's second emit before a read returns QueueFull and preserves the first event. Reading frees capacity. Stop rejects later emissions, waits for native acknowledgment, and permits already accepted events to drain before end-of-stream. Cancellation never rolls back an event already processed. A universal queue cannot preserve synchronous return-valued callbacks; those retain a compatible explicit native contract.

Unacknowledged shutdown is not a routine successful close. The native protocol must define acknowledgment for a safe registration. If an accepted implementation violates that promise, the runtime quarantines that module: it rejects new registrations and calls, and retains only already-live wrapper state needed to prevent stale access. The consumed owner belongs to that quarantine; it is not silently freed or lost.

The stop task reports a boundary failure naming the quarantined module and retained resources. Quarantine cannot grow through new accepted registrations. Recovery is an explicit restart of the isolated worker, or termination/restart of the in-process host. No automatic restart, retry, or rollback occurs. Retrying deregistration is exposed only for a contract that establishes idempotence. An application that needs recoverable shutdown must select a boundary that can provide it.

A correlated callback failure is recorded once and appears in its emission task and the stop task after safe drain. For an external notification without an emission task, the registration records the failure and the stop task reports it. Existing unrecoverable panic policy remains separate. Neither task claims to undo effects already performed.

Preserve D-EFF4/D-EFF5 and each binder's language leaf. Every reached foreign call retains FFI.<Lang>, even when its native body is proved pure. The descriptor also includes established reachable callback and cleanup effects. Unknown native behavior remains marked as foreign and unverified; a declared row alone does not enforce native authority.

Registration is charged the retained callback's effects. For this C byte source, registration includes FFI.C and IO.Write. D-AUTHORITY-SCOPE1 and D-SCAP1 remain unchanged: Authority handles are lexical and cannot be stored, captured, or transferred into the registration.

An existing authority-holding scope must cover registration, every callback, cleanup, and stop completion. Sema rejects registration escape and requires owner consumption and drain before that scope exits. An existing application or process scope qualifies when it outlives the registration. Foreign notifications use the registration's checked effect set within that live scope, never the emitter's ambient authority. Emit and stop retain their reachable effects.

A wait deadline or quarantine does not permit that authority scope to exit while callbacks remain live. Scope completion still requires quiescence. A terminable isolated boundary can establish quiescence; otherwise forced shutdown requires host termination. No owned Authority transfer or new capability surface is proposed.

An admitted callback may never return. Default stop completion then remains pending and captures stay retained. A configured wait deadline may produce the existing TaskFailure.DeadlineBlown without freeing live state. No in-process timeout can safely reclaim executing captures. Bounded shutdown requires an explicitly selected terminable isolation boundary, or host termination.

Completion tasks return ordinary operation results. Their join keeps only the existing TaskFailure variants: Cancelled, DeadlineBlown, and Panicked. Stopped and native boundary failures belong to the operation result inside the task, not new TaskFailure variants. Both rails must be handled. No completion task or owning registration may become reachable from its own retained callback.

### Recommendation

Managed registration preserves native callback behavior where the lifecycle can be established. Message APIs remain explicit alternatives for suitable notification sources.

- Natural captured callbacks with checked lifetime ownership.
- Native scheduling and synchronous behavior remain expressible.
- Effects include callbacks and cleanup.

**Why not B:** The bounded queue is useful for suitable notification sources, but changes delivery placement and timing. It cannot cover synchronous return-valued callbacks.

**Remaining limit:** A callback that never returns can prevent safe in-process release indefinitely. Live captures cannot be freed safely; a deadline does not stop native execution. Bounded reclamation needs a terminable isolation boundary or host termination.

Safe release waits for admitted callbacks, potentially forever. A bounded shutdown requirement needs explicit isolation with termination or termination of the host.

### Review status

Fresh agent: /root/ffi_beginner_review. Skill: rli5. Verified lexical authority, scope lifetime through quiescence, existing task failures, operation results, self-stop, and nonreturning callbacks.

Author model family: OpenAI. Adversarial model family: OpenAI. Fresh agent: /root/ffi_sol_review. Verified admission, self-stop, both result rails, nontermination, lexical authority, quarantine, and release after acknowledgment and drain.

## D-FFI-BUILD1 — Explicit activation of an imported build plan

How should a project activate an imported build plan?

Jet already permits optional build import. This choice defines how a reviewed plan becomes active: through an explicit accept command or declarative package intent used by the next build. Both preserve existing host builds.

A binding is generated glue between languages. A facade is the callable interface that glue presents. An artifact is a compiled library or generated file. ABI means the machine-level calling and data-layout rules. Ownership decides who releases a value; lifetime decides when access is valid. Effects are operations such as file access. Isolation separates execution so foreign memory faults cannot reach protected host memory.

Native trust means relying on identified code to obey an audited promise. It is not proof. A runtime is the host machinery for executing code and managing objects. Runtime attachment prepares a foreign thread to enter that machinery. A component model is a common interface of values and resource handles shared by several languages.

Maya has a CMake game with C++ and Jet modules. Optional graph import is already approved. She needs to inspect the proposed actions and choose exactly when Jet starts executing them. Both options rebuild the same game from the same verified plan.

CMake owns target and custom-command execution today. Its File API exposes structured project information but does not prove arbitrary custom scripts have declared every input. Source: https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html

### Current experience

Current CMake-owned rebuild for the same mixed game.

```
cmake -S . -B build
cmake --build build
```

### Comparison: CMake

Native target execution and structured File API metadata; the API is not a full script proof.

```
# https://cmake.org/cmake/help/latest/manual/cmake-file-api.7.html
cmake -S . -B build
cmake --build build
```

### A. Activate through explicit preview and accept — recommended

Preserve optional graph import and existing host builds. Review a candidate, then use a named accept command to activate its exact digest. Ordinary builds never activate new import intent.

```
# PROPOSED; imports the existing CMake game build.
jet build --import cmake:build --preview
jet build --import cmake:build --accept <plan-digest>
jet build
# Existing CMake build remains available until explicit acceptance.
```

### B. Activate through declarative package intent

Keep graph import. Store the selected source and candidate digest in package.jet; the next ordinary build validates and activates that intent. Stale or incomplete intent blocks activation.

```
# PROPOSED: obtain the same read-only candidate.
jet build --import cmake:build --preview
# Then edit package.jet:
# build: .{ import: "cmake:build", plan: "<plan-digest>" }
jet build
# The build checks the digest before activating the declared plan.
```

### Exact behavior and edge cases

Both choices preserve D-BUILDLEGACY1=A, which already approves optional graph import and lets CI ban it. This ballot chooses only the public activation surface and exact acceptance behavior. It reuses existing graph and compiler-driver mechanisms, including #1044 and #1347. Binding generation and native facades work under either choice.

Preview reads the configured CMake build and emits a candidate in Jet's existing generated cache. It shows dependencies, commands, toolchain identities, inputs, outputs, environment, working directories, custom actions, and unsupported edges. Exit 0 means a reviewable candidate was produced, not that ownership changed. Incomplete plans must say partial ownership.

Accept rechecks all candidate inputs against the supplied digest and atomically selects the plan through the existing package and .jet/lock configuration. A stale digest or changed input returns exit 1 and preserves the previous selection. Internal failures retain exit 101. Existing package configuration and its build artifact model remain the one home; this ballot does not invent a new manifest schema.

Jet may execute a fully modeled action graph. An opaque native build may remain an explicit action with declared inputs and outputs; its inner build owner remains visible. Full ownership is rejected if required actions cannot be established. No traced build run is treated as proof of an arbitrary script. Ambient environment, dynamic dependencies, generated tools, and cross-compilation belong to the model.

Both options must rebuild the same game after a header change and preserve generated-source ordering, link inputs, debug information, errors, and relevant artifact behavior. Timestamps and compiler identity are part of required observations when the project depends on them. A plan cannot claim equality from matching one executable's stdout. Cache correctness requires complete inputs and correct invalidation.

Without explicit command acceptance in A or explicit package intent in B, default behavior never imports or activates a build. The expert can inspect the full action plan and decline it. Failure does not rewrite the host build files. Selecting another compiler is explicit and requires its own supported contract. Accepted partial ownership is not advertised as full build replacement. Existing source and build files remain authoritative; repeated import compares the new candidate against the selected plan.

The native-driver path remains available in both options. A manifest-free compiler invocation does not acquire a package build owner. No automatic restart, network access, dependency upgrade, or publication is implied by build import.

Option B proposes the exact package.jet field build: .{ import: "cmake:build", plan: "<plan-digest>" }. The preview command produces the digest without changing project files. A reviewed edit to those fields expresses activation intent; the next ordinary build checks the same source, toolchain, action closure, and policy before activation.

B does not interpret an arbitrary digest as proof. The referenced checked candidate must exist and match its inputs. Failed validation preserves the previous resolved plan, reports the invalid intent, and blocks that build. Successful validation atomically publishes the resolved lock entry. A CI policy banning imports rejects either activation form.

Both choices are explicit. B combines activation with an ordinary build after a configuration edit; A keeps activation as a separate command. B retains native-driver use and full graph import. Neither option removes an already-approved build capability.

### Recommendation

A separate accept action makes the ownership transition explicit for humans and CI, with exact input checks before activation.

- Existing host builds and optional import remain supported.
- Activation is separate from ordinary builds.
- One checked accept action pins the reviewed plan.

**Why not B:** Declarative activation makes an ordinary build also perform an ownership transition. A separates that transition into one named, checked action.

**Remaining limit:** Some opaque build actions cannot be imported completely. An arbitrary script can read undeclared inputs; observing one run cannot establish its whole behavior.

Incomplete action models stay explicit foreign actions or block complete import; Jet cannot silently infer missing behavior.

### Review status

Fresh agent: /root/ffi_beginner_review. Skill: rli5. Verified both activation paths preserve approved import, CI bans, digest checks, atomic lock publication, and partial-ownership disclosure.

Author model family: OpenAI. Adversarial model family: OpenAI. Fresh agent: /root/ffi_sol_review. Verified both activation methods preserve approved import, matched CMake behavior, digest validation, failure atomicity, and opaque-action limits.

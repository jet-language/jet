# Decision ballots — owner input

Ratified → [`syntax-decisions.md`](syntax-decisions.md). Picks → [`decision-ballots-owner.md`](decision-ballots-owner.md).

**HTML:** [`decision-ballots.html`](decision-ballots.html) · **Specs:** [`docs/plans/`](../plans/)

---

## Open

*(none — gaps #4 & #5 ratified 2026-06-16 as **U11–U18**; see
[`syntax-decisions.md`](syntax-decisions.md) and the picks in
[`decision-ballots-owner.md`](decision-ballots-owner.md).)*

---

## Ratified (CBIND + LL2)

| ID | Decision |
|---|---|
| **D-CBIND2** | Auto on compile/build + **`jet bind`** subcommand (same backend) |
| **D-CBIND3** | Bindgen helper crate (I6 waiver) |
| **D-CBIND5** | **`String`** at `char*` / `const char*` boundary |
| **D-CBIND6** | **`#define` constants only** in bind output; skip function-like macros |
| **D-LL2** | **`@audit("…")`** required on `@unsafe { … }` |

C FFI surface (**D-CFFI2-SYN**), **`use`** (**D-S16-USE**), link resolution — **S59** / [`m14-c-ffi.md`](../plans/epoch-2/m14-c-ffi.md).

---

## D-CBIND6 — reference (ratified **B**)

**`jet bind`** emits integer/float/string-literal **`#define`** constants into the
generated `@bindgen module`. Function-like macros and `#include` expansion are skipped;
users add **`@extern module c.<lib> { fn … }`** overlay for macro-wrapped APIs.

---

## gaps #4/#5 — ratified surface (U11–U18)

Decided 2026-06-16. The ballot tables that produced these are in git history; the
authoritative records are **U11–U18** in
[`syntax-decisions.md`](syntax-decisions.md). End-state worked `~/.jet/config.jet`
the implementing agent builds toward:

```jet
// ~/.jet/config.jet
module halcyon {
    sources: {
        default:  github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixos-unstable,
    }

    system.halcyon: {                            // U18: System inferred from `system.`
        target: linux.x64,                       // U13: typed platform value, not a string
        packages: [ default.[firefox, btop, ripgrep], unstable.zed-editor ],
        services: {
            pipewire:  { enable: true },         // U12/U18: Service inferred
            openssh:   { enable: true, ports: [22] },
        },
        options: [                               // U13: ordered key: value list, no set()
            net.hostName:       halcyon,         // bare word (identifier-like)
            time.timeZone:      "Europe/London", // quoted (free-form string)
            users.nate.shell:   default.fish,
        ],
    }
}

module installer {
    image.halcyon-iso: { from: system.halcyon, format: iso }  // U14: target/packages inherited
}
```

Applied with `jetpack os switch @halcyon` (U15/U16). A realized `library` package
is consumed with `use <pkg>` (U17).

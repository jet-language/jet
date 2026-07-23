---
title: Owner scratch
---
Track | Cards | Why independent |
|---|---|---|
| **e4 jetpack core** | #395 #396 #397 #399 #420 #422 #423 #427 #428 #430 #431 #432 #433 #517 #330 #6 | Package manager is a separate binary; needs the compiler, not e3's crypto/web/FFI/corelib pillars. 2 already building. |
| **e6 Canvas** | #488 #492 #389 #375–#383 #387 #388 #390 #391 #489 #491 #493 | Visual editor is a separate frontend over graph JSON; design already ratified. #389 is a live P0-ish bug. |
| **e5** | #672 (pure spec-vs-shipped audit), #673 (computed modules — you already authorized pulling this up) | Audit is read-only; #673 extends comptime independently. |
| **e8 cross-cutting tooling** | #211 CI, #12 Debugger, #288 core.files, #398 build sandbox | Parked in e8 but cut across everything; no e3 gate. |
| **e9 planning-phase** | #668 TIR spec freeze, #669 compile-speed bets, #670 bootstrap validation harness | Design/planning can start now even though *impl* is late.

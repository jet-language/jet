# Interop

Calling C and Rust, foreign projections, and programs in other languages.

Run a lesson from the repository root, for example `jet run Examples/Interop/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [Murmur3](Murmur3/) — MurmurHash3_x86_32 port.

## FFI

- [Callbacks](FFI/Callbacks/)
- [Cbind](FFI/Cbind/)
- [FFI](FFI/FFI.jet) — The safe way to call native code is a typed binding: `extern rust` names a crate and gives each imported function a Jet signature, so every call is…
- [InlineC](FFI/InlineC.jet)
- [NativeC](FFI/NativeC/) — A C library binds through `#Import c.<lib>`: each symbol gets a Jet signature, so signed, unsigned, float, Bool, String and write-back `&` arguments cross…
- [OpaqueHandles](FFI/OpaqueHandles/) — Generated opaque C handles stay checked at the Jet boundary.
- [PlainC](FFI/PlainC.c) — A C file runs with plain `jet run PlainC.c`: Jet tells its language by the extension, compiles and links it with the C toolchain Jet programs use, and runs it.

## Languages

- [Cobol](Languages/Cobol/)
- [GuestLibrary](Languages/GuestLibrary/) — Package used by the lessons beside it.
- [Lua](Languages/Lua/)
- [Octave](Languages/Octave/)
- [Perl](Languages/Perl/)
- [Php](Languages/Php/)
- [PowerShell](Languages/PowerShell/)
- [Python](Languages/Python/)
- [R](Languages/R/)
- [Ruby](Languages/Ruby/)

## Polyglot

- [PolyglotAda](Polyglot/PolyglotAda/) — Run `jet inspect bind ada geodesy.ads --pkg geodesy`, then `jet run run.jet`.
- [PolyglotCom](Polyglot/PolyglotCom/) — Windows-only COM example.
- [PolyglotDart](Polyglot/PolyglotDart/) — Run `jet inspect bind dart callbacks.dart --jet run.jet --pkg callbacks`, then `dart run host.dart` from the generated project.
- [PolyglotDotNet](Polyglot/PolyglotDotNet/) — Run `jet inspect bind cs Counter.cs --pkg counter`, then `jet run run.jet`.
- [PolyglotFortran](Polyglot/PolyglotFortran/) — Run `jet inspect bind fortran matrix.f90 --pkg matrix`, then `jet run run.jet`.
- [PolyglotGo](Polyglot/PolyglotGo/) — Run `jet inspect bind go handles.go --pkg handles`, then `jet run run.jet`.
- [PolyglotJava](Polyglot/PolyglotJava/) — Run `jet inspect bind java Counter.java --pkg counter`, then `jet run run.jet`.
- [PolyglotPascal](Polyglot/PolyglotPascal/) — Run `jet inspect bind pascal Counter.pas --pkg counter`, then `jet run run.jet`.
- [PolyglotTcl](Polyglot/PolyglotTcl/) — Run `jet inspect bind tcl eda.tcl --pkg eda`, then `jet run run.jet`.

# Bridges probe

## What I built

- `pkg/zreader.c` wraps the installed zlib `gzFile` handle. It allocates a reader, opens `sample.txt.gz`, reads two lines, and closes/frees the handle.
- `pkg/zreader.h` exposes three scalar-safe functions. `jet inspect bind zreader.h --pkg zreader` bound all three.
- `pkg/reader.hpp`/`reader.cpp` expose a C++ `bridge::Reader` class. The implementation reads `sample.txt` through Linux syscalls, so the foreign code owns the file descriptor and Jet owns only the opaque handle.
- `pkg/rust_run.jet` declares `extern rust "base64@0.22"`.

## What worked

Command:
`/home/nate/Projects/Github/jet/scripts/agent/jet-env jet inspect bind zreader.h --pkg zreader`

Output: `bound 3 functions from \`zreader.h\` → .jet/bindings/c/zreader.jet`.

After declaring `z: c@nixpkgs:zlib` and adding a direct `use c.z` declaration so the transitive zlib link is present, this native build succeeded:

`jet build zlib_run.jet --verbose`

Output included `link -> build/zlib_run` and `Built build/zlib_run in 9.6s ✓`. Running `./build/zlib_run` printed:

```
22
1.3.2
22
```

The two 22-byte results are the two lines read through zlib; `1.3.2` is `zlibVersion()` through the second C import. This demonstrates a real C bridge, native link, opaque-handle cleanup, and a foreign dependency.

C++ binding generation also worked:

`jet inspect bind cpp reader.hpp --target x86_64-unknown-linux-gnu --clang /nix/store/xy32w323i90dlgscl5zbac0dyvgk7lig-clang-wrapper-21.1.8/bin/clang++ --ar /nix/store/3d1c302vw7kc8a5vknhmn34c0pd7zm6m-gcc-wrapper-15.3.0/bin/ar --pkg reader --namespace bridge -I . -L . -l reader_impl`

Output: `bound 2 C++ members from \`reader.hpp\` → .jet/bindings/cpp/reader.jet`.

## Gaps

1. Direct binding cannot consume common opaque C APIs. `jet inspect bind zlib_probe.h --pkg zlib` returned E3208: `No bindable C function prototypes found for \`zlib\`` because the header contains `gzFile`. A library author must hand-write a C adapter plus an archive. The adapter was 34 lines and had to choose ownership, allocation, null handling, buffering, and cleanup itself.
2. Native link closure is not transitive. The adapter archive depends on zlib, but `zreader: c@...` alone emitted only `-lzreader`; `z: c@system` tried to provision `nixpkgs#z` and returned E3210. The working package needed an explicit `z: c@nixpkgs:zlib`, a direct `use c.z`, and `RUSTFLAGS=-L native=<scratch>` for the local archive. This is repeated package/linker ceremony for every wrapped dependency.
3. Rust crate bridges are authority-gated before the bridge can be tested. `jet check rust_run.jet` passed, but `jet build rust_run.jet --verbose` returned E1803: `Application authority is undecided for \`FFI\`` and requires `allow: [FFI]`. No native Rust-crate execution was possible in this probe after that gate.

## Friction and defects

- `jet run zlib_run.jet` returned E0956 (`Extern call ... has no prepared bridge`) even though the documented path is native `jet build`; the diagnostic says “Report this as a compiler bug” rather than explaining that resident run does not prepare native FFI.
- The C++ binder emitted `Reader.{ value: value }`. `jet check cpp_run.jet` rejected it with E0320: `Struct construction uses Reader{…}, not Reader.{…}` (D-LIT-DOT1), at the generated wrapper's imported source.
- A low-level C++ attempt (`jet build cpp_low.jet`) then returned E3208: `Local C++ archive inventory is missing bound symbol(s): TLS_MODULE_BASE_`. `nm -g .jet/bindings/cpp/libjet_cpp_reader.a` showed `_TLS_MODULE_BASE_` as an undefined C++ shim symbol. The validator's ignorable list covers C++ runtime names but not this symbol, so a generated C++ shim is rejected before linking.
- The C++ generated source uses `#Import module c.jet_cpp_reader` and safe opaque-handle APIs, but the generated wrapper's syntax defect prevents importing it without a manual edit. The low-level overlay passed `jet check`, isolating the failure to generated/bridge assembly rather than the user program.

## Cross-domain reading

The mined video, image, GIS, medical-imaging, and climate reports all identify format/domain readers (FFmpeg, OpenCV/libvips, GDAL/GEOS, DICOM/NIfTI/HDF5, NetCDF/Zarr/Arrow/Parquet) as absent from today's core. A bridge can reach them, but the C opaque-handle and transitive-link gaps recur across games, AI/ML, data, GUI, and science. No niche library is proposed here.

## Verdict

C is usable for a small hand-authored scalar adapter, but not safe-by-construction for ordinary opaque library handles; C++ binding generation is currently blocked by two compiler defects; Rust bridge reachability is unproven because authority stops the native build. The highest-leverage primitive is a typed foreign-handle adapter/link-closure path that owns cleanup and dependencies without per-library C shims.

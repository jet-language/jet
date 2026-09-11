// The foundations slate: eleven ballots, each with its primitive card.
// Owner law (2026-09-02): recommended option is A and listed first; gains and
// losses are the reality; every remaining loss carries why it cannot be
// designed away; no jargon; one question per ballot.
const BY = 'Owner, 2026-09-02: "Remember we need to spend effort up front figuring out how to eliminate the losses so we dont pay for them later and get the best of all worlds."';

export const slate = [
  // ───────────────────────────── 1. real-time boundary ─────────────────────────────
  {
    card: {
      title: 'Real-time boundary: fixed-rate callbacks, exact waits, callback-safe shared cells, overrun receipts',
      tags: ['realtime', 'games', 'embedded', 'gui', 'audio'],
      body: 'Probes prim-realtime (G1-G4), area-games (G4), area-embedded (G4) found no way to run audio, control loops, or interrupt handlers on a deadline: no callback boundary (E1001 core.audio), time.sleep truncates nanoseconds to milliseconds (100 x 999_999ns slept 245 us), Shared locks and channel sends pass !Time and !Mem.Alloc checks though they block, and a 10 M-iteration CPU loop under a 5 ms #Context completes with no miss recorded. Evidence: docs/audits/domain-foundations-2026-09-02/all-gaps.json.',
      plan: 'After ratification: add the Time.Wait effect leaf and make blocking core calls carry it; make scalar shared cells lock-free and wait-free; honor nanosecond sleeps and add time.sleep_until(Instant); add rt.callback(rate:, frames:, fn) with a timer clock in core and an overrun receipt; audio and MIDI device sources land as first-party packages over the foreign-handle bridge. Every tier calls the same Prelude functions (I9); diagnostics registered with snapshots (I4); Syntax.rs entries for any new marker (I7).',
      criteria: [
        'A 48 kHz, 256-frame callback runs ten seconds on Linux with zero overruns recorded, and the receipt reports overruns when the callback is made slow on purpose. Proof: example golden plus receipt output on default jet run, --release, and jet build.',
        'A blocking call inside a -[Time.Wait]> callback is a registered compile error with a UI snapshot; a scalar shared cell read inside the callback compiles. Proof: snapshot and example.',
        '100 calls of time.sleep(999_999ns) elapse between 95 ms and 130 ms, and time.sleep_until(instant) wakes within 200 us of the instant on Linux. Proof: timing golden on every tier.',
      ],
    },
    decision: {
      id: 'D-FOUND-REALTIME1', group: 'runtime',
      title: 'Real-time boundary: fixed-rate callbacks with exact waits and overrun receipts',
      gist: 'How does a Jet library run audio, control loops, and interrupt handlers on a deadline without allocating?',
      lesson: 'Audio and control code runs on a fixed clock: a callback gets a buffer every few milliseconds and must finish before the next one. Jet can already forbid allocation in a function. It has no callback boundary, its sleeps round to milliseconds, and its shared cells can block. A library author fakes the clock with a timer and cannot prove a missed deadline.',
      story: 'Mia writes a synthesizer in Jet. Her render function must fill 256 samples every 5.3 ms. She wants the runtime to call it on time, refuse any allocation or lock inside it, and tell her how many buffers she missed. Today she loops with a sleep that rounds to a millisecond and counts misses by hand.',
      inWild: 'Every audio engine (CoreAudio, JUCE, cpal) drives a callback from the device thread, forbids allocation and locks inside it, and reports an underrun as an "xrun" count. Real-time control (ROS 2, RTIC) binds handlers to a clock or an interrupt and measures jitter.',
      options: [
        { key: 'A', name: 'Callback boundary with exact waits and lock-free cells', detail: 'The runtime drives a fixed-rate callback. Its effect row forbids allocation and blocking through a new Time.Wait leaf that blocking calls carry. Scalar shared cells become lock-free reads and writes. Sleeps keep nanoseconds and an absolute wait exists. Every overrun lands in the receipt.', code: 'level :: shared 0.8\nstream :: rt.callback(rate: 48_000, frames: 256,\n    -[Mem.Alloc, Time.Wait]> fn(out: &[Float]) { render(out, level) })\ntime.sleep_until(stream.next_deadline())\nprint(stream.receipt().missed)' },
        { key: 'B', name: 'Library only: keep the timer fake, add nanosecond sleep', detail: 'Only time.sleep changes. Authors keep spinning on a timer and count misses by hand.', code: 'loop tick < 10_000 {\n    process(&buffer)\n    time.sleep(5_333_333ns)\n}' },
        { key: 'C', name: 'Hard real-time scheduler with priorities and static WCET proof', detail: 'A priority scheduler and a compiler-proved worst-case execution time for every handler.', code: '#RealTime(priority: 90, wcet: 200us)\nfn render(out: &[Float]) { ... }' },
      ],
      comparisons: [
        { lang: 'Rust (cpal)', note: 'The device drives the callback; a miss is reported as an xrun.', code: 'let stream = device.build_output_stream(&config,\n    move |data: &mut [f32], _| render(data),   // no alloc, no lock\n    move |err| eprintln!("xrun: {err}"), None)?;\nstream.play()?;' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A is small: it reuses effect rows, shared cells, and receipts, and matches how every audio engine works.',
        whyNot: [
          { key: 'B', reason: 'B leaves misses unproven and blocking calls undetected; every library repeats the same loop.' },
          { key: 'C', reason: 'C is years of compiler work, platform-specific, and beginners never need it.' },
        ],
        tradeoff: 'Unavoidable: overruns are counted, not prevented, because safe code cannot be interrupted mid-function. Unavoidable: exact waits need a high-resolution timer the OS must grant.',
      },
      surface: {
        gist: 'How does a Jet library run audio, control loops, and interrupt handlers on a deadline without allocating?',
        lesson: 'Audio and control code runs on a fixed clock: a callback gets a buffer every few milliseconds and must finish before the next one. Jet can already forbid allocation in a function. It has no callback boundary, its sleeps round to milliseconds, and its shared cells can block. A library author fakes the clock with a timer and cannot prove a missed deadline.',
        trio: {
          current: { note: 'prim-realtime probe: a timer fake, no deadline, no miss count', code: '// prim-realtime: the fake\nfn run() -[Time]> {\n    loop tick < 10_000 {\n        process(&buffer)          // no deadline, no miss count\n        time.sleep(999_999ns)     // slept 2 us: rounded to milliseconds\n    }\n}' },
          wild: { lang: 'Rust (cpal)', note: 'The device drives the callback; a miss is an xrun.', code: 'let stream = device.build_output_stream(&config,\n    move |data: &mut [f32], _| render(data),   // no alloc, no lock\n    move |err| eprintln!("xrun: {err}"), None)?;\nstream.play()?;' },
        },
        options: [
          { key: 'A', name: 'Callback boundary with exact waits and lock-free cells', gist: 'The runtime calls you on time, forbids blocking, and counts misses.', gains: ['Allocation and blocking inside the callback are compile errors', 'Misses are counted in the receipt, never hidden', 'Nanosecond sleeps and absolute waits'], losses: ['Overruns are counted, not prevented', 'Exact waits need an OS high-resolution timer'], proposed: { code: '// A: the runtime drives the callback; the row forbids allocation\n// and blocking; misses are counted in the receipt, never hidden\nlevel :: shared 0.8                       // scalar cell: lock-free\nstream :: rt.callback(rate: 48_000, frames: 256,\n    -[Mem.Alloc, Time.Wait]> fn(out: &[Float]) {\n        render(out, level)                // one atomic read\n    })\ntime.sleep_until(stream.next_deadline()) // absolute, nanosecond exact\nprint(stream.receipt().missed)           // overruns: 0' } },
          { key: 'B', name: 'Library only: keep the timer fake, add nanosecond sleep', gist: 'Only sleep changes; authors keep spinning and counting by hand.', gains: ['Nothing new in the runtime'], losses: ['Misses stay unproven', 'Blocking calls stay undetected', 'Every library repeats the loop'], proposed: { code: '// B: only the sleep gets exact\nloop tick < 10_000 {\n    process(&buffer)\n    time.sleep(5_333_333ns)\n}' } },
          { key: 'C', name: 'Hard real-time scheduler with static worst-case proof', gist: 'Priorities and a compiler-proved worst-case time per handler.', gains: ['Certification-grade evidence'], losses: ['Years of compiler work', 'Platform-specific scheduling', 'Beginners never need it'], proposed: { code: '// C: a scheduler and a proved bound\n#RealTime(priority: 90, wcet: 200us)\nfn render(out: &[Float]) { ... }' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A is small: it reuses effect rows, shared cells, and receipts, and matches how every audio engine works.',
          whyNot: [
            { key: 'B', reason: 'B leaves misses unproven and blocking calls undetected; every library repeats the same loop.' },
            { key: 'C', reason: 'C is years of compiler work, platform-specific, and beginners never need it.' },
          ],
          gains: ['Blocking and allocation are compile errors inside the callback', 'Misses are counted in the receipt', 'Exact waits'],
          losses: ['Overruns are counted, not prevented', 'Exact waits need an OS high-resolution timer'],
          tradeoff: 'Unavoidable: overruns are counted, not prevented, because safe code cannot be interrupted mid-function. Unavoidable: exact waits need a high-resolution timer the OS must grant.',
        },
      },
    },
  },

  // ───────────────────────────── 2. opaque C handles ─────────────────────────────
  {
    card: {
      title: 'Opaque C handles and native link closure in generated bindings',
      tags: ['ffi', 'games', 'ai-ml', 'data', 'gui', 'science', 'text'],
      body: 'Probes prim-bridges (G1, G2, G3), prim-text (G4), area-games (G5): jet inspect bind refuses headers whose functions take or return opaque pointers (E3208 for zlib gzFile and HarfBuzz), so authors write a C adapter per library and free resources by hand; transitive native libraries must be declared by hand (E3210, undefined gzopen at link); the Rust bridge stops at E1803 until allow: [FFI] is granted. Evidence: docs/audits/domain-foundations-2026-09-02/all-gaps.json.',
      plan: 'After ratification: the C binder maps opaque pointer typedefs to generated handle types with #Close (D-FFI-CAP1) using a recognizable free function or an overlay naming it; pointer-to-scalar parameters bind as & lends; a binding module declares its link closure once (links: [...]) resolved through c@system deps; the FFI authority error names the exact grant. Diagnostics registered with snapshots; zlib and HarfBuzz examples with goldens on every tier.',
      criteria: [
        'jet inspect bind on zlib.h produces a GzFile handle with #Close(gzclose) and a program reads a .gz file through it on every execution tier. Proof: example golden.',
        'HarfBuzz shaping through the generated binding returns glyph ids and positions for an Arabic string without #Unsafe in user code. Proof: example golden.',
        'A binding whose archive needs libz links with one declaration and a missing declaration is a registered diagnostic naming the library. Proof: snapshot.',
      ],
    },
    decision: {
      id: 'D-FOUND-HANDLE1', group: 'runtime',
      title: 'Opaque C handles and native link closure in generated bindings',
      gist: 'How does a library bind a C library that hands out opaque handles, without hand-written adapters?',
      lesson: 'Most C libraries (zlib, HarfBuzz, SQLite, FFmpeg) return an opaque pointer and expect it back later, then freed. Jet\'s binder refuses those headers, so an author writes a C adapter per library and frees by hand. Jet already has the ownership words: ^ transfers, & lends for one call, #Close closes once. The binder only needs to use them.',
      story: 'Omar wants gzip reading in Jet. zlib\'s API is gzopen, gzgets, gzclose over an opaque gzFile. The binder says no bindable prototypes. He writes zr_open, zr_gets, zr_close in C over an integer table, then links zlib by hand after an undefined-symbol error.',
      inWild: 'Swift imports C opaque pointers as OpaquePointer and authors wrap them in a class whose deinit frees them. Rust bindgen emits the pointer type and authors implement Drop. Both leave ownership to the author; neither refuses the header.',
      options: [
        { key: 'A', name: 'Generated handle types with #Close, one link declaration', detail: 'An opaque pointer typedef becomes a handle type. The function that frees it becomes its close, found by name or named in the overlay. Returning constructors transfer with ^, borrowing calls lend with &. The binding declares its transitive libraries once.', code: 'use c.zlib\nfn lines(path: String) [String] !CError -> {\n    file :: zlib.gzopen(path, "rb")\n    rows := []\n    loop line in zlib.gz_lines(&file) { rows.push(line) }\n    return Ok(rows)\n}' },
        { key: 'B', name: 'Generated C shim with integer handles', detail: 'The binder writes the adapter C for you: a table of pointers indexed by Int.', code: 'h :: zlib.zr_open(path)\nzlib.zr_close(h)   // unchecked Int; double free reachable' },
        { key: 'C', name: 'Raw pointers under #Unsafe for every opaque API', detail: 'The binder emits the raw pointer type; every use sits in a reasoned #Unsafe block.', code: '#Unsafe("zlib handle") {\n    file :: zlib.gzopen(path, "rb")\n    zlib.gzclose(file)\n}' },
      ],
      comparisons: [
        { lang: 'Swift', note: 'Opaque pointers import as OpaquePointer; a class frees in deinit.', code: 'final class GzReader {\n    let file: gzFile\n    init(path: String) { file = gzopen(path, "rb") }\n    deinit { gzclose(file) }\n}' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A binds the libraries every domain needs with the ownership words Jet already ratified, and no user code is unsafe.',
        whyNot: [
          { key: 'B', reason: 'B hides the pointer behind an unchecked Int: leaks stay silent and a double free is one typo away.' },
          { key: 'C', reason: 'C makes every library author write unsafe blocks and every audit repeat per library.' },
        ],
        tradeoff: 'Unavoidable: C carries no ownership data, so when no free function is recognizable the author names it in the overlay. Unavoidable: a handle crosses tasks only when the overlay says the library is thread-safe.',
      },
      surface: {
        gist: 'How does a library bind a C library that hands out opaque handles, without hand-written adapters?',
        lesson: 'Most C libraries (zlib, HarfBuzz, SQLite, FFmpeg) return an opaque pointer and expect it back later, then freed. Jet\'s binder refuses those headers, so an author writes a C adapter per library and frees by hand. Jet already has the ownership words: ^ transfers, & lends for one call, #Close closes once. The binder only needs to use them.',
        trio: {
          current: { note: 'prim-bridges probe: the binder refuses zlib; a C adapter hides the pointer', code: '$ jet inspect bind zlib_probe.h --pkg zlib\nError [E3208]: No bindable C function prototypes found for zlib\n// the workaround: a C adapter over an integer table\nint  zr_open(const char* path);\nint  zr_gets(int h, char* buf, int n);\nvoid zr_close(int h);' },
          wild: { lang: 'Swift', note: 'Opaque pointers import; a class frees in deinit.', code: 'final class GzReader {\n    let file: gzFile\n    init(path: String) { file = gzopen(path, "rb") }\n    deinit { gzclose(file) }\n}' },
        },
        options: [
          { key: 'A', name: 'Generated handle types with #Close, one link declaration', gist: 'The binder turns opaque pointers into owned handles that close once.', gains: ['zlib, HarfBuzz, SQLite, FFmpeg bind without adapters', 'No unsafe in user code; close runs exactly once', 'Link libraries declared once, checked at build'], losses: ['An unrecognizable free function is named in the overlay', 'Thread-safety is declared, not inferred'], proposed: { code: '// A: an opaque pointer becomes a handle type; its free function\n// becomes its close; the link closure is declared once\nuse c.zlib                          // .jet/bindings/c/zlib.jet:\n                                    //   pub struct GzFile #Close(gzclose)\nfn lines(path: String) [String] !CError -> {\n    file :: zlib.gzopen(path, "rb")  // ^GzFile: this call owns it\n    rows := []\n    loop line in zlib.gz_lines(&file) { rows.push(line) }   // & lends\n    return Ok(rows)                  // file closes exactly once here\n}\n// package.jet:  deps: { zlib: c@system(links: [z]) }' } },
          { key: 'B', name: 'Generated C shim with integer handles', gist: 'The binder writes the adapter and hands out Int handles.', gains: ['No new handle type'], losses: ['Handles are unchecked Ints', 'Leaks stay silent', 'Double free is reachable'], proposed: { code: '// B: an Int stands in for the pointer\nh :: zlib.zr_open(path)\nline :: zlib.zr_gets(h)\nzlib.zr_close(h)               // or forget to' } },
          { key: 'C', name: 'Raw pointers under #Unsafe for every opaque API', gist: 'Emit the pointer; every use is a reasoned unsafe block.', gains: ['Nothing to build in the binder'], losses: ['Every library author writes unsafe', 'Audits repeat per library', 'Beginners avoid those libraries'], proposed: { code: '// C: raw pointer, unsafe at every use\n#Unsafe("zlib handle") {\n    file :: zlib.gzopen(path, "rb")\n    zlib.gzclose(file)\n}' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A binds the libraries every domain needs with the ownership words Jet already ratified, and no user code is unsafe.',
          whyNot: [
            { key: 'B', reason: 'B hides the pointer behind an unchecked Int: leaks stay silent and a double free is one typo away.' },
            { key: 'C', reason: 'C makes every library author write unsafe blocks and every audit repeat per library.' },
          ],
          gains: ['Real C libraries bind without adapters', 'Close runs exactly once, no unsafe in user code', 'Link libraries declared once'],
          losses: ['An unrecognizable free function is named in the overlay', 'Thread-safety is declared, not inferred'],
          tradeoff: 'Unavoidable: C carries no ownership data, so when no free function is recognizable the author names it in the overlay. Unavoidable: a handle crosses tasks only when the overlay says the library is thread-safe.',
        },
      },
    },
  },

  // ───────────────────────────── 3. sandbox capabilities ─────────────────────────────
  {
    card: {
      title: 'Capability-scoped host imports for sandbox guests and resource-scoped file rights',
      tags: ['capabilities', 'backend', 'cli', 'gui', 'games', 'plugins'],
      body: 'Probe prim-capabilities (G1-G3): a sandbox guest cannot use any effect (E1258 on fs.read, E3304 on Net), may export only homogeneous scalars (E1260), and a resource-scoped FS.Read grant does not bind to core.files (leaf grant gives E1803; a broad grant reads outside the root). Evidence: docs/audits/domain-foundations-2026-09-02/all-gaps.json. Law: one rights tree (D-AUTHORITY-MODEL1), tighten-only scopes (D-AUTHORITY-SCOPE1), target: sandbox (D-ONCE-SANDBOX1); D-PLUGIN1 and D-DEP-WASM1 are open spec-only records.',
      plan: 'After ratification: a guest package declares authority.needs; the host lends a tightened Authority to plugin.load; guest core.files and core.net calls route through host-provided imports checked against that Authority with a no-follow root; #Codable records, lists, Option, and Result cross through generated typed interfaces versioned by the existing interface snapshot (E1257); resource-scoped grants bind to core.files on the host too. Diagnostics with snapshots; sandbox example extended with goldens on every tier; ratify the sandbox rules under D-PLUGIN1/D-DEP-WASM1 in the same change.',
      criteria: [
        'A guest that needs FS.Read builds for target: sandbox, reads inside the granted folder, and a read outside it is a registered denial naming the grant. Proof: example golden and snapshot.',
        'A guest exports a function over a #Codable record list and the host calls it with typed values. Proof: example golden on every tier.',
        'A host-side core.files read under a resource-scoped FS.Read grant succeeds inside the root and fails with a registered diagnostic outside it. Proof: snapshot and example.',
      ],
    },
    decision: {
      id: 'D-FOUND-SANDBOX1', group: 'safety',
      title: 'Capability-scoped host imports for sandbox guests',
      gist: 'How does a host give a plugin exactly the file and network rights it needs, and nothing more?',
      lesson: 'A sandbox package runs in WebAssembly with no effects at all. It may not read a file or open a socket. It may only export functions over plain numbers and text. Real plugins need bounded access: read this folder, call this host. Jet already has one rights tree with path-scoped leaves. The sandbox boundary does not use it yet.',
      story: 'Priya ships a search plugin. It must index files under one folder the host chooses and nothing else. Today the guest cannot read at all, so she moves all reading into the host and streams text across the boundary by hand.',
      inWild: 'The WebAssembly Component Model lets a guest declare imports in WIT; WASI pre-opens one directory as the guest\'s whole filesystem. Deno grants permissions per path and host. Both give bounded rights, not all-or-nothing.',
      options: [
        { key: 'A', name: 'Guest declares needs; host lends a tightened Authority', detail: 'The guest names the rights it needs. The host lends an Authority value tightened to a folder or host. Guest file and network calls go through host imports checked against it. Records cross through generated typed interfaces.', code: '// guest package.jet\nauthority: { needs: [FS.Read, Net.Connect] }\npub fn index(rows: [Row]) Int -[FS.Read]> { ... }\n// host\nplugin :: plugin.load("index.wasm",\n    authority: authority.tighten(allow: ["FS.Read:/data/in"]))\ntotal :: plugin.index(rows)' },
        { key: 'B', name: 'Keep sandboxes effect-free; pass data through the host', detail: 'No change. The host does all reading and networking and passes scalars across.', code: 'text :: files.read("/data/in/a.txt")\nn :: plugin.count(text)     // Text in, Int out' },
        { key: 'C', name: 'Grant effects by ambient package rights', detail: 'A sandbox package inherits the host\'s authority like a native dependency does.', code: 'plugin :: plugin.load("index.wasm")   // gets the host\'s rights' },
      ],
      comparisons: [
        { lang: 'WIT (Component Model)', note: 'The guest declares imports; the host binds them per capability.', code: 'world plugin {\n  import wasi:filesystem/preopens@0.2.0;\n  export index: func(rows: list<row>) -> u32;\n}' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A keeps the sandbox promise and reuses the one rights tree: the guest asks, the host tightens, every call carries exactly those rights.',
        whyNot: [
          { key: 'B', reason: 'B turns every plugin interface into host plumbing; no plugin can be self-contained.' },
          { key: 'C', reason: 'C gives a plugin the host\'s full rights and breaks the sandbox promise.' },
        ],
        tradeoff: 'Unavoidable: rich values are copied across the boundary because the guest has its own memory. Unavoidable: each import call pays one host hop.',
      },
      surface: {
        gist: 'How does a host give a plugin exactly the file and network rights it needs, and nothing more?',
        lesson: 'A sandbox package runs in WebAssembly with no effects at all. It may not read a file or open a socket. It may only export functions over plain numbers and text. Real plugins need bounded access: read this folder, call this host. Jet already has one rights tree with path-scoped leaves. The sandbox boundary does not use it yet.',
        trio: {
          current: { note: 'prim-capabilities probe: a guest that reads one file cannot build; rich exports cannot cross', code: '$ jet build guest/run.jet --target=sandbox\nError [E1258]: A sandbox can\'t use any effect      // fs.read at run.jet:4\n$ jet build rich_guest/run.jet --target=sandbox\nError [E1260]: `pub fn sum` is not one homogeneous Int, Float, Bool, or Text' },
          wild: { lang: 'WIT (Component Model)', note: 'The guest declares imports; the host binds each capability.', code: 'world plugin {\n  import wasi:filesystem/preopens@0.2.0;   // one pre-opened folder\n  import host: interface { log: func(msg: string); }\n  export index: func(rows: list<row>) -> u32;\n}' },
        },
        options: [
          { key: 'A', name: 'Guest declares needs; host lends a tightened Authority', gist: 'The guest asks for rights; the host lends exactly those, scoped.', gains: ['Plugins read one folder and nothing else', 'Records and lists cross as typed values', 'One rights tree, no second capability system'], losses: ['Rich values are copied across the boundary', 'Each import call pays one host hop'], proposed: { code: '// A: the guest names what it needs; the host lends an Authority\n// tightened to a folder; records cross as generated typed interfaces\n// guest package.jet\nauthority: { needs: [FS.Read, Net.Connect] }\npub fn index(rows: [Row]) Int -[FS.Read]> { ... }   // Row is #Codable\n// host\nplugin :: plugin.load("index.wasm",\n    authority: authority.tighten(allow: ["FS.Read:/data/in",\n                                         "Net.Connect:api.example"]))\ntotal :: plugin.index(rows)      // the call carries exactly those rights' } },
          { key: 'B', name: 'Keep sandboxes effect-free; pass data through the host', gist: 'No change; the host does all reading and passes scalars.', gains: ['Smallest attack surface'], losses: ['Every plugin interface becomes host plumbing', 'No plugin is self-contained', 'Only numbers and text cross'], proposed: { code: '// B: the host reads; the guest counts\ntext :: files.read("/data/in/a.txt")\nn :: plugin.count(text)          // Text in, Int out' } },
          { key: 'C', name: 'Grant effects by ambient package rights', gist: 'A sandbox package inherits the host\'s rights like a dependency.', gains: ['No new surface'], losses: ['A plugin gets the host\'s full rights', 'The sandbox promise is broken'], proposed: { code: '// C: ambient rights\nplugin :: plugin.load("index.wasm")   // gets the host\'s rights' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A keeps the sandbox promise and reuses the one rights tree: the guest asks, the host tightens, every call carries exactly those rights.',
          whyNot: [
            { key: 'B', reason: 'B turns every plugin interface into host plumbing; no plugin can be self-contained.' },
            { key: 'C', reason: 'C gives a plugin the host\'s full rights and breaks the sandbox promise.' },
          ],
          gains: ['Plugins get bounded rights', 'Typed records cross the boundary', 'One rights tree'],
          losses: ['Rich values are copied across the boundary', 'Each import call pays one host hop'],
          tradeoff: 'Unavoidable: rich values are copied across the boundary because the guest has its own memory. Unavoidable: each import call pays one host hop.',
        },
      },
    },
  },

  // ───────────────────────────── 4. lifecycle ─────────────────────────────
  {
    card: {
      title: 'Signals and request deadlines as cancellation; closable child stdin',
      tags: ['lifecycle', 'backend', 'cli', 'web', 'games', 'gui', 'embedded'],
      body: 'Probes area-backend (G3, G4), area-cli (G1): no typed signal subscription (E1004 core.process.signal), no request-scoped deadline middleware (E1004 core.http.server.timeout), and ProcessStdin has no close so a producer feeding an arbitrary child times out (E0102, 60 s). Law: cancellation is preemptive at wait points (D-CANCELMODEL1), #Context deadlines emit E3003 (D-DEADLINE1), on_interrupt is Ctrl-C only (D-OSFACTS1), graceful HTTP shutdown is shipped. Evidence: docs/research/domain-foundations/all-gaps.json.',
      plan: 'After ratification: process.on_signal(.Term | .Hup | .Int) routes the signal into cancellation of the root task; http.serve(deadline:) wraps each request in a #Context; ProcessStdin.close() sends EOF; all through the same Prelude functions on every tier; diagnostics with snapshots; backend and CLI examples with goldens.',
      criteria: [
        'A service receives SIGTERM, in-flight requests finish or are cancelled at their next wait point, the listener drains, and the process exits 0 inside the grace period. Proof: example golden with a scripted signal on every tier.',
        'A route slower than its deadline is cancelled with E3003 and the client sees a 503 with the registered body. Proof: example and snapshot.',
        'A producer writes rows to a child, closes stdin, and the child completes; the previous 60 s timeout no longer occurs. Proof: CLI example golden.',
      ],
    },
    decision: {
      id: 'D-FOUND-LIFECYCLE1', group: 'runtime',
      title: 'Signals and request deadlines as cancellation',
      gist: 'How does a service stop cleanly on SIGTERM and cancel a slow request with the cancellation Jet already has?',
      lesson: 'Jet cancels work at wait points and keeps cleanup running. The outside world cannot trigger that cancellation. Only Ctrl-C has a handler and there is no SIGTERM. A server cannot put a deadline on one request. A child\'s stdin cannot be closed to say "no more". The backend and CLI probes stopped their servers by hand and waited on a child forever.',
      story: 'Dev deploys a Jet API under systemd. On deploy, systemd sends SIGTERM and waits ten seconds. His server must finish the requests in flight, refuse new ones, and exit. Today nothing in Jet hears the signal.',
      inWild: 'Go turns a signal into a cancelled context and wraps handlers with a timeout; Node ends a child\'s stdin with end(). Every production runtime maps signals and per-request deadlines onto the same cancellation it uses everywhere else.',
      options: [
        { key: 'A', name: 'Signals cancel the root task; server deadlines are contexts; stdin closes', detail: 'SIGTERM cancels the program\'s root task at its next wait point; serve drains under that cancellation; a route deadline is a per-request #Context; ProcessStdin gains close.', code: 'fn run() -[Net, Process.Signal]> {\n    process.on_signal(.Term)\n    server :: http.serve(mux, deadline: 5s)\n    server.wait()\n}\nchild.stdin.write_all(rows)\nchild.stdin.close()' },
        { key: 'B', name: 'Library only: poll a shared flag set by a handler', detail: 'A signal handler flips a shared Bool; every loop polls it; deadlines stay on the client side.', code: 'stop :: shared false\nloop !stop { serve_one() }' },
        { key: 'C', name: 'A separate lifecycle object apart from tasks', detail: 'A new Lifecycle value with its own stop and deadline methods, independent of task cancellation.', code: 'life :: Lifecycle.new()\nlife.on_stop(fn() { server.close() })' },
      ],
      comparisons: [
        { lang: 'Go', note: 'A signal becomes a cancelled context; a handler gets a deadline.', code: 'ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGTERM)\nsrv := &http.Server{Handler: http.TimeoutHandler(mux, 5*time.Second, "slow")}\n<-ctx.Done()\nsrv.Shutdown(context.Background())' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A adds three names and no new mechanism: signals and deadlines become the cancellation every wait point already honors.',
        whyNot: [
          { key: 'B', reason: 'B makes every service copy the same polling loop and leaves deadlines on the client.' },
          { key: 'C', reason: 'C is a second way to stop work next to task cancellation; two mechanisms for one meaning.' },
        ],
        tradeoff: 'Unavoidable: SIGKILL ends the process without cleanup. Unavoidable: a CPU-bound handler notices its deadline at its next wait point, by the cancellation law.',
      },
      surface: {
        gist: 'How does a service stop cleanly on SIGTERM and cancel a slow request with the cancellation Jet already has?',
        lesson: 'Jet cancels work at wait points and keeps cleanup running. The outside world cannot trigger that cancellation. Only Ctrl-C has a handler and there is no SIGTERM. A server cannot put a deadline on one request. A child\'s stdin cannot be closed to say "no more". The backend and CLI probes stopped their servers by hand and waited on a child forever.',
        trio: {
          current: { note: 'area-backend and area-cli probes: no signal, no per-request deadline, no EOF', code: '$ jet check signal_probe.jet\nError [E1004]: `core.process` has no item `signal`\n$ jet check timeout_probe.jet\nError [E1004]: `core.http.server` has no item `timeout`\n$ jet check stdin_close.jet\nError [E0102]: `ProcessStdin` has no method `close`   // child waits forever' },
          wild: { lang: 'Go', note: 'A signal becomes a cancelled context; a handler gets a deadline.', code: 'ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGTERM)\ndefer stop()\nsrv := &http.Server{Handler: http.TimeoutHandler(mux, 5*time.Second, "slow")}\ngo srv.ListenAndServe()\n<-ctx.Done()\nsrv.Shutdown(context.Background())' },
        },
        options: [
          { key: 'A', name: 'Signals cancel the root task; deadlines are contexts; stdin closes', gist: 'Three names on the cancellation Jet already has.', gains: ['SIGTERM drains the server and runs cleanup', 'One request, one deadline, one E3003', 'Children get EOF'], losses: ['SIGKILL still skips cleanup', 'CPU-bound handlers notice at the next wait point'], proposed: { code: '// A: SIGTERM cancels the root task at its next wait point; serve\n// drains under it; a route deadline is a per-request Context\nfn run() -[Net, Process.Signal]> {\n    process.on_signal(.Term)            // Term now cancels the root task\n    server :: http.serve(mux, deadline: 5s)   // each request: #Context(5s)\n    server.wait()                       // returns when the drain completes\n}\n// a child that needs EOF\nchild :: process.run(spec)\nchild.stdin.write_all(rows)\nchild.stdin.close()                     // EOF; the child finishes' } },
          { key: 'B', name: 'Library only: poll a shared flag set by a handler', gist: 'A handler flips a flag; every loop polls it.', gains: ['Nothing new in core'], losses: ['Every service copies the loop', 'Deadlines stay client-side', 'Children still never see EOF'], proposed: { code: '// B: poll a flag\nstop :: shared false\nprocess.on_interrupt(fn() { stop = true })\nloop !stop { serve_one() }' } },
          { key: 'C', name: 'A separate lifecycle object apart from tasks', gist: 'A new value with its own stop and deadline methods.', gains: ['Explicit object to pass around'], losses: ['A second way to stop work', 'Two mechanisms for one meaning'], proposed: { code: '// C: a second stop mechanism\nlife :: Lifecycle.new()\nlife.on_stop(fn() { server.close() })\nlife.deadline(5s)' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A adds three names and no new mechanism: signals and deadlines become the cancellation every wait point already honors.',
          whyNot: [
            { key: 'B', reason: 'B makes every service copy the same polling loop and leaves deadlines on the client.' },
            { key: 'C', reason: 'C is a second way to stop work next to task cancellation; two mechanisms for one meaning.' },
          ],
          gains: ['Clean SIGTERM drain with cleanup', 'Per-request deadlines', 'Children get EOF'],
          losses: ['SIGKILL still skips cleanup', 'CPU-bound handlers notice at the next wait point'],
          tradeoff: 'Unavoidable: SIGKILL ends the process without cleanup. Unavoidable: a CPU-bound handler notices its deadline at its next wait point, by the cancellation law.',
        },
      },
    },
  },

  // ───────────────────────────── 5. mixed-type operators ─────────────────────────────
  {
    card: {
      title: 'Operators between different types: typed right side and result on the existing hooks (amends D-OPDEF1)',
      tags: ['typelevel', 'science', 'games', 'data', 'embedded', 'numerics'],
      body: 'Probe prim-units (G3): an operator implementation with a different right-hand type is refused (E0907 `div` doesn\'t match `Div`; E0360 no `/` for Meter), so every mixed operation becomes a named helper at every call site. D-OPDEF1 fixes same-operand-type hooks; this ballot amends it. Units already mix through #UnitFamily; vectors, matrices, money-times-rate, and durations-times-counts do not. Evidence: docs/audits/domain-foundations-2026-09-02/all-gaps.json.',
      plan: 'After ratification: the operator hooks accept a right-hand type parameter and a result type (impl Vector.Mul(Float) { fn mul(self, rhs: Float) Vector }); one implementation per (left, right) pair; #Commutative derives the mirror; symbols, precedence, and overload sets unchanged; diagnostics for a missing pair name both types; Syntax.rs entry for the marker; examples with goldens on every tier; docs/spec/syntax-decisions.md amended.',
      criteria: [
        'Vector * Float, Meter / Second -> Speed on plain structs, and Money * Rate compile and run with identical results on every execution tier. Proof: example goldens.',
        'A missing pair reports a registered diagnostic naming both operand types with a fix; #Commutative derives the mirror impl. Proof: snapshots and example.',
        'D-OPDEF1 is amended in docs/spec/syntax-decisions.md and Syntax.rs records #Commutative with this decision id. Proof: the spec diff and the Syntax.rs row.',
      ],
    },
    decision: {
      id: 'D-FOUND-OPMIX1', group: 'syntax',
      title: 'Operators between different types',
      gist: 'May a library define Vector * Float and Money * Rate, or must every mixed operation be a named method?',
      lesson: 'Today an operator implementation must keep the same type on both sides and as the result. A geometry package writes v.scale(2.0) and a money package writes price.times(rate); every user pays that at every call. Units already get mixed arithmetic through #UnitFamily. Other libraries do not.',
      story: 'Lena writes a 2-D game math package. Scaling a vector by a number is the most common line in her users\' code, and it must read v * 2.0. Jet refuses her impl because the right side is a Float.',
      inWild: 'Rust names the right side and the output in the trait (impl Mul<f64> for Vector { type Output = Vector }); Swift declares static func * (lhs: Vector, rhs: Double) -> Vector. Both keep symbols and precedence fixed; only the types vary.',
      options: [
        { key: 'A', name: 'Typed right side and result on the existing hooks', detail: 'The same hooks, with the right side and result named. One implementation per (left, right) pair. #Commutative derives the mirror. Symbols, precedence, and overload sets stay as they are.', code: 'impl Vector.Mul(Float) #Commutative {\n    fn mul(self, rhs: Float) Vector -> Vector{ x: self.x * rhs, y: self.y * rhs }\n}\nv :: Vector{ x: 1.0, y: 2.0 } * 2.0' },
        { key: 'B', name: 'Keep same-type operators; named methods for mixed cases', detail: 'No change. Mixed operations stay named helpers.', code: 'v :: Vector{ x: 1.0, y: 2.0 }.scale(2.0)\ns :: meters_per_second(distance, elapsed)' },
        { key: 'C', name: 'Full operator overloading with overload sets and new symbols', detail: 'Any number of implementations per symbol and user-defined symbols.', code: 'operator <*> (a: Vector, b: Vector) Float -> dot(a, b)' },
      ],
      comparisons: [
        { lang: 'Swift', note: 'The operator names both operand types and the result.', code: 'static func * (lhs: Vector, rhs: Double) -> Vector {\n    Vector(x: lhs.x * rhs, y: lhs.y * rhs)\n}\nlet v = Vector(x: 1, y: 2) * 2.0' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A gives libraries the operators their users expect while keeping every rule that made D-OPDEF1 safe: fixed symbols, fixed precedence, one impl per pair.',
        whyNot: [
          { key: 'B', reason: 'B keeps call-site ceremony everywhere and lets each library invent a name for multiplication.' },
          { key: 'C', reason: 'C brings overload ambiguity and unreadable symbols, the reasons D-OPDEF1 exists.' },
        ],
        tradeoff: 'Designed away: the mirror order is derived by #Commutative, so no pair is written twice. No remaining loss was found.',
      },
      surface: {
        gist: 'May a library define Vector * Float and Money * Rate, or must every mixed operation be a named method?',
        lesson: 'Today an operator implementation must keep the same type on both sides and as the result. A geometry package writes v.scale(2.0) and a money package writes price.times(rate); every user pays that at every call. Units already get mixed arithmetic through #UnitFamily. Other libraries do not.',
        trio: {
          current: { note: 'prim-units probe with plain structs: a different right side is refused', code: 'impl Meter.Div { fn div(self, rhs: Second) Speed -> ... }\nError [E0907]: `div` doesn\'t match `Div` (impl methods must match the trait signature exactly)\nError [E0360]: No `/` operator is defined for `Meter`\n// the workaround, paid by every caller\nspeed :: meters_per_second(distance, elapsed)' },
          wild: { lang: 'Swift', note: 'The operator names both operand types and the result.', code: 'struct Vector { var x: Double, y: Double }\nstatic func * (lhs: Vector, rhs: Double) -> Vector {\n    Vector(x: lhs.x * rhs, y: lhs.y * rhs)\n}\nlet v = Vector(x: 1, y: 2) * 2.0' },
        },
        options: [
          { key: 'A', name: 'Typed right side and result on the existing hooks', gist: 'Same hooks, same symbols; the pair and result are named.', gains: ['v * 2.0 and price * rate read as math', 'One impl per pair; mirror derived', 'Symbols and precedence unchanged'], losses: [], proposed: { code: '// A: the same hooks, with the right side and result named; one\n// implementation per (left, right) pair; symbols and precedence unchanged\nimpl Vector.Mul(Float) #Commutative {\n    fn mul(self, rhs: Float) Vector -> Vector{ x: self.x * rhs, y: self.y * rhs }\n}\nimpl Meter.Div(Second) {\n    fn div(self, rhs: Second) Speed -> Speed{ value: self.value / rhs.value }\n}\nv :: Vector{ x: 1.0, y: 2.0 } * 2.0      // Vector; 2.0 * v also works\ns :: distance / elapsed                    // Speed' } },
          { key: 'B', name: 'Keep same-type operators; named methods for mixed cases', gist: 'No change; mixed operations stay helpers.', gains: ['No overload resolution to build'], losses: ['Ceremony at every call site', 'Each library invents a name for *'], proposed: { code: '// B: named helpers, paid by every caller\nv :: Vector{ x: 1.0, y: 2.0 }.scale(2.0)\ns :: meters_per_second(distance, elapsed)' } },
          { key: 'C', name: 'Full operator overloading with overload sets and new symbols', gist: 'Any number of impls per symbol and user-defined symbols.', gains: ['Maximum expressiveness'], losses: ['Overload ambiguity', 'Unreadable symbols', 'Side-effect meanings hidden in operators'], proposed: { code: '// C: new symbols and overload sets\noperator <*> (a: Vector, b: Vector) Float -> dot(a, b)\noperator * (a: Vector, b: Vector) Vector -> cross(a, b)' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A gives libraries the operators their users expect while keeping every rule that made D-OPDEF1 safe: fixed symbols, fixed precedence, one impl per pair.',
          whyNot: [
            { key: 'B', reason: 'B keeps call-site ceremony everywhere and lets each library invent a name for multiplication.' },
            { key: 'C', reason: 'C brings overload ambiguity and unreadable symbols, the reasons D-OPDEF1 exists.' },
          ],
          gains: ['Mixed-type math reads as math', 'One impl per pair, mirror derived', 'Symbols and precedence unchanged'],
          losses: [],
          tradeoff: 'Designed away: the mirror order is derived by #Commutative, so no pair is written twice. No remaining loss was found.',
        },
      },
    },
  },

  // ───────────────────────────── 6. literals for library types ─────────────────────────────
  {
    card: {
      title: 'Literals for library types: a literal capability on the existing context rule',
      tags: ['typelevel', 'numerics', 'data', 'backend', 'science'],
      body: 'Probe prim-exact-numerics (G4): a plain literal does not reach a library type (E0112 accept wants Rational but literal 1 is Int) and a suffix is refused (E0134 `rat`), so Rational and scaled Decimal values need a constructor at every call. Law: context selects a literal\'s type (D-LITCARRIER1); user-defined prefixes are deferred (D-UNIFYLIT1). Evidence: docs/audits/domain-foundations-2026-09-02/all-gaps.json.',
      plan: 'After ratification: a type adopts Literal.Int or Literal.Float through a compile-time constructor (@fn from_literal); the Float form receives the literal digits as text so exact types stay exact; the existing context rule picks the target; no new syntax; diagnostics for a lossy or out-of-range literal with snapshots; example with goldens on every tier.',
      criteria: [
        'accept(1) with accept taking a Rational, and sum(0.10, 0.20) with a scaled Decimal, compile and produce exact values on every execution tier. Proof: example goldens.',
        'A literal the type cannot represent exactly is a registered compile error with a UI snapshot. Proof: snapshot.',
        'Without a typed context the literal stays Int or Float as today; the existing literal goldens are unchanged. Proof: goldens sweep.',
      ],
    },
    decision: {
      id: 'D-FOUND-LITERAL1', group: 'syntax',
      title: 'Literals for library types',
      gist: 'May a library type accept a plain literal, so accept(1) builds a Rational and 0.10 a Decimal?',
      lesson: 'A literal takes its type from context in Jet, but only for built-in carriers. A package\'s Rational or scaled Decimal needs Rational{numerator: 1, denominator: 1} at every call, and a suffix like 1rat is refused. Swift solves this with a protocol a type adopts; Jet can do the same with no new syntax.',
      story: 'Sam builds an exact money package. His users write total = price * 3 and tax = 0.10 in every program. Today each of those literals needs a constructor call, and the money code reads worse than the Python it replaces.',
      inWild: 'Swift types adopt ExpressibleByIntegerLiteral and ExpressibleByFloatLiteral; the compiler calls the initializer where context asks for the type. Rust has no literal hook and users write Decimal::from_str("0.10") everywhere, a known pain.',
      options: [
        { key: 'A', name: 'Literal capability on the existing context rule', detail: 'A type adopts a literal capability with a compile-time constructor. The float form receives the digits as text so exact types stay exact. Context already picks the target type; nothing new to type.', code: 'impl Rational.Literal.Int {\n    @fn from_literal(value: Int) Rational -> Rational{ numerator: value, denominator: 1 }\n}\naccept(1)' },
        { key: 'B', name: 'Library suffixes like 1rat and 0.10dec', detail: 'A package declares suffixes the way unit families do.', code: 'total :: 1rat + 2rat\ntax :: 0.10dec' },
        { key: 'C', name: 'No change: constructors or factory calls', detail: 'Keep constructors at every call.', code: 'accept(Rational{ numerator: 1, denominator: 1 })\ntax :: Decimal2.parse("0.10") ?? panic("bad literal")' },
      ],
      comparisons: [
        { lang: 'Swift', note: 'A type adopts a literal protocol; context does the rest.', code: 'struct Rational: ExpressibleByIntegerLiteral {\n    init(integerLiteral value: Int) { self.init(value, 1) }\n}\nlet r: Rational = 1\naccept(3)' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A removes the ceremony with a capability a type adopts, keeps exactness by passing digits as text, and adds no syntax.',
        whyNot: [
          { key: 'B', reason: 'B grows a suffix zoo, collides with unit suffixes, and D-UNIFYLIT1 deferred it for that reason.' },
          { key: 'C', reason: 'C keeps ceremony at every call and makes exact money code read worse than Python.' },
        ],
        tradeoff: 'Designed away: exactness is kept by handing the float literal\'s digits to the type as text. No remaining loss was found.',
      },
      surface: {
        gist: 'May a library type accept a plain literal, so accept(1) builds a Rational and 0.10 a Decimal?',
        lesson: 'A literal takes its type from context in Jet, but only for built-in carriers. A package\'s Rational or scaled Decimal needs Rational{numerator: 1, denominator: 1} at every call, and a suffix like 1rat is refused. Swift solves this with a protocol a type adopts; Jet can do the same with no new syntax.',
        trio: {
          current: { note: 'prim-exact-numerics probe: literals do not reach library types', code: 'accept(1)\nError [E0112]: accept wants Rational but literal 1 is Int\ntotal :: 1rat\nError [E0134]: `rat` isn\'t a known unit suffix\n// the workaround at every call\naccept(Rational{ numerator: 1, denominator: 1 })' },
          wild: { lang: 'Swift', note: 'A type adopts a literal protocol; context does the rest.', code: 'struct Rational: ExpressibleByIntegerLiteral {\n    init(integerLiteral value: Int) { self.init(value, 1) }\n}\nlet r: Rational = 1          // no constructor at the call\naccept(3)                    // accept(_ r: Rational)' },
        },
        options: [
          { key: 'A', name: 'Literal capability on the existing context rule', gist: 'A type adopts a literal capability; context builds the value.', gains: ['accept(1) and 0.10 just work for library types', 'Exact digits reach exact types', 'No new syntax or suffix table'], losses: [], proposed: { code: '// A: a type adopts a literal capability; the compiler builds the\n// value at compile time wherever context already picks a literal\'s type\nimpl Rational.Literal.Int {\n    @fn from_literal(value: Int) Rational -> Rational{ numerator: value, denominator: 1 }\n}\nimpl Decimal2.Literal.Float {\n    @fn from_literal(digits: String) Decimal2 !Err -> Decimal2.parse(digits)\n}\naccept(1)                     // Rational, built at compile time\ntotal :: sum(0.10, 0.20)      // exact Decimal2; a lossy literal fails to compile' } },
          { key: 'B', name: 'Library suffixes like 1rat and 0.10dec', gist: 'Packages declare suffixes the way unit families do.', gains: ['Explicit at the call site'], losses: ['A suffix zoo across packages', 'Collides with unit suffixes', 'Deferred by D-UNIFYLIT1'], proposed: { code: '// B: suffixes\ntotal :: 1rat + 2rat\ntax :: 0.10dec' } },
          { key: 'C', name: 'No change: constructors or factory calls', gist: 'Keep constructors at every call.', gains: ['Nothing to build'], losses: ['Ceremony at every call', 'Exact money code reads worse than Python'], proposed: { code: '// C: today\naccept(Rational{ numerator: 1, denominator: 1 })\ntax :: Decimal2.parse("0.10") ?? panic("bad literal")' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A removes the ceremony with a capability a type adopts, keeps exactness by passing digits as text, and adds no syntax.',
          whyNot: [
            { key: 'B', reason: 'B grows a suffix zoo, collides with unit suffixes, and D-UNIFYLIT1 deferred it for that reason.' },
            { key: 'C', reason: 'C keeps ceremony at every call and makes exact money code read worse than Python.' },
          ],
          gains: ['Library types take plain literals', 'Exact digits reach exact types', 'No new syntax'],
          losses: [],
          tradeoff: 'Designed away: exactness is kept by handing the float literal\'s digits to the type as text. No remaining loss was found.',
        },
      },
    },
  },

  // ───────────────────────────── 7. zero-copy views ─────────────────────────────
  {
    card: {
      title: 'Zero-copy views over text, bytes, and mapped files',
      tags: ['memory', 'text', 'cli', 'web', 'ai-ml', 'data', 'backend'],
      body: 'Probes prim-text (G2), prim-storage (G1), prim-numerics-perf (G3): core.text returns a new String per grapheme, word, and line (fixed_sigs.rs:2250-2263), there is no byte view (E1004 core.text.bytes) and no memory mapping (E1004 core.files.mmap); a 100 MB scan measured 6.9 s in Jet against 33 ms in Rust and a BPE tokenizer 108 s against 3.2 s in Python. Law: View<T>/ViewMut<T> with provenance (D-MEM-VIEWRET1), read-view into a non-view slot copies (D-MEM-COPYSEM1), Pin (D-PIN1-3). Evidence: docs/research/domain-foundations/all-gaps.json.',
      plan: 'After ratification: text and byte iteration APIs return View<str> and View<[U8]> (storing them copies by the existing law, so callers do not change); files.map(path) lends a private read-only mapping whose windows are byte views pinned for the mapping\'s life; shared mappings stay an expert #Unsafe path; the same Prelude functions serve every tier; goldens for the 100 MB scan and the tokenizer with timings against Rust and Python.',
      criteria: [
        'A 100 MB line scan through text views runs under 150 ms in release and the BPE tokenizer under 5 s on a 1 MiB corpus, with identical output on every execution tier. Proof: timing goldens.',
        'Storing a returned view into a struct field or list copies exactly as D-MEM-COPYSEM1 says and existing text goldens are unchanged. Proof: goldens sweep.',
        'files.map reads a 1 GB file through byte views without reading it whole, refuses a write while mapped, and a view escaping the mapping is a registered compile error. Proof: example and snapshot.',
      ],
    },
    decision: {
      id: 'D-FOUND-VIEW1', group: 'runtime',
      title: 'Zero-copy views over text, bytes, and mapped files',
      gist: 'Can a scanner walk 100 MB of text or bytes without copying every piece into a new String?',
      lesson: 'Jet\'s View<T> already lends part of a value without copying, with checked provenance. But the text API returns fresh Strings for every grapheme, word, and line, and files have no memory mapping. The text probe measured a 100 MB scan at 6.9 s in Jet against 33 ms in Rust: allocation, not logic. Views over text, bytes, and mapped files close that gap in one mechanism.',
      story: 'Ana writes a log tool. It scans gigabytes of lines and keeps a few. Each line becomes a new String today, so her tool is two hundred times slower than ripgrep and she cannot map the file at all.',
      inWild: 'Rust slices borrow the buffer (&str, &[u8]) and memmap2 lends the file itself; Go strings and []byte share backing storage; Java has CharSequence views and MappedByteBuffer. None copies per token.',
      options: [
        { key: 'A', name: 'Views everywhere: text spans, byte windows, mapped files', detail: 'Text and byte APIs return views. Storing a view copies by the existing law, so old code keeps working. files.map lends a private read-only mapping whose windows are byte views.', code: 'loop word in text.word_views(chunk) { if word == "ERROR" { count += 1 } }\nmap :: files.map(path)\nloop line in map.lines() { if line.starts_with(b"ERROR") { count += 1 } }' },
        { key: 'B', name: 'Return offsets, not views', detail: 'APIs return Int spans and callers slice by hand.', code: 'loop span in text.word_spans(chunk) {\n    if chunk.slice(span.start, span.end) == "ERROR" { count += 1 }\n}' },
        { key: 'C', name: 'Copy, but faster', detail: 'Keep returning Strings but allocate them in an arena.', code: 'arena :: mem.arena()\nloop word in text.words_in(arena, chunk) { ... }' },
      ],
      comparisons: [
        { lang: 'Rust', note: 'Slices borrow the buffer; mmap lends the file itself.', code: 'let map = unsafe { Mmap::map(&file)? };\nfor line in map.split(|b| *b == b\'\\n\') {\n    if line.starts_with(b"ERROR") { count += 1 }\n}' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A uses the view law Jet already ratified, keeps every caller working through the copy-on-store rule, and reaches the incumbents\' speed.',
        whyNot: [
          { key: 'B', reason: 'B pushes slicing and Unicode boundaries onto every caller; off-by-one bugs return.' },
          { key: 'C', reason: 'C still copies every token and leaks arenas into every API.' },
        ],
        tradeoff: 'Unavoidable: a view cannot cross a task or channel, by the memory law. Unavoidable: a mapping holds a lock on the file so its bytes cannot change under a view.',
      },
      surface: {
        gist: 'Can a scanner walk 100 MB of text or bytes without copying every piece into a new String?',
        lesson: 'Jet\'s View<T> already lends part of a value without copying, with checked provenance. But the text API returns fresh Strings for every grapheme, word, and line, and files have no memory mapping. The text probe measured a 100 MB scan at 6.9 s in Jet against 33 ms in Rust: allocation, not logic. Views over text, bytes, and mapped files close that gap in one mechanism.',
        trio: {
          current: { note: 'prim-text and prim-storage probes: every unit is a new String; no bytes, no mapping', code: 'loop word in text.words(chunk) { count += 1 }   // words: [String]\n$ jet check byte_gap.jet\nError [E1004]: `core.text` has no item `bytes`\n$ jet check mmap_probe.jet\nError [E1004]: `core.files` has no item `mmap`\n// measured: 100 MB scan 6.9 s (Jet) vs 0.033 s (Rust)' },
          wild: { lang: 'Rust', note: 'Slices borrow the buffer; mmap lends the file itself.', code: 'let map = unsafe { Mmap::map(&file)? };          // &[u8], no copy\nfor line in map.split(|b| *b == b\'\\n\') {         // borrowed slices\n    if line.starts_with(b"ERROR") { count += 1 }\n}' },
        },
        options: [
          { key: 'A', name: 'Views everywhere: text spans, byte windows, mapped files', gist: 'The view law Jet has, returned by text, bytes, and files.', gains: ['Scans at incumbent speed, no per-token allocation', 'Old code keeps working: storing a view copies', 'Large files read through a private mapping'], losses: ['A view cannot cross a task or channel', 'A mapping locks the file while open'], proposed: { code: '// A: the same View<T> the memory law already has, now returned by\n// text, bytes, and a private read-only file mapping\nloop word in text.word_views(chunk) {            // View<str>: no copy\n    if word == "ERROR" { count += 1 }\n}\nkept :: text.word_views(chunk).first()           // stored: copies (law)\nmap :: files.map(path)                           // private, read-only\nloop line in map.lines() {                       // byte views into the map\n    if line.starts_with(b"ERROR") { count += 1 }\n}' } },
          { key: 'B', name: 'Return offsets, not views', gist: 'APIs return Int spans; callers slice by hand.', gains: ['No change to the view law'], losses: ['Every caller slices by hand', 'Unicode boundary bugs return'], proposed: { code: '// B: spans\nloop span in text.word_spans(chunk) {\n    if chunk.slice(span.start, span.end) == "ERROR" { count += 1 }\n}' } },
          { key: 'C', name: 'Copy, but faster', gist: 'Keep Strings; allocate them in an arena.', gains: ['API unchanged'], losses: ['Still copies every token', 'Arenas leak into every API', 'Still far from incumbents'], proposed: { code: '// C: arena strings\narena :: mem.arena()\nloop word in text.words_in(arena, chunk) { ... }' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A uses the view law Jet already ratified, keeps every caller working through the copy-on-store rule, and reaches the incumbents\' speed.',
          whyNot: [
            { key: 'B', reason: 'B pushes slicing and Unicode boundaries onto every caller; off-by-one bugs return.' },
            { key: 'C', reason: 'C still copies every token and leaks arenas into every API.' },
          ],
          gains: ['Incumbent speed for scans and tokenizers', 'Old code keeps working', 'Large files through mappings'],
          losses: ['A view cannot cross a task or channel', 'A mapping locks the file while open'],
          tradeoff: 'Unavoidable: a view cannot cross a task or channel, by the memory law. Unavoidable: a mapping holds a lock on the file so its bytes cannot change under a view.',
        },
      },
    },
  },

  // ───────────────────────────── 8. typed receipt sections ─────────────────────────────
  {
    card: {
      title: 'Typed sections on the one receipt, with shared query and diff',
      tags: ['receipts', 'data', 'backend', 'embedded', 'ai-ml', 'tooling'],
      body: 'Probes prim-receipts (G1), area-embedded (G6), prim-tooling-hooks (G2): jet-receipt-v2 fixes claim/status/stdout/stderr/digest (Source/ReceiptStore.rs:47-54, 301-328); typed domain evidence (a regression fit, a flash checksum, a model metric) stays in printed JSON that jet status and jet perf compare never see. Law: one receipt and one status truth (D-DEVR-TWICE1, D-DEVR-STATUS1, D-CLAIM1); closed artifact suffixes (D-ARTIFACT-EXT1). Evidence: docs/audits/domain-foundations-2026-09-02/all-gaps.json.',
      plan: 'After ratification: #Receipt("name") on a #Codable record declares a typed section; receipt.attach(value) writes it under the program\'s receipt without changing identity, claim, or the v2 fields; jet status --json, jet receipt diff, and jet perf compare read sections by name; sections are size-capped with a registered error; the same Prelude function serves every tier; example with goldens.',
      criteria: [
        'A program attaches a typed section and jet status --json shows it under sections with the receipt identity unchanged. Proof: example golden and a receipt digest comparison.',
        'jet receipt diff between two runs prints per-field deltas of the typed section, and jet perf compare shows the section beside the wall trace. Proof: CLI goldens.',
        'A section over the size cap or without #Codable is a registered diagnostic with a snapshot. Proof: snapshot.',
      ],
    },
    decision: {
      id: 'D-FOUND-RECEIPT1', group: 'tooling',
      title: 'Typed sections on the one receipt',
      gist: 'May a library attach its own typed evidence to Jet\'s receipt and query or diff it with the same tools?',
      lesson: 'Jet has one receipt for check, build, test, prove, and benchmarks, and one status view over it. A data pipeline, a training run, or a firmware build wants to say more: r-squared, loss curve, flash checksum. Today that evidence lives in printed JSON that jet status and jet perf compare never see. The receipt\'s fixed fields must not change.',
      story: 'Noor trains a model nightly. She wants jet status to show last night\'s loss, and a diff between two runs to show what moved. Today she greps stdout.',
      inWild: 'MLflow runs carry typed metrics and params beside artifacts and the UI diffs two runs by field. Bazel\'s build event protocol and GitHub check runs attach typed outputs to one canonical record. Nobody keeps a second ledger.',
      options: [
        { key: 'A', name: 'Typed receipt sections declared by the program', detail: 'A #Receipt record is written under the program\'s receipt as a named typed section. Status, compare, and diff read it like built-in fields. Identity and the fixed fields never change.', code: '#Receipt("regression")\nstruct RegressionEvidence { slope: Float, r2: Float }\nfn run() {\n    fit :: ols(rows)\n    receipt.attach(RegressionEvidence{ slope: fit.slope, r2: fit.r2 })\n}' },
        { key: 'B', name: 'Library JSON files next to the receipt', detail: 'Programs write sidecar files; tools ignore them.', code: 'files.write("evidence.json", json.to_string(fit))' },
        { key: 'C', name: 'Free-form key/value tags on the receipt', detail: 'Untyped string tags on the receipt.', code: 'receipt.tag("r2", "0.998")' },
      ],
      comparisons: [
        { lang: 'Python (MLflow)', note: 'A run carries typed metrics beside the artifact; the UI diffs runs by field.', code: 'with mlflow.start_run():\n    mlflow.log_param("model", "ols")\n    mlflow.log_metric("r2", 0.998)\n    mlflow.log_artifact("fit.json")' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A keeps one receipt and one status truth while letting every domain\'s evidence ride the same tools, typed and diffable.',
        whyNot: [
          { key: 'B', reason: 'B creates a second truth that status and compare never read, and no identity check covers it.' },
          { key: 'C', reason: 'C is untyped: no schema, string diffs, and silent typos in keys.' },
        ],
        tradeoff: 'Designed away: sections are capped by the store\'s existing size rule with a clear error, so the store cannot bloat. No remaining loss was found.',
      },
      surface: {
        gist: 'May a library attach its own typed evidence to Jet\'s receipt and query or diff it with the same tools?',
        lesson: 'Jet has one receipt for check, build, test, prove, and benchmarks, and one status view over it. A data pipeline, a training run, or a firmware build wants to say more: r-squared, loss curve, flash checksum. Today that evidence lives in printed JSON that jet status and jet perf compare never see. The receipt\'s fixed fields must not change.',
        trio: {
          current: { note: 'prim-receipts probe: typed evidence stays outside the receipt', code: 'evidence :: RegressionEvidence{ slope: 1.9, r2: 0.998 }\nprint(json.to_string(evidence))       // a line of stdout\n$ jet status src/evidence.jet --json  // action, claim, reason, receipt, state\n$ jet perf compare baseline shifted   // wall trace only; typed values unseen' },
          wild: { lang: 'Python (MLflow)', note: 'A run carries typed metrics beside the artifact; runs diff by field.', code: 'with mlflow.start_run():\n    mlflow.log_param("model", "ols")\n    mlflow.log_metric("r2", 0.998)\n    mlflow.log_artifact("fit.json")\n# mlflow runs compare <run_a> <run_b>' },
        },
        options: [
          { key: 'A', name: 'Typed receipt sections declared by the program', gist: 'A typed record rides the receipt; status and diff read it.', gains: ['Domain evidence is typed, queried, and diffed', 'One receipt, one status truth', 'Identity and fixed fields unchanged'], losses: [], proposed: { code: '// A: a #Receipt record is written under the program\'s receipt as a\n// named typed section; status, compare, and diff read it like built-ins\n#Receipt("regression")\nstruct RegressionEvidence { slope: Float, r2: Float }\nfn run() {\n    fit :: ols(rows)\n    receipt.attach(RegressionEvidence{ slope: fit.slope, r2: fit.r2 })\n}\n$ jet status --json          // ... "sections": { "regression": {...} }\n$ jet receipt diff base new  // regression.r2: 0.991 -> 0.998' } },
          { key: 'B', name: 'Library JSON files next to the receipt', gist: 'Programs write sidecar files; tools ignore them.', gains: ['Nothing in core'], losses: ['Two truths', 'Tools stay blind', 'No identity check'], proposed: { code: '// B: a sidecar\nfiles.write("evidence.json", json.to_string(fit))' } },
          { key: 'C', name: 'Free-form key/value tags on the receipt', gist: 'Untyped string tags.', gains: ['Simplest to build'], losses: ['No schema', 'Diffs are string compares', 'Silent key typos'], proposed: { code: '// C: tags\nreceipt.tag("r2", "0.998")' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A keeps one receipt and one status truth while letting every domain\'s evidence ride the same tools, typed and diffable.',
          whyNot: [
            { key: 'B', reason: 'B creates a second truth that status and compare never read, and no identity check covers it.' },
            { key: 'C', reason: 'C is untyped: no schema, string diffs, and silent typos in keys.' },
          ],
          gains: ['Typed domain evidence on the one receipt', 'Diff and compare read it', 'Identity unchanged'],
          losses: [],
          tradeoff: 'Designed away: sections are capped by the store\'s existing size rule with a clear error, so the store cannot bloat. No remaining loss was found.',
        },
      },
    },
  },

  // ───────────────────────────── 9. GUI platform services ─────────────────────────────
  {
    card: {
      title: 'Platform services in core.ui: dialogs, clipboard, IME, drag, shortcuts, accessible names, text shaping',
      tags: ['gui', 'ui', 'text', 'accessibility'],
      body: 'Probe area-gui (G2-G6) and prim-text (G4): core.ui has no file dialogs, clipboard/selection, IME composition, drag and drop, shortcut registry, accessible names (only a role enum; D-A11YGATE1 is a lint), and no core.font shaping (E1004/E1001 for each). A library cannot reach these safely: they live in the operating system. Law: ui.* callable surface is current (D-ONCE-UITREE1=C), click is the portable core event with richer events by explicit capability (D-UI-EVT-SET1=D), same-source native UI (D-NATIVEUI3=A). Evidence: docs/research/domain-foundations/all-gaps.json.',
      plan: 'After ratification: ui.host exposes typed services (open_file/save_file with filters and cancellation under a resource-scoped FS grant, clipboard with a UI.Clipboard capability, IME composition and drag/drop on text_input, shortcut: on button and a host action table, label:/description: on nodes reaching the native accessibility tree, and font.shape returning glyph runs through the HarfBuzz binding); headless hosts stub each service deterministically; one example per service with goldens on headless and native hosts.',
      criteria: [
        'Open and save dialogs, clipboard read/write, IME composition, drag/drop, Ctrl+S shortcut, and an accessible name are exercised in one example on the native host and deterministically headless on every execution tier. Proof: goldens and a native host screenshot.',
        'An Arabic and an emoji string shape to positioned glyph runs through font.shape with the same output on every tier. Proof: golden.',
        'A service used without its capability is a registered compile error with a snapshot. Proof: snapshot.',
      ],
    },
    decision: {
      id: 'D-FOUND-PLATFORM1', group: 'web-ui',
      title: 'Platform services in core.ui',
      gist: 'How does a desktop app open a file dialog, paste, take IME input, and name its controls for a screen reader?',
      lesson: 'core.ui draws trees, routes clicks, and mounts on three hosts. A real editor also needs the host\'s services: clipboard, selection, input-method composition, drag and drop, file dialogs, shortcuts, and accessible names. None exist, and a library cannot reach them safely because they live in the operating system. Text shaping (fonts, Arabic, emoji) is the same kind of service.',
      story: 'Kai builds a notes app. Paste, Ctrl+S, "Open file", a screen-reader label on the save button, and Japanese input through an IME are all missing. None can be added from a package without unsafe OS calls.',
      inWild: 'SwiftUI attaches services as modifiers: .keyboardShortcut, .accessibilityLabel, .fileImporter; UIPasteboard and UITextInput handle clipboard and IME. Flutter has Clipboard, Semantics, file_selector, and TextInput with composition. Both make services typed values on the same tree.',
      options: [
        { key: 'A', name: 'One ui.host service set, typed and capability-gated', detail: 'Services live under ui.host as typed values. Each carries its own capability, so a package must ask for the clipboard or file dialogs. Labels and shortcuts are node arguments. font.shape returns glyph runs.', code: 'button("Save", shortcut: .cmd("s"), label: "Save the note") { save() }\npath :: ui.host.open_file(filters: [.text]) ?? return\ntext :: ui.host.clipboard.read_text()\neditor :: text_input(state, ime: .native, on_drop: fn(items) { ... })' },
        { key: 'B', name: 'Leave services to packages over FFI', detail: 'Each GUI package binds GTK, Cocoa, or Win32 itself.', code: 'use c.gtk\n#Unsafe("gtk clipboard") { gtk.gtk_clipboard_get(...) }' },
        { key: 'C', name: 'Bind one toolkit (GTK) as the service layer', detail: 'core.ui exposes GTK\'s services directly on every platform.', code: 'ui.gtk.file_chooser_dialog(...)' },
      ],
      comparisons: [
        { lang: 'Swift (SwiftUI)', note: 'Services are typed modifiers on the same tree.', code: 'Button("Save") { save() }\n    .keyboardShortcut("s", modifiers: .command)\n    .accessibilityLabel("Save the note")\n    .fileImporter(isPresented: $open, allowedContentTypes: [.text]) { r in load(r) }' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A gives every host the same typed services, gated by the rights tree. A notes app is then safe on desktop, headless in tests, and honest about its needs.',
        whyNot: [
          { key: 'B', reason: 'B makes every GUI package bind the OS itself, unsafe, with no headless test host.' },
          { key: 'C', reason: 'C ships one toolkit\'s look everywhere, has no mobile path, and adds a second UI model.' },
        ],
        tradeoff: 'Unavoidable: headless hosts stub services (a dialog returns cancelled) because there is no OS behind them. Unavoidable: each native host implements the set separately.',
      },
      surface: {
        gist: 'How does a desktop app open a file dialog, paste, take IME input, and name its controls for a screen reader?',
        lesson: 'core.ui draws trees, routes clicks, and mounts on three hosts. A real editor also needs the host\'s services: clipboard, selection, input-method composition, drag and drop, file dialogs, shortcuts, and accessible names. None exist, and a library cannot reach them safely because they live in the operating system. Text shaping (fonts, Arabic, emoji) is the same kind of service.',
        trio: {
          current: { note: 'area-gui probe: every host service is missing', code: '$ jet check missing_dialog_api.jet\nError [E1004]: `core.ui` has no item `file_open_dialog`\nError [E1004]: `core.ui` has no item `clipboard_read`\nError [E1004]: `core.ui` has no item `shortcut`\nError [E1004]: `core.ui` has no item `aria_label`\nError [E1001]: There is no core module `core.font`' },
          wild: { lang: 'Swift (SwiftUI)', note: 'Services are typed modifiers on the same tree.', code: 'Button("Save") { save() }\n    .keyboardShortcut("s", modifiers: .command)\n    .accessibilityLabel("Save the note")\n    .fileImporter(isPresented: $open, allowedContentTypes: [.text]) { r in load(r) }\n// clipboard: UIPasteboard.general.string; IME: UITextInput' },
        },
        options: [
          { key: 'A', name: 'One ui.host service set, typed and capability-gated', gist: 'Typed services on the same tree, each behind its capability.', gains: ['Dialogs, clipboard, IME, drag, shortcuts, names, shaping', 'Same code on desktop, mobile, and headless tests', 'A package must ask for the clipboard'], losses: ['Headless hosts stub services', 'Each native host implements the set'], proposed: { code: '// A: services live under ui.host, are typed values, and each carries\n// its own capability so a package must ask for the clipboard or files\nbutton("Save", shortcut: .cmd("s"), label: "Save the note") { save() }\npath :: ui.host.open_file(filters: [.text]) ?? return     // FS.Read scoped\ntext :: ui.host.clipboard.read_text()                     // UI.Clipboard\neditor :: text_input(state, ime: .native, on_drop: fn(items) { ... })\nrun :: font.shape("مرحبا", face: font.system(.body))      // glyph runs' } },
          { key: 'B', name: 'Leave services to packages over FFI', gist: 'Each GUI package binds the OS itself.', gains: ['Core stays small'], losses: ['Unsafe OS calls in every package', 'No headless test host', 'Three bindings per feature'], proposed: { code: '// B: bind the toolkit yourself\nuse c.gtk\n#Unsafe("gtk clipboard") { gtk.gtk_clipboard_get(...) }' } },
          { key: 'C', name: 'Bind one toolkit (GTK) as the service layer', gist: 'GTK\'s services on every platform.', gains: ['Fast to ship'], losses: ['One toolkit\'s look everywhere', 'No mobile path', 'A second UI model'], proposed: { code: '// C: one toolkit everywhere\nui.gtk.file_chooser_dialog(...)' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A gives every host the same typed services, gated by the rights tree. A notes app is then safe on desktop, headless in tests, and honest about its needs.',
          whyNot: [
            { key: 'B', reason: 'B makes every GUI package bind the OS itself, unsafe, with no headless test host.' },
            { key: 'C', reason: 'C ships one toolkit\'s look everywhere, has no mobile path, and adds a second UI model.' },
          ],
          gains: ['Every host service, typed', 'Same code on desktop, mobile, headless', 'Rights-gated'],
          losses: ['Headless hosts stub services', 'Each native host implements the set'],
          tradeoff: 'Unavoidable: headless hosts stub services (a dialog returns cancelled) because there is no OS behind them. Unavoidable: each native host implements the set separately.',
        },
      },
    },
  },

  // ───────────────────────────── 10. embedded substrate ─────────────────────────────
  {
    card: {
      title: 'Interrupts, DMA ownership, and typed register blocks from the target profile',
      tags: ['embedded', 'hardware', 'realtime'],
      body: 'Probe area-embedded (G2, G3, G7): no interrupt or vector-table surface (E1004 core.interrupt; core.sys.on_interrupt is a process hook), no DMA ownership (E1004 core.dma; mem.pin proves address stability only), and register access is hand-written #Unsafe per board (jet inspect unsafe: pointer_from_address + volatile). Law: typed target profiles with memory regions, linker, allocator, and audit (D-TARGET-SURFACE1, D-TARGET-MEMORY1, D-TARGET-LINKER1), #Layout(c) (D-REPRC1), volatile spelling (D-FLAGSHIP-MMIO1), pin (D-PIN1-3). Evidence: docs/research/domain-foundations/all-gaps.json.',
      plan: 'After ratification: the board profile carries the vendor SVD; the compiler generates typed register blocks (width, access mode, volatile) under the profile module; #Interrupt(vector, effects) binds a bounded handler that may not allocate or wait; dma.start(channel, ^buffer) transfers buffer ownership and wait() returns it; host replay keeps the same semantics; Syntax.rs entries; diagnostics with snapshots; the embedded example builds for board.sensor_v1 and replays on the host with goldens.',
      criteria: [
        'A UART receive interrupt bound with #Interrupt fills a bounded ring and a handler that allocates or waits is a registered compile error. Proof: example on board or emulator plus snapshots.',
        'A DMA transfer takes buffer ownership, the buffer is unusable until wait() returns it, and a use before completion is a registered compile error. Proof: example and snapshot.',
        'Register blocks generated from the profile are width-checked and volatile without user #Unsafe, and jet inspect unsafe reports zero user gates in the example. Proof: inspect output and golden.',
      ],
    },
    decision: {
      id: 'D-FOUND-BOARD1', group: 'runtime',
      title: 'Interrupts, DMA, and register blocks from the target profile',
      gist: 'How does firmware bind an interrupt, own a DMA buffer, and name registers without hand-written unsafe blocks?',
      lesson: 'Jet\'s target profiles already know a board\'s memory regions, linker layout, and allocator. Firmware needs three more things the profile could give it. A vector entry that runs a bounded handler. A buffer that a DMA engine owns until completion. Typed register blocks with widths and access modes. Today each is a hand-written unsafe block, and interrupts have no surface at all.',
      story: 'Tomas writes a sensor node. The UART interrupt must push bytes into a ring and a DMA channel must own the sample buffer while it fills. Registers must be typed so a 16-bit write cannot hit a 32-bit register. Today all three are raw addresses inside unsafe blocks.',
      inWild: 'Rust\'s svd2rust generates typed register blocks from the chip\'s SVD file, RTIC binds an interrupt to a task with declared shared resources, and embedded-dma transfers buffer ownership until completion. Zig and Ada SPARK have the same three ideas.',
      options: [
        { key: 'A', name: 'Profile-generated registers, bound handlers, owned DMA transfers', detail: 'The board profile carries the SVD; the compiler generates typed register blocks. A handler is bound to a vector with an effect row that forbids allocation and waiting. DMA takes the buffer by ^ and returns it on completion.', code: 'use board.sensor_v1 as board\nboard.usart1.cr1.rxneie.set()\n#Interrupt(board.USART1, -[Mem.Alloc, Time.Wait]>)\nfn on_uart() { ring.push(board.usart1.dr.read()) }\ntransfer :: dma.start(board.dma1.ch4, ^buffer)\nbuffer :: transfer.wait()' },
        { key: 'B', name: 'Library packages over #Unsafe per board', detail: 'No compiler change; each board package hand-writes registers and handlers in unsafe blocks.', code: '#Unsafe("UART data register") {\n    mem.volatile_write(mem.pointer_from_address(0x4000_C000), byte)\n}' },
        { key: 'C', name: 'Bind Rust PAC and HAL crates through the Rust bridge', detail: 'Use svd2rust output and embedded-hal from Jet via extern rust.', code: 'extern rust "stm32f4xx-hal@0.21"\ndp :: hal.pac.Peripherals.take()' },
      ],
      comparisons: [
        { lang: 'Rust (svd2rust + RTIC)', note: 'Typed registers from the SVD; an interrupt bound to a task with declared resources.', code: '#[task(binds = USART1, shared = [rx_ring])]\nfn on_uart(cx: on_uart::Context) {\n    cx.shared.rx_ring.lock(|r| r.push(dp.USART1.dr.read().bits()))\n}\ndp.USART1.cr1.write(|w| w.rxneie().set_bit());' },
      ],
      rec: 'A',
      recommendation: {
        why: 'A extends the typed profile Jet already has, so registers, handlers, and DMA are checked by the compiler and no driver needs unsafe.',
        whyNot: [
          { key: 'B', reason: 'B repeats unsafe per board, checks no widths or modes, and makes every driver an audit.' },
          { key: 'C', reason: 'C leaks Rust types into Jet APIs, shows rustc to users, and has no interpreter or JIT parity.' },
        ],
        tradeoff: 'Unavoidable: vendor SVD files must be vetted per board because they are vendor data. Unavoidable: a handler sees only shared cells and may not wait or allocate, so it stays bounded.',
      },
      surface: {
        gist: 'How does firmware bind an interrupt, own a DMA buffer, and name registers without hand-written unsafe blocks?',
        lesson: 'Jet\'s target profiles already know a board\'s memory regions, linker layout, and allocator. Firmware needs three more things the profile could give it. A vector entry that runs a bounded handler. A buffer that a DMA engine owns until completion. Typed register blocks with widths and access modes. Today each is a hand-written unsafe block, and interrupts have no surface at all.',
        trio: {
          current: { note: 'area-embedded probe: no interrupt, no DMA, registers by hand', code: '$ jet check missing_interrupt.jet\nError [E1004]: `core` has no item `interrupt`\n$ jet check missing_dma.jet\nError [E1004]: `core` has no item `dma`\n#Unsafe("UART data register") {\n    mem.volatile_write(mem.pointer_from_address(0x4000_C000), byte)\n}' },
          wild: { lang: 'Rust (svd2rust + RTIC)', note: 'Typed registers from the SVD; an interrupt bound to a task.', code: '#[task(binds = USART1, shared = [rx_ring])]\nfn on_uart(cx: on_uart::Context) {\n    cx.shared.rx_ring.lock(|r| r.push(dp.USART1.dr.read().bits()))\n}\nlet dp = pac::Peripherals::take();\ndp.USART1.cr1.write(|w| w.rxneie().set_bit());' },
        },
        options: [
          { key: 'A', name: 'Profile-generated registers, bound handlers, owned DMA transfers', gist: 'The typed profile grows registers, handlers, and DMA ownership.', gains: ['Width and mode checked registers, no user unsafe', 'Handlers cannot allocate or wait', 'DMA buffers cannot be touched mid-transfer'], losses: ['Vendor SVD files must be vetted', 'Handlers see only shared cells'], proposed: { code: '// A: the board profile carries the SVD; the compiler generates typed\n// register blocks; a handler is bounded; DMA takes the buffer\nuse board.sensor_v1 as board                 // registers from the profile\nboard.usart1.cr1.rxneie.set()                // typed, volatile, width-checked\n#Interrupt(board.USART1, -[Mem.Alloc, Time.Wait]>)\nfn on_uart() { ring.push(board.usart1.dr.read()) }   // bounded, no alloc\ntransfer :: dma.start(board.dma1.ch4, ^buffer)      // ^ transfers ownership\nbuffer :: transfer.wait()                           // returns on completion' } },
          { key: 'B', name: 'Library packages over #Unsafe per board', gist: 'Each board package hand-writes unsafe registers and handlers.', gains: ['No compiler work'], losses: ['Unsafe repeated per board', 'No width or mode checks', 'Every driver is an audit'], proposed: { code: '// B: today, by hand\n#Unsafe("UART data register") {\n    mem.volatile_write(mem.pointer_from_address(0x4000_C000), byte)\n}' } },
          { key: 'C', name: 'Bind Rust PAC and HAL crates through the Rust bridge', gist: 'Use svd2rust and embedded-hal from Jet.', gains: ['Mature ecosystem today'], losses: ['Rust types leak into Jet APIs', 'rustc errors reach users', 'No interpreter or JIT parity'], proposed: { code: '// C: through the Rust bridge\nextern rust "stm32f4xx-hal@0.21"\ndp :: hal.pac.Peripherals.take()\ndp.USART1.cr1.write(...)' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'A extends the typed profile Jet already has, so registers, handlers, and DMA are checked by the compiler and no driver needs unsafe.',
          whyNot: [
            { key: 'B', reason: 'B repeats unsafe per board, checks no widths or modes, and makes every driver an audit.' },
            { key: 'C', reason: 'C leaks Rust types into Jet APIs, shows rustc to users, and has no interpreter or JIT parity.' },
          ],
          gains: ['Checked registers without unsafe', 'Bounded handlers', 'Owned DMA transfers'],
          losses: ['Vendor SVD files must be vetted', 'Handlers see only shared cells'],
          tradeoff: 'Unavoidable: vendor SVD files must be vetted per board because they are vendor data. Unavoidable: a handler sees only shared cells and may not wait or allocate, so it stays bounded.',
        },
      },
    },
  },

  // ───────────────────────────── 11. batch core API ─────────────────────────────
  {
    card: {
      title: 'Core API additions surfaced by the probes: eleven names, no new mechanism',
      tags: ['core-api', 'cli', 'games', 'backend', 'web', 'data', 'science', 'gui', 'embedded', 'tooling'],
      body: 'The probes surfaced eleven plain additions to shipped core modules. None is a new mechanism; each is a name a library author reached for and found missing. The owner gates them as one batch (2026-09-02): accept all, accept the essentials, or reject.\n\n1. ProcessStdin.close() sends EOF to a child (area-cli-G1; E0102).\n2. files.walk(root, ignore: .gitignore) ignore-aware traversal with nested rules, negation, anchoring (area-cli-G2).\n3. game.run(scene, replay:, frames:) frame budget for headless replay (area-games-G1; E0764).\n4. core.game.raylib gamepad buttons/axes and texture atlas decode plus draw_sprite (area-games-G2, G3; E1004).\n5. core.db.pool(url, max:) bounded acquisition, health/reset, readiness, drain (area-backend-G1; E1004).\n6. core.web OpenAPI export from Router facts with operation identity (area-backend-G2; refs e14 #2472).\n7. Stream.key_by/window/watermark event-time operators with late-event disposition (prim-distributed-G1; E0102).\n8. DateTime leap-second label 23:59:60 on UTC instants (prim-time-G1; E3002; refs open D-TIMEDEPTH1).\n9. jet package: desktop bundle, installer, signing metadata (area-gui-G7; E2101; refs e14 #2490 game packaging).\n10. jet flash --target board.<name>: program a device and record a typed receipt (area-embedded-G6; E2101).\n11. build.graph() and receipt diff as a typed in-process query for tools (prim-tooling-hooks-G2; refs D-BUILDQUERY1).\n\nEssentials, if the owner picks B: 1, 3, 5, 7, 9. Evidence per item: docs/research/domain-foundations/all-gaps.json.',
      plan: 'After ratification: each accepted item is one small card in its area milestone (created by the implementing orchestrator), implemented through the same Prelude functions on every tier, with a registered diagnostic and snapshot where a new error exists, an example with goldens, and the reference docs updated. Item 6 lands under e14 #2472; item 9 shares its bundler with e14 #2490.',
      criteria: [
        'Every accepted item has its own card in its area milestone with an example and goldens on every execution tier. Proof: the card list.',
        'Rejected items are logged here with the reason so the next probe wave does not re-raise them. Proof: the card log.',
      ],
    },
    decision: {
      id: 'D-FOUND-COREAPI1', group: 'stdlib',
      title: 'Core API additions surfaced by the probes',
      gist: 'Which of these eleven core additions, each surfaced by a probe, should land as ordinary work?',
      lesson: 'Each item adds a name to a shipped core module without a new mechanism. Examples: closing a child\'s stdin, an ignore-aware file walk, a frame budget for game replay, a bounded database pool. Also: OpenAPI export from routes, event-time stream operators, a leap-second label, a desktop bundler, a device flasher, a typed build-graph query. The full list with evidence is on the card.',
      story: 'Each of the eight area probes hit two or three of these on its way to a working program and wrote a workaround. None needed a new idea, only a name that was not there.',
      inWild: 'Node ends a child\'s stdin with end() and pools connections with pg.Pool. ripgrep walks with .gitignore rules and Godot runs headless for N frames. Flink keys and windows streams by event time. cargo-bundle packages desktop apps and probe-rs flashes boards with a report.',
      options: [
        { key: 'A', name: 'Accept all eleven as ordinary work', detail: 'Every item becomes a small card in its area milestone.', code: 'child.stdin.close()\nfiles.walk(root, ignore: .gitignore)\ngame.run(scene, replay: r, frames: 600)\ndb.pool(url, max: 10)\nstream.key_by(k).window(5s, watermark: 2s)\njet package' },
        { key: 'B', name: 'Accept the essentials only', detail: 'Items 1, 3, 5, 7, and 9 land; the rest wait for a second probe.', code: 'child.stdin.close()\ngame.run(scene, replay: r, frames: 600)\ndb.pool(url, max: 10)\nstream.key_by(k).window(5s, watermark: 2s)\njet package' },
        { key: 'C', name: 'Reject: libraries and packages do it', detail: 'Nothing lands in core; each area battery carries its own version.', code: '// no change to core' },
      ],
      comparisons: [
        { lang: 'JavaScript (Node)', note: 'A child\'s stdin can be ended; a pool is one line.', code: 'child.stdin.write(rows); child.stdin.end();\nconst pool = new Pool({ max: 10, idleTimeoutMillis: 30_000 });\nconst { rows } = await pool.query("select 1");' },
      ],
      rec: 'A',
      recommendation: {
        why: 'All eleven are names on shipped modules with probe evidence; batching them saves the owner ten ballots and the areas ten workarounds.',
        whyNot: [
          { key: 'B', reason: 'B leaves six proven gaps for a second probe wave to find again.' },
          { key: 'C', reason: 'C makes every battery re-implement core behavior and splits one meaning across packages.' },
        ],
        tradeoff: 'Designed away: each item is a small separate card, so a bad one can be frozen alone. No remaining loss was found.',
      },
      surface: {
        gist: 'Which of these eleven core additions, each surfaced by a probe, should land as ordinary work?',
        lesson: 'Each item adds a name to a shipped core module without a new mechanism. Examples: closing a child\'s stdin, an ignore-aware file walk, a frame budget for game replay, a bounded database pool. Also: OpenAPI export from routes, event-time stream operators, a leap-second label, a desktop bundler, a device flasher, a typed build-graph query. The full list with evidence is on the card.',
        trio: {
          current: { note: 'Two of the eleven, as the probes hit them', code: '$ jet check stdin_close.jet\nError [E0102]: `ProcessStdin` has no method `close`   // child waits 60 s\n$ jet check pool_probe.jet\nError [E1004]: `core.db` has no item `pool`           // one connection per service' },
          wild: { lang: 'JavaScript (Node)', note: 'A child\'s stdin can be ended; a pool is one line.', code: 'child.stdin.write(rows); child.stdin.end();\nconst pool = new Pool({ max: 10, idleTimeoutMillis: 30_000 });\nconst { rows } = await pool.query("select 1");' },
        },
        options: [
          { key: 'A', name: 'Accept all eleven as ordinary work', gist: 'Every item becomes a small card in its area milestone.', gains: ['Eleven proven gaps closed', 'One decision instead of eleven', 'Each lands as its own small card'], losses: [], proposed: { code: '// A: eleven names, no new mechanism (full list on the card)\nchild.stdin.close()                                  // EOF to a child\nfiles.walk(root, ignore: .gitignore)                 // ignore-aware walk\ngame.run(scene, replay: r, frames: 600)              // frame budget\nrl.gamepad_down(0, .A); rl.draw_sprite(atlas, "hero", x, y)\ndb.pool(url, max: 10)                                // bounded pool\nweb.openapi(router)                                  // contract export\nstream.key_by(k).window(5s, watermark: 2s)           // event time\ntime.datetime(2016, 12, 31, 23, 59, 60)              // leap-second label\njet package                                          // desktop bundle\njet flash --target board.sensor_v1                   // device receipt\nbuild.graph().diff(previous)                         // typed graph query' } },
          { key: 'B', name: 'Accept the essentials only', gist: 'Items 1, 3, 5, 7, and 9 land; the rest wait.', gains: ['Smaller first batch'], losses: ['Six proven gaps found again next wave'], proposed: { code: '// B: the essentials\nchild.stdin.close()\ngame.run(scene, replay: r, frames: 600)\ndb.pool(url, max: 10)\nstream.key_by(k).window(5s, watermark: 2s)\njet package' } },
          { key: 'C', name: 'Reject: libraries and packages do it', gist: 'Nothing lands in core.', gains: ['Core unchanged'], losses: ['Every battery re-implements core behavior', 'One meaning split across packages'], proposed: { code: '// C: no change to core; each battery carries its own version' } },
        ],
        recommendation: {
          rec: 'A',
          why: 'All eleven are names on shipped modules with probe evidence; batching them saves the owner ten ballots and the areas ten workarounds.',
          whyNot: [
            { key: 'B', reason: 'B leaves six proven gaps for a second probe wave to find again.' },
            { key: 'C', reason: 'C makes every battery re-implement core behavior and splits one meaning across packages.' },
          ],
          gains: ['Eleven proven gaps closed', 'One decision instead of eleven', 'Each lands as its own card'],
          losses: [],
          tradeoff: 'Designed away: each item is a small separate card, so a bad one can be frozen alone. No remaining loss was found.',
        },
      },
    },
  },
];

export const shortAuthorizedBy = BY;

# D-MAP-WRITE1 adversarial review — 2026-11-08

Author family: Claude Opus. Reviewer family: GPT. Fresh reviewer: ReviewMapWrite.

## Verdict

Agree with A only as a direction: checked copies and shared atomic words avoid exposing ordinary safe references into foreign-writable storage. Its fault recovery, costs and execution-tier contract remain unproved. Do not post or implement it until those gaps pass review.

B remains a valid conservative choice. C addresses private editing, not the shared counter. D is a legitimate expert option after replacing its escaping byte list with an unsafe raw capability. Added E: accept only kernel-enforced non-shrinkable backing, explicitly distinguishing volatile IPC from durable ordinary files.

## Exercised evidence

Probes reside under `$JET_SCRATCH_ROOT/lanes/Main/reviews/D-MAP-WRITE1/`. Runs used CPUs 16–31, nice 19 and a 4 GB memory cgroup.

- `Current.jet`: existing lock/read/write/fsync operations printed `visit 1`, `visit 2`, `visit 3`. An external write changed an already borrowed mapped byte from `3` to `9`.
- `Shrink.jet`: borrowed a mapped byte before external truncation. Truncation returned zero; accessing that borrowed byte produced `Fault [SIGBUS]` at line 20 and exit **135**.
- `Advice.jet`: rejected with **E0102**, because `MappedFile` has no `advise` method.
- Ballot validation returned **ok** after the final edit.

These runs do not establish concurrent-counter correctness, power-loss durability, AArch64 behavior or recovery for any proposed API. Peer comparisons below are checked source contracts, not locally executed peer results.

## Claims attacked and corrected

|Claim|Verdict|Correction|
|---|---|---|
|Private read-only maps are stable private copies|**Fails**|A borrowed byte changed after an external write. Private mapping is not a frozen snapshot.|
|Bounds checks and shared flock prevent truncate faults|**Fails**|The independent borrowed-view probe exited 135. Advisory locking cannot exclude nonparticipants.|
|A can reuse the existing fault handler as a recovery window|**Fails**|`Fault.jet:73–80,120–145` installs one-shot fatal handling, uses a process-wide reporting latch and terminates. Recovery requires a different vetted branch before those effects.|
|A cannot crash or expose invalid memory|**Fails as an established claim**|No implementation or recovery proof exists. Converted this assurance into explicit blocking criteria.|
|Copying reads are snapshots|**Holds only within stated limits**|Returned bytes are owned, but may mix concurrent updates. Failed reads discard output; failed writes may have changed a prefix.|
|Atomics handle arbitrary foreign changes|**Fails**|Added alignment, eight-byte containment, compatible participants, memory-order, overflow and foreign-interference rules. Atomicity is not multiword consistency or persistence.|
|CAS directly supplies correct lock-free rings|**Fails**|Added ABA, generation/reclamation and initialization requirements. Raw mapped words cannot become Jet object pointers.|
|Each atomic access costs one instruction plus a call|**Fails**|Removed the claim. Costs include validation, lifetime pinning, recovery bookkeeping, scheduling constraints and ordering. Some targets require retry sequences.|
|`msync` makes the new count durably committed|**Holds only within stated limits**|`MS_SYNC` waits for filesystem writeback. It does not provide a transaction, concurrent-writer snapshot, directory persistence or power-failure atomicity.|
|The existing counter costs exactly four syscalls|**Fails**|Runtime paths also open, seek and close descriptors. No throughput comparison was measured.|
|`mprotect` only matters for writable file maps, and Jet has none|**Fails**|It changes local page permissions, including execute permissions. `HostSpawn.jet:493–495` already uses it for a guard page. Scope its census disposition separately.|
|`MappedFile.advise` covers `posix_madvise`|**Fails**|The executable probe reports no such method. Removed invented coverage.|
|A private-copy web fallback preserves shared-map meaning|**Fails**|It loses shared visibility and ordering. Removed the fallback and added an I9 gate.|
|Safe mutable lists are suitable for C or D|**Fails as specified**|C now requires checked access; D requires an unsafe raw capability without escaping safe aliases.|
|Existing truncate-SIGBUS needs no independent action|**Fails**|Added a separate-card proposal regardless of the ballot result. SIGBUS alone does not prove memory corruption; borrowed-view safety and controlled I/O-failure behavior still require resolution.|

Checked implementation references include `CompilerMapped.jet:75–139,182–200`, `Fault.jet:73–80,120–145`, and `Core/Files/Files.jet:906–924`. No ratified decision was cited to justify closing broader census scope.

## Fair peer comparison

- **Rust memmap2:** reference-producing file-backed constructors are unsafe, including private-copy constructors. `MmapRaw` exposes pointers without references, a strong precedent for D. Sources: [MmapOptions](https://docs.rs/memmap2/latest/memmap2/struct.MmapOptions.html), [MmapRaw](https://docs.rs/memmap2/latest/memmap2/struct.MmapRaw.html).
- **Go:** `syscall.Mmap` returns bytes; mmap-go adds mapping choices and lifecycle methods. Saying truncation always kills Go was unfair: `SetPanicOnFault` permits goroutine-local panic behavior. Sources: [syscall](https://raw.githubusercontent.com/golang/go/master/src/syscall/syscall_linux.go), [mmap-go](https://raw.githubusercontent.com/edsrzf/mmap-go/master/mmap.go), [SetPanicOnFault](https://raw.githubusercontent.com/golang/go/master/src/runtime/debug/garbage.go).
- **Java:** `MappedByteBuffer` supplies direct mapped access, not compulsory copies. Its API promises an unspecified exception for inaccessible regions, possibly later. Linux-x86 HotSpot explicitly handles mapped-access SIGBUS; its unsafe-access error path constructs `InternalError`. That exact type is not the portable API promise. Sources: [API](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/nio/MappedByteBuffer.html), [signal handling](https://raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/os_cpu/linux_x86/os_linux_x86.cpp), [error construction](https://raw.githubusercontent.com/openjdk/jdk/master/src/hotspot/share/runtime/handshake.cpp).
- **.NET:** `MemoryMappedFile` offers accessors, streams, shared views and private copies. Unix flushing uses `MS_SYNC | MS_INVALIDATE`. Bounds checks and safe handles alone do not establish external-truncation recovery. Sources: [API](https://learn.microsoft.com/en-us/dotnet/api/system.io.memorymappedfiles.memorymappedfile), [Unix implementation](https://raw.githubusercontent.com/dotnet/runtime/main/src/libraries/System.IO.MemoryMappedFiles/src/System/IO/MemoryMappedFiles/MemoryMappedView.Unix.cs).
- **LMDB:** zero-copy reads, snapshot transactions, serialized writers and crash recovery are stronger guarantees than map leaves alone. Its default read-only mapping protects against stray writes; `MDB_WRITEMAP` trades that protection for write performance. Source: [LMDB contract](https://raw.githubusercontent.com/LMDB/lmdb/mdb.master/libraries/liblmdb/lmdb.h).
- **SQLite:** optional mmap reads avoid copies; modifications use copied pages and `xWrite`. SQLite explicitly documents mapped-I/O crash hazards. Its transaction protocol must not be attributed to mapping alone. Source: [SQLite mmap](https://www.sqlite.org/mmap.html).

## Gap list and proposed disposition

1. **G1, separate card:** safe read-only maps expose externally mutable borrowed bytes and fault after truncation. Resolve the I1 borrowing contract and controlled diagnostic/recovery behavior, whichever option wins.
2. **G2, blocking ballot/build line:** prove handler re-entry, simultaneous threads, carrier migration, nesting, cleanup, repeated faults and close/address-reuse races. Match faulting instructions and operation identity, not addresses alone.
3. **G3, blocking ballot/build line:** prove shared atomic layout, alignment, ordering, foreign interference, overflow, CAS and ABA limits. Measure real access and contention costs.
4. **G4, blocking ballot/build line:** specify partial writes, flush failures, concurrent writers and metadata durability. Compare equal durability workloads.
5. **G5, blocking ballot line:** provide one execution-tier meaning or obtain an explicit owner-ratified exception. No private-copy web substitute.
6. **G6, census correction:** keep `posix_madvise` unresolved; decide `msync` and broader `mprotect` scope separately.
7. **G7, blocking build line:** cover initialization, resize, active writers, inode aliases, replacement/unlink, authority and resource lifetime.
8. **G8, alternative contracts:** prevent C/D from exporting unsafe backing as safe lists. E needs backing discovery/transfer and equivalent host guarantees; shrink seals do not provide ordinary-file durability or eliminate every fault.

## Posting restriction

Do not post the ballot as ready, claim A is safe or faster, or mark the broader census rows complete. `draft:false` satisfies the validator's posting-payload format; it is not substantive approval.

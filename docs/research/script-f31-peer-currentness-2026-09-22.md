# SCRIPT-F31: peer currentness and evidence gaps

Checked 2026-09-22 against first-party release records and documentation. This
note adds a dated current observation to the historical 2026-09-11 capture. It
does not edit or relabel that capture.

## Scope and method

The bounded scope is the source-currentness work named by
[SCRIPT-F31](mine-for-jet-2026-09-12.md#finding-script-f31): Lua archive and
compatibility, .NET release/API identity, and selected Asio/Tokio defaults and
deltas. Sources below are official project or maintainer sources. No peer was
installed, built, loaded, or run. No timing or Jet-parity claim follows from
this source review.

## Findings

| Area | Historical identity retained from the capture | Current source evidence | Limit that remains |
|---|---|---|---|
| Lua | Lua 5.4.8; lua-cjson 2.1.0; rolling LuaJIT 2.1 | Lua's official archive lists the 5.4.8 tarball and SHA-256, the later 5.4.9 tarball, and Lua 5.5.1. The version page says 5.4.9 is the final 5.4 release and 5.5.1 is current. The CJSON maintainer still lists stable 2.1.0; upstream's development manual names Lua 5.1, 5.2, 5.3, or LuaJIT. LuaJIT's 2.1 docs state Lua 5.1 API/ABI compatibility with selected later extensions; its release policy is rolling, with no release tarballs. | The 5.4.8 tarball's presence is verified, but its internal member list was not inspected: the retained source inventory does not identify which member names were missing. CJSON compatibility with Lua 5.4.9 or 5.5.1 remains unestablished; the manuals do not provide a runtime result. LuaJIT has no fixed current commit in the captured evidence. |
| .NET | .NET 9.0.3 and SDK 9.0.201; API pages used a `net-9.0` view | Microsoft's 9.0.3 release note maps that runtime to SDK 9.0.201. The current .NET release table lists .NET 9.0.20; its September 2026 release note identifies SDK 9.0.121. Versioned `net-9.0` Learn pages cover `ProcessStartInfo.ArgumentList`, `JsonSerializer`, and `NativeLibrary`. | The API views identify a .NET major/minor target, not a 9.0.3 patch-specific contract. No .NET executable, project, process launch, JSON operation, or native library load was run. |
| Asio | Boost 1.88.0 / Asio 1.34.2 reference capture | The Asio site lists standalone Asio 1.38.2 as the stable release; Boost lists 1.92.0 and its Asio documentation identifies Asio 1.38.2. The versioned configuration page records defaults including scheduler concurrency hint `0`, scheduler locking `true`, resolver threads `0` (at most one resolver thread is created on first asynchronous resolve), and timer heap reserve `0`. The 1.38.2 `steady_timer` uses a steady clock; waits on an expired timer complete immediately, and changing an active expiry cancels pending waits. Boost 1.90 changed the default candidate executor for `associated_executor` from `system_executor` to `inline_executor`; current Boost docs show `inline_executor`. Boost 1.92 adds `scheduler.assume_continuation`, true for hint `1` and false otherwise. | These are selected documentation facts, not a complete 1.34.2-to-1.38.2 API diff or a runtime/race test. Backend-specific defaults apply only under the documented platform conditions. |
| Tokio | Tokio 1.44.2 versioned documentation capture | The release list identifies Tokio 1.53.1 as current on 2026-09-22. Its versioned `#[tokio::main]` docs say the default flavor is multi-threaded, default worker count is the system CPU count, and the builder expansion calls `enable_all()`. The current-thread flavor is single-threaded. The 1.53.1 release note lists its stable signal/MSRV fix and unstable timer-cancellation fix. | The exact 1.44.2 `#[tokio::main]` page was not retrievable in this review, so no claim is made that its defaults match 1.53.1. The selected current defaults are documented; the complete 1.44.2-to-1.53.1 API change history was not enumerated. No Tokio program was built or run. |

## Unavailable and unmeasured evidence

- Lua's official listing establishes that the 5.4.8 source archive exists. It
  does not establish that an unspecified requested member is present. The
  original captured member-level inventory was not available in the retained
  report, and the archive was not downloaded or unpacked.
- The CJSON sources do not establish compatibility with Lua 5.4.9 or 5.5.1.
  Treat both as unknown, not as compatible or incompatible.
- LuaJIT's official status and download pages describe a rolling branch and
  no release tarballs. An exact current source commit is therefore not supplied
  by the retained capture.
- Microsoft Learn pages are versioned by framework target, not by every .NET
  patch. The exact 9.0.3 API behavior is unavailable from those page identities.
- Current Asio docs and release notes resolve selected defaults and the named
  executor change. They do not replace a full historical-to-current API diff
  or execution evidence.
- Current Tokio docs resolve the selected 1.53.1 runtime defaults. The
  historical 1.44.2 page could not be fetched here; the old-to-current default
  delta remains unknown.
- No peer executable, library compatibility, output, error behavior, or
  performance was tested. Historical receipts remain historical.

## Primary sources

- Lua [official archive](https://www.lua.org/ftp/), [version history](https://www.lua.org/versions.html), and [5.5.1 download page](https://www.lua.org/download.html).
- Lua CJSON [maintainer page](https://kyne.au/~mark/software/lua-cjson.php), [2.1.0 manual](https://github.com/mpx/lua-cjson/blob/2.1.0/manual.txt), and [current development manual](https://github.com/mpx/lua-cjson/blob/master/manual.adoc).
- LuaJIT [extensions](https://luajit.org/extensions.html), [status/release policy](https://luajit.org/status.html), and [download policy](https://luajit.org/download.html).
- .NET [9.0.3 release note](https://github.com/dotnet/core/blob/main/release-notes/9.0/9.0.3/9.0.3.md), [current releases](https://github.com/dotnet/core/releases), [September 2026 9.0.20 / SDK 9.0.121 note](https://github.com/dotnet/core/blob/main/release-notes/9.0/9.0.20/9.0.121.md), and versioned [`ArgumentList`](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.processstartinfo.argumentlist?view=net-9.0), [`JsonSerializer`](https://learn.microsoft.com/en-us/dotnet/api/system.text.json.jsonserializer?view=net-9.0), and [`NativeLibrary`](https://learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.nativelibrary?view=net-9.0) pages.
- Asio [release/download page](https://think-async.com/Asio/Download), [1.38.2 docs](https://think-async.com/Asio/asio-1.38.2/doc/), versioned [1.38.2 runtime configuration](https://think-async.com/Asio/boost_asio_1_38_2/doc/html/boost_asio/overview/core/configuration.html) and [`steady_timer`](https://think-async.com/Asio/asio-1.38.2/doc/asio/reference/steady_timer.html), [Boost 1.90 release notes](https://www.boost.org/releases/1.90.0/), [Boost 1.92 release notes](https://www.boost.org/releases/latest/), and current [`associated_executor`](https://www.boost.org/latest/doc/html/boost_asio/reference/associated_executor.html) docs.
- Tokio [release list](https://github.com/tokio-rs/tokio/releases), [1.53.1 release note](https://github.com/tokio-rs/tokio/releases/tag/tokio-1.53.1), and versioned [`#[tokio::main]` docs](https://docs.rs/tokio/1.53.1/tokio/attr.main.html).

## Card criterion mapping

1. The archive listing, current releases, API versions, and selected defaults
   are verified above. Every remaining scoped source gap and unmeasured claim
   is named with its exact limit.
2. Historical capture identities remain separate from observations checked on
   2026-09-22. No old executable or frozen receipt was relabeled current.
3. Work stayed within SCRIPT-F31's Lua, .NET, Asio, and Tokio gaps. It adds no
   language/API proposal or broader peer campaign.

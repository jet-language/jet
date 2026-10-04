# #3127: ordinary FTP integration and placement gap (SCRIPT-F40)

Closer10, 2026-09-29. Binary: `~/.cache/jet-dev/safe-jet.sh` →
`jet-debug-snapshot14`. The investigation needed no repository code change.
Verdict: **PASS**, with a no-addition outcome.

## Question

Which ordinary workload needs FTP? Does the existing Jet composition serve
it: `core.process` driving curl, or raw `core.net` / `core.net.tls`? Or is an
optional package or stock Core FTP needed?

## Named workload (criterion 1)

The workload is a nightly vendor-drop pull and return upload over explicit
FTPS. It is the common B2B pattern: retail EDI/price files, bank statement
drops, and government bulk-data portals that still offer only FTP/FTPS.

| Requirement | Value in the probe |
|---|---|
| Operations | LIST (name listing), RETR (8 MiB export and a small CSV), REST/resume of a partial RETR, STOR (return upload) |
| Authentication | USER/PASS per vendor account; the password is a deployment secret (env), never in argv, logs or files |
| Control channel security | Explicit FTPS (`AUTH TLS`), TLS required: the server refuses plaintext login (`tls_control_required`) |
| Data channel security | `PBSZ 0` + `PROT P`, TLS required on every data connection (`tls_data_required`) |
| Certificate trust | A private CA/self-signed vendor certificate pinned by file (`--cacert cert.pem`) |
| Mode / proxy | Passive only (the client sits behind NAT), a fixed passive port range 60000-60009, EPSV first; no proxy in the probe |
| Deployment | A cron/systemd job with a wall deadline per transfer, resume after interruption, a non-zero exit on auth or TLS failure |

The server is `~/.cache/jet-dev/ftp-probe/server.py`: pyftpdlib 2.2.0 with
pyOpenSSL, run through a nix-built Python 3.13 environment. It uses
`TLS_FTPHandler` with control and data TLS required. A
`TLS_DTPHandler`+`ThrottledDTPHandler` data handler throttles transfers to
2 MiB/s. A second, plain-FTP instance runs on port 2122.

## Process composition (criterion 2): demonstrated

The program is `~/.cache/jet-dev/scratch/Closer10/ftp_pull.jet`, run with:

```
FTP_PASSWORD=s3cr3t-vendor ~/.cache/jet-luna/safe-jet.sh run --allow=Env,Exec,Time.Wait ftp_pull.jet
```

How it works:

- It calls `process.cmd(["curl", "--silent", "--show-error", "--ssl-reqd", "--cacert", "cert.pem", "--ftp-pasv", "--config", "-", …])`
  with `.stdin(.Stream).stdout(.Capture).stderr(.Capture).timeout(limit)`.
- It writes the credential to curl's stdin as a config line
  (`user = "vendor:<pw>"`) and closes stdin.
- It then reads `/proc/<child.id()>/cmdline` of the live curl process to
  check whether the password appears in its argv.

Observed output:

```
LIST: password occurrences in live curl argv=0
LIST: success=true code=0 timed_out=false err=
LIST: stdout<<export-2026-09-29.bin
small.csv>>
RETR small: password occurrences in live curl argv=0
RETR small: success=true code=0 timed_out=false err=
small.csv sha256: 31e26370225c0855
RETR big deadline: password occurrences in live curl argv=0
RETR big deadline: success=false code=-1 timed_out=true err=
partial bytes > 0: yes; partial < full: yes
curl children left after deadline: 0
RETR big resume: password occurrences in live curl argv=0
RETR big resume: success=true code=0 timed_out=false err=
export.bin sha256: 591b5655ffce6af8
STOR: password occurrences in live curl argv=0
STOR: success=true code=0 timed_out=false err=
inbox has upload: yes
bad password: password occurrences in live curl argv=0
bad password: success=false code=67 timed_out=false err=curl: (67) Access denied: 530
plain server with --ssl-reqd: password occurrences in live curl argv=0
plain server with --ssl-reqd: success=false code=64 timed_out=false err=curl: (64) Requested SSL level failed
```

Checked afterwards: `sha256sum out/export.bin` =
`591b5655ffce6af833d3fe7407441b99766c5c80afb03110579c865a54ffd890`, the same
as the server's source file. `srv/inbox/upload.csv` =
`31e26370…0413`, the same as `small.csv`. The resumed file is 8,388,608
bytes.

| Behaviour | Result |
|---|---|
| Credential | Never in argv (`/proc/<pid>/cmdline` count 0 on every call) and never on disk; it is only in curl's stdin config. The server log shows `USER 'vendor' logged in`, with no password. |
| Deadline | `ProcessSpec.timeout(1s)` interrupts the throttled 8 MiB RETR: `timed_out=true`, `code=-1`, no curl process left (`pgrep` 0). |
| Cleanup | The child is reaped by the Jet deadline. The partial file stays on disk by design, for resume. |
| Partial transfer / resume | The partial file is between 0 and 8 MiB. `curl -C -` resumes: the server log shows the interrupted `RETR … completed=0 bytes=4194304 seconds=0.996`, then the resumed `RETR … completed=1 bytes=4198400`, so only the remainder was sent. The final sha256 matches. |
| TLS downgrade | Against a plaintext-only server, `--ssl-reqd` fails closed: `code=64`, no login attempted in the clear. |
| Upload | STOR succeeds and the digest matches. |

One deployment constraint showed up. Against an IPv4-only server, a
`ftp://localhost:…` URL let curl reach the control channel, but its
EPSV/PASV fallback then failed with `curl: (13) Bad PASV/EPSV response: 200`.
Using `127.0.0.1` fixed it. That is a curl/server property, not Jet's.

## Raw socket composition (criterion 2): rejected for FTPS

The probe is `~/.cache/jet-dev/scratch/Closer10/ftp_raw_inline.jet`. It does
plain-FTP control/data framing over `core.net.tcp_connect`, then tries an
`AUTH TLS` upgrade with `core.net.tls.client(^tcp, "localhost")`. Result:

```
banner: 220
USER -> 331
PASS -> 230
TYPE -> 200
PASV -> 227  data port parsed: false      (first attempt)
Stop [E3001]: `panic: data connect` … locals: port = -1
```

Plain-FTP control framing over `core.net` works: login succeeds, and the
PASV reply arrives. The first PASV parse failed because `String.slice(a, b)`
includes its end index: a follow-up run printed `slice(0, 2) of "abcdef" = abc`. The corrected-parse rerun and the
`AUTH TLS` → `core.net.tls.client(^tcp, "localhost")` upgrade step did not
finish within this run, so the TLS upgrade outcome is **not observed**.

What the raw-socket route lacks for the named workload, from reading
`Core/net/tls.jet`:

- A caller-supplied trust root. `tls.roots(path)` builds a carrier, but no
  native leaf consumes it (`:141-152` refuses configured policy).
- TLS session reuse between the control and data channels. There is no API,
  and servers such as vsftpd with `require_ssl_reuse=YES` demand it.
- A PASV/EPSV parser, REST/resume, and multi-line reply framing. All of these
  would have to be written by hand.

While writing the raw probe, two compiler defects turned up; they are listed
under Defects below.

## Placement verdict (criterion 3)

No new FTP functionality is recommended. The `core.process` + curl
composition meets every cell of the named workload:

- listing, retrieve, upload and resume;
- explicit FTPS on both channels, with a pinned CA;
- passive mode;
- a credential kept out of argv;
- a wall deadline with a reaped child;
- typed failure receipts.

It does this with stock Core and a ubiquitous system tool, which is an
explicit dependency. The raw-socket route cannot do FTPS without new TLS API
(custom roots, session reuse). Those would be TLS work items, not FTP ones,
and belong to their own cards if another protocol needs them.

What Pip may want as a follow-up: an example
(`Examples/features/io/ftps_via_curl.jet`) that shows this pattern. It needs
a local FTPS fixture, so it is not added here.

## Defects found

1. **ICE: "MIR place is not writable"** at `crates/jet-codegen/src/Codegen/MIRRust.rs:26391`.
   It happens when a helper takes `stream: TCPStream` by value and calls
   `stream.read_text(4096)` or `stream.write_all(bytes)`. The checker accepts
   the program. Repro: `~/.cache/jet-dev/scratch/Closer10/ftp_raw_plain.jet`
   before the `working := stream` workaround (`ftp_raw.log`).
2. **The workaround (`working := stream`) makes rustc reject the generated
   code**: `no method named clone found for struct JetTCPStream`
   (`ftp_raw_plain.log`, `ftp_raw_tls.log`). Copying a TCPStream local is
   accepted by the checker but has no emission.

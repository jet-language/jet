# Security deep scan: Tower control plane

This file is the detailed source authority for the 11 candidates owned by Tower
card #1377. It uses the current checkout. The canceled discovery scan did not
run centralized validation, so each disposition below is a current-tree source
disposition with hostile regression tests in the checkout. The targeted Tower
commands passed: `node --test plugins/tower/test/server.test.mjs`,
`node --test plugins/tower/test/wave.test.mjs`,
`node --test plugins/tower/test/docs.test.mjs`,
`node --test plugins/tower/test/store.test.mjs`,
`node --test plugins/tower/test/acceptance-queue.test.mjs`,
`node --test plugins/tower/test/repair.test.mjs`, and
`node --test plugins/tower/test/security-hardening.test.mjs`.

The summary report links here for #1377. The similarly named
`security-deep-scan-2026-08-03-full.md` is the separate #1378 memory/ABI
artifact and is not authoritative for this Tower campaign.

The boundary labels are:

- source: the input or control flow that starts the path;
- control: the check that limits the path;
- sink: the operation that could cross the trust boundary;
- impact: the failure if the control is missing;
- precondition: the access needed to reach the boundary;
- validation: the source or test proof in this checkout.

## `tower-default-network-auth-bypass`

Disposition: `already-fixed`.

Source: Tower requests enter `authed` and `csrfAllowed` before route dispatch in
`plugins/tower/app/server.mjs:310-316`.

Control: `requestTrusted` requires the configured token whenever token mode is
enabled. Without a token, `isLocal` checks the socket address, parsed `Host`, and
`X-Forwarded-For` at `plugins/tower/app/server.mjs:166-206`. A no-token server
binds to `127.0.0.1` at `plugins/tower/app/server.mjs:559-560`.

Host does not authenticate a request: the no-token server trusts only a loopback socket whose Host is literal loopback and whose X-Forwarded-For is empty or literal loopback; when a token is configured, remote access is allowed only after the configured token matches.

Sink: Read and mutation routes run only after authentication and CSRF checks at
`plugins/tower/app/server.mjs:247-262,310-316`.

Impact: A remote client cannot use the default no-token server as an API.

Precondition: The client must reach the HTTP listener with a non-loopback
socket, a rebound host, or a forged forwarded address.

Validation: `plugins/tower/test/server.test.mjs:78-86` checks rebound and
forwarded requests. The same test checks headerless and cross-site mutations at
`plugins/tower/test/server.test.mjs:88-125`.

## `tower-docs-symlink-read`

Disposition: `already-fixed`.

Source: The docs API accepts a caller path at
`plugins/tower/app/server.mjs:386-393`, then calls `showDoc` or `listDocs`.

Control: `plugins/tower/app/docs.mjs:74-151` keeps root and descendant
directories open, rejects moved descriptors, and checks physical containment.
`plugins/tower/app/docs.mjs:287-350` uses `lstat`, `O_NOFOLLOW`,
`O_NONBLOCK`, regular-file checks, and post-operation directory guards.

Sink: Reads use `readOpenedFile` and `showDoc` at
`plugins/tower/app/docs.mjs:345-350,622-641`.

Impact: A symlink, hardlink, special file, or moved held directory cannot make
the docs reader read an unintended file or block on a device.

Precondition: The caller must request a docs path or list the docs tree while
an attacker changes the filesystem.

Validation: Existing symlink and swap coverage is at
`plugins/tower/test/docs.test.mjs:80-152`. Hardlink, special-file, and held-
directory coverage is at
`plugins/tower/test/security-hardening.test.mjs:33-123`.

## `tower-owner-authorization-bypass`

Disposition: `already-fixed`.

Source: Generic mutation routes pass caller fields through the route table at
`plugins/tower/app/server.mjs:115-156,521-533`.

Control: Owner claims require `ownerSessionTrusted` at
`plugins/tower/app/server.mjs:202-209,529-530`. Store-level attribution and
ratified-decision gates remain at
`plugins/tower/app/store.mjs:1743-1747,2006-2011,2061-2143`. The acceptance
path uses a session-bound, one-use challenge at
`plugins/tower/app/server.mjs:454-501`.

Sink: `store.mutate` applies the selected mutation only after the route guard.

Impact: A caller cannot turn a generic agent mutation into owner acceptance.

Precondition: The caller must send a privileged `by` or owner-verification
payload to a mutation route.

Validation: `plugins/tower/test/server.test.mjs:127-137` checks forged owner
fields. Dedicated acceptance checks are at
`plugins/tower/test/acceptance-queue.test.mjs:285-374`.

## `tower-loopback-csrf`

Disposition: `already-fixed`.

Source: Browser requests reach `csrfAllowed` before any route body is read at
`plugins/tower/app/server.mjs:226-233,310-316`.

Control: Cross-site mutation requests need same-origin metadata or the explicit
authenticated CLI channel. The docs GET route and claimable brief GET route are
treated as mutations at `plugins/tower/app/server.mjs:226-230`.

Sink: Mutation route handlers are entered only after the CSRF check.

Impact: A hostile web page cannot cause a browser to change the Tower board.

Precondition: The attacker must make a browser send a request with missing or
cross-site origin metadata.

Validation: `plugins/tower/test/server.test.mjs:88-125` checks headerless,
cross-site, docs, and claimable-brief requests.

## `tower-owner-payload-forgery`

Disposition: `already-fixed`.

Source: The generic server route derives the owner claim from `by`, `quote`, and
batch quotes at `plugins/tower/app/server.mjs:529-530`. Its rejection-audit
side path also receives caller `by` values at
`plugins/tower/app/server.mjs:234-239,514-527`.

Control: `ownerSessionTrusted` gates those claims. The rejection-audit helper
maps an unverified `by: "owner"` to the neutral default and only preserves it
when the owner session is trusted. Store-level owner-or-quote validation
remains at `plugins/tower/app/store.mjs:1743-1747,2006-2011`.

Sink: Normal state mutations run through `store.mutate` only after the claim
check; rejected acceptance attempts append an audit event with the sanitized
actor.

Impact: Caller text cannot forge owner attribution without an authenticated
owner session or an explicit owner quote.

Precondition: The caller must submit a privileged attribution field.

Validation: `plugins/tower/test/acceptance-queue.test.mjs:244-260` checks that a
forged owner payload is rejected and cannot write an owner-attributed audit
event. `plugins/tower/test/server.test.mjs:127-137` and
`plugins/tower/test/store.test.mjs:87-95` cover the remaining forged and
missing-attribution paths.

## `tower-docs-symlink-write`

Disposition: `already-fixed`.

Source: Docs add and update routes call the descriptor-relative writers at
`plugins/tower/app/server.mjs:394-402`.

Control: `plugins/tower/app/docs.mjs:155-220` opens every directory without
following links. `plugins/tower/app/docs.mjs:413-471` creates files with
exclusive, no-follow, nonblocking flags and random atomic temporary names. Its
post-rename identity check removes an unexpected destination entry.
Destination checks reject symlinks, special files, and multiply-linked files.

Sink: `createDoc` and `updateDoc` write at
`plugins/tower/app/docs.mjs:644-723`.

Impact: Docs writes cannot follow an attacker-controlled directory or file.

Precondition: The caller must request an add or update while a path component or
destination changes.

Validation: Existing symlink swap coverage is at
`plugins/tower/test/docs.test.mjs:80-152`. Hardlink, special-file, destination,
backup, and temp coverage is at
`plugins/tower/test/security-hardening.test.mjs:33-195,224-328`.

## `tower-docs-symlink-delete`

Disposition: `already-fixed`.

Source: Delete and archive routes call `deleteDoc` and `archiveDoc` at
`plugins/tower/app/server.mjs:403-410`.

Control: `unlinkAt` and `renameAt` guard both held parent directories before and
after each descriptor-relative operation at
`plugins/tower/app/docs.mjs:354-390`. Source and destination identities are
checked at `plugins/tower/app/docs.mjs:726-794`.

Sink: The only destructive operations are descriptor-relative unlink and rename.

Impact: A symlink, hardlink, special file, or moved directory cannot redirect a
delete or archive operation outside the docs root.

Precondition: The caller must request delete or archive during a filesystem
swap.

Validation: Existing symlink and swap coverage is at
`plugins/tower/test/docs.test.mjs:80-152`. Hardlink and special-file coverage
is at `plugins/tower/test/security-hardening.test.mjs:33-69`.

## `tower-docs-symlink-walk`

Disposition: `already-fixed`.

Source: `listDocs` calls `walkMd` at
`plugins/tower/app/docs.mjs:547-571,592-618`.

Control: `walkMd` uses `readDirectoryAt`, skips symlinks, opens directories
without following links, and reads only safe regular files at
`plugins/tower/app/docs.mjs:547-571`.

Sink: Recursive directory reads and markdown reads use held descriptors.

Impact: Docs inventory cannot traverse a symlinked or moved directory, block on
a special file, or include a hardlinked document.

Precondition: The caller must request docs inventory while the tree is hostile.

Validation: Existing walk coverage is at `plugins/tower/test/docs.test.mjs:94-152`.
Special-file and held-directory coverage is at
`plugins/tower/test/security-hardening.test.mjs:33-123`.

## `cd005-tower-token-dns-rebind`

Disposition: `already-fixed`.

Source: No-token requests use local trust; token-mode requests must present the
configured token. The paths meet in `requestTrusted` at
`plugins/tower/app/server.mjs:166-206,247-262`.

Control: `loopbackHost` accepts only literal loopback names and addresses.
`loopbackForwarded` rejects a non-loopback forwarded address. In token mode,
`requestTrusted` bypasses local trust and requires `tokenMatches`.

Sink: No-token local trust reaches route dispatch only when socket, Host, and
forwarded address are loopback. Token mode reaches dispatch only after the
configured token matches.

Impact: A DNS-rebound or forwarded remote request without the configured token
cannot gain default local access. This finding does not claim that a valid
remote token is local-only; token possession is the explicit remote-access
credential.

Precondition: The attacker must control the request host or forwarded header.

Validation: `plugins/tower/test/server.test.mjs:78-86` checks both hostile forms
against the no-token local-trust path.

## `tower-tracked-state-priority-xss`

Disposition: `already-fixed`.

Source: Tracked priority reaches the projected card and UI card/detail templates
at `plugins/tower/app/store.mjs:844-860` and
`plugins/tower/app/ui/tower.js:568-570,1029-1030`.

Control: The UI escapes priority before using it in class and text contexts.

Sink: `esc(c.priority)` is used at both priority HTML sinks.

Impact: A tracked priority value cannot inject HTML or script into the Tower UI.

Precondition: An attacker must first place a hostile value in tracked board
state.

Validation: `plugins/tower/test/store.test.mjs:97-102` checks both escaped
priority sinks and rejects the old unescaped form.

## `tower-ratified-decision-integrity-bypass`

Disposition: `already-fixed`.

Source: Generic decision routes dispatch to store mutations at
`plugins/tower/app/server.mjs:125-130,521-533`.

Control: `reopenDecision`, `updateDecision`, and `deleteDecision` reject agent
changes to ratified decisions at `plugins/tower/app/store.mjs:2061-2143`.

Sink: Decision state, provenance, outcomes, and audit events change only after
the owner gate.

Impact: A generic agent caller cannot reopen, edit, or delete a ratified owner
decision.

Precondition: The caller must target an existing ratified decision.

Validation: `plugins/tower/test/store.test.mjs:69-85` checks the generic
mutation paths and preserves the ratified record after each hostile attempt.

## Fresh-review blocker closeout for #1377

The fresh-review blocker fixes are present in the current source as follows;
the targeted hostile regressions passed in
`plugins/tower/test/security-hardening.test.mjs`.

- B1: `plugins/tower/app/paths.mjs:241-303` reads a single-link regular source,
  validates the real backup directory, and creates each backup with a random
  exclusive no-follow destination. Post-write identity validation removes an
  unexpected destination.
- B2: `plugins/tower/app/repair-journal.mjs:19-105` validates the backup
  directory, journal, and named backup sources with `lstat` and single-link
  regular-file checks. Recovery reads pinned descriptors and delegates restore
  to `paths.mjs:188-235`, whose temporary names are random, exclusive, and
  no-follow. The regressions are in
  `plugins/tower/test/security-hardening.test.mjs:224-286`.
- B3: `plugins/tower/app/docs.mjs:359-471` compares the closed temporary entry
  with the post-rename destination, while `plugins/tower/app/repair.mjs:179-301`
  pins each stage identity before close and verifies the destination identity
  after rename. Both remove an unexpected symlink, hardlink, special file, or
  empty directory. `plugins/tower/test/security-hardening.test.mjs:288-328`
  exercises a closed-stage swap and verifies that no attacker entry remains at
  the live destination.
- B4: `plugins/tower/app/store.mjs:327-336,356-424` strictly validates card
  `num`/`workOrder`, criterion `n`/`status`, epoch and milestone shapes, and
  stored phase values. `plugins/tower/app/ui/tower.js:23-33,230-234,565-575,
  791-799,865-868,974-976,1028-1045,1080-1087,1189-1193` applies context-correct
  escaping or allowlists at the corresponding HTML sinks. Injection regressions
  are at `plugins/tower/test/security-hardening.test.mjs:330-406`.
- B5: this file is the detailed #1377 authority; the summary report points to
  it, while the similarly named #1378 artifact is explicitly out of scope.

## Pair persistence correction for #1377

`tower.json` and `history.json` now share the existing repair journal for normal
store mutations, undo, and archive restore. `store.mjs:69-105` holds one
descriptor-pinned data directory for the pre-write backups, journal, both
atomic writes, sync, and journal removal. It recovers both backups on an error.
Pair readers take bounded no-follow snapshots through one held descriptor and
retry around a pending journal or changed pair. The call sites are
`store.mjs:136-219,228-260`.

`repair-journal.mjs:31-124` remains the recovery mechanism. A pending journal
restores both stores before the next read. `paths.mjs:131-235` gives each JSON
write an unpredictable exclusive temporary file, rejects a symlink or unsafe
destination, removes an attacker entry after a close-to-rename swap, syncs the
file, renames it, and syncs the containing directory. `paths.mjs:241-330`
applies the same source and destination checks to backups. Repair source reads,
backup creation, staging, renames, sync, and journal removal share one held
data-directory descriptor in `repair.mjs:227-371`.

The hostile crash states are covered by
`plugins/tower/test/security-hardening.test.mjs:197-222`. It leaves the journal
after a prepared state, after a history-only write, and after both writes, then
checks that the next store read restores the exact pre-state pair. Hostile
recovery source and temporary-entry cases are at
`plugins/tower/test/security-hardening.test.mjs:224-328`; bounded live/history/
undo reads and stored UI-field hostile values are covered by
`security-hardening.test.mjs:197-247,367-422`.

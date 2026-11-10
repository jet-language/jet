# Core TLS and crypto security re-review (Tower #6694 c4)

- **Date:** 2026-11-10
- **Source:** dev `76955986d0` (train t6c: TlsLead-41b, 42, 45, 46, 50). TlsLead-41 landed earlier, in `ecf04d985a`.
- **Prior review:** `Docs/audits/2026-11-09-core-tls-crypto-security-review.md`

## Scope and method

- **Read-only review.** I did not use bash or git, sent nothing over the network, ran no payloads, and **ran no tests**.
- **Where the source came from.** I read it from the Lander's worktree `.agent-worktrees/Main-dev-collectall/`. That worktree's HEAD is `refs/heads/dev`, which points to `76955986d0067a6d2c42d098f5cc2ff4c7878671`. I could not rule out uncommitted edits in it. I also read the integration inputs: `integ/t6c/` `REPORT.md`, `order`, `tests/*.out` and the five `.rebased` TlsLead patches.
- **How test results are reported.** Each named test is marked **PASS per integ REPORT**. That means the integration run recorded a pass (`integ/t6c` for the t6c SUBs, `integ/t5a`/`t5b` for TlsLead-41). I did not run any of them.
  - The last TLS patch in the t6c order is TlsLead-50. Its full `jet test Core/Net/SSL/TLS.jet` run (`tests/TlsLead-50.2.out`) shows "45 passed, 0 failed, 0 skipped", with all the t6c tests in place.
  - `TlsLead-45.1.out` shows the four new tests in `Core/Net/Tls.jet` passing.
  - `TlsLead-46.1.out` shows the emission check passing.
  - `TlsLead-42.2.out` shows the soak tier passing under `JET_DUDECT_SOAK=1`.
- **Implementer notes were not trusted.** I ignored the msg and commitmsg files and checked each claim against the code.
- **Notation.** `[INFERENCE]` marks a claim I did not observe directly.

## 1. Prior findings

| ID | Prior sev. | Status | Fix location (dev 76955986d0) | Proving test (PASS per integ REPORT) |
|---|---|---|---|---|
| F-ZERO-1 | Medium | **Fixed** | See below | Tls.jet:1369, Tls.jet:1421 |
| F-CT-1 | Low | **Fixed** | ECDSA.jet:84-141; Bignum.jet:382-397, 540-557, 630-681, 1351-1361, 1444-1452 | ECDSA.jet:238 (RFC 6979 A.2.5); TLS.jet:3971; emission case |
| F-TLS13-1 | Low | **Fixed** | Gen13.jet:157-161, 169-202 (174, 180-184), 264; TLS.jet:1160-1172, 2491-2494 | TLS.jet:2915 |
| I-1 | Info | **Fixed** | Tls.jet `__peer_identity` / `__certificate` (~1743-1765) | Tls.jet:1832 |
| I-2 / dudect gaps (c14) | Info | **Fixed; soak depth residual (I-5)** | TLS.jet:3971-4349 | See section 3 |

### F-ZERO-1: fixed

These four pieces address the three fix items of the prior review:

1. **Close impl on the stream state.** `impl __TLSState.Close` (Tls.jet:98-111) calls `ssl_free` and wipes `buffer` and `pending`. `__release` (114-119) is called on `close` (274). It is also called on handshake failure: `__handshake` (1136-1144) wraps `__handshake_steps`.
2. **Engine frees.**
   - `ssl_handshake_free` (TLS.jet:334-352) wipes premaster, randbytes, the TLS 1.3 secrets, `finished_digest` and the PSK, and releases `transform_handshake`.
   - `ssl_free` (359-378) wipes the I/O buffers, releases every `tpool`/`spool` entry and wipes `conf.psk`.
   - `ssl_transform_release`/`wipe` (Defs.jet:1527-1541) wipes `iv_enc`/`iv_dec` and drops the cipher and MAC contexts.
   - `ssl_session_release`/`wipe` (Defs.jet:1555-1572) wipes `master`, `resumption_key` and all four app secrets.
3. **The dropped key objects wipe themselves.** Each has a `Close` impl:
   - `Mpi` (Bignum.jet:695), which covers the ECDH `d` and `z` and the RSA and EC private keys
   - `HashCtx` (MD.jet:120-128), which wipes `buf`, `s32` and `s64`, so HMAC inner and outer states are wiped too
   - `AESKey` (Cipher.jet:40), `GHASHKey`/`GCM`/`CCM`/`ChaCha20`/`Poly1305` (Aead.jet:102, 215, 425, 835, 954)
   - `HMACDrbg` (DRBG.jet:308)
4. **The TLS 1.3 handshake is freed when it completes.** `ssl_handshake_step` calls `ssl_handshake_free` as soon as the state reaches `HANDSHAKE_OVER` under TLS 1.3 (TLS.jet:1388). It no longer waits for the first application record (Msg.jet:1852 is still there as a second path). TLS 1.2 frees in `ssl_handshake_wrapup` through `ssl_handshake_wrapup_free_hs_transform` (TLS.jet:1998-2004, 2040).

**Tests:** Tls.jet:1369 "A finished TLS 1.2 or 1.3 handshake keeps none of its secrets, and close and release free the engine and wipe the plaintext". It checks with `__test_handshake_freed` (1344: `ecdh_ctx`, transcript, `transform_handshake`, secrets) and `__test_released` (1365). Tls.jet:1421 "A refused TLS handshake frees the engine and wipes the buffers on both ends".

### F-CT-1: fixed

- `sign_core` draws t (ECDSA.jet:112) and computes s = t(e + rd)·(kt)⁻¹ mod N entirely in N-sized limbs:
  - `core_mul_mod` (116, 120, 122, 125)
  - `core_add_mod` (118)
  - `gcd_modinv_odd(kt)` (123)
  - r through `mod_ct` (107)
- No `div_mpi`, `mul_mpi` or `mod_mpi` runs on secrets.
- The helpers are constant-flow:
  - `core_montmul` uses a masked final subtraction (Bignum.jet:382-397).
  - `core_gcd_modinv_odd` runs a fixed number of iterations with conditional assign and swap (630-681).
- TLS signing goes through `ecdsa_write_signature` → `ecdsa_sign_det` (ECDSA.jet:218-219, 160-173). It uses an RFC 6979 nonce, with t drawn from the connection RNG.
- **Tests:**
  - ECDSA.jet:238 (RFC 6979 A.2.5 vector under blinding): PASS per `integ/t5a` and `t5b`, TlsLead-41, `jet test Core/Crypto/ECDSA.jet`.
  - TLS.jet:3971 "ECDSA signing on P-256 and P-384 takes as long on fixed and random nonces".
  - The emission case roots `__core_crypto_ecdsa::sign_core`.

### F-TLS13-1: fixed

- `ssl_tls13_parse_certificate_verify` accepts only algorithms that RFC 8446 allows in CertificateVerify (Gen13.jet:174 via TLS.jet:1161-1163: ECDSA P-256/384/521 and RSA-PSS-RSAE).
- It also requires the algorithm to match the key: `ssl_tls13_key_fits` (Gen13.jet:157-161) checks `pk_can_do` plus the curve from `ssl_tls13_sig_alg_curve` (TLS.jet:1167-1172), used at 180-184.
- The write side applies the same filter (Gen13.jet:264), and the misleading comment is corrected (TLS.jet:2491-2494).
- **Test:** TLS.jet:2915. It shows that PSS is accepted, that PKCS#1 v1.5 gets `decrypt_error`, and that a P-256 key under `ecdsa_secp384r1_sha384` gets `decrypt_error`.

### I-1: fixed

- The second DER walk is gone. The peer identity is built from the parsed `X509Crt`:
  - names come from `__name_text` over `asn1.NamedData`
  - times come from `__unix_ms` over `x509.X509Time`
  - the SPKI comes from `crt.pk_raw`
- A dNSName that is not valid UTF-8 is reported as escaped bytes instead of failing.
- **Test:** Tls.jet:1832.

### I-2 and the dudect gaps (c14): fixed

- Section 3 shows each item the prior review said was never measured now has a test.
- The ECDSA test now varies the nonce (TLS.jet:3971).
- A soak tier exists at TLS.jet:4321-4349. The depth residual is I-5.

### c16 (Lucky13, TlsLead-50): verified correct

- **Padding check.** `ssl_decrypt_buf` (Msg.jet:359-414) checks the padding with masks over the last min(256, len) bytes, as mbedtls does (404-411).
- **MAC range.** `cbc_len` is taken from the public length before the padding is removed (397). The MAC range is `max_len = cbc_len − maclen` and `min_len = max(0, max_len − 256)` (423-424).
- **MAC computation.** `ssl_ct_hmac` (61-83) runs the same hash operations for every offset in `min_len..max_len` (inclusive, matching mbedtls `<=`). It selects the output with `memcpy_if`, and it now finishes and wipes the inner context (77-78) and the selected inner digest (81).
- **Peer MAC extraction.** The peer's MAC is read at the secret offset with `memcpy_offset`, which reads every candidate offset (ConstTime.jet:173-175). The highest index it reads is `data + cbc_len − 1`, inside the record.
- **Length checks.** The length checks at 362 and 372 keep `data_len ≥ ivlen` after the IV is removed, so the read at 386 stays in bounds on both the ETM and the non-ETM path.
- **Single verdict.** There is one `InvalidMac` verdict after all the work (437).
- **Test:** TLS.jet:4274, bound |t| < 4.5.
- **Reachability.** Under Core's own policy (Tls.jet:639-647, AEAD suites only) this path is not reachable. The fix protects direct users of the engine.

## 2. New findings

### F-ZERO-2 (Low): the TLS DRBG seed is never wiped

**Source to sink.**
- `__context_new` (Core/Net/Tls.jet:607-611) gets 48 bytes from `core.net.tls.__host_entropy` into `seed` (608) and passes it to `__context_seeded` (610). Nothing ever wipes `seed`, including on the length-mismatch return at 609.
- `__context_seeded` wipes only its own copy, `material` (617-621).
- The generator is `hmac_drbg_seed_buf` (DRBG.jet:358-361). A buffer-seeded instance never reseeds (DRBG.jet:304-305, 418). Its personalization is the constant `"jet_tls"`.

**Impact.**
- Every random value the context draws is a deterministic function of those 48 bytes:
  - ephemeral ECDHE and X25519 private keys (`ecp.gen_privkey` via ECDH.jet:46)
  - the ECDSA blinding t
  - CBC IVs (Msg.jet:279)
- Someone who recovers the seed from freed heap can recompute the ephemeral keys, then the premaster and every traffic key. That decrypts recorded traffic for that connection, which breaks forward secrecy.
- The seed outlives `close()`. Leaving secrets behind after close is exactly what F-ZERO-1 was meant to stop, so this is a gap in that fix's coverage.
- It does **not** give the long-term ECDSA key, because TLS signing uses RFC 6979 nonces (ECDSA.jet:219).

**Why Low.** It needs a separate memory-disclosure primitive, the same precondition as F-ZERO-1.

**Second residue site [INFERENCE].** `material := seed.copy()` followed by `material.extend("jet_tls".bytes())` (617-618) grows a 48-byte list to 55 bytes. If growth reallocates and frees the old buffer without wiping it, a second copy of the seed is left behind. I did not observe the list growth policy. The same pattern holds private-key bytes at ECDSA.jet:163-166 (`data` with d, then pushes) and at DRBG.jet:376/383 (not on the TLS path).

**Fix.**
1. In `__context_new`, call `crypto.wipe(seed)` on every path after line 608, or have `__context_seeded` take `&&seed` and wipe it.
2. Build `material` at its exact final capacity before copying the seed into it (the `mem.__list_reserve_exact` pattern already used at Cipher.jet:95). Do the same in `ecdsa_sign_det` (ECDSA.jet:163-166).
3. Add a test in Tls.jet that seeds a context from a known buffer and asserts the buffer is all zeros afterwards.

## 3. Dudect coverage (c14)

All of these tests pass in `integ/t6c/tests/TlsLead-50.2.out`. The harness is `ct_measure`/`ct_dudect` (TLS.jet:3628-3666, threshold 10), and its sanity test is TLS.jet:3697.

| Required item | Test (TLS.jet line) | Target |
|---|---|---|
| Bitsliced AES | 4075 "the bitsliced AES and the portable GHASH take as long on fixed and random keys and data" | `cipher.blocks_portable` (Cipher.jet:651) |
| Portable GHASH | 4075 (same test) | `aead.ghash_portable` (Aead.jet:147) |
| HKDF / TLS 1.3 key schedule | 4124 | RFC 8448 schedule |
| TLS 1.2 PRF and Finished | 4124 | `ssl_tls_prf_generic` |
| HMAC_DRBG | 4158 | seeding and generation |
| `ssl_decrypt_buf` (TLS 1.3 AEAD tag) | 4200 | record layer |
| `ssl_decrypt_buf` (TLS 1.2 CBC padding/MAC) | 4274 | record layer, \|t\| < 4.5 |
| Extra: ECDSA with varying nonce | 3971 | `sign_core` |
| Extra: ECDH with varying peer point | 3995 | X25519, P-256, P-384 |
| Extra: RSA key and PSS hash | 4037 | RSA-2048 |
| Extra: AEAD key set-up and seal | 4091 | AES-GCM, ChaCha20-Poly1305 |
| Extra: Bignum `exp_mod` and `gcd_modinv_odd` | 4307 | Bignum |
| Soak tier | 4321-4349 | Runs only under `JET_DUDECT_SOAK=1` (`TlsLead-42.2.out`) |

**Emission check.** `tests/cases/core_net_tls/constant_time_primitives_emit_no_secret_branch_or_index.case` now logs `Core/Crypto/,Core/Net/SSL/` and checks `Core/Net/SSL/Keys13.jet` and `ssl_tls_prf_generic`. The entry test is TLS.jet:3886. The record layer is not in the emission check, so it is covered by timing tests only (I-6).

## 4. Fresh full-scope pass: reviewed, no finding

These areas were read in this pass:

- **Record layer** (Msg.jet:61-446)
  - CBC: see c16.
  - AEAD: explicit-IV and tag length checks (330, 336) come before any slicing; a failed tag gives `InvalidMac`.
  - TLS 1.3 inner plaintext: an all-zero record gives `InvalidRecord` (151-162).
  - Encrypt side: bounds checks (210-214, 234, 247, 263, 273, 278), and the plaintext copy is wiped.
- **ConstTime.jet** (whole file): masks only. `memcmp` reads every byte. `memcpy_offset` reads every candidate.
- **ECDH.jet** (whole file) and **ECP.jet** `mul_internal` (1627-1633): `check_privkey` and `check_pubkey` run on every scalar multiplication, including the peer point. A result at infinity is refused (ECDH.jet:56).
- **ECDSA.jet signing** and the deterministic-nonce path.
- **Bignum.jet** montmul, modular add/mul, gcd/modinv, `mod_ct`.
- **MD.jet** `HashCtx` and its Close.
- **DRBG.jet** HMAC_DRBG: the seeded reseed path wipes its seed and nonce; the buffer-seeded instance never reseeds, as in mbedtls.
- **X509CRT.jet**
  - extension parsing: a duplicate extension is refused (302); an unknown or unhandled critical extension is refused (290, 296, 334, 341)
  - basicConstraints and pathLen (175-185, 1086)
  - keyUsage and EKU (535-553), and `check_parent` (1063-1064: only a trusted root may be v1/v2, as in mbedtls)
  - validity time (1089, 1125-1126), name matching (960-969: SAN takes priority over CN), profile checks (1153-1154, 1185, 1219-1220)
- **TLS.jet**
  - config defaults (724-763): renegotiation disabled through `disable_renegotiation = SSL_RENEGOTIATION_DISABLED` (Defs.jet:88, 1144); legacy renegotiation off; EMS and ETM on
  - `ssl_verify_certificate` (2458-2516): refuses to verify without a hostname under REQUIRED (2470); refuses REQUIRED without trust anchors (2513-2516); checks EKU serverAuth/clientAuth (2372-2375)
  - the handshake step and free paths
- **Downgrade protection**
  - Client13.jet:438-491: the sentinel is checked when TLS 1.3 was offered, and a TLS 1.2 ServerHello is refused when min > 1.2.
  - Server12.jet:676-689: the server writes the sentinel.
- **Tls.jet policy and lifecycle**
  - policy (607-699): AEAD-only suites; groups x25519/P-256/P-384; sig_algs ECDSA/PSS/PKCS#1 for TLS 1.2 only, with TLS 1.3 restricted by F-TLS13-1's filter
  - the client always uses REQUIRED (697); an empty hostname cannot slip past verification (675 combined with TLS.jet:2470)
  - the server does not request client certificates (201)
- **`#Unsafe` in scope.** Every region in Cipher.jet, ExpertCt.jet, Crypto.jet, Jetc.jet, Vault.jet and TLS.jet (test vectors only) has a justification string that matches what it does. The library regions are exact-capacity reservations, expert or vault provider boundaries, and Jetc copies that wipe on both outcomes.

These files were **not re-read line by line in this pass**. They rest on the 2026-11-09 review, which recorded no finding in them:

- ASN1.jet, PEM.jet, PKParse.jet, PKWrite.jet, PKCS5/7/12.jet
- RSA.jet (private core and PKCS#1/OAEP unpadding)
- ECPCurves.jet, Keys13.jet
- Client12/Server12 ServerKeyExchange and Finished, Client13/Server13 HRR, key-share and PSK binder parsing
- Ticket/Cache/Cookie/Ser.jet

I did not diff them against the 2026-11-09 baseline.

## 5. Info residuals (non-blocking)

- **I-3.** `gcd_modinv_odd` copies `ap := a.p.copy()` (Bignum.jet:1448) and never wipes it. In ECDSA that copy holds kt, which is uniformly random and independent of k on its own. RSA blinding calls also pass through here.
- **I-4.** `sign_core` returns early on RNG or allocation failure without wiping `dl` and `work`:
  - lines 96 and 103 (`RandomFailed`)
  - lines 107 and 121-124 (`?? return`), which also leave `rd`, `sum`, `sum_t`, `tl` and `kl`
  - These are plain `[U64]`, which has no Close.
- **I-5.** The soak tier is opt-in at 2000-6000 samples, still well below 10⁵. It was run once in integration (TlsLead-42). Whether a nightly sets `JET_DUDECT_SOAK` is unknown.
- **I-6.** The record layer (Msg.jet) is outside the emission check and is covered by dudect only. The CBC path cannot be reached under Core's AEAD-only policy.

## Verdict

**1 open finding.** Criterion c4 is not met until it is fixed.

1. **F-ZERO-2, Low.** `Core/Net/Tls.jet:608-610` (with 617-618 [INFERENCE]): the 48-byte HMAC_DRBG seed for each TLS context is never wiped, so the connection's ephemeral keys can be recovered after close. **Fix:** wipe `seed` on every path in `__context_new`, build `material` at its exact capacity (and the same in ECDSA.jet:163-166), and add a test in Tls.jet asserting the seed buffer is zero after `__context_seeded`.

All prior findings (F-ZERO-1, F-CT-1, F-TLS13-1, I-1, I-2 and the c14 dudect gaps) are fixed at dev `76955986d0`. The c16 CBC/Lucky13 fix is correct. Every dudect item c4 requires is present and passes per the integ REPORT. I-3 to I-6 are informational and do not block the verdict.

## Confirmation 2026-11-10

A read-only confirmation pass at dev `c460ba91e5`, with no tests run (test results are cited from the integration reports). Between the audit base `76955986d0` and `c460ba91e5`, only two submissions touch Core/Crypto or Core/Net/Tls*.jet: TlsLead-51 (Block, Cipher, CipherLayer, ConstTime, DES, SSL/Msg.jet, SSL/TLS.jet and the emission fixture) and TlsLead-53 (`f39b9ac11a`: Bignum.jet, ECDSA.jet and Tls.jet).

|ID|Status at c460ba91e5|Evidence|
|---|---|---|
|F-ZERO-2 (Low)|Closed|`__context_new` wipes the seed on the length-mismatch return. `__context_seeded(endpoint, seed: &[U8])` reserves the material at its exact size, wipes the seed right after copying it (before its only error return), and wipes the material after `hmac_drbg_seed_buf`. `__identity_validate` builds its seed at its exact size after all of its early returns. `ecdsa_sign_det` builds `data` the same way and wipes `db` and `data`. The #Test "Seeding a TLS context wipes the seed…" passes.|
|I-3 (Info)|Closed|`gcd_modinv_odd` wipes its working copy `ap`. Its early returns all come before `ap` exists.|
|I-4 (Info)|Partly closed|Every explicit return in `sign_core` goes through `sign_failed`, which wipes `dl` and `work`; the success path wipes every attempt secret. **Residual I-4b:** four calls propagate their errors automatically and skip `sign_failed`: `derive_mpi` (ECDSA.jet:93), `ecp.gen_privkey` for k (108), `ecp.ecp_mul` (109) and `ecp.gen_privkey` for t (115). On those exits `dl` (the private key's limbs) and `work` are not wiped. They are reachable only through RNG or allocation failure. The fix is an explicit `?? return Err(sign_failed(dl, work, err))` at each one; it is tracked on #6694 c4.|
|I-5 (Info)|Unchanged|The soak tier is still opt-in and below 10^5 samples.|
|I-6 (Info)|Emission gap closed|TlsLead-51 adds `check Core/Net/SSL/Msg.jet` to the emission spec, and that case passes. The CBC path is still unreachable under Core's AEAD-only policy.|

The TlsLead-51 diff adds no new finding:

- `ctime.table_entry` reads every table word and selects with borrow masks. The ARIA, Camellia and DES S-boxes all go through it, and the DES PC1 tables are replaced by bit arithmetic.
- In CBC record decryption, the padding and content lengths are masked words that stay in bounds whether the padding is bad or good. The record length is revealed only after the single `InvalidMac` verdict, as in Mbed TLS.
- The cipher padding checks keep masked words until the final verdict.
- The new `public` emission entries cover only verdicts, revealed lengths, configuration, the CCM counter block and connection IDs.

Verdict: no Low-or-higher finding is open. One Info residual, I-4b, stays open until its fix lands.
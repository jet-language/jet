# Core TLS and crypto path: security review (2026-11-09)

Card: Tower #6694, criterion c4. Source: dev commit c057ea659d. The
integration tree used ($JET_SCRATCH_ROOT/lanes/Main/integ/t3c/tree) has no
patch that touches Core/Crypto or Core/Net. Upstream reference: mbedtls 3.6.

A read-only security-reviewer agent wrote the review. Main recorded it. No
probes were run: the reviewer had no shell, so each finding rests on a
source argument with file and line.

## Scope

- Core/Crypto/*.jet: ConstTime, Bignum, ECP, ECDSA, RSA, DRBG, ASN1,
  X509CRT, MD, Cipher (AES), Aead (GCM, ChaCha20-Poly1305), Crypto, Expert,
  ExpertCt, Jetc and Vault (the #Unsafe inventory only).
- Core/Net/Tls.jet: the public TLS stream, its policy and the system roots.
- Core/Net/SSL/*.jet: Defs, TLS, Msg, Client, Client12, Client13, Server12,
  Server13, Gen13 and Keys13, plus the dudect harness (TLS.jet:3472-3768).
- The following are off Core's path; the review only confirmed that Core's
  policy (Tls.jet:606-632) configures none of them: Ticket, Cookie, PSK,
  DTLS, the CBC/MAC record mode, Block (ARIA/Camellia), DES, CTR_DRBG, DHM,
  PKCS5, PKCS12, ECJPAKE and LMS.

## Method

- Read each port side by side with mbedtls 3.6.
- Traced peer-controlled input (records, handshake messages, certificates,
  extensions) to the checks that guard it.
- Traced secret data (keys, nonces, the premaster and the traffic secrets)
  to every branch, table index and variable-length loop it reaches.
- Mapped each secret-dependent primitive on the TLS path to a dudect test
  in TLS.jet.

## Findings

| ID | Severity | Location | Issue | Fix |
|---|---|---|---|---|
| F-ZERO-1 | Medium | Core/Net/Tls.jet:242-249, 1254-1277, 1103-1118; Core/Net/SSL/Defs.jet:949-1382; Msg.jet:1842; Gen13.jet:499-505 | Core's TLS stream never frees or wipes the engine's secrets. | See F-ZERO-1. |
| F-CT-1 | Low | Core/Crypto/ECDSA.jet:73-101 (94, 96-98); Bignum.jet:1127 | ECDSA signing drops mbedtls's blinding of the s computation and reduces unblinded secrets with variable-time code. | Restore mbedtls's t-blinding exactly. |
| F-TLS13-1 | Low | Core/Net/SSL/Gen13.jet:163-173; TLS.jet:1178-1191, 2468-2470; Tls.jet:625-631 | The TLS 1.3 CertificateVerify signature algorithm is not tied to the peer's key, as RFC 8446 requires. | See F-TLS13-1. |
| I-1 | Info | Core/Net/Tls.jet:1528-1734 | The second DER walk in `__certificate_parts` and `__der_element` is stricter than x509. | Accept what x509 accepts, or reuse the parsed X509Crt fields. |
| I-2 | Info | Core/Net/SSL/TLS.jet:3683-3717 | The dudect EC and RSA runs are small. | Raise the counts in a nightly or soak tier. |

I-1: the walk runs only on certificates that are already verified, and it is
bounds-checked. It rejects a UTCTime without seconds and a dNSName that is
not UTF-8. The only cost is a failed handshake, so it affects availability,
not security.

I-2: ECDH, ECDSA and RSA run 200-600 measurements each. That finds only
large leaks; dudect usually needs 10^5 or more.

### F-ZERO-1 (Medium): engine secrets are never freed or wiped

**Issue.** `TLSStream.close` and `__close_write` only send close_notify and
shut down the socket. Nothing in Tls.jet calls `ssltls.ssl_free`; its only
callers are Ser.jet:565 and TLS.jet's own internals. The handshake-failure
path (`__handshake`, 1103-1118) frees nothing either.

**Evidence.** SSLContext, SSLSession, SSLTransform and SSLHandshake have no
`impl Close`, and `[U8]` has no destructor. So these die unwiped:

- `SSLSession.master` (Defs.jet:990), `resumption_key` (998) and
  `app_secrets` (972-975)
- `SSLTransform` `iv_enc` and `iv_dec` (956-957), and the cipher keys
- the `SSLHandshake` premaster, randbytes, TLS 1.3 secrets and PSK (1226,
  1266-1276)
- `SSLConfig.psk` (1171)
- `in_buf` and `out_buf` (1336, 1359)
- `__TLSState.buffer` and `pending`, which hold plaintext

In TLS 1.3 the handshake structure is freed only when the first
application-data record arrives (Msg.jet:1842); Gen13.jet:499-505 does not
free it. This breaks Core's own convention: DRBG, GHASHKey and
X25519SecretKey all wipe in Close.

**Fix.**

1. Give `__TLSState` (or SSLContext) an `impl Close` that calls `ssl_free`
   and wipes `buffer` and `pending`.
2. Run it on close and on handshake failure.
3. Free the TLS 1.3 handshake when the handshake completes, as mbedtls's
   handshake wrapup does.

### F-CT-1 (Low): ECDSA signing reduces unblinded secrets in variable time

**Issue.** mbedtls `ecdsa_sign_restartable` draws a random t
(`gen_privkey(p_rng_blind)`) and computes s = (e + r·d)·t · (k·t)^-1 mod n,
so no reduction ever sees an unblinded secret. Jet computes s without t.

**Evidence.**

- `inv = gcd_modinv_odd(k)` (ECDSA.jet:96) is constant time.
- After it, `mul_mpi(inv.i, sum)` (97) and `mod_mpi(prod, grp.n)` (98) run
  on the unblinded k^-1 and e + r·d.
- `mod_mpi` goes through `div_mpi` (Bignum.jet:1127, HAC 14.20), whose
  quotient-correction loops and `used_limbs` depend on the dividend's top
  limbs.
- `mul_mpi(r, d)` (94) also sizes its work by the `used_limbs` of d.

The leak tracks the high bits of e + r·d mod n, where r and e are public for
each signature. That is a Hidden Number Problem instance in d, so enough
signatures with partial leakage allow lattice key recovery (the Minerva
class). A Core TLS server signs on demand for remote peers: the
ServerKeyExchange in TLS 1.2 and the CertificateVerify in TLS 1.3.

Severity is Low because the leak is a few data-dependent correction
iterations, not a timing difference over the whole scalar.

**Fix.** Restore the t-blinding exactly as mbedtls 3.6 does it.

### F-TLS13-1 (Low): CertificateVerify algorithm not tied to the peer key

**Issue.** `ssl_tls13_parse_certificate_verify` accepts any algorithm Core
offered, as long as `pk_can_do` holds.

**Evidence.**

- Core offers `rsa_pkcs1_sha256/384/512` in TLS 1.3 (Tls.jet:629-630,
  through `ssl_tls13_sig_alg_is_supported`, TLS.jet:1156-1158), and
  `ssl_get_pk_type_and_md_alg_from_sig_alg` maps them to RSA PKCS#1 v1.5
  (TLS.jet:1179-1183). So a TLS 1.3 peer's PKCS#1 v1.5 CertificateVerify is
  accepted, although RFC 8446 §4.4.3 says RSA signatures MUST use
  RSASSA-PSS.
- `ssl_tls13_sig_alg_for_cert_verify_is_supported` (TLS.jet:1151) runs only
  when Core chooses its own signature (Gen13.jet:253, Server13.jet:542).
- For ECDSA, only `pk_can_do` is checked (Gen13.jet:170). The curve that the
  code point names (`ecdsa_secp256r1_sha256` and so on) is never compared
  with the key's group.
- TLS 1.3 skips the TLS 1.2 curve check (TLS.jet:2470), so a TLS 1.3 leaf on
  a curve outside Core's policy is accepted. The comment at TLS.jet:2468
  ("checked with the signature") is false.

The signature is still checked against the authenticated key, so this is
non-conformance and a policy bypass, not an authentication bypass. mbedtls
3.6 `ssl_tls13_parse_certificate_verify` appears to have the same shape, so
the defect is probably inherited from upstream; this was not verified.

**Fix.**

1. In Gen13.jet after line 163, reject the signature unless
   `ssl_tls13_sig_alg_for_cert_verify_is_supported(algorithm)` holds.
2. For ECDSA code points, require `pk_get_ec_group_id` to equal the named
   curve.
3. Correct the comment at TLS.jet:2468.

## Reviewed with no finding

- **ConstTime.jet.** `ctime.memcmp` and the masks are correct.
  `constant_time_equal_bytes` (Crypto.jet:372) handles a length mismatch
  safely.
- **Record protection (Msg.jet:55-436).**
  - CBC padding and the constant-flow HMAC match mbedtls.
  - Every failure returns InvalidMac.
  - With encrypt-then-MAC, the MAC is checked before decryption.
  - The AEAD tag is compared in constant time.
- **AES (Cipher.jet).** It is bitsliced (the aes_ct64 layout with the
  Boyar-Peralta S-box), and the key schedule's `sub_word` uses the same
  circuit (Cipher.jet:545-599). No table is indexed by a secret.
- **Aead.jet.**
  - GHASH is ghash_ctmul64 (Aead.jet:55-197).
  - Poly1305 does its final reduction under a mask (1058-1061).
  - ChaCha20 only adds, rotates and xors.
- **DRBG.jet.** Reseed errors propagate, and Close wipes the state.
- **Bignum.jet.**
  - `core_montmul` does its final subtraction under a mask (364-379).
  - `core_exp_mod` does a constant-time window lookup for a secret exponent
    (437-500).
  - `core_gcd_modinv_odd` is constant time (538-589).
  - `exp_mod_optionally_safe` (1238-1273) matches mbedtls.
- **RSA.jet `private_core` (586-612).** Base blinding, exponent blinding and
  the CRT fault check match mbedtls 3.6.
- **ECP.jet.**
  - `comb_select` is constant time (1323-1335).
  - `comb_recode_scalar` uses `safe_cond_assign`.
  - `mul_mxz` runs a fixed number of iterations with `cond_swap` and
    randomized coordinates.
  - The `jac_add_mixed` branches (1130-1148) match mbedtls and are taken
    with negligible probability.
  - `check_pubkey` and `check_privkey` run inside `mul_internal`.
- **ASN1.jet `get_len` (119-138).** It takes at most 4 length bytes, is
  bounds-checked, and cannot overflow.
- **X509CRT.jet.**
  - Unknown critical extensions are refused (296), and a known but unparsed
    critical extension returns FeatureUnavailable (341).
  - The two signature algorithms must match (351-405).
  - Chain building and verification match mbedtls 3.6 (1061-1224): path
    length, the CA bit, keyCertSign, MAX_INTERMEDIATE_CA and time.
  - Name matching, wildcards and IP SANs match (712-723, 904-932, 960-969).
- **`ssl_verify_certificate` (TLS.jet:2435-2498).**
  - A client set to REQUIRED with no hostname fails (2447).
  - Verification fails without a CA chain (2489).
  - keyUsage and the serverAuth EKU are checked (2324-2358).
- **Tls.jet.**
  - The client sets authmode REQUIRED (664).
  - The system roots must load when they are required (799).
  - The policy (606-632) allows only AEAD suites, and only the groups
    X25519, P-256 and P-384.
  - No tickets, PSK or DTLS are configured.
- **TLS 1.2 client.**
  - The ServerKeyExchange curve and the signature algorithm must both be
    ones Core offered (Client12.jet:449-450, 545).
  - extended_master_secret is accepted only when Core offered it (201-205).
- **TLS 1.3 client.**
  - ServerHello is parsed with the allowed-extension mask for that message.
  - The suite must be one Core offered, and it must match after a
    HelloRetryRequest (Client13.jet:576-582).
  - The key exchange mode must be enabled (646-658).
- **Downgrade sentinel.** Client13.jet:460-491 and Server12.jet:676-689 are
  correct.
- **Renegotiation.** It is disabled by default (Defs.jet:1144), and the RFC
  5746 checks compare verify data in constant time (Client12.jet:413-418;
  Server12.jet:65-76, 439-449).
- **Finished.** TLS 1.2 (TLS.jet:2108-2117) and TLS 1.3 (Gen13.jet:459-478)
  compare with `ctime.memcmp`. The TLS 1.3 binder compares are constant time
  (Server13.jet:229, 241-257).
- **Keys13.jet.** The HkdfLabel encoding with the "tls13 " prefix, the
  length limits (121-124), the "derived" evolution (181-188) and the
  Finished key wipe (246-251) follow RFC 8446 §7.1.
- **ALPN parsing (TLS.jet:2261 onward).** It is bounds-checked.
- **#Unsafe blocks in scope.** Each is a capability or audit marker with a
  stated reason, and none reaches a memory-unsafe sink:
  - Cipher.jet:79 and 95 and ExpertCt.jet:13, 39 and 325 are exact-capacity
    reserves.
  - Crypto.jet:287-305, 646, 1187, 1214, 1522, 1620, 1629 and 1635 are
    expert-API boundaries. Their typed callers validate widths and reject
    an all-zero X25519 result (Crypto.jet:956-962).
  - Jetc.jet:98-273, Vault.jet:230-362 and Net.jet:140-433 are provider or
    carrier boundaries.
  - TLS.jet:3319-3767 is test-vector and dudect code.

  Out-of-bounds indexing in Jet panics.

## dudect coverage gaps

The harness is TLS.jet:3472-3768. It covers:

- `ctime.memcmp` and `constant_time_equal_bytes`
- the AES-128 and AES-256 key and block
- AES-256-GCM and ChaCha20-Poly1305 open, varying only the tag
- the HMAC-SHA-256 and HMAC-SHA-384 key
- X25519, P-256 and P-384 ECDH
- P-256 and P-384 ECDSA signing
- RSA-2048 signing
- PEM base64 decoding
- the expert X25519, Ed25519, GCM and XChaCha20-Poly1305 functions

The first clause of c4, "covers every secret-dependent primitive", is not
met. These are the gaps on Core's TLS path:

1. **The ECDSA nonce (3690-3704).** Signing uses `vec_rng()`
   (TLS.jet:2664-2667), an HMAC_DRBG with a fixed seed, so the nonce k and
   the blinding are the same in both classes and only d varies. Never
   measured: `gen_privkey`, the comb with k, `gcd_modinv_odd(k)`, and the
   `mul_mpi` and `mod_mpi` steps of F-CT-1.
2. **Variable-base ECDH (3679-3688).** The peer point is the generator
   (`ct_generator`, 3674), so only the `p_eq_g` static-table path
   (ECP.jet:1407) is measured. TLS ECDHE runs `comb_precompute` on peer
   points (1408-1416), and that path is not measured for P-256 or P-384.
3. **RSA (3706-3718).** The key is fixed and the blinding deterministic;
   only the hash varies. Not measured: different private keys or exponents,
   and RSA-PSS `pss_sign_core` (RSA.jet:885), which Core's TLS 1.3 policy
   uses.
4. **Hardware dispatch.** `blocks` (Cipher.jet:642-647) and `ghash`
   (Aead.jet:139-143) prefer the AES-NI and PCLMULQDQ leaves. On such a
   host the AES and GCM tests measure only the hardware path, and the
   bitsliced AES and `ghash_portable` fallbacks go unmeasured.
5. **AEAD open.** Only the tag varies. The key, H, the ChaCha20 keystream,
   the Poly1305 r and s, and AEAD seal are never varied.
6. **No test at all:**
   - HKDF and the TLS 1.3 key schedule (Keys13.jet)
   - the TLS 1.2 PRF and the Finished verify_data
   - HMAC_DRBG generate
   - Bignum `exp_mod` and `gcd_modinv_odd` on their own
   - `ssl_decrypt_buf` in the record layer, end to end
7. **Sample sizes.** EC and RSA take 200-600 measurements (I-2).

Off Core's TLS path, none of these has a test:

- CTR_DRBG, DHM/FFDHE, PKCS5 PBKDF2, PKCS12, PEM decryption, ECJPAKE and LMS
- P-521, Curve448 and brainpool
- PKCS#1 v1.5 and OAEP decryption unpadding (RSA.jet:728-860)
- the CBC/MAC record mode (`ssl_ct_hmac`)
- the Ticket and Cookie HMAC
- Block.jet (ARIA/Camellia) and DES.jet, which are documented as not
  constant time (Block.jet:10-12, DES.jet:7-10)

## Verdict

Fails. The open findings are F-ZERO-1 (Medium), F-CT-1 (Low) and F-TLS13-1
(Low), plus I-1 and I-2 (Info). Criterion c4 is not met on either clause:
the review records open findings, and the dudect coverage gaps above remain.

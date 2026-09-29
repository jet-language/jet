# #3256 — Common cryptographic primitive coverage (core.crypto)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-luna/safe-jet.sh`).
Primary sources read: `Core/crypto/{crypto,expert,random}.jet`, `Core/math/random.jet`,
`Compiler/JetFoundation/Source/Registry/{CoreCallRows,CorePlatformSignatures}.jet`,
`crates/jet-pkg-model/src/Prelude/Crypto.rs`, `crates/jet-jit/Cargo.toml:48-66`.
Reference vectors were generated independently with Node 24 / OpenSSL 3.6
(`~/.cache/jet-test-scratch/Closer00/crypto_ref.mjs`) and a BLAKE3 reference port
(`blake3_ref.py`, matches the published empty and "abc" vectors); they equal the
published RFC/FIPS/draft vectors cited in the witness header.

## 1. Accounting (criterion 1)

"Body" says where the executed meaning lives. *Jet* = pure-Jet body in Core,
hand-written. *Host* = a `CoreCallRows.jet` row routes the call to a Rust
Prelude leaf in `crates/jet-pkg-model/src/Prelude/Crypto.rs` built on pinned
RustCrypto/dalek crates (`crates/jet-jit/Cargo.toml:49-65`: aes-gcm 0.10.3,
argon2 0.5.3, blake3 1.8.2, chacha20poly1305 0.10.1, ed25519-dalek 2.2.0,
hkdf 0.12.4, sha2 0.10.9, subtle 2.6.1, x25519-dalek 2.0.1). "Targets" = which
bodies can run where: Jet bodies are target-neutral; host leaves exist only where
the Rust Prelude is linked (native AOT/JIT/interpreter); no web/no-OS crypto
leaf was found.

| API | Algorithm / spec | Key / nonce / sizes | Error on bad size | Body | Secret lifetime |
|---|---|---|---|---|---|
| `sha1` | SHA-1, RFC 3174 | – | none (infallible) | Jet | – |
| `sha224`, `sha256`, `sha384`, `sha512` | SHA-2, FIPS 180-4 | – | none | Jet | – |
| `sha3_224..sha3_512` | SHA-3, FIPS 202 | – | none | Jet | – |
| `blake3` | BLAKE3-256 | – | none | Jet | – |
| `new`/`update`/`digest` (`Hasher`) | SHA-256 over buffered bytes | – | none | Jet struct; host rows `__hasher_*` also exist | buffer is a plain `[U8]` |
| `hmac_sha256` | HMAC, RFC 2104 / 4231 | any key length | none | Jet | – |
| `hkdf_sha256` | HKDF-SHA256, RFC 5869 | len 0..=8160 | `CryptoError.Length` | Host row (`jet_crypto_hkdf_sha256_impl`, hkdf crate) + Jet body | returns `Secret` |
| `pbkdf2_hmac` | PBKDF2-HMAC-SHA256, RFC 8018 | iter ≥1, len 1..=64 MiB | `CryptoError.Length` | Jet | returns plain `[U8]` |
| `constant_time_equal(_bytes)` | – | – | none | Host row (`subtle::ct_eq`) + Jet body with length/index branches | – |
| `seal`/`open` | JETV1: X25519 recipients + XChaCha20-Poly1305 | 1..=256 recipients, 32-byte keys, ≤16 MiB | `KeyRejected`, `Length`, `OpenFailed` | Host rows (`jet_crypto_seal/open_typed_impl`) + Jet body | host zeroizes; Jet copies are plain |
| `file_seal`/`file_open` | JETV / JETC v2 (diverge, see #3667) | – | `FileCryptoError` | Host rows + Jet body | – |
| `sign`/`verify` | Ed25519, RFC 8032 | seed 32, sig 64 | `KeyRejected` | Host rows (ed25519-dalek) + Jet body | host zeroizes |
| `x25519`/`x25519_public`/`x25519_shared` | X25519, RFC 7748 | 32/32 | `KeyRejected` (Jet); `String` in the registry signature for `x25519_public/_shared` | Host rows (x25519-dalek) + Jet body | `SharedSecret` plain struct |
| `wrap`/`unwrap` | JETW v1 (X25519 + HKDF + AEAD) | ≤1 MiB secret | `KeyRejected` | Host rows + Jet body | – |
| `password_hash(_with_salt)`/`password_verify` | Argon2id v19, m=65536 t=3 p=1, 32-byte tag, PHC text | salt 8..=64 | `Length`, `KeyRejected` | Jet (calls `expert.argon2id`) | `PasswordHash{text}` |
| `generatekey`, `privateencrypt`, `privatedecrypt`, `publicencrypt`, `publicdecrypt` | legacy spellings | – | as `seal`/`open` | Jet wrappers | – |
| `expert.aes256gcm_seal/open` | AES-256-GCM, SP 800-38D | key 32, nonce 12 | `Length` | Jet (table S-box) | – |
| `expert.xchacha20poly1305_seal/open` | XChaCha20-Poly1305 (draft-irtf-cfrg-xchacha-03) | key 32, nonce 24 | `Length` | Jet | – |
| `expert.argon2id` | Argon2id, RFC 9106 | m 8192..=262144 KiB, t 1..=10, p 1..=8, out 16..=64, salt 8..=64 | `Length` | Jet | returns `Secret` |
| `expert.ed25519_sign`/`ed25519_verify_strict` | Ed25519 | 32 / 64 | `KeyRejected` / `Length` | Jet | – |
| `expert.x25519_raw`, `hkdf_sha256_raw` | X25519, HKDF | 32/32; len ≤8160 | `KeyRejected`, `Length` | Host rows for `x25519_raw`; Jet for `hkdf_sha256_raw` | returns `Secret` |
| `random.bytes`/`token_*`/`int_range`/… | OS CSPRNG (`core.crypto.random`) | n 0..=1 MiB | `Length`, `Unavailable` | Host cell | – |

Legacy-named exports (plan item): `generatekey` = X25519 keygen from the
CSPRNG; `privateencrypt(k)` = `seal` to `x25519_public(k)`;
`publicencrypt(pk)` = `seal([pk])`; `privatedecrypt`/`publicdecrypt` = `open`.
They are aliases of the envelope, not RSA-style primitives, so their names
promise something they do not do. That goes to a ballot (below).

Secret lifetime: in `Core/crypto/crypto.jet:96-118` every secret-bearing type
(`Secret`, `SigningKey`, `X25519SecretKey`, `SharedSecret`) is a plain struct
with a `pub bytes: [U8]` field. There is no zeroization in Jet and nothing stops
printing. Observed: `print("{crypto.Secret{bytes: "hunter2".bytes()}.bytes}")`
prints `[104, 117, 110, 116, 101, 114, 50]` and `String.from_bytes` prints
`hunter2`, with no `#Unsafe` (`--interpret`, `secretprobe.jet`). That violates
D-CRYPTO-API1 (spec: secret values "cannot use ordinary equality, printing,
interpolation…; every [exposure] call requires an audited `#Unsafe` region").
The Rust leaves do zeroize (`Crypto.rs:221-224`, `zeroize(&mut raw)`).

## 2. Audited implementations vs novel crypto (criterion 2): verdict

**Not satisfied today.** The hashes, HMAC, PBKDF2, AES-GCM, XChaCha20-Poly1305,
Argon2id and the Jet Ed25519/X25519 bodies are hand-written Jet
(`crypto.jet:21-28` header: "Written in Jet"). None has an external audit.
Two side-channel hazards are visible in the source:
- `expert.jet` AES uses a table S-box (`C_AES_SBOX`, :14) indexed by
  secret-dependent bytes (a classic cache-timing leak).
- Ed25519/X25519/Poly1305 Jet bodies use unbounded `Int` arithmetic
  (`C_P25519`, `C_ED_*` at :22-32; `poly1305` `le_int`), whose cost depends
  on operand size, so they are not constant-time. The Jet
  `constant_time_equal_bytes` (crypto.jet:229-244) also branches on
  `i < n` / `i < m`.

The audited RustCrypto/dalek leaves exist and are pinned, but only some rows
reach them, and #3667 is moving those meanings into Jet. The unresolved
question (ship hand-written Jet crypto vs keep audited leaves as the executed
meaning) is an owner vote; ballot draft B1 below.

Correctness today (known-answer vectors, `jet run`, single-call probes
`cprobe.sh`): correct for sha1, sha224, sha256, HMAC-SHA256, PBKDF2 and
`constant_time_equal_bytes`. **Wrong** for sha384 (all-zero digest), sha3_256
(all-zero digest) and blake3 ("abc" → `f483716b…`, expected `6437b3ac…`).
sha512 ICEs. AES-256-GCM traps. The full matrix per tier is in §5.

## 3. Cryptographic vs simulation randomness (criterion 3)

The split exists by module and doc comment: `Core/crypto/random.jet:5-11`
(fail-closed OS CSPRNG, `CryptoError.Unavailable`, no fallback) vs
`Core/math/random.jet:1-9` (deterministic, seedable, "This is not a CSPRNG").
The confusable cell is `core.math.random.bytes`/`randbytes`/`getrandbits`
(:131, :234, :237). These are seeded simulation bytes under the same verbs as
the CSPRNG. **Met as an inventory.** The executable proof is broken on every
tier: the existing `crypto/random_api_split` golden (after a tiny example fix
for the current API: `crypto_rand.bytes(16) ?? return`, and `hkdf_sha256(ikm, …)`
now takes `[U8]`) fails as follows. AOT: generated Rust E0599
`JetCryptoError::Length` not found. `jet run`: ICE "MIR user call … does not
preserve its checked Result/Option return carrier". `--interpret`: E3001
"Value doesn't fit in destination type" at `Core/math/random.jet:139`.

## 4. Gaps and scoped owner votes (criterion 4)

No new algorithm or dependency is adopted here. Ballot drafts (one decision
each, Pip files them):

- **B1 Executed meaning for primitives.** A: audited pinned crates
  (RustCrypto/dalek) are the executed meaning on native tiers, and Jet bodies
  are the reference/web fallback, gated by the same KAT matrix. B: hand-written
  Jet is the only meaning (current #3667 direction), with an external audit,
  constant-time review (no table AES, fixed-width field arithmetic) before 1.0.
  C: hybrid, with hashes in Jet and AEAD/signature/KDF on audited leaves.
  Recommendation: C. Hashes are low-risk and easy to KAT, while AEAD/curve code
  needs constant-time guarantees Jet cannot yet state. Beginner path: no change
  (`seal`/`sign`). Expert path: `expert.*` names unchanged.
- **B2 HMAC/HKDF over SHA-512** (`hmac_sha512`, `hkdf_sha512`): needed for
  JOSE HS512 and some KDF profiles. Uses the existing SHA-512 body; no new
  dependency.
- **B3 ECDSA P-256 and RSA-PSS/PKCS#1 v1.5 verification only** (JOSE/WebAuthn
  interop). New dependency (p256/rsa crates) or new Jet code. Verify-only keeps
  the key-handling surface small.
- **B4 scrypt** (legacy password hashes import): new algorithm. Recommend
  verify-only for migration.
- **B5 Legacy spellings** `generatekey`, `privateencrypt`, `publicencrypt`,
  `privatedecrypt`, `publicdecrypt`: A remove (greenfield cutover), B keep as
  documented aliases. Recommend A: the names suggest RSA semantics they do not
  have.
- **B6 Secret exposure**: make `Secret`/key `bytes` non-public with
  `#Unsafe` accessors and zeroize-on-drop, per the ratified D-CRYPTO-API1. This
  is an implementation fix, not a new vote (filed as a defect).

## 5. Known-answer matrix (criterion 5)

Witness: `Examples/features/crypto/primitive_matrix.jet` (new). It covers
RFC/FIPS vectors for every exported primitive plus a typed-error row for each
size check. Two rows were removed to let the rest compile: `sha512` (ICE:
"checked Core call `core.crypto.sha512` has no canonical TIR record"; earlier
`Digest512::hex` "missing checked function target" and `.bytes` "missing
checked MIR owner type") and the streaming `Hasher` (ICE "MIR entry Err has an
invalid checked carrier"). Error rows print only `Err`, because
`CryptoError` variants cannot be observed from user code: matching
`.OpenFailed` ICEs (`PatternTest`), `error == .Length` is rejected with E0305
"`CryptoError` is a struct, not an enum", and naming `crypto.CryptoError` in a
type position is E1004. Bare `CryptoError` names a different prelude type (E0109).

The whole witness still does not compile on any tier (next ICE: "missing
checked MIR owner type" at an inline `crypto.X25519SecretKey{…}` argument).
So c5 was measured per call (`~/.cache/jet-test-scratch/Closer00/cprobe.py`,
`hash_aot.jet`; outputs in `probe*_{run,int}.txt`) against the independent
references:

| primitive (vector) | `jet run` | `--interpret` | AOT |
|---|---|---|---|
| sha1, sha256 ("abc") | ok | ok | ok |
| sha224 ("abc") | ok | E0956 `slice_list_range` unsupported | ok |
| sha384 ("abc") | WRONG all-zero | E0956 | WRONG all-zero |
| sha512 | ICE (no TIR record) | ICE | ICE |
| sha3_224/256/384/512 ("abc") | WRONG all-zero | WRONG all-zero | WRONG all-zero |
| blake3 ("abc") | WRONG `f483716b…` | WRONG same | WRONG same |
| hmac_sha256 (RFC 4231 #2) | ok | ok | ok |
| pbkdf2_hmac (c=1/2) | ok | ok | build fails: generated Rust E0599 `JetCryptoError::Length` |
| Hasher new/update/digest | ICE (MIR entry Err carrier) | not run | not run |
| hkdf_sha256 (RFC 5869 A.1) | not measured (probe file collision) | E0956 unsupported | not run |
| x25519_public (RFC 7748) | ICE `JIT drop CryptoError enum discriminant is invalid` | E0956 unsupported | build fails: E0560 `JetX25519SecretKey` is a tuple struct |
| sign (RFC 8032 #1) | ICE (same JIT drop) | ok `e5564300…7a100b` | build fails (E0560, key literal) |
| expert.ed25519_sign (RFC 8032 #1) | E3010 divided by zero | E3010 divided by zero | not run |
| expert.x25519_raw | ICE bad sequence handle | E0956 unsupported | not run |
| expert.xchacha20poly1305_seal ("ab") | ok `906e0e07…` (Node ref) | ok | not run |
| expert.aes256gcm_seal (GCM TC13) | E3010 trap at expert.jet:941 | E3001 same | not run |
| expert.argon2id (m=8192,t=1,p=1) | ICE (Result carrier) | E0956 fuel exhausted | not run |
| length errors: aes256gcm short key / hkdf len 8161 | `Err` / `Err` | `Err` / E0956 unsupported | not run |
| password_hash_with_salt (m=65536,t=3) | no output | E0956 fuel exhausted | not run |
| x25519_public with 1-byte key | ICE missing MIR owner type | not run | not run |

**Criterion 5 unmet.** No golden was blessed.

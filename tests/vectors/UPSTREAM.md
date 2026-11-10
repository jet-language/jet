# Test vector sources

The algorithm vector tests in `Core/Net/SSL/TLS.jet` (the `#Test`s after "The
published test vectors", run with `jet test Core/Net/SSL/TLS.jet` from the
repository root) read these files.

## NIST CAVP (committed)

NIST CAVP vectors are works of the United States government and are not
subject to copyright. They come from the response files of the Cryptographic
Algorithm Validation Program (csrc.nist.gov,
Projects/Cryptographic-Algorithm-Validation-Program):

| Archive | Archive SHA-256 |
| --- | --- |
| `gcmtestvectors.zip` | `f9fc479e134cde2980b3bb7cddbcb567b2cd96fd753835243ed067699f26a023` |
| `drbgtestvectors.zip` | `5f7e5658ebd5b4e6785a7b12fa32333511d2acc2f2d9c5ae1ffa16b699377769` |
| `186-4ecdsatestvectors.zip` | `fe47cc92b4cee418236125c9ffbcd9bb01c8c34e74a4ba195d954bcb72824752` |
| `ecccdhtestvectors.zip` | `5fff092551f2d72e89a3d9362711878708f9a14b502f0dfae819649105b0ea39` |
| `186-3rsatestvectors.zip` | `8405aeb3572a4f98ed4b1a3ccb3f2f49e725462dd28ec4759d6a15d88855d19c` |
| `tls.zip` (SP 800-135 KDFs) | `5944b3affa6bfccd1fc4b2876ab273526910447aa754effae0db9fe7c8412793` |

Each file keeps the source's header comment lines and, from the sections that
match the parameters Core's TLS enables, whole records byte for byte (CRLF as
NIST ships them):

| File | Source | Kept | SHA-256 |
| --- | --- | --- | --- |
| `nist/gcmEncryptExtIV128.rsp` | `gcmEncryptExtIV128.rsp` | `[IVlen = 96]` with `[Taglen = 128]`, first 5 records of each | `97eb36c1b871531aedba126b226fb919b5a1fe66939ea52600b82e12111405fb` |
| `nist/gcmEncryptExtIV256.rsp` | `gcmEncryptExtIV256.rsp` | the same | `bf7f6a29b89ff84db4433810ff2469e26d2413e1592339efcca7d33fbc982aca` |
| `nist/gcmDecrypt128.rsp` | `gcmDecrypt128.rsp` | the same | `a7552d523a1929f6bcf0b9018b4955909a8ec2b447777c0701ffa5d4db709292` |
| `nist/gcmDecrypt256.rsp` | `gcmDecrypt256.rsp` | the same | `5c84898019f012bd743b9afeef3bb0904d9f30471a71a18f37e8612963afbfaf` |
| `nist/HMAC_DRBG_SHA256.rsp` | `drbgtestvectors/nr/HMAC_DRBG.rsp` | `[SHA-256]`, first 5 records of each | `373dd00ec7891e9626bc948b9fb70257b343a56e06dd5edca0fedc5dff2b5887` |
| `nist/ECDSA_SigVer.rsp` | `186-4ecdsatestvectors/SigVer.rsp` | `[P-256,SHA-256]` and `[P-384,SHA-384]` | `a2fab37238b9794c626d518e81b26b514f22c959004d5910ce50a0f0c14a970f` |
| `nist/ECC_CDH_Primitive.txt` | `KAS_ECC_CDH_PrimitiveTest.txt` | `[P-256]` and `[P-384]` | `95457a9d82637c48448e99a9adbfaf590e45032adc0a5c68885df38715ebff07` |
| `nist/TLS_PRF.rsp` | `tls/tls.rsp` | the whole file; the tests run the TLS 1.2 SHA-256 and SHA-384 sections | `006e47616f4dd6a783d549f917a48cf16eabf873973f2e2ad747ad866239154e` |
| `nist/RSA_SigGen15.rsp` | `SigGen15_186-3.rsp` | `[mod = 2048]` and `[mod = 3072]`; the tests skip the SHA-224 records | `b18a5fd3ee2755837c433ad4668b3e29a9c2c9dd0b657f6acc9bbe9ce6699951` |
| `nist/RSA_SigVer15.rsp` | `SigVer15_186-3.rsp` | the same | `fc585947f6ab0e70162b283c65c036285755cd3d8daab6dfd2a7d3104edcb971` |
| `nist/RSA_SigVerPSS.rsp` | `SigVerPSS_186-3.rsp` | the same, also skipping SHA-1 | `f8d28db6cf0274da4b482aad9a0df83a07b8a9abf4440e7958be421a4a0c0325` |

## Wycheproof (fetched, not committed)

Wycheproof (github.com/C2SP/wycheproof) is Apache-2.0, so none of its files
are in the repository. Each test fetches the files it needs from commit
`d17cc89fa5c8f4f4d378b5385ebf75da5d0926c9`,
`https://raw.githubusercontent.com/C2SP/wycheproof/<commit>/testvectors_v1/<name>_test.json`,
over Core's own HTTPS client. It checks the SHA-256 pinned in
`vec_wycheproof_pin` and caches the file under
`$XDG_CACHE_HOME/jet/vectors/wycheproof/<commit>/` (`~/.cache` when
`XDG_CACHE_HOME` is unset). A cached file with another digest is fetched
again; a fetched file with another digest fails the test. The pinned files:

- AEAD: `aes_gcm` (128- and 256-bit keys, 96-bit IVs, 128-bit tags) and
  `chacha20_poly1305` (96-bit nonces).
- Key agreement: `x25519`, `ecdh_secp256r1_ecpoint` and `ecdh_secp384r1_ecpoint`.
- ECDSA: `ecdsa_secp256r1_sha256` and `ecdsa_secp384r1_sha384`.
- RSA PKCS#1 v1.5 verification: `rsa_signature_{2048,3072,4096}_sha{256,384}` and
  `rsa_signature_{2048,4096}_sha512`.
- RSA PKCS#1 v1.5 signing: `rsa_pkcs1_{2048,3072,4096}_sig_gen` (the SHA-256,
  SHA-384 and SHA-512 groups).
- RSASSA-PSS: `rsa_pss_{2048,3072,4096}_sha256_mgf1_32`,
  `rsa_pss_{2048,4096}_sha384_mgf1_48` and `rsa_pss_4096_sha512_mgf1_64`.
- MAC and KDF: `hmac_sha256`, `hmac_sha384`, `hkdf_sha256`, `hkdf_sha384` and
  `pbkdf2_hmacsha256`.
- core.crypto.expert: `aes_gcm` (256-bit keys) and `x25519` again, through its
  own AES-256-GCM and X25519; `xchacha20_poly1305` (192-bit nonces); and
  `ed25519` (strict verification).

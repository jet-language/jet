# Vector sources

Only the vectors `run.jet` runs are kept. NIST CAVP vectors are works of the
United States government, not subject to copyright.

## NIST CAVP

From the byte-oriented response files of the Cryptographic Algorithm
Validation Program (csrc.nist.gov, Projects/Cryptographic-Algorithm-Validation-Program),
unchanged except where noted:

| Archive | Archive SHA-256 |
| --- | --- |
| `shs/shabytetestvectors.zip` | `929ef80b7b3418aca026643f6f248815913b60e01741a44bba9e118067f4c9b8` |
| `sha3/sha-3bytetestvectors.zip` | `cd07701af2e47f5cc889d642528b4bf11f8b6eb55797c7307a96828ed8d8fc8c` |
| `mac/hmactestvectors.zip` | `418c3837d38f249d6668146bd0090db24dd3c02d2e6797e3de33860a387ae4bd` |

| File | SHA-256 |
| --- | --- |
| `nist/SHA1ShortMsg.rsp` | `be0991ddc5372932d55804b11713c9140d10435ef4b316a0773e3506eec79cda` |
| `nist/SHA224ShortMsg.rsp` | `0dad6656c08f77252f6ccb789e42284fd61fc53bba30e83162800aa3d2aa939f` |
| `nist/SHA256ShortMsg.rsp` | `75e1cb83994638481808e225b9eb0c1ebd0c232d952ac42b61abce6363be283c` |
| `nist/SHA384ShortMsg.rsp` | `7ea7bcf00fadc20949fae63703e40681ddf288fea808471cb3cbc95f3ec16811` |
| `nist/SHA512ShortMsg.rsp` | `e53a36c03609e5a3e3cc4b6e117a499db7864c23ec825c6cec99503a45f40764` |
| `nist/SHA3_224ShortMsg.rsp` | `c2c57e8090b270a7ca154602803c8a30859e737ca65729701df02080dd6c75fd` |
| `nist/SHA3_256ShortMsg.rsp` | `e75b1ded16e9862eaaef9ad9b89402b9832f86fe818292634b2e54dc6b3540ff` |
| `nist/SHA3_384ShortMsg.rsp` | `0e15aa35474c85084a0a1682c28ae9d4ebde61daa130ea0b408f51a1e9ec3150` |
| `nist/SHA3_512ShortMsg.rsp` | `f73a39d8091d58c9225cfad8e6d8cfa3e8d67532c7029bf376baef0d0d86d5b6` |
| `nist/HMAC_L32.rsp` | `cabe908410bf48ee3a3c578450599f89789d81a4deecd67250c0b0da0b460487` |

`HMAC_L32.rsp` is lines 1-5 (the header) and 4735-6311 (the `[L=32]`,
HMAC-SHA-256, section) of `HMAC.rsp` (SHA-256
`cf27b6ad24fc3d72f8d18f8cee8a2b366302395cc50f5d9e607fbce4e98001eb`), byte for
byte; core.crypto exports HMAC with SHA-256 only.

## Not committed

Wycheproof and Mbed TLS suite data are not committed to the repository
(#6694). The Wycheproof HKDF-SHA-256 and PBKDF2-HMAC-SHA-256 vectors for
core.crypto's public KDFs run in Core/Net/SSL/TLS.jet's vector tests, which
fetch them at test time pinned by SHA-256 (tests/vectors/UPSTREAM.md).

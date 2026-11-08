# Vector sources

Only the vectors `run.jet` runs are kept. Each upstream license sits beside
its files.

## mbedtls 3.6.7

From `tests/suites/` of the mbedtls-3.6.7 release (license: Apache-2.0 OR
GPL-2.0-or-later, `mbedtls/LICENSE`). Each file keeps the test paragraphs of
the calls the public core.crypto functions cover, unchanged:

| File | Upstream SHA-256 | Kept calls |
| --- | --- | --- |
| `test_suite_shax.data` | `3315b759c0a51dfd869652dfdc9258c42c99fdd069fab94a02b9d4ace32cac1d` | `mbedtls_sha1`, `sha224`, `mbedtls_sha256`, `sha384`, `mbedtls_sha512`, `mbedtls_sha3`, `mbedtls_sha3_multi`, `sha3_reuse` |
| `test_suite_md.data` | `55afbc9cfad9ee102f7a603e60d78da1ab7a017d8c26695be092b2a337ff939f` | `md_hex`, `md_hex_multi`, `md_text`, `md_text_multi` for SHA-1, SHA-2 and SHA-3; `mbedtls_md_hmac`, `md_hmac_multi` for SHA-256 |
| `test_suite_hkdf.data` | `b085656f59a63228bc74ef0615d7ee32844b3acbffd06817a79a7fdeb8aae41e` | `test_hkdf` for SHA-256 |
| `test_suite_pkcs5.data` | `bb752d91fd85158e36e571dbabae4994dc83af130016cd08a95b66d981cd2d5f` | `pbkdf2_hmac` for SHA-256 |

## Wycheproof

From `testvectors_v1/` of github.com/C2SP/wycheproof at commit
`d17cc89fa5c8f4f4d378b5385ebf75da5d0926c9` (license: Apache-2.0,
`wycheproof/LICENSE`), unchanged:

| File | SHA-256 |
| --- | --- |
| `hmac_sha256_test.json` | `2d201cfa61d1bf95e6f5d07d96634b4a348b31e8eaa277ad7c8d09677b7a743f` |
| `hkdf_sha256_test.json` | `bb2b462a38b251cb52a2aede706d6d4b62b26864f4e80c95497507ddb07c5f1e` |
| `pbkdf2_hmacsha256_test.json` | `1bf37af2cefe40c829ee9ecebb3505bb6424be8824bc97aa3e1c2076e860d192` |

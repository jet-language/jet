# musl 1.2.5 math differences against correctly rounded results

Jet's `core.math` elementary functions (`Core/Math/Math.jet`) are correctly
rounded to nearest-even (CORE-MATH algorithms verified against MPFR) on every
tier (`O0`, `O1`, `O2`, and WebAssembly). Where musl 1.2.5's `libm` differs from
the correctly rounded IEEE 754-2019 / MPFR result, Jet matches the correctly
rounded result rather than reproducing musl's misroundings or domain bugs.

Executable evidence lives in the committed MPFR comparison corpora under
[`Tests/Cases/CoreMathNumeric/CoreMathCorrectlyRoundedFunctionsMatchMpfrOnEveryTier.fixture/corpus`](../../../Tests/Cases/CoreMathNumeric/CoreMathCorrectlyRoundedFunctionsMatchMpfrOnEveryTier.fixture/corpus/)
(`Float` / binary64) and
[`Tests/Cases/CoreMathNumeric/CoreMathCorrectlyRoundedF32FormsMatchMpfrOnEveryTier.fixture/corpus`](../../../Tests/Cases/CoreMathNumeric/CoreMathCorrectlyRoundedF32FormsMatchMpfrOnEveryTier.fixture/corpus/)
(`F32` / binary32). Each corpus row records the input bits, the MPFR correctly
rounded bits, and (prefixed with `~`) musl 1.2.5's differing bits.

## Documented musl 1.2.5 bugs and multi-ULP divergences

| Function | Width | Input (hex bits / value) | Correctly rounded (Jet / MPFR) | musl 1.2.5 result | Defect / error |
| --- | --- | --- | --- | --- | --- |
| `acosh` | binary64 | `0xc0db4c36d0db7fed` (`-27952.85649764532`) | `0x7ff8000000000000` (`NaN`) | `0xc014e7b7b9cc1f1c` (`-5.226286796...`) | Domain bug: `acosh` of large negative inputs returns a finite number instead of `NaN` (96 negative rows in `acosh.txt`, because musl checks the masked exponent before rejecting negative inputs). |
| `acosh` | binary64 | `0xc120273b10a6d4af` (`-529309.5325228...`) | `0x7ff8000000000000` (`NaN`) | `0x400cde3e6d71f866` (`3.608517510...`) | Same `acosh` negative-input domain bug returning a positive finite number instead of `NaN`. |
| `acosh` | binary64 | `0xc16838d4f45658b5` (`-12699303.6355...`) | `0x7ff8000000000000` (`NaN`) | `0x40280c2f1188807f` (`12.02379660...`) | Same `acosh` negative-input domain bug returning a finite number instead of `NaN`. |
| `lgamma` | binary64 | `0xc004000000000000` (`-2.5`) | `0xbfaccbf9f5ed0f16` | `0xbfaccbf9f5ed0f20` | 10 ULP rounding error near negative half-integers. |
| `tgamma` (`gamma`) | binary64 | `0xc000000000000001` (`-2.0000000000000004`) | `0xc30ffffffffffffc` | `0xc30ffffffffffff7` | 5 ULP rounding error near negative integer poles. |
| `exp10` | binary64 | `0xc02fb0000000000a` (`-15.843750000000018`) | `0x3ca000000000001b` | 4 ULP off | 4 ULP worst-case error in musl 1.2.5 `exp10` (`Core/Math/Math.jet` unit test). |
| `erfc` | binary64 | `0x3ff5db559fe5c0ba` | `0x3fab53cf571d328f` | `0x3fab53cf571d328c` | 3 ULP rounding error in the transition region. |
| `atan2` | binary64 | `y = 1.0`, `x = 10.0` | `0x3fb984316a5e1098` (`0.09966865249116202`) | `0x3fb984316a5e1099` (`0.09966865249116204`) | 1 ULP rounding error on an exact decimal pair. |

## Binary64 (`Float`) corpus summary

Every function below is verified across its committed corpus (`Tests/Cases/CoreMathNumeric/CoreMathCorrectlyRoundedFunctionsMatchMpfrOnEveryTier.fixture/corpus/<fn>.txt`). The representative row cites one input where musl 1.2.5 differs from the correctly rounded MPFR / Jet result:

| Function | Differing rows / total | Sample input (hex bits) | Correctly rounded (MPFR / Jet) | musl 1.2.5 |
| --- | --- | --- | --- | --- |
| `acos` | 537 / 7702 | `0x3fe90d094c374fd2` | `0x3fe57d8f36e4e0ab` | `0x3fe57d8f36e4e0ac` |
| `acosh` | 1384 / 7589 | `0xc0db4c36d0db7fed` | `0x7ff8000000000000` | `0xc014e7b7b9cc1f1c` |
| `asin` | 280 / 8331 | `0x3eaba84aadd7cdc6` | `0x3eaba84aadd7d138` | `0x3eaba84aadd7d137` |
| `asinh` | 1500 / 7586 | `0x3ff8000000000000` | `0x3ff31dc0090b63d8` | `0x3ff31dc0090b63d9` |
| `atan` | 2832 / 13209 | `0x400921fb54442d18` | `0x3ff433b8a322ddd3` | `0x3ff433b8a322ddd2` |
| `atan2` | 1237 / 10540 | `0x3ff0000000000000`, `0x401c000000000000` | `0x3fc229aec47638dd` | `0x3fc229aec47638dc` |
| `atanh` | 1285 / 7575 | `0x3fe0000000000000` | `0x3fe193ea7aad030b` | `0x3fe193ea7aad030a` |
| `cbrt` | 319 / 7590 | `0x4004000000000000` | `0x3ff5b7209557b0ee` | `0x3ff5b7209557b0ed` |
| `cos` | 1591 / 18462 | `0x3ef2813ee0832006` | `0x3fefffffffea991e` | `0x3fefffffffea991f` |
| `cosh` | 1186 / 8163 | `0x401c000000000000` | `0x4081228949ba3a8c` | `0x4081228949ba3a8b` |
| `erf` | 1160 / 7568 | `0x3fe8000000000000` | `0x3fe6c1c9759d0e5f` | `0x3fe6c1c9759d0e60` |
| `erfc` | 1018 / 7569 | `0x3ff5db559fe5c0ba` | `0x3fab53cf571d328f` | `0x3fab53cf571d328c` |
| `exp` | 1208 / 12416 | `0xbf90a8cb17028063` | `0x3fef7bcdb03a7527` | `0x3fef7bcdb03a7528` |
| `exp10` | 1282 / 8602 | `0x3fe0000000000000` | `0x40094c583ada5b53` | `0x40094c583ada5b52` |
| `exp2` | 874 / 7603 | `0x4080202ff1388649` | `0x6030430115918f99` | `0x6030430115918f98` |
| `expm1` (`exp_m1`) | 1016 / 7808 | `0x3ff0000000000000` | `0x3ffb7e151628aed3` | `0x3ffb7e151628aed2` |
| `hypot` | 1012 / 10603 | `0x3ff8000000000000`, `0x3fb999999999999a` | `0x3ff80da360d9e626` | `0x3ff80da360d9e625` |
| `lgamma` | 2014 / 7733 | `0xc004000000000000` | `0xbfaccbf9f5ed0f16` | `0xbfaccbf9f5ed0f20` |
| `log` (`ln`) | 987 / 8748 | `0x401e56c8f99c683e` | `0x40003588c70fba6b` | `0x40003588c70fba6a` |
| `log10` | 1021 / 7674 | `0x02433f31a0be9a29` | `0xc0729094f488c359` | `0xc0729094f488c35a` |
| `log1p` (`ln_1p`) | 857 / 7803 | `0x4000000000000000` | `0x3ff193ea7aad030b` | `0x3ff193ea7aad030a` |
| `log2` | 941 / 7691 | `0x3ff9091e8671fb08` | `0x3fe4ab4c471ac03c` | `0x3fe4ab4c471ac03b` |
| `pow` | 2586 / 20656 | `0x7fefffffffffffff`, `0x3fe0000000000000` | `0x5fefffffffffffff` | `0x5ff0000000000000` |
| `sin` | 1392 / 18141 | `0x4004000000000000` | `0x3fe326af0dcfcab1` | `0x3fe326af0dcfcab0` |
| `sinh` | 870 / 8042 | `0x400921fb54442d18` | `0x402718f45d72e671` | `0x402718f45d72e672` |
| `tan` | 2325 / 13325 | `0x401c000000000000` | `0x3febe2e6e13eea79` | `0x3febe2e6e13eea78` |
| `tanh` | 1869 / 7637 | `0x3f6000b47bdb383f` | `0x3f6000b32658e41f` | `0x3f6000b32658e420` |
| `tgamma` (`gamma`) | 2098 / 7705 | `0xc000000000000001` | `0xc30ffffffffffffc` | `0xc30ffffffffffff7` |

## Binary32 (`F32`) corpus summary

The binary32 corpus (`Tests/Cases/CoreMathNumeric/CoreMathCorrectlyRoundedF32FormsMatchMpfrOnEveryTier.fixture/corpus/<fn>.txt`) stores each exact binary32 operand and result widened losslessly to its binary64 hex representation:

| Function | Differing rows / total | Sample input (widened hex) | Correctly rounded (MPFR / Jet) | musl 1.2.5 (`*f`) |
| --- | --- | --- | --- | --- |
| `acos` | 965 / 6099 | `0x0000000000000000` | `0x3ff921fb60000000` | `0x3ff921fb40000000` |
| `acosh` | 259 / 6070 | `0x3ff8000000000000` | `0x3feecc2ca0000000` | `0x3feecc2cc0000000` |
| `asin` | 124 / 6120 | `0xbfc93730a0000000` | `0xbfc961b040000000` | `0xbfc961b020000000` |
| `asinh` | 1021 / 6073 | `0x3fe0000000000000` | `0x3fdecc2ca0000000` | `0x3fdecc2cc0000000` |
| `atan` | 972 / 6105 | `0x46293e5940000000` | `0x3ff921fb60000000` | `0x3ff921fb40000000` |
| `atan2` | 272 / 9491 | `0x3ff0000000000000`, `0x4004000000000000` | `0x3fd85a3760000000` | `0x3fd85a3780000000` |
| `atanh` | 432 / 6070 | `0x3fb99999a0000000` | `0x3fb9af93e0000000` | `0x3fb9af93c0000000` |
| `cos` | 5 / 6105 | `0x3fd12f2820000000` | `0x3feeda7900000000` | `0x3feeda7920000000` |
| `cosh` | 473 / 6115 | `0x3ff0000000000000` | `0x3ff8b07560000000` | `0x3ff8b07540000000` |
| `erf` | 195 / 6065 | `0x3810000000000000` | `0x38120dd760000000` | `0x38120dd740000000` |
| `erfc` | 506 / 6064 | `0xbfe0000000000000` | `0x3ff853f7a0000000` | `0x3ff853f7c0000000` |
| `exp` | 4 / 6137 | `0xc0165ef860000000` | `0x3f6e837440000000` | `0x3f6e837460000000` |
| `exp10` | 1103 / 6063 | `0xbff8000000000000` | `0x3fa030dc40000000` | `0x3fa030dc60000000` |
| `expm1` (`exp_m1`) | 316 / 6063 | `0x3ff0000000000000` | `0x3ffb7e1520000000` | `0x3ffb7e1500000000` |
| `hypot` | 1145 / 10866 | `0x3fb99999a0000000`, `0x4018000000000000` | `0x401800da80000000` | `0x401800da60000000` |
| `lgamma` | 1865 / 6069 | `0x3ff8000000000000` | `0xbfbeeb95c0000000` | `0xbfbeeb95a0000000` |
| `log` (`ln`) | 16 / 6131 | `0x3feb3c19a0000000` | `0xbfc4a39080000000` | `0xbfc4a39060000000` |
| `log10` | 137 / 6119 | `0x4024923260000000` | `0x3ff0321520000000` | `0x3ff0321500000000` |
| `log1p` (`ln_1p`) | 206 / 6076 | `0x4004000000000000` | `0x3ff40b5120000000` | `0x3ff40b5140000000` |
| `log2` | 6 / 6064 | `0x400cebe720000000` | `0x3ffdaa45a0000000` | `0x3ffdaa4580000000` |
| `pow` | 1414 / 12699 | `0x4012000000000000`, `0xbff8000000000000` | `0x3fbad15360000000` | `0x3fbad15380000000` |
| `sin` | 2 / 6101 | `0xc015d6aa40000000` | `0x3fe7795b80000000` | `0x3fe7795b60000000` |
| `sinh` | 1010 / 6119 | `0x4004000000000000` | `0x40183368c0000000` | `0x40183368e0000000` |
| `tan` | 703 / 6097 | `0x4004000000000000` | `0xbfe7e79b40000000` | `0xbfe7e79b60000000` |
| `tanh` | 748 / 6117 | `0x3fe0000000000000` | `0x3fdd9353e0000000` | `0x3fdd9353c0000000` |
| `tgamma` (`gamma`) | 1 / 6070 | `0x3cfc26d160000000` | `0x42e22fedc0000000` | `0x42e22fede0000000` |

Note: `cbrtf`, `exp2f`, and `sqrtf` have 0 differing rows against MPFR on the
committed binary32 corpus. The C23 half-turn (`*_pi`), `exp2_m1`, `exp10_m1`,
`log2_1p`, `log10_1p`, `pow_n`, `pow_r`, `root_n`, `compound_n`, and `rsqrt`
functions are not provided by musl 1.2.5 and are implemented directly from
CORE-MATH in [`Core/Math/Math.jet`](../../../Core/Math/Math.jet).

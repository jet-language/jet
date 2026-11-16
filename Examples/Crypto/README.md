# Crypto

Hashing, signing, sealed envelopes, keys and authentication.

Run a lesson from the repository root, for example `jet run Examples/Crypto/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [AuthSessions](AuthSessions.jet)
- [CryptoEnvelope](CryptoEnvelope.jet)
- [CryptoErrorRun](CryptoErrorRun.jet) — `CryptoError` and `FileCryptoError` belong to the Core error family, so a plain `run: fn()` passes a crypto failure up without any local handling.
- [CryptoMigration](CryptoMigration.jet)
- [CryptoSign](CryptoSign.jet)
- [CryptoSuite](CryptoSuite.jet)
- [ExpertProjection](ExpertProjection.jet)
- [Hash](Hash.jet)
- [PasswordAuth](PasswordAuth.jet)
- [RandomAPISplit](RandomAPISplit.jet)
- [TypedCrypto](TypedCrypto.jet)
- [VaultKeys](VaultKeys.jet)
- [VaultKeyWrap](VaultKeyWrap.jet) — Portable vault backups remain type-bound and import through the same prepare-authorize-commit authority flow as local mutation.

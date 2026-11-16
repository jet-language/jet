# Security family synthesis

## Family verdict

Security ecosystems win by preserving the boundary around a high-risk action: target scope before traffic, typed bytes before parsing, provider proof before identity, authorization context before access, and evidence identity before a result is trusted. The six-domain security census contains 184 P0 rows across 6 finished domains. Jet already supplies a meaningful base—opaque crypto roles, taint and authority facts, typed network/HTTP, bounded processes, structured inspection, exact bytes/numbers, and deterministic fuzz/replay law—but the domain products remain mostly gaps. The family should build shared evidence and capability seams first, then add domain packages behind explicit owner gates.

M-VALIDATION-RECEIPTS is the canonical security ledger choice. It joins taint, authority, proof, fuzz, replay, and benchmark receipts. M-EVIDENCE remains a reused inspector/projection mechanism, not a second security ledger. M-NETWORKING, M-PACKAGING, and M-STORAGE are reused where their existing generic boundaries already match; no duplicate IDs were minted for them.

## Mechanisms

| ID | Mechanism | Domains | Gate kind | Jet state | ID status |
|---|---|---:|---|---|---|
| `M-EVIDENCE` | Inspection, diagnostics, and reproducible evidence | 2 | none | mixed | M-EVIDENCE |
| `M-NETWORKING` | Typed network and service protocol boundaries | 2 | none | mixed | M-NETWORKING |
| `M-PACKAGING` | Package, dependency, and native-provider boundary | 1 | stdlib-dependency | mixed | M-PACKAGING |
| `M-STORAGE` | Durable scientific stores and caches | 2 | stdlib-dependency | mixed | M-STORAGE |
| `M-VALIDATION-RECEIPTS` | Cross-tier validation, reproducibility, and evidence receipts | 6 | invariant | mixed | M-VALIDATION-RECEIPTS |
| `M-PACKET-IO` | Authorized packet, proxy, capture, and scan I/O | 1 | public-api | real-gap | new |
| `M-FINDINGS-RULES` | Portable security rules, dataflow findings, and SARIF publication | 4 | public-api | real-gap | new |
| `M-BINARY-LOADER` | Executable-format loader and address-aware binary model | 1 | public-api | real-gap | new |
| `M-BINARY-EXECUTION` | Bounded native disassembly, CFG, emulation, and symbolic execution | 1 | public-api | real-gap | new |
| `M-EVIDENCE-CHAIN` | Evidence-source, collection, case, and chain-of-custody model | 1 | public-api | real-gap | new |
| `M-SAMPLE-SAFETY` | Malware and dangerous-sample capability boundary | 2 | invariant | real-gap | new |
| `M-IDENTITY-PROVIDER` | Provider-bound identity, metadata, JWKS, OIDC, SAML, and passkeys | 1 | public-api | mixed | new |
| `M-AUTHZ-RELATIONS` | Relationship-based authorization with explainable consistency | 1 | public-api | real-gap | new |
| `M-CHAIN-ABI` | Typed chain ABI/RPC codecs, transaction lifecycle, and custody signing | 1 | public-api | real-gap | new |
| `M-CRYPTO-INTEROP` | Standards crypto interop, algorithm agility, and backend assurance | 1 | public-api | mixed | new |

The mechanism records keep P0 feature names and needs from the six source census files. Domain-only concerns remain in domains.json with a null mechanism_id. Existing family IDs are explicitly marked in mechanisms.json through existing_mechanism_id; new security IDs are M-PACKET-IO, M-FINDINGS-RULES, M-BINARY-LOADER, M-BINARY-EXECUTION, M-EVIDENCE-CHAIN, M-SAMPLE-SAFETY, M-IDENTITY-PROVIDER, M-AUTHZ-RELATIONS, M-CHAIN-ABI, and M-CRYPTO-INTEROP.

## Domain matrix

| Domain | Benchmark peer | P0 rows | Gauntlet coverage | Biggest beat vector |
|---|---|---:|---|---|
| `security-tooling` | Nmap 7.x and Masscan for authorized network scanning; Semgrep and CodeQL for 1,000,000-line static analysis | 37 | none | Typed policy at the effect boundary can make ordinary safe paths harder to misuse than ambient Python/Ruby/C security scripts, but raw scanner and packet capabilities are not shipped. |
| `reverse-engineering` | Capstone for bulk decode; Ghidra/Binary Ninja for analysis; angr/Unicorn for CFG/emulation; YARA for signature scans | 29 | formats.binary-kernel | One typed evidence ledger can bind sample bytes, loader options, analysis facts, authority, findings, and replay identity instead of separate project, script, trace, and scan artifacts. |
| `cryptography-engineering` | BoringSSL crypto/* with OpenSSL 3.5 speed harness | 24 | none | A typed safe envelope hides nonce and algorithm footguns while retaining an explicit expert rung. |
| `digital-forensics` | Volatility 3, The Sleuth Kit, osqueryd, and Velociraptor | 31 | none | An evidence-carrying chain can bind source, transform, output, hash, signature, authority, and verification more tightly than loose collection files. |
| `identity-auth` | SpiceDB authorization checks; OpenFGA, Ory, Keycloak, and WebAuthn providers for secondary flows | 33 | none (gauntlet/measurement-manifest.json has no identity-auth, SpiceDB, OpenFGA, OIDC, or passkey cell) | One typed security ledger can join credential taint, authority, proof, fuzz, provider evidence, and tier traces. |
| `blockchain-web3` | Alloy EVM toolkit; Solana runtime; Noir/Barretenberg and Halo2 provers | 30 | none — gauntlet/matrix.json and gauntlet/measurement-manifest.json contain no blockchain, EVM, Solana, or ZK cell | Typed wire and custody facts can make ABI, RPC, signing, receipt, and authority evidence one compiler-owned ledger. |

Performance objects are copied from each domain's performance.json. No security-specific gauntlet cell establishes a Jet win. Existing reverse-engineering formats.binary-kernel is only a deterministic record parser; it does not cover disassembly, decompilation, emulation, or YARA. Every other security performance record reports none.

## Cross-family shared rows

| Canonical domain id | Shared row(s) | Family mechanism reference | Duplication rule |
|---|---|---|---|
| fintech-payments | Account-access consent resources; Open Banking account/balance/transaction resources; Open Banking payment consents/payment initiation; FAPI advanced authorization controls; idempotency/status/pagination/rate limits | M-IDENTITY-PROVIDER for FAPI/provider proof; M-NETWORKING for transport; M-EVIDENCE-CHAIN for consent/status evidence | Canonical fintech rows remain in fintech-payments; security cites the domain id and feature only. |
| networking-protocols | Typed TCP/UDP/Unix sockets and options; Packet capture and replay; XDP/AF_XDP; DPDK burst buffers and queues | M-NETWORKING for ordinary typed sockets; M-PACKET-IO for raw/capture/privileged packet paths | Canonical networking rows remain in networking-protocols; security does not copy them. |
| wasm-plugins-sandboxing | Zero-import application sandbox; Wire and parameter byte caps; Selective WASI filesystem/network/clock/random imports | M-SAMPLE-SAFETY for dangerous-sample capability policy; M-VALIDATION-RECEIPTS for limits/evidence | Canonical sandbox rows remain in wasm-plugins-sandboxing; security cites the domain id and feature only. |
| package-registries-supply-chain | deterministic SPDX SBOM; CycloneDX SBOM; current CycloneDX schema interoperability; rich SBOM identity fields; lock-backed provenance inspector; package signing and TOFU pinning; registry trust configuration; SLSA provenance verification; Sigstore keyless verification | M-PACKAGING for package/SBOM/provider boundary; M-VALIDATION-RECEIPTS for verifiable receipts | Canonical package rows remain in package-registries-supply-chain; security cites the domain id and exact feature names only. |

## Owner gates

| Suggested decision | Gate | Affected mechanisms/domains |
|---|---|---|
| `D-SEC-LEDGER1` | Use M-VALIDATION-RECEIPTS as the canonical security evidence ledger joining taint, authority, proof, fuzz, replay, and benchmark receipts; retain M-EVIDENCE only for inspector projections. | M-VALIDATION-RECEIPTS, M-EVIDENCE; all six domains |
| `D-SEC-PACKET1` | Choose authorized proxy, raw packet, pcap, scan, rate, retry, target-scope, and resumable-artifact boundaries. | M-PACKET-IO; security-tooling |
| `D-SEC-FINDINGS1` | Choose rule language, taint/dataflow scope, detector taxonomy, SARIF/JSON output, suppression, conversion loss, and query-pack policy. | M-FINDINGS-RULES; security-tooling, reverse-engineering, digital-forensics, blockchain-web3 |
| `D-SEC-BINARY1` | Choose executable formats, address/relocation model, binary database, IL, disassembly, CFG, decompiler, emulation, and symbolic limits. | M-BINARY-LOADER, M-BINARY-EXECUTION; reverse-engineering |
| `D-SEC-SAMPLE1` | Set malware/dangerous-sample quarantine, target identity, privilege, network, time, memory, disk, callback, and teardown policy; align with WASI sandbox capability imports. | M-SAMPLE-SAFETY; reverse-engineering, security-tooling; wasm-plugins-sandboxing cross-reference |
| `D-SEC-FORENSICS1` | Choose endpoint/image/case boundaries, evidence chain, collection/ingest lifecycle, FIM, timeline, report formats, and explicit loss states. | M-EVIDENCE-CHAIN; digital-forensics |
| `D-SEC-IDENTITY1` | Approve provider-bound OIDC discovery/JWKS/PKCE/nonce/browser proof, passkeys, MFA, SAML, and account-linking policy before changing fail-closed OAuth. | M-IDENTITY-PROVIDER; identity-auth; fintech-payments FAPI cross-reference |
| `D-SEC-AUTHZ1` | Choose Core versus external ReBAC, relationship schema, computed permissions, caveats, consistency/freshness, and audit semantics. | M-AUTHZ-RELATIONS; identity-auth |
| `D-SEC-CHAIN1` | Choose typed ABI/RPC, chain host/fork, custody/signing, account constraints, transaction/receipt, and provider dependency boundaries. | M-CHAIN-ABI; blockchain-web3 |
| `D-SEC-CRYPTO1` | Choose PEM/PKCS#8, AEAD/detached/in-place, algorithm agility, PQ, backend/FIPS/no_std, target proof, and KAT interoperability scope. | M-CRYPTO-INTEROP; cryptography-engineering |
| `D-SEC-GAUNTLET1` | Add matched security, binary, crypto, forensic, identity, and chain workloads with correctness first, then timing, memory, resource, and tier metrics. | M-VALIDATION-RECEIPTS; all six domains |

## Contradictions and synthesis resolutions

| Topic | Source 1 | Source 2 | Resolution |
|---|---|---|---|
| Crypto/auth example health | cryptography-engineering/probes/crypto_suite.receipt.txt records ten E0405 errors; identity-auth/probes/jet-auth-checks.txt records passing conformance fixtures but E0405 in illustrative auth examples; blockchain-web3/probes/crypto_sign.receipt.txt records three E0405 errors. | The same domains' reports describe core.crypto/core.auth primitives as present and identity conformance as passing. | Treat E0405 as stale example return-contract drift, not missing primitives. Keep DEF-SEC-CRYPTO-EXAMPLE-E0405-001 open until examples are repaired and rerun. |
| Proof command versus proof evidence | security-tooling and blockchain-web3 help probes show jet prove exists; cryptography-engineering guarantees probe is successful only at explicit-file scope. | proof-formal/probes/basic-proof.txt and distributed-systems/probes/prove.out both fail with E2105 before proof production. | Help and guarantees are not producer proof. Record E2105 as a shared family defect and keep proof/replay rows ratified-in-progress or unverified. |
| Generic evidence versus forensic chain | security-tooling and identity-auth reports call the existing inspect/authority/taint substrate a strong ledger; digital-forensics/report.md says generic files, codecs, tables, and watchers are not collection, case, FIM, or timeline. | digital-forensics P0 rows require endpoint identity, image layers, case lifecycle, and loss-aware reports. | Reuse the generic ledger as substrate, but promote M-EVIDENCE-CHAIN only for source/case/chain semantics. |
| Typed network versus packet capability | security-tooling and blockchain-web3 reports mark typed sockets/HTTP transport as shipped foundations. | security-tooling Scapy/Nmap/Masscan rows and networking-protocols packet rows require raw frames, pcap, scan classification, rate/loss controls, XDP/DPDK, or QUIC. | Reuse M-NETWORKING for ordinary transport; keep M-PACKET-IO owner-gated and do not claim packet/scanner parity from sockets. |
| SBOM/provenance completeness | security-tooling reports trust, signatures, provenance, and SBOM inspection as shipped; package-registries-supply-chain/census.json records deterministic SPDX and CycloneDX rows, a lock-backed inspector, signing, and registry trust as already implemented, while current CycloneDX schema interoperability, rich identity fields, and SLSA/Sigstore verification remain gates or gaps. | package-registries-supply-chain/probes/provenance-json.txt reports integrity enforced but transparency, publisher, and build fields not recorded. | Count the native projections and integrity as existing substrate, but keep standard-schema, rich-identity, and external-attestation limitations explicit. |
| Binary benchmark coverage | reverse-engineering/performance.json names formats.binary-kernel as related coverage. | reverse-engineering/report.md:121-134 says that cell only parses deterministic records and FNV-1a, with no 10 MB binary analysis run. | Keep reverse-engineering gauntlet coverage as none for claims; add a dedicated 10 MB cell. |

## Defects

See defects.json for the recurring E0405 example drift, shared E2105 prove identity-hash failure, reverse E2101 binary command gap, and digital-forensics E1001 missing module. E0510 expert crypto and E1001/E2101 negative diagnostics are not silently reclassified as successes; expected negative checks remain evidence of their boundaries.

## Traceability notes

All P0 counts, features, and needs in domains.json and mechanisms.json are generated directly from the six finished census.json files. Performance values and gauntlet fields are copied directly from the six performance.json files. The domain reports supply verdict, persona, beat-vector, avoid-list, and owner-gate context. Cross-family references intentionally cite canonical domain ids and feature names without copying their rows. No Tower writes, builds, formatters, or project-wide tests were run.

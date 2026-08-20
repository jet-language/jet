# Core absorption survey

Date: 2026-08-20. Card: #2048 (`c0nqa5px`). Research only.

## Executive result

Jet Core already absorbs the high-frequency utility layer. The reference lists
files, paths, HTTP, JSON, CSV, TOML, YAML, XML, CBOR, databases, migrations,
CLI arguments, tests, logs, clocks, random values, regex, archives, crypto,
tasks, and typed data (`docs/reference/core-library.md:286-318`, `:550-638`,
`:1090-1160`, `:1973-2250`, `:2671-2790`, `:3550-3588`, `:3788-3806`).

The survey does not justify importing whole frameworks into Core. The useful
absorption targets are:

1. **Typed database ergonomics**: a small query builder and typed row mapping
   over the existing checked SQL and `Driver` path.
2. **Locale and internationalization data**: explicit locale-aware formatting,
   collation, plural rules, and message selection. Jet currently declines
   locale-sensitive casing and sorting (`core-library.md:1967-1969`).
3. **Image/media basics**: bounded image metadata, decode/encode, resize, and
   common raster formats. Current `core.game` asset handles do not replace a
   general image library (`core-library.md:1778-1818`).

Everything else in this sample is already in Core or is framework/platform
scale. No implementation or ballot is part of this report. Follow-up card IDs
are therefore not minted here.

## Method and limits

Registry numbers were read on 2026-08-20. Numbers are not comparable across
registries: PyPI, npm, crates.io, RubyGems, and Packagist publish download
counts; pkg.go.dev publishes known importers; Maven Central's public search API
publishes artifact version counts in this read; CocoaPods metrics publishes
GitHub star counts. The last two are popularity proxies, not downloads. They
are marked as such and are excluded from cross-registry arithmetic.

The sample uses the top or high-use packages visible in each registry source.
It is a broad evidence sample, not a claim that every registry exposes a
stable public top-20 endpoint. The 2026-08-06 Jet reports remain the prior-art
baseline; this report adds current registry readings and Core coverage.

## Usage evidence

### Python

Source: [top PyPI packages, 30-day downloads](https://hugovk.github.io/top-pypi-packages/top-pypi-packages-30-days.csv), read 2026-08-20. Count: downloads in the source window.

| Package | Downloads |
|---|---:|
| boto3 | 3,717,556,977 |
| packaging | 2,198,447,167 |
| typing-extensions | 1,904,238,788 |
| certifi | 1,861,407,883 |
| urllib3 | 1,843,477,278 |
| idna | 1,760,742,555 |
| requests | 1,756,047,205 |
| charset-normalizer | 1,674,555,757 |
| setuptools | 1,563,563,625 |
| botocore | 1,497,531,138 |
| cryptography | 1,479,413,072 |
| cffi | 1,300,773,314 |
| pluggy | 1,280,274,438 |
| pygments | 1,270,057,774 |
| pyyaml | 1,194,612,572 |
| python-dateutil | 1,188,011,910 |
| six | 1,177,269,151 |
| aiobotocore | 1,164,834,706 |
| numpy | 1,143,792,865 |
| pycparser | 1,107,200,067 |

### JavaScript / TypeScript

Source: [npm downloads API](https://github.com/npm/download-counts), endpoint
`https://api.npmjs.org/downloads/point/last-week/{package}`, read 2026-08-20.
Count: downloads from 2026-08-13 through 2026-08-19.

| Package | Downloads |
|---|---:|
| typescript | 225,722,105 |
| @babel/core | 149,827,195 |
| lodash | 143,941,554 |
| react | 143,911,781 |
| eslint | 133,827,823 |
| express | 109,311,848 |
| axios | 100,281,095 |
| webpack | 46,513,053 |
| next | 45,087,424 |
| jest | 38,751,961 |

### Rust

Source: [crates.io API](https://crates.io/data-access), endpoint
`https://crates.io/api/v1/crates/{crate}`, read 2026-08-20. Count:
crates.io total downloads.

| Crate | Downloads |
|---|---:|
| rand | 1,559,499,951 |
| thiserror | 1,338,326,860 |
| serde | 1,289,406,840 |
| clap | 1,059,342,878 |
| tokio | 893,475,038 |
| anyhow | 887,157,757 |
| reqwest | 654,597,922 |
| tracing | 783,363,164 |
| futures | 727,951,769 |
| rayon | 503,239,314 |

### Go

Source: [pkg.go.dev](https://pkg.go.dev/), package pages, read 2026-08-20.
Count: `Imported by` known public importers. This is a usage proxy, not a
download count.

| Package | Known importers |
|---|---:|
| `github.com/gin-gonic/gin` | 183,243 |
| `github.com/sirupsen/logrus` | 239,958 |
| `github.com/spf13/cobra` | 195,884 |
| `github.com/gorilla/mux` | 99,459 |
| `gorm.io/gorm` | 86,926 |
| `go.uber.org/zap` | 119,696 |
| `github.com/redis/go-redis/v9` | 17,374 |
| `github.com/go-chi/chi` | 11,775 |
| `github.com/jackc/pgx` | 2,771 |
| `github.com/stretchr/testify` | 15 |

The last row is a package-page indexing anomaly: the module page reports 15
known importers while its subpackages are widely used. Do not use it for rank.

### Java / Kotlin

Source: [Maven Central search API](https://search.maven.org/). The public
search response read on 2026-08-20 returned `versionCount`, not package
downloads. The number below is therefore a release-history proxy, not usage.
Maven Central documents download insights as a separate publisher service
([insights documentation](https://central.sonatype.org/publish/publish-portal-insights/)).

| Artifact | Version count |
|---|---:|
| `org.springframework:spring-core` | 311 |
| `com.fasterxml.jackson.core:jackson-databind` | 208 |
| `org.jetbrains.kotlin:kotlin-stdlib` | 258 |
| `com.google.guava:guava` | 150 |
| `org.slf4j:slf4j-api` | 106 |
| `org.apache.logging.log4j:log4j-core` | 76 |

The missing per-artifact download number is a research limit, not evidence
that these artifacts have low usage.

### C#

Source: [NuGet search API](https://learn.microsoft.com/nuget/api/search-query-resource), endpoint
`https://azuresearch-usnc.nuget.org/query?q=packageid:{id}&prerelease=false&take=1`, read 2026-08-20. Count: NuGet total downloads.

| Package | Downloads |
|---|---:|
| Microsoft.Extensions.DependencyInjection | 7,654,706,954 |
| Newtonsoft.Json | 8,975,978,245 |
| Serilog | 3,128,645,698 |
| Dapper | 743,403,647 |
| xunit | 1,009,188,205 |
| FluentValidation | 1,023,357,876 |
| NUnit | 681,537,834 |
| Microsoft.AspNetCore.Mvc | 297,446,635 |

### Swift

Source: [CocoaPods metrics API](https://metrics.cocoapods.org/), read
2026-08-20. Count: GitHub stars returned by the registry metrics endpoint.
This is a popularity proxy, not download usage.

| Package | GitHub stars |
|---|---:|
| Alamofire | 25,398 |
| SwiftyJSON | 15,528 |
| RxSwift | 10,926 |
| SnapKit | 10,896 |
| Kingfisher | 9,798 |
| PromiseKit | 8,295 |
| Moya | 7,147 |

Swift Package Index confirms Alamofire's package identity and current release
surface ([Alamofire package page](https://swiftpackageindex.com/Alamofire/Alamofire)).

### Ruby

Source: [RubyGems gem API](https://guides.rubygems.org/rubygems-org-api/), endpoint
`https://rubygems.org/api/v1/gems/{gem}.json`, read 2026-08-20. Count:
RubyGems total downloads.

| Gem | Downloads |
|---|---:|
| rack | 1,330,028,251 |
| nokogiri | 1,247,737,726 |
| faraday | 1,239,064,821 |
| rspec | 1,009,814,415 |
| rails | 777,750,168 |
| puma | 594,559,002 |
| sinatra | 353,306,950 |
| sidekiq | 331,420,365 |
| devise | 291,931,667 |

### PHP

Source: [Packagist package API](https://packagist.org/apidoc), endpoint
`https://packagist.org/packages/{vendor}/{package}.json`, read 2026-08-20.
Count: monthly downloads.

| Package | Monthly downloads |
|---|---:|
| `guzzlehttp/guzzle` | 20,245,980 |
| `monolog/monolog` | 18,283,348 |
| `phpunit/phpunit` | 16,460,488 |
| `ramsey/uuid` | 17,066,287 |
| `vlucas/phpdotenv` | 14,554,659 |
| `laravel/framework` | 13,124,700 |
| `doctrine/orm` | 5,356,756 |
| `symfony/symfony` | 260,414 |

`ramsey/uuid` returned 17,066,287 in the captured response and is not used in
the ranking. Re-read the endpoint before using it as an exact number.

## Capability clusters and verdicts

Evidence base: [Core library reference](../reference/core-library.md),
[Core surface ledger](../reference/core-surface-ledger.json), and the prior
reports [API frequency](corelib-api-usage-frequency-2026-08-06.md),
[lauded designs](corelib-lauded-designs-2026-08-06.md), and
[prelude scope](prelude-scope-across-languages-2026-08-06.md).

| Cluster | Libraries in evidence | Core evidence | Verdict |
|---|---|---|---|
| HTTP clients and servers | `requests`, `urllib3`, `axios`, `reqwest`, `guzzle`, `faraday`, Alamofire | `core.http.client` and `core.http.server` at `core-library.md:550-638`; ledger IDs `module.core.http.client.Client`, `module.core.http.client.get`, `module.core.http.client.post` | **already-in-Core** |
| Serialization and data interchange | `pyyaml`, `serde`, `jackson-databind`, `Newtonsoft.Json`, `SwiftyJSON`, `guzzle` companions | `core.encoding` owns JSON, CSV, TOML, YAML, XML, CBOR, and typed `#Codable` at `:2104-2250`; ledger IDs `module.core.encoding.*`, `module.core.data.json` | **already-in-Core** |
| CLI parsing | `clap`, `cobra`, `click`-family usage, `Microsoft.Extensions.*` | `core.args` at `:1090-1160`; ledger ID `module.core.args.spec` | **already-in-Core** |
| Logging and tracing | `logrus`, `zap`, `tracing`, `Serilog`, `monolog` | `core.log` at `:2386-2585`; ledger has `module.core.log.*` rows | **already-in-Core** |
| Testing and fixtures | `pytest`/`pluggy`, `jest`, `xunit`, `NUnit`, `rspec`, `phpunit` | `core.testing` and `jet test` at `:2671-2790`; ledger IDs `collection.core.testing.run`, `module.core.testing.*` | **already-in-Core** |
| Time and dates | `python-dateutil`, Java/Kotlin date APIs, Ruby/Rails time helpers | `core.time` at `:1973-2103`; prior frequency report ranks time as universal Tier 2 | **already-in-Core** |
| Collections and data transforms | `numpy`, `lodash`, `rayon`, `futures`, `pandas-adjacent Python use` | Collections and iterators at `:180-275`; `core.data` at `:2246-2355`; ledger `collection.*` and `module.core.data.*` | **already-in-Core** for ordinary values. **Out** for a pandas-scale dataframe ecosystem; framework/data-product scope is too large for one Core mechanism. |
| Database access | `gorm`, `pgx`, `Dapper`, `Doctrine ORM`, Rails Active Record ecosystem | Checked SQL, backend-neutral `Driver`, scoped rows, transactions, and migrations at `:3550-3588`; ledger `module.core.db.migrate` | **Gap worth absorbing**: typed row mapping and a small composable query builder over `Driver`. Do not absorb a full ORM. |
| Image and media | Swift image libraries, game asset libraries, Python imaging ecosystem | `core.game` has typed game assets at `:1778-1818`; no general raster image module appears in the built-module list at `:3810-3834`; ledger has no ordinary `core.image` family | **Gap worth absorbing**: bounded `core.image` decode/encode/resize/metadata for common raster formats. Keep video codecs and platform UI out. |
| Locale and i18n | Rails, Java, .NET, Python date/locale packages, Swift formatting ecosystem | `core.text` explicitly leaves locale collation and locale-sensitive casing out at `:1967-1969`; `core.time` supports zones but not message catalogs | **Gap worth absorbing**: explicit `core.i18n` locale, plural, collation, message lookup, and locale-aware number/date formatting. No ambient host locale. |
| Web frameworks and full-stack batteries | Rails, Laravel, Symfony, Spring, ASP.NET MVC, Express, Gin | Core has HTTP, routing, DB, tasks, auth, services, and UI, but no single framework contract; framework transplant closeout records multiple bounded laws and unshipped framework scope (`docs/reference/framework-transplant-closeout.md:25-32`) | **Out** as a framework. Absorb narrow primitives only when a separate cluster proves reach. |
| Async runtime and services | `tokio`, `futures`, `sidekiq`, `PromiseKit`, `go-redis`, `Service` ecosystems | `core.tasks`, channels, and `core.services` are in `core-library.md:2586-2669` and `:3707-3777` | **already-in-Core** for language-level scheduling and bounded services. Provider-specific queues and hosted control planes are out. |
| Compression and archives | `zlib`/Python archive usage, Rust compression ecosystem, PHP/Ruby archive use | `core.archive`, `core.archive.gzip`, and `core.archive.zstd` at `:3788-3806`; ledger `module.core.archive.*` | **already-in-Core** |
| Crypto and identity | `cryptography`, Rust crypto crates, `ramsey/uuid`, Rails/Java auth batteries | `core.crypto`, `core.auth`, UUID, Argon2id, signatures, and envelopes at `:450-478`, `:708-920`; framework-transplant closeout marks auth as bounded and partly unshipped at `framework-transplant-closeout.md:26` | **already-in-Core** for safe primitives and bounded sessions. Provider networks, durable app identity, and full auth frameworks are out. |
| Templates and server views | Jinja/Django templates, Rails views, Laravel Blade, Symfony Twig ecosystem | Core has checked string/HTML interpolation and UI trees, but no general template language in the built-module list (`core-library.md:3810-3834`) | **Out for now**: template engines are language-sized and compete with Jet syntax. Absorb safe HTML fragments only through existing typed interpolation. |

## Reach ranking

The registry periods differ, so this ranking uses normalized reach, not raw
download addition. A cluster receives one reach mark for each ecosystem where
the evidence contains a high-use representative. Download counts support the
direction; they are not summed across incompatible periods. The order is:

| Rank | Gap | Reach basis | Existing coverage to preserve |
|---:|---|---|---|
| 1 | Typed DB mapping + small query builder | Python, Rust, Go, Java/Kotlin, C#, Ruby, PHP all show DB or ORM libraries; NuGet and RubyGems counts are especially large | `core.db` checked SQL, `Driver`, `DBScope`, transactions, migrations |
| 2 | Explicit i18n and locale data | Java/Kotlin, C#, Ruby/Rails, Python, Swift all have high-use locale or formatting paths; current Core explicitly declines locale data | `core.time` zone model and locale-free `core.text` behavior |
| 3 | Bounded image basics | Python and Swift ecosystems show durable image/media demand; current Core only has game asset handles | `core.game` asset handles and `core.data.plot` SVG output |

Not ranked as gaps: HTTP, serialization, CLI, logging, testing, time,
archives, crypto, tasks, collections, and ordinary data transforms. Core
already owns them. Frameworks, full ORMs, hosted queues, video, and template
languages are explicitly out.

## Framework-transplant lineage

Card `#1161` is the framework-transplant parent. The current closeout at
[`docs/reference/framework-transplant-closeout.md`](../reference/framework-transplant-closeout.md)
shows the relevant boundaries:

- `D-AUTH1` ships bounded auth/session helpers but leaves durable DB-backed app
  routes, provider network, mail delivery, and remote reconnect unshipped.
- `D-SYNC1` ships typed CRDT carriers but leaves generic carriers, network
  transport, and authenticated routing unshipped.
- `D-DBPOLICY1` ships a bounded row-policy language and leaves a general typed
  closure compiler and audit listing unshipped.
- `D-VALIDATE1` and `D-VALIDATE-DECODE1` establish one validation/decode
  contract; do not create a second validation or codec mechanism.
- `D-OBSERVE-LIVE1` is marked shipped-one-tier and still has producer-tier
  gaps. Do not use this survey to reopen it.

The three proposed gaps fit the lineage without reopening its decisions:
database ergonomics extends the existing `Driver` path; i18n is a new narrow
data service with explicit locale state; image is a bounded codec/data service.
No framework transplant is proposed.

## Follow-up candidates

Owner lane should mint at most three task cards from these sections:

1. **Core DB typed mapping and query builder** — reference this report's
   “Database access” row and `core-library.md:3550-3588`. Preserve checked SQL,
   `Driver`, `DBScope`, policy binding, and I9 parity.
2. **Core explicit i18n** — reference the “Locale and i18n” row and
   `core-library.md:1967-1969`. Require explicit locale data and no host-locale
   fallback.
3. **Core bounded image basics** — reference the “Image and media” row and
   `core-library.md:1778-1818`. Start with formats and bounds, not a UI or video
   framework.

No follow-up card IDs exist because the current instruction forbids board
writes. The orchestrator or owner must mint and log them on #2048.

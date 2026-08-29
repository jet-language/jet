# Index and cache publication

The prepared endpoints are separate subdomains:

```text
index.jet-lang.dev/     signed nixpkgs index
cache.jet-lang.dev/     signed Jetpack/Hangar cache objects
```

The files are static. A publisher writes immutable targets first, verifies
their signatures, and writes the mutable channel manifest last.

## Index contract

The existing `jetpack-nix-index` producer owns the bytes and signature
requests. The served paths are:

```text
/v1/<channel>/manifest.json
/v1/<channel>/manifest.json.sig.json
/index-v1/<revision>/<system>/<sha256>.json.zst
/index-v1/<revision>/<system>/<sha256>.json.zst.sig.json
```

`manifest.json` and its sidecar use `application/json`. Compressed index
targets use `application/zstd`; target signature sidecars use
`application/json`. The manifest and target signatures use the existing
`NixIndex` domains and Ed25519 sidecar shape. The manifest URL points at
`index.jet-lang.dev`, and the target digest is the SHA-256 of the compressed
bytes. Targets are immutable and content-addressed. A manifest is signed only
after every target and target sidecar is present.

The existing client reads these host-owned knobs as a pair:

```text
<JETPACK_ROOT>/config/nix-index-v1.endpoint
<JETPACK_ROOT>/trust/nix-index-v1.ed25519.pub
```

Both files must be installed together. The endpoint is explicit for the
signed catalog; production use requires HTTPS. Loopback HTTP remains available
for local development. The public key uses
`key-id:base64-public-key`; private signing keys stay with the offline signer.

## Cache contract

The prepared Hangar cache tree contains:

```text
/nar/<output-hash>.nar
/<output-hash>-<entry-id>.narinfo
/trust/<output-hash>-<entry-id>.receipt
```

NAR files use `application/octet-stream`. `narinfo` and receipt files use
`text/plain`. The `narinfo` is signed by the existing HMAC cache-role key and
the receipt uses the existing `jet-cache-receipt-v1` protocol. The role key,
builder allowlist, and witness state remain in the host Jetpack root. They are
not copied into the site tree.

This is distinct from the upstream Nix cache client, whose existing paired
knobs are `config/nix-cache-v1.endpoint` and
`trust/nix-cache-v1.ed25519.pub`, with Nix's normal `nix-cache-info`,
`<store-hash>.narinfo`, and `nar/` layout.

## Prepare a local tree

Generate and sign index targets with the existing producer. Then stage the
index files and every eligible Hangar entry in one local tree:

```sh
tools/jetpack-infra/stage-index-cache.sh \
  --index-root <index-producer-output> \
  --hangar-root <jetpack-root> \
  --output <publication-staging> \
  --channel nixpkgs-unstable \
  --role public
```

The command invokes `jetpack hangar cache stage`, which reuses the existing
Hangar provenance checks, canonical NAR writer, cache receipt, and signing
machinery. It never contacts a remote endpoint.

## Publish when hosting exists

After production signatures, DNS, TLS, and a reviewed signer/uploader are
available, the one-command file publication step is:

```sh
rsync -a --checksum --ignore-existing <publication-staging>/ "$HOST_ROOT/"
```

`HOST_ROOT`, object-store or web-server credentials, production key custody,
and owner approval remain pending. This lane prepares files only.

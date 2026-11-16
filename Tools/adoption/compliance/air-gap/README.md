# Air-gap fixture contract

[`fixture.json`](../../fixtures/air-gap/fixture.json) is deterministic,
fixture-only state. It contains no production key, signing secret, network
endpoint, or release claim. Its hashes, byte counts, trust-root labels, and
expected transitions are the executable fixture contract.

The scenarios model four transitions with network denied:

1. install `jet-1.0.0` from local bytes when its digest and builder are valid;
2. replace it with `jet-1.1.0` from local bytes before revocation;
3. revoke `builder-fixture-v2` and deny use of that release; and
4. deny a later replacement with the revoked builder while retaining the
   receipt and requiring a rebuild from a reviewed release.

The fixture checker validates path safety, content digests, revocation reason,
secret absence, and the allow/deny sequence. It checks structure and state; it
does not perform production cryptography and is not a Hangar archive.

## Production command boundary

For an actual air-gapped archive, use the existing Jetpack operations:

```sh
jetpack hangar verify <release.hangar>
jetpack hangar import <release.hangar>
jetpack env --prep --offline
```

`jetpack update --offline` remains a network-class refusal unless a local
catalog is explicitly supplied. A release replacement therefore consists of a
verified local archive import followed by offline environment preparation, not
a silent network update. Retain the verification receipt and the deny result
when a builder or release is revoked.

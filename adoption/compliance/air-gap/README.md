# Air-gap fixture contract

[`fixture.json`](../../fixtures/air-gap/fixture.json) is deterministic,
fixture-only state. It contains no production key, signing secret, network
endpoint, or release claim. It exercises the state transitions an operator
must prove before claiming an air-gapped workflow:

1. install a verified release from local bytes with network denied;
2. import a newer verified release from local bytes and use it offline;
3. revoke the newer builder and deny later use of that release;
4. retain the failed receipt and require a rebuild or reviewed replacement.

The fixture records expected outcomes and content digests. It does not pretend
to be a Hangar archive. Production verification uses the existing commands:

```sh
jetpack hangar verify <release.hangar>
jetpack hangar import <release.hangar>
jetpack env --prep --offline
```

`jetpack update --offline` remains a network-class refusal unless a local
catalog is explicitly supplied. The runbook therefore treats an offline
release replacement as a verified local archive import followed by offline
environment preparation, not as a silent network update.

The fixture checker validates the allow/deny sequence, hashes, revocation
reason, path safety, and secret absence. It does not perform cryptography.

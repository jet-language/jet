# Air-gap fixture data

This tree is deterministic test data, not a release archive. The executable
contract is [`fixture.json`](fixture.json): it records the fixture schema,
plain-text payload paths, SHA-256 values, byte counts, fixture public-key
identity, builder revocation, and expected allow/deny transitions.

The payloads are deliberately plain text so a review can inspect every byte.
They contain no private key, production trust root, signing secret, endpoint,
or release claim. The fixture validator checks hashes, path safety, network
labels, secret absence, and the install/update/revocation sequence; it does not
perform production cryptography.

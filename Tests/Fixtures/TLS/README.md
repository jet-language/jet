TLS Fixtures
============

Hermetic TLS tests use checked-in localhost certificates. They are not trusted
by the OS; they are for local servers that deliberately exercise the TLS client
path without touching the internet.

Regenerate with:

```
openssl req -x509 -newkey rsa:2048 \
  -keyout localhost.key.pem \
  -out localhost.cert.pem \
  -days 3650 -nodes \
  -subj /CN=localhost \
  -addext subjectAltName=DNS:localhost,IP:127.0.0.1
```
# Custom-root fixture

`custom-root.ca.cert.pem` signs `custom-root.server.cert.pem`; the leaf
certificate is valid only for `localhost`. `custom-root.server.key.pem` is
test-only. TLS and HTTP client tests use this chain to prove custom trust roots
extend or replace native roots without disabling hostname verification.

# ECDSA fixture

`ec-root.ca.cert.pem` (P-384, self-signed with ECDSA SHA-384) signs
`ec-root.server.cert.pem`, a P-256 leaf valid only for `localhost`.
`ec-root.server.key.pem` is test-only; the CA key was not kept. The TLS
interop case (#6694) uses this chain for its ECDSA rows. Regenerate with:

```
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-384 -nodes \
  -keyout ca.key.pem -out ec-root.ca.cert.pem -days 3650 -sha384 \
  -subj /CN=Jet-Test-EC-CA \
  -addext basicConstraints=critical,CA:TRUE \
  -addext keyUsage=critical,keyCertSign,cRLSign
openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
  -keyout ec-root.server.key.pem -out leaf.csr -subj /CN=localhost
openssl x509 -req -in leaf.csr -CA ec-root.ca.cert.pem -CAkey ca.key.pem \
  -CAcreateserial -out ec-root.server.cert.pem -days 3650 -sha384 \
  -extfile leaf.ext
```

where `leaf.ext` holds `basicConstraints=critical,CA:FALSE`,
`keyUsage=critical,digitalSignature`, `extendedKeyUsage=serverAuth,clientAuth`
and `subjectAltName=DNS:localhost`.

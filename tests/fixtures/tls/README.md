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
# SMTP custom-root fixture

`smtp.ca.cert.pem` signs `smtp.server.cert.pem`; the leaf certificate is valid
only for `localhost`. `smtp.server.key.pem` is test-only. Card #530 uses this
chain to prove `SystemPlusCa` extends native roots without disabling hostname
verification.

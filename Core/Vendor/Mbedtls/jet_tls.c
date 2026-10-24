// jet_tls.c: the mechanics core.net.tls needs from the vendored mbedTLS
// (#5484). Jet's C FFI has no callback ABI, so the BIO wiring and the
// config-sized contexts live here. Every function returns mbedTLS's own status
// code; core.net.tls owns all policy: trust and verification defaults, version
// bounds, readiness waits and deadlines, WANT_READ/WANT_WRITE retries, and what
// each code means. The socket descriptor stays owned by the Jet TCPStream:
// jet_tls_free never closes it.
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>

#include "mbedtls/ctr_drbg.h"
#include "mbedtls/entropy.h"
#include "mbedtls/net_sockets.h"
#include "mbedtls/pk.h"
#include "mbedtls/ssl.h"
#include "mbedtls/x509_crt.h"
#include "psa/crypto.h"

#define JET_TLS_ALPN_MAX 16
#define JET_TLS_BUFFER 16384

typedef struct {
    mbedtls_ssl_context ssl;
    mbedtls_ssl_config conf;
    mbedtls_x509_crt ca;
    mbedtls_x509_crt own;
    mbedtls_pk_context key;
    mbedtls_entropy_context entropy;
    mbedtls_ctr_drbg_context drbg;
    mbedtls_net_context net;
    char *alpn[JET_TLS_ALPN_MAX + 1];
    int alpn_count;
    unsigned char buffer[JET_TLS_BUFFER];
} jet_tls;

static jet_tls *jet_tls_of(uint64_t handle) {
    return (jet_tls *) (uintptr_t) handle;
}

// mbedtls_net_send uses write(2), which raises SIGPIPE on a reset peer in a
// host that has not ignored it; send with MSG_NOSIGNAL like the Jet runtime's
// own socket writes. The status mapping is mbedtls_net_send's.
static int jet_tls_send(void *context, const unsigned char *data, size_t length) {
    int fd = ((mbedtls_net_context *) context)->fd;
    ssize_t sent = send(fd, data, length, MSG_NOSIGNAL);
    if (sent >= 0) {
        return (int) sent;
    }
    if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) {
        return MBEDTLS_ERR_SSL_WANT_WRITE;
    }
    if (errno == EPIPE || errno == ECONNRESET) {
        return MBEDTLS_ERR_NET_CONN_RESET;
    }
    return MBEDTLS_ERR_NET_SEND_FAILED;
}

void jet_tls_free(uint64_t handle) {
    jet_tls *tls = jet_tls_of(handle);
    if (tls == NULL) {
        return;
    }
    mbedtls_ssl_free(&tls->ssl);
    mbedtls_ssl_config_free(&tls->conf);
    mbedtls_x509_crt_free(&tls->ca);
    mbedtls_x509_crt_free(&tls->own);
    mbedtls_pk_free(&tls->key);
    mbedtls_ctr_drbg_free(&tls->drbg);
    mbedtls_entropy_free(&tls->entropy);
    for (int index = 0; index < tls->alpn_count; index++) {
        free(tls->alpn[index]);
    }
    free(tls);
}

// endpoint is MBEDTLS_SSL_IS_CLIENT (0) or MBEDTLS_SSL_IS_SERVER (1).
int32_t jet_tls_new(uint64_t *out, int32_t endpoint) {
    *out = 0;
    int status = (int) psa_crypto_init();
    if (status != 0) {
        return status;
    }
    jet_tls *tls = calloc(1, sizeof *tls);
    if (tls == NULL) {
        return MBEDTLS_ERR_SSL_ALLOC_FAILED;
    }
    mbedtls_ssl_init(&tls->ssl);
    mbedtls_ssl_config_init(&tls->conf);
    mbedtls_x509_crt_init(&tls->ca);
    mbedtls_x509_crt_init(&tls->own);
    mbedtls_pk_init(&tls->key);
    mbedtls_entropy_init(&tls->entropy);
    mbedtls_ctr_drbg_init(&tls->drbg);
    mbedtls_net_init(&tls->net);
    status = mbedtls_ctr_drbg_seed(&tls->drbg, mbedtls_entropy_func, &tls->entropy, (const unsigned char *) "jet_tls", 7);
    if (status == 0) {
        status = mbedtls_ssl_config_defaults(&tls->conf, endpoint, MBEDTLS_SSL_TRANSPORT_STREAM, MBEDTLS_SSL_PRESET_DEFAULT);
    }
    if (status != 0) {
        jet_tls_free((uint64_t) (uintptr_t) tls);
        return status;
    }
    mbedtls_ssl_conf_rng(&tls->conf, mbedtls_ctr_drbg_random, &tls->drbg);
    *out = (uint64_t) (uintptr_t) tls;
    return 0;
}

// mode is MBEDTLS_SSL_VERIFY_NONE (0), OPTIONAL (1) or REQUIRED (2).
int32_t jet_tls_authmode(uint64_t handle, int32_t mode) {
    mbedtls_ssl_conf_authmode(&jet_tls_of(handle)->conf, mode);
    return 0;
}

// Versions are mbedtls_ssl_protocol_version values (0x0303 TLS 1.2, 0x0304 TLS 1.3).
int32_t jet_tls_versions(uint64_t handle, int32_t min, int32_t max) {
    jet_tls *tls = jet_tls_of(handle);
    mbedtls_ssl_conf_min_tls_version(&tls->conf, (mbedtls_ssl_protocol_version) min);
    mbedtls_ssl_conf_max_tls_version(&tls->conf, (mbedtls_ssl_protocol_version) max);
    return 0;
}

// Adds PEM roots; a positive result counts certificates mbedTLS skipped.
int32_t jet_tls_ca_pem(uint64_t handle, const char *pem) {
    jet_tls *tls = jet_tls_of(handle);
    int status = mbedtls_x509_crt_parse(&tls->ca, (const unsigned char *) pem, strlen(pem) + 1);
    if (status >= 0) {
        mbedtls_ssl_conf_ca_chain(&tls->conf, &tls->ca, NULL);
    }
    return status;
}

// Adds the roots of one PEM/DER bundle file; same result as jet_tls_ca_pem.
int32_t jet_tls_ca_file(uint64_t handle, const char *path) {
    jet_tls *tls = jet_tls_of(handle);
    int status = mbedtls_x509_crt_parse_file(&tls->ca, path);
    if (status >= 0) {
        mbedtls_ssl_conf_ca_chain(&tls->conf, &tls->ca, NULL);
    }
    return status;
}

// Adds the roots of every file in one directory; same result as jet_tls_ca_pem.
int32_t jet_tls_ca_dir(uint64_t handle, const char *path) {
    jet_tls *tls = jet_tls_of(handle);
    int status = mbedtls_x509_crt_parse_path(&tls->ca, path);
    if (status >= 0) {
        mbedtls_ssl_conf_ca_chain(&tls->conf, &tls->ca, NULL);
    }
    return status;
}

// The number of root certificates loaded so far.
int64_t jet_tls_ca_count(uint64_t handle) {
    int64_t count = 0;
    for (const mbedtls_x509_crt *cert = &jet_tls_of(handle)->ca; cert != NULL && cert->raw.p != NULL; cert = cert->next) {
        count++;
    }
    return count;
}

// The local certificate chain and private key (server identity or client
// certificate), both PEM. The key must be the leaf certificate's key.
int32_t jet_tls_own_pem(uint64_t handle, const char *cert, const char *key) {
    jet_tls *tls = jet_tls_of(handle);
    int status = mbedtls_x509_crt_parse(&tls->own, (const unsigned char *) cert, strlen(cert) + 1);
    if (status != 0) {
        return status;
    }
    status = mbedtls_pk_parse_key(&tls->key, (const unsigned char *) key, strlen(key) + 1, NULL, 0, mbedtls_ctr_drbg_random, &tls->drbg);
    if (status != 0) {
        return status;
    }
    status = mbedtls_pk_check_pair(&tls->own.pk, &tls->key, mbedtls_ctr_drbg_random, &tls->drbg);
    if (status != 0) {
        return status;
    }
    return mbedtls_ssl_conf_own_cert(&tls->conf, &tls->own, &tls->key);
}

// Appends one ALPN protocol to the offered (client) or accepted (server) list.
int32_t jet_tls_alpn_add(uint64_t handle, const char *protocol) {
    jet_tls *tls = jet_tls_of(handle);
    if (tls->alpn_count == JET_TLS_ALPN_MAX) {
        return MBEDTLS_ERR_SSL_BAD_INPUT_DATA;
    }
    size_t length = strlen(protocol) + 1;
    char *copy = malloc(length);
    if (copy == NULL) {
        return MBEDTLS_ERR_SSL_ALLOC_FAILED;
    }
    memcpy(copy, protocol, length);
    tls->alpn[tls->alpn_count++] = copy;
    tls->alpn[tls->alpn_count] = NULL;
    return mbedtls_ssl_conf_alpn_protocols(&tls->conf, (const char **) tls->alpn);
}

// Binds the finished config to a connected socket. An empty hostname sets no
// SNI/verification name (servers).
int32_t jet_tls_setup(uint64_t handle, int64_t fd, const char *hostname) {
    jet_tls *tls = jet_tls_of(handle);
    int status = mbedtls_ssl_setup(&tls->ssl, &tls->conf);
    if (status != 0) {
        return status;
    }
    if (hostname[0] != 0) {
        status = mbedtls_ssl_set_hostname(&tls->ssl, hostname);
        if (status != 0) {
            return status;
        }
    }
    tls->net.fd = (int) fd;
    mbedtls_ssl_set_bio(&tls->ssl, &tls->net, jet_tls_send, mbedtls_net_recv, NULL);
    return 0;
}

int32_t jet_tls_handshake(uint64_t handle) {
    return mbedtls_ssl_handshake(&jet_tls_of(handle)->ssl);
}

// Reads at most min(limit, 16384) plaintext bytes into the handle's buffer
// (jet_tls_buffer); a positive result is the byte count, 0 is EOF.
int32_t jet_tls_read(uint64_t handle, int64_t limit) {
    jet_tls *tls = jet_tls_of(handle);
    size_t length = limit < JET_TLS_BUFFER ? (size_t) limit : JET_TLS_BUFFER;
    return mbedtls_ssl_read(&tls->ssl, tls->buffer, length);
}

uint64_t jet_tls_buffer(uint64_t handle) {
    return (uint64_t) (uintptr_t) jet_tls_of(handle)->buffer;
}

// Plaintext bytes already decrypted and buffered by mbedTLS.
int64_t jet_tls_bytes_avail(uint64_t handle) {
    return (int64_t) mbedtls_ssl_get_bytes_avail(&jet_tls_of(handle)->ssl);
}

// Writes from `length` bytes at address `data`; a positive result is the
// count mbedTLS accepted.
int32_t jet_tls_write(uint64_t handle, uint64_t data, int64_t length) {
    return mbedtls_ssl_write(&jet_tls_of(handle)->ssl, (const unsigned char *) (uintptr_t) data, (size_t) length);
}

int32_t jet_tls_close_notify(uint64_t handle) {
    return mbedtls_ssl_close_notify(&jet_tls_of(handle)->ssl);
}

// MBEDTLS_X509_BADCERT_* flags of the peer chain check.
int64_t jet_tls_verify_result(uint64_t handle) {
    return (int64_t) mbedtls_ssl_get_verify_result(&jet_tls_of(handle)->ssl);
}

int32_t jet_tls_version(uint64_t handle) {
    return (int32_t) mbedtls_ssl_get_version_number(&jet_tls_of(handle)->ssl);
}

const char *jet_tls_ciphersuite(uint64_t handle) {
    const char *name = mbedtls_ssl_get_ciphersuite(&jet_tls_of(handle)->ssl);
    return name == NULL ? "" : name;
}

const char *jet_tls_alpn(uint64_t handle) {
    const char *name = mbedtls_ssl_get_alpn_protocol(&jet_tls_of(handle)->ssl);
    return name == NULL ? "" : name;
}

// Length of a NUL-terminated string the engine or the environment owns. Jet
// imports this rather than libc `strlen`: a C symbol may be imported by only
// one C module (E1341), and core.db's SQLite module already imports `strlen`.
uint64_t jet_tls_cstr_len(const char *text) {
    return (uint64_t) strlen(text);
}

// The DER of peer certificate `index` (0 is the leaf); BAD_INPUT_DATA past
// the end of the chain.
int32_t jet_tls_peer_cert(uint64_t handle, int64_t index, uint64_t *data, uint64_t *length) {
    const mbedtls_x509_crt *cert = mbedtls_ssl_get_peer_cert(&jet_tls_of(handle)->ssl);
    for (int64_t at = 0; cert != NULL && at < index; at++) {
        cert = cert->next;
    }
    if (cert == NULL || cert->raw.p == NULL) {
        return MBEDTLS_ERR_SSL_BAD_INPUT_DATA;
    }
    *data = (uint64_t) (uintptr_t) cert->raw.p;
    *length = (uint64_t) cert->raw.len;
    return 0;
}

#include "projection_javascript.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct napi_env__ *napi_env;
typedef struct napi_value__ *napi_value;
typedef struct napi_callback_info__ *napi_callback_info;
typedef int32_t napi_status;
typedef napi_value (*napi_callback)(napi_env, napi_callback_info);
enum { napi_ok = 0 };

extern napi_status napi_get_cb_info(napi_env, napi_callback_info, size_t *, napi_value *, napi_value *, void **);
extern napi_status napi_get_value_bigint_int64(napi_env, napi_value, int64_t *);
extern napi_status napi_get_value_bigint_uint64(napi_env, napi_value, uint64_t *, bool *);
extern napi_status napi_get_buffer_info(napi_env, napi_value, void **, size_t *);
extern napi_status napi_create_bigint_int64(napi_env, int64_t, napi_value *);
extern napi_status napi_create_int32(napi_env, int32_t, napi_value *);
extern napi_status napi_create_bigint_uint64(napi_env, uint64_t, napi_value *);
extern napi_status napi_create_object(napi_env, napi_value *);
extern napi_status napi_create_buffer_copy(napi_env, size_t, const void *, void **, napi_value *);
extern napi_status napi_get_named_property(napi_env, napi_value, const char *, napi_value *);
extern napi_status napi_set_named_property(napi_env, napi_value, const char *, napi_value);
extern napi_status napi_create_function(napi_env, const char *, size_t, napi_callback, void *, napi_value *);
extern napi_status napi_throw_error(napi_env, const char *, const char *);

static napi_value jet_error(napi_env env, const char *message) {
    napi_throw_error(env, NULL, message);
    return NULL;
}

static bool jet_args(napi_env env, napi_callback_info info, size_t expected, napi_value *argv) {
    size_t argc = expected;
    return napi_get_cb_info(env, info, &argc, argv, NULL, NULL) == napi_ok && argc == expected;
}

static napi_value jet_status(napi_env env, int32_t status) {
    napi_value result;
    napi_value value;
    if (napi_create_object(env, &result) != napi_ok ||
        napi_create_int32(env, status, &value) != napi_ok ||
        napi_set_named_property(env, result, "status", value) != napi_ok) {
        return NULL;
    }
    return result;
}

static bool jet_property_u64(napi_env env, napi_value object, const char *name, uint64_t *out) {
    napi_value value;
    bool lossless = false;
    return napi_get_named_property(env, object, name, &value) == napi_ok &&
        napi_get_value_bigint_uint64(env, value, out, &lossless) == napi_ok && lossless;
}

static bool jet_token(napi_env env, napi_value object, uint64_t *slot, uint64_t *generation) {
    return jet_property_u64(env, object, "slot", slot) &&
        jet_property_u64(env, object, "generation", generation);
}

static bool jet_set_u64(napi_env env, napi_value object, const char *name, uint64_t value) {
    napi_value encoded;
    return napi_create_bigint_uint64(env, value, &encoded) == napi_ok &&
        napi_set_named_property(env, object, name, encoded) == napi_ok;
}

static napi_value jet_result(napi_env env, int32_t status, const JetComponent *error) {
    napi_value result = jet_status(env, status);
    if (result == NULL) return NULL;
    if (error != NULL && error->ptr != NULL && error->len != 0) {
        napi_value payload;
        if (napi_create_buffer_copy(env, error->len, error->ptr, NULL, &payload) != napi_ok ||
            napi_set_named_property(env, result, "error", payload) != napi_ok) {
            jet_component_free(*error);
            return jet_error(env, "could not expose guest error payload");
        }
    }
    if (error != NULL) jet_component_free(*error);
    return result;
}

static napi_value jet_napi_add(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    int64_t input;
    if (!jet_args(env, info, 1, argv) || napi_get_value_bigint_int64(env, argv[0], &input) != napi_ok) {
        return jet_error(env, "add expects one signed I64 BigInt");
    }
    napi_value value;
    if (napi_create_bigint_int64(env, add(input), &value) != napi_ok) {
        return jet_error(env, "could not create result BigInt");
    }
    return value;
}
static napi_value jet_napi_label(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    void *data = NULL;
    size_t len = 0;
    if (!jet_args(env, info, 1, argv) || napi_get_buffer_info(env, argv[0], &data, &len) != napi_ok) {
        return jet_error(env, "label expects a Buffer");
    }
    JetText result = label((JetText){(const uint8_t *)data, len});
    if (result.ptr == NULL && result.len != 0) return jet_error(env, "label returned an invalid text value");
    napi_value value;
    if (napi_create_buffer_copy(env, result.len, result.ptr, NULL, &value) != napi_ok) {
        jet_text_free(result);
        return jet_error(env, "could not expose label result");
    }
    jet_text_free(result);
    return value;
}

static napi_value jet_napi_project_component(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    void *data = NULL;
    size_t len = 0;
    if (!jet_args(env, info, 1, argv) || napi_get_buffer_info(env, argv[0], &data, &len) != napi_ok) {
        return jet_error(env, "project_component expects a Buffer");
    }
    JetComponent result = project_component(
        (JetComponent){(const uint8_t *)data, len}
    );
    if (result.ptr == NULL && result.len != 0) return jet_error(env, "project_component returned an invalid component");
    napi_value value;
    if (napi_create_buffer_copy(env, result.len, result.ptr, NULL, &value) != napi_ok) {
        jet_component_free(result);
        return jet_error(env, "could not expose component result");
    }
    jet_component_free(result);
    return value;
}

static napi_value jet_napi_document_open(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    void *data = NULL;
    size_t len = 0;
    projection_javascript_Document owner = {0, 0};
    JetComponent error = {0, 0};
    if (!jet_args(env, info, 1, argv) || napi_get_buffer_info(env, argv[0], &data, &len) != napi_ok) {
        return jet_error(env, "document_open expects a Buffer");
    }
    int32_t status = projection_javascript_document_open((const uint8_t *)data, len, &owner, &error);
    napi_value result = jet_result(env, status, &error);
    if (result == NULL || status != PROJECTION_JAVASCRIPT_OK) return result;
    if (!jet_set_u64(env, result, "slot", owner.slot) || !jet_set_u64(env, result, "generation", owner.generation)) {
        return jet_error(env, "could not create document token");
    }
    return result;
}

static napi_value jet_napi_document_bytes(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    uint64_t slot = 0, generation = 0;
    projection_javascript_Document owner;
    projection_javascript_View view = {0, 0};
    JetComponent error = {0, 0};
    if (!jet_args(env, info, 1, argv) || !jet_token(env, argv[0], &slot, &generation)) {
        return jet_error(env, "document_bytes expects a resource token");
    }
    owner.slot = slot;
    owner.generation = generation;
    int32_t status = projection_javascript_document_bytes(&owner, &view, &error);
    napi_value result = jet_result(env, status, &error);
    if (result == NULL || status != PROJECTION_JAVASCRIPT_OK) return result;
    if (!jet_set_u64(env, result, "slot", view.slot) || !jet_set_u64(env, result, "generation", view.generation)) {
        return jet_error(env, "could not create view token");
    }
    return result;
}

static napi_value jet_napi_view_at(napi_env env, napi_callback_info info) {
    napi_value argv[2];
    uint64_t slot = 0, generation = 0;
    int64_t index = 0;
    projection_javascript_View view;
    int64_t value = 0;
    JetComponent error = {0, 0};
    if (!jet_args(env, info, 2, argv) || !jet_token(env, argv[0], &slot, &generation) ||
        napi_get_value_int64(env, argv[1], &index) != napi_ok || index < 0) {
        return jet_error(env, "view_at expects a token and non-negative integer index");
    }
    view.slot = slot;
    view.generation = generation;
    int32_t status = projection_javascript_view_at(&view, (size_t)index, &value, &error);
    napi_value result = jet_result(env, status, &error);
    if (result == NULL) return NULL;
    napi_value encoded;
    if (napi_create_bigint_int64(env, value, &encoded) != napi_ok ||
        napi_set_named_property(env, result, "value", encoded) != napi_ok) {
        return jet_error(env, "could not create view value");
    }
    return result;
}

static napi_value jet_napi_document_replace(napi_env env, napi_callback_info info) {
    napi_value argv[2];
    uint64_t slot = 0, generation = 0;
    void *data = NULL;
    size_t len = 0;
    projection_javascript_Document owner;
    JetComponent error = {0, 0};
    if (!jet_args(env, info, 2, argv) || !jet_token(env, argv[0], &slot, &generation) ||
        napi_get_buffer_info(env, argv[1], &data, &len) != napi_ok) {
        return jet_error(env, "document_replace expects a token and Buffer");
    }
    owner.slot = slot;
    owner.generation = generation;
    int32_t status = projection_javascript_document_replace(
        &owner, (const uint8_t *)data, len, &error);
    napi_value result = jet_result(env, status, &error);
    if (result == NULL || status != PROJECTION_JAVASCRIPT_OK) return result;
    if (!jet_set_u64(env, result, "generation", owner.generation)) {
        return jet_error(env, "could not expose replacement generation");
    }
    return result;
}

static napi_value jet_napi_document_close(napi_env env, napi_callback_info info) {
    napi_value argv[1];
    uint64_t slot = 0, generation = 0;
    projection_javascript_Document owner;
    JetComponent error = {0, 0};
    if (!jet_args(env, info, 1, argv) || !jet_token(env, argv[0], &slot, &generation)) {
        return jet_error(env, "document_close expects a resource token");
    }
    owner.slot = slot;
    owner.generation = generation;
    return jet_result(env, projection_javascript_document_close(&owner, &error), &error);
}

__attribute__((visibility("default"))) napi_value napi_register_module_v1(napi_env env, napi_value exports) {
    struct {
        const char *name;
        napi_callback callback;
    } functions[] = {
        {"add", jet_napi_add},
        {"label", jet_napi_label},
        {"project_component", jet_napi_project_component},
        {"projection_javascript_document_open", jet_napi_document_open},
        {"projection_javascript_document_bytes", jet_napi_document_bytes},
        {"projection_javascript_view_at", jet_napi_view_at},
        {"projection_javascript_document_replace", jet_napi_document_replace},
        {"projection_javascript_document_close", jet_napi_document_close},
    };
    for (size_t index = 0; index < sizeof(functions) / sizeof(functions[0]); ++index) {
        napi_value function;
        size_t length = strlen(functions[index].name);
        if (napi_create_function(env, functions[index].name, length, functions[index].callback, NULL, &function) != napi_ok ||
            napi_set_named_property(env, exports, functions[index].name, function) != napi_ok) {
            return NULL;
        }
    }
    return exports;
}

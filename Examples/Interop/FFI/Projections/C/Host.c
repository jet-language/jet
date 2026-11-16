#include "projection_c.h"
#include <string.h>

int main(void) {
    const uint8_t initial[] = {10, 20, 30};
    const uint8_t replacement[] = {40, 50, 60};
    projection_c_Document document = {0, 0};
    projection_c_View stale = {0, 0};
    int64_t value = 0;
    JetComponent error = {0, 0};
    if (projection_c_document_open(initial, sizeof(initial), &document, &error) != PROJECTION_C_OK) return 1;
    if (projection_c_document_bytes(&document, &stale, &error) != PROJECTION_C_OK) return 1;
    if (projection_c_view_at(&stale, 1, &value, &error) != PROJECTION_C_OK || value != 20) return 1;
    if (projection_c_document_replace(&document, replacement, sizeof(replacement), &error) != PROJECTION_C_OK) return 1;
    if (projection_c_view_at(&stale, 1, &value, &error) != PROJECTION_C_EXPIRED_VIEW) return 1;
    projection_c_View fresh = {0, 0};
    if (projection_c_document_bytes(&document, &fresh, &error) != PROJECTION_C_OK) return 1;
    if (projection_c_view_at(&fresh, 1, &value, &error) != PROJECTION_C_OK || value != 50) return 1;
    if (projection_c_document_close(&document, &error) != PROJECTION_C_OK) return 1;
    if (projection_c_document_close(&document, &error) != PROJECTION_C_CLOSED) return 1;
    const char label_input[] = "hello";
    JetText label_output = label((JetText){(const uint8_t *)label_input, sizeof(label_input) - 1});
    if (label_output.ptr == NULL || label_output.len != sizeof(label_input) - 1 ||
        memcmp(label_output.ptr, label_input, label_output.len) != 0) return 1;
    jet_text_free(label_output);
    const char record_input[] = "[{\"count\":7,\"label\":\"ok\"}]";
    JetComponent record_output = project_component(
        (JetComponent){(const uint8_t *)record_input, sizeof(record_input) - 1}
    );
    if (record_output.ptr == NULL || record_output.len == 0) return 1;
    jet_component_free(record_output);
    return add(41) == 42 ? 0 : 1;
}

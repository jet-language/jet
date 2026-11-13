/* Prints every static text header the unit's exports return: its byte
 * length and text, or "null header" where a let-bound call lowered to 0
 * (#6777). */
#include <stdint.h>
#include <stdio.h>

extern int64_t static_direct(void);
extern int64_t static_direct_empty(void);
extern int64_t static_let(void);
extern int64_t static_let_helper(void);
extern int64_t static_let_empty(void);
extern int64_t static_let_loop(int64_t);

/* A static text header is [data, byte length, capacity]. */
static void show(const char *name, int64_t header) {
    if (header == 0) {
        printf("%s: null header\n", name);
        return;
    }
    const int64_t *words = (const int64_t *)(intptr_t)header;
    printf("%s: %ld \"", name, (long)words[1]);
    if (words[1] > 0) {
        fwrite((const char *)(intptr_t)words[0], 1, (size_t)words[1], stdout);
    }
    printf("\"\n");
}

int main(void) {
    show("direct", static_direct());
    show("let", static_let());
    show("direct empty", static_direct_empty());
    show("let through a helper", static_let_helper());
    show("let empty", static_let_empty());
    show("let before a loop", static_let_loop(3));
    return 0;
}

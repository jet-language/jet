#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#define SIDE ((size_t)4096)
#define ELEMENTS (SIDE * SIDE)

int main(void) {
    double *left = malloc(ELEMENTS * sizeof(*left));
    double *right = malloc(ELEMENTS * sizeof(*right));
    double *mapped = malloc(ELEMENTS * sizeof(*mapped));
    if (left == NULL || right == NULL || mapped == NULL) {
        free(mapped);
        free(right);
        free(left);
        return 1;
    }

    for (size_t i = 0; i < ELEMENTS; ++i) {
        left[i] = 1.0;
        right[i] = 2.0;
    }
    for (size_t i = 0; i < ELEMENTS; ++i) {
        mapped[i] = left[i] + right[i];
    }

    const double corner = mapped[ELEMENTS - 1];
    double checksum = 0.0;
    for (size_t i = 0; i < ELEMENTS; ++i) {
        checksum += mapped[i];
    }
    if (corner != 3.0 || checksum != 50331648.0) {
        free(mapped);
        free(right);
        free(left);
        return 1;
    }
    printf(
        "shape:[4096, 4096] numel:%zu corner:%.1f checksum:%.0f\n",
        ELEMENTS,
        corner,
        checksum
    );

    free(mapped);
    free(right);
    free(left);
    return 0;
}

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned char *sieve(size_t n) {
    unsigned char *prime = malloc(n);
    memset(prime, 1, n);
    if (n > 0) prime[0] = 0;
    if (n > 1) prime[1] = 0;
    size_t p = 3;
    while (p * p < n) {
        if (prime[p] == 1) {
            size_t multiple = p * p;
            while (multiple < n) {
                prime[multiple] = 0;
                multiple += p * 2;
            }
        }
        p += 2;
    }
    return prime;
}

int main(int argc, char **argv) {
    size_t n = (size_t)strtoull(argv[1], NULL, 10);
    unsigned char *prime = sieve(n);
    size_t count = 0;
    size_t largest = 0;
    if (n > 2) {
        count = 1;
        largest = 2;
        for (size_t i = 3; i < n; i += 2) {
            if (prime[i] == 1) {
                count += 1;
                largest = i;
            }
        }
    }
    printf("count %zu\n", count);
    printf("largest %zu\n", largest);
    free(prime);
    return 0;
}

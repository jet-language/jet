#include <stdio.h>

int main(int argc, char **argv) {
    printf("hello from C\n");
    for (int i = 1; i < argc; i++) printf("%s\n", argv[i]);
    return 3;
}

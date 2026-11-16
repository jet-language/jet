#include <stdio.h>

template <typename T> T twice(T value) { return value + value; }

struct Counter {
    int count = 0;
    void bump() { count += 1; }
};

int main() {
    Counter counter;
    counter.bump();
    counter.bump();
    printf("twice %d, count %d\n", twice(21), counter.count);
    return 0;
}

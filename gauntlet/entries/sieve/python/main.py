import sys


def sieve(n):
    prime = bytearray([1]) * n
    if n > 0:
        prime[0] = 0
    if n > 1:
        prime[1] = 0
    p = 3
    while p * p < n:
        if prime[p] == 1:
            multiple = p * p
            while multiple < n:
                prime[multiple] = 0
                multiple += p * 2
        p += 2
    return prime


n = int(sys.argv[1])
prime = sieve(n)
count = 0
largest = 0
if n > 2:
    count = 1
    largest = 2
    for i in range(3, n, 2):
        if prime[i] == 1:
            count += 1
            largest = i
print(f"count {count}")
print(f"largest {largest}")

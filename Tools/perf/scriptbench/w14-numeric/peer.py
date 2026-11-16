total = 0
for i in range(1, 30000001):
    total += i * i % 7919
print(total)

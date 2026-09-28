from multiprocessing import Pool

def work(n):
    total = n
    for i in range(1, 120001):
        total = (total * 33 + i + n) % 1000003
    return total

with Pool(16) as pool:
    results = pool.map(work, range(256))
print("python_workers=16 items=256 results=%d" % len(results))

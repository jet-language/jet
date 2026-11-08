import subprocess

ok = 0
for _ in range(100):
    if subprocess.run(["true"]).returncode == 0:
        ok += 1
print(f"children ok={ok}")

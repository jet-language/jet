import subprocess


calc = subprocess.run(
    ["python3", "-c", "print(6*7)"], capture_output=True, text=True, check=True
)
print(f"calc {calc.stdout.strip()}")

source = subprocess.Popen(
    ["python3", "-c", "print('b');print('a');print('c')"], stdout=subprocess.PIPE
)
sorter = subprocess.Popen(["sort"], stdin=source.stdout, stdout=subprocess.PIPE)
source.stdout.close()
sorted_output, _ = sorter.communicate()
source.wait()
print(f"sorted {','.join(sorted_output.decode().splitlines())}")

try:
    subprocess.run(
        ["python3", "-c", "import time;time.sleep(5)"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=0.3,
        check=True,
    )
except subprocess.TimeoutExpired:
    print("slow timeout")

try:
    subprocess.run(
        ["python3", "-c", "import sys;sys.exit(3)"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
except subprocess.CalledProcessError as error:
    print(f"exit {error.returncode}")

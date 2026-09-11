import numpy as np

SIDE = 4096

left = np.full((SIDE, SIDE), 1.0, dtype=np.float64)
right = np.full((SIDE, SIDE), 2.0, dtype=np.float64)
mapped = left + right

corner = mapped[-1, -1]
checksum = np.sum(mapped, dtype=np.float64)
if corner != 3.0 or checksum != 50_331_648.0:
    raise RuntimeError("tensor map validation failed")
print(
    f"shape:{list(mapped.shape)} numel:{mapped.size} "
    f"corner:{corner:.1f} checksum:{int(checksum)}"
)

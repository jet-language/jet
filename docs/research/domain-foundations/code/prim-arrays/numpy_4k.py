import time
import numpy as np
try:
    import xarray as xr
except ImportError:
    xr = None

start = time.perf_counter()
left = np.full((4096, 4096), 1.0, dtype=np.float64)
right = np.full((4096, 4096), 2.0, dtype=np.float64)
mapped = left + right
corner = mapped[-1, -1]
print(f"shape:{list(mapped.shape)} numel:{mapped.size} corner:{corner}")
print(f"elapsed:{time.perf_counter()-start:.6f} sec")

if xr is not None:
    start = time.perf_counter()
    xleft = xr.DataArray(left, dims=("y", "x"))
    xright = xr.DataArray(right, dims=("y", "x"))
    xmapped = xleft + xright
    xcorner = xmapped.data[-1, -1]
    print(f"xarray dims:{list(xmapped.dims)} numel:{xmapped.size} corner:{xcorner}")
    print(f"xarray_elapsed:{time.perf_counter()-start:.6f} sec")
else:
    print("xarray:unavailable")

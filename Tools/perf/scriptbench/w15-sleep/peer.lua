-- Lua has no sleep in its standard library; LuaJIT's FFI calls usleep(3). Run under luajit only.
local ffi = require("ffi")
ffi.cdef("int usleep(unsigned int usec);")
for _ = 1, 30 do ffi.C.usleep(100000) end
print("slept 30")

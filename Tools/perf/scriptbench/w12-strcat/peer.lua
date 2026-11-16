-- Lua strings are immutable; `..` in a loop is quadratic, so the idiomatic form is a parts table.
local parts = {}
for i = 1, 2000000 do parts[i] = "abc," end
local s = table.concat(parts)
print(string.format("len=%d", #s))

-- One program for Lua 5.4 and LuaJIT, which share no binary unpacker, no bit
-- operators and no 64-bit integer: fields are decoded from string.byte, and the
-- 64-bit FNV-1a hash is two 32-bit halves held exactly in doubles.
local byte, floor, format = string.byte, math.floor, string.format

local file = assert(io.open(arg[1] or "records.bin", "rb"))
local data = file:read("*a")
file:close()
if #data < 8 or data:sub(1, 4) ~= "JGB1" then
  error("bad magic")
end

-- XOR of two bytes, indexed [a * 256 + b].
local xor8 = {}
for a = 0, 255 do
  for b = 0, 255 do
    local x, bit, ra, rb = 0, 1, a, b
    for _ = 1, 8 do
      if ra % 2 ~= rb % 2 then
        x = x + bit
      end
      ra, rb, bit = floor(ra / 2), floor(rb / 2), bit * 2
    end
    xor8[a * 256 + b] = x
  end
end

-- A little-endian IEEE 754 binary64 from its eight bytes.
local function f64(b0, b1, b2, b3, b4, b5, b6, b7)
  local sign = b7 >= 128 and -1 or 1
  local exponent = (b7 % 128) * 16 + floor(b6 / 16)
  local mantissa = ((((((b6 % 16) * 256 + b5) * 256 + b4) * 256 + b3) * 256 + b2) * 256 + b1) * 256 + b0
  if exponent == 0 then
    return sign * mantissa * 2.0 ^ -1074
  elseif exponent == 2047 then
    return mantissa == 0 and sign * math.huge or 0 / 0
  end
  return sign * (mantissa + 4503599627370496) * 2.0 ^ (exponent - 1075)
end

local b4, b5, b6, b7 = byte(data, 5, 8)
local count = ((b7 * 256 + b6) * 256 + b5) * 256 + b4
local at = 9
local total = 0.0
-- FNV-1a offset 0xcbf29ce484222325 as high and low 32-bit halves.
local hi, lo = 3421674724, 2216829733
local TWO32, TWO24 = 4294967296, 16777216
for _ = 1, count do
  -- id:u32 value:f64 name_len:u16, then name_len name bytes
  local i0, i1, i2, i3, v0, v1, v2, v3, v4, v5, v6, v7, n0, n1 = byte(data, at, at + 13)
  if not n1 then
    error("truncated record")
  end
  local id = ((i3 * 256 + i2) * 256 + i1) * 256 + i0
  local name_len = n1 * 256 + n0
  at = at + 14
  if at + name_len - 1 > #data then
    error("truncated name")
  end
  if id % 7 == 0 then
    total = total + f64(v0, v1, v2, v3, v4, v5, v6, v7)
  end
  for k = at, at + name_len - 1 do
    local low = lo % 256
    lo = lo - low + xor8[low * 256 + byte(data, k)]
    -- (hi * 2^32 + lo) * (2^40 + 435) modulo 2^64.
    local t = lo * 435
    local carry = floor(t / TWO32)
    hi = (hi * 435 + carry + (lo % TWO24) * 256) % TWO32
    lo = t % TWO32
  end
  at = at + name_len
end

io.write(format("records %d\nsum7 %.6f\nfnv %08x%08x\n", count, total, hi, lo))

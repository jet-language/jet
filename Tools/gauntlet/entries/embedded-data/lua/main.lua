-- Fields are decoded from string.byte, which Lua 5.4 and LuaJIT share; every
-- aggregate stays below 2^53, so LuaJIT's doubles hold it exactly.
local byte, min, max = string.byte, math.min, math.max

local FRAME = 12 -- channel:u8 flags:u8 sample:u16 tick:u32 energy:u16 load:u16

local file = assert(io.open(arg[1] or "telemetry.bin", "rb"))
local data = file:read("*a")
file:close()
if #data < 8 or data:sub(1, 4) ~= "EDB1" then
  error("bad input")
end
local h0, h1, h2, h3, c0, c1, c2, c3 = byte(data, 1, 8)
local count = ((c3 * 256 + c2) * 256 + c1) * 256 + c0
if #data < 8 + count * FRAME then
  error("bad input")
end

local checksum = h0 + h1 + h2 + h3 + c0 + c1 + c2 + c3
local valid, sample_max, sample_sum, energy_sum, load_sum, tick_sum = 0, 0, 0, 0, 0, 0
local channels = { 0, 0, 0, 0 }
local sample_min = 65535
local at = 9
for _ = 1, count do
  local channel, flags, s0, s1, t0, t1, t2, t3, e0, e1, l0, l1 = byte(data, at, at + 11)
  at = at + FRAME
  checksum = checksum + channel + flags + s0 + s1 + t0 + t1 + t2 + t3 + e0 + e1 + l0 + l1
  if channel >= 4 then
    error("bad channel")
  end
  if flags == 0 then
    local sample = s1 * 256 + s0
    valid = valid + 1
    channels[channel + 1] = channels[channel + 1] + 1
    sample_min = min(sample_min, sample)
    sample_max = max(sample_max, sample)
    sample_sum = sample_sum + sample
    tick_sum = tick_sum + ((t3 * 256 + t2) * 256 + t1) * 256 + t0
    energy_sum = energy_sum + e1 * 256 + e0
    load_sum = load_sum + l1 * 256 + l0
  end
end
-- Bytes past the last frame still count toward the checksum.
for k = at, #data do
  checksum = checksum + byte(data, k)
end

io.write(string.format(
  "frames %d\nvalid %d\nchannels %d %d %d %d\nsample %d %d %d\nenergy %d\nload %d\nticks %d\nchecksum %d\n",
  count, valid, channels[1], channels[2], channels[3], channels[4],
  sample_min, sample_max, sample_sum, energy_sum, load_sum, tick_sum, checksum))

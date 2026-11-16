local floor, min, max = math.floor, math.min, math.max

-- crc16 shifts the 16 bits of word in low bit first; the polynomial step adds
-- 4129 modulo 65536, as every rail of this entry does. Lua has no portable
-- bit operators (LuaJIT has none of Lua 5.4's), so the shifts are arithmetic.
local function crc16(word)
  local crc, value = 65535, word
  for _ = 1, 16 do
    local top = floor(crc / 32768)
    local incoming = value % 2
    crc = (crc * 2) % 65536
    if top ~= incoming then
      crc = (crc + 4129) % 65536
    end
    value = floor(value / 2)
  end
  return crc
end

-- Divides toward zero, as the other rails' integer division does.
local function signed_div8(value)
  if value >= 0 then
    return floor(value / 8)
  end
  return -floor(-value / 8)
end

local function clamp(value, low, high)
  return min(max(value, low), high)
end

local ticks = tonumber(arg[1])
local ring = {}
for i = 1, 16 do
  ring[i] = 0
end
local ring_position = 1
local running_sum, integrator, actuator, watchdog, watchdog_resets = 0, 0, 0, 0, 0
local accepted, rejected, faults, status_reg, mmio_checksum = 0, 0, 0, 0, 0
local control_reg = 4095

for tick = 0, ticks - 1 do
  local raw = (tick * 73 + 19) % 4096
  local frame = (raw * 17 + tick * 31 + 7) % 65536
  local expected_crc = crc16(frame)
  local received_crc = expected_crc
  if tick % 127 == 0 then
    received_crc = (expected_crc + 1) % 65536
  end
  watchdog = watchdog + 1

  if received_crc ~= expected_crc then
    rejected = rejected + 1
    faults = faults + 1
    status_reg = 2
  else
    local sample = (raw * 3 + 11) % 4096
    local old = ring[ring_position]
    ring[ring_position] = sample
    ring_position = ring_position % 16 + 1
    running_sum = running_sum + sample - old
    local e = 2048 - floor(running_sum / 16)
    integrator = clamp(integrator + e, -8192, 8192)
    actuator = clamp(e * 4 + signed_div8(integrator), -4095, 4095)
    accepted = accepted + 1
    status_reg = 1
  end

  control_reg = actuator + 4095
  mmio_checksum = (mmio_checksum * 33 + status_reg * 257 + control_reg + received_crc) % 1000000007
  if watchdog == 64 then
    watchdog = 0
    watchdog_resets = watchdog_resets + 1
  end
end

local ring_checksum = 0
for i = 1, 16 do
  ring_checksum = (ring_checksum * 131 + ring[i]) % 1000000007
end
io.write(string.format(
  "ticks %d\naccepted %d rejected %d\nwatchdog_resets %d faults %d\nactuator %d\nmmio_checksum %d\nring_checksum %d\n",
  ticks, accepted, rejected, watchdog_resets, faults, actuator, mmio_checksum, ring_checksum))

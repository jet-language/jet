local SIDE = 4096
local ELEMENTS = SIDE * SIDE

local function filled(value)
  local values = {}
  for i = 1, ELEMENTS do
    values[i] = value
  end
  return values
end

local left = filled(1.0)
local right = filled(2.0)
local mapped = {}
for i = 1, ELEMENTS do
  mapped[i] = left[i] + right[i]
end

local corner = mapped[ELEMENTS]
local checksum = 0.0
for i = 1, ELEMENTS do
  checksum = checksum + mapped[i]
end
if corner ~= 3.0 or checksum ~= 50331648.0 then
  error("tensor map validation failed")
end
io.write(string.format("shape:[%d, %d] numel:%d corner:%.1f checksum:%.0f\n", SIDE, SIDE, #mapped, corner, checksum))

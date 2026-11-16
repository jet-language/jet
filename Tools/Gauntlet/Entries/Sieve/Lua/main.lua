-- An odd-only sieve: slot i stands for 2*i + 1, and slot 0 (the number 1) is
-- never counted. The multiples of p = 2*i + 1 start at p*p, slot 2*i*(i + 1).
local n = tonumber(arg[1])
local slots = math.floor(n / 2)
local composite = {}
for i = 1, slots - 1 do
  composite[i] = false
end

local i = 1
while (2 * i + 1) * (2 * i + 1) < n do
  if not composite[i] then
    local p = 2 * i + 1
    for m = 2 * i * (i + 1), slots - 1, p do
      composite[m] = true
    end
  end
  i = i + 1
end

local count, largest = 0, 0
if n > 2 then
  count, largest = 1, 2
  for slot = 1, slots - 1 do
    if not composite[slot] then
      count = count + 1
      largest = 2 * slot + 1
    end
  end
end
io.write(string.format("count %d\nlargest %d\n", count, largest))

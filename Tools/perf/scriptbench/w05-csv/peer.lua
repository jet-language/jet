local count, qty, value = 0, 0, 0
local first = true
for line in io.lines("w05.csv") do
  if first then
    first = false
  else
    local _, _, _, q, price = line:match("^([^,]*),([^,]*),([^,]*),([^,]*),([^,]*)$")
    q = tonumber(q)
    count = count + 1
    qty = qty + q
    value = value + q * tonumber(price)
  end
end
print(string.format("rows=%d qty=%d value=%d", count, qty, value))

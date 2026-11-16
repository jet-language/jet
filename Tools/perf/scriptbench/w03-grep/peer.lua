local count, total = 0, 0
for line in io.lines("data/log100m.txt") do
  local ms = line:match("status=5%d%d ms=(%d+)")
  if ms then
    count = count + 1
    total = total + tonumber(ms)
  end
end
print("matches " .. count)
print("ms_sum " .. total)

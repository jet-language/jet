local REQUEST = '^([0-9.]+) %- %- %[[^%]]+%] "GET /api/[^ ]+ HTTP/1%.1" 5[0-9][0-9] '

local file = io.open(arg[1] or "", "rb")
if not file then os.exit(2) end

local counts = {}
local matches = 0
for line in file:lines() do
  local ip = string.match(line, REQUEST)
  if ip then
    counts[ip] = (counts[ip] or 0) + 1
    matches = matches + 1
  end
end
file:close()

local ranked = {}
for ip, count in pairs(counts) do ranked[#ranked + 1] = {ip = ip, count = count} end
table.sort(ranked, function(a, b)
  if a.count ~= b.count then return a.count > b.count end
  return a.ip < b.ip
end)

local out = {string.format("matches %d\n", matches)}
for i = 1, math.min(5, #ranked) do
  out[#out + 1] = string.format("%d %s\n", ranked[i].count, ranked[i].ip)
end
io.write(table.concat(out))

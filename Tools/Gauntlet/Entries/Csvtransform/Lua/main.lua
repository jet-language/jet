-- Split one RFC 4180 record into fields; quoted fields may hold commas and doubled quotes.
local function split_record(line, fields)
  local i, n, k = 1, #line, 0
  while true do
    k = k + 1
    if string.byte(line, i) == 34 then
      local parts, j = {}, i + 1
      while true do
        local q = string.find(line, '"', j, true)
        parts[#parts + 1] = string.sub(line, j, q - 1)
        if string.byte(line, q + 1) == 34 then
          parts[#parts + 1] = '"'
          j = q + 2
        else
          i = q + 1
          break
        end
      end
      fields[k] = table.concat(parts)
      if i > n then return k end
      i = i + 1
    else
      local comma = string.find(line, ",", i, true)
      if not comma then
        fields[k] = string.sub(line, i)
        return k
      end
      fields[k] = string.sub(line, i, comma - 1)
      i = comma + 1
    end
  end
end

-- An odd quote count means a quoted field runs on past this line.
local function open_quote(line)
  if not string.find(line, '"', 1, true) then return false end
  local _, quotes = string.gsub(line, '"', '"')
  return quotes % 2 == 1
end

local file = assert(io.open(arg[1], "rb"))
local lines = file:lines()
local header = lines()
while header and open_quote(header) do header = header .. "\n" .. lines() end

local groups = {}
local total_count, total_sum = 0, 0.0
local fields = {}
for line in lines do
  while open_quote(line) do line = line .. "\n" .. lines() end
  if line ~= "" then
    split_record(line, fields)
    local amount = tonumber(fields[4])
    if amount and amount > 0.0 then
      local region = fields[2]
      local group = groups[region]
      if not group then
        group = {0, 0.0}
        groups[region] = group
      end
      group[1] = group[1] + 1
      group[2] = group[2] + amount
      total_count = total_count + 1
      total_sum = total_sum + amount
    end
  end
end
file:close()

local regions = {}
for region in pairs(groups) do regions[#regions + 1] = region end
table.sort(regions)
local out = {}
for _, region in ipairs(regions) do
  local group = groups[region]
  out[#out + 1] = string.format("%s n=%d sum=%.2f\n", region, group[1], group[2])
end
out[#out + 1] = string.format("total n=%d sum=%.2f\n", total_count, total_sum)
io.write(table.concat(out))

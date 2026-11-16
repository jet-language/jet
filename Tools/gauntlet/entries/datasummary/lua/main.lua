-- Split one RFC 4180 record into fields; quoted fields may hold commas and doubled quotes.
local function split_record(line)
  local fields = {}
  local i, n = 1, #line
  while true do
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
      fields[#fields + 1] = table.concat(parts)
      if i > n then return fields end
      i = i + 1
    else
      local comma = string.find(line, ",", i, true)
      if not comma then
        fields[#fields + 1] = string.sub(line, i)
        return fields
      end
      fields[#fields + 1] = string.sub(line, i, comma - 1)
      i = comma + 1
    end
  end
end

local function mean_of(values)
  local sum = 0.0
  for i = 1, #values do sum = sum + values[i] end
  return sum / #values
end

local function summary(values)
  local ordered = {}
  for i = 1, #values do ordered[i] = values[i] end
  table.sort(ordered)
  local n = #ordered
  local mean = mean_of(values)
  local median = ordered[math.floor((n - 1) / 2) + 1]
  local p95 = ordered[math.floor((19 * n + 19) / 20)]
  local squares = 0.0
  for i = 1, n do
    local delta = values[i] - mean
    squares = squares + delta * delta
  end
  return string.format("n=%d mean=%.2f median=%.2f p95=%.2f sd=%.2f",
    n, mean, median, p95, math.sqrt(squares / n))
end

local file = assert(io.open(arg[1] or "measurements.csv", "rb"))
local lines = file:lines()
local headers = split_record(lines())
local group_index, value_index
for index, name in ipairs(headers) do
  if name == "group" then group_index = index end
  if name == "value" then value_index = index end
end
local groups = {}
for line in lines do
  line = string.gsub(line, "\r$", "")
  if line ~= "" then
    local row = split_record(line)
    local group = row[group_index]
    local values = groups[group]
    if not values then
      values = {}
      groups[group] = values
    end
    values[#values + 1] = tonumber(row[value_index])
  end
end
file:close()

local names = {}
for name in pairs(groups) do names[#names + 1] = name end
table.sort(names)
local out = {}
local all_values = {}
for _, name in ipairs(names) do
  local values = groups[name]
  for i = 1, #values do all_values[#all_values + 1] = values[i] end
  out[#out + 1] = name .. " " .. summary(values) .. "\n"
end
out[#out + 1] = string.format("overall n=%d mean=%.2f\n", #all_values, mean_of(all_values))
io.write(table.concat(out))

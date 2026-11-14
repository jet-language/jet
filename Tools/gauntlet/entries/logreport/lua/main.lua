-- Lua's standard library has no UTC calendar conversion (os.time reads local time),
-- so RFC 3339 timestamps go through the civil-date day count.
local floor = math.floor

local function days_from_civil(y, m, d)
  if m <= 2 then y = y - 1 end
  local era = floor(y / 400)
  local yoe = y - era * 400
  local mp = (m + 9) % 12
  local doy = floor((153 * mp + 2) / 5) + d - 1
  local doe = yoe * 365 + floor(yoe / 4) - floor(yoe / 100) + doy
  return era * 146097 + doe - 719468
end

local function parse_rfc3339(text)
  local y, mo, d, h, mi, s, zone = string.match(text,
    "^(%d%d%d%d)%-(%d%d)%-(%d%d)[Tt](%d%d):(%d%d):(%d%d)%.?%d*(.*)$")
  if not y then error("bad timestamp: " .. text) end
  local offset = 0
  if zone ~= "Z" and zone ~= "z" then
    local sign, oh, om = string.match(zone, "^([+-])(%d%d):(%d%d)$")
    if not sign then error("bad timestamp: " .. text) end
    offset = (tonumber(oh) * 60 + tonumber(om)) * 60
    if sign == "-" then offset = -offset end
  end
  local days = days_from_civil(tonumber(y), tonumber(mo), tonumber(d))
  return days * 86400 + tonumber(h) * 3600 + tonumber(mi) * 60 + tonumber(s) - offset
end

local levels = {"DEBUG", "INFO", "WARN", "ERROR"}
local level_counts = {}
local error_counts = {}
local first_text, last_text = "", ""
local first_time, last_time = 0, 0
local total = 0

local file = assert(io.open(arg[1], "rb"))
for line in file:lines() do
  local timestamp_text, level, component = string.match(line, "^([^ ]*) ([^ ]*) ([^ ]*)")
  local timestamp = parse_rfc3339(timestamp_text)
  if total == 0 then first_text, first_time = timestamp_text, timestamp end
  last_text, last_time = timestamp_text, timestamp
  level_counts[level] = (level_counts[level] or 0) + 1
  if level == "ERROR" then error_counts[component] = (error_counts[component] or 0) + 1 end
  total = total + 1
end
file:close()

local out = {}
for _, level in ipairs(levels) do
  out[#out + 1] = string.format("%s %d\n", level, level_counts[level] or 0)
end
out[#out + 1] = "top-error-components:\n"
local ranked = {}
for name, count in pairs(error_counts) do ranked[#ranked + 1] = {name = name, count = count} end
table.sort(ranked, function(a, b)
  if a.count ~= b.count then return a.count > b.count end
  return a.name < b.name
end)
for i = 1, math.min(3, #ranked) do
  out[#out + 1] = string.format("%d %s\n", ranked[i].count, ranked[i].name)
end
out[#out + 1] = string.format("span %s .. %s (%ds)\n", first_text, last_text, last_time - first_time)
io.write(table.concat(out))

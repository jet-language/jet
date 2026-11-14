-- Lua's standard library has no UTC calendar conversion (os.time and os.date read
-- local time), so offsets normalize through the civil-date day count.
local floor = math.floor
local DAYS = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"}

local function days_from_civil(y, m, d)
  if m <= 2 then y = y - 1 end
  local era = floor(y / 400)
  local yoe = y - era * 400
  local mp = (m + 9) % 12
  local doy = floor((153 * mp + 2) / 5) + d - 1
  local doe = yoe * 365 + floor(yoe / 4) - floor(yoe / 100) + doy
  return era * 146097 + doe - 719468
end

local function civil_from_days(z)
  z = z + 719468
  local era = floor(z / 146097)
  local doe = z - era * 146097
  local yoe = floor((doe - floor(doe / 1460) + floor(doe / 36524) - floor(doe / 146096)) / 365)
  local doy = doe - (365 * yoe + floor(yoe / 4) - floor(yoe / 100))
  local mp = floor((5 * doy + 2) / 153)
  local d = doy - floor((153 * mp + 2) / 5) + 1
  local m = mp < 10 and mp + 3 or mp - 9
  local y = yoe + era * 400
  if m <= 2 then y = y + 1 end
  return y, m, d
end

local function parse_rfc3339(text)
  local y, mo, d, h, mi, s, zone = string.match(text,
    "^(%d%d%d%d)%-(%d%d)%-(%d%d)[Tt](%d%d):(%d%d):(%d%d)%.?%d*(.*)$")
  if not y then error("invalid timestamp " .. text) end
  local offset = 0
  if zone ~= "Z" and zone ~= "z" then
    local sign, oh, om = string.match(zone, "^([+-])(%d%d):(%d%d)$")
    if not sign then error("invalid timestamp " .. text) end
    offset = (tonumber(oh) * 60 + tonumber(om)) * 60
    if sign == "-" then offset = -offset end
  end
  local days = days_from_civil(tonumber(y), tonumber(mo), tonumber(d))
  return days * 86400 + tonumber(h) * 3600 + tonumber(mi) * 60 + tonumber(s) - offset
end

local meetings = {}
local file = assert(io.open(arg[1] or "meetings.txt", "rb"))
for line in file:lines() do
  local name, raw = string.match(line, "^([^|]*)|(.*)$")
  if name then meetings[#meetings + 1] = {name = name, utc = parse_rfc3339(raw)} end
end
file:close()
table.sort(meetings, function(a, b)
  if a.utc ~= b.utc then return a.utc < b.utc end
  return a.name < b.name
end)

local out = {}
for position, meeting in ipairs(meetings) do
  local days = floor(meeting.utc / 86400)
  local seconds = meeting.utc - days * 86400
  local y, m, d = civil_from_days(days)
  local gap = position == 1 and "-" or tostring(floor((meeting.utc - meetings[position - 1].utc) / 60))
  out[#out + 1] = string.format("%s utc=%04d-%02d-%02dT%02d:%02d:%02dZ day=%s gap=%s\n",
    meeting.name, y, m, d, floor(seconds / 3600), floor(seconds % 3600 / 60), seconds % 60,
    DAYS[(days + 4) % 7 + 1], gap)
end
out[#out + 1] = string.format("span %d minutes %d\n",
  floor((meetings[#meetings].utc - meetings[1].utc) / 60), #meetings)
io.write(table.concat(out))

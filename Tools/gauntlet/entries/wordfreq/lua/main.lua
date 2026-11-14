local file = assert(io.open(arg[1], "rb"))
local data = file:read("*a")
file:close()
-- Lua 5.4 checks the input is UTF-8 as the other rails do; LuaJIT has no utf8
-- library.
if utf8 and not utf8.len(data) then
  error("input is not valid UTF-8")
end

-- %S+ splits on the C locale's ASCII whitespace: space and \t \n \v \f \r.
local counts = {}
local total = 0
for word in data:gmatch("%S+") do
  counts[word] = (counts[word] or 0) + 1
  total = total + 1
end

local words = {}
for word in pairs(counts) do
  words[#words + 1] = word
end
table.sort(words, function(a, b)
  local ca, cb = counts[a], counts[b]
  if ca ~= cb then
    return ca > cb
  end
  return a < b
end)

local lines = {}
for i = 1, math.min(20, #words) do
  lines[i] = string.format("%d %s\n", counts[words[i]], words[i])
end
io.write(table.concat(lines))
io.write(string.format("distinct %d total %d\n", #words, total))

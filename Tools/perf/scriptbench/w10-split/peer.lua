local lines = {}
for i = 0, 19999 do lines[#lines + 1] = "alpha beta gamma delta epsilon " .. i end
local text = table.concat(lines, "\n")
local count, long, bytes = 0, 0, 0
for line in (text .. "\n"):gmatch("([^\n]*)\n") do
  for word in line:gmatch("[^ ]+") do
    count = count + 1
    bytes = bytes + #word
    if #word > 4 then long = long + 1 end
  end
end
print(string.format("words=%d long=%d bytes=%d", count, long, bytes))

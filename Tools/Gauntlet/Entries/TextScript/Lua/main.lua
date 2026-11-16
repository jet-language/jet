local file = assert(io.open(arg[1] or "notes.txt", "rb"))
local lines = {}
for line in file:lines() do
  local cleaned = string.match(line, "^%s*(.-)%s*$")
  if cleaned ~= "" then lines[#lines + 1] = cleaned end
end
file:close()
table.sort(lines)
io.write(string.format("lines %d\n", #lines), table.concat(lines, "\n"), "\n")

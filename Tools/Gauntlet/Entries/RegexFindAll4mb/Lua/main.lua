local file = assert(io.open(arg[1] or "corpus.txt", "rb"))
local text = file:read("*a")
file:close()

-- Non-overlapping literal matches: a plain find (no pattern magic) that
-- resumes after each match.
local find = string.find
local matches = 0
local at = 1
while true do
  local _, last = find(text, "struct", at, true)
  if not last then
    break
  end
  matches = matches + 1
  at = last + 1
end
io.write(string.format("matches %d\n", matches))

local counts, total, distinct = {}, 0, 0
for line in io.lines("data/words.txt") do
  for word in line:lower():gmatch("%a+") do
    local n = counts[word]
    if not n then distinct = distinct + 1 end
    counts[word] = (n or 0) + 1
    total = total + 1
  end
end
local rows = {}
for word, n in pairs(counts) do rows[#rows + 1] = { word = word, n = n } end
table.sort(rows, function(a, b)
  if a.n ~= b.n then return a.n > b.n end
  return a.word < b.word
end)
print("total " .. total)
print("distinct " .. distinct)
for i = 1, 10 do print(rows[i].n .. " " .. rows[i].word) end

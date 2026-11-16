local seen, unique = {}, 0
for line in io.lines("w13.txt") do
  if not seen[line] then
    seen[line] = true
    unique = unique + 1
  end
end
print(string.format("unique=%d", unique))

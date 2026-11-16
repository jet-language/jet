local total = 0
for i = 1, 30000000 do total = total + i * i % 7919 end
print(string.format("%d", total))

total = 0
1.upto(30_000_000) { |i| total += i * i % 7919 }
puts total

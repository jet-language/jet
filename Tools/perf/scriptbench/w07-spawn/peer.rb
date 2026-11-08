ok = 0
100.times { ok += 1 if system("true") }
puts "children ok=#{ok}"

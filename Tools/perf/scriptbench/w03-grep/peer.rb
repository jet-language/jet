count = 0
total = 0
File.foreach("data/log100m.txt") do |line|
  if (m = line.match(/status=5\d\d ms=(\d+)/))
    count += 1
    total += m[1].to_i
  end
end
puts "matches #{count}"
puts "ms_sum #{total}"

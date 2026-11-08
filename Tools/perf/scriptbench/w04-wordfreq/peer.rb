counts = Hash.new(0)
File.foreach("data/words.txt") do |line|
  line.downcase.scan(/[a-z]+/) { |word| counts[word] += 1 }
end
puts "total #{counts.values.sum}"
puts "distinct #{counts.size}"
counts.sort_by { |word, n| [-n, word] }.first(10).each { |word, n| puts "#{n} #{word}" }

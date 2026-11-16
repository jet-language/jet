require "set"

seen = Set.new
File.foreach("w13.txt", chomp: true) { |line| seen << line }
puts "unique=#{seen.size}"

require "net/http"

total = 0
200.times do |i|
  total += Net::HTTP.get(URI("http://127.0.0.1:18080/item/#{i}")).bytesize
end
puts "requests 200"
puts "bytes #{total}"

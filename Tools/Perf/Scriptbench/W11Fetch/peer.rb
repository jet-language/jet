require "net/http"

urls = (0...100).map { |i| URI("http://127.0.0.1:18411/item/#{i}") }
queue = Queue.new
urls.each_with_index { |url, i| queue << [url, i] }
codes = Array.new(urls.length)
workers = Array.new(32) do
  Thread.new do
    loop do
      url, i = queue.pop(true) rescue break
      codes[i] = Net::HTTP.get_response(url).code.to_i
    end
  end
end
workers.each(&:join)
puts "ok=#{codes.count(200)}"

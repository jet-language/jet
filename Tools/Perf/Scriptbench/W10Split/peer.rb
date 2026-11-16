text = (0...20000).map { |i| "alpha beta gamma delta epsilon #{i}" }.join("\n")
count = long = bytes = 0
text.split("\n").each do |line|
  line.split(" ").each do |word|
    count += 1
    bytes += word.length
    long += 1 if word.length > 4
  end
end
puts "words=#{count} long=#{long} bytes=#{bytes}"

files = 0
bytes = 0
Dir.glob("w02/**/*") do |path|
  next unless File.file?(path)
  files += 1
  bytes += File.size(path)
end
puts "files=#{files} bytes=#{bytes}"

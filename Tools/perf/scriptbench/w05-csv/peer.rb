count = qty = value = 0
File.open("w05.csv") do |handle|
  handle.gets
  handle.each_line do |line|
    fields = line.chomp.split(",")
    q = fields[3].to_i
    count += 1
    qty += q
    value += q * fields[4].to_i
  end
end
puts "rows=#{count} qty=#{qty} value=#{value}"

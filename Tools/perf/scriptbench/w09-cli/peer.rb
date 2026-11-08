require "optparse"

command = ARGV.shift
opts = { times: 1, shout: false }
case command
when "greet"
  OptionParser.new do |o|
    o.on("--name NAME") { |v| opts[:name] = v }
    o.on("--times N", Integer) { |v| opts[:times] = v }
    o.on("--shout") { opts[:shout] = true }
  end.parse!
  abort "--name is required" unless opts[:name]
  text = "Hello, #{opts[:name]}!"
  opts[:times].times { puts opts[:shout] ? text.upcase : text }
when "add"
  OptionParser.new do |o|
    o.on("--a N", Integer) { |v| opts[:a] = v }
    o.on("--b N", Integer) { |v| opts[:b] = v }
  end.parse!
  puts opts[:a] + opts[:b]
else
  abort "usage: main <greet|add> [options]"
end

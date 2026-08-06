# Emit the Ruby comparison surface for the Core surface ledger.
#
# Every "Ruby has this" claim resolves against this snapshot, which is read
# from a real interpreter, so a constructed member name is never evidence.
#
# Regenerate:
#   nix shell nixpkgs#ruby --command ruby scripts/agent/surface-ruby.rb \
#       > docs/reference/surfaces/ruby-surface.json

require "json"
require "set"
require "date"
require "time"
require "json"
require "csv"
require "uri"
require "securerandom"
require "digest"
require "base64"
require "fileutils"
require "pathname"
require "tempfile"
require "socket"
require "openssl"
require "net/http"
require "zlib"
require "stringio"
require "logger"
require "benchmark"

# Canonical Jet-facing container names map to the Ruby class or module that
# holds the same workflow. Instance methods and singleton methods are both
# operations; a container Ruby does not ship is recorded as absent, never
# omitted, so the gap stays countable.
INSTANCE = {
  "List" => [Array, Enumerable],
  "Iter" => [Enumerator, Enumerator::Lazy],
  "Map" => [Hash],
  "Set" => [Set],
  "String" => [String],
  "ByteBuffer" => [StringIO],
  "Deque" => [Thread::Queue],
  "core.time" => [Time, Date, DateTime],
  "core.regex" => [Regexp, MatchData],
  "core.url" => [URI::Generic],
  "core.path" => [Pathname],
  "core.net" => [Socket, TCPSocket],
  "core.log" => [Logger],
}.freeze

SINGLETON = {
  "core.math" => [Math],
  "core.random" => [Random],
  "core.crypto.random" => [SecureRandom],
  "core.encoding.json" => [JSON],
  "core.encoding.csv" => [CSV],
  "core.encoding.base64" => [Base64],
  "core.crypto" => [Digest],
  "core.os" => [Process],
  "core.files" => [Dir],
  "core.path" => [File],
  "core.archive" => [Zlib],
  "core.tls" => [OpenSSL::SSL],
  "core.http" => [Net::HTTP],
  "core.testing" => [Benchmark],
  "core.io" => [IO],
}.freeze

ABSENT = {
  "core.fmt" => "formatting lives on Kernel#format and String#%, which Object and Kernel provide to every class",
  "core.binary" => "binary reading lives on IO, recorded under core.io",

  "core.env" => "environment access lives on Process and ENV, recorded under core.os",
  "core.process" => "process control lives on Process, recorded under core.os",

  "SortedSet" => "SortedSet left the Ruby standard library in Ruby 3.0",
  "PriorityQueue" => "no Ruby standard-library priority queue",
  "BitSet" => "no Ruby standard-library bit set; Integer carries bit operations",
  "Cache" => "no Ruby standard-library cache with an eviction policy",
  "core.data" => "no Ruby standard-library statistics",
  "core.db" => "no Ruby standard-library database client",
  "core.encoding.base32" => "no Ruby standard-library base32 codec",
  "core.encoding.hex" => "pack and unpack carry hex, but Ruby ships no hex codec module",
  "core.encoding.toml" => "no Ruby standard-library TOML decoder",
  "core.tasks" => "Thread and Fiber are core classes, but Ruby ships no task or async module",
  "core.text" => "String carries text handling; Ruby ships no separate text module",
  "core.text.unicode" => "no Ruby standard-library Unicode property database",
  "core.uuid" => "SecureRandom.uuid exists, but Ruby ships no UUID module",
}.freeze

INHERITED = (
  Object.instance_methods + Kernel.instance_methods +
  Kernel.private_instance_methods + Object.private_instance_methods
).map(&:to_s).to_set.freeze

def strip_inherited(names)
  names.reject { |n| INHERITED.include?(n) }
end

def instance_ops(mods)
  strip_inherited(mods.flat_map { |m| m.instance_methods(false) }.map(&:to_s).uniq).sort
end

def singleton_ops(mods)
  names = mods.flat_map { |m| m.singleton_methods(false) + (m.is_a?(Module) ? m.instance_methods(false) : []) }
              .map(&:to_s).uniq
  strip_inherited(names).sort
end

containers = {}
INSTANCE.each { |name, mods| containers[name] = { "present" => true, "operations" => instance_ops(mods) } }
SINGLETON.each do |name, mods|
  ops = singleton_ops(mods)
  containers[name] = { "present" => true, "operations" => ops }
end
ABSENT.each { |name, reason| containers[name] = { "present" => false, "reason" => reason, "operations" => [] } }

puts JSON.pretty_generate({
  "language" => "Ruby",
  "sourceKind" => "runtime introspection",
  "runtime" => "ruby #{RUBY_VERSION}",
  "scopeRule" => "Methods defined directly on the class or module that holds each workflow, minus every name Object and Kernel already provide. Ruby redefines dup, freeze, hash, inspect and eql? on many classes, so counting them scored one inherited protocol as a gap in every container that redefines it.",
  "officialReferences" => ["https://ruby-doc.org/3.4.1/"],
  "containers" => containers,
  "totals" => {
    "containers" => containers.size,
    "presentContainers" => containers.count { |_, v| v["present"] },
    "operations" => containers.sum { |_, v| v["operations"].size },
  },
})

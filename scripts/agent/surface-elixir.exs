# Emit the Elixir comparison surface for the Core surface ledger.
#
# Every "Elixir has this" claim resolves against this snapshot, which is read
# from a real runtime, so a constructed member name is never evidence. Erlang
# modules reachable from Elixir are included where Elixir ships no module of
# its own for that workflow; the recorded container names which one answered.
#
# Regenerate:
#   nix shell nixpkgs#elixir --command elixir scripts/agent/surface-elixir.exs \
#       > docs/reference/surfaces/elixir-surface.json

containers = [
  {"List", [List, Enum]},
  {"Iter", [Stream]},
  {"Map", [Map, Keyword]},
  {"Set", [MapSet]},
  {"SortedSet", [:gb_sets]},
  {"Deque", [:queue]},
  {"String", [String]},
  {"ByteBuffer", [:binary]},
  {"core.math", [:math, Integer, Float]},
  {"core.random", [:rand]},
  {"core.crypto.random", [:crypto]},
  {"core.crypto", [:crypto]},
  {"core.time", [DateTime, Date, Time, NaiveDateTime, Calendar]},
  {"core.encoding.json", [JSON]},
  {"core.encoding.base64", [Base]},
  {"core.encoding.base32", [Base]},
  {"core.encoding.hex", [Base]},
  {"core.regex", [Regex]},
  {"core.files", [File]},
  {"core.path", [Path]},
  {"core.env", [System]},
  {"core.os", [System]},
  {"core.process", [System, Port]},
  {"core.net", [:gen_tcp, :inet]},
  {"core.tls", [:ssl]},
  {"core.http", [:httpc]},
  {"core.url", [URI]},
  {"core.tasks", [Task, Agent, GenServer, Process]},
  {"core.testing", [ExUnit, ExUnit.Assertions]},
  {"core.log", [Logger]},
  {"core.binary", [:binary, Bitwise]},
  {"core.archive", [:zip, :zlib]},
  {"core.io", [IO]},
  {"core.fmt", [Kernel, Inspect]},
  {"core.text", [String]},
  {"core.text.unicode", [String.Unicode]}
]

absent = [
  {"PriorityQueue", "no Elixir or Erlang standard-library priority queue"},
  {"BitSet", "no Elixir standard-library bit set; bitstrings carry bit operations"},
  {"Cache", "no Elixir standard-library cache with an eviction policy"},
  {"core.uuid", "no Elixir standard-library UUID generator"},
  {"core.db", "no Elixir standard-library database client"},
  {"core.data", "no Elixir standard-library statistics module"},
  {"core.encoding.csv", "no Elixir standard-library CSV codec"},
  {"core.encoding.toml", "no Elixir standard-library TOML decoder"}
]

exports = fn module ->
  if Code.ensure_loaded?(module) do
    functions =
      if function_exported?(module, :__info__, 1) do
        module.__info__(:functions) ++ module.__info__(:macros)
      else
        module.module_info(:exports)
      end

    functions
    |> Enum.map(fn {name, _arity} -> Atom.to_string(name) end)
    |> Enum.reject(&String.starts_with?(&1, "_"))
    |> Enum.reject(&(&1 in ["module_info", "behaviour_info"]))
    |> Enum.uniq()
    |> Enum.sort()
  else
    :missing
  end
end

present =
  for {name, modules} <- containers, into: %{} do
    results = Enum.map(modules, fn m -> {m, exports.(m)} end)
    missing = for {m, :missing} <- results, do: inspect(m)
    ops = results |> Enum.flat_map(fn {_, r} -> if r == :missing, do: [], else: r end) |> Enum.uniq() |> Enum.sort()

    if ops == [] do
      raise "no operations found for container #{name}; modules=#{inspect(modules)}"
    end

    {name,
     %{
       "present" => true,
       "modules" => Enum.map(modules, &inspect/1),
       "unloadableModules" => missing,
       "operations" => ops
     }}
  end

missing_map =
  for {name, reason} <- absent, into: %{} do
    {name, %{"present" => false, "reason" => reason, "operations" => []}}
  end

all = Map.merge(present, missing_map)

IO.puts(
  JSON.encode!(%{
    "language" => "Elixir",
    "sourceKind" => "runtime introspection",
    "runtime" => "elixir #{System.version()} on erlang/otp #{System.otp_release()}",
    "scopeRule" =>
      "Exported functions and macros of the modules that hold each workflow. Private and reflection entries (leading underscore, module_info, behaviour_info) are excluded because they are not workflow operations.",
    "officialReferences" => ["https://hexdocs.pm/elixir/Enum.html", "https://hexdocs.pm/elixir/api-reference.html"],
    "containers" => all,
    "totals" => %{
      "containers" => map_size(all),
      "presentContainers" => map_size(present),
      "operations" => present |> Enum.map(fn {_, v} -> length(v["operations"]) end) |> Enum.sum()
    }
  })
)

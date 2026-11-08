local command = arg[1]
local opts = {}
local i = 2
while i <= #arg do
  local key = arg[i]:match("^%-%-(.+)$")
  if not key then error("unexpected argument " .. arg[i]) end
  local value = arg[i + 1]
  if value == nil or value:match("^%-%-") then
    opts[key] = true
    i = i + 1
  else
    opts[key] = value
    i = i + 2
  end
end

if command == "greet" then
  local text = "Hello, " .. assert(opts.name, "--name is required") .. "!"
  for _ = 1, tonumber(opts.times or 1) do
    print(opts.shout and text:upper() or text)
  end
elseif command == "add" then
  print(math.tointeger(tonumber(opts.a) + tonumber(opts.b)) or tonumber(opts.a) + tonumber(opts.b))
else
  io.stderr:write("usage: main <greet|add> [options]\n")
  os.exit(2)
end

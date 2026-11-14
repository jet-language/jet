-- A single-process HTTP/1.1 key-value service over a LuaSocket TCP listener;
-- Lua's standard library has no sockets, and LuaSocket has no HTTP server.
local socket = require("socket")
local cjson = require("cjson")

local REASONS = {[200] = "OK", [404] = "Not Found"}
local NOT_FOUND = '{"error":"not found"}'

local function reply(client, status, body)
  client:send(string.format(
    "HTTP/1.1 %d %s\r\nContent-Type: application/json\r\nContent-Length: %d\r\nConnection: close\r\n\r\n%s",
    status, REASONS[status], #body, body))
end

-- Serve one request; returns true once /shutdown has been answered.
local function handle(client, values)
  local request = client:receive("*l")
  if not request then return false end
  local method, target = string.match(request, "^(%S+) (%S+) HTTP/%d%.%d$")
  if not method then return false end
  local path = string.match(target, "^[^?]*")
  local content_length = 0
  while true do
    local header = client:receive("*l")
    if not header then return false end
    if header == "" then break end
    local name, value = string.match(header, "^([^:]+):%s*(.-)%s*$")
    if name and string.lower(name) == "content-length" then
      content_length = tonumber(value) or 0
    end
  end
  local body = ""
  if content_length > 0 then
    body = client:receive(content_length)
    if not body then return false end
  end

  local key = string.match(path, "^/kv/(.*)$")
  if method == "GET" and path == "/health" then
    reply(client, 200, '{"status":"ok"}')
  elseif method == "GET" and path == "/shutdown" then
    reply(client, 200, '{"bye":true}')
    return true
  elseif method == "GET" and key then
    local value = values[key]
    if value then
      reply(client, 200, string.format('{"key":%s,"value":%s}', cjson.encode(key), cjson.encode(value)))
    else
      reply(client, 404, NOT_FOUND)
    end
  elseif method == "PUT" and key then
    values[key] = body
    reply(client, 200, string.format('{"stored":%s}', cjson.encode(key)))
  else
    reply(client, 404, NOT_FOUND)
  end
  return false
end

local server = assert(socket.bind("127.0.0.1", tonumber(arg[1])))
local values = {}
local stopping = false
while not stopping do
  local client = server:accept()
  if client then
    stopping = handle(client, values)
    client:close()
  end
end
server:close()

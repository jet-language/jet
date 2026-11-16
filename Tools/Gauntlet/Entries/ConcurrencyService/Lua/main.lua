-- A line-oriented TCP worker service over a LuaSocket listener (Lua's standard
-- library has no sockets). Lua has no threads, so the four workers are coroutines
-- driven by a scheduler that sleeps until the next worker is due.
local socket = require("socket")

local WORKERS = 4
local QUEUE_LIMIT = 32
local jobs = {first = 1, last = 0}

local function next_job()
  if jobs.first > jobs.last then return nil end
  local item = jobs[jobs.first]
  jobs[jobs.first] = nil
  jobs.first = jobs.first + 1
  return item
end

-- A worker yields false while idle and its wake time while sleeping on a job.
local function worker()
  while true do
    local item = next_job()
    if item == nil then
      coroutine.yield(false)
    else
      coroutine.yield(socket.gettime() + 0.001)
      local results = item.results
      results[#results + 1] = item.value * item.value
    end
  end
end

local workers = {}
for i = 1, WORKERS do workers[i] = {co = coroutine.create(worker), wake = false} end

local function run_batch(count)
  local results = {}
  for value = 1, count do
    jobs.last = jobs.last + 1
    jobs[jobs.last] = {value = value, results = results}
  end
  while #results < count do
    local now = socket.gettime()
    local earliest = math.huge
    for _, w in ipairs(workers) do
      local due = w.wake and w.wake <= now
      if due or (not w.wake and jobs.first <= jobs.last) then
        local ok, wake = coroutine.resume(w.co)
        if not ok then error(wake) end
        w.wake = wake
      end
      if w.wake and w.wake < earliest then earliest = w.wake end
    end
    if #results < count and earliest > now then socket.sleep(earliest - now) end
  end
  local total = 0
  for _, square in ipairs(results) do total = total + square end
  return total
end

local function serve(client)
  local line = client:receive("*l")
  if not line then return false end
  local command = string.match(line, "^%s*(.-)%s*$")
  local response = "error"
  local stop = false
  if command == "ready" then
    response = "ready"
  elseif command == "shutdown" then
    response = "bye"
    stop = true
  else
    local count = tonumber(string.match(command, "^batch ([+-]?%d+)$"))
    if count and count > 0 and count <= QUEUE_LIMIT then
      response = string.format("batch %d total %d", count, run_batch(count))
    end
  end
  client:send(response .. "\n")
  return stop
end

local listener = assert(socket.bind("127.0.0.1", tonumber(arg[1])))
local stopping = false
while not stopping do
  local client = listener:accept()
  if client then
    stopping = serve(client)
    client:close()
  end
end
listener:close()

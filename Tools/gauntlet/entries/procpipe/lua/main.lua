-- luaposix process control: Lua's standard library starts children only through a
-- shell (os.execute, io.popen) and has no pipelines of argv lists or timeouts.
local unistd = require("posix.unistd")
local wait = require("posix.sys.wait")
local signal = require("posix.signal")
local ptime = require("posix.time")
local fcntl = require("posix.fcntl")

-- Fork and exec argv with stdin/stdout redirected; `close` lists the parent's pipe
-- ends the child must not hold open.
local function spawn(argv, stdin_fd, stdout_fd, close)
  local pid = assert(unistd.fork())
  if pid == 0 then
    if stdin_fd then unistd.dup2(stdin_fd, unistd.STDIN_FILENO) end
    if stdout_fd then unistd.dup2(stdout_fd, unistd.STDOUT_FILENO) end
    for _, fd in ipairs(close) do unistd.close(fd) end
    local args = {}
    for i = 2, #argv do args[i - 1] = argv[i] end
    unistd.execp(argv[1], args)
    unistd._exit(127)
  end
  return pid
end

local function read_all(fd)
  local chunks = {}
  while true do
    local chunk = assert(unistd.read(fd, 4096))
    if chunk == "" then break end
    chunks[#chunks + 1] = chunk
  end
  unistd.close(fd)
  return table.concat(chunks)
end

-- Run argv to completion, capturing stdout; returns the output and exit code.
local function run(argv)
  local r, w = assert(unistd.pipe())
  local pid = spawn(argv, nil, w, {r, w})
  unistd.close(w)
  local output = read_all(r)
  local _, how, code = wait.wait(pid)
  if how ~= "exited" then error(argv[1] .. " ended by " .. how) end
  return output, code
end

local function trim(text)
  return string.match(text, "^%s*(.-)%s*$")
end

local function monotonic()
  local now = ptime.clock_gettime(ptime.CLOCK_MONOTONIC)
  return now.tv_sec + now.tv_nsec / 1e9
end

local calc, calc_code = run({"python3", "-c", "print(6*7)"})
if calc_code ~= 0 then error("calc failed") end
print("calc " .. trim(calc))

local source_r, source_w = assert(unistd.pipe())
local sorted_r, sorted_w = assert(unistd.pipe())
local all = {source_r, source_w, sorted_r, sorted_w}
local source = spawn({"python3", "-c", "print('b');print('a');print('c')"}, nil, source_w, all)
local sorter = spawn({"sort"}, source_r, sorted_w, all)
unistd.close(source_r)
unistd.close(source_w)
unistd.close(sorted_w)
local sorted_output = read_all(sorted_r)
wait.wait(source)
wait.wait(sorter)
local sorted_lines = {}
for line in string.gmatch(trim(sorted_output), "[^\n]+") do sorted_lines[#sorted_lines + 1] = line end
print("sorted " .. table.concat(sorted_lines, ","))

local null = assert(fcntl.open("/dev/null", fcntl.O_WRONLY))
local slow = spawn({"python3", "-c", "import time;time.sleep(5)"}, nil, null, {null})
unistd.close(null)
local deadline = monotonic() + 0.3
local timed_out = false
while true do
  local done = wait.wait(slow, wait.WNOHANG)
  if done ~= 0 then break end
  if monotonic() >= deadline then
    signal.kill(slow, signal.SIGKILL)
    wait.wait(slow)
    timed_out = true
    break
  end
  ptime.nanosleep({tv_sec = 0, tv_nsec = 5000000})
end
if timed_out then print("slow timeout") end

local _, exit_code = run({"python3", "-c", "import sys;sys.exit(3)"})
if exit_code ~= 0 then print("exit " .. exit_code) end

-- lua-cjson keeps the store; Lua's standard library has no JSON.
local cjson = require("cjson")

local STORE = "tasks.json"

local function load()
  local file = io.open(STORE, "rb")
  if not file then return {tasks = {}} end
  local store = cjson.decode(file:read("*a"))
  file:close()
  return store
end

local function save(store)
  local file = assert(io.open(STORE, "wb"))
  file:write(cjson.encode(store))
  file:close()
end

local command = arg[1] or ""
local store = load()
local tasks = store.tasks
if command == "add" then
  local text = arg[2] or ""
  local task = {id = #tasks + 1, text = text, done = false}
  tasks[#tasks + 1] = task
  save(store)
  print(string.format("added %d %s", task.id, text))
elseif command == "done" then
  local task_id = tonumber(arg[2] or "-1")
  for _, task in ipairs(tasks) do
    if task.id == task_id then
      task.done = true
      save(store)
      print(string.format("done %d", task_id))
      return
    end
  end
  print(string.format("no task %d", task_id))
elseif command == "list" then
  local open_count = 0
  local lines = {}
  for _, task in ipairs(tasks) do
    if not task.done then open_count = open_count + 1 end
    lines[#lines + 1] = string.format("[%s] %d %s\n", task.done and "x" or " ", task.id, task.text)
  end
  io.write(table.concat(lines))
  print(string.format("open %d done %d", open_count, #tasks - open_count))
end

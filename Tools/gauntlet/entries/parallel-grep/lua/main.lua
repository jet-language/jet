-- LuaFileSystem walks the tree: Lua's standard library cannot list a directory.
-- Lua has no threads, so the scan runs on its one interpreter thread.
local lfs = require("lfs")

local root = arg[1] or "files"
local needle = arg[2] or "needle-7f"

local function collect(directory, files)
  for name in lfs.dir(directory) do
    if name ~= "." and name ~= ".." then
      local full = directory .. "/" .. name
      local mode = lfs.symlinkattributes(full, "mode")
      if mode == "directory" then
        collect(full, files)
      elseif mode == "file" and string.sub(name, -4) == ".txt" then
        files[#files + 1] = full
      end
    end
  end
  return files
end

-- Non-overlapping plain occurrences; the needle holds no line break, so the
-- whole-text count equals the sum of per-line counts.
local function count_occurrences(text)
  if needle == "" then return 0 end
  local count, offset = 0, 1
  while true do
    local _, last = string.find(text, needle, offset, true)
    if not last then return count end
    count = count + 1
    offset = last + 1
  end
end

local files = collect(root, {})
table.sort(files)
local out = {}
local matched, total = 0, 0
for _, path in ipairs(files) do
  local file = assert(io.open(path, "rb"))
  local count = count_occurrences(file:read("*a"))
  file:close()
  if count > 0 then
    matched = matched + 1
    total = total + count
    out[#out + 1] = string.format("%s:%d\n", path, count)
  end
end
out[#out + 1] = string.format("files %d/%d total %d\n", matched, #files, total)
io.write(table.concat(out))

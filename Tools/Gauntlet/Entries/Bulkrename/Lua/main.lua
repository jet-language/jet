-- LuaFileSystem lists the directory: Lua's standard library cannot. os.rename renames.
local lfs = require("lfs")

local directory = arg[1] or "photos"
local photos = {}
local files = 0
for name in lfs.dir(directory) do
  if lfs.attributes(directory .. "/" .. name, "mode") == "file" then
    files = files + 1
    local digits = string.match(name, "^IMG_(%d+)%.jpeg$")
    if digits then photos[#photos + 1] = {number = tonumber(digits), name = name} end
  end
end
table.sort(photos, function(a, b)
  if a.number ~= b.number then return a.number < b.number end
  return a.name < b.name
end)

local out = {}
for index, photo in ipairs(photos) do
  local target = string.format("photo-%04d.jpg", index)
  assert(os.rename(directory .. "/" .. photo.name, directory .. "/" .. target))
  out[#out + 1] = string.format("renamed %s -> %s\n", photo.name, target)
end
out[#out + 1] = string.format("renamed %d skipped %d\n", #photos, files - #photos)
io.write(table.concat(out))

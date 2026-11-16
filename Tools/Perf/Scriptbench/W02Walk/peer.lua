-- Lua has no directory listing in its standard library; find lists the files and their sizes.
local files, bytes = 0, 0
local listing = io.popen("find w02 -type f -printf '%s\\n'")
for size in listing:lines() do
  files = files + 1
  bytes = bytes + tonumber(size)
end
listing:close()
print(string.format("files=%d bytes=%d", files, bytes))

-- LuaSocket's HTTP client and lua-cjson: Lua's standard library has no sockets or JSON.
local http = require("socket.http")
local cjson = require("cjson")

local function get(url)
  local body, status = http.request(url)
  if not body then error(status) end
  return status, cjson.decode(body)
end

local base = arg[1] or "http://127.0.0.1:18400"
local status, listing = get(base .. "/items")
assert(status == 200)
local out = {string.format("items %d\n", #listing.items)}
for _, item_id in ipairs({2, 5, 99}) do
  local item_status, item = get(base .. "/items/" .. item_id)
  if item_status == 404 then
    out[#out + 1] = string.format("item %d missing\n", item_id)
  else
    out[#out + 1] = string.format("item %d %s qty=%d\n", item.id, item.name, item.qty)
  end
end
local total = 0
for _, item in ipairs(listing.items) do total = total + item.qty end
out[#out + 1] = string.format("total-qty %d\n", total)
io.write(table.concat(out))

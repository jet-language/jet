local count = 0

local function display() return "count: " .. count end
local function increment() count = count + 1 end
local function reset() count = 0 end

for _ = 1, 3 do increment() end
print(display())
reset()
print(display())

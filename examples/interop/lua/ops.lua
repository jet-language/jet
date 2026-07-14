local calls = 0

function transform(input)
  calls = calls + 1
  return { calls = calls, input = input, values = { true, 2, "three" } }
end

function echo(input)
  return input
end

function fail_call(input)
  error("private Lua detail: " .. tostring(input))
end

function spin(input)
  while true do calls = calls + 1 end
end

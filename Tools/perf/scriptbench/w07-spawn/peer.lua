local ok = 0
for _ = 1, 100 do
  local status = os.execute("true")
  if status == true or status == 0 then ok = ok + 1 end
end
print(string.format("children ok=%d", ok))

local cos, sin, abs, pi, floor = math.cos, math.sin, math.abs, math.pi, math.floor

local N = 4096
local SCALAR_COUNT = N * 2
-- Every real and imaginary scalar must be within this absolute tolerance.
local TOLERANCE = 1.0e-9

-- An iterative radix-2 transform over parallel real and imaginary arrays
-- (index 1 holds element 0): a bit-reversal permutation, then butterflies of
-- doubling width with a twiddle factor stepped per offset.
local function fft(input)
  local size = #input
  local re, im = {}, {}
  for i = 1, size do
    re[i], im[i] = input[i], 0.0
  end
  local j = 0
  for i = 0, size - 2 do
    if i < j then
      re[i + 1], re[j + 1] = re[j + 1], re[i + 1]
      im[i + 1], im[j + 1] = im[j + 1], im[i + 1]
    end
    local bit = floor(size / 2)
    while j >= bit do
      j = j - bit
      bit = floor(bit / 2)
    end
    j = j + bit
  end
  local width = 2
  while width <= size do
    local half = floor(width / 2)
    local angle = -2 * pi / width
    local step_re, step_im = cos(angle), sin(angle)
    for start = 1, size, width do
      local w_re, w_im = 1.0, 0.0
      for k = start, start + half - 1 do
        local h = k + half
        local u_re, u_im = re[k], im[k]
        local v_re = re[h] * w_re - im[h] * w_im
        local v_im = re[h] * w_im + im[h] * w_re
        re[k], im[k] = u_re + v_re, u_im + v_im
        re[h], im[h] = u_re - v_re, u_im - v_im
        w_re, w_im = w_re * step_re - w_im * step_im, w_re * step_im + w_im * step_re
      end
    end
    width = width * 2
  end
  return re, im
end

local input = {}
for i = 1, N do
  input[i] = 0.0
end
input[1] = 1.0
local re, im = fft(input)
local output = {}
for i = 1, N do
  output[2 * i - 1], output[2 * i] = re[i], im[i]
end
local failures = 0
if #output ~= SCALAR_COUNT then
  failures = failures + 1
else
  for i = 1, N do
    if not (abs(output[2 * i - 1] - 1.0) <= TOLERANCE) then
      failures = failures + 1
    end
    if not (abs(output[2 * i]) <= TOLERANCE) then
      failures = failures + 1
    end
  end
end
if failures ~= 0 then
  error(string.format("fft validation failed: %d", failures))
end
io.write("fft4096 checked=8192 tolerance=1e-9\n")

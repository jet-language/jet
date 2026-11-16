local sqrt = math.sqrt

local PI = 3.141592653589793
local SOLAR_MASS = 4.0 * PI * PI
local YEAR = 365.24

local function body(x, y, z, vx, vy, vz, mass)
  return { x = x, y = y, z = z, vx = vx * YEAR, vy = vy * YEAR, vz = vz * YEAR, mass = mass * SOLAR_MASS }
end

local bodies = {
  { x = 0.0, y = 0.0, z = 0.0, vx = 0.0, vy = 0.0, vz = 0.0, mass = SOLAR_MASS },
  body(4.84143144246472090, -1.16032004402742839, -0.103622044471123109,
    0.00166007664274403694, 0.00769901118419740425, -0.0000690460016972063023, 0.000954791938424326609),
  body(8.34336671824457987, 4.12479856412430479, -0.403523417114321381,
    -0.00276742510726862411, 0.00499852801234917238, 0.0000230417297573763929, 0.000285885980666130812),
  body(12.8943695621391310, -15.1111514016986312, -0.223307578892655734,
    0.00296460137564761618, 0.00237847173959480950, -0.0000296589568540237556, 0.0000436624404335156298),
  body(15.3796971148509165, -25.9193146099879641, 0.179258772950371181,
    0.00268067772490389322, 0.00162824170038242201, -0.0000951592254519715870, 0.0000515138902046611451),
}
local count = #bodies

local function energy()
  local total = 0.0
  for i = 1, count do
    local b = bodies[i]
    total = total + 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz)
    for j = i + 1, count do
      local other = bodies[j]
      local dx, dy, dz = b.x - other.x, b.y - other.y, b.z - other.z
      total = total - b.mass * other.mass / sqrt(dx * dx + dy * dy + dz * dz)
    end
  end
  return total
end

local function offset_momentum()
  local px, py, pz = 0.0, 0.0, 0.0
  for i = 2, count do
    local b = bodies[i]
    px = px + b.vx * b.mass
    py = py + b.vy * b.mass
    pz = pz + b.vz * b.mass
  end
  local sun = bodies[1]
  sun.vx = -px / sun.mass
  sun.vy = -py / sun.mass
  sun.vz = -pz / sun.mass
end

local function advance(steps)
  local dt = 0.01
  for _ = 1, steps do
    for i = 1, count do
      local bi = bodies[i]
      local bix, biy, biz, bimass = bi.x, bi.y, bi.z, bi.mass
      local bivx, bivy, bivz = bi.vx, bi.vy, bi.vz
      for j = i + 1, count do
        local bj = bodies[j]
        local dx, dy, dz = bix - bj.x, biy - bj.y, biz - bj.z
        local distance_sq = dx * dx + dy * dy + dz * dz
        local mag = dt / (distance_sq * sqrt(distance_sq))
        local bjm = bj.mass * mag
        bivx = bivx - dx * bjm
        bivy = bivy - dy * bjm
        bivz = bivz - dz * bjm
        local bim = bimass * mag
        bj.vx = bj.vx + dx * bim
        bj.vy = bj.vy + dy * bim
        bj.vz = bj.vz + dz * bim
      end
      bi.vx, bi.vy, bi.vz = bivx, bivy, bivz
      bi.x = bix + dt * bivx
      bi.y = biy + dt * bivy
      bi.z = biz + dt * bivz
    end
  end
end

offset_momentum()
io.write(string.format("%.9f\n", energy()))
advance(tonumber(arg[1]))
io.write(string.format("%.9f\n", energy()))

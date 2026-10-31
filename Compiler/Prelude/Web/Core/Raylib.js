// Shared headless core.game.raylib adapter for IR Web.
// Native display/input remains the AOT bridge; browser calls preserve the same
// typed carriers, no-input defaults, and sprite transcript shape. Drawing
// paints the same RGBA canvas as the native runtime (RouteAGame.jet): Core
// scan-converts each primitive into clipped runs and jet_raylib_fill_rect
// copies or src-over blends one run.

const JET_RAYLIB_CANVAS_SIDE_MAX = 16384;
const jet_raylib_canvas = { width: 0, height: 0, pixels: new Uint8Array(0) };

const jet_raylib_draw_calls = [];

function jet_raylib_window_open(width, height, title) {
  return {
    type_name: "RaylibWindow",
    width: Number(width),
    height: Number(height),
    title: String(title),
    native: false,
  };
}

function jet_raylib_window_should_close(_window) {
  return true;
}

function jet_raylib_window_ready(_window) {
  return false;
}

function jet_raylib_canvas_side(value) {
  return Math.min(Math.max(Number(value) || 0, 0), JET_RAYLIB_CANVAS_SIDE_MAX);
}

function jet_raylib_begin_drawing(window) {
  const width = jet_raylib_canvas_side(window?.width);
  const height = jet_raylib_canvas_side(window?.height);
  if (width === jet_raylib_canvas.width && height === jet_raylib_canvas.height) return;
  jet_raylib_canvas.width = width;
  jet_raylib_canvas.height = height;
  jet_raylib_canvas.pixels = new Uint8Array(width * height * 4);
}

function jet_raylib_canvas_extent() {
  return (BigInt(jet_raylib_canvas.width) << 16n) | BigInt(jet_raylib_canvas.height);
}

function jet_raylib_fill_rect(x, y, width, height, rgba, replace) {
  const canvas = jet_raylib_canvas;
  const color = Number(BigInt(rgba) & 0xffffffffn);
  const alpha = color & 255;
  if (!replace && alpha === 0) return;
  const x0 = Math.max(Number(x), 0);
  const y0 = Math.max(Number(y), 0);
  const x1 = Math.min(Number(x) + Number(width), canvas.width);
  const y1 = Math.min(Number(y) + Number(height), canvas.height);
  if (x1 <= x0 || y1 <= y0) return;
  const source = [(color >>> 24) & 255, (color >>> 16) & 255, (color >>> 8) & 255, alpha];
  const copy = replace || alpha === 255;
  for (let row = y0; row < y1; row += 1) {
    for (let at = (row * canvas.width + x0) * 4, stop = (row * canvas.width + x1) * 4; at < stop; at += 4) {
      for (let channel = 0; channel < 4; channel += 1) {
        canvas.pixels[at + channel] = copy ? source[channel] : Math.floor((source[channel] * alpha + canvas.pixels[at + channel] * (255 - alpha) + 127) / 255);
      }
    }
  }
}

function jet_raylib_end_drawing() {}

function jet_raylib_close_window(_window) {
  jet_raylib_canvas.width = 0;
  jet_raylib_canvas.height = 0;
  jet_raylib_canvas.pixels = new Uint8Array(0);
}

function jet_raylib_set_target_fps(_fps) {}
function jet_raylib_key_down(_name) {
  return false;
}

function jet_raylib_gamepad_down(_gamepad, _button) {
  return false;
}

function jet_raylib_gamepad_axis(_gamepad, _axis) {
  return 0;
}


function jet_raylib_atlas_name(path) {
  const parts = String(path).split(/[\\/]/);
  const file = parts[parts.length - 1] || "atlas";
  return file.replace(/\.[^.]*$/, "") || "atlas";
}

function jet_raylib_load_texture_atlas(path) {
  const atlasPath = String(path);
  return {
    type_name: "RaylibTextureAtlas",
    path: atlasPath,
    name: jet_raylib_atlas_name(atlasPath),
    texture_path: atlasPath.replace(/\.[^.]*$/, ".png"),
    regions: [],
  };
}

function jet_raylib_draw_sprite(atlas, region, x, y) {
  const regions = Array.isArray(atlas?.regions) ? atlas.regions : [];
  const found = regions.find((item) => item && item.name === region);
  if (!found) return;
  jet_raylib_draw_calls.push({
    atlas: String(atlas.name ?? "atlas"),
    region: String(region),
    x: Number(x),
    y: Number(y),
    source_x: Number(found.x),
    source_y: Number(found.y),
    width: Number(found.width),
    height: Number(found.height),
  });
}

function jet_raylib_take_draw_calls() {
  return jet_raylib_draw_calls.splice(0, jet_raylib_draw_calls.length);
}

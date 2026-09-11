// Shared headless core.game.raylib adapter for MIR Web.
// Native display/input remains the AOT bridge; browser calls preserve the same
// typed carriers, no-input defaults, and sprite transcript shape.

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

function jet_raylib_begin_drawing(_window) {}
function jet_raylib_clear_background(_color) {}
function jet_raylib_end_drawing() {}
function jet_raylib_close_window(_window) {}
function jet_raylib_set_target_fps(_fps) {}
function jet_raylib_key_down(_name) {
  return false;
}

function jet_raylib_color(r, g, b, a) {
  return {
    type_name: "RaylibColor",
    r: Number(r),
    g: Number(g),
    b: Number(b),
    a: Number(a),
  };
}

function jet_raylib_draw_rectangle(_x, _y, _width, _height, _color) {}
function jet_raylib_draw_text(_text, _x, _y, _size, _color) {}

function jet_raylib_gamepad_down(_gamepad, _button) {
  return false;
}

function jet_raylib_gamepad_axis(_gamepad, _axis) {
  return 0;
}

function jet_raylib_load_sound(path) {
  return { type_name: "RaylibSound", path: String(path) };
}

function jet_raylib_play_sound(sound) {
  return Boolean(sound && sound.path);
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

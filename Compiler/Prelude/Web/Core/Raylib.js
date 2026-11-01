// Shared headless core.game.raylib adapter for IR Web.
// Native display/input remains the AOT bridge; browser calls preserve the same
// typed carriers, no-input defaults, and sprite transcript shape. Drawing
// paints the same RGBA canvas as the native runtime (RouteAGame.jet): Core
// scan-converts each primitive into clipped runs and jet_raylib_fill_rect
// copies or src-over blends one run (jet_raylib_fill_mask paints a glyph's
// runs in one call); sprites blit from the texture Core decoded and bound to
// the atlas.

const JET_RAYLIB_CANVAS_SIDE_MAX = 16384;
const jet_raylib_canvas = { width: 0, height: 0, pixels: new Uint8Array(0) };
// close_window releases every texture, as the native runtime does; textures
// bound before the latest close carry an older generation and draw nothing.
let jet_raylib_texture_generation = 0;

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

// A 1-bit glyph mask: bit 8 * c + r (c < 7, r < 8) paints the scale x scale
// cell at (x + c * scale, y + r * scale), each column's stretches of set rows
// as one run, as the native runtime paints them. A scale below 1 or above
// 2^32 paints nothing.
function jet_raylib_fill_mask(x, y, scale, mask, rgba) {
  const cell = BigInt(scale);
  if (cell < 1n || cell > 4294967296n) return;
  const bits = BigInt(mask);
  for (let column = 0n; column < 7n; column += 1n) {
    let rows = (bits >> (column * 8n)) & 255n;
    let row = 0n;
    while (rows !== 0n) {
      if ((rows & 1n) === 0n) {
        rows >>= 1n;
        row += 1n;
        continue;
      }
      const start = row;
      while ((rows & 1n) !== 0n) {
        rows >>= 1n;
        row += 1n;
      }
      jet_raylib_fill_rect(BigInt(x) + column * cell, BigInt(y) + start * cell, cell, (row - start) * cell, rgba, false);
    }
  }
}

function jet_raylib_end_drawing() {}

function jet_raylib_close_window(_window) {
  jet_raylib_canvas.width = 0;
  jet_raylib_canvas.height = 0;
  jet_raylib_canvas.pixels = new Uint8Array(0);
  jet_raylib_texture_generation += 1;
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

function jet_raylib_atlas_texture_path(atlas) {
  return String(atlas?.texture_path ?? "");
}

// Binds Core's decoded RGBA8 pixels (`width` per row) to a copy of the atlas;
// pixels that do not form whole rows within the canvas side cap bind nothing.
function jet_raylib_atlas_bind_texture(atlas, pixels, width) {
  const result = { ...atlas };
  const bytes = Uint8Array.from(Array.from(pixels ?? [], Number));
  const columns = Number(width);
  if (!(columns > 0 && columns <= JET_RAYLIB_CANVAS_SIDE_MAX)) return result;
  const rows = Math.floor(bytes.length / (columns * 4));
  if (rows <= 0 || rows > JET_RAYLIB_CANVAS_SIDE_MAX || rows * columns * 4 !== bytes.length) return result;
  result.texture = { width: columns, height: rows, pixels: bytes, generation: jet_raylib_texture_generation };
  return result;
}

// Blits the first region named `region`; the texture, region and canvas
// clip it and each pixel blends src-over like jet_raylib_fill_rect.
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
  const texture = atlas.texture;
  const canvas = jet_raylib_canvas;
  if (!texture || texture.generation !== jet_raylib_texture_generation || canvas.width === 0) return;
  const sx = Number(found.x);
  const sy = Number(found.y);
  const left = Number(x);
  const top = Number(y);
  const firstColumn = Math.max(0, -sx, -left);
  const endColumn = Math.min(Number(found.width), texture.width - sx, canvas.width - left);
  const firstRow = Math.max(0, -sy, -top);
  const endRow = Math.min(Number(found.height), texture.height - sy, canvas.height - top);
  for (let row = firstRow; row < endRow; row += 1) {
    for (let column = firstColumn; column < endColumn; column += 1) {
      const from = ((sy + row) * texture.width + sx + column) * 4;
      const at = ((top + row) * canvas.width + left + column) * 4;
      const alpha = texture.pixels[from + 3];
      if (alpha === 0) continue;
      for (let channel = 0; channel < 4; channel += 1) {
        const source = channel === 3 ? alpha : texture.pixels[from + channel];
        canvas.pixels[at + channel] = alpha === 255 ? source : Math.floor((source * alpha + canvas.pixels[at + channel] * (255 - alpha) + 127) / 255);
      }
    }
  }
}

function jet_raylib_take_draw_calls() {
  return jet_raylib_draw_calls.splice(0, jet_raylib_draw_calls.length);
}

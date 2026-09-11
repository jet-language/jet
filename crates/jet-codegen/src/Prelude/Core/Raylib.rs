// Shared core.game.raylib semantics used by AOT, JIT, and the interpreter.
// Tier adapters own only their handle/ABI carriers and native calls.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetRaylibAtlasRegion {
    pub(crate) name: String,
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) width: i64,
    pub(crate) height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetRaylibAtlasSpec {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) texture_path: Option<String>,
    pub(crate) regions: Vec<JetRaylibAtlasRegion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetRaylibSpriteDrawCall {
    pub(crate) atlas: String,
    pub(crate) region: String,
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) source_x: i64,
    pub(crate) source_y: i64,
    pub(crate) width: i64,
    pub(crate) height: i64,
}

thread_local! {
    static JET_RAYLIB_DRAW_CALLS: std::cell::RefCell<Vec<JetRaylibSpriteDrawCall>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) fn jet_raylib_record_draw_call(call: JetRaylibSpriteDrawCall) {
    JET_RAYLIB_DRAW_CALLS.with(|calls| calls.borrow_mut().push(call));
}

#[allow(dead_code)]
pub(crate) fn jet_raylib_take_draw_calls() -> Vec<JetRaylibSpriteDrawCall> {
    JET_RAYLIB_DRAW_CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
}

pub(crate) fn jet_raylib_sprite_draw_call(
    atlas_name: &str,
    regions: &[JetRaylibAtlasRegion],
    region: &str,
    x: i64,
    y: i64,
) -> Option<JetRaylibSpriteDrawCall> {
    let region_data = regions.iter().find(|item| item.name == region)?;
    Some(JetRaylibSpriteDrawCall {
        atlas: atlas_name.to_string(),
        region: region.to_string(),
        x,
        y,
        source_x: region_data.x,
        source_y: region_data.y,
        width: region_data.width,
        height: region_data.height,
    })
}

pub(crate) fn jet_raylib_load_texture_atlas_spec(path: &str) -> JetRaylibAtlasSpec {
    let fallback_name = std::path::Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("atlas")
        .to_string();
    let text = std::fs::read_to_string(path).ok();
    jet_raylib_parse_texture_atlas(path, text.as_deref(), fallback_name)
}

fn jet_raylib_parse_texture_atlas(
    path: &str,
    text: Option<&str>,
    fallback_name: String,
) -> JetRaylibAtlasSpec {
    let mut name = fallback_name;
    let mut texture_path = None;
    let mut regions = Vec::new();
    if let Some(text) = text {
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if let Some(value) = line.strip_prefix("atlas:") {
                if !value.trim().is_empty() {
                    name = value.trim().to_string();
                }
            } else if let Some(value) = line.strip_prefix("texture:") {
                if !value.trim().is_empty() {
                    texture_path = Some(jet_raylib_atlas_texture_path(path, value.trim()));
                }
            } else if let Some(region) = jet_raylib_atlas_region(line) {
                regions.push(region);
            }
        }
    }
    if texture_path.is_none() {
        texture_path = Some(
            std::path::Path::new(path)
                .with_extension("png")
                .to_string_lossy()
                .into_owned(),
        );
    }
    JetRaylibAtlasSpec {
        path: path.to_string(),
        name,
        texture_path,
        regions,
    }
}

fn jet_raylib_atlas_region(line: &str) -> Option<JetRaylibAtlasRegion> {
    let rest = line
        .strip_prefix("region:")
        .or_else(|| line.strip_prefix("region "))?;
    let mut fields = rest.split_whitespace();
    let name = fields.next()?.to_string();
    let x = fields.next()?.parse().ok()?;
    let y = fields.next()?.parse().ok()?;
    let width = fields.next()?.parse().ok()?;
    let height = fields.next()?.parse().ok()?;
    Some(JetRaylibAtlasRegion {
        name,
        x,
        y,
        width,
        height,
    })
}

fn jet_raylib_atlas_texture_path(atlas_path: &str, texture: &str) -> String {
    let texture_path = std::path::Path::new(texture);
    if texture_path.is_absolute() {
        texture_path.to_string_lossy().into_owned()
    } else {
        std::path::Path::new(atlas_path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(texture_path)
            .to_string_lossy()
            .into_owned()
    }
}

pub(crate) fn jet_raylib_button_code(name: &str) -> Option<i32> {
    match name {
        "A" | "a" | "South" | "south" | "RightFaceDown" | "right_face_down" => Some(7),
        "B" | "b" | "East" | "east" | "RightFaceRight" | "right_face_right" => Some(6),
        "X" | "x" | "West" | "west" | "RightFaceLeft" | "right_face_left" => Some(8),
        "Y" | "y" | "North" | "north" | "RightFaceUp" | "right_face_up" => Some(5),
        "DPadUp" | "dpad_up" | "LeftFaceUp" | "left_face_up" => Some(1),
        "DPadRight" | "dpad_right" | "LeftFaceRight" | "left_face_right" => Some(2),
        "DPadDown" | "dpad_down" | "LeftFaceDown" | "left_face_down" => Some(3),
        "DPadLeft" | "dpad_left" | "LeftFaceLeft" | "left_face_left" => Some(4),
        "LeftShoulder" | "left_shoulder" | "LeftTrigger1" | "left_trigger_1" => Some(9),
        "LeftTrigger" | "left_trigger" | "LeftTrigger2" | "left_trigger_2" => Some(10),
        "RightShoulder" | "right_shoulder" | "RightTrigger1" | "right_trigger_1" => Some(11),
        "RightTrigger" | "right_trigger" | "RightTrigger2" | "right_trigger_2" => Some(12),
        "Back" | "back" | "MiddleLeft" | "middle_left" => Some(13),
        "Start" | "start" | "Middle" | "middle" => Some(14),
        "Guide" | "guide" | "MiddleRight" | "middle_right" => Some(15),
        "LeftThumb" | "left_thumb" => Some(16),
        "RightThumb" | "right_thumb" => Some(17),
        "Left" | "left" => Some(18),
        "Right" | "right" => Some(19),
        _ => None,
    }
}

pub(crate) fn jet_raylib_axis_code(name: &str) -> Option<i32> {
    match name {
        "LeftX" | "left_x" | "leftx" => Some(0),
        "LeftY" | "left_y" | "lefty" => Some(1),
        "RightX" | "right_x" | "rightx" => Some(2),
        "RightY" | "right_y" | "righty" => Some(3),
        "LeftTrigger" | "left_trigger" | "lefttrigger" => Some(4),
        "RightTrigger" | "right_trigger" | "righttrigger" => Some(5),
        _ => None,
    }
}

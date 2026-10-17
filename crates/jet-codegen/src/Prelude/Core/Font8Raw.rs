// OWNER #32: existing host shape_text operation, with no source panic policy.
// Replaced by Jet runtime layer L8, dies at cutover.
pub fn jet_font_shape_raw(text: &str, face: &JetFontFace) -> Result<JetGlyphRun, String> {
    jet_ui_with_current_host(|host| host.shape_text(text, face).map_err(|error| format!("{error:?}")))
}

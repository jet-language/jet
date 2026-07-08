#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raylib_skeleton_signatures_are_registered() {
        let window = core_fixed_sig("core.raylib", "window_open")
            .expect("raylib window_open signature")
            .1
            .expect("window_open return type");
        assert_eq!(window, Type::Named("RaylibWindow".to_string()));

        let color = core_fixed_sig("core.raylib", "color")
            .expect("raylib color signature")
            .1
            .expect("color return type");
        assert_eq!(color, Type::Named("RaylibColor".to_string()));

        let items = core_module_items("core.raylib");
        assert!(items.contains(&"window_ready".to_string()));
        assert!(items.contains(&"draw_text".to_string()));
        assert!(items.contains(&"draw_rectangle".to_string()));
        assert!(items.contains(&"key_down".to_string()));
        assert!(items.contains(&"close_window".to_string()));
    }
}

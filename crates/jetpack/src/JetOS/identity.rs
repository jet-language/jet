const JETOS_RELEASE_VERSION: &str = "26.10";
const JETOS_RELEASE_CODENAME: &str = "Apex";
const JETOS_RELEASE_CODENAME_ID: &str = "apex";
const JETOS_WALLPAPER_SVG: &str = include_str!("assets/apex-wallpaper.svg");

fn jetos_release_label(prerelease: bool) -> String {
    let suffix = if prerelease { "-pre" } else { "" };
    format!(
        "jetos {}{} ({})",
        JETOS_RELEASE_VERSION, suffix, JETOS_RELEASE_CODENAME
    )
}

fn render_jetos_os_release(prerelease: bool) -> String {
    let suffix = if prerelease { "-pre" } else { "" };
    format!(
        "NAME=jetos\nID=jetos\nVERSION=\"{}{} ({})\"\nVERSION_ID={}{}\nVERSION_CODENAME={}\nPRETTY_NAME=\"{}\"\nHOME_URL=\"https://jet.dev/jetos\"\n",
        JETOS_RELEASE_VERSION,
        suffix,
        JETOS_RELEASE_CODENAME,
        JETOS_RELEASE_VERSION,
        suffix,
        JETOS_RELEASE_CODENAME_ID,
        jetos_release_label(prerelease),
    )
}

fn write_jetos_identity_assets(dir: &Path) -> std::io::Result<()> {
    let backgrounds = dir.join("share/backgrounds/jetos");
    fs::create_dir_all(&backgrounds)?;
    fs::write(backgrounds.join("apex.svg"), JETOS_WALLPAPER_SVG)
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn jetos_release_identity_pins_stable_and_prerelease_forms() {
        assert_eq!(jetos_release_label(false), "jetos 26.10 (Apex)");
        assert_eq!(jetos_release_label(true), "jetos 26.10-pre (Apex)");
        assert_eq!(
            render_jetos_os_release(true),
            "NAME=jetos\nID=jetos\nVERSION=\"26.10-pre (Apex)\"\nVERSION_ID=26.10-pre\nVERSION_CODENAME=apex\nPRETTY_NAME=\"jetos 26.10-pre (Apex)\"\nHOME_URL=\"https://jet.dev/jetos\"\n"
        );
        for surface in [render_jetos_os_release(false), render_jetos_os_release(true)] {
            assert!(!surface.contains("NixOS") && !surface.contains("Yarara"));
        }
    }
}

use std::fs;
use std::path::Path;

const JETOS_RELEASE_VERSION: &str = "26.10";
const JETOS_RELEASE_CODENAME: &str = "Apex";
const JETOS_RELEASE_CODENAME_ID: &str = "apex";
const JETOS_WALLPAPER_SVG: &str = include_str!("assets/apex-wallpaper.svg");

pub(super) fn jetos_release_label(prerelease: bool) -> String {
    let suffix = if prerelease { "-pre" } else { "" };
    format!(
        "jetos {}{} ({})",
        JETOS_RELEASE_VERSION, suffix, JETOS_RELEASE_CODENAME
    )
}

pub(super) fn render_jetos_os_release(prerelease: bool) -> String {
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

pub(super) fn write_jetos_identity_assets(dir: &Path) -> std::io::Result<()> {
    let backgrounds = dir.join("share/backgrounds/jetos");
    fs::create_dir_all(&backgrounds)?;
    fs::write(backgrounds.join("apex.svg"), JETOS_WALLPAPER_SVG)
}

#[cfg(test)]
mod identity_tests {
    use super::super::etc_boot_facts::write_etc_tree;
    use super::super::installer_media::{
        copy_generation_payload_deref, render_installed_limine_conf, render_installer_limine_conf,
    };
    use super::super::system_facts::write_hardware_facts;
    use super::super::types::Generation;
    use super::*;
    use jet_env_model::ModuleEval::{self, SystemPlan};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn identity_test_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "jetos-identity-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

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

    #[test]
    fn jetos_release_identity_reaches_generation_and_installer_projection() {
        let root = identity_test_dir();
        let generation = root.join("generation");
        let installer = root.join("installer/current-system");
        let system = SystemPlan {
            name: "halcyon".to_string(),
            target: "linux.x64".to_string(),
            packages: Vec::new(),
            services: Vec::new(),
            options: vec![ModuleEval::OptionPlan {
                key: "hardware.halcyon.specialisation.plasmaBeta".to_string(),
                value: "true".to_string(),
            }],
        };
        fs::create_dir_all(&generation).unwrap();
        write_etc_tree(&generation, &system).unwrap();
        write_hardware_facts(&generation, &system).unwrap();

        let generation_record = Generation {
            name: "identity-proof".to_string(),
            host: system.name.clone(),
            path: generation.clone(),
            created_at: 0,
        };
        let installed_limine = render_installed_limine_conf(&system, &generation_record);
        let installer_limine =
            render_installer_limine_conf(&system, &generation_record, "/dev/sda");
        copy_generation_payload_deref(&generation, &installer).unwrap();

        let os_release = fs::read_to_string(generation.join("etc/os-release")).unwrap();
        let usr_os_release =
            fs::read_to_string(generation.join("usr/lib/os-release")).unwrap();
        let specialisation = fs::read_to_string(
            generation.join("boot/specialisations/plasmaBeta.conf"),
        )
        .unwrap();
        let wallpaper =
            fs::read_to_string(generation.join("share/backgrounds/jetos/apex.svg")).unwrap();
        assert_eq!(os_release, render_jetos_os_release(false));
        assert_eq!(usr_os_release, os_release);
        assert_eq!(wallpaper, JETOS_WALLPAPER_SVG);
        assert!(specialisation
            .contains("title jetos 26.10 (Apex) — halcyon (plasmaBeta)"));
        assert!(installed_limine.contains("/jetos 26.10 (Apex) — halcyon verify"));
        assert!(installer_limine.contains("/Install jetos 26.10 (Apex) — halcyon"));
        assert_eq!(
            fs::read_to_string(installer.join("etc/os-release")).unwrap(),
            os_release
        );
        assert_eq!(
            fs::read_to_string(installer.join("usr/lib/os-release")).unwrap(),
            usr_os_release
        );
        assert_eq!(
            fs::read_to_string(installer.join("share/backgrounds/jetos/apex.svg")).unwrap(),
            wallpaper
        );
        for surface in [
            os_release,
            usr_os_release,
            specialisation,
            wallpaper,
            installed_limine,
            installer_limine,
        ] {
            assert!(!surface.contains("NixOS") && !surface.contains("Yarara"));
        }
        fs::remove_dir_all(root).unwrap();
    }
}

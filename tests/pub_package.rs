use std::fs;
use std::path::PathBuf;

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("jet-pub-package-{tag}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    fn join(&self, path: &str) -> PathBuf {
        self.path.join(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn pub_package_function_is_visible_inside_project_scope() {
    let s = Scratch::new("same");
    fs::write(
        s.join("helper.jet"),
        "pub(package) fn secret() -> String {\n    return \"ok\"\n}\n",
    )
    .unwrap();
    fs::write(
        s.join("main.jet"),
        "use helper;\n\nfn main() {\n    print(helper.secret())\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&s.join("main.jet").to_string_lossy());
    assert!(
        diags.is_empty(),
        "expected same-package access to pass, got {diags:?}"
    );
}

#[test]
fn pub_package_function_is_hidden_from_path_dependency_consumer() {
    let s = Scratch::new("dep");
    let app = s.join("app");
    let dep = s.join("dep");
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&dep).unwrap();
    fs::write(
        app.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { dep: path@../dep }\n",
    )
    .unwrap();
    fs::write(
        app.join("main.jet"),
        "use dep;\n\nfn main() {\n    print(dep.secret())\n}\n",
    )
    .unwrap();
    fs::write(
        dep.join("pkg.jet"),
        "payload: { name: \"dep\", version: \"0.1.0\" }\n",
    )
    .unwrap();
    fs::write(
        dep.join("dep.jet"),
        "pub(package) fn secret() -> String {\n    return \"hidden\"\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&app.join("main.jet").to_string_lossy());
    assert!(
        diags.iter().any(|d| d.code == "E0605"),
        "expected downstream access to report E0605, got {diags:?}"
    );
}

#[test]
fn pub_package_type_and_field_are_visible_inside_project_scope() {
    let s = Scratch::new("type");
    fs::write(
        s.join("helper.jet"),
        "pub(package) struct Secret {\n    pub(package) value: String\n}\n\npub fn make() -> Secret {\n    return Secret.{ value: \"ok\" }\n}\n",
    )
    .unwrap();
    fs::write(
        s.join("main.jet"),
        "use helper;\n\nfn main() {\n    s #= helper.make()\n    print(s.value)\n}\n",
    )
    .unwrap();

    let diags = jet::check_with_path(&s.join("main.jet").to_string_lossy());
    assert!(
        diags.is_empty(),
        "expected same-package type/field access to pass, got {diags:?}"
    );
}

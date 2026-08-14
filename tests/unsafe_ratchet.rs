use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("jet-unsafe-ratchet-{tag}-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("package.jet"), "name: \"fixture\"\nversion: \"0.1.0\"\n").unwrap();
        fs::write(root.join("safety.md"), "# Safety\n").unwrap();
        Self { root }
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn baseline(&self) -> PathBuf {
        self.root.join("safety.md")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn ratchet(fixture: &Fixture, update: bool) -> Output {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/agent/check-unsafe-ratchet.mjs");
    let mut command = Command::new("node");
    command
        .arg(script)
        .args(["--root", fixture.root.to_str().unwrap()])
        .args(["--baseline", fixture.baseline().to_str().unwrap()]);
    if update {
        command.arg("--update");
    }
    command.output().unwrap()
}

fn seed(fixture: &Fixture) {
    let output = ratchet(fixture, true);
    assert!(output.status.success(), "baseline seed failed: {}", String::from_utf8_lossy(&output.stderr));
}

pub fn ratchet_trips_on_seeded_growth() {
    let fixture = Fixture::new("growth");
    fixture.write("main.jet", "#Unsafe(\"seed region\") {}\n");
    seed(&fixture);
    fixture.write("new.jet", "#Unsafe(\"new region reason\") {}\n");

    let output = ratchet(&fixture, false);
    assert!(!output.status.success(), "growth must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fixture: 1 -> 2"), "{stderr}");
    assert!(stderr.contains("new.jet"), "{stderr}");
    assert!(stderr.contains("new region reason"), "{stderr}");
}

pub fn ratchet_allows_shrink() {
    let fixture = Fixture::new("shrink");
    fixture.write(
        "main.jet",
        "#Unsafe(\"keep this region\") {}\n#Unsafe(\"remove this region\") {}\n",
    );
    seed(&fixture);
    fixture.write("main.jet", "#Unsafe(\"keep this region\") {}\n");

    let output = ratchet(&fixture, false);
    assert!(output.status.success(), "shrink must pass: {}", String::from_utf8_lossy(&output.stderr));
    let baseline = fs::read_to_string(fixture.baseline()).unwrap();
    assert!(baseline.contains("\"total\": 1"), "{baseline}");
    assert!(!baseline.contains("remove this region"), "{baseline}");
}

pub fn generated_ffi_does_not_move_baseline() {
    let fixture = Fixture::new("generated-ffi");
    fixture.write("main.jet", "#Unsafe(\"authored region\") {}\n");
    seed(&fixture);
    let before = fs::read_to_string(fixture.baseline()).unwrap();
    fixture.write(
        ".jet/bindings/c/generated.jet",
        "#Bindgen module c.fixture.__bindgen__ {\n    #Unsafe(\"generated FFI region\") fn foreign() {}\n}\n",
    );

    let output = ratchet(&fixture, false);
    assert!(output.status.success(), "generated FFI must not grow baseline: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(before, fs::read_to_string(fixture.baseline()).unwrap());
}

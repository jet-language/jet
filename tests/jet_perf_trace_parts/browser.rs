#[test]
fn perf_attach_uses_compiler_browser_map_after_source_changes() {
    let _guard = SELF_ATTACH_LOCK.lock().unwrap();
    let root = temp_workspace();
    let source = root.join("browser.jet");
    let original = r##"#Target(Web)
use core.web as web
module handlers {
    #Target(JS)
    pub fn init() { web.on("#field", "input", (ev) => {}) }
}
#Target(JS)
fn run() { handlers.init() }
"##;
    fs::write(&source, original).unwrap();
    let compiled = jet::compile_web_with_path(original, source.to_str().unwrap()).unwrap();
    let manifest = compiled.web.unwrap().manifest_json;
    let relay = jet::DevServer::BrowserTrace::Relay::new(&manifest).unwrap();
    fs::write(&source, "fn run() {}\n").unwrap();
    relay
        .record(b"class=event&symbol=handlers__init%24handler0&start_ns=100&duration_ns=25&clock_ns=125")
        .unwrap();

    let out = root.join("browser.jettrace");
    let attach = run_jet(
        &root,
        &["perf", "attach", &std::process::id().to_string(), "--out", out.to_str().unwrap()],
    );
    assert!(attach.status.success(), "{}", String::from_utf8_lossy(&attach.stderr));
    let bytes = fs::read(&out).unwrap();
    verify_jettrace(&bytes).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"name\":\"handlers__init$handler0\""), "{text}");
    assert!(text.contains(&format!("\"sha256\":\"{}\"", jet::SHA256::sha256_hex(original.as_bytes()))), "{text}");
    assert!(!text.contains(&jet::SHA256::sha256_hex(b"fn run() {}\n")), "source was reread: {text}");
    assert!(text.contains("\"source_maps\":[{"), "missing embedded source maps: {text}");
    assert!(text.contains("\"kind\":\"js\""), "missing js source map: {text}");
    // Host facts merge into the same artifact (D-PERF-BROWSER-TRANSPORT1=A).
    assert!(
        text.contains("\"domain\":\"wall\"") || text.contains("\"domain\":\"cpu\"") || text.contains("\"native\":[{"),
        "browser attach missing host domains: {text}"
    );
    drop(relay);
    let _ = fs::remove_dir_all(root);
}
#[test]
fn perf_run_records_capture_allowlist_and_source_maps() {
    let root = temp_workspace();
    let source = root.join("allow.jet");
    fs::write(&source, "fn run() { print(\"ok\\n\") }\n").unwrap();
    let out = root.join("allow.jettrace");
    let run = run_jet(
        &root,
        &[
            "perf",
            "run",
            "allow.jet",
            "--capture=urls,values",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert!(
        run.status.success(),
        "perf run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = fs::read_to_string(&out).unwrap();
    verify_jettrace(text.as_bytes()).unwrap();
    assert!(
        text.contains("\"allowlist\":[\"urls\",\"values\"]"),
        "allowlist missing from capture_policy: {text}"
    );
    assert!(text.contains("\"source_maps\":[{"), "{text}");
    assert!(text.contains("\"kind\":\"jet\""), "{text}");
    assert!(text.contains("sourcesContent"), "{text}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn perf_capture_rejects_unknown_allowlist_field() {
    let root = temp_workspace();
    let source = root.join("bad.jet");
    fs::write(&source, "fn run() {}\n").unwrap();
    let run = run_jet(
        &root,
        &["perf", "run", "bad.jet", "--capture=cookies", "--out", "x.jettrace"],
    );
    assert_eq!(run.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("cookies"), "{stderr}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn perf_attach_drives_real_devserver_browser_lifecycle() {
    if !common::have_rustc() {
        eprintln!("note: skipping real browser relay lifecycle (need rustc)");
        return;
    }
    let _guard = SELF_ATTACH_LOCK.lock().unwrap();
    let root = temp_workspace();
    let source = root.join("app.jet");
    let original = r##"#Target(Web)
use core.web as web
module handlers {
    #Target(JS)
    pub fn init() { web.on("#field", "input", (ev) => {}) }
}
#Target(JS)
fn run() { handlers.init() }
"##;
    fs::write(&source, original).unwrap();
    let port = unused_local_port();
    let dev = Command::new(jet())
        .current_dir(&root)
        .args([
            "dev",
            "app.jet",
            "--target=web",
            &format!("--port={port}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start real web devserver");
    let dev_pid = dev.id();
    let dev = ChildGuard(dev);

    let initial_html = wait_http(port, "/", |_| true);
    assert!(
        !initial_html.contains("__jetPerfNow"),
        "normal jet dev timed browser work"
    );
    assert!(
        !initial_html.contains("/__jet_perf_browser"),
        "normal jet dev emitted perf beacon"
    );
    let initial_version = wait_http(port, "/__jet_dev_version", |body| !body.trim().is_empty());

    let trace = root.join("browser.jettrace");
    let attach = Command::new(jet())
        .current_dir(&root)
        .args([
            "perf",
            "attach",
            &dev_pid.to_string(),
            "--out",
            trace.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start real jet perf attach");
    let attached_html = wait_http(port, "/", |html| perf_nonce(html).is_some());
    let nonce = perf_nonce(&attached_html).unwrap().to_string();
    assert!(attached_html.contains("self.__jetPerfNow"));
    assert!(attached_html.contains("navigator.sendBeacon(\"/__jet_perf_browser?nonce=\""));
    let event = "class=event&symbol=handlers__init%24handler0&start_ns=10&duration_ns=5&clock_ns=15";
    assert_eq!(
        http_request(
            port,
            "POST",
            &format!("/__jet_perf_browser?nonce={nonce}"),
            event
        )
        .unwrap()
        .0,
        204
    );
    let output = attach.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let trace_text = fs::read_to_string(&trace).unwrap();
    verify_jettrace(trace_text.as_bytes()).unwrap();
    assert!(
        trace_text.contains("\"name\":\"handlers__init$handler0\""),
        "{trace_text}"
    );
    let active_version = wait_http(port, "/__jet_dev_version", |body| body != initial_version);

    fs::write(&source, format!("{original}\n// rebuilt\n")).unwrap();
    let rebuilt_version = wait_http(port, "/__jet_dev_version", |body| body != active_version);
    assert_ne!(rebuilt_version, active_version);
    let rebuilt_html = wait_http(port, "/", |html| {
        perf_nonce(html).is_some_and(|next| next != nonce)
    });
    let rebuilt_nonce = perf_nonce(&rebuilt_html).unwrap().to_string();
    assert_eq!(
        http_request(
            port,
            "POST",
            &format!("/__jet_perf_browser?nonce={nonce}"),
            event
        )
        .unwrap()
        .0,
        403
    );
    assert_eq!(
        http_request(
            port,
            "POST",
            &format!("/__jet_perf_browser?nonce={rebuilt_nonce}"),
            event
        )
        .unwrap()
        .0,
        204
    );

    drop(dev);
    let _ = fs::remove_dir_all(root);
}

//! JetOS Studio tests (Tower card #367 slice 6 split).
//!
//! Covers the Studio headless/serve/transaction surface over a realized
//! JetOS generation. Split out of the former `tests/jetpack.rs`.

use std::fs;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::*;

#[test]
fn jetos_studio_headless_opens_installed_app_projection() {
    let root = Scratch::new("studio-root");
    let switch = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "studio-app",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        switch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let generation = root.path.join("systems/generations/studio-app");
    let out = jetos()
        .args(["studio", "--headless", "--no-color"])
        .env("JETOS_STUDIO_ROOT", &generation)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("studio/index.html"),
        "stdout should print app path: {stdout}"
    );

    let open_bin = root.path.join("open-bin");
    fs::create_dir_all(&open_bin).unwrap();
    write_executable(&open_bin.join("xdg-open"), "#!/bin/sh\nexit 0\n");
    let mut child = jetos()
        .args(["studio", "--no-color"])
        .env("JETOS_STUDIO_ROOT", &generation)
        .env("PATH", &open_bin)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    {
        use std::io::BufRead;
        reader.read_line(&mut line).unwrap();
    }
    let addr = line
        .trim()
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/studio/"))
        .expect("default Studio launch must open its local projection service");
    let page = studio_http(addr, "GET", "/studio/", "");
    assert!(page.contains("data-page-kind=\"dashboard\""), "page: {page}");
    let _ = child.kill();
    let _ = child.wait();
}


#[test]
fn jetos_studio_serve_exposes_projection_json() {
    let root = Scratch::new("studio-serve-root");
    let switch = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "studio-serve",
            "--no-color",
            "--offline",
        ])
        .current_dir(config_example_dir())
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        switch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let generation = root.path.join("systems/generations/studio-serve");
    let mut child = jetos()
        .args(["studio", "--serve", "127.0.0.1:0", "--no-color"])
        .env("JETOS_STUDIO_ROOT", &generation)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    {
        use std::io::BufRead;
        reader.read_line(&mut line).unwrap();
    }
    let addr = line
        .trim()
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/studio/"))
        .expect("service url");
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    {
        use std::io::Write;
        stream
            .write_all(b"GET /studio/data.json HTTP/1.1\r\nHost: local\r\n\r\n")
            .unwrap();
    }
    let mut response = String::new();
    {
        use std::io::Read;
        stream.read_to_string(&mut response).unwrap();
    }
    assert!(response.contains("200 OK"), "response: {response}");
    assert!(
        response.contains("jetos-studio-projection"),
        "response: {response}"
    );
    assert!(response.contains("openssh"), "response: {response}");
    let page = studio_http(addr, "GET", "/studio/", "");
    assert!(page.contains("200 OK"), "page: {page}");
    assert!(
        page.contains("data-page-kind=\"dashboard\"")
            && page.contains("data-page-registry=\"studio-pages\"")
            && page.contains("data-page-kind=\"changeset\""),
        "served Studio must be dashboard/sidebar/Changeset app: {page}"
    );
    assert!(
        page.contains("data-changeset-action=\"apply\"")
            && page.contains("data-changeset-action=\"discard\""),
        "served Studio must expose one Changeset apply path: {page}"
    );
    let _ = child.kill();
    let _ = child.wait();
}


#[test]
fn jetos_studio_transaction_previews_and_writes_source() {
    let project = Scratch::new("studio-edit-project");
    copy_dir_recursive(&config_example_dir(), &project.path);
    let root = Scratch::new("studio-edit-root");
    let switch = jet()
        .args([
            "os",
            "switch",
            "halcyon",
            "--name",
            "studio-edit",
            "--no-color",
            "--yes",
            "--offline",
        ])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        switch.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let generation = root.path.join("systems/generations/studio-edit");
    let mut child = jetos()
        .args([
            "studio",
            project.path.to_str().unwrap(),
            "--host",
            "halcyon",
            "--serve",
            "127.0.0.1:0",
            "--no-color",
        ])
        .env("JETOS_STUDIO_ROOT", &generation)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let server_pid = child.id();
    let stdout = child.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    {
        use std::io::BufRead;
        reader.read_line(&mut line).unwrap();
    }
    let addr = line
        .trim()
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/studio/"))
        .expect("service url");
    let session = studio_session(addr);
    let other_session = studio_session(addr);
    let initial_data = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(initial_data.contains("live-checked-plan"), "data: {initial_data}");
    assert!(initial_data.contains("network.hostName"), "data: {initial_data}");
    for field in ["page_registry", "renderer", "system_plan", "services", "packages", "options", "proof_state", "generations"] {
        assert!(initial_data.contains(field), "missing live model `{field}`: {initial_data}");
    }
    let bypass = studio_stage_option(addr, &session, "network.hostName", "bypass", true);
    assert!(bypass.contains("400 Bad Request"), "bypass: {bypass}");
    assert!(bypass.contains("direct Studio writes are disabled"), "bypass: {bypass}");
    let inserted = studio_stage_option(addr, &session, "network.mtu", "1500", false);
    assert!(inserted.contains("@@ -1,"), "inserted: {inserted}");
    assert!(inserted.contains("+            network.mtu: 1500,"), "inserted: {inserted}");
    let inserted_owner = studio_changeset_owner(&inserted, &session);
    let _ = studio_owned_transaction(addr, "discard", &inserted_owner);
    let preview = studio_stage_option(addr, &session, "network.hostName", "aurora", false);
    assert!(preview.contains("200 OK"), "preview: {preview}");
    assert!(preview.contains("\"write\":false"), "preview: {preview}");
    assert!(preview.contains("\"state\":\"staged\""), "preview: {preview}");
    assert!(preview.contains("\"staged_count\":1"), "preview: {preview}");
    let preview_owner = studio_changeset_owner(&preview, &session);
    assert!(
        preview.contains("-            network.hostName: halcyon,"),
        "preview: {preview}"
    );
    assert!(
        preview.contains("+            network.hostName: aurora,"),
        "preview: {preview}"
    );
    let config = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(
        config.contains("network.hostName: halcyon"),
        "config: {config}"
    );
    let source = studio_http(addr, "GET", "/studio/source", "");
    assert!(
        source.contains("network.hostName: halcyon"),
        "source: {source}"
    );
    let stolen_discard = studio_http(addr, "POST", "/studio/transaction", &format!("{{\"op\":\"discard\",\"session_id\":\"{other_session}\",\"token\":\"{}\",\"base_revision\":\"{}\"}}", preview_owner.token, preview_owner.base_revision));
    assert!(stolen_discard.contains("409 Conflict"), "stolen discard: {stolen_discard}");
    let stolen_apply = studio_http(addr, "POST", "/studio/transaction", &format!("{{\"op\":\"apply\",\"session_id\":\"{other_session}\",\"token\":\"{}\",\"base_revision\":\"{}\"}}", preview_owner.token, preview_owner.base_revision));
    assert!(stolen_apply.contains("409 Conflict"), "stolen apply: {stolen_apply}");
    let staged = studio_owned_transaction(addr, "status", &preview_owner);
    assert!(staged.contains("200 OK"), "staged: {staged}");
    assert!(staged.contains("\"state\":\"staged\""), "staged: {staged}");
    assert!(staged.contains("\"staged_count\":1"), "staged: {staged}");
    assert!(staged.contains("network.hostName"), "staged: {staged}");
    let discarded = studio_owned_transaction(addr, "discard", &preview_owner);
    assert!(discarded.contains("\"state\":\"discarded\""), "discarded: {discarded}");
    let empty = studio_session_transaction(addr, "status", &session);
    assert!(empty.contains("\"state\":\"empty\""), "empty: {empty}");
    let preview = studio_stage_option(addr, &session, "network.hostName", "aurora", false);
    assert!(preview.contains("\"state\":\"staged\""), "preview: {preview}");
    let preview_owner = studio_changeset_owner(&preview, &session);
    let original = fs::read_to_string(project.join("config.jet")).unwrap();
    fs::write(project.join("config.jet"), format!("{original}// external edit\n")).unwrap();
    let stale = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(stale.contains("409 Conflict"), "stale: {stale}");
    assert!(stale.contains("changed after this Changeset"), "stale: {stale}");
    fs::write(project.join("config.jet"), &original).unwrap();
    let config_q = test_shell_quote(&project.join("config.jet"));
    let lock_path = project.join(".config.jet.studio.lock");
    let mut compliant_writer = std::process::Command::new("flock")
        .arg("-x")
        .arg(&lock_path)
        .arg("sh")
        .arg("-c")
        .arg(format!("sleep 0.05; printf '%s\\n' '// compliant external process' >> {config_q}"))
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let raced_apply = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(compliant_writer.wait().unwrap().success());
    assert!(raced_apply.contains("409 Conflict"), "cross-process CAS: {raced_apply}");
    let externally_written = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(externally_written.contains("compliant external process"), "external write was clobbered: {externally_written}");
    fs::write(project.join("config.jet"), &original).unwrap();
    let mut noncompliant_writer = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("i=0; while [ $i -lt 30 ]; do printf '%s\\n' '// noncompliant external process' >> {config_q}; i=$((i + 1)); sleep 0.01; done"))
        .spawn()
        .unwrap();
    let noncompliant_apply = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(noncompliant_writer.wait().unwrap().success());
    assert!(noncompliant_apply.contains("409 Conflict"), "noncompliant CAS: {noncompliant_apply}");
    let externally_written = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(externally_written.contains("noncompliant external process"), "noncompliant write was clobbered: {externally_written}");
    fs::write(project.join("config.jet"), &original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let original_mode = fs::metadata(&project.path).unwrap().permissions().mode();
        fs::set_permissions(&project.path, fs::Permissions::from_mode(0o555)).unwrap();
        let failed = studio_owned_transaction(addr, "apply", &preview_owner);
        fs::set_permissions(&project.path, fs::Permissions::from_mode(original_mode)).unwrap();
        assert!(failed.contains("500 Internal Server Error"), "failed: {failed}");
        assert!(failed.contains("\"reprojected\":false"), "failed: {failed}");
    }
    let write = studio_owned_transaction(addr, "apply", &preview_owner);
    assert!(write.contains("200 OK"), "write: {write}");
    assert!(write.contains("\"state\":\"applied\""), "write: {write}");
    assert!(write.contains("\"reprojected\":true"), "write: {write}");
    assert!(write.contains("\"staged_count\":0"), "write: {write}");
    let config = fs::read_to_string(project.join("config.jet")).unwrap();
    assert!(
        config.contains("network.hostName: aurora"),
        "config: {config}"
    );
    let source = studio_http(addr, "GET", "/studio/source", "");
    assert!(
        source.contains("network.hostName: aurora"),
        "source: {source}"
    );
    let live_data = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(live_data.contains("live-checked-plan"), "data: {live_data}");
    assert!(live_data.contains("aurora"), "data: {live_data}");
    assert!(live_data.contains("\"renderer\":\"dashboard\""), "data: {live_data}");
    let plan = jet()
        .args(["os", "plan", "halcyon", "--json", "--no-color", "--offline"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let stdout = String::from_utf8_lossy(&plan.stdout);
    assert!(stdout.contains("aurora"), "plan: {stdout}");
    let check = studio_http(addr, "POST", "/studio/run", "{\"action\":\"check\"}");
    assert!(check.contains("\"success\":true"), "check: {check}");
    let plan = studio_http(addr, "POST", "/studio/run", "{\"action\":\"plan\"}");
    assert!(plan.contains("\"success\":true"), "plan: {plan}");
    assert!(plan.contains("aurora"), "plan: {plan}");
    let build = studio_http(addr, "POST", "/studio/run", "{\"action\":\"build\"}");
    assert!(build.contains("\"success\":true"), "build: {build}");
    let unproved_switch = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    assert!(unproved_switch.contains("409 Conflict"), "switch: {unproved_switch}");
    let config_path = project.join("config.jet");
    let proved_source = fs::read_to_string(&config_path).unwrap();
    let raced_source = proved_source.replace("network.hostName: aurora", "network.hostName: intruder");
    let race_path = config_path.clone();
    let race = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(race_path, raced_source).unwrap();
    });
    let raced_proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    race.join().unwrap();
    assert!(raced_proof.contains("\"source_revision\":"), "proof race: {raced_proof}");
    let raced_switch = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    assert!(raced_switch.contains("409 Conflict"), "proof race switch: {raced_switch}");
    let failed_proof_state = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(failed_proof_state.contains("\"state\":\"unproved\""), "failed proof badge: {failed_proof_state}");
    fs::write(&config_path, &proved_source).unwrap();
    let snapshot_mutation = studio_attack_snapshot(server_pid, false);
    let mutated_proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    snapshot_mutation.join().unwrap();
    assert!(mutated_proof.contains("\"success\":true"), "sealed snapshot: {mutated_proof}");
    let snapshot_replacement = studio_attack_snapshot(server_pid, true);
    let replaced_proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    snapshot_replacement.join().unwrap();
    assert!(replaced_proof.contains("\"success\":true"), "sealed snapshot: {replaced_proof}");
    let proved_source = format!("{proved_source}// unbuilt proof-only revision\n");
    fs::write(&config_path, &proved_source).unwrap();
    let proof = studio_http(addr, "POST", "/studio/run", "{\"action\":\"proof\"}");
    assert!(proof.contains("\"success\":true"), "proof: {proof}");
    assert!(proof.contains("aurora"), "proof: {proof}");
    assert!(proof.contains("\"source_revision\":"), "proof: {proof}");
    assert!(proof.contains("source_proof"), "proof: {proof}");
    assert!(proof.contains("input_plan_sha256"), "proof: {proof}");
    let proof_response = studio_json(&proof);
    assert_eq!(
        proof_response.get("success").unwrap(),
        &jetpack::JSON::JSONValue::Bool(true),
        "proof: {proof}"
    );
    let proof_revision = json_string(&proof_response, "source_revision");
    let proof_stdout = json_string(&proof_response, "stdout");
    let proof_artifact = jetpack::JSON::parse(proof_stdout.trim())
        .unwrap_or_else(|error| panic!("invalid Studio proof artifact: {error}: {proof_stdout}"));
    let proof_generation = json_string(&proof_artifact, "generation");
    let proof_source = proof_artifact
        .get("source_proof")
        .unwrap_or_else(|error| panic!("missing proof source binding: {error}: {proof_artifact:?}"));
    let proof_source_sha256 = json_string(proof_source, "source_sha256");
    let proof_input_plan_sha256 = json_string(proof_source, "input_plan_sha256");
    let proof_plan_sha256 = json_string(proof_source, "plan_sha256");
    assert_eq!(proof_source_sha256, proof_revision);
    let proved_projection = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(proved_projection.contains("\"state\":\"proved\""), "proved badge: {proved_projection}");
    assert!(proved_projection.contains(&proof_revision), "proved revision: {proved_projection}");
    let switch_race_path = config_path.clone();
    let switch_raced_source = proved_source.replace("network.hostName: aurora", "network.hostName: unproved");
    let switch_race = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1));
        fs::write(switch_race_path, switch_raced_source).unwrap();
    });
    let switched = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    switch_race.join().unwrap();
    assert!(switched.contains("\"success\":false"), "switch race: {switched}");
    assert!(switched.contains("\"source_changed_after\":true"), "switch: {switched}");
    assert!(switched.contains("rolled back"), "switch race: {switched}");
    let current_after_race = fs::read_link(root.join("systems/current")).unwrap();
    assert_eq!(current_after_race.file_name().unwrap(), "studio-edit");
    fs::write(&config_path, &proved_source).unwrap();
    let candidate_plan_before = fs::read(root.join("systems/generations/zz-studio-candidate/plan.json")).unwrap();
    let switched = studio_http(addr, "POST", "/studio/run", "{\"action\":\"switch\"}");
    assert!(switched.contains("\"success\":true"), "switch: {switched}");
    let current = fs::read_link(root.join("systems/current")).unwrap();
    assert_eq!(current.file_name().unwrap(), proof_generation.as_str());
    let current_source_proof =
        fs::read_to_string(root.join("systems/current/source-proof.json")).unwrap();
    let current_source_proof = jetpack::JSON::parse(&current_source_proof)
        .unwrap_or_else(|error| panic!("invalid current generation source proof: {error}"));
    assert_eq!(
        json_string(&current_source_proof, "source_sha256"),
        proof_source_sha256
    );
    assert_eq!(
        json_string(&current_source_proof, "input_plan_sha256"),
        proof_input_plan_sha256
    );
    assert_eq!(
        json_string(&current_source_proof, "plan_sha256"),
        proof_plan_sha256
    );
    assert_eq!(
        fs::read(root.join("systems/generations/zz-studio-candidate/plan.json")).unwrap(),
        candidate_plan_before,
        "switch must not rebuild a candidate"
    );
    let generations = studio_http(addr, "POST", "/studio/run", "{\"action\":\"generations\"}");
    assert!(
        generations.contains("zz-studio-candidate"),
        "generations: {generations}"
    );
    let applied_source = proved_source.replace("// unbuilt proof-only revision\n", "");
    fs::write(&config_path, applied_source).unwrap();
    let rollback = studio_session_transaction(addr, "stage-rollback", &session);
    assert!(rollback.contains("\"state\":\"staged\""), "rollback: {rollback}");
    let rollback_owner = studio_changeset_owner(&rollback, &session);
    assert!(rollback.contains("-            network.hostName: aurora,"), "rollback: {rollback}");
    assert!(rollback.contains("+            network.hostName: halcyon,"), "rollback: {rollback}");
    let rollback_apply = studio_owned_transaction(addr, "apply", &rollback_owner);
    assert!(rollback_apply.contains("\"reprojected\":true"), "rollback: {rollback_apply}");
    let restored = studio_http(addr, "GET", "/studio/data.json", "");
    assert!(restored.contains("halcyon"), "restored: {restored}");
    let literal = "\"café \\\"lab\\\" \\\\share\"";
    let escaped = studio_stage_option(addr, &session, "network.interface", literal, false);
    assert!(escaped.contains("café"), "escaped: {escaped}");
    let escaped_owner = studio_changeset_owner(&escaped, &session);
    let escaped_apply = studio_owned_transaction(addr, "apply", &escaped_owner);
    assert!(escaped_apply.contains("\"reprojected\":true"), "escaped: {escaped_apply}");
    let escaped_source = fs::read_to_string(&config_path).unwrap();
    assert!(
        escaped_source.contains(&format!("network.interface: {literal},")),
        "escaped source: {escaped_source}"
    );
    let _ = child.kill();
    let _ = child.wait();
}


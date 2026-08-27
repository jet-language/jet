use super::parse::Parsed;
use crate::Output::Theme;
use crate::Store;

pub(super) fn cmd_why(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        return why_error(
            theme,
            parsed,
            "E1274",
            "why needs a package name",
            "`jetpack why` reads the package's environment request and stored provenance records.",
            "write `jetpack why <package>`.",
        );
    };
    let roots = Store::resolve();
    match Store::why_package(&roots, query) {
        Ok(Some(report)) => {
            if parsed.flags.json {
                println!("{}", report.to_json());
            } else {
                print!("{}", report.text());
            }
            0
        }
        Ok(None) => why_error(
            theme,
            parsed,
            "E1274",
            &format!("no stored package record for `{query}`"),
            "`jetpack why` can answer only packages with a realized StoreEntry or recorded build attempt.",
            &format!("realize `{query}` first, then rerun `jetpack why {query}`."),
        ),
        Err(error) => why_error(
            theme,
            parsed,
            "E1274",
            "couldn't read package provenance",
            &error.to_string(),
            "repair the Hangar provenance records, then retry the explanation.",
        ),
    }
}

fn why_error(
    theme: &Theme,
    parsed: &Parsed,
    code: &str,
    what: &str,
    why: &str,
    fix: &str,
) -> i32 {
    if parsed.flags.json {
        let diagnostic = jet_foundation::Diagnostics::Diagnostic::error(
            code,
            what.to_string(),
            why.to_string(),
            fix.to_string(),
            None,
        );
        print!(
            "{}",
            jet_foundation::Diagnostics::render_all_json(
                &jet_foundation::Diagnostics::ReportPath::from_process(""),
                "",
                &[diagnostic],
            )
        );
    } else {
        theme.error_coded(code, what, why, fix);
    }
    2
}

#[cfg(test)]
mod tests {
    use super::Store::{PackageWhy, WhyDisk, WhyLocation, WhyOrigin, WhyRequesting, WhyTrust};

    fn sample() -> PackageWhy {
        PackageWhy {
            query: "ripgrep".to_string(),
            package: "ripgrep".to_string(),
            available: true,
            requesting: WhyRequesting {
                env_file: WhyLocation {
                    path: "/work/env.jet".to_string(),
                    line: Some(12),
                    text: Some("packages: [ripgrep]".to_string()),
                },
                lock_file: WhyLocation {
                    path: "/work/.jet/lock".to_string(),
                    line: Some(8),
                    text: Some("name = \"ripgrep\"".to_string()),
                },
            },
            origin: WhyOrigin {
                catalog: "official-signed".to_string(),
                endpoint: "https://index.example".to_string(),
                cache_endpoint: "https://cache.example".to_string(),
                signature_chain: "present".to_string(),
                source: "/nix/store/rg.drv".to_string(),
                source_digest: "source-hash".to_string(),
            },
            trust: WhyTrust {
                grade: "signed".to_string(),
                reason: "the catalog and signature chain are verified".to_string(),
            },
            disk: WhyDisk {
                bytes: Some(4096),
                objects: 2,
            },
            dependents: vec!["tool@1.0 (tool@jetpack)".to_string()],
            receipt: "sha256-receipt".to_string(),
        }
    }

    #[test]
    fn human_and_json_why_reports_contain_the_same_facts() {
        let report = sample();
        let text = report.text();
        for fact in [
            "env file",
            "/work/env.jet:12",
            "packages: [ripgrep]",
            "lock line",
            "/work/.jet/lock:8",
            "name = \"ripgrep\"",
            "catalog",
            "endpoint",
            "signature chain",
            "signed",
            "disk",
            "dependents",
            "receipt",
        ] {
            assert!(text.contains(fact), "human report misses {fact}");
        }

        let json = report.to_json();
        assert!(crate::JSON::parse(&json).is_ok());
        for fact in [
            "\"env_file\"",
            "\"lock_file\"",
            "\"catalog\"",
            "\"endpoint\"",
            "\"signature_chain\"",
            "\"source_digest\"",
            "\"grade\"",
            "\"bytes\"",
            "\"dependents\"",
            "\"receipt\"",
        ] {
            assert!(json.contains(fact), "JSON report misses {fact}");
        }

        let object = crate::JSON::parse(&json)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(object.get("action").unwrap().as_str().unwrap(), "why");
        assert_eq!(
            object
                .get("requesting")
                .unwrap()
                .get("env_file")
                .unwrap()
                .get("line")
                .unwrap(),
            &crate::JSON::JSONValue::Number(12)
        );
        assert_eq!(
            object
                .get("trust")
                .unwrap()
                .get("grade")
                .unwrap()
                .as_str()
                .unwrap(),
            "signed"
        );
        assert_eq!(
            object
                .get("origin")
                .unwrap()
                .get("endpoint")
                .unwrap()
                .as_str()
                .unwrap(),
            "https://index.example"
        );
        assert_eq!(
            object
                .get("requesting")
                .unwrap()
                .get("lock_file")
                .unwrap()
                .get("text")
                .unwrap()
                .as_str()
                .unwrap(),
            "name = \"ripgrep\""
        );
        assert_eq!(
            object
                .get("dependents")
                .unwrap()
                .as_array()
                .unwrap()[0]
                .as_str()
                .unwrap(),
            "tool@1.0 (tool@jetpack)"
        );
        assert_eq!(
            object
                .get("disk")
                .unwrap()
                .get("bytes")
                .unwrap(),
            &crate::JSON::JSONValue::Number(4096)
        );
    }
}

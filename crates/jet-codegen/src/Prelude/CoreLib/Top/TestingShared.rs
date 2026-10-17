// Physical leaves for Core/testing/testing.jet. Evidence classification,
// sanitation, diff bounds/LCS, and lifecycle ordering stay in Jet.
// Plain records deliberately use the existing private Core record mapper.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetTestingEvidenceRead {
    pub(crate) ok: bool,
    pub(crate) text: String,
    pub(crate) missing: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetTestingTempFacts {
    pub(crate) pid: u64,
    pub(crate) thread: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetTestingTempPath {
    pub(crate) path: Vec<u8>,
    pub(crate) text: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetTestingTempRemove {
    pub(crate) ok: bool,
    pub(crate) missing: bool,
    pub(crate) cause: String,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JetTestingTempCreate {
    pub(crate) ok: bool,
    pub(crate) cause: String,
}

pub(crate) fn jet_testing_evidence_read_raw(path: &String) -> JetTestingEvidenceRead {
    match std::fs::read_to_string(path) {
        Ok(text) => JetTestingEvidenceRead { ok: true, text, missing: false },
        Err(error) => JetTestingEvidenceRead {
            ok: false,
            text: String::new(),
            missing: error.kind() == std::io::ErrorKind::NotFound,
        },
    }
}

pub(crate) fn jet_testing_temp_facts_raw() -> JetTestingTempFacts {
    JetTestingTempFacts {
        pid: u64::from(std::process::id()),
        thread: format!("{:?}", std::thread::current().id()),
    }
}

pub(crate) fn jet_testing_temp_join_raw(name: &String) -> JetTestingTempPath {
    let path = std::env::temp_dir().join(name);
    let text = path.to_string_lossy().into_owned();
    JetTestingTempPath { path: path.into_os_string().into_encoded_bytes(), text }
}

fn jet_testing_temp_os_path(path: &Vec<u8>) -> &std::ffi::OsStr {
    // SAFETY: the trusted source passes __TempPath.path unchanged from
    // jet_testing_temp_join_raw. It is opaque: never constructed, sliced,
    // edited, or decoded in Jet. These bytes came from into_encoded_bytes
    // on this same platform and are only borrowed back for OS operations.
    unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(path) }
}

pub(crate) fn jet_testing_temp_remove_raw(path: &Vec<u8>) -> JetTestingTempRemove {
    match std::fs::remove_dir_all(jet_testing_temp_os_path(path)) {
        Ok(()) => JetTestingTempRemove { ok: true, missing: false, cause: String::new() },
        Err(error) => JetTestingTempRemove {
            ok: false,
            missing: error.kind() == std::io::ErrorKind::NotFound,
            cause: error.to_string(),
        },
    }
}

pub(crate) fn jet_testing_temp_create_raw(path: &Vec<u8>) -> JetTestingTempCreate {
    match std::fs::create_dir_all(jet_testing_temp_os_path(path)) {
        Ok(()) => JetTestingTempCreate { ok: true, cause: String::new() },
        Err(error) => JetTestingTempCreate { ok: false, cause: error.to_string() },
    }
}

pub(crate) fn jet_testing_text_window_raw(text: &String, start: u64, end: u64) -> String {
    text[start as usize..end as usize].to_owned()
}

#[cfg(all(test, unix))]
mod testing_temp_byte_path_tests {
    #[test]
    fn non_utf8_unix_name_round_trips_through_opaque_path_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let name = vec![b't', b'm', b'p', 0xff, b'x'];
        let encoded = std::ffi::OsString::from_vec(name.clone()).into_encoded_bytes();
        assert_eq!(super::jet_testing_temp_os_path(&encoded).as_bytes(), name);
        assert_eq!(super::jet_testing_temp_os_path(&encoded).to_string_lossy(), "tmp\u{fffd}x");
    }
}

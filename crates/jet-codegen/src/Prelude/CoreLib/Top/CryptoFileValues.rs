// Value hooks of the JETC stat carrier (CryptoFileIo.rs), emitted only into
// generated runtimes: the resident host includes CryptoFileIo.rs without the
// Jet value traits. `__JetcStat` is a Core record carried natively, so a
// record holding one (`JetcParent.metadata`) shows, debugs and clones it with
// the same canonical record shape a source record would produce.
impl JetShow for JetCryptoJetcStat {
    fn jet_show(&self) -> String {
        jet_debug_record("__JetcStat", [
            ("status".to_string(), self.status.jet_show()),
            ("errno".to_string(), self.errno.jet_show()),
            ("dev".to_string(), self.dev.jet_show()),
            ("ino".to_string(), self.ino.jet_show()),
            ("len".to_string(), self.len.jet_show()),
            ("mtime".to_string(), self.mtime.jet_show()),
            ("mtime_ns".to_string(), self.mtime_ns.jet_show()),
            ("nlink".to_string(), self.nlink.jet_show()),
            ("is_file".to_string(), self.is_file.jet_show()),
            ("is_dir".to_string(), self.is_dir.jet_show()),
        ])
    }
}

impl JetDisplay for JetCryptoJetcStat {
    fn jet_display(&self) -> String {
        <Self as JetShow>::jet_show(self)
    }
}

impl JetDebug for JetCryptoJetcStat {
    fn jet_debug(&self) -> String {
        let field = |name: &str, value: String, storage_index: usize| JetDebugField {
            name: name.to_string(),
            value,
            storage_index,
            redacted: false,
        };
        jet_debug_record_fields("__JetcStat", [
            field("status", self.status.jet_debug(), 0),
            field("errno", self.errno.jet_debug(), 1),
            field("dev", self.dev.jet_debug(), 2),
            field("ino", self.ino.jet_debug(), 3),
            field("len", self.len.jet_debug(), 4),
            field("mtime", self.mtime.jet_debug(), 5),
            field("mtime_ns", self.mtime_ns.jet_debug(), 6),
            field("nlink", self.nlink.jet_debug(), 7),
            field("is_file", self.is_file.jet_debug(), 8),
            field("is_dir", self.is_dir.jet_debug(), 9),
        ])
    }
}

// Raw host facts used by Jet-owned civil policy; adapters do no defaulting.
pub(crate) fn jet_time_wall_parts() -> (u64, u32, bool) {
    let (duration, before_epoch) = match std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
    {
        Ok(duration) => (duration, false),
        Err(error) => (error.duration(), true),
    };
    (duration.as_secs(), duration.subsec_nanos(), before_epoch)
}

pub(crate) fn jet_time_zone_environment(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

pub(crate) fn jet_time_zone_file(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

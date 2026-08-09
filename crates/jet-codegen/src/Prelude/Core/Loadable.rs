// D-PENDING1=B: one tag policy for `Loadable` across value and packed tiers.

pub(crate) const JET_LOADABLE_IDLE: u8 = 0;
pub(crate) const JET_LOADABLE_LOADING: u8 = 1;
pub(crate) const JET_LOADABLE_LOADED: u8 = 2;
pub(crate) const JET_LOADABLE_FAILED: u8 = 3;

pub(crate) fn jet_loadable_is_tag(actual: u8, expected: u8) -> bool {
    actual == expected
}

pub(crate) fn jet_loadable_has_value(actual: u8) -> bool {
    jet_loadable_is_tag(actual, JET_LOADABLE_LOADED)
}

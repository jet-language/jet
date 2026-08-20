// D-TESTDATA1: deterministic, locale-aware fake data for `core.testing`.
// Tables stay small, vendored, and ordered. Every domain consumes a fixed
// number of SplitMix64 draws so seeded output stays portable across tiers.

const EN_FIRST: &[&str] = &["Alice", "Bob", "Clara", "Diego"];
const EN_LAST: &[&str] = &["Smith", "Jones", "Taylor", "Brown"];
const EN_HOSTS: &[&str] = &["example.test", "mail.test", "demo.test"];
const EN_STREETS: &[&str] = &["Oak Street", "Pine Street", "Maple Street", "Cedar Street"];
const EN_CITIES: &[&str] = &["Austin", "Boston", "Denver", "Portland"];

const DE_FIRST: &[&str] = &["Anna", "Bruno", "Clara", "Dieter"];
const DE_LAST: &[&str] = &["Schmidt", "Fischer", "Weber", "Wagner"];
const DE_HOSTS: &[&str] = &["beispiel.test", "mail.test", "demo.test"];
const DE_STREETS: &[&str] = &["Bahnhofstraße", "Hauptstraße", "Lindenweg", "Gartenweg"];
const DE_CITIES: &[&str] = &["Berlin", "Bremen", "Dresden", "Hamburg"];

fn fake_tables(fake: &jet_std::Fake) -> (&'static [&'static str], &'static [&'static str], &'static [&'static str], &'static [&'static str], &'static [&'static str]) {
    if fake.locale == 1 {
        (DE_FIRST, DE_LAST, DE_HOSTS, DE_STREETS, DE_CITIES)
    } else {
        (EN_FIRST, EN_LAST, EN_HOSTS, EN_STREETS, EN_CITIES)
    }
}

fn fake_pick<'a>(state: &mut u64, values: &'a [&'a str]) -> &'a str {
    values[jet_seeded_rng_int(state, 0, values.len() as i64 - 1) as usize]
}

pub(crate) fn jet_testing_fake_new(seed: i64) -> jet_std::Fake {
    jet_std::Fake {
        state: seed as u64,
        locale: 0,
    }
}

pub(crate) fn jet_fake_locale(fake: &jet_std::Fake, locale: &String) -> jet_std::Fake {
    let code = match locale.as_str() {
        "en" => 0,
        "de" => 1,
        other => panic!("unsupported fake-data locale `{other}`; supported locales: en, de"),
    };
    jet_std::Fake {
        state: fake.state ^ (code as u64).wrapping_mul(0xD1B5_4A32_D192_ED03),
        locale: code,
    }
}

pub(crate) fn jet_fake_name(fake: &mut jet_std::Fake) -> String {
    let (first, last, _, _, _) = fake_tables(fake);
    format!("{} {}", fake_pick(&mut fake.state, first), fake_pick(&mut fake.state, last))
}

pub(crate) fn jet_fake_email(fake: &mut jet_std::Fake) -> String {
    let (first, last, hosts, _, _) = fake_tables(fake);
    let first = fake_pick(&mut fake.state, first).to_ascii_lowercase();
    let last = fake_pick(&mut fake.state, last).to_ascii_lowercase();
    format!("{first}.{last}@{}", fake_pick(&mut fake.state, hosts))
}

pub(crate) fn jet_fake_host(fake: &mut jet_std::Fake) -> String {
    let (_, _, hosts, _, _) = fake_tables(fake);
    format!("www.{}", fake_pick(&mut fake.state, hosts))
}

pub(crate) fn jet_fake_address(fake: &mut jet_std::Fake) -> String {
    let (_, _, _, streets, cities) = fake_tables(fake);
    let number = jet_seeded_rng_int(&mut fake.state, 1, 999);
    format!("{number} {}, {}", fake_pick(&mut fake.state, streets), fake_pick(&mut fake.state, cities))
}

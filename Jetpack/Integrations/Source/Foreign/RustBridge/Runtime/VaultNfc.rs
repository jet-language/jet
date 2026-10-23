const VAULT_HANGUL_SBASE: u32 = 0xAC00;
const VAULT_HANGUL_LBASE: u32 = 0x1100;
const VAULT_HANGUL_VBASE: u32 = 0x1161;
const VAULT_HANGUL_TBASE: u32 = 0x11A7;
const VAULT_HANGUL_LCOUNT: u32 = 19;
const VAULT_HANGUL_VCOUNT: u32 = 21;
const VAULT_HANGUL_TCOUNT: u32 = 28;
const VAULT_HANGUL_NCOUNT: u32 = VAULT_HANGUL_VCOUNT * VAULT_HANGUL_TCOUNT;
const VAULT_HANGUL_SCOUNT: u32 = VAULT_HANGUL_LCOUNT * VAULT_HANGUL_NCOUNT;

fn jet_vault_ccc(cp: u32) -> u8 {
    UNICODE_CCC
        .binary_search_by(|&(start, end, _)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .map(|index| UNICODE_CCC[index].2)
        .unwrap_or(0)
}

fn jet_vault_hangul_decompose(cp: u32) -> Option<[u32; 3]> {
    if !(VAULT_HANGUL_SBASE..VAULT_HANGUL_SBASE + VAULT_HANGUL_SCOUNT).contains(&cp) {
        return None;
    }
    let index = cp - VAULT_HANGUL_SBASE;
    let trailing = index % VAULT_HANGUL_TCOUNT;
    Some([
        VAULT_HANGUL_LBASE + index / VAULT_HANGUL_NCOUNT,
        VAULT_HANGUL_VBASE + (index % VAULT_HANGUL_NCOUNT) / VAULT_HANGUL_TCOUNT,
        if trailing == 0 { 0 } else { VAULT_HANGUL_TBASE + trailing },
    ])
}

fn jet_vault_decomposition(cp: u32) -> Option<&'static [u32]> {
    let index = UNICODE_DECOMP_INDEX
        .binary_search_by_key(&cp, |&(codepoint, _, _, _)| codepoint)
        .ok()?;
    let (_, start, len, canonical) = UNICODE_DECOMP_INDEX[index];
    if canonical == 0 {
        return None;
    }
    Some(&UNICODE_DECOMP_POOL[start as usize..(start + len) as usize])
}

fn jet_vault_decompose(cp: u32, output: &mut Vec<u32>) {
    if let Some([leading, vowel, trailing]) = jet_vault_hangul_decompose(cp) {
        output.push(leading);
        output.push(vowel);
        if trailing != 0 {
            output.push(trailing);
        }
    } else if let Some(decomposition) = jet_vault_decomposition(cp) {
        for &part in decomposition {
            jet_vault_decompose(part, output);
        }
    } else {
        output.push(cp);
    }
}

fn jet_vault_compose_pair(first: u32, second: u32) -> Option<u32> {
    if (VAULT_HANGUL_LBASE..VAULT_HANGUL_LBASE + VAULT_HANGUL_LCOUNT).contains(&first)
        && (VAULT_HANGUL_VBASE..VAULT_HANGUL_VBASE + VAULT_HANGUL_VCOUNT).contains(&second)
    {
        return Some(
            VAULT_HANGUL_SBASE
                + ((first - VAULT_HANGUL_LBASE) * VAULT_HANGUL_VCOUNT
                    + second
                    - VAULT_HANGUL_VBASE)
                    * VAULT_HANGUL_TCOUNT,
        );
    }
    if (VAULT_HANGUL_SBASE..VAULT_HANGUL_SBASE + VAULT_HANGUL_SCOUNT).contains(&first)
        && (first - VAULT_HANGUL_SBASE) % VAULT_HANGUL_TCOUNT == 0
        && second > VAULT_HANGUL_TBASE
        && second < VAULT_HANGUL_TBASE + VAULT_HANGUL_TCOUNT
    {
        return Some(first + second - VAULT_HANGUL_TBASE);
    }
    UNICODE_COMPOSE_PAIRS
        .binary_search_by(|&(left, right, _)| (left, right).cmp(&(first, second)))
        .ok()
        .map(|index| UNICODE_COMPOSE_PAIRS[index].2)
}

fn jet_vault_nfc(input: &str) -> String {
    let mut decomposed = Vec::with_capacity(input.len());
    for character in input.chars() {
        jet_vault_decompose(character as u32, &mut decomposed);
    }
    for index in 1..decomposed.len() {
        let class = jet_vault_ccc(decomposed[index]);
        if class == 0 {
            continue;
        }
        let mut cursor = index;
        while cursor > 0 && jet_vault_ccc(decomposed[cursor - 1]) > class {
            decomposed.swap(cursor - 1, cursor);
            cursor -= 1;
        }
    }

    let mut composed = Vec::with_capacity(decomposed.len());
    let mut starter = None;
    let mut last_class = -1i32;
    for codepoint in decomposed {
        let class = jet_vault_ccc(codepoint) as i32;
        if let Some(starter_index) = starter {
            if let Some(result) = jet_vault_compose_pair(composed[starter_index], codepoint) {
                if last_class < class {
                    composed[starter_index] = result;
                    continue;
                }
            }
        }
        composed.push(codepoint);
        if class == 0 {
            starter = Some(composed.len() - 1);
            last_class = -1;
        } else {
            last_class = class;
        }
    }
    composed.into_iter().filter_map(char::from_u32).collect()
}

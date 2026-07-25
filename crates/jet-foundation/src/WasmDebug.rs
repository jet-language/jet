//! Std-only Wasm DWARF line-table reader and `sourceMappingURL` custom section.
//! Used by the web artifact writer to join rustc code offsets to Jet lines.

use std::str;

/// One decoded `.debug_line` row. `address` is the Wasm code-section-relative
/// offset rustc embeds for `wasm32-unknown-unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmLineRow {
    pub address: u64,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub is_stmt: bool,
    pub end_sequence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasmDebugError {
    Malformed(&'static str),
}

/// Payload start of the Wasm Code section (section id 10), if present.
pub fn code_section_payload_offset(wasm: &[u8]) -> Result<Option<usize>, WasmDebugError> {
    if wasm.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err(WasmDebugError::Malformed("unsupported Wasm header"));
    }
    let mut at = 8usize;
    while at < wasm.len() {
        let id = *wasm.get(at).ok_or(WasmDebugError::Malformed("truncated section id"))?;
        at += 1;
        let size = read_uleb(wasm, &mut at)?;
        let payload_start = at;
        let end = at
            .checked_add(size)
            .ok_or(WasmDebugError::Malformed("section size overflow"))?;
        if end > wasm.len() {
            return Err(WasmDebugError::Malformed("section extends past file"));
        }
        if id == 10 {
            return Ok(Some(payload_start));
        }
        at = end;
    }
    Ok(None)
}

/// Read every `.debug_line` custom section in `wasm`.
pub fn parse_debug_line(wasm: &[u8]) -> Result<Vec<WasmLineRow>, WasmDebugError> {
    let mut rows = Vec::new();
    for payload in custom_section_payloads(wasm, b".debug_line")? {
        decode_debug_line_section(payload, &mut rows)?;
    }
    Ok(rows)
}

/// Append a relative `sourceMappingURL` custom section (Chrome/Firefox Wasm maps).
pub fn embed_source_mapping_url(wasm: &mut Vec<u8>, url: &str) -> Result<(), WasmDebugError> {
    if wasm.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err(WasmDebugError::Malformed("unsupported Wasm header"));
    }
    // Touch-parse so a truncated module fails before we append.
    let _ = custom_section_payloads(wasm, b"")?;
    const NAME: &str = "sourceMappingURL";
    let mut payload = Vec::new();
    put_uleb(&mut payload, NAME.len());
    payload.extend_from_slice(NAME.as_bytes());
    payload.extend_from_slice(url.as_bytes());
    wasm.push(0);
    put_uleb(wasm, payload.len());
    wasm.extend_from_slice(&payload);
    Ok(())
}

fn custom_section_payloads<'a>(
    wasm: &'a [u8],
    want_name: &[u8],
) -> Result<Vec<&'a [u8]>, WasmDebugError> {
    if wasm.get(..8) != Some(b"\0asm\x01\0\0\0") {
        return Err(WasmDebugError::Malformed("unsupported Wasm header"));
    }
    let mut at = 8usize;
    let mut found = Vec::new();
    while at < wasm.len() {
        let id = *wasm.get(at).ok_or(WasmDebugError::Malformed("truncated section id"))?;
        at += 1;
        let size = read_uleb(wasm, &mut at)?;
        let end = at
            .checked_add(size)
            .ok_or(WasmDebugError::Malformed("section size overflow"))?;
        if end > wasm.len() {
            return Err(WasmDebugError::Malformed("section extends past file"));
        }
        let payload = &wasm[at..end];
        if id == 0 {
            let mut name_at = 0usize;
            let name_len = read_uleb(payload, &mut name_at)?;
            let name_end = name_at
                .checked_add(name_len)
                .ok_or(WasmDebugError::Malformed("custom name overflow"))?;
            if name_end > payload.len() {
                return Err(WasmDebugError::Malformed("custom name past payload"));
            }
            let name = &payload[name_at..name_end];
            if want_name.is_empty() || name == want_name {
                found.push(&payload[name_end..]);
            }
        }
        at = end;
    }
    Ok(found)
}

fn decode_debug_line_section(
    section: &[u8],
    rows: &mut Vec<WasmLineRow>,
) -> Result<(), WasmDebugError> {
    let mut at = 0usize;
    while at < section.len() {
        let unit_start = at;
        let (unit_len, dwarf64) = read_initial_length(section, &mut at)?;
        let unit_end = unit_start
            .checked_add(if dwarf64 { 12 } else { 4 })
            .and_then(|h| h.checked_add(unit_len))
            .ok_or(WasmDebugError::Malformed("line unit length overflow"))?;
        if unit_end > section.len() {
            return Err(WasmDebugError::Malformed("line unit past section"));
        }
        let version = read_u16(section, &mut at)?;
        if version < 2 || version > 5 {
            return Err(WasmDebugError::Malformed("unsupported .debug_line version"));
        }
        let prologue_length = if dwarf64 {
            read_u64(section, &mut at)? as usize
        } else {
            read_u32(section, &mut at)? as usize
        };
        let prologue_end = at
            .checked_add(prologue_length)
            .ok_or(WasmDebugError::Malformed("prologue length overflow"))?;
        if prologue_end > unit_end {
            return Err(WasmDebugError::Malformed("prologue past unit"));
        }
        let minimum_instruction_length = *section
            .get(at)
            .ok_or(WasmDebugError::Malformed("missing min_inst_length"))?;
        at += 1;
        if minimum_instruction_length == 0 {
            return Err(WasmDebugError::Malformed("min_inst_length is zero"));
        }
        let maximum_operations_per_instruction = if version >= 4 {
            let v = *section
                .get(at)
                .ok_or(WasmDebugError::Malformed("missing max_ops_per_inst"))?;
            at += 1;
            if v == 0 {
                return Err(WasmDebugError::Malformed("max_ops_per_inst is zero"));
            }
            v
        } else {
            1
        };
        let default_is_stmt = *section
            .get(at)
            .ok_or(WasmDebugError::Malformed("missing default_is_stmt"))?
            != 0;
        at += 1;
        let line_base = *section
            .get(at)
            .ok_or(WasmDebugError::Malformed("missing line_base"))? as i8;
        at += 1;
        let line_range = *section
            .get(at)
            .ok_or(WasmDebugError::Malformed("missing line_range"))?;
        at += 1;
        if line_range == 0 {
            return Err(WasmDebugError::Malformed("line_range is zero"));
        }
        let opcode_base = *section
            .get(at)
            .ok_or(WasmDebugError::Malformed("missing opcode_base"))?;
        at += 1;
        let mut standard_opcode_lengths = Vec::new();
        for _ in 1..opcode_base {
            let len = *section
                .get(at)
                .ok_or(WasmDebugError::Malformed("truncated opcode lengths"))?;
            at += 1;
            standard_opcode_lengths.push(len);
        }
        let mut include_directories = vec![String::new()];
        loop {
            let dir = read_cstring(section, &mut at)?;
            if dir.is_empty() {
                break;
            }
            include_directories.push(dir);
        }
        let mut file_names = Vec::new();
        loop {
            let name = read_cstring(section, &mut at)?;
            if name.is_empty() {
                break;
            }
            let dir_index = read_uleb(section, &mut at)?;
            let _mod_time = read_uleb(section, &mut at)?;
            let _length = read_uleb(section, &mut at)?;
            let dir = include_directories
                .get(dir_index)
                .cloned()
                .unwrap_or_default();
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            file_names.push(path);
        }
        // Skip any remaining prologue bytes (DWARF5 / producer padding).
        at = prologue_end;

        let mut address: u64 = 0;
        let mut op_index: u32 = 0;
        let mut file: u32 = 1;
        let mut line: i64 = 1;
        let mut column: u32 = 0;
        let mut is_stmt = default_is_stmt;
        let mut basic_block = false;
        let mut prologue_end = false;
        let mut epilogue_begin = false;
        let mut discriminator: u32 = 0;

        let emit = |rows: &mut Vec<WasmLineRow>,
                    address: u64,
                    file: u32,
                    line: i64,
                    column: u32,
                    is_stmt: bool,
                    end_sequence: bool,
                    file_names: &[String]| {
            if line <= 0 {
                return;
            }
            let file_name = file_names
                .get(file.saturating_sub(1) as usize)
                .cloned()
                .unwrap_or_default();
            rows.push(WasmLineRow {
                address,
                file: file_name,
                line: line as u32,
                column,
                is_stmt,
                end_sequence,
            });
        };

        while at < unit_end {
            let op = *section
                .get(at)
                .ok_or(WasmDebugError::Malformed("truncated line op"))?;
            at += 1;
            if op == 0 {
                let len = read_uleb(section, &mut at)?;
                let ext_end = at
                    .checked_add(len)
                    .ok_or(WasmDebugError::Malformed("ext op length overflow"))?;
                if ext_end > unit_end {
                    return Err(WasmDebugError::Malformed("ext op past unit"));
                }
                if len == 0 {
                    continue;
                }
                let ext = *section
                    .get(at)
                    .ok_or(WasmDebugError::Malformed("missing ext opcode"))?;
                at += 1;
                match ext {
                    1 => {
                        // DW_LNE_end_sequence
                        emit(
                            rows,
                            address,
                            file,
                            line,
                            column,
                            is_stmt,
                            true,
                            &file_names,
                        );
                        address = 0;
                        op_index = 0;
                        file = 1;
                        line = 1;
                        column = 0;
                        is_stmt = default_is_stmt;
                        basic_block = false;
                        prologue_end = false;
                        epilogue_begin = false;
                        discriminator = 0;
                    }
                    2 => {
                        // DW_LNE_set_address
                        let addr_bytes = ext_end - at;
                        address = match addr_bytes {
                            4 => read_u32(section, &mut at)? as u64,
                            8 => read_u64(section, &mut at)?,
                            _ => {
                                return Err(WasmDebugError::Malformed(
                                    "unsupported set_address size",
                                ))
                            }
                        };
                        op_index = 0;
                    }
                    3 => {
                        // DW_LNE_define_file — skip for wasm rustc output.
                        let _ = read_cstring(section, &mut at)?;
                        let _ = read_uleb(section, &mut at)?;
                        let _ = read_uleb(section, &mut at)?;
                        let _ = read_uleb(section, &mut at)?;
                    }
                    4 => {
                        discriminator = read_uleb(section, &mut at)? as u32;
                    }
                    _ => {}
                }
                at = ext_end;
                let _ = (basic_block, prologue_end, epilogue_begin, discriminator);
            } else if op < opcode_base {
                match op {
                    1 => {
                        // DW_LNS_copy
                        emit(
                            rows,
                            address,
                            file,
                            line,
                            column,
                            is_stmt,
                            false,
                            &file_names,
                        );
                        basic_block = false;
                        prologue_end = false;
                        epilogue_begin = false;
                        discriminator = 0;
                    }
                    2 => {
                        let op_advance = read_uleb(section, &mut at)? as u32;
                        advance_pc(
                            &mut address,
                            &mut op_index,
                            op_advance,
                            minimum_instruction_length,
                            maximum_operations_per_instruction,
                        );
                    }
                    3 => {
                        line += read_sleb(section, &mut at)?;
                    }
                    4 => {
                        file = read_uleb(section, &mut at)? as u32;
                    }
                    5 => {
                        column = read_uleb(section, &mut at)? as u32;
                    }
                    6 => {
                        is_stmt = !is_stmt;
                    }
                    7 => {
                        basic_block = true;
                    }
                    8 => {
                        let advance = (255 - opcode_base as u32) / line_range as u32;
                        advance_pc(
                            &mut address,
                            &mut op_index,
                            advance,
                            minimum_instruction_length,
                            maximum_operations_per_instruction,
                        );
                    }
                    9 => {
                        let operand = read_u16(section, &mut at)? as u64;
                        address += operand;
                        op_index = 0;
                    }
                    10 => {
                        prologue_end = true;
                    }
                    11 => {
                        epilogue_begin = true;
                    }
                    12 => {
                        let _isa = read_uleb(section, &mut at)?;
                    }
                    _ => {
                        let idx = (op as usize).saturating_sub(1);
                        let args = *standard_opcode_lengths
                            .get(idx)
                            .ok_or(WasmDebugError::Malformed("unknown standard opcode"))?;
                        for _ in 0..args {
                            let _ = read_uleb(section, &mut at)?;
                        }
                    }
                }
            } else {
                let adjusted = op - opcode_base;
                let line_delta = line_base as i64 + (adjusted % line_range) as i64;
                let op_advance = (adjusted / line_range) as u32;
                advance_pc(
                    &mut address,
                    &mut op_index,
                    op_advance,
                    minimum_instruction_length,
                    maximum_operations_per_instruction,
                );
                line += line_delta;
                emit(
                    rows,
                    address,
                    file,
                    line,
                    column,
                    is_stmt,
                    false,
                    &file_names,
                );
                basic_block = false;
                prologue_end = false;
                epilogue_begin = false;
                discriminator = 0;
            }
        }
        at = unit_end;
    }
    Ok(())
}

fn advance_pc(
    address: &mut u64,
    op_index: &mut u32,
    op_advance: u32,
    min_inst_length: u8,
    max_ops: u8,
) {
    let max_ops = max_ops as u32;
    let new_op = (*op_index as u64) + op_advance as u64;
    *address += (new_op / max_ops as u64) * min_inst_length as u64;
    *op_index = (new_op % max_ops as u64) as u32;
}

fn read_initial_length(buf: &[u8], at: &mut usize) -> Result<(usize, bool), WasmDebugError> {
    let first = read_u32(buf, at)?;
    if first == 0xffff_ffff {
        Ok((read_u64(buf, at)? as usize, true))
    } else if first >= 0xffff_fff0 {
        Err(WasmDebugError::Malformed("reserved initial length"))
    } else {
        Ok((first as usize, false))
    }
}

fn read_u16(buf: &[u8], at: &mut usize) -> Result<u16, WasmDebugError> {
    let end = at
        .checked_add(2)
        .ok_or(WasmDebugError::Malformed("u16 overflow"))?;
    let bytes = buf
        .get(*at..end)
        .ok_or(WasmDebugError::Malformed("truncated u16"))?;
    *at = end;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(buf: &[u8], at: &mut usize) -> Result<u32, WasmDebugError> {
    let end = at
        .checked_add(4)
        .ok_or(WasmDebugError::Malformed("u32 overflow"))?;
    let bytes = buf
        .get(*at..end)
        .ok_or(WasmDebugError::Malformed("truncated u32"))?;
    *at = end;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(buf: &[u8], at: &mut usize) -> Result<u64, WasmDebugError> {
    let end = at
        .checked_add(8)
        .ok_or(WasmDebugError::Malformed("u64 overflow"))?;
    let bytes = buf
        .get(*at..end)
        .ok_or(WasmDebugError::Malformed("truncated u64"))?;
    *at = end;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_uleb(buf: &[u8], at: &mut usize) -> Result<usize, WasmDebugError> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *buf
            .get(*at)
            .ok_or(WasmDebugError::Malformed("truncated ULEB"))?;
        *at += 1;
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return Err(WasmDebugError::Malformed("ULEB too large"));
        }
    }
    usize::try_from(result).map_err(|_| WasmDebugError::Malformed("ULEB exceeds usize"))
}

fn read_sleb(buf: &[u8], at: &mut usize) -> Result<i64, WasmDebugError> {
    let mut result: i64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *buf
            .get(*at)
            .ok_or(WasmDebugError::Malformed("truncated SLEB"))?;
        *at += 1;
        result |= i64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 64 && (byte & 0x40) != 0 {
                result |= !0i64 << shift;
            }
            return Ok(result);
        }
        if shift >= 64 {
            return Err(WasmDebugError::Malformed("SLEB too large"));
        }
    }
}

fn read_cstring(buf: &[u8], at: &mut usize) -> Result<String, WasmDebugError> {
    let start = *at;
    while *at < buf.len() && buf[*at] != 0 {
        *at += 1;
    }
    if *at >= buf.len() {
        return Err(WasmDebugError::Malformed("unterminated cstring"));
    }
    let s = str::from_utf8(&buf[start..*at])
        .map_err(|_| WasmDebugError::Malformed("cstring is not utf-8"))?
        .to_string();
    *at += 1;
    Ok(s)
}

fn put_uleb(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_source_mapping_url_appends_custom_section() {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        embed_source_mapping_url(&mut wasm, "app.wasm.map").unwrap();
        let payloads = custom_section_payloads(&wasm, b"sourceMappingURL").unwrap();
        assert_eq!(payloads, vec![b"app.wasm.map".as_slice()]);
    }
}

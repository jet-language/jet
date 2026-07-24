// D-ENCSTREAM-SURFACE1 hostile streaming law seam (card #711).
// Std-only hook for encoding codec FileReader/FileWriter I/O. Active only when
// JET_ENC_HOSTILE_IO=1; production callers never set it.

#[derive(Clone, Copy, Debug, Default)]
struct JetEncodingHostileIoPlan {
    read_one_byte: bool,
    write_chunk: usize,
    interrupt_reads: u8,
    interrupt_writes: u8,
    fail_read_after: Option<u64>,
    fail_write_after: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct JetEncodingHostileIoStats {
    read_calls: u64,
    write_calls: u64,
    flush_calls: u64,
    bytes_read: u64,
    bytes_written: u64,
    interrupts: u64,
}

struct JetEncodingHostileIoState {
    plan: Option<JetEncodingHostileIoPlan>,
    stats: JetEncodingHostileIoStats,
    read_total: u64,
    write_total: u64,
}

impl JetEncodingHostileIoState {
    fn active_plan(&self) -> Option<JetEncodingHostileIoPlan> {
        self.plan
    }
}

thread_local! {
    static JET_ENCODING_HOSTILE_IO: std::cell::RefCell<JetEncodingHostileIoState> =
        std::cell::RefCell::new(JetEncodingHostileIoState {
            plan: None,
            stats: JetEncodingHostileIoStats::default(),
            read_total: 0,
            write_total: 0,
        });
}

fn jet_encoding_hostile_io_enabled() -> bool {
    std::env::var_os("JET_ENC_HOSTILE_IO").is_some_and(|v| v != "0")
}

fn jet_encoding_hostile_io_plan_from_env() -> Option<JetEncodingHostileIoPlan> {
    if !jet_encoding_hostile_io_enabled() {
        return None;
    }
    let read_one_byte = std::env::var_os("JET_ENC_HOSTILE_READ_ONE").is_some_and(|v| v != "0");
    let write_chunk = std::env::var("JET_ENC_HOSTILE_WRITE_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let interrupt_reads = std::env::var("JET_ENC_HOSTILE_INTERRUPT_READS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let interrupt_writes = std::env::var("JET_ENC_HOSTILE_INTERRUPT_WRITES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let fail_read_after = std::env::var("JET_ENC_HOSTILE_FAIL_READ_AFTER")
        .ok()
        .and_then(|v| v.parse().ok());
    let fail_write_after = std::env::var("JET_ENC_HOSTILE_FAIL_WRITE_AFTER")
        .ok()
        .and_then(|v| v.parse().ok());
    Some(JetEncodingHostileIoPlan {
        read_one_byte,
        write_chunk,
        interrupt_reads,
        interrupt_writes,
        fail_read_after,
        fail_write_after,
    })
}

fn jet_encoding_hostile_io_with<F, R>(f: F) -> R
where
    F: FnOnce(&mut JetEncodingHostileIoState) -> R,
{
    JET_ENCODING_HOSTILE_IO.with(|cell| f(&mut cell.borrow_mut()))
}

fn jet_encoding_hostile_io_plan() -> Option<JetEncodingHostileIoPlan> {
    jet_encoding_hostile_io_with(|state| {
        if let Some(plan) = state.active_plan() {
            return Some(plan);
        }
        let plan = jet_encoding_hostile_io_plan_from_env()?;
        state.plan = Some(plan);
        Some(plan)
    })
}

fn jet_encoding_hostile_io_reset() {
    jet_encoding_hostile_io_with(|state| {
        *state = JetEncodingHostileIoState {
            plan: None,
            stats: JetEncodingHostileIoStats::default(),
            read_total: 0,
            write_total: 0,
        };
    });
}

fn jet_encoding_hostile_io_stats() -> JetEncodingHostileIoStats {
    jet_encoding_hostile_io_with(|state| state.stats)
}

fn jet_encoding_hostile_io_fail_io() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, "hostile encoding I/O failure")
}

fn jet_encoding_hostile_io_apply_interrupt(
    state: &mut JetEncodingHostileIoState,
    plan: JetEncodingHostileIoPlan,
    reads: bool,
) -> Result<(), std::io::Error> {
    let count = if reads {
        plan.interrupt_reads
    } else {
        plan.interrupt_writes
    };
    if count == 0 {
        return Ok(());
    }
    state.stats.interrupts += 1;
    if reads {
        state.plan = Some(JetEncodingHostileIoPlan {
            interrupt_reads: count - 1,
            ..plan
        });
    } else {
        state.plan = Some(JetEncodingHostileIoPlan {
            interrupt_writes: count - 1,
            ..plan
        });
    }
    Err(std::io::ErrorKind::Interrupted.into())
}

fn jet_encoding_file_read(reader: &mut JetFileReader, buf: &mut [u8]) -> std::io::Result<usize> {
    let Some(plan) = jet_encoding_hostile_io_plan() else {
        use std::io::Read;
        return reader.inner.read(buf);
    };
    jet_encoding_hostile_io_with(|state| {
        state.stats.read_calls += 1;
        if let Err(error) = jet_encoding_hostile_io_apply_interrupt(state, plan, true) {
            return Err(error);
        }
        if let Some(limit) = plan.fail_read_after {
            if state.read_total >= limit {
                return Err(jet_encoding_hostile_io_fail_io());
            }
        }
        use std::io::Read;
        let file = reader.inner.get_mut();
        let want = if plan.read_one_byte {
            1
        } else {
            buf.len()
        };
        let end = want.min(buf.len());
        let count = file.read(&mut buf[..end])?;
        if count == 0 {
            return Ok(0);
        }
        state.read_total += count as u64;
        state.stats.bytes_read += count as u64;
        if let Some(limit) = plan.fail_read_after {
            if state.read_total > limit {
                return Err(jet_encoding_hostile_io_fail_io());
            }
        }
        Ok(count)
    })
}

fn jet_encoding_file_write_all(writer: &mut JetFileWriter, bytes: &[u8]) -> std::io::Result<()> {
    let Some(plan) = jet_encoding_hostile_io_plan() else {
        use std::io::Write;
        return writer.inner.write_all(bytes);
    };
    jet_encoding_hostile_io_with(|state| {
        state.stats.write_calls += 1;
        if let Err(error) = jet_encoding_hostile_io_apply_interrupt(state, plan, false) {
            return Err(error);
        }
        if let Some(limit) = plan.fail_write_after {
            if state.write_total >= limit {
                return Err(jet_encoding_hostile_io_fail_io());
            }
        }
        use std::io::Write;
        let file = writer.inner.get_mut();
        let chunk = if plan.write_chunk == 0 {
            bytes.len()
        } else {
            plan.write_chunk
        };
        let mut offset = 0usize;
        while offset < bytes.len() {
            let end = (offset + chunk).min(bytes.len());
            let slice = &bytes[offset..end];
            if let Some(limit) = plan.fail_write_after {
                let next = state.write_total.saturating_add(slice.len() as u64);
                if next > limit {
                    let allowed = limit.saturating_sub(state.write_total) as usize;
                    if allowed == 0 {
                        return Err(jet_encoding_hostile_io_fail_io());
                    }
                    file.write_all(&slice[..allowed])?;
                    state.write_total += allowed as u64;
                    state.stats.bytes_written += allowed as u64;
                    return Err(jet_encoding_hostile_io_fail_io());
                }
            }
            file.write_all(slice)?;
            state.write_total += slice.len() as u64;
            state.stats.bytes_written += slice.len() as u64;
            offset = end;
        }
        Ok(())
    })
}

fn jet_encoding_file_flush(writer: &mut JetFileWriter) -> std::io::Result<()> {
    let Some(plan) = jet_encoding_hostile_io_plan() else {
        use std::io::Write;
        return writer.inner.flush();
    };
    jet_encoding_hostile_io_with(|state| {
        state.stats.flush_calls += 1;
        if let Err(error) = jet_encoding_hostile_io_apply_interrupt(state, plan, false) {
            return Err(error);
        }
        use std::io::Write;
        writer.inner.get_mut().flush()
    })
}

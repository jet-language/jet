// D-ENCSTREAM-SURFACE1 hostile streaming law seam (card #711 / #715 C6).
// Std-only hook for encoding codec FileReader/FileWriter I/O. Active only when
// JET_ENC_HOSTILE_IO=1; production callers never set it.
//
// Optional schedules (tests only):
//   JET_ENC_HOSTILE_READ_PLAN=n1,n2,...   max bytes per successive read; last repeats
//   JET_ENC_HOSTILE_WRITE_PLAN=n1,n2,...  max bytes per successive write slice; last repeats
//   JET_ENC_HOSTILE_READ_ONE=1            shorthand for every read capped at 1
//   JET_ENC_HOSTILE_WRITE_MAX=n           fixed write chunk (ignored when WRITE_PLAN set)

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
    read_plan: Vec<usize>,
    read_plan_idx: usize,
    write_plan: Vec<usize>,
    write_plan_idx: usize,
}

impl JetEncodingHostileIoState {
    fn fresh() -> Self {
        Self {
            plan: None,
            stats: JetEncodingHostileIoStats::default(),
            read_total: 0,
            write_total: 0,
            read_plan: Vec::new(),
            read_plan_idx: 0,
            write_plan: Vec::new(),
            write_plan_idx: 0,
        }
    }

    fn active_plan(&self) -> Option<JetEncodingHostileIoPlan> {
        self.plan
    }

    fn next_read_cap(&mut self, plan: JetEncodingHostileIoPlan, buf_len: usize) -> usize {
        if !self.read_plan.is_empty() {
            let i = self.read_plan_idx.min(self.read_plan.len() - 1);
            let cap = self.read_plan[i].max(1);
            if self.read_plan_idx < self.read_plan.len() {
                self.read_plan_idx += 1;
            }
            return cap.min(buf_len);
        }
        if plan.read_one_byte {
            1.min(buf_len)
        } else {
            buf_len
        }
    }

    fn next_write_chunk(&mut self, plan: JetEncodingHostileIoPlan, remaining: usize) -> usize {
        if !self.write_plan.is_empty() {
            let i = self.write_plan_idx.min(self.write_plan.len() - 1);
            let cap = self.write_plan[i].max(1);
            if self.write_plan_idx < self.write_plan.len() {
                self.write_plan_idx += 1;
            }
            return cap.min(remaining);
        }
        if plan.write_chunk == 0 {
            remaining
        } else {
            plan.write_chunk.min(remaining)
        }
    }
}

thread_local! {
    static JET_ENCODING_HOSTILE_IO: std::cell::RefCell<JetEncodingHostileIoState> =
        std::cell::RefCell::new(JetEncodingHostileIoState::fresh());
}

fn jet_encoding_hostile_io_enabled() -> bool {
    std::env::var_os("JET_ENC_HOSTILE_IO").is_some_and(|v| v != "0")
}

fn jet_encoding_hostile_parse_plan(raw: &str) -> Vec<usize> {
    raw.split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse::<usize>().ok().filter(|n| *n > 0)
            }
        })
        .collect()
}

fn jet_encoding_hostile_io_plan_from_env() -> Option<(JetEncodingHostileIoPlan, Vec<usize>, Vec<usize>)> {
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
    let read_plan = std::env::var("JET_ENC_HOSTILE_READ_PLAN")
        .ok()
        .map(|v| jet_encoding_hostile_parse_plan(&v))
        .unwrap_or_default();
    let write_plan = std::env::var("JET_ENC_HOSTILE_WRITE_PLAN")
        .ok()
        .map(|v| jet_encoding_hostile_parse_plan(&v))
        .unwrap_or_default();
    Some((
        JetEncodingHostileIoPlan {
            read_one_byte,
            write_chunk,
            interrupt_reads,
            interrupt_writes,
            fail_read_after,
            fail_write_after,
        },
        read_plan,
        write_plan,
    ))
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
        let (plan, read_plan, write_plan) = jet_encoding_hostile_io_plan_from_env()?;
        state.plan = Some(plan);
        state.read_plan = read_plan;
        state.write_plan = write_plan;
        state.read_plan_idx = 0;
        state.write_plan_idx = 0;
        Some(plan)
    })
}

fn jet_encoding_hostile_io_reset() {
    jet_encoding_hostile_io_with(|state| {
        *state = JetEncodingHostileIoState::fresh();
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
        let want = state.next_read_cap(plan, buf.len());
        let count = file.read(&mut buf[..want])?;
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
        let mut offset = 0usize;
        while offset < bytes.len() {
            let chunk = state.next_write_chunk(plan, bytes.len() - offset);
            let end = offset + chunk;
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

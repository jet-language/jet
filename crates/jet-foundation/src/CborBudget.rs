//! Shared whole-CBOR allocation accounting for every execution tier.

pub const DATA_TREE_SLOT_BYTES: usize = 32;
pub const MAP_ENTRY_SLOT_BYTES: usize = 56;

#[derive(Debug)]
pub struct CborAllocationError {
    limit: usize,
    what: &'static str,
}

impl CborAllocationError {
    fn limit_digits(&self) -> usize {
        let mut value = self.limit;
        let mut digits = 1;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        digits
    }

    pub fn reason_len(&self) -> usize {
        self.what
            .len()
            .saturating_add(" allocation exceeds max_bytes ".len())
            .saturating_add(self.limit_digits())
    }

    pub fn write_reason(&self, output: &mut String) {
        use std::fmt::Write as _;
        let _ = write!(
            output,
            "{} allocation exceeds max_bytes {}",
            self.what, self.limit
        );
    }
}

pub struct CborAllocationBudget {
    limit: usize,
    live: usize,
}

impl CborAllocationBudget {
    pub fn new(limit: usize) -> Self {
        Self { limit, live: 0 }
    }

    pub fn reserve(
        &mut self,
        count: usize,
        unit: usize,
        what: &'static str,
    ) -> Result<usize, CborAllocationError> {
        let requested = count.checked_mul(unit).ok_or_else(|| CborAllocationError {
            limit: self.limit,
            what,
        })?;
        let next = self
            .live
            .checked_add(requested)
            .ok_or_else(|| CborAllocationError {
                limit: self.limit,
                what,
            })?;
        if next > self.limit {
            return Err(CborAllocationError {
                limit: self.limit,
                what,
            });
        }
        self.live = next;
        Ok(requested)
    }

    pub fn release(&mut self, requested: usize) {
        self.live = self.live.saturating_sub(requested);
    }

    /// Record a terminal error's owned strings after decoding has failed.
    /// The diagnostic must keep its exact path and reason even when the data
    /// budget is already exhausted; no further codec work can retain data.
    pub fn reserve_terminal_error(&mut self, requested: usize) {
        self.live = self.live.saturating_add(requested);
    }

    /// Reserve a vector's replacement allocation while its old allocation is
    /// still live. All decoder vectors start empty and grow through this
    /// method, so `live` includes their actual capacities, not only lengths.
    pub fn reserve_vec<T>(
        &mut self,
        values: &mut Vec<T>,
        additional: usize,
        unit: usize,
        what: &'static str,
    ) -> Result<(), CborAllocationError> {
        let needed = values
            .len()
            .checked_add(additional)
            .ok_or_else(|| CborAllocationError {
                limit: self.limit,
                what,
            })?;
        if needed <= values.capacity() {
            return Ok(());
        }
        let slot = unit;
        if slot == 0 {
            values
                .try_reserve_exact(additional)
                .map_err(|_| CborAllocationError {
                    limit: self.limit,
                    what,
                })?;
            return Ok(());
        }
        let old_bytes = values
            .capacity()
            .checked_mul(slot)
            .ok_or_else(|| CborAllocationError {
                limit: self.limit,
                what,
            })?;
        let requested_bytes = needed
            .checked_mul(slot)
            .ok_or_else(|| CborAllocationError {
                limit: self.limit,
                what,
            })?;
        let retained = self
            .live
            .checked_sub(old_bytes)
            .ok_or_else(|| CborAllocationError {
                limit: self.limit,
                what,
            })?;
        let next = retained
            .checked_add(requested_bytes)
            .ok_or_else(|| CborAllocationError {
                limit: self.limit,
                what,
            })?;
        if next > self.limit {
            return Err(CborAllocationError {
                limit: self.limit,
                what,
            });
        }
        if values.try_reserve_exact(additional).is_err() {
            return Err(CborAllocationError {
                limit: self.limit,
                what,
            });
        }
        let actual_bytes =
            values
                .capacity()
                .checked_mul(slot)
                .ok_or_else(|| CborAllocationError {
                    limit: self.limit,
                    what,
                })?;
        let next = match retained.checked_add(actual_bytes) {
            Some(next) if next <= self.limit => next,
            _ => {
                let values = std::mem::take(values);
                drop(values);
                self.live = retained;
                return Err(CborAllocationError {
                    limit: self.limit,
                    what,
                });
            }
        };
        self.live = next;
        Ok(())
    }
}

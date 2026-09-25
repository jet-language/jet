use cranelift_codegen::binemit::Reloc;
use cranelift_module::ModuleReloc;
use cranelift_module::ModuleRelocTarget;
use cranelift_module::{ModuleError, ModuleResult};

fn checked_addend(base: *const u8, addend: i64) -> ModuleResult<*const u8> {
    let target = (base as usize as i128) + i128::from(addend);
    usize::try_from(target)
        .map(|target| target as *const u8)
        .map_err(|_| ModuleError::Backend(anyhow::anyhow!("relocation addend overflows address space")))
}

pub(crate) fn checked_rel32(target: *const u8, at: *const u8) -> ModuleResult<i32> {
    let displacement = (target as usize as i128) - (at as usize as i128);
    i32::try_from(displacement).map_err(|_| {
        ModuleError::Backend(anyhow::anyhow!(
            "x86-64 JIT relocation target {target:p} is outside signed rel32 reach from {at:p}"
        ))
    })
}

fn relocation_error(message: impl std::fmt::Display) -> ModuleError {
    ModuleError::Backend(anyhow::anyhow!("{message}"))
}

/// Reads a 32bit instruction at `iptr`, and writes it again after
/// being altered by `modifier`
unsafe fn modify_inst32(iptr: *mut u32, modifier: impl FnOnce(u32) -> u32) {
    let inst = iptr.read_unaligned();
    let new_inst = modifier(inst);
    iptr.write_unaligned(new_inst);
}

#[derive(Clone)]
pub(crate) struct CompiledBlob {
    pub(crate) ptr: *mut u8,
    pub(crate) size: usize,
    pub(crate) relocs: Vec<ModuleReloc>,
}

unsafe impl Send for CompiledBlob {}

impl CompiledBlob {
    pub(crate) fn perform_relocations(
        &self,
        get_address: impl Fn(&ModuleRelocTarget) -> ModuleResult<*const u8>,
        get_got_entry: impl Fn(&ModuleRelocTarget) -> ModuleResult<*const u8>,
        get_plt_entry: impl Fn(&ModuleRelocTarget) -> ModuleResult<*const u8>,
    ) -> ModuleResult<()> {
        use std::ptr::write_unaligned;

        for &ModuleReloc {
            kind,
            offset,
            ref name,
            addend,
        } in &self.relocs
        {
            let offset = usize::try_from(offset)
                .map_err(|_| relocation_error("relocation offset overflows address space"))?;
            let width = match kind {
                Reloc::Abs8 | Reloc::RiscvCallPlt => 8,
                _ => 4,
            };
            let _end = offset
                .checked_add(width)
                .filter(|end| *end <= self.size)
                .ok_or_else(|| relocation_error("relocation extends beyond compiled blob"))?;
            if self.ptr.is_null() {
                return Err(relocation_error("relocation has no compiled blob storage"));
            }
            let at = unsafe { self.ptr.add(offset) };

            match kind {
                Reloc::Abs4 => {
                    let what = checked_addend(get_address(name)?, addend)?;
                    let value = u32::try_from(what as usize)
                        .map_err(|_| relocation_error("absolute 32-bit relocation overflows"))?;
                    unsafe { write_unaligned(at as *mut u32, value) };
                }
                Reloc::Abs8 => {
                    let what = checked_addend(get_address(name)?, addend)?;
                    let value = u64::try_from(what as usize)
                        .map_err(|_| relocation_error("absolute 64-bit relocation overflows"))?;
                    unsafe { write_unaligned(at as *mut u64, value) };
                }
                Reloc::X86PCRel4 | Reloc::X86CallPCRel4 => {
                    let what = checked_addend(get_address(name)?, addend)?;
                    let pcrel = checked_rel32(what, at)?;
                    unsafe { write_unaligned(at as *mut i32, pcrel) };
                }
                Reloc::X86GOTPCRel4 => {
                    let what = checked_addend(get_got_entry(name)?, addend)?;
                    let pcrel = checked_rel32(what, at)?;
                    unsafe { write_unaligned(at as *mut i32, pcrel) };
                }
                Reloc::X86CallPLTRel4 => {
                    let what = checked_addend(get_plt_entry(name)?, addend)?;
                    let pcrel = checked_rel32(what, at)?;
                    unsafe { write_unaligned(at as *mut i32, pcrel) };
                }
                Reloc::S390xPCRel32Dbl | Reloc::S390xPLTRel32Dbl => {
                    let what = checked_addend(get_address(name)?, addend)? as usize as i128;
                    let pcrel = i32::try_from((what - (at as usize as i128)) >> 1)
                        .map_err(|_| relocation_error("s390x PC-relative relocation overflows"))?;
                    unsafe { write_unaligned(at as *mut i32, pcrel) };
                }
                Reloc::Arm64Call => {
                    let base = get_address(name)?;
                    let byte_diff = (base as usize as i128) - (at as usize as i128);
                    if byte_diff % 4 != 0 {
                        return Err(relocation_error("unaligned AArch64 call relocation"));
                    }
                    let diff = byte_diff >> 2;
                    if !(-(1i128 << 25)..(1i128 << 25)).contains(&diff) {
                        return Err(relocation_error("AArch64 call target is out of range"));
                    }
                    let imm26 = (diff as u32) & 0x03ff_ffff;
                    unsafe { modify_inst32(at as *mut u32, |inst| inst | imm26) };
                }
                Reloc::Aarch64AdrGotPage21 => {
                    if addend != 0 {
                        return Err(relocation_error(
                            "AArch64 GOT page relocation does not support addends",
                        ));
                    }
                    let what = get_got_entry(name)? as usize;
                    let what_page = (what & !0xfff) as i128;
                    let at_page = ((at as usize) & !0xfff) as i128;
                    let pcrel = what_page - at_page;
                    if !(-(1i128 << 32)..(1i128 << 32)).contains(&pcrel) {
                        return Err(relocation_error("AArch64 GOT page is out of range"));
                    }
                    let val = pcrel >> 12;

                    let immlo = ((val as u32) & 0b11) << 29;
                    let immhi = (((val as u32) >> 2) & 0x7ffff) << 5;
                    let mask = !((0x7ffff << 5) | (0b11 << 29));
                    unsafe { modify_inst32(at as *mut u32, |adrp| (adrp & mask) | immlo | immhi) };
                }
                Reloc::Aarch64Ld64GotLo12Nc => {
                    if addend != 0 {
                        return Err(relocation_error(
                            "AArch64 GOT load relocation does not support addends",
                        ));
                    }
                    let what = get_got_entry(name)? as usize;
                    if what & 0b111 != 0 {
                        return Err(relocation_error("AArch64 GOT entry is not 8-byte aligned"));
                    }
                    let val = (what as u32) >> 3;
                    let imm9 = (val & 0x1ff) << 10;
                    let mask = !(0x1ff << 10);
                    unsafe { modify_inst32(at as *mut u32, |ldr| (ldr & mask) | imm9) };
                }
                Reloc::RiscvCallPlt => {
                    // A R_RISCV_CALL_PLT relocation expects auipc+jalr instruction pair.
                    // It is the equivalent of two relocations:
                    // 1. R_RISCV_PCREL_HI20 on the `auipc`
                    // 2. R_RISCV_PCREL_LO12_I on the `jalr`

                    let what = checked_addend(get_address(name)?, addend)? as usize as i128;
                    let pcrel = i32::try_from(what - (at as usize as i128))
                        .map_err(|_| relocation_error("RISC-V call relocation is out of range"))?
                        as u32;

                    // See https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc#pc-relative-symbol-addresses
                    // for a better explanation of the following code.
                    //
                    // Unlike the regular symbol relocations, here both "sub-relocations" point to the same address.
                    //
                    // `pcrel` is a signed value (+/- 2GiB range), when splitting it into two parts, we need to
                    // ensure that `hi20` is close enough to `pcrel` to be able to add `lo12` to it and still
                    // get a valid address.
                    //
                    // `lo12` is also a signed offset (+/- 2KiB range) relative to the `hi20` value.
                    //
                    // `hi20` should also be shifted right to be the "true" value. But we also need it
                    // left shifted for the `lo12` calculation and it also matches the instruction encoding.
                    let hi20 = pcrel.wrapping_add(0x800) & 0xFFFFF000;
                    let lo12 = pcrel.wrapping_sub(hi20) & 0xFFF;

                    unsafe {
                        // Do a R_RISCV_PCREL_HI20 on the `auipc`
                        let auipc_addr = at as *mut u32;
                        modify_inst32(auipc_addr, |auipc| (auipc & 0xFFF) | hi20);

                        // Do a R_RISCV_PCREL_LO12_I on the `jalr`
                        let jalr_addr = at.add(4) as *mut u32;
                        modify_inst32(jalr_addr, |jalr| (jalr & 0xFFFFF) | (lo12 << 20));
                    }
                }
                _ => return Err(relocation_error(format!("unsupported relocation kind {kind:?}"))),
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::{checked_rel32, CompiledBlob};
    use cranelift_codegen::binemit::Reloc;
    use cranelift_module::{ModuleReloc, ModuleRelocTarget};

    #[test]
    fn rel32_boundaries_and_both_overflow_directions() {
        let at = 8usize << 30;
        let at_ptr = at as *const u8;

        assert_eq!(
            checked_rel32((at + i32::MAX as usize) as *const u8, at_ptr).unwrap(),
            i32::MAX
        );
        assert_eq!(
            checked_rel32((at - (1usize << 31)) as *const u8, at_ptr).unwrap(),
            i32::MIN
        );
        assert!(checked_rel32((at + (1usize << 31)) as *const u8, at_ptr).is_err());
        assert!(checked_rel32((at - (1usize << 31) - 1) as *const u8, at_ptr).is_err());
    }

    fn x86_relocation_fails_at_distance(kind: Reloc, distance: i64) -> bool {
        let mut storage = [0u8; 4];
        let at = storage.as_mut_ptr();
        let address = at as usize;
        let target = if distance >= 0 {
            address.checked_add(distance as usize).unwrap()
        } else {
            address.checked_sub(distance.unsigned_abs() as usize).unwrap()
        };
        let blob = CompiledBlob {
            ptr: at,
            size: storage.len(),
            relocs: vec![ModuleReloc {
                offset: 0,
                kind,
                name: ModuleRelocTarget::user(0, 0),
                addend: 0,
            }],
        };
        blob.perform_relocations(
            |_| Ok(target as *const u8),
            |_| Ok(target as *const u8),
            |_| Ok(target as *const u8),
        )
        .is_err()
    }

    #[test]
    fn code_got_and_plt_relocations_reject_both_rel32_overflow_directions() {
        for kind in [
            Reloc::X86PCRel4,
            Reloc::X86CallPCRel4,
            Reloc::X86GOTPCRel4,
            Reloc::X86CallPLTRel4,
        ] {
            assert!(x86_relocation_fails_at_distance(kind, 1i64 << 31));
            assert!(x86_relocation_fails_at_distance(
                kind,
                -((1i64 << 31) + 1)
            ));
        }
    }
}

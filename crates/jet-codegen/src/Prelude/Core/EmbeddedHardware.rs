// D-FOUND-BOARD1=A / I9: one target-profile hardware kernel shared by every
// execution tier.  A target adapter supplies the generated fact slices and
// chooses the register backend; this file owns width/access checks, volatile
// operations, bounded interrupt delivery, and DMA ownership transitions.
//
// The generated register types carry width, access mode, and volatility in
// their Rust type.  The target backend is the only place that performs raw
// pointer operations.  Host replay uses JetRegisterReplay with the same
// generated register type and the same JetHardwareFacts; it is not a second
// semantic implementation.

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

// ── Target facts consumed by generated adapters ─────────────────────────────

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetRegisterWidth {
    U8,
    U16,
    U32,
    U64,
}

impl JetRegisterWidth {
    pub const fn bytes(self) -> u64 {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }

    pub const fn bits(self) -> u16 {
        (self.bytes() * 8) as u16
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetRegisterAccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

impl JetRegisterAccessMode {
    pub const fn can_read(self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadWrite)
    }

    pub const fn can_write(self) -> bool {
        matches!(self, Self::WriteOnly | Self::ReadWrite)
    }
}

/// The generated type uses one of these marker types for a register's access
/// mode.  A read method is not implemented for `JetWriteOnly`, and a write
/// method is not implemented for `JetReadOnly`; an invalid operation therefore
/// fails during generated Rust type checking rather than at run time.
pub trait JetRegisterAccess {
    const MODE: JetRegisterAccessMode;
}

pub struct JetReadOnly;
pub struct JetWriteOnly;
pub struct JetReadWrite;

impl JetRegisterAccess for JetReadOnly {
    const MODE: JetRegisterAccessMode = JetRegisterAccessMode::ReadOnly;
}
impl JetRegisterAccess for JetWriteOnly {
    const MODE: JetRegisterAccessMode = JetRegisterAccessMode::WriteOnly;
}
impl JetRegisterAccess for JetReadWrite {
    const MODE: JetRegisterAccessMode = JetRegisterAccessMode::ReadWrite;
}

pub trait JetRegisterReadable: JetRegisterAccess {}
pub trait JetRegisterWritable: JetRegisterAccess {}

impl JetRegisterReadable for JetReadOnly {}
impl JetRegisterReadable for JetReadWrite {}
impl JetRegisterWritable for JetWriteOnly {}
impl JetRegisterWritable for JetReadWrite {}

/// Values supported by generated register widths.  The conversion is bitwise,
/// so a register's unsigned representation is preserved on both target and
/// replay backends.
pub trait JetRegisterValue: Copy {
    const WIDTH: JetRegisterWidth;

    fn from_bits(bits: u64) -> Self;
    fn into_bits(self) -> u64;
}

impl JetRegisterValue for u8 {
    const WIDTH: JetRegisterWidth = JetRegisterWidth::U8;

    fn from_bits(bits: u64) -> Self {
        bits as u8
    }

    fn into_bits(self) -> u64 {
        self as u64
    }
}

impl JetRegisterValue for u16 {
    const WIDTH: JetRegisterWidth = JetRegisterWidth::U16;

    fn from_bits(bits: u64) -> Self {
        bits as u16
    }

    fn into_bits(self) -> u64 {
        self as u64
    }
}

impl JetRegisterValue for u32 {
    const WIDTH: JetRegisterWidth = JetRegisterWidth::U32;

    fn from_bits(bits: u64) -> Self {
        bits as u32
    }

    fn into_bits(self) -> u64 {
        self as u64
    }
}

impl JetRegisterValue for u64 {
    const WIDTH: JetRegisterWidth = JetRegisterWidth::U64;

    fn from_bits(bits: u64) -> Self {
        bits
    }

    fn into_bits(self) -> u64 {
        self
    }
}

/// One generated register fact.  `JetRegisterBlockSpec` and
/// `JetHardwareFacts` are emitted from the compiler's `TargetHardwareFacts`;
/// they deliberately contain no user-provided address or unsafe gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetRegisterSpec<'a> {
    pub name: &'a str,
    pub offset: u64,
    pub width: JetRegisterWidth,
    pub access: JetRegisterAccessMode,
    pub volatile: bool,
}

impl<'a> JetRegisterSpec<'a> {
    pub const fn new(
        name: &'a str,
        offset: u64,
        width: JetRegisterWidth,
        access: JetRegisterAccessMode,
        volatile: bool,
    ) -> Self {
        Self {
            name,
            offset,
            width,
            access,
            volatile,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetRegisterBlockSpec<'a> {
    pub name: &'a str,
    pub base: u64,
    pub size_bytes: u64,
    pub registers: &'a [JetRegisterSpec<'a>],
}

impl<'a> JetRegisterBlockSpec<'a> {
    pub const fn new(
        name: &'a str,
        base: u64,
        size_bytes: u64,
        registers: &'a [JetRegisterSpec<'a>],
    ) -> Self {
        Self {
            name,
            base,
            size_bytes,
            registers,
        }
    }

    pub fn register(&self, name: &str) -> Option<&JetRegisterSpec<'a>> {
        self.registers.iter().find(|register| register.name == name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetInterruptSpec<'a> {
    pub name: &'a str,
    pub vector: u16,
    pub forbidden_effects: &'a [&'a str],
    pub bounded: bool,
}

impl<'a> JetInterruptSpec<'a> {
    pub const fn new(name: &'a str, vector: u16, bounded: bool) -> Self {
        Self {
            name,
            vector,
            forbidden_effects: &[],
            bounded,
        }
    }

    pub const fn with_effects(
        name: &'a str,
        vector: u16,
        forbidden_effects: &'a [&'a str],
        bounded: bool,
    ) -> Self {
        Self {
            name,
            vector,
            forbidden_effects,
            bounded,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDmaOwnership {
    Borrowed,
    Transfer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDmaChannelSpec<'a> {
    pub name: &'a str,
    pub channel: u16,
    pub transfer_width: JetRegisterWidth,
    pub ownership: JetDmaOwnership,
    pub max_transfer_bytes: Option<u64>,
}

impl<'a> JetDmaChannelSpec<'a> {
    pub const fn new(name: &'a str, channel: u16) -> Self {
        Self {
            name,
            channel,
            transfer_width: JetRegisterWidth::U8,
            ownership: JetDmaOwnership::Transfer,
            max_transfer_bytes: None,
        }
    }

    pub const fn with_transfer(
        name: &'a str,
        channel: u16,
        transfer_width: JetRegisterWidth,
        ownership: JetDmaOwnership,
        max_transfer_bytes: Option<u64>,
    ) -> Self {
        Self {
            name,
            channel,
            transfer_width,
            ownership,
            max_transfer_bytes,
        }
    }
}

/// The target adapter lowers the compiler's `TargetHardwareFacts` into these
/// immutable slices.  Every hardware operation below consults this same fact
/// plane, including host replay.
#[derive(Clone, Copy)]
pub struct JetHardwareFacts<'a> {
    pub profile_id: Option<&'a str>,
    pub capabilities: &'a [&'a str],
    pub svd_source: Option<&'a str>,
    pub svd_sha256: Option<&'a str>,
    pub register_blocks: &'a [JetRegisterBlockSpec<'a>],
    pub interrupts: &'a [JetInterruptSpec<'a>],
    pub dma_channels: &'a [JetDmaChannelSpec<'a>],
}

impl<'a> JetHardwareFacts<'a> {
    pub const fn new(
        register_blocks: &'a [JetRegisterBlockSpec<'a>],
        interrupts: &'a [JetInterruptSpec<'a>],
        dma_channels: &'a [JetDmaChannelSpec<'a>],
    ) -> Self {
        Self {
            profile_id: None,
            capabilities: &[],
            svd_source: None,
            svd_sha256: None,
            register_blocks,
            interrupts,
            dma_channels,
        }
    }

    pub const fn with_svd(
        svd_source: &'a str,
        svd_sha256: &'a str,
        register_blocks: &'a [JetRegisterBlockSpec<'a>],
        interrupts: &'a [JetInterruptSpec<'a>],
        dma_channels: &'a [JetDmaChannelSpec<'a>],
    ) -> Self {
        Self {
            profile_id: None,
            capabilities: &[],
            svd_source: Some(svd_source),
            svd_sha256: Some(svd_sha256),
            register_blocks,
            interrupts,
            dma_channels,
        }
    }

    pub const fn with_profile(
        profile_id: &'a str,
        capabilities: &'a [&'a str],
        svd_source: Option<&'a str>,
        svd_sha256: Option<&'a str>,
        register_blocks: &'a [JetRegisterBlockSpec<'a>],
        interrupts: &'a [JetInterruptSpec<'a>],
        dma_channels: &'a [JetDmaChannelSpec<'a>],
    ) -> Self {
        Self {
            profile_id: Some(profile_id),
            capabilities,
            svd_source,
            svd_sha256,
            register_blocks,
            interrupts,
            dma_channels,
        }
    }

    pub fn register_block(&self, name: &str) -> Option<&JetRegisterBlockSpec<'a>> {
        self.register_blocks
            .iter()
            .find(|block| block.name == name)
    }

    pub fn interrupt(&self, selector: &str) -> Option<&JetInterruptSpec<'a>> {
        let vector = selector.parse::<u16>().ok();
        self.interrupts.iter().find(|interrupt| {
            interrupt.name == selector || vector.is_some_and(|value| value == interrupt.vector)
        })
    }

    pub fn interrupt_vector(&self, vector: u16) -> Option<&JetInterruptSpec<'a>> {
        self.interrupts
            .iter()
            .find(|interrupt| interrupt.vector == vector)
    }

    pub fn dma_channel(&self, name: &str) -> Option<&JetDmaChannelSpec<'a>> {
        self.dma_channels
            .iter()
            .find(|channel| channel.name == name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetHardwareRegisterFact<'a> {
    pub block: &'a str,
    pub register: &'a str,
    pub base: u64,
    pub offset: u64,
    pub width: JetRegisterWidth,
    pub access: JetRegisterAccessMode,
    pub volatile: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetHardwareInterruptFact<'a> {
    pub name: &'a str,
    pub vector: u16,
    pub bounded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetHardwareDmaFact<'a> {
    pub name: &'a str,
    pub channel: u16,
    pub transfer_width: JetRegisterWidth,
    pub ownership: JetDmaOwnership,
    pub max_transfer_bytes: Option<u64>,
}

/// Checked target facts are the only semantic input to the replay host. AOT
/// uses the emitted static facts; resident adapters implement this trait for
/// the canonical owned `TargetHardwareFacts` without copying policy.
pub trait JetHardwareFactSource {
    fn profile_id(&self) -> Option<&str> {
        None
    }

    fn register_fact(
        &self,
        block: &str,
        register: &str,
    ) -> Option<JetHardwareRegisterFact<'_>>;

    fn interrupt_fact(&self, selector: &str) -> Option<JetHardwareInterruptFact<'_>>;

    fn interrupt_vector_fact(&self, vector: u16) -> Option<JetHardwareInterruptFact<'_>>;

    fn dma_fact(&self, channel: &str) -> Option<JetHardwareDmaFact<'_>>;
}

impl<'a> JetHardwareFactSource for JetHardwareFacts<'a> {
    fn profile_id(&self) -> Option<&str> {
        self.profile_id
    }

    fn register_fact(
        &self,
        block: &str,
        register: &str,
    ) -> Option<JetHardwareRegisterFact<'_>> {
        let block_fact = self.register_block(block)?;
        let register_fact = block_fact.register(register)?;
        Some(JetHardwareRegisterFact {
            block: block_fact.name,
            register: register_fact.name,
            base: block_fact.base,
            offset: register_fact.offset,
            width: register_fact.width,
            access: register_fact.access,
            volatile: register_fact.volatile,
        })
    }

    fn interrupt_fact(&self, selector: &str) -> Option<JetHardwareInterruptFact<'_>> {
        self.interrupt(selector).map(|interrupt| JetHardwareInterruptFact {
            name: interrupt.name,
            vector: interrupt.vector,
            bounded: interrupt.bounded,
        })
    }

    fn interrupt_vector_fact(&self, vector: u16) -> Option<JetHardwareInterruptFact<'_>> {
        self.interrupt_vector(vector)
            .map(|interrupt| JetHardwareInterruptFact {
                name: interrupt.name,
                vector: interrupt.vector,
                bounded: interrupt.bounded,
            })
    }

    fn dma_fact(&self, channel: &str) -> Option<JetHardwareDmaFact<'_>> {
        self.dma_channel(channel).map(|dma| JetHardwareDmaFact {
            name: dma.name,
            channel: dma.channel,
            transfer_width: dma.transfer_width,
            ownership: dma.ownership,
            max_transfer_bytes: dma.max_transfer_bytes,
        })
    }
}

impl From<jet_foundation::TargetMachine::RegisterWidth> for JetRegisterWidth {
    fn from(width: jet_foundation::TargetMachine::RegisterWidth) -> Self {
        match width {
            jet_foundation::TargetMachine::RegisterWidth::U8 => Self::U8,
            jet_foundation::TargetMachine::RegisterWidth::U16 => Self::U16,
            jet_foundation::TargetMachine::RegisterWidth::U32 => Self::U32,
            jet_foundation::TargetMachine::RegisterWidth::U64 => Self::U64,
        }
    }
}

impl From<jet_foundation::TargetMachine::TargetRegisterAccessMode> for JetRegisterAccessMode {
    fn from(mode: jet_foundation::TargetMachine::TargetRegisterAccessMode) -> Self {
        match mode {
            jet_foundation::TargetMachine::TargetRegisterAccessMode::ReadOnly => Self::ReadOnly,
            jet_foundation::TargetMachine::TargetRegisterAccessMode::WriteOnly => Self::WriteOnly,
            jet_foundation::TargetMachine::TargetRegisterAccessMode::ReadWrite => Self::ReadWrite,
        }
    }
}

impl From<jet_foundation::TargetMachine::TargetDmaOwnership> for JetDmaOwnership {
    fn from(ownership: jet_foundation::TargetMachine::TargetDmaOwnership) -> Self {
        match ownership {
            jet_foundation::TargetMachine::TargetDmaOwnership::Borrowed => Self::Borrowed,
            jet_foundation::TargetMachine::TargetDmaOwnership::Transfer => Self::Transfer,
        }
    }
}

impl JetHardwareFactSource for jet_foundation::TargetMachine::TargetHardwareFacts {
    fn register_fact(
        &self,
        block: &str,
        register: &str,
    ) -> Option<JetHardwareRegisterFact<'_>> {
        let block_fact = self.register_block(block)?;
        let register_fact = block_fact.register(register)?;
        Some(JetHardwareRegisterFact {
            block: block_fact.name.as_str(),
            register: register_fact.name.as_str(),
            base: block_fact.base,
            offset: register_fact.offset,
            width: register_fact.width.into(),
            access: register_fact.access.into(),
            volatile: register_fact.volatile,
        })
    }

    fn interrupt_fact(&self, selector: &str) -> Option<JetHardwareInterruptFact<'_>> {
        self.interrupt(selector).map(|interrupt| JetHardwareInterruptFact {
            name: interrupt.name.as_str(),
            vector: interrupt.vector,
            bounded: interrupt.bounded,
        })
    }

    fn interrupt_vector_fact(&self, vector: u16) -> Option<JetHardwareInterruptFact<'_>> {
        self.interrupts
            .iter()
            .find(|interrupt| interrupt.vector == vector)
            .map(|interrupt| JetHardwareInterruptFact {
                name: interrupt.name.as_str(),
                vector: interrupt.vector,
                bounded: interrupt.bounded,
            })
    }

    fn dma_fact(&self, channel: &str) -> Option<JetHardwareDmaFact<'_>> {
        self.dma_channel(channel).map(|dma| JetHardwareDmaFact {
            name: dma.name.as_str(),
            channel: dma.channel,
            transfer_width: dma.transfer_width.into(),
            ownership: dma.ownership.into(),
            max_transfer_bytes: dma.max_transfer_bytes,
        })
    }
}

// ── Typed register block ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetRegisterError {
    UnknownRegister,
    WidthMismatch {
        expected: JetRegisterWidth,
        actual: JetRegisterWidth,
    },
    AccessMismatch {
        expected: JetRegisterAccessMode,
        actual: JetRegisterAccessMode,
    },
    VolatileMismatch {
        expected: bool,
        actual: bool,
    },
    AddressOverflow,
    Misaligned,
}

/// A register backend is deliberately tiny.  The generated type supplies the
/// width and volatility as facts; the target adapter supplies only storage.
pub trait JetRegisterBackend {
    fn read(&self, address: u64, width: JetRegisterWidth, volatile_access: bool) -> u64;

    fn write(&self, address: u64, width: JetRegisterWidth, value: u64, volatile_access: bool);
}

/// A typed register.  Width and access are type parameters, and volatility is
/// a const parameter, so generated fields cannot silently widen, reverse an
/// access mode, or turn a volatile fact into an ordinary load.
pub struct JetRegister<'a, W, A, B, const VOLATILE: bool>
where
    W: JetRegisterValue,
    A: JetRegisterAccess,
    B: JetRegisterBackend,
{
    backend: &'a B,
    address: u64,
    marker: PhantomData<fn() -> (W, A)>,
}

impl<'a, W, A, B, const VOLATILE: bool> JetRegister<'a, W, A, B, VOLATILE>
where
    W: JetRegisterValue,
    A: JetRegisterAccess,
    B: JetRegisterBackend,
{
    pub fn new(backend: &'a B, address: u64) -> Self {
        Self {
            backend,
            address,
            marker: PhantomData,
        }
    }

    pub fn from_spec(
        backend: &'a B,
        base: u64,
        spec: &JetRegisterSpec<'_>,
    ) -> Result<Self, JetRegisterError> {
        if spec.width != W::WIDTH {
            return Err(JetRegisterError::WidthMismatch {
                expected: spec.width,
                actual: W::WIDTH,
            });
        }
        if spec.access != A::MODE {
            return Err(JetRegisterError::AccessMismatch {
                expected: spec.access,
                actual: A::MODE,
            });
        }
        if spec.volatile != VOLATILE {
            return Err(JetRegisterError::VolatileMismatch {
                expected: spec.volatile,
                actual: VOLATILE,
            });
        }
        let address = base
            .checked_add(spec.offset)
            .ok_or(JetRegisterError::AddressOverflow)?;
        if address % spec.width.bytes() != 0 {
            return Err(JetRegisterError::Misaligned);
        }
        Ok(Self::new(backend, address))
    }

    pub const fn address(&self) -> u64 {
        self.address
    }
}

impl<'a, W, A, B, const VOLATILE: bool> JetRegister<'a, W, A, B, VOLATILE>
where
    W: JetRegisterValue,
    A: JetRegisterReadable,
    B: JetRegisterBackend,
{
    pub fn read(&self) -> W {
        W::from_bits(self.backend.read(self.address, W::WIDTH, VOLATILE))
    }
}

impl<'a, W, A, B, const VOLATILE: bool> JetRegister<'a, W, A, B, VOLATILE>
where
    W: JetRegisterValue,
    A: JetRegisterWritable,
    B: JetRegisterBackend,
{
    pub fn write(&self, value: W) {
        self.backend
            .write(self.address, W::WIDTH, value.into_bits(), VOLATILE);
    }
}

/// A generated block adapter can use this helper when emitting named fields.
/// The returned field still carries its generated width/access/volatile type.
pub struct JetRegisterBlock<'backend, 'spec, B>
where
    B: JetRegisterBackend,
{
    backend: &'backend B,
    spec: &'spec JetRegisterBlockSpec<'spec>,
}

impl<'backend, 'spec, B> JetRegisterBlock<'backend, 'spec, B>
where
    B: JetRegisterBackend,
{
    pub fn new(backend: &'backend B, spec: &'spec JetRegisterBlockSpec<'spec>) -> Self {
        Self { backend, spec }
    }

    pub const fn name(&self) -> &'spec str {
        self.spec.name
    }

    pub const fn base(&self) -> u64 {
        self.spec.base
    }

    pub fn register<W, A, const VOLATILE: bool>(
        &self,
        name: &str,
    ) -> Result<JetRegister<'backend, W, A, B, VOLATILE>, JetRegisterError>
    where
        W: JetRegisterValue,
        A: JetRegisterAccess,
    {
        let spec = self
            .spec
            .register(name)
            .ok_or(JetRegisterError::UnknownRegister)?;
        JetRegister::from_spec(self.backend, self.spec.base, spec)
    }
}

/// The only backend that performs target MMIO.  The selected profile has
/// already validated the address, width, alignment, and volatile fact before
/// generated code reaches this kernel.  User code never enters this block.
#[derive(Clone, Copy, Debug, Default)]
pub struct JetVolatileRegisterBackend;

impl JetRegisterBackend for JetVolatileRegisterBackend {
    fn read(&self, address: u64, width: JetRegisterWidth, volatile_access: bool) -> u64 {
        // SAFETY: TargetMachineFacts validate the MMIO region and generated
        // register types validate width/alignment.  This is the vetted target
        // adapter seam; no user pointer or address reaches these operations.
        unsafe {
            match (width, volatile_access) {
                (JetRegisterWidth::U8, true) => core::ptr::read_volatile(address as *const u8) as u64,
                (JetRegisterWidth::U16, true) => {
                    core::ptr::read_volatile(address as *const u16) as u64
                }
                (JetRegisterWidth::U32, true) => {
                    core::ptr::read_volatile(address as *const u32) as u64
                }
                (JetRegisterWidth::U64, true) => core::ptr::read_volatile(address as *const u64),
                (JetRegisterWidth::U8, false) => core::ptr::read(address as *const u8) as u64,
                (JetRegisterWidth::U16, false) => core::ptr::read(address as *const u16) as u64,
                (JetRegisterWidth::U32, false) => core::ptr::read(address as *const u32) as u64,
                (JetRegisterWidth::U64, false) => core::ptr::read(address as *const u64),
            }
        }
    }

    fn write(&self, address: u64, width: JetRegisterWidth, value: u64, volatile_access: bool) {
        // SAFETY: see `read`; the generated profile fact owns the address and
        // width, and the operation is confined to this vetted kernel.
        unsafe {
            match (width, volatile_access) {
                (JetRegisterWidth::U8, true) => {
                    core::ptr::write_volatile(address as *mut u8, value as u8)
                }
                (JetRegisterWidth::U16, true) => {
                    core::ptr::write_volatile(address as *mut u16, value as u16)
                }
                (JetRegisterWidth::U32, true) => {
                    core::ptr::write_volatile(address as *mut u32, value as u32)
                }
                (JetRegisterWidth::U64, true) => {
                    core::ptr::write_volatile(address as *mut u64, value)
                }
                (JetRegisterWidth::U8, false) => core::ptr::write(address as *mut u8, value as u8),
                (JetRegisterWidth::U16, false) => {
                    core::ptr::write(address as *mut u16, value as u16)
                }
                (JetRegisterWidth::U32, false) => {
                    core::ptr::write(address as *mut u32, value as u32)
                }
                (JetRegisterWidth::U64, false) => core::ptr::write(address as *mut u64, value),
            }
        }
    }
}
// The no-OS target path cannot depend on `std` or allocator-backed containers.
// Keep interrupt state in fixed storage; the hosted replay bridge below uses the
// same handler carrier and registry API.
#[cfg(target_os = "none")]
const JET_HARDWARE_INTERRUPT_CAPACITY: usize = 64;
#[cfg(target_os = "none")]
const JET_HARDWARE_PENDING_CAPACITY: usize = 64;

#[cfg(target_os = "none")]
#[derive(Clone, Copy)]
pub struct JetHardwareInterruptHandler {
    pub fn_ptr: i64,
    pub env: i64,
    pub has_env: bool,
}

#[cfg(target_os = "none")]
#[derive(Clone, Copy)]
pub struct JetHardwareInterruptRegistry {
    handlers: [Option<(u16, JetHardwareInterruptHandler)>; JET_HARDWARE_INTERRUPT_CAPACITY],
}

#[cfg(target_os = "none")]
impl Default for JetHardwareInterruptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "none")]
impl JetHardwareInterruptRegistry {
    pub const fn new() -> Self {
        Self {
            handlers: [None; JET_HARDWARE_INTERRUPT_CAPACITY],
        }
    }

    pub fn clear(&mut self) {
        self.handlers = [None; JET_HARDWARE_INTERRUPT_CAPACITY];
    }

    pub fn register(
        &mut self,
        vector: u16,
        handler: JetHardwareInterruptHandler,
    ) -> bool {
        if self
            .handlers
            .iter()
            .flatten()
            .any(|(bound, _)| *bound == vector)
        {
            return false;
        }
        let Some(slot) = self.handlers.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some((vector, handler));
        true
    }

    fn handler(&self, vector: u16) -> Option<JetHardwareInterruptHandler> {
        self.handlers
            .iter()
            .flatten()
            .find_map(|(bound, handler)| (*bound == vector).then_some(*handler))
    }
}

#[cfg(target_os = "none")]
#[derive(Clone, Copy)]
pub struct JetHardwarePendingCallbacks {
    callbacks: [Option<JetHardwareInterruptHandler>; JET_HARDWARE_PENDING_CAPACITY],
    len: usize,
}

#[cfg(target_os = "none")]
impl JetHardwarePendingCallbacks {
    const fn empty() -> Self {
        Self {
            callbacks: [None; JET_HARDWARE_PENDING_CAPACITY],
            len: 0,
        }
    }
}

#[cfg(target_os = "none")]
static JET_HARDWARE_PENDING_VECTORS: [core::sync::atomic::AtomicU16; JET_HARDWARE_PENDING_CAPACITY] =
    [const { core::sync::atomic::AtomicU16::new(0) }; JET_HARDWARE_PENDING_CAPACITY];
#[cfg(target_os = "none")]
static JET_HARDWARE_PENDING_HEAD: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
#[cfg(target_os = "none")]
static JET_HARDWARE_PENDING_TAIL: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

#[cfg(target_os = "none")]
pub fn jet_hardware_has_pending() -> bool {
    JET_HARDWARE_PENDING_HEAD.load(Ordering::Acquire)
        != JET_HARDWARE_PENDING_TAIL.load(Ordering::Acquire)
}

#[cfg(target_os = "none")]
pub fn jet_hardware_enqueue_pending(vector: u16) {
    let tail = JET_HARDWARE_PENDING_TAIL.load(Ordering::Relaxed);
    let head = JET_HARDWARE_PENDING_HEAD.load(Ordering::Acquire);
    if tail.wrapping_sub(head) >= JET_HARDWARE_PENDING_CAPACITY {
        return;
    }
    JET_HARDWARE_PENDING_VECTORS[tail % JET_HARDWARE_PENDING_CAPACITY]
        .store(vector, Ordering::Relaxed);
    JET_HARDWARE_PENDING_TAIL.store(tail.wrapping_add(1), Ordering::Release);
}

#[cfg(target_os = "none")]
pub fn jet_hardware_dispatch_pending(
    registry: &JetHardwareInterruptRegistry,
) -> JetHardwarePendingCallbacks {
    let mut callbacks = JetHardwarePendingCallbacks::empty();
    let mut head = JET_HARDWARE_PENDING_HEAD.load(Ordering::Relaxed);
    let tail = JET_HARDWARE_PENDING_TAIL.load(Ordering::Acquire);
    while head != tail && callbacks.len < callbacks.callbacks.len() {
        let vector = JET_HARDWARE_PENDING_VECTORS[head % JET_HARDWARE_PENDING_CAPACITY]
            .load(Ordering::Acquire);
        if let Some(handler) = registry.handler(vector) {
            callbacks.callbacks[callbacks.len] = Some(handler);
            callbacks.len += 1;
        }
        head = head.wrapping_add(1);
    }
    JET_HARDWARE_PENDING_HEAD.store(head, Ordering::Release);
    callbacks
}

#[cfg(target_os = "none")]
pub fn jet_hardware_invoke_pending(callbacks: JetHardwarePendingCallbacks) -> usize {
    let delivered = callbacks.len;
    for callback in callbacks.callbacks[..callbacks.len].iter().flatten() {
        // SAFETY: generated target code inserts only checked C-ABI handlers.
        unsafe {
            if callback.has_env {
                let invoke: extern "C" fn(i64) =
                    core::mem::transmute(callback.fn_ptr as usize);
                invoke(callback.env);
            } else {
                let invoke: extern "C" fn() =
                    core::mem::transmute(callback.fn_ptr as usize);
                invoke();
            }
        }
    }
    delivered
}

// JET_EMBEDDED_HARDWARE_HOST_BEGIN: std-only replay and resident bridge.
// ── Erased adapter bridge ───────────────────────────────────────────────────

/// The typed target-adapter seam keeps checked AOT metadata as strings.
/// Resident/JIT code uses the erased companion trait below after it has
/// deliberately allocated compile-time string handles.
pub trait JetHardwareHost {
    fn setup(
        &mut self,
        profile_id: &str,
        setup_kind: &str,
        item: &str,
        width_or_vector: i64,
        ownership_or_handler: &str,
    ) -> i64;

    fn register_read(
        &mut self,
        profile_id: &str,
        block: &str,
        register: &str,
        width: i64,
    ) -> i64;

    fn register_write(
        &mut self,
        profile_id: &str,
        block: &str,
        register: &str,
        width: i64,
        value: i64,
    ) -> i64;

    fn dma_start(
        &mut self,
        profile_id: &str,
        channel: &str,
        address: u64,
        bytes: u64,
    ) -> i64;

    fn dma_wait(&mut self, profile_id: &str, channel: &str, token: i64) -> i64;

    fn trigger_interrupt(&mut self, _vector: u16) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }
}

/// Erased JIT adapter seam. String handles are allocated by the resident
/// compiler and resolved by its host; this trait is never used by AOT.
pub trait JetHardwareErasedHost {
    fn setup(
        &mut self,
        profile_id: i64,
        setup_kind: i64,
        item: i64,
        width_or_vector: i64,
        ownership_or_handler: i64,
    ) -> i64;

    fn register_read(&mut self, profile_id: i64, block: i64, register: i64, width: i64) -> i64;

    fn register_write(
        &mut self,
        profile_id: i64,
        block: i64,
        register: i64,
        width: i64,
        value: i64,
    ) -> i64;

    fn dma_start(
        &mut self,
        profile_id: i64,
        channel: i64,
        address: i64,
        bytes: i64,
    ) -> i64;

    fn dma_wait(&mut self, profile_id: i64, channel: i64, token: i64) -> i64;

    /// Register a compile-time string handle before the first call. This is
    /// an adapter-only side channel; it never crosses the generated ABI.
    fn register_string_handle(&mut self, _handle: i64, _value: &str) {}

    /// Queue a canonical replay event for a bound hardware interrupt. This is
    /// adapter-only and never part of generated program ABI.
    fn trigger_interrupt(&mut self, _vector: u16) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }
}

/// Setup kind values passed by MIR package setup installation.
pub const JET_HARDWARE_SETUP_DMA_CONFIGURE: i64 = 0;
pub const JET_HARDWARE_SETUP_INTERRUPT_BIND: i64 = 1;
/// Canonical setup tags shared by typed, erased, and evaluator adapters.
pub const JET_HARDWARE_SETUP_DMA_CONFIGURE_NAME: &str = "dma-configure";
pub const JET_HARDWARE_SETUP_INTERRUPT_BIND_NAME: &str = "interrupt-bind";
/// Canonical DMA ownership tags shared by typed and erased adapters.
pub const JET_HARDWARE_DMA_BORROWED_NAME: &str = "borrowed";
pub const JET_HARDWARE_DMA_TRANSFER_NAME: &str = "transfer";

/// Stable width tags passed through the erased ABI.
pub const JET_HARDWARE_WIDTH_U8: i64 = 1;
pub const JET_HARDWARE_WIDTH_U16: i64 = 2;
pub const JET_HARDWARE_WIDTH_U32: i64 = 4;
pub const JET_HARDWARE_WIDTH_U64: i64 = 8;

/// The bridge's negative status. A selected adapter may publish a richer
/// runtime stop through its own carrier, but never returns an invented value.
pub const JET_HARDWARE_UNAVAILABLE: i64 = -1;

#[derive(Clone, Copy)]
struct JetReplayDmaConfig {
    channel: u16,
    width: JetRegisterWidth,
    ownership: JetDmaOwnership,
}

#[derive(Clone, Copy)]
struct JetReplayTransfer {
    handle: i64,
    channel: u16,
    address: u64,
    bytes: u64,
}

/// Deterministic replay policy shared by typed AOT, interpreter, and resident
/// adapters. The fact source is checked target metadata; this host owns only
/// fixed-capacity storage and state transitions.
pub struct JetHardwareReplayHost<F>
where
    F: JetHardwareFactSource,
{
    facts: F,
    registers: JetRegisterReplay<128>,
    configured_dma: [Option<JetReplayDmaConfig>; 64],
    transfers: [Option<JetReplayTransfer>; 128],
    interrupt_bindings: [Option<u16>; 64],
    next_transfer: i64,
}

impl<F> JetHardwareReplayHost<F>
where
    F: JetHardwareFactSource,
{
    pub fn new(facts: F) -> Self {
        Self {
            facts,
            registers: JetRegisterReplay::new(),
            configured_dma: [None; 64],
            transfers: [None; 128],
            interrupt_bindings: [None; 64],
            next_transfer: 1,
        }
    }

    fn profile_matches(&self, profile_id: &str) -> bool {
        self.facts
            .profile_id()
            .is_none_or(|expected| expected == profile_id)
    }

    fn width_tag(width: JetRegisterWidth) -> i64 {
        width.bytes() as i64
    }

    fn width_from_tag(tag: i64) -> Option<JetRegisterWidth> {
        match tag {
            JET_HARDWARE_WIDTH_U8 => Some(JetRegisterWidth::U8),
            JET_HARDWARE_WIDTH_U16 => Some(JetRegisterWidth::U16),
            JET_HARDWARE_WIDTH_U32 => Some(JetRegisterWidth::U32),
            JET_HARDWARE_WIDTH_U64 => Some(JetRegisterWidth::U64),
            _ => None,
        }
    }
    fn ownership_tag(ownership: JetDmaOwnership) -> &'static str {
        match ownership {
            JetDmaOwnership::Borrowed => JET_HARDWARE_DMA_BORROWED_NAME,
            JetDmaOwnership::Transfer => JET_HARDWARE_DMA_TRANSFER_NAME,
        }
    }

    fn ownership_from_tag(tag: &str) -> Option<JetDmaOwnership> {
        match tag {
            JET_HARDWARE_DMA_BORROWED_NAME => Some(JetDmaOwnership::Borrowed),
            JET_HARDWARE_DMA_TRANSFER_NAME => Some(JetDmaOwnership::Transfer),
            _ => None,
        }
    }

    fn ensure_register(
        registers: &mut JetRegisterReplay<128>,
        fact: &JetHardwareRegisterFact<'_>,
    ) -> Option<u64> {
        let address = fact.base.checked_add(fact.offset)?;
        if registers.value(address, fact.width).is_none() {
            registers.define(address, fact.width, 0).ok()?;
        }
        Some(address)
    }

    fn dma_configured(
        &self,
        channel: u16,
        width: JetRegisterWidth,
        ownership: JetDmaOwnership,
    ) -> bool {
        self.configured_dma.iter().flatten().any(|config| {
            config.channel == channel
                && config.width == width
                && config.ownership == ownership
        })
    }

    fn interrupt_bound(&self, vector: u16) -> bool {
        self.interrupt_bindings
            .iter()
            .flatten()
            .any(|bound| *bound == vector)
    }
}

impl<F> JetHardwareHost for JetHardwareReplayHost<F>
where
    F: JetHardwareFactSource,
{
    fn setup(
        &mut self,
        profile_id: &str,
        setup_kind: &str,
        item: &str,
        width_or_vector: i64,
        ownership_or_handler: &str,
    ) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        match setup_kind {
            JET_HARDWARE_SETUP_DMA_CONFIGURE_NAME => {
                let Some(width) = Self::width_from_tag(width_or_vector) else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                let Some(ownership) = Self::ownership_from_tag(ownership_or_handler) else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                let Some(fact) = self.facts.dma_fact(item) else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                if Self::width_tag(fact.transfer_width) != Self::width_tag(width)
                    || Self::ownership_tag(fact.ownership) != Self::ownership_tag(ownership)
                {
                    return JET_HARDWARE_UNAVAILABLE;
                }
                if self.dma_configured(fact.channel, width, ownership) {
                    return 0;
                }
                let Some(slot) = self.configured_dma.iter_mut().find(|slot| slot.is_none())
                else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                *slot = Some(JetReplayDmaConfig {
                    channel: fact.channel,
                    width,
                    ownership,
                });
                0
            }
            JET_HARDWARE_SETUP_INTERRUPT_BIND_NAME => {
                let Ok(vector) = u16::try_from(width_or_vector) else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                let Some(fact) = self.facts.interrupt_fact(item) else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                if !fact.bounded
                    || fact.vector != vector
                    || ownership_or_handler.is_empty()
                {
                    return JET_HARDWARE_UNAVAILABLE;
                }
                if self.interrupt_bound(vector) {
                    return 0;
                }
                let Some(slot) = self
                    .interrupt_bindings
                    .iter_mut()
                    .find(|slot| slot.is_none())
                else {
                    return JET_HARDWARE_UNAVAILABLE;
                };
                *slot = Some(vector);
                0
            }
            _ => JET_HARDWARE_UNAVAILABLE,
        }
    }

    fn register_read(
        &mut self,
        profile_id: &str,
        block: &str,
        register: &str,
        width: i64,
    ) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let Some(fact) = self.facts.register_fact(block, register) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(expected_width) = Self::width_from_tag(width) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        if fact.width != expected_width || !fact.access.can_read() {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let Some(_) = Self::ensure_register(&mut self.registers, &fact) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let spec = JetRegisterSpec::new(
            fact.register,
            fact.offset,
            fact.width,
            fact.access,
            fact.volatile,
        );
        jet_replay_register_read(&self.registers, fact.base, &spec)
            .map(|value| value as i64)
            .unwrap_or(JET_HARDWARE_UNAVAILABLE)
    }

    fn register_write(
        &mut self,
        profile_id: &str,
        block: &str,
        register: &str,
        width: i64,
        value: i64,
    ) -> i64 {
        if !self.profile_matches(profile_id) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let Some(fact) = self.facts.register_fact(block, register) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(expected_width) = Self::width_from_tag(width) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        if fact.width != expected_width || !fact.access.can_write() {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let Some(_) = Self::ensure_register(&mut self.registers, &fact) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let spec = JetRegisterSpec::new(
            fact.register,
            fact.offset,
            fact.width,
            fact.access,
            fact.volatile,
        );
        if jet_replay_register_write(&self.registers, fact.base, &spec, value as u64).is_some() {
            0
        } else {
            JET_HARDWARE_UNAVAILABLE
        }
    }
    fn dma_start(
        &mut self,
        profile_id: &str,
        channel: &str,
        address: u64,
        bytes: u64,
    ) -> i64 {
        if !self.profile_matches(profile_id) || address == 0 || bytes == 0 {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let Some(fact) = self.facts.dma_fact(channel) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let width = Self::width_tag(fact.transfer_width) as u64;
        if fact.ownership != JetDmaOwnership::Transfer
            || address % width != 0
            || bytes % width != 0
            || fact
                .max_transfer_bytes
                .is_some_and(|limit| bytes > limit)
            || !self.dma_configured(fact.channel, fact.transfer_width, fact.ownership)
        {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let handle = self.next_transfer;
        self.next_transfer = self.next_transfer.saturating_add(1).max(1);
        let Some(slot) = self.transfers.iter_mut().find(|slot| slot.is_none()) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        *slot = Some(JetReplayTransfer {
            handle,
            channel: fact.channel,
            address,
            bytes,
        });
        handle
    }

    fn dma_wait(&mut self, profile_id: &str, channel: &str, token: i64) -> i64 {
        if !self.profile_matches(profile_id) || token <= 0 {
            return JET_HARDWARE_UNAVAILABLE;
        }
        let Some(fact) = self.facts.dma_fact(channel) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(slot) = self.transfers.iter_mut().find(|slot| {
            slot.is_some_and(|entry| entry.handle == token && entry.channel == fact.channel)
        }) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        let Some(entry) = *slot else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        *slot = None;
        let _ = (entry.address, entry.bytes);
        0
    }

    fn trigger_interrupt(&mut self, vector: u16) -> i64 {
        let Some(fact) = self.facts.interrupt_vector_fact(vector) else {
            return JET_HARDWARE_UNAVAILABLE;
        };
        if !fact.bounded || !self.interrupt_bound(vector) {
            return JET_HARDWARE_UNAVAILABLE;
        }
        jet_hardware_enqueue_pending(vector);
        0
    }
}

/// One process-wide, FIFO hardware interrupt queue shared by every tier.
static JET_HARDWARE_PENDING_INTERRUPTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::VecDeque<u16>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::VecDeque::new()));
static JET_HARDWARE_PENDING_READY: AtomicBool = AtomicBool::new(false);

pub fn jet_hardware_has_pending() -> bool {
    JET_HARDWARE_PENDING_READY.load(Ordering::Acquire)
}

pub fn jet_hardware_enqueue_pending(vector: u16) {
    let mut queue = JET_HARDWARE_PENDING_INTERRUPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    queue.push_back(vector);
    JET_HARDWARE_PENDING_READY.store(true, Ordering::Release);
}

pub fn jet_hardware_drain_pending() -> Vec<u16> {
    if !jet_hardware_has_pending() {
        return Vec::new();
    }
    let mut queue = JET_HARDWARE_PENDING_INTERRUPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let drained = queue.drain(..).collect();
    JET_HARDWARE_PENDING_READY.store(false, Ordering::Release);
    drained
}

pub fn jet_hardware_clear_pending() {
    let mut queue = JET_HARDWARE_PENDING_INTERRUPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    queue.clear();
    JET_HARDWARE_PENDING_READY.store(false, Ordering::Release);
}

/// Checked callback identity retained by the resident hardware registry.
/// Handler signatures are validated before a pointer enters this carrier.
#[derive(Clone, Copy)]
pub struct JetHardwareInterruptHandler {
    pub fn_ptr: i64,
    pub env: i64,
    pub has_env: bool,
}

#[derive(Clone, Default)]
pub struct JetHardwareInterruptRegistry {
    handlers: std::collections::BTreeMap<u16, JetHardwareInterruptHandler>,
}

impl JetHardwareInterruptRegistry {
    pub fn clear(&mut self) {
        self.handlers.clear();
    }

    pub fn register(
        &mut self,
        vector: u16,
        handler: JetHardwareInterruptHandler,
    ) -> bool {
        self.handlers.insert(vector, handler).is_none()
    }

    fn handler(&self, vector: u16) -> Option<JetHardwareInterruptHandler> {
        self.handlers.get(&vector).copied()
    }
}

/// Drain the Foundation queue in FIFO order and resolve vectors to checked
/// callback pointers. Callers must release runtime borrows before invoking the
/// returned pointers through `jet_hardware_invoke_pending`.
pub fn jet_hardware_dispatch_pending(
    registry: &JetHardwareInterruptRegistry,
) -> Vec<JetHardwareInterruptHandler> {
    if !jet_hardware_has_pending() {
        return Vec::new();
    }
    let mut queue = JET_HARDWARE_PENDING_INTERRUPTS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut callbacks = Vec::new();
    let mut undelivered = std::collections::VecDeque::new();
    while let Some(vector) = queue.pop_front() {
        if let Some(handler) = registry.handler(vector) {
            callbacks.push(handler);
        } else {
            undelivered.push_back(vector);
        }
    }
    *queue = undelivered;
    JET_HARDWARE_PENDING_READY.store(!queue.is_empty(), Ordering::Release);
    callbacks
}

/// Invoke pointers detached by `jet_hardware_dispatch_pending`. This split is
/// required because a handler may re-enter any runtime host operation.
pub fn jet_hardware_invoke_pending(
    callbacks: impl IntoIterator<Item = JetHardwareInterruptHandler>,
) -> usize {
    let callbacks = callbacks.into_iter().collect::<Vec<_>>();
    let delivered = callbacks.len();
    for callback in callbacks {
        // SAFETY: the resident compiler inserts only a validated zero-argument
        // handler pointer (or an explicitly validated captured callback).
        unsafe {
            if callback.has_env {
                let invoke: extern "C" fn(i64) =
                    std::mem::transmute(callback.fn_ptr as usize);
                invoke(callback.env);
            } else {
                let invoke: extern "C" fn() =
                    std::mem::transmute(callback.fn_ptr as usize);
                invoke();
            }
        }
    }
    delivered
}
pub struct JetHardwareUnavailableHost;

impl JetHardwareHost for JetHardwareUnavailableHost {
    fn setup(
        &mut self,
        _profile_id: &str,
        _setup_kind: &str,
        _item: &str,
        _width_or_vector: i64,
        _ownership_or_handler: &str,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn register_read(
        &mut self,
        _profile_id: &str,
        _block: &str,
        _register: &str,
        _width: i64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn register_write(
        &mut self,
        _profile_id: &str,
        _block: &str,
        _register: &str,
        _width: i64,
        _value: i64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn dma_start(
        &mut self,
        _profile_id: &str,
        _channel: &str,
        _address: u64,
        _bytes: u64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn dma_wait(&mut self, _profile_id: &str, _channel: &str, _token: i64) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }
}

impl JetHardwareErasedHost for JetHardwareUnavailableHost {
    fn setup(
        &mut self,
        _profile_id: i64,
        _setup_kind: i64,
        _item: i64,
        _width_or_vector: i64,
        _ownership_or_handler: i64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn register_read(
        &mut self,
        _profile_id: i64,
        _block: i64,
        _register: i64,
        _width: i64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn register_write(
        &mut self,
        _profile_id: i64,
        _block: i64,
        _register: i64,
        _width: i64,
        _value: i64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn dma_start(
        &mut self,
        _profile_id: i64,
        _channel: i64,
        _address: i64,
        _bytes: i64,
    ) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }

    fn dma_wait(&mut self, _profile_id: i64, _channel: i64, _token: i64) -> i64 {
        JET_HARDWARE_UNAVAILABLE
    }
}

thread_local! {
    static JET_HARDWARE_CURRENT_HOST:
        std::cell::RefCell<Option<*mut dyn JetHardwareHost>> =
        std::cell::RefCell::new(None);
    static JET_HARDWARE_CURRENT_ERASED_HOST:
        std::cell::RefCell<Option<*mut dyn JetHardwareErasedHost>> =
        std::cell::RefCell::new(None);
    static JET_HARDWARE_DEFAULT_HOST:
        std::cell::RefCell<JetHardwareUnavailableHost> =
        std::cell::RefCell::new(JetHardwareUnavailableHost);
    static JET_HARDWARE_CURRENT_FACTS:
        std::cell::RefCell<Option<&'static JetHardwareFacts<'static>>> =
        std::cell::RefCell::new(None);
}

/// Guard restoring the previous ambient typed hardware adapter on drop.
struct JetHardwareHostScope {
    previous: Option<*mut dyn JetHardwareHost>,
}

impl Drop for JetHardwareHostScope {
    fn drop(&mut self) {
        JET_HARDWARE_CURRENT_HOST.with(|slot| {
            slot.replace(self.previous);
        });
    }
}

/// Guard restoring the previous ambient erased hardware adapter on drop.
struct JetHardwareErasedHostScope {
    previous: Option<*mut dyn JetHardwareErasedHost>,
}

impl Drop for JetHardwareErasedHostScope {
    fn drop(&mut self) {
        JET_HARDWARE_CURRENT_ERASED_HOST.with(|slot| {
            slot.replace(self.previous);
        });
    }
}
/// Guard restoring the previous checked static hardware facts.
pub struct JetHardwareFactsScope {
    previous: Option<&'static JetHardwareFacts<'static>>,
}

impl Drop for JetHardwareFactsScope {
    fn drop(&mut self) {
        JET_HARDWARE_CURRENT_FACTS.with(|slot| {
            slot.replace(self.previous);
        });
    }
}

/// Install the immutable checked facts used by typed AOT DMA wrappers.
pub fn jet_hardware_facts_scope(
    facts: &'static JetHardwareFacts<'static>,
) -> JetHardwareFactsScope {
    let previous = JET_HARDWARE_CURRENT_FACTS.with(|slot| slot.replace(Some(facts)));
    JetHardwareFactsScope { previous }
}

pub fn jet_hardware_current_facts() -> Option<&'static JetHardwareFacts<'static>> {
    JET_HARDWARE_CURRENT_FACTS.with(|slot| *slot.borrow())
}

pub fn jet_hardware_with_host<R>(
    host: &mut dyn JetHardwareHost,
    body: impl FnOnce() -> R,
) -> R {
    // SAFETY: this private guard restores TLS before the borrowed host can
    // expire, including during unwinding. Callers cannot detach the guard.
    let pointer = unsafe {
        std::mem::transmute::<*mut (dyn JetHardwareHost + '_), *mut (dyn JetHardwareHost + 'static)>(host)
    };
    let previous = JET_HARDWARE_CURRENT_HOST.with(|slot| slot.replace(Some(pointer)));
    let _scope = JetHardwareHostScope { previous };
    body()
}

pub fn jet_hardware_with_erased_host<R>(
    host: &mut dyn JetHardwareErasedHost,
    body: impl FnOnce() -> R,
) -> R {
    // SAFETY: the borrow spans body(), and the private guard restores TLS
    // before that borrow ends, including during unwinding.
    let pointer = unsafe {
        std::mem::transmute::<*mut (dyn JetHardwareErasedHost + '_), *mut (dyn JetHardwareErasedHost + 'static)>(host)
    };
    let previous = JET_HARDWARE_CURRENT_ERASED_HOST.with(|slot| slot.replace(Some(pointer)));
    let _scope = JetHardwareErasedHostScope { previous };
    body()
}

pub fn jet_hardware_with_current_host<R>(
    body: impl FnOnce(&mut dyn JetHardwareHost) -> R,
) -> R {
    JET_HARDWARE_CURRENT_HOST.with(|slot| {
        let current = slot.borrow_mut();
        if let Some(pointer) = *current {
            // SAFETY: the enclosing private scope keeps the host alive.
            // Holding the TLS borrow prevents a reentrant mutable alias.
            unsafe { body(&mut *pointer) }
        } else {
            JET_HARDWARE_DEFAULT_HOST.with(|host| body(&mut *host.borrow_mut()))
        }
    })
}

pub fn jet_hardware_with_current_erased_host<R>(
    body: impl FnOnce(&mut dyn JetHardwareErasedHost) -> R,
) -> R {
    JET_HARDWARE_CURRENT_ERASED_HOST.with(|slot| {
        let current = slot.borrow_mut();
        if let Some(pointer) = *current {
            // SAFETY: the enclosing private scope keeps the host alive.
            // Holding the TLS borrow prevents a reentrant mutable alias.
            unsafe { body(&mut *pointer) }
        } else {
            JET_HARDWARE_DEFAULT_HOST.with(|host| body(&mut *host.borrow_mut()))
        }
    })
}

/// Typed AOT/native wrappers. No metadata handle is invented at this layer.
pub fn jet_hardware_setup_typed(
    profile_id: &str,
    setup_kind: &str,
    item: &str,
    width_or_vector: i64,
    ownership_or_handler: &str,
) -> i64 {
    jet_hardware_with_current_host(|host| {
        host.setup(
            profile_id,
            setup_kind,
            item,
            width_or_vector,
            ownership_or_handler,
        )
    })
}

pub fn jet_hardware_register_read_typed(
    profile_id: &str,
    block: &str,
    register: &str,
    width: i64,
) -> i64 {
    jet_hardware_with_current_host(|host| host.register_read(profile_id, block, register, width))
}

pub fn jet_hardware_register_write_typed(
    profile_id: &str,
    block: &str,
    register: &str,
    width: i64,
    value: i64,
) -> i64 {
    jet_hardware_with_current_host(|host| {
        host.register_write(profile_id, block, register, width, value)
    })
}

pub fn jet_hardware_dma_start_typed<T: JetDmaPayload>(
    profile_id: &str,
    channel: &str,
    buffer: T,
) -> JetDmaTransfer<'static, T> {
    let Some(facts) = jet_hardware_current_facts() else {
        jet_panic(
            "<core.hardware>",
            0,
            "DMA start requires checked static target hardware facts",
        );
    };
    if facts
        .profile_id
        .is_some_and(|expected| expected != profile_id)
    {
        jet_panic(
            "<core.hardware>",
            0,
            "DMA start profile does not match checked target hardware facts",
        );
    }
    let address = buffer.dma_address();
    let bytes = buffer.dma_bytes();
    let mut runtime = JetDmaRuntime::new(facts);
    let mut transfer = runtime
        .start(channel, JetDmaBuffer::new(buffer, address, bytes))
        .unwrap_or_else(|_| {
            jet_panic(
                "<core.hardware>",
                0,
                "DMA start payload violates checked channel facts",
            )
        });
    let host_token =
        jet_hardware_with_current_host(|host| host.dma_start(profile_id, channel, address, bytes));
    if host_token <= 0 {
        jet_panic(
            "<core.hardware>",
            0,
            "DMA start adapter reported unavailable status",
        );
    }
    transfer.set_host_token(host_token);
    transfer
}

pub fn jet_hardware_dma_wait_typed<T: JetDmaPayload>(
    profile_id: &str,
    channel: &str,
    mut transfer: JetDmaTransfer<'static, T>,
) -> T {
    if transfer.channel() != channel {
        jet_panic(
            "<core.hardware>",
            0,
            "DMA wait channel does not match checked transfer",
        );
    }
    let token = transfer.host_token();
    if token <= 0 {
        jet_panic(
            "<core.hardware>",
            0,
            "DMA wait transfer has no host token",
        );
    }
    let status =
        jet_hardware_with_current_host(|host| host.dma_wait(profile_id, channel, token));
    if status != 0 {
        jet_panic(
            "<core.hardware>",
            0,
            "DMA wait adapter reported unavailable status",
        );
    }
    transfer.mark_complete();
    transfer
        .wait()
        .unwrap_or_else(|_| {
            jet_panic(
                "<core.hardware>",
                0,
                "DMA wait completed without an owned buffer",
            )
        })
        .into_inner()
}

/// Erased setup installation entry used by JIT wrappers.
pub fn jet_hardware_setup(
    profile_id: i64,
    setup_kind: i64,
    item: i64,
    width_or_vector: i64,
    ownership_or_handler: i64,
) -> i64 {
    jet_hardware_with_current_erased_host(|host| {
        host.setup(
            profile_id,
            setup_kind,
            item,
            width_or_vector,
            ownership_or_handler,
        )
    })
}

pub fn jet_hardware_register_read(profile_id: i64, block: i64, register: i64, width: i64) -> i64 {
    jet_hardware_with_current_erased_host(|host| host.register_read(profile_id, block, register, width))
}

pub fn jet_hardware_register_write(
    profile_id: i64,
    block: i64,
    register: i64,
    width: i64,
    value: i64,
) -> i64 {
    jet_hardware_with_current_erased_host(|host| {
        host.register_write(profile_id, block, register, width, value)
    })
}

pub fn jet_hardware_dma_start(
    profile_id: i64,
    channel: i64,
    address: i64,
    bytes: i64,
) -> i64 {
    jet_hardware_with_current_erased_host(|host| {
        host.dma_start(profile_id, channel, address, bytes)
    })
}

pub fn jet_hardware_dma_wait(profile_id: i64, channel: i64, transfer: i64) -> i64 {
    jet_hardware_with_current_erased_host(|host| host.dma_wait(profile_id, channel, transfer))
}



/// Fixed-capacity host replay storage.  It uses the same `JetRegister` type as
/// target MMIO, so replay cannot bypass width/access/volatile checks.
pub struct JetRegisterReplay<const N: usize> {
    cells: [JetReplayCell; N],
}

struct JetReplayCell {
    defined: bool,
    address: u64,
    width: JetRegisterWidth,
    value: Cell<u64>,
}

impl JetReplayCell {
    fn empty() -> Self {
        Self {
            defined: false,
            address: 0,
            width: JetRegisterWidth::U8,
            value: Cell::new(0),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetRegisterReplayError {
    Full,
    Duplicate,
}

impl<const N: usize> JetRegisterReplay<N> {
    pub fn new() -> Self {
        Self {
            cells: core::array::from_fn(|_| JetReplayCell::empty()),
        }
    }

    pub fn define(
        &mut self,
        address: u64,
        width: JetRegisterWidth,
        initial: u64,
    ) -> Result<(), JetRegisterReplayError> {
        if self
            .cells
            .iter()
            .any(|cell| cell.defined && cell.address == address)
        {
            return Err(JetRegisterReplayError::Duplicate);
        }
        let Some(cell) = self.cells.iter_mut().find(|cell| !cell.defined) else {
            return Err(JetRegisterReplayError::Full);
        };
        cell.defined = true;
        cell.address = address;
        cell.width = width;
        cell.value.set(initial);
        Ok(())
    }

    pub fn define_spec(
        &mut self,
        base: u64,
        spec: &JetRegisterSpec<'_>,
        initial: u64,
    ) -> Result<(), JetRegisterReplayError> {
        let address = base.saturating_add(spec.offset);
        self.define(address, spec.width, initial)
    }

    pub fn value(&self, address: u64, width: JetRegisterWidth) -> Option<u64> {
        self.cells
            .iter()
            .find(|cell| cell.defined && cell.address == address && cell.width == width)
            .map(|cell| cell.value.get())
    }
}

impl<const N: usize> Default for JetRegisterReplay<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> JetRegisterBackend for JetRegisterReplay<N> {
    fn read(&self, address: u64, width: JetRegisterWidth, _volatile_access: bool) -> u64 {
        self.value(address, width).unwrap_or(0)
    }

    fn write(&self, address: u64, width: JetRegisterWidth, value: u64, _volatile_access: bool) {
        if let Some(cell) = self
            .cells
            .iter()
            .find(|cell| cell.defined && cell.address == address && cell.width == width)
        {
            cell.value.set(value);
        }
    }
}
/// Replay uses the same width/access/volatile type witness as target MMIO.
/// The only difference is the backend storage, so host execution cannot
/// silently widen or bypass a checked register fact.
fn jet_replay_register_read_typed<W, A, const VOLATILE: bool>(
    backend: &JetRegisterReplay<128>,
    base: u64,
    spec: &JetRegisterSpec<'_>,
) -> Option<u64>
where
    W: JetRegisterValue,
    A: JetRegisterReadable,
{
    JetRegister::<W, A, JetRegisterReplay<128>, VOLATILE>::from_spec(backend, base, spec)
        .ok()
        .map(|register| register.read().into_bits())
}

fn jet_replay_register_read(
    backend: &JetRegisterReplay<128>,
    base: u64,
    spec: &JetRegisterSpec<'_>,
) -> Option<u64> {
    match (spec.width, spec.access) {
        (JetRegisterWidth::U8, JetRegisterAccessMode::ReadOnly) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u8, JetReadOnly, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u8, JetReadOnly, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U8, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u8, JetReadWrite, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u8, JetReadWrite, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U16, JetRegisterAccessMode::ReadOnly) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u16, JetReadOnly, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u16, JetReadOnly, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U16, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u16, JetReadWrite, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u16, JetReadWrite, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U32, JetRegisterAccessMode::ReadOnly) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u32, JetReadOnly, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u32, JetReadOnly, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U32, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u32, JetReadWrite, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u32, JetReadWrite, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U64, JetRegisterAccessMode::ReadOnly) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u64, JetReadOnly, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u64, JetReadOnly, false>(backend, base, spec)
            }
        }
        (JetRegisterWidth::U64, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_read_typed::<u64, JetReadWrite, true>(backend, base, spec)
            } else {
                jet_replay_register_read_typed::<u64, JetReadWrite, false>(backend, base, spec)
            }
        }
        _ => None,
    }
}

fn jet_replay_register_write_typed<W, A, const VOLATILE: bool>(
    backend: &JetRegisterReplay<128>,
    base: u64,
    spec: &JetRegisterSpec<'_>,
    value: u64,
) -> Option<()>
where
    W: JetRegisterValue,
    A: JetRegisterWritable,
{
    let register =
        JetRegister::<W, A, JetRegisterReplay<128>, VOLATILE>::from_spec(backend, base, spec)
            .ok()?;
    register.write(W::from_bits(value));
    Some(())
}

fn jet_replay_register_write(
    backend: &JetRegisterReplay<128>,
    base: u64,
    spec: &JetRegisterSpec<'_>,
    value: u64,
) -> Option<()> {
    match (spec.width, spec.access) {
        (JetRegisterWidth::U8, JetRegisterAccessMode::WriteOnly) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u8, JetWriteOnly, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u8, JetWriteOnly, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U8, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u8, JetReadWrite, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u8, JetReadWrite, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U16, JetRegisterAccessMode::WriteOnly) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u16, JetWriteOnly, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u16, JetWriteOnly, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U16, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u16, JetReadWrite, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u16, JetReadWrite, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U32, JetRegisterAccessMode::WriteOnly) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u32, JetWriteOnly, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u32, JetWriteOnly, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U32, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u32, JetReadWrite, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u32, JetReadWrite, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U64, JetRegisterAccessMode::WriteOnly) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u64, JetWriteOnly, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u64, JetWriteOnly, false>(
                    backend, base, spec, value,
                )
            }
        }
        (JetRegisterWidth::U64, JetRegisterAccessMode::ReadWrite) => {
            if spec.volatile {
                jet_replay_register_write_typed::<u64, JetReadWrite, true>(
                    backend, base, spec, value,
                )
            } else {
                jet_replay_register_write_typed::<u64, JetReadWrite, false>(
                    backend, base, spec, value,
                )
            }
        }
        _ => None,
    }
}

// ── Bounded interrupts ─────────────────────────────────────────────────────

/// A callback is a function pointer, not a boxed closure.  The context exposes
/// only the handler's shared cells; it contains no allocation or wait API.
pub enum JetInterruptHandler<C> {
    Context(fn(&mut C)),
    Bare(fn()),
}

impl<C> Copy for JetInterruptHandler<C> {}
impl<C> Clone for JetInterruptHandler<C> {
    fn clone(&self) -> Self {
        *self
    }
}

struct JetInterruptBinding<C> {
    vector: u16,
    handler: JetInterruptHandler<C>,
}

impl<C> Copy for JetInterruptBinding<C> {}
impl<C> Clone for JetInterruptBinding<C> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetInterruptError {
    UnknownVector,
    UnboundedVector,
    DuplicateBinding,
    BindingTableFull,
    UnboundVector,
    QueueFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetInterruptDispatch {
    Empty,
    Dispatched(u16),
}

/// A fixed binding table and FIFO vector queue.  Registration and dispatch do
/// not allocate, block, or invoke a handler with access to this controller.
pub struct JetInterruptController<'a, C, const BINDINGS: usize, const PENDING: usize> {
    facts: &'a JetHardwareFacts<'a>,
    bindings: [Option<JetInterruptBinding<C>>; BINDINGS],
    pending: [u16; PENDING],
    head: usize,
    tail: usize,
    pending_len: usize,
    dropped: usize,
}

impl<'a, C, const BINDINGS: usize, const PENDING: usize>
    JetInterruptController<'a, C, BINDINGS, PENDING>
{
    pub fn new(facts: &'a JetHardwareFacts<'a>) -> Self {
        Self {
            facts,
            bindings: [None; BINDINGS],
            pending: [0; PENDING],
            head: 0,
            tail: 0,
            pending_len: 0,
            dropped: 0,
        }
    }

    pub fn bind(
        &mut self,
        vector: u16,
        handler: JetInterruptHandler<C>,
    ) -> Result<(), JetInterruptError> {
        let Some(spec) = self.facts.interrupt_vector(vector) else {
            return Err(JetInterruptError::UnknownVector);
        };
        if !spec.bounded {
            return Err(JetInterruptError::UnboundedVector);
        }
        if self.binding_index(vector).is_some() {
            return Err(JetInterruptError::DuplicateBinding);
        }
        let Some(slot) = self.bindings.iter_mut().find(|slot| slot.is_none()) else {
            return Err(JetInterruptError::BindingTableFull);
        };
        *slot = Some(JetInterruptBinding { vector, handler });
        Ok(())
    }

    pub fn raise(&mut self, vector: u16) -> Result<(), JetInterruptError> {
        if self.facts.interrupt_vector(vector).is_none() {
            return Err(JetInterruptError::UnknownVector);
        }
        if self.binding_index(vector).is_none() {
            return Err(JetInterruptError::UnboundVector);
        }
        if self.pending_len == PENDING {
            self.dropped = self.dropped.saturating_add(1);
            return Err(JetInterruptError::QueueFull);
        }
        self.pending[self.tail] = vector;
        self.tail = ring_next(self.tail, PENDING);
        self.pending_len += 1;
        Ok(())
    }

    pub fn dispatch_next(&mut self, context: &mut C) -> JetInterruptDispatch {
        if self.pending_len == 0 {
            return JetInterruptDispatch::Empty;
        }
        let vector = self.pending[self.head];
        self.head = ring_next(self.head, PENDING);
        self.pending_len -= 1;
        if let Some(binding) = self.binding(vector) {
            match binding.handler {
                JetInterruptHandler::Context(handler) => handler(context),
                JetInterruptHandler::Bare(handler) => handler(),
            }
        }
        JetInterruptDispatch::Dispatched(vector)
    }

    pub fn dispatch_all(&mut self, context: &mut C) -> usize {
        let mut dispatched = 0;
        while self.pending_len != 0 {
            let _ = self.dispatch_next(context);
            dispatched += 1;
        }
        dispatched
    }

    pub fn clear_pending(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.pending_len = 0;
    }

    pub const fn pending_len(&self) -> usize {
        self.pending_len
    }

    pub const fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.iter().filter(|slot| slot.is_some()).count()
    }

    fn binding_index(&self, vector: u16) -> Option<usize> {
        self.bindings.iter().position(|slot| {
            slot.as_ref()
                .is_some_and(|binding| binding.vector == vector)
        })
    }

    fn binding(&self, vector: u16) -> Option<JetInterruptBinding<C>> {
        self.binding_index(vector)
            .and_then(|index| self.bindings[index])
    }
}

fn ring_next(index: usize, capacity: usize) -> usize {
    if capacity == 0 || index + 1 == capacity {
        0
    } else {
        index + 1
    }
}

mod jet_dma_payload_sealed {
    pub trait Element {
        const WIDTH: u64;
    }

    macro_rules! fixed_width_element {
        ($($ty:ty => $width:expr),* $(,)?) => {
            $(impl Element for $ty {
                const WIDTH: u64 = $width;
            })*
        };
    }

    fixed_width_element!(
        u8 => 1,
        u16 => 2,
        u32 => 4,
        u64 => 8,
        i8 => 1,
        i16 => 2,
        i32 => 4,
        i64 => 8,
        f32 => 4,
        f64 => 8,
    );

    pub trait Container {}

    impl<T: Element> Container for Vec<T> {}
    impl<T: Element> Container for Box<[T]> {}
    impl<T: Element, const N: usize> Container for [T; N] {}
}

/// A sealed, contiguous owning DMA payload. Only fixed-width integer and
/// floating-point element buffers are admitted; handles, records, strings,
/// booleans, and characters cannot satisfy this trait.
pub trait JetDmaPayload: jet_dma_payload_sealed::Container {
    fn dma_address(&self) -> u64;
    fn dma_bytes(&self) -> u64;
}

impl<T: jet_dma_payload_sealed::Element> JetDmaPayload for Vec<T> {
    fn dma_address(&self) -> u64 {
        self.as_ptr() as usize as u64
    }

    fn dma_bytes(&self) -> u64 {
        (self.len() as u64).saturating_mul(T::WIDTH)
    }
}

impl<T: jet_dma_payload_sealed::Element> JetDmaPayload for Box<[T]> {
    fn dma_address(&self) -> u64 {
        self.as_ptr() as usize as u64
    }

    fn dma_bytes(&self) -> u64 {
        (self.len() as u64).saturating_mul(T::WIDTH)
    }
}

impl<T: jet_dma_payload_sealed::Element, const N: usize> JetDmaPayload for [T; N] {
    fn dma_address(&self) -> u64 {
        self.as_ptr() as usize as u64
    }

    fn dma_bytes(&self) -> u64 {
        (N as u64).saturating_mul(T::WIDTH)
    }
}

// ── DMA ownership transfer ─────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDmaError {
    UnknownChannel,
    BorrowedChannel,
    ZeroLength,
    AddressMisaligned,
    TransferTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDmaTransferState {
    InFlight,
    Complete,
}

/// A CPU-owned buffer.  It is intentionally not `Clone`; moving it into
/// `JetDmaRuntime::start` is the ownership transfer, and no buffer accessor is
/// exposed from `JetDmaTransfer` while the device owns it.
pub struct JetDmaBuffer<T> {
    value: T,
    address: u64,
    bytes: u64,
}

impl<T> JetDmaBuffer<T> {
    pub fn new(value: T, address: u64, bytes: u64) -> Self {
        Self {
            value,
            address,
            bytes,
        }
    }

    pub const fn address(&self) -> u64 {
        self.address
    }

    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn as_ref(&self) -> &T {
        &self.value
    }

    pub fn as_mut(&mut self) -> &mut T {
        &mut self.value
    }

    pub fn into_inner(self) -> T {
        self.value
    }
}

pub struct JetDmaTransfer<'a, T> {
    channel: JetDmaChannelSpec<'a>,
    sequence: u64,
    host_token: i64,
    state: JetDmaTransferState,
    buffer: Option<JetDmaBuffer<T>>,
    completion_handle: Option<
        jet_foundation::ResourceSchedule::JetFrameCompletionHandle,
    >,
}

impl<'a, T> JetDmaTransfer<'a, T> {
    fn new(
        channel: JetDmaChannelSpec<'a>,
        sequence: u64,
        buffer: JetDmaBuffer<T>,
    ) -> Self {
        let completion_handle =
            match jet_foundation::ResourceSchedule::current_frame_completion_handle(
                "jet_dma_transfer",
            ) {
                Ok(handle) => handle,
                Err(error) => jet_panic("<core.hardware>", 0, &error),
            };
        Self {
            channel,
            sequence,
            host_token: JET_HARDWARE_UNAVAILABLE,
            state: JetDmaTransferState::InFlight,
            buffer: Some(buffer),
            completion_handle,
        }
    }

    fn host_token(&self) -> i64 {
        self.host_token
    }

    pub fn set_host_token(&mut self, token: i64) {
        self.host_token = token;
    }
    /// Called by the target completion adapter, or by `start_replay` for the
    /// deterministic host adapter. It performs no wait and cannot expose the
    /// owned buffer before completion.
    /// Mark the provider's real DMA completion event. This is the only point
    /// at which the checked frame may release a retained transfer resource.
    pub fn mark_complete(&mut self) {
        self.state = JetDmaTransferState::Complete;
        if let Some(handle) = &self.completion_handle {
            if let Err(error) = handle.signal() {
                jet_panic("<core.hardware>", 0, &error);
            }
        } else if jet_foundation::ResourceSchedule::current_frame_completion_state().is_some() {
            jet_panic(
                "<core.hardware>",
                0,
                "checked DMA completion is missing its submission binding",
            );
        }
    }

    pub const fn channel(&self) -> &'a str {
        self.channel.name
    }

    pub const fn channel_number(&self) -> u16 {
        self.channel.channel
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn state(&self) -> JetDmaTransferState {
        self.state
    }

    pub const fn is_complete(&self) -> bool {
        matches!(self.state, JetDmaTransferState::Complete)
    }

    /// Return the buffer only after the device has completed the transfer.
    /// An incomplete wait returns the transfer itself so ownership is never
    /// lost and a later completion can retry the same operation.
    pub fn wait(mut self) -> Result<JetDmaBuffer<T>, JetDmaWaitError<'a, T>> {
        if !self.is_complete() {
            return Err(JetDmaWaitError::Incomplete(self));
        }
        match self.buffer.take() {
            Some(buffer) => Ok(buffer),
            None => Err(JetDmaWaitError::Incomplete(self)),
        }
    }
}

pub enum JetDmaWaitError<'a, T> {
    Incomplete(JetDmaTransfer<'a, T>),
}

impl<'a, T> JetDmaWaitError<'a, T> {
    pub fn into_transfer(self) -> JetDmaTransfer<'a, T> {
        match self {
            Self::Incomplete(transfer) => transfer,
        }
    }
}

/// Shared DMA state machine.  The sema projection checks ordered ownership
/// facts; this runtime repeats only the target-independent buffer shape checks
/// needed by target and replay adapters.
pub struct JetDmaRuntime<'a> {
    facts: &'a JetHardwareFacts<'a>,
    next_sequence: u64,
}

impl<'a> JetDmaRuntime<'a> {
    pub fn new(facts: &'a JetHardwareFacts<'a>) -> Self {
        Self {
            facts,
            next_sequence: 0,
        }
    }

    pub fn start<T>(
        &mut self,
        channel: &str,
        buffer: JetDmaBuffer<T>,
    ) -> Result<JetDmaTransfer<'a, T>, JetDmaError> {
        let Some(spec) = self.facts.dma_channel(channel).copied() else {
            return Err(JetDmaError::UnknownChannel);
        };
        if spec.ownership != JetDmaOwnership::Transfer {
            return Err(JetDmaError::BorrowedChannel);
        }
        let width = spec.transfer_width.bytes();
        if buffer.bytes == 0 {
            return Err(JetDmaError::ZeroLength);
        }
        if buffer.address % width != 0 || buffer.bytes % width != 0 {
            return Err(JetDmaError::AddressMisaligned);
        }
        if spec
            .max_transfer_bytes
            .is_some_and(|limit| buffer.bytes > limit)
        {
            return Err(JetDmaError::TransferTooLarge);
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(JetDmaTransfer::new(spec, sequence, buffer))
    }

    /// Host replay completes through the same transfer state machine.  The
    /// only replay policy is that the deterministic adapter reports completion
    /// immediately; ownership still moves CPU → device → CPU through start and
    /// wait, and transfer sequence order remains observable.
    pub fn start_replay<T>(
        &mut self,
        channel: &str,
        buffer: JetDmaBuffer<T>,
    ) -> Result<JetDmaTransfer<'a, T>, JetDmaError> {
        let mut transfer = self.start(channel, buffer)?;
        transfer.mark_complete();
        Ok(transfer)
    }

    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
}

/// Convenience aggregate for host replay and target adapters.  It contains no
/// alternate semantics: both fields are the controllers above, fed by the
/// same immutable `JetHardwareFacts`.
pub struct JetHardwareReplay<'a, C, const BINDINGS: usize, const PENDING: usize> {
    pub interrupts: JetInterruptController<'a, C, BINDINGS, PENDING>,
    pub dma: JetDmaRuntime<'a>,
}

impl<'a, C, const BINDINGS: usize, const PENDING: usize>
    JetHardwareReplay<'a, C, BINDINGS, PENDING>
{
    pub fn new(facts: &'a JetHardwareFacts<'a>) -> Self {
        Self {
            interrupts: JetInterruptController::new(facts),
            dma: JetDmaRuntime::new(facts),
        }
    }
}

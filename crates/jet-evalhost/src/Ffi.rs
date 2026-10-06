//! Resident JIT FFI: load the prepared bridge cdylib and call `*_cabi` trampolines.
//! Same bridge AOT links — no parallel/fake native path.

use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::MIR::{
    MirAbi, MirAccess, MirArtifactId, MirArtifactTarget, MirCEnumTag, MirForeign, MirForeignAbi, MirForeignLanguage, MirHandleId, MirLinkUnitId, MirProgram,
    MirRuntimeValue, MirScalarKind, MirStructLayout, MirType, MirTypeDefKind, MirTypeId,
    MirTypeKind, MirVariantPayload
};
use jet_codegen::Comptime::{AmbientMirExternResult, AmbientMirExternWriteback};
use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{
    Arc, Mutex
};
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

#[cfg(unix)]
const RTLD_NOW: c_int = 2;

thread_local! {
    static BRIDGE_CDYLIB: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Install the prepared bridge cdylib for the next resident bind.
/// Interpreter sets this instead of mutating optimized MIR link artifacts.
pub fn set_bridge_cdylib(path: Option<PathBuf>) {
    BRIDGE_CDYLIB.with(|slot| *slot.borrow_mut() = path);
}

/// The bridge's panic reporter, as a plain Rust `fn`.
///
/// D-JITUNWIND1 (#1995 / #1997): the `extern "C"` frame the bridge actually
/// calls is the shim `host_seam::guarded` generates below, never this body,
/// because rustc gives an `extern "C"` *body* an abort-on-unwind shim. The
/// failed foreign call itself returns a nonzero status that `call_cabi`
/// reports as a host fault; the evaluator has no separate channel for the
/// reporter's message text.
fn ffi_reporter(_message: *const u8, _len: usize) {}

#[repr(C)]
#[derive(Clone, Copy)]
struct FfiSlot {
    value: u64,
    ptr: *mut u8,
    len: usize
}

/// Heap storage kept alive for the duration of one descriptor-driven call.
/// The allocation uses the checked record alignment rather than relying on a
/// `Vec<u64>`'s incidental alignment.  This matters for `CAligned` records
/// whose native callee may issue aligned loads directly.
struct FfiStorage {
    ptr: NonNull<u8>,
    layout: Layout,
    len: usize
}

impl FfiStorage {
    fn allocate(len: usize, align: usize) -> Self {
        let layout = Layout::from_size_align(len.max(1), align.max(1))
            .expect("checked C/CAligned layout has a valid allocation alignment");
        // SAFETY: `layout` has a non-zero size and was constructed from the
        // checked MIR size/alignment pair.
        let ptr = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| handle_alloc_error(layout));
        Self { ptr, layout, len }
    }

    fn from_bytes(bytes: &[u8], align: usize) -> Self {
        let storage = Self::allocate(bytes.len(), align);
        if !bytes.is_empty() {
            // SAFETY: `storage` owns an allocation at least `bytes.len()`
            // bytes long; the copy is byte-wise and bounded by both slices.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), storage.ptr.as_ptr(), bytes.len());
            }
        }
        storage
    }

    fn zeroed(len: usize, align: usize) -> Self {
        Self::allocate(len, align)
    }

    fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the byte view is bounded by the logical byte length.
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len) }
    }

    fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the byte view is bounded by the logical byte length.
        unsafe { std::slice::from_raw_parts_mut(self.as_ptr(), self.len) }
    }
}

impl Drop for FfiStorage {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `layout` came from the matching allocation above
        // and this value owns that allocation exclusively.
        unsafe { dealloc(self.ptr.as_ptr(), self.layout) };
    }
}
type UniformFfi = unsafe extern "C" fn(*const FfiSlot, usize, *mut FfiSlot) -> i32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamAbi {
    Int,
    Float,
    Bool,
    String,
    Handle,
    List,
    /// Checked `#[repr(C)]` nominal records use byte storage and a generated
    /// typed C trampoline; they are never collapsed into an integer carrier.
    Record,
    /// Managed callback start rows are loaded through their typed callback
    /// helper, never through a uniform `*_cabi` call.
    Callback
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RetAbi {
    Unit,
    Int,
    Float,
    Bool,
    String,
    Handle,
    List,
    /// A checked C record returned by value, copied from the trampoline's
    /// owned byte buffer before the bridge frees it.
    Record
}

#[derive(Clone)]
struct FfiRecordField {
    name: String,
    ty: MirType,
    offset: usize
}

#[derive(Clone)]
struct FfiRecordDesc {
    id: MirTypeId,
    name: String,
    size: usize,
    align: usize,
    fields: Vec<FfiRecordField>,
    /// Present for checked C enums. Payload enums use the record carrier;
    /// unit-only enums retain their scalar carrier but still use this
    /// descriptor to validate native discriminants.
    enum_info: Option<FfiEnumDesc>
}

#[derive(Clone)]
struct FfiEnumVariantDesc {
    name: String,
    discriminant: i64,
    payload: MirVariantPayload,
    offset: usize,
    size: usize,
    align: usize,
    stride: usize,
    field_offsets: Vec<usize>
}

#[derive(Clone)]
struct FfiEnumDesc {
    tag: MirCEnumTag,
    tag_size: usize,
    has_payload: bool,
    variants: Vec<FfiEnumVariantDesc>
}

#[derive(Clone)]
struct FfiEntrySpec {
    wrapper_name: String,
    params: Vec<ParamAbi>,
    /// `None` is deliberate for scalar/string/list carriers; this must not
    /// collapse mixed handle rows to the foreign return-handle identity.
    param_handles: Vec<Option<MirHandleId>>,
    param_types: Vec<MirType>,
    param_access: Vec<MirAccess>,
    ret: RetAbi,
    ret_type: Option<MirType>,
    handle: Option<MirHandleId>,
    close_handle: Option<MirHandleId>
}

#[derive(Clone)]
struct FfiEntry {
    param_handles: Vec<Option<MirHandleId>>,
    params: Vec<ParamAbi>,
    param_types: Vec<MirType>,
    param_access: Vec<MirAccess>,
    ret: RetAbi,
    ret_type: Option<MirType>,
    handle: Option<MirHandleId>,
    close_handle: Option<MirHandleId>,
    /// Function pointer into the loaded cdylib.
    ptr: *const ()
}

// Safety: entries are only used while the loaded library is kept alive.
unsafe impl Send for FfiEntry {}
unsafe impl Sync for FfiEntry {}
/// Owner token for the separate provider handle used by Foundation's reader
/// slot.  The wrapper handle below may be replaced independently.
struct FfiProviderLibrary {
    handle: *mut c_void
}

unsafe impl Send for FfiProviderLibrary {}
unsafe impl Sync for FfiProviderLibrary {}

impl Drop for FfiProviderLibrary {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: Foundation drops ProviderLease-owned registration before
            // this owner Arc is released.
            unsafe {
                dlclose(self.handle);
            }
        }
    }
}

struct FfiState {
    handle: *mut c_void,
    clear_panic_hook_fn: unsafe extern "C" fn(),
    free_fn: Option<unsafe extern "C" fn(*mut u8, usize)>,
    take_failure_fn: unsafe extern "C" fn() -> i8,
    artifact_identity: String,
    by_wrapper: HashMap<String, FfiEntry>,
    records: HashMap<MirTypeId, FfiRecordDesc>,
    /// Foundation owns the registration slot; this lease keeps its provider
    /// callback and dynamic owner live until FFI teardown.
    arrow_provider_lease: Option<jet_foundation::ArrowFileReader::ProviderLease>
}

unsafe impl Send for FfiState {}

static FFI_STATE: Mutex<Option<FfiState>> = Mutex::new(None);
/// Why the native bridge could not be bound.
pub(crate) enum BindError {
    Message(String)
}

fn target_applicable(foreign: &MirForeign, target: MirArtifactTarget) -> bool {
    match target {
        MirArtifactTarget::Interpreter => foreign.target_applicability.interpreter,
        MirArtifactTarget::RustAot => foreign.target_applicability.rust_aot,
        MirArtifactTarget::Web => foreign.target_applicability.web
    }
}
pub(crate) fn bridge_wrapper_name(foreign: &MirForeign) -> String {
    if foreign.key.starts_with("jet_ffi_guest") {
        return foreign.key.clone();
    }
    match foreign.foreign_language {
        // C, C++, and assembly bodies are compiled into the prepared bridge
        // cdylib and exported under the `jet_ffi_` trampoline name; only Rust
        // items link by their own symbol.
        MirForeignLanguage::C | MirForeignLanguage::Cpp | MirForeignLanguage::Assembly => {
            format!("jet_ffi_{}", foreign.name)
        }
        MirForeignLanguage::Rust => foreign.symbol.clone()
    }
}

fn bridge_path_for_links(
    program: &MirProgram,
    artifact: &jet_foundation::MIR::MirArtifactPlan,
    selected_links: Option<&std::collections::BTreeSet<MirLinkUnitId>>,
) -> Option<std::path::PathBuf> {
    if let Some(path) = BRIDGE_CDYLIB.with(|slot| slot.borrow().clone()) {
        return Some(path);
    }
    artifact.links.iter().find_map(|link_id| {
        if selected_links.is_some_and(|selected| !selected.contains(link_id)) {
            return None;
        }
        program
            .links
            .iter()
            .find(|link| link.id == *link_id)?
            .artifacts
            .iter()
            .find(|artifact| {
                matches!(
                    artifact.kind,
                    jet_foundation::MIR::MirLinkArtifactKind::DynamicLibrary
                )
            })
            .map(|artifact| std::path::PathBuf::from(&artifact.path))
    })
}

#[cfg(not(unix))]
fn ensure_source_helper_data_provider(
    _state: &mut FfiState,
    _path: Option<&std::path::Path>,
    needs_data_provider: bool,
) -> Result<(), String> {
    if needs_data_provider {
        return Err("jit ffi: cdylib load unsupported on this host".into());
    }
    Ok(())
}

fn bridge_path(
    program: &MirProgram,
    artifact: &jet_foundation::MIR::MirArtifactPlan,
) -> Option<std::path::PathBuf> {
    bridge_path_for_links(program, artifact, None)
}

/// Resolve a nominal MIR type against the checked lifecycle rows. Never infer
/// opacity from the nominal shape alone: unclassified records retain their
/// typed CModule route.
fn mir_handle_id_for_type(program: &MirProgram, ty: &MirType) -> Option<MirHandleId> {
    program.handles.iter().find_map(|handle| {
        (handle.ty.same_checked_type(ty)
            || matches!(
                (&handle.ty.kind(), &ty.kind()),
                (MirTypeKind::Apply { name: left, .. }, MirTypeKind::Apply { name: right, .. })
                    if left.id == right.id
            ))
        .then_some(handle.id)
    })
}

fn mir_foreign_handle_type(program: &MirProgram, ty: &MirType) -> bool {
    mir_handle_id_for_type(program, ty).is_some()
}
fn mir_type_identity(ty: &MirType) -> Option<MirTypeId> {
    ty.identity.or_else(|| match ty.kind() {
        MirTypeKind::Apply { name, .. } => Some(name.id),
        _ => None
    })
}

fn mir_enum_desc<'a>(
    ty: &MirType,
    records: &'a HashMap<MirTypeId, FfiRecordDesc>,
) -> Option<&'a FfiEnumDesc> {
    let id = mir_type_identity(ty)?;
    records.get(&id)?.enum_info.as_ref()
}

fn mir_record_type(
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> bool {
    mir_type_identity(ty).and_then(|id| records.get(&id)).is_some_and(|record| {
        record
            .enum_info
            .as_ref()
            .is_none_or(|enum_info| enum_info.has_payload)
    })
}

fn mir_scalar_enum_type(
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> bool {
    mir_enum_desc(ty, records).is_some_and(|enum_info| !enum_info.has_payload)
}

fn mir_bridge_specs(
    program: &MirProgram,
    artifact: &jet_foundation::MIR::MirArtifactPlan,
) -> Result<Vec<FfiEntrySpec>, BindError> {
    let records = record_descriptors(program)?;
    let mut specs = Vec::new();
    for foreign in &program.foreign {
        let in_module = artifact.modules.contains(&foreign.module_id);
        let in_link = foreign
            .link
            .is_some_and(|link_id| artifact.links.contains(&link_id));
        if !(in_module || in_link) || !target_applicable(foreign, artifact.target) {
            continue;
        }
        let params = foreign
            .params
            .iter()
            .map(|parameter| {
                mir_param_abi(
                    &parameter.ty,
                    mir_foreign_handle_type(program, &parameter.ty),
                    !foreign.bridge_eligible && mir_record_type(&parameter.ty, &records),
                    mir_scalar_enum_type(&parameter.ty, &records),
                )
                .ok_or_else(|| {
                    BindError::Message(format!(
                        "jit ffi: foreign `{}` parameter `{}` has no bridge carrier",
                        foreign.name, parameter.name
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let param_handles = foreign
            .params
            .iter()
            .map(|parameter| mir_handle_id_for_type(program, &parameter.ty))
            .collect::<Vec<_>>();
        let param_types = foreign
            .params
            .iter()
            .map(|parameter| parameter.ty.clone())
            .collect::<Vec<_>>();
        let param_access = foreign
            .params
            .iter()
            .map(|parameter| parameter.access)
            .collect::<Vec<_>>();
        let ret_type = foreign.return_type.clone();
        let ret = mir_ret_abi(
            foreign.return_type.as_ref(),
            foreign
                .return_type
                .as_ref()
                .is_some_and(|ty| mir_foreign_handle_type(program, ty)),
            !foreign.bridge_eligible
                && foreign
                    .return_type
                    .as_ref()
                    .is_some_and(|ty| mir_record_type(ty, &records)),
            foreign
                .return_type
                .as_ref()
                .is_some_and(|ty| mir_scalar_enum_type(ty, &records)),
        )
        .ok_or_else(|| {
            BindError::Message(format!(
                "jit ffi: foreign `{}` return type has no bridge carrier",
                foreign.name
            ))
        })?;
        if foreign.path.is_empty() || foreign.symbol.is_empty() {
            return Err(BindError::Message(format!(
                "jit ffi: foreign `{}` has no checked bridge identity",
                foreign.name
            )));
        }
        let close_handle = program
            .handles
            .iter()
            .find_map(|lifecycle| (lifecycle.close_foreign == Some(foreign.id)).then_some(lifecycle.id));
        specs.push(FfiEntrySpec {
            wrapper_name: bridge_wrapper_name(foreign),
            params,
            param_handles,
            param_types,
            param_access,
            ret,
            ret_type,
            handle: foreign
                .return_type
                .as_ref()
                .and_then(|ty| mir_handle_id_for_type(program, ty)),
            close_handle
        });
    }
    specs.sort_by(|left, right| left.wrapper_name.cmp(&right.wrapper_name));
    specs.dedup_by(|left, right| {
        left.wrapper_name == right.wrapper_name
            && left.params == right.params
            && left.param_handles == right.param_handles
            && left
                .param_types
                .iter()
                .zip(&right.param_types)
                .all(|(left, right)| left.same_checked_type(right))
            && left
                .param_access
                .iter()
                .eq(right.param_access.iter())
            && left.ret == right.ret
            && left
                .ret_type
                .as_ref()
                .zip(right.ret_type.as_ref())
                .is_none_or(|(left, right)| left.same_checked_type(right))
            && left.handle == right.handle
            && left.close_handle == right.close_handle
    });
    Ok(specs)
}
fn align_up(value: usize, align: usize) -> Option<usize> {
    let align = align.max(1);
    let remainder = value % align;
    value.checked_add((align - remainder) % align)
}

fn record_descriptors(program: &MirProgram) -> Result<HashMap<MirTypeId, FfiRecordDesc>, BindError> {
    fn declared_alignment(
        layout: Option<MirStructLayout>,
        fact: Option<&jet_foundation::Layout::LayoutAlignmentFact>,
    ) -> Option<usize> {
        match layout {
            Some(MirStructLayout::C) => Some(1),
            Some(MirStructLayout::CAligned { .. }) => usize::try_from(fact?.effective_alignment)
                .ok()
                .filter(|alignment| {
                    alignment.is_power_of_two() && *alignment <= isize::MAX as usize
                }),
            None | Some(MirStructLayout::Columnar) => None
        }
    }

    fn layout_for_type(
        ty: &MirType,
        defs: &[jet_foundation::MIR::MirTypeDef],
        memo: &mut HashMap<MirTypeId, (usize, usize)>,
        active: &mut HashSet<MirTypeId>,
    ) -> Option<(usize, usize)> {
        match ty.kind() {
            MirTypeKind::Apply { name: nominal, .. } => {
                let id = ty.identity.or(Some(nominal.id))?;
                if let Some(layout) = memo.get(&id).copied() {
                    return Some(layout);
                }
                if !active.insert(id) {
                    return None;
                }
                let definition = defs.iter().find(|definition| definition.id == id)?;
                let result = match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. }
                        if declared_alignment(
                            definition.layout,
                            definition.layout_alignment.as_ref(),
                        )
                        .is_some() =>
                    {
                        let mut offset = 0usize;
                        let mut align = declared_alignment(
                            definition.layout,
                            definition.layout_alignment.as_ref(),
                        )?;
                        for field in fields {
                            let (size, field_align) =
                                layout_for_type(&field.ty, defs, memo, active)?;
                            offset = align_up(offset, field_align)?;
                            offset = offset.checked_add(size)?;
                            align = align.max(field_align);
                        }
                        Some((align_up(offset, align)?, align))
                    }
                    MirTypeDefKind::Enum { .. } if definition.c_layout_tag.is_some() => {
                        let bytes = definition.enum_layout.as_ref()?.bytes?;
                        Some((
                            usize::try_from(bytes.size).ok()?,
                            usize::try_from(bytes.alignment).ok()?.max(1),
                        ))
                    }
                    MirTypeDefKind::Distinct { base, .. } => {
                        layout_for_type(base, defs, memo, active)
                    }
                    MirTypeDefKind::Alias { target } => {
                        layout_for_type(target, defs, memo, active)
                    }
                    _ => None
                };
                active.remove(&id);
                if let Some(result) = result {
                    memo.insert(id, result);
                }
                result
            }
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => {
                layout_for_type(base, defs, memo, active)
            }
            MirTypeKind::Int => Some((8, 8)),
            _ => {
                let size = match ty.layout.size {
                    jet_foundation::MIR::MirSize::Static(size) => usize::try_from(size).ok()?,
                    jet_foundation::MIR::MirSize::Dynamic => return None
                };
                let align = match ty.layout.align {
                    jet_foundation::MIR::MirSize::Static(align) => usize::try_from(align).ok()?,
                    jet_foundation::MIR::MirSize::Dynamic => return None
                };
                Some((size, align.max(1)))
            }
        }
    }

    let mut descriptors = HashMap::new();
    let mut memo = HashMap::new();
    for definition in &program.types {
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            continue;
        };
        let Some(explicit_align) =
            declared_alignment(definition.layout, definition.layout_alignment.as_ref())
        else {
            continue;
        };
        let mut active = HashSet::new();
        let mut offset = 0usize;
        let mut align = explicit_align;
        let mut descriptor_fields = Vec::with_capacity(fields.len());
        let mut valid = true;
        for field in fields {
            let Some((size, field_align)) =
                layout_for_type(&field.ty, &program.types, &mut memo, &mut active)
            else {
                valid = false;
                break;
            };
            let Some(field_offset) = align_up(offset, field_align) else {
                valid = false;
                break;
            };
            descriptor_fields.push(FfiRecordField {
                name: field.name.clone(),
                ty: field.ty.clone(),
                offset: field_offset
            });
            let Some(next) = field_offset.checked_add(size) else {
                valid = false;
                break;
            };
            offset = next;
            align = align.max(field_align);
        }
        if !valid {
            continue;
        }
        let Some(size) = align_up(offset, align) else {
            continue;
        };
        memo.insert(definition.id, (size, align));
        descriptors.insert(
            definition.id,
            FfiRecordDesc {
                id: definition.id,
                name: definition.name.clone(),
                size,
                align,
                fields: descriptor_fields,
                enum_info: None
            },
        );
    }

    for definition in &program.types {
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            continue;
        };
        let Some(tag) = definition.c_layout_tag else {
            continue;
        };
        let Some(layout) = definition.enum_layout.as_ref() else {
            return Err(BindError::Message(format!(
                "jit ffi: C enum `{}` has no checked target layout facts",
                definition.name
            )));
        };
        let Some(bytes) = layout.bytes else {
            return Err(BindError::Message(format!(
                "jit ffi: C enum `{}` has unknown target layout facts",
                definition.name
            )));
        };
        if layout.fields.len() != variants.len() {
            return Err(BindError::Message(format!(
                "jit ffi: C enum `{}` layout facts do not cover every variant",
                definition.name
            )));
        }
        let size = usize::try_from(bytes.size).map_err(|_| {
            BindError::Message(format!(
                "jit ffi: C enum `{}` has an unrepresentable size",
                definition.name
            ))
        })?;
        let align = usize::try_from(bytes.alignment)
            .ok()
            .filter(|alignment| alignment.is_power_of_two() && *alignment <= isize::MAX as usize)
            .ok_or_else(|| {
                BindError::Message(format!(
                    "jit ffi: C enum `{}` has an invalid checked alignment",
                    definition.name
                ))
            })?;
        if bytes.stride != bytes.size {
            return Err(BindError::Message(format!(
                "jit ffi: C enum `{}` has unsupported non-unit stride facts",
                definition.name
            )));
        }
        let tag_size = match tag {
            MirCEnumTag::CInt => 4,
            MirCEnumTag::U8 | MirCEnumTag::I8 => 1,
            MirCEnumTag::U16 | MirCEnumTag::I16 => 2,
            MirCEnumTag::U32 | MirCEnumTag::I32 => 4,
            MirCEnumTag::U64 | MirCEnumTag::I64 => 8
        };
        let mut enum_variants = Vec::with_capacity(variants.len());
        let mut has_payload = false;
        for (variant, fact) in variants.iter().zip(&layout.fields) {
            if fact.name != variant.name {
                return Err(BindError::Message(format!(
                    "jit ffi: C enum `{}` has stale facts for variant `{}`",
                    definition.name, variant.name
                )));
            }
            let offset = fact.offset.and_then(|value| usize::try_from(value).ok());
            let variant_size = fact.size.and_then(|value| usize::try_from(value).ok());
            let variant_align = fact
                .alignment
                .and_then(|value| usize::try_from(value).ok())
                .filter(|alignment| alignment.is_power_of_two() && *alignment <= isize::MAX as usize);
            let stride = fact.stride.and_then(|value| usize::try_from(value).ok());
            let (offset, variant_size, variant_align, stride) =
                match (offset, variant_size, variant_align, stride) {
                    (Some(offset), Some(size), Some(align), Some(stride)) => {
                        (offset, size, align, stride)
                    }
                    _ => {
                        return Err(BindError::Message(format!(
                            "jit ffi: C enum `{}` has unknown facts for variant `{}`",
                            definition.name, variant.name
                        )));
                    }
                };
            let mut field_offsets = Vec::new();
            match &variant.payload {
                MirVariantPayload::Unit => {
                    if offset != 0 || variant_size != tag_size || stride != tag_size {
                        return Err(BindError::Message(format!(
                            "jit ffi: C enum `{}` unit variant `{}` disagrees with its tag facts",
                            definition.name, variant.name
                        )));
                    }
                }
                MirVariantPayload::Single(payload) => {
                    has_payload = true;
                    let (payload_size, payload_align) = layout_for_type(
                        payload,
                        &program.types,
                        &mut memo,
                        &mut HashSet::new(),
                    )
                    .ok_or_else(|| {
                        BindError::Message(format!(
                            "jit ffi: C enum `{}` variant `{}` has an unknown payload layout",
                            definition.name, variant.name
                        ))
                    })?;
                    if payload_size != variant_size
                        || payload_align != variant_align
                        || stride != payload_size
                    {
                        return Err(BindError::Message(format!(
                            "jit ffi: C enum `{}` variant `{}` payload facts disagree with its checked type",
                            definition.name, variant.name
                        )));
                    }
                    field_offsets.push(0);
                }
                MirVariantPayload::Named(payload_fields) => {
                    has_payload = true;
                    let mut payload_offset = 0usize;
                    let mut payload_alignment = 1usize;
                    for field in payload_fields {
                        let (field_size, field_align) = layout_for_type(
                            &field.ty,
                            &program.types,
                            &mut memo,
                            &mut HashSet::new(),
                        )
                        .ok_or_else(|| {
                            BindError::Message(format!(
                                "jit ffi: C enum `{}` variant `{}` has an unknown field layout",
                                definition.name, variant.name
                            ))
                        })?;
                        let field_offset = align_up(payload_offset, field_align).ok_or_else(|| {
                            BindError::Message(format!(
                                "jit ffi: C enum `{}` variant `{}` field layout overflowed",
                                definition.name, variant.name
                            ))
                        })?;
                        field_offsets.push(field_offset);
                        payload_offset = field_offset.checked_add(field_size).ok_or_else(|| {
                            BindError::Message(format!(
                                "jit ffi: C enum `{}` variant `{}` field layout overflowed",
                                definition.name, variant.name
                            ))
                        })?;
                        payload_alignment = payload_alignment.max(field_align);
                    }
                    let payload_size = align_up(payload_offset, payload_alignment).ok_or_else(|| {
                        BindError::Message(format!(
                            "jit ffi: C enum `{}` variant `{}` field layout overflowed",
                            definition.name, variant.name
                        ))
                    })?;
                    if payload_size != variant_size
                        || payload_alignment != variant_align
                        || stride != payload_size
                    {
                        return Err(BindError::Message(format!(
                            "jit ffi: C enum `{}` variant `{}` field facts disagree with its checked layout",
                            definition.name, variant.name
                        )));
                    }
                }
            }
            let discriminant = variant.discriminant.ok_or_else(|| {
                BindError::Message(format!(
                    "jit ffi: C enum `{}` variant `{}` has no checked discriminant",
                    definition.name, variant.name
                ))
            })?;
            enum_variants.push(FfiEnumVariantDesc {
                name: variant.name.clone(),
                discriminant,
                payload: variant.payload.clone(),
                offset,
                size: variant_size,
                align: variant_align,
                stride,
                field_offsets
            });
        }
        descriptors.insert(
            definition.id,
            FfiRecordDesc {
                id: definition.id,
                name: definition.name.clone(),
                size,
                align,
                fields: Vec::new(),
                enum_info: Some(FfiEnumDesc {
                    tag,
                    tag_size,
                    has_payload,
                    variants: enum_variants
                })
            },
        );
        memo.insert(definition.id, (size, align));
    }

    let mut changed = true;
    while changed {
        changed = false;
        for definition in &program.types {
            if descriptors.contains_key(&definition.id) {
                continue;
            }
            let base = match &definition.kind {
                MirTypeDefKind::Distinct { base, .. } => Some(base),
                MirTypeDefKind::Alias { target } => Some(target),
                _ => None
            };
            let Some(base) = base else {
                continue;
            };
            let Some(base_id) = mir_type_identity(base) else {
                continue;
            };
            let Some(base_descriptor) = descriptors.get(&base_id).cloned() else {
                continue;
            };
            descriptors.insert(
                definition.id,
                FfiRecordDesc {
                    id: definition.id,
                    name: definition.name.clone(),
                    size: base_descriptor.size,
                    align: base_descriptor.align,
                    fields: base_descriptor.fields,
                    enum_info: base_descriptor.enum_info
                },
            );
            changed = true;
        }
    }
    Ok(descriptors)
}

pub(crate) fn bind_mir_ffi(
    program: &MirProgram,
    artifact_id: MirArtifactId,
) -> Result<(), BindError> {
    clear_ffi();
    let artifact = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| BindError::Message(format!("jit ffi: artifact {artifact_id:?} is missing")))?;
    if !matches!(
        artifact.target,
        MirArtifactTarget::Interpreter
    ) {
        return Err(BindError::Message(format!(
            "jit ffi: artifact `{}` is not an executable JIT target",
            artifact.name
        )));
    }
    let entries = mir_bridge_specs(program, artifact)?;
    let needs_data_provider = program
        .facts
        .runtime_parts
        .contains(&jet_foundation::MIR::MirRuntimePartId::Data);
    let path = bridge_path(program, artifact);
    // `core.db` marks Data for AOT Arrow/sqlite preludes, but JIT sqlite is
    // in-process rusqlite. A Data-only program with no foreign entries must not
    // ICE for a missing cdylib. Load the Arrow provider only when a bridge exists.
    if entries.is_empty() {
        let Some(path) = path else {
            return Ok(());
        };
        if !needs_data_provider {
            return Ok(());
        }
        let records = record_descriptors(program)?;
        return load_cdylib(
            &path,
            &entries,
            records,
            true,
            artifact.artifact_identity.clone(),
        )
        .map_err(BindError::Message);
    }
    let path = path.ok_or_else(|| {
        BindError::Message(format!(
            "jit ffi: artifact `{}` needs a bridge for foreign calls, but has no dynamic link",
            artifact.name
        ))
    })?;
    let records = record_descriptors(program)?;
    load_cdylib(
        &path,
        &entries,
        records,
        needs_data_provider,
        artifact.artifact_identity.clone(),
    )
    .map_err(BindError::Message)
}

#[cfg(not(unix))]
fn extend_source_helper_ffi_state(
    state: &mut FfiState,
    entries: &[FfiEntrySpec],
) -> Result<(), String> {
    let _ = (state, entries);
    Err("jit ffi: cdylib extension unsupported on this host".into())
}

pub(crate) fn clear_ffi() {
    let mut slot = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut state) = slot.take() {
        let provider_lease = state.arrow_provider_lease.take();
        // Release Foundation's callback registration before dropping the
        // wrapper reference. Escaped Arrow batches retain the erased provider
        // owner independently, so their release callbacks remain valid.
        drop(provider_lease);
        if !state.handle.is_null() {
            unsafe {
                (state.clear_panic_hook_fn)();
                dlclose(state.handle);
            }
        }
    }
}

#[cfg(unix)]
fn load_arrow_provider(
    path: &Path,
) -> Result<jet_foundation::ArrowFileReader::ProviderLease, String> {
    let c_path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "jit ffi: bad provider bridge path".to_string())?;
    let provider_handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW) };
    if provider_handle.is_null() {
        let error = unsafe { dlerror() };
        let detail = if error.is_null() {
            "unknown loader error".to_string()
        } else {
            unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(format!(
            "jit ffi: dlopen provider {}: {detail}",
            path.display()
        ));
    }
    let provider_name = CString::new("jet_data_read").expect("static symbol has no NUL");
    let provider_ptr = unsafe { dlsym(provider_handle, provider_name.as_ptr()) };
    if provider_ptr.is_null() {
        unsafe {
            dlclose(provider_handle);
        }
        return Err(format!(
            "jit ffi: missing symbol `jet_data_read` in {}",
            path.display()
        ));
    }
    // SAFETY: Parquet.rs exports exactly Foundation's Provider ABI.
    let provider: jet_foundation::ArrowFileReader::Provider =
        unsafe { std::mem::transmute(provider_ptr) };
    let owner = Arc::new(FfiProviderLibrary {
        handle: provider_handle
    });
    let owner_erased: Arc<dyn std::any::Any + Send + Sync> = owner.clone();
    match jet_foundation::ArrowFileReader::register_owned(provider, owner_erased) {
        Ok(lease) => Ok(lease),
        Err(error) => {
            drop(owner);
            Err(format!(
                "jit ffi: data provider registration failed for {}: {error}",
                path.display()
            ))
        }
    }
}

fn load_cdylib(
    path: &Path,
    entries: &[FfiEntrySpec],
    records: HashMap<MirTypeId, FfiRecordDesc>,
    needs_data_provider: bool,
    artifact_identity: String,
) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = (path, entries, records, needs_data_provider, artifact_identity);
        return Err("jit ffi: cdylib load unsupported on this host".into());
    }
    #[cfg(unix)]
    {
        let arrow_provider_lease = if needs_data_provider {
            Some(load_arrow_provider(path)?)
        } else {
            None
        };
        let c_path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "jit ffi: bad cdylib path".to_string())?;
        let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW) };
        if handle.is_null() {
            let err = unsafe { CStr::from_ptr(dlerror()) }
                .to_string_lossy()
                .into_owned();
            return Err(format!("jit ffi: dlopen {}: {err}", path.display()));
        }
        let setter_name = CString::new("jet_ffi_set_reporter").unwrap();
        let setter_ptr = unsafe { dlsym(handle, setter_name.as_ptr()) };
        if setter_ptr.is_null() {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_set_reporter` in {}",
                path.display()
            ));
        }
        // The reporter the bridge stores is the generated no-unwind shim, whose
        // C signature is `ffi_reporter`'s own (D-JITUNWIND1). The setter is
        // typed to take that address rather than a `fn` item so the shim is the
        // only thing this crate can pass: a raw `ffi_reporter as *const u8` here
        // would be the unguarded boundary
        // `tests/jit_no_unwind_boundary.rs` scans for.
        let set_reporter = unsafe {
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*const u8)>(setter_ptr)
        };
        unsafe { set_reporter(crate::host_seam::guarded(ffi_reporter)) };
        let clear_hook_name = CString::new("jet_ffi_clear_panic_hook").unwrap();
        let clear_hook_ptr = unsafe { dlsym(handle, clear_hook_name.as_ptr()) };
        if clear_hook_ptr.is_null() {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_clear_panic_hook` in {}",
                path.display()
            ));
        }
        let clear_panic_hook_fn =
            unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn()>(clear_hook_ptr) };
        let failure_name = CString::new("jet_ffi_take_failure").unwrap();
        let failure_ptr = unsafe { dlsym(handle, failure_name.as_ptr()) };
        if failure_ptr.is_null() {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_take_failure` in {}",
                path.display()
            ));
        }
        let take_failure_fn = unsafe {
            std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> i8>(failure_ptr)
        };
        let free_name = CString::new("jet_ffi_cabi_free").unwrap();
        let free_ptr = unsafe { dlsym(handle, free_name.as_ptr()) };
        let free_fn = if free_ptr.is_null() {
            None
        } else {
            Some(unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut u8, usize)>(free_ptr)
            })
        };
        if free_fn.is_none() && entries.iter().any(|entry| entry.ret == RetAbi::String) {
            unsafe {
                dlclose(handle);
            }
            return Err(format!(
                "jit ffi: missing symbol `jet_ffi_cabi_free` in {}",
                path.display()
            ));
        }
        let mut by_wrapper = HashMap::new();
        for entry in entries {
            let is_callback_start = entry.params.contains(&ParamAbi::Callback);
            let symbol = if is_callback_start {
                format!("{}_callback_start", entry.wrapper_name)
            } else {
                format!("{}_cabi", entry.wrapper_name)
            };
            let c_name =
                CString::new(symbol.as_str()).map_err(|_| "jit ffi: bad symbol".to_string())?;
            let raw_ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
            if raw_ptr.is_null() {
                unsafe {
                    dlclose(handle);
                }
                return Err(format!(
                    "jit ffi: missing symbol `{symbol}` in {}",
                    path.display()
                ));
            }
            by_wrapper.insert(
                entry.wrapper_name.clone(),
                FfiEntry {
                    params: entry.params.clone(),
                    param_handles: entry.param_handles.clone(),
                    param_types: entry.param_types.clone(),
                    param_access: entry.param_access.clone(),
                    ret: entry.ret,
                    ret_type: entry.ret_type.clone(),
                    handle: entry.handle,
                    close_handle: entry.close_handle,
                    ptr: raw_ptr as *const ()
                },
            );
        }
        *FFI_STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(FfiState {
            handle,
            clear_panic_hook_fn,
            free_fn,
            take_failure_fn,
            artifact_identity,
            by_wrapper,
            records,
            arrow_provider_lease
        });
        Ok(())
    }
}

fn ffi_diag(wrapper: &str, detail: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("extern call `{wrapper}` {}", detail.into()),
        "the runtime FFI adapter could not marshal this call through the prepared bridge"
            .to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

const FFI_RUNTIME_PANIC: &str = "panic: a foreign function panicked";

fn ffi_runtime_diag(span: Span) -> Diagnostic {
    Diagnostic::from_row("E3014", &[("msg", FFI_RUNTIME_PANIC)], Some(span))
}

fn ffi_bridge_host_fault(wrapper: &str) -> Diagnostic {
    Diagnostic::runtime_host_fault(
        String::new(),
        format!("foreign FFI bridge `{wrapper}` panicked"),
    )
}

fn call_cabi<R>(
    state: &FfiState,
    wrapper: &str,
    span: Span,
    call: impl FnOnce() -> R,
) -> Result<R, Diagnostic> {
    jet_codegen::scheduler::jet_scheduler_world_reject_uncontrolled("foreign");
    let _ = unsafe { (state.take_failure_fn)() };
    let result = jet_codegen::scheduler::jet_scheduler_catch_foreign_boundary(call);
    let failure = unsafe { (state.take_failure_fn)() };
    match (failure, result) {
        (1, _) => Err(ffi_runtime_diag(span)),
        (0, Ok(value)) => Ok(value),
        _ => Err(ffi_bridge_host_fault(wrapper))
    }
}
fn call_uniform(
    state: &FfiState,
    wrapper: &str,
    entry: &FfiEntry,
    slots: &[FfiSlot],
    out: &mut FfiSlot,
    span: Span,
) -> Result<(), Diagnostic> {
    let function: UniformFfi = unsafe { std::mem::transmute(entry.ptr) };
    let rc = call_cabi(state, wrapper, span, || unsafe {
        function(slots.as_ptr(), slots.len(), out as *mut FfiSlot)
    })?;
    if rc == 0 {
        Ok(())
    } else {
        Err(ffi_diag(wrapper, format!("returned {rc}"), span))
    }
}

mod contract_kernel {
    use jet_foundation::Outcome::{
        jet_err, jet_render_err, jet_render_runtime_stop, JetAbsent, JetErr, JetRuntimeDiagnostic,
    };
    include!("../../jet-codegen/src/Prelude/Core/Contracts.rs");
}

fn ffi_int_range_diag(span: Span) -> Diagnostic {
    let code = contract_kernel::JET_C_INT_RANGE_CODE;
    let row = jet_foundation::Registry::diagnostic(code)
        .expect("C Int range diagnostic must be registered");
    Diagnostic::error(
        code,
        contract_kernel::jet_c_int_range_message().to_string(),
        row.why.to_string(),
        row.fix.to_string(),
        Some(span),
    )
}

fn runtime_int_result(value: i64) -> MirRuntimeValue {
    const SMALL_MIN: i64 = -(1i64 << 62);
    const SMALL_MAX: i64 = (1i64 << 62) - 1;
    if (SMALL_MIN..=SMALL_MAX).contains(&value) {
        MirRuntimeValue::Int(value)
    } else {
        MirRuntimeValue::BigInt(value.to_string())
    }
}

fn release_cabi_buffer(
    free_fn: Option<unsafe extern "C" fn(*mut u8, usize)>,
    ptr: *mut u8,
    len: usize,
) {
    if !ptr.is_null() {
        if let Some(free) = free_fn {
            unsafe { free(ptr, len) };
        }
    }
}

fn runtime_string_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(MirRuntimeValue::Moved) => Err(ffi_diag(
            wrapper,
            format!("argument {index} was moved"),
            span,
        )),
        Some(MirRuntimeValue::String(value)) => Ok(value.clone()),
        Some(MirRuntimeValue::NativeOwned(_)) => Err(ffi_diag(
            wrapper,
            format!("argument {index} contains a native-owned payload"),
            span,
        )),
        _ => Err(ffi_diag(
            wrapper,
            format!("argument {index} is not a String"),
            span,
        ))

    }
}

fn runtime_handle_arg(
    args: &[MirRuntimeValue],
    index: usize,
    wrapper: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    let value = match args.get(index) {
        Some(MirRuntimeValue::Moved) => {
            return Err(ffi_diag(
                wrapper,
                format!("argument {index} was moved"),
                span,
            ));
        }
        Some(MirRuntimeValue::Int(value)) => *value,
        Some(MirRuntimeValue::BigInt(value)) => {
            value.parse::<i64>().map_err(|_| ffi_int_range_diag(span))?
        }
        Some(MirRuntimeValue::NativeOwned(_)) => {
            return Err(ffi_diag(
                wrapper,
                format!("argument {index} contains a native-owned payload"),
                span,
            ));
        }
        _ => {
            return Err(ffi_diag(
                wrapper,
                format!("argument {index} is not an opaque handle"),
                span,
            ));
        }
    };
    Ok(value)
}

fn runtime_handle_result(
    handle: Option<MirHandleId>,
    raw: i64,
    wrapper: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    if handle.is_none() {
        return Err(ffi_diag(
            wrapper,
            "returned a handle without a checked handle identity",
            span,
        ));
    }
    Ok(MirRuntimeValue::Int(raw))
}

fn runtime_float_result(value: f64, ret_f32: bool) -> MirRuntimeValue {
    MirRuntimeValue::Float {
        value: if ret_f32 { value as f32 as f64 } else { value },
        f32: ret_f32
    }
}

fn type_size_align(
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
) -> Option<(usize, usize)> {
    match ty.kind() {
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty.identity.or(Some(nominal.id))?;
            records.get(&id).map(|record| (record.size, record.align))
        }
        MirTypeKind::Int => Some((8, 8)),
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => type_size_align(base, records),
        _ => {
            let size = match ty.layout.size {
                jet_foundation::MIR::MirSize::Static(size) => usize::try_from(size).ok()?,
                jet_foundation::MIR::MirSize::Dynamic => return None
            };
            let align = match ty.layout.align {
                jet_foundation::MIR::MirSize::Static(align) => usize::try_from(align).ok()?,
                jet_foundation::MIR::MirSize::Dynamic => return None
            };
            Some((size, align.max(1)))
        }
    }
}
fn enum_tag_is_signed(tag: MirCEnumTag) -> bool {
    matches!(
        tag,
        MirCEnumTag::CInt
            | MirCEnumTag::I8
            | MirCEnumTag::I16
            | MirCEnumTag::I32
            | MirCEnumTag::I64
    )
}

fn enum_tag_value_valid(tag: MirCEnumTag, tag_size: usize, value: i64) -> bool {
    let Some(bits) = tag_size.checked_mul(8) else {
        return false;
    };
    if bits == 0 || bits > 64 {
        return false;
    }
    if enum_tag_is_signed(tag) {
        if bits == 64 {
            true
        } else {
            let minimum = -(1i64 << (bits - 1));
            let maximum = (1i64 << (bits - 1)) - 1;
            (minimum..=maximum).contains(&value)
        }
    } else {
        value >= 0 && (bits == 64 || (value as u64) <= ((1u64 << bits) - 1))
    }
}

fn write_enum_tag(
    bytes: &mut [u8],
    enum_info: &FfiEnumDesc,
    value: i64,
    wrapper: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    if bytes.len() < enum_info.tag_size {
        return Err(ffi_diag(wrapper, "enum storage is shorter than its checked tag", span));
    }
    if !enum_tag_value_valid(enum_info.tag, enum_info.tag_size, value) {
        return Err(ffi_diag(
            wrapper,
            "enum discriminant does not fit its checked C tag",
            span,
        ));
    }
    let raw = value as u64;
    bytes[..enum_info.tag_size].copy_from_slice(&raw.to_ne_bytes()[..enum_info.tag_size]);
    Ok(())
}

fn read_enum_tag(
    bytes: &[u8],
    enum_info: &FfiEnumDesc,
    wrapper: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    if bytes.len() < enum_info.tag_size {
        return Err(ffi_diag(wrapper, "native enum storage is shorter than its checked tag", span));
    }
    let mut raw_bytes = [0u8; 8];
    raw_bytes[..enum_info.tag_size].copy_from_slice(&bytes[..enum_info.tag_size]);
    let raw = u64::from_ne_bytes(raw_bytes);
    let bits = enum_info.tag_size * 8;
    let value = if enum_tag_is_signed(enum_info.tag) {
        if bits == 64 {
            raw as i64
        } else if raw & (1u64 << (bits - 1)) != 0 {
            (raw | (!0u64 << bits)) as i64
        } else {
            raw as i64
        }
    } else {
        i64::try_from(raw).map_err(|_| {
            ffi_diag(
                wrapper,
                "native enum tag is outside the checked discriminant range",
                span,
            )
        })?
    };
    Ok(value)
}

fn enum_variant_by_name<'a>(
    enum_info: &'a FfiEnumDesc,
    name: &str,
    wrapper: &str,
    span: Span,
) -> Result<&'a FfiEnumVariantDesc, Diagnostic> {
    enum_info
        .variants
        .iter()
        .find(|variant| variant.name == name)
        .ok_or_else(|| ffi_diag(wrapper, format!("unknown C enum variant `{name}`"), span))
}

fn enum_variant_by_discriminant<'a>(
    enum_info: &'a FfiEnumDesc,
    discriminant: i64,
    wrapper: &str,
    span: Span,
) -> Result<&'a FfiEnumVariantDesc, Diagnostic> {
    enum_info
        .variants
        .iter()
        .find(|variant| variant.discriminant == discriminant)
        .ok_or_else(|| {
            ffi_diag(
                wrapper,
                "native C enum discriminant is invalid",
                span,
            )
        })
}

fn encode_enum_bytes_at(
    value: &MirRuntimeValue,
    enum_info: &FfiEnumDesc,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    bytes: &mut [u8],
    wrapper: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let (variant_name, args, scalar_discriminant) = match value {
        MirRuntimeValue::Enum {
            variant, args, ..
        } => (variant.as_str(), args.as_slice(), None),
        MirRuntimeValue::Int(discriminant) if !enum_info.has_payload => {
            ("", &[][..], Some(*discriminant))
        }
        MirRuntimeValue::NativeOwned(_) => {
            return Err(ffi_diag(
                wrapper,
                "native-owned payload cannot cross the C bridge",
                span,
            ));
        }
        _ => {
            return Err(ffi_diag(
                wrapper,
                "received a non-enum value for a checked C enum",
                span,
            ))
        }
    };
    let variant = if let Some(discriminant) = scalar_discriminant {
        enum_variant_by_discriminant(enum_info, discriminant, wrapper, span)?
    } else {
        enum_variant_by_name(enum_info, variant_name, wrapper, span)?
    };
    let discriminant = scalar_discriminant.unwrap_or(variant.discriminant);
    write_enum_tag(bytes, enum_info, discriminant, wrapper, span)?;
    let payload_end = variant.offset.checked_add(variant.size).ok_or_else(|| {
        ffi_diag(wrapper, "enum payload offset overflowed its checked C layout", span)
    })?;
    let payload = bytes.get_mut(variant.offset..payload_end).ok_or_else(|| {
        ffi_diag(wrapper, "enum payload exceeds its checked C layout", span)
    })?;
    if !matches!(&variant.payload, MirVariantPayload::Unit) {
        payload.fill(0);
    }
    match &variant.payload {
        MirVariantPayload::Unit => {
            if !args.is_empty() {
                return Err(ffi_diag(
                    wrapper,
                    "unit C enum variant received a payload",
                    span,
                ));
            }
        }
        MirVariantPayload::Single(payload_ty) => {
            if args.len() != 1 || args[0].0.is_some() {
                return Err(ffi_diag(
                    wrapper,
                    "single-payload C enum variant has the wrong argument shape",
                    span,
                ));
            }
            let field_bytes = encode_bytes(&args[0].1, payload_ty, records, wrapper, span)?;
            let field_offset = *variant.field_offsets.first().ok_or_else(|| {
                ffi_diag(wrapper, "C enum payload has no checked field offset", span)
            })?;
            let end = field_offset.checked_add(field_bytes.len()).ok_or_else(|| {
                ffi_diag(wrapper, "C enum payload field offset overflowed", span)
            })?;
            payload
                .get_mut(field_offset..end)
                .ok_or_else(|| ffi_diag(wrapper, "C enum payload field exceeds its layout", span))?
                .copy_from_slice(&field_bytes);
        }
        MirVariantPayload::Named(payload_fields) => {
            if args.len() != payload_fields.len() {
                return Err(ffi_diag(
                    wrapper,
                    "named C enum variant has the wrong argument count",
                    span,
                ));
            }
            for ((field, argument), field_offset) in payload_fields
                .iter()
                .zip(args)
                .zip(&variant.field_offsets)
            {
                if argument.0.as_deref() != Some(field.name.as_str()) {
                    return Err(ffi_diag(
                        wrapper,
                        format!("C enum payload is missing field `{}`", field.name),
                        span,
                    ));
                }
                let field_bytes = encode_bytes(&argument.1, &field.ty, records, wrapper, span)?;
                let end = field_offset.checked_add(field_bytes.len()).ok_or_else(|| {
                    ffi_diag(wrapper, "C enum payload field offset overflowed", span)
                })?;
                payload
                    .get_mut(*field_offset..end)
                    .ok_or_else(|| {
                        ffi_diag(wrapper, "C enum payload field exceeds its layout", span)
                    })?
                    .copy_from_slice(&field_bytes);
            }
        }
    }
    Ok(())
}

fn decode_enum_bytes(
    bytes: &[u8],
    enum_info: &FfiEnumDesc,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    enum_name: &str,
    wrapper: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let discriminant = read_enum_tag(bytes, enum_info, wrapper, span)?;
    let variant = enum_variant_by_discriminant(enum_info, discriminant, wrapper, span)?;
    let payload_end = variant.offset.checked_add(variant.size).ok_or_else(|| {
        ffi_diag(wrapper, "enum payload offset overflowed its checked C layout", span)
    })?;
    let payload = bytes.get(variant.offset..payload_end).ok_or_else(|| {
        ffi_diag(wrapper, "enum payload exceeds its checked C layout", span)
    })?;
    let mut args = Vec::new();
    match &variant.payload {
        MirVariantPayload::Unit => {}
        MirVariantPayload::Single(payload_ty) => {
            let field_offset = *variant.field_offsets.first().ok_or_else(|| {
                ffi_diag(wrapper, "C enum payload has no checked field offset", span)
            })?;
            let (field_size, _) = type_size_align(payload_ty, records).ok_or_else(|| {
                ffi_diag(wrapper, "C enum payload field has no static layout", span)
            })?;
            let end = field_offset.checked_add(field_size).ok_or_else(|| {
                ffi_diag(wrapper, "C enum payload field offset overflowed", span)
            })?;
            let field_bytes = payload.get(field_offset..end).ok_or_else(|| {
                ffi_diag(wrapper, "C enum payload field exceeds its layout", span)
            })?;
            args.push((
                None,
                decode_bytes(field_bytes, payload_ty, records, wrapper, span)?,
            ));
        }
        MirVariantPayload::Named(payload_fields) => {
            for (field, field_offset) in payload_fields.iter().zip(&variant.field_offsets) {
                let (field_size, _) = type_size_align(&field.ty, records).ok_or_else(|| {
                    ffi_diag(wrapper, "C enum payload field has no static layout", span)
                })?;
                let end = field_offset.checked_add(field_size).ok_or_else(|| {
                    ffi_diag(wrapper, "C enum payload field offset overflowed", span)
                })?;
                let field_bytes = payload.get(*field_offset..end).ok_or_else(|| {
                    ffi_diag(wrapper, "C enum payload field exceeds its layout", span)
                })?;
                args.push((
                    Some(field.name.clone()),
                    decode_bytes(field_bytes, &field.ty, records, wrapper, span)?,
                ));
            }
        }
    }
    Ok(MirRuntimeValue::Enum {
        type_name: enum_name.to_string(),
        variant: variant.name.clone(),
        args
    })
}

fn value_int(
    value: &MirRuntimeValue,
    wrapper: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(*value),
        MirRuntimeValue::BigInt(value) => value
            .parse::<i64>()
            .map_err(|_| ffi_int_range_diag(span)),
        MirRuntimeValue::Moved => Err(ffi_diag(wrapper, "received a moved value", span)),
        MirRuntimeValue::NativeOwned(_) => Err(ffi_diag(
            wrapper,
            "native-owned payload cannot cross the C bridge",
            span,
        )),
        _ => Err(ffi_diag(wrapper, "received a non-integer value", span))
    }
}

fn encode_bytes(
    value: &MirRuntimeValue,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    wrapper: &str,
    span: Span,
) -> Result<Vec<u8>, Diagnostic> {
    let (size, _) = type_size_align(ty, records).ok_or_else(|| {
        ffi_diag(
            wrapper,
            format!("type `{}` has no static C layout", ty.canonical_key()),
            span,
        )
    })?;
    let mut bytes = vec![0; size];
    encode_bytes_at(value, ty, records, &mut bytes, wrapper, span)?;
    Ok(bytes)
}

fn encode_bytes_at(
    value: &MirRuntimeValue,
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    bytes: &mut [u8],
    wrapper: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    if matches!(value, MirRuntimeValue::NativeOwned(_)) {
        return Err(ffi_diag(
            wrapper,
            "native-owned payload cannot cross the C bridge",
            span,
        ));
    }
    let write_uint = |bytes: &mut [u8], value: u64| {
        let raw = value.to_ne_bytes();
        bytes.copy_from_slice(&raw[..bytes.len()]);
    };
    match ty.kind() {
        MirTypeKind::Int => write_uint(bytes, value_int(value, wrapper, span)? as u64),
        MirTypeKind::IntN { .. } => write_uint(bytes, value_int(value, wrapper, span)? as u64),
        MirTypeKind::Float => {
            let MirRuntimeValue::Float { value, .. } = value else {
                return Err(ffi_diag(wrapper, "received a non-Float value", span));
            };
            bytes.copy_from_slice(&value.to_ne_bytes()[..bytes.len()]);
        }
        MirTypeKind::Float32 => {
            let MirRuntimeValue::Float { value, .. } = value else {
                return Err(ffi_diag(wrapper, "received a non-Float value", span));
            };
            bytes.copy_from_slice(&(*value as f32).to_ne_bytes()[..bytes.len()]);
        }
        MirTypeKind::Bool => {
            let MirRuntimeValue::Bool(value) = value else {
                return Err(ffi_diag(wrapper, "received a non-Bool value", span));
            };
            bytes[0] = u8::from(*value);
        }
        MirTypeKind::Char => {
            let MirRuntimeValue::Char(value) = value else {
                return Err(ffi_diag(wrapper, "received a non-Char value", span));
            };
            write_uint(bytes, *value as u32 as u64);
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => {
            encode_bytes_at(value, base, records, bytes, wrapper, span)?
        }
        MirTypeKind::FixedList { elem, .. } => {
            let MirRuntimeValue::List(values) = value else {
                return Err(ffi_diag(wrapper, "received a non-list fixed array", span));
            };
            let (element_size, _) = type_size_align(elem, records).ok_or_else(|| {
                ffi_diag(wrapper, "fixed-array element has no static C layout", span)
            })?;
            if element_size == 0 || bytes.len() % element_size != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "fixed-array C layout is not element-aligned",
                    span,
                ));
            }
            let expected_len = bytes.len() / element_size;
            if values.len() != expected_len {
                return Err(ffi_diag(
                    wrapper,
                    "fixed-array value count does not match its checked C layout",
                    span,
                ));
            }
            for (index, field_value) in values.iter().enumerate() {
                let start = index * element_size;
                encode_bytes_at(
                    field_value,
                    elem,
                    records,
                    &mut bytes[start..start + element_size],
                    wrapper,
                    span,
                )?;
            }
        }
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty.identity.or(Some(nominal.id)).ok_or_else(|| {
                ffi_diag(wrapper, "record has no checked type identity", span)
            })?;
            let descriptor = records.get(&id).ok_or_else(|| {
                ffi_diag(
                    wrapper,
                    format!("record `{}` has no checked C layout", nominal.name),
                    span,
                )
            })?;
            if let Some(enum_info) = descriptor.enum_info.as_ref() {
                return encode_enum_bytes_at(value, enum_info, records, bytes, wrapper, span);
            }
            let MirRuntimeValue::Struct { fields, .. } = value else {
                return Err(ffi_diag(wrapper, "received a non-record value", span));
            };
            for (index, field) in descriptor.fields.iter().enumerate() {
                let field_value = fields
                    .iter()
                    .find(|(name, _)| name == &field.name)
                    .or_else(|| fields.get(index))
                    .map(|(_, value)| value)
                    .ok_or_else(|| {
                        ffi_diag(
                            wrapper,
                            format!("record is missing field `{}`", field.name),
                            span,
                        )
                    })?;
                let field_bytes =
                    encode_bytes(field_value, &field.ty, records, wrapper, span)?;
                let end = field.offset.checked_add(field_bytes.len()).ok_or_else(|| {
                    ffi_diag(wrapper, "record field offset overflowed", span)
                })?;
                let destination = bytes.get_mut(field.offset..end).ok_or_else(|| {
                    ffi_diag(wrapper, "record field exceeds its checked C layout", span)
                })?;
                destination.copy_from_slice(&field_bytes);
            }
        }
        _ => {
            return Err(ffi_diag(
                wrapper,
                format!("type `{}` cannot cross the C bridge", ty.canonical_key()),
                span,
            ))
        }
    }
    Ok(())
}

fn decode_bytes(
    bytes: &[u8],
    ty: &MirType,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    wrapper: &str,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let (size, _) = type_size_align(ty, records).ok_or_else(|| {
        ffi_diag(
            wrapper,
            format!("type `{}` has no static C layout", ty.canonical_key()),
            span,
        )
    })?;
    if bytes.len() < size {
        return Err(ffi_diag(wrapper, "native data is shorter than its C layout", span));
    }
    let bytes = &bytes[..size];
    let read_uint = |bytes: &[u8]| {
        let mut raw = [0; 8];
        raw[..bytes.len()].copy_from_slice(bytes);
        u64::from_ne_bytes(raw)
    };
    match ty.kind() {
        MirTypeKind::Int => Ok(runtime_int_result(read_uint(bytes) as i64)),
        MirTypeKind::IntN { signed, bits } => {
            let value = read_uint(bytes);
            let value = match (*signed, *bits) {
                (true, 8) => (value as i8) as i64,
                (true, 16) => (value as i16) as i64,
                (true, 32) => (value as i32) as i64,
                (true, 64) => value as i64,
                (false, 8) => (value as u8) as i64,
                (false, 16) => (value as u16) as i64,
                (false, 32) => (value as u32) as i64,
                (false, 64) => value as i64,
                _ => {
                    return Err(ffi_diag(
                        wrapper,
                        "integer width has no checked C representation",
                        span,
                    ))
                }
            };
            Ok(runtime_int_result(value))
        }
        MirTypeKind::Float => {
            let mut raw = [0; 8];
            raw.copy_from_slice(&bytes[..8]);
            Ok(MirRuntimeValue::Float {
                value: f64::from_ne_bytes(raw),
                f32: false
            })
        }
        MirTypeKind::Float32 => {
            let mut raw = [0; 4];
            raw.copy_from_slice(&bytes[..4]);
            Ok(MirRuntimeValue::Float {
                value: f32::from_ne_bytes(raw) as f64,
                f32: true
            })
        }
        MirTypeKind::Bool => Ok(MirRuntimeValue::Bool(bytes[0] != 0)),
        MirTypeKind::Char => {
            let value = read_uint(bytes) as u32;
            char::from_u32(value)
                .map(MirRuntimeValue::Char)
                .ok_or_else(|| ffi_diag(wrapper, "native data is not a valid Char", span))
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => {
            decode_bytes(bytes, base, records, wrapper, span)
        }
        MirTypeKind::FixedList { elem, .. } => {
            let (element_size, _) = type_size_align(elem, records).ok_or_else(|| {
                ffi_diag(wrapper, "fixed-array element has no static C layout", span)
            })?;
            if element_size == 0 || bytes.len() % element_size != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "fixed-array C layout is not element-aligned",
                    span,
                ));
            }
            bytes
                .chunks_exact(element_size)
                .map(|chunk| decode_bytes(chunk, elem, records, wrapper, span))
                .collect::<Result<Vec<_>, _>>()
                .map(MirRuntimeValue::List)
        }
        MirTypeKind::Apply { name: nominal, .. } => {
            let id = ty.identity.or(Some(nominal.id)).ok_or_else(|| {
                ffi_diag(wrapper, "record has no checked type identity", span)
            })?;
            let descriptor = records.get(&id).ok_or_else(|| {
                ffi_diag(
                    wrapper,
                    format!("record `{}` has no checked C layout", nominal.name),
                    span,
                )
            })?;
            if let Some(enum_info) = descriptor.enum_info.as_ref() {
                return decode_enum_bytes(bytes, enum_info, records, &descriptor.name, wrapper, span);
            }
            let mut fields = Vec::with_capacity(descriptor.fields.len());
            for field in &descriptor.fields {
                let (_, field_size) = type_size_align(&field.ty, records)
                    .map(|(size, _)| (field.offset, size))
                    .ok_or_else(|| {
                        ffi_diag(wrapper, "record field has no static C layout", span)
                    })?;
                let end = field.offset.checked_add(field_size).ok_or_else(|| {
                    ffi_diag(wrapper, "record field offset overflowed", span)
                })?;
                let field_bytes = bytes.get(field.offset..end).ok_or_else(|| {
                    ffi_diag(wrapper, "record field exceeds its checked C layout", span)
                })?;
                fields.push((
                    field.name.clone(),
                    decode_bytes(field_bytes, &field.ty, records, wrapper, span)?,
                ));
            }
            Ok(MirRuntimeValue::Struct {
                type_name: descriptor.name.clone(),
                fields
            })
        }
        _ => Err(ffi_diag(
            wrapper,
            format!("type `{}` cannot cross the C bridge", ty.canonical_key()),
            span,
        ))
    }
}

fn prepare_runtime_call(
    args: &[MirRuntimeValue],
    entry: &FfiEntry,
    records: &HashMap<MirTypeId, FfiRecordDesc>,
    wrapper: &str,
    span: Span,
) -> Result<(Vec<FfiSlot>, Vec<FfiStorage>), Diagnostic> {
    let mut slots = Vec::with_capacity(args.len());
    let mut storage = Vec::new();
    for (index, ((abi, ty), access)) in entry
        .params
        .iter()
        .zip(&entry.param_types)
        .zip(&entry.param_access)
        .enumerate()
    {
        let value = args.get(index).ok_or_else(|| {
            ffi_diag(wrapper, format!("argument {index} is missing"), span)
        })?;
        match abi {
            ParamAbi::Callback => {
                return Err(ffi_diag(
                    wrapper,
                    "managed callback start requires the callback registration adapter",
                    span,
                ));
            }
            ParamAbi::Record => {
                let (size, align) = type_size_align(ty, records).ok_or_else(|| {
                    ffi_diag(wrapper, "record parameter has no static C layout", span)
                })?;
                let bytes = encode_bytes(value, ty, records, wrapper, span)?;
                if bytes.len() != size {
                    return Err(ffi_diag(
                        wrapper,
                        "record parameter encoding does not match its checked C layout",
                        span,
                    ));
                }
                let data = FfiStorage::from_bytes(&bytes, align);
                let ptr = data.as_ptr();
                storage.push(data);
                slots.push(FfiSlot {
                    value: 0,
                    ptr,
                    len: size
                });
            }
            ParamAbi::List => {
                let MirTypeKind::List(inner) = ty.kind() else {
                    return Err(ffi_diag(wrapper, "list carrier has a non-list type", span));
                };
                let MirRuntimeValue::List(values) = value else {
                    return Err(ffi_diag(wrapper, format!("argument {index} is not a List"), span));
                };
                let mut bytes = Vec::new();
                for value in values {
                    bytes.extend(encode_bytes(value, inner, records, wrapper, span)?);
                }
                let (_, element_align) = type_size_align(inner, records).ok_or_else(|| {
                    ffi_diag(wrapper, "list element has no static C layout", span)
                })?;
                let data = FfiStorage::from_bytes(&bytes, element_align);
                let ptr = if bytes.is_empty() {
                    std::ptr::null_mut()
                } else {
                    data.as_ptr()
                };
                let len = values.len();
                storage.push(data);
                slots.push(FfiSlot {
                    value: 0,
                    ptr,
                    len
                });
            }
            ParamAbi::String => {
                let value = runtime_string_arg(args, index, wrapper, span)?;
                let mut bytes = value.into_bytes();
                let len = bytes.len();
                if *access == MirAccess::Write {
                    bytes.push(0);
                }
                let data = FfiStorage::from_bytes(&bytes, 1);
                let ptr = data.as_ptr();
                storage.push(data);
                slots.push(FfiSlot {
                    value: 0,
                    ptr,
                    len
                });
            }
            ParamAbi::Handle => {
                let raw = runtime_handle_arg(args, index, wrapper, span)?;
                slots.push(FfiSlot {
                    value: raw as u64,
                    ptr: std::ptr::null_mut(),
                    len: 0
                });
            }
            ParamAbi::Int | ParamAbi::Float | ParamAbi::Bool => {
                if *access == MirAccess::Write {
                    let (size, align) = type_size_align(ty, records).ok_or_else(|| {
                        ffi_diag(wrapper, "write parameter has no static C layout", span)
                    })?;
                    let mut data = FfiStorage::zeroed(size, align);
                    encode_bytes_at(value, ty, records, data.as_bytes_mut(), wrapper, span)?;
                    let ptr = data.as_ptr();
                    storage.push(data);
                    slots.push(FfiSlot {
                        value: 0,
                        ptr,
                        len: 0
                    });
                } else {
                    let bits = if let Some(enum_info) = mir_enum_desc(ty, records) {
                        if enum_info.has_payload {
                            return Err(ffi_diag(
                                wrapper,
                                "payload C enum has no scalar representation",
                                span,
                            ));
                        }
                        let discriminant = match value {
                            MirRuntimeValue::Enum { variant, args, .. } => {
                                if !args.is_empty() {
                                    return Err(ffi_diag(
                                        wrapper,
                                        "unit C enum variant received a payload",
                                        span,
                                    ));
                                }
                                enum_variant_by_name(enum_info, variant, wrapper, span)?
                                    .discriminant
                            }
                            MirRuntimeValue::Int(value) => {
                                enum_variant_by_discriminant(enum_info, *value, wrapper, span)?;
                                *value
                            }
                            MirRuntimeValue::NativeOwned(_) => {
                                return Err(ffi_diag(
                                    wrapper,
                                    "native-owned payload cannot cross the C bridge",
                                    span,
                                ));
                            }
                            _ => {
                                return Err(ffi_diag(
                                    wrapper,
                                    "argument is not a checked C enum value",
                                    span,
                                ))
                            }
                        };
                        discriminant as u64
                    } else {
                        match ty.kind() {
                            MirTypeKind::Float => {
                                let MirRuntimeValue::Float { value, .. } = value else {
                                    return Err(ffi_diag(wrapper, "argument is not a Float", span));
                                };
                                value.to_bits()
                            }
                            MirTypeKind::Float32 => {
                                let MirRuntimeValue::Float { value, .. } = value else {
                                    return Err(ffi_diag(wrapper, "argument is not a Float", span));
                                };
                                (*value as f32).to_bits() as u64
                            }
                            MirTypeKind::Bool => {
                                let MirRuntimeValue::Bool(value) = value else {
                                    return Err(ffi_diag(wrapper, "argument is not a Bool", span));
                                };
                                u64::from(*value)
                            }
                            MirTypeKind::Int
                            | MirTypeKind::IntN { .. }
                            | MirTypeKind::InlineRange { .. }
                            | MirTypeKind::Tagged { .. }
                            | MirTypeKind::Quantity { .. } => {
                                value_int(value, wrapper, span)? as u64
                            }
                            _ => {
                                return Err(ffi_diag(
                                    wrapper,
                                    "parameter has no scalar C representation",
                                    span,
                                ))
                            }
                        }
                    };
                    slots.push(FfiSlot {
                        value: bits,
                        ptr: std::ptr::null_mut(),
                        len: 0
                    });
                }
            }
        }
    }
    Ok((slots, storage))
}

fn decode_runtime_result(
    state: &FfiState,
    entry: &FfiEntry,
    out: &FfiSlot,
    wrapper: &str,
    span: Span,
    ret_f32: bool,
) -> Result<MirRuntimeValue, Diagnostic> {
    match entry.ret {
        RetAbi::Unit => Ok(MirRuntimeValue::Unit),
        RetAbi::Int => {
            let Some(ty) = entry.ret_type.as_ref() else {
                return Ok(runtime_int_result(out.value as i64));
            };
            let Some(id) = mir_type_identity(ty) else {
                return Ok(runtime_int_result(out.value as i64));
            };
            let Some(descriptor) = state.records.get(&id) else {
                return Ok(runtime_int_result(out.value as i64));
            };
            let Some(enum_info) = descriptor.enum_info.as_ref() else {
                return Ok(runtime_int_result(out.value as i64));
            };
            if enum_info.has_payload {
                return Err(ffi_diag(
                    wrapper,
                    "payload C enum has no scalar return carrier",
                    span,
                ));
            }
            let discriminant =
                read_enum_tag(&out.value.to_ne_bytes(), enum_info, wrapper, span)?;
            let variant = enum_variant_by_discriminant(enum_info, discriminant, wrapper, span)?;
            Ok(MirRuntimeValue::Enum {
                type_name: descriptor.name.clone(),
                variant: variant.name.clone(),
                args: Vec::new()
            })
        }
        RetAbi::Float => {
            let value = if entry
                .ret_type
                .as_ref()
                .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Float32))
            {
                f32::from_bits(out.value as u32) as f64
            } else {
                f64::from_bits(out.value)
            };
            Ok(runtime_float_result(value, ret_f32))
        }
        RetAbi::Bool => Ok(MirRuntimeValue::Bool(out.value != 0)),
        RetAbi::Handle => runtime_handle_result(entry.handle, out.value as i64, wrapper, span),
        RetAbi::String => {
            if out.ptr.is_null() && out.len != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if out.ptr.is_null() {
                Vec::new()
            } else {
                // SAFETY: the generated trampoline owns this result buffer
                // until the host copies it and invokes its matching free hook.
                unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec()
            };
            release_cabi_buffer(state.free_fn, out.ptr, out.len);
            String::from_utf8(bytes)
                .map(MirRuntimeValue::String)
                .map_err(|_| ffi_diag(wrapper, "returned invalid UTF-8", span))
        }
        RetAbi::Record => {
            let ty = entry
                .ret_type
                .as_ref()
                .ok_or_else(|| ffi_diag(wrapper, "record result has no type", span))?;
            let (size, _) = type_size_align(ty, &state.records).ok_or_else(|| {
                ffi_diag(wrapper, "record result has no static C layout", span)
            })?;
            if out.ptr.is_null() || out.len != size {
                release_cabi_buffer(state.free_fn, out.ptr, out.len);
                return Err(ffi_diag(
                    wrapper,
                    "returned record buffer does not match its checked C layout",
                    span,
                ));
            }
            let bytes = unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec();
            release_cabi_buffer(state.free_fn, out.ptr, out.len);
            decode_bytes(&bytes, ty, &state.records, wrapper, span)
        }
        RetAbi::List => {
            let MirTypeKind::List(inner) = entry
                .ret_type
                .as_ref()
                .ok_or_else(|| ffi_diag(wrapper, "list result has no type", span))?
                .kind()
            else {
                return Err(ffi_diag(wrapper, "list result has a non-list type", span));
            };
            let (element_size, _) = type_size_align(inner, &state.records).ok_or_else(|| {
                ffi_diag(wrapper, "list element has no static C layout", span)
            })?;
            let total = element_size.checked_mul(out.len).ok_or_else(|| {
                ffi_diag(wrapper, "returned list length overflowed its C layout", span)
            })?;
            if out.ptr.is_null() && total != 0 {
                return Err(ffi_diag(
                    wrapper,
                    "returned a null list buffer with a non-zero length",
                    span,
                ));
            }
            let bytes = if total == 0 {
                Vec::new()
            } else {
                // SAFETY: the C trampoline reports the native pointer and
                // element count; copy before any owner can be invalidated.
                unsafe { std::slice::from_raw_parts(out.ptr, total) }.to_vec()
            };
            let values = (0..out.len)
                .map(|index| {
                    let start = index * element_size;
                    decode_bytes(
                        &bytes[start..start + element_size],
                        inner,
                        &state.records,
                        wrapper,
                        span,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(MirRuntimeValue::List(values))
        }
    }
}
fn decode_runtime_writebacks(
    state: &FfiState,
    entry: &FfiEntry,
    args: &[MirRuntimeValue],
    slots: &[FfiSlot],
    storage: &[FfiStorage],
    wrapper: &str,
    span: Span,
) -> Result<Vec<AmbientMirExternWriteback>, Diagnostic> {
    let mut storage_index = 0usize;
    let mut writebacks = Vec::new();
    for (index, ((abi, ty), access)) in entry
        .params
        .iter()
        .zip(&entry.param_types)
        .zip(&entry.param_access)
        .enumerate()
    {
        match abi {
            ParamAbi::Record => {
                let current_storage = storage_index;
                storage_index += 1;
                if *access != MirAccess::Write {
                    continue;
                }
                let bytes = storage
                    .get(current_storage)
                    .map(FfiStorage::as_bytes)
                    .ok_or_else(|| ffi_diag(wrapper, "mutable record storage is missing", span))?;
                let value = decode_bytes(bytes, ty, &state.records, wrapper, span)?;
                writebacks.push(AmbientMirExternWriteback {
                    parameter: index,
                    value
                });
            }
            ParamAbi::List => {
                let current_storage = storage_index;
                storage_index += 1;
                if *access != MirAccess::Write {
                    continue;
                }
                let MirTypeKind::List(inner) = ty.kind() else {
                    return Err(ffi_diag(wrapper, "list carrier has a non-list type", span));
                };
                let (element_size, _) =
                    type_size_align(inner, &state.records).ok_or_else(|| {
                        ffi_diag(
                            wrapper,
                            "mutable list element has no static C layout",
                            span,
                        )
                    })?;
                let slot = slots.get(index).copied().ok_or_else(|| {
                    ffi_diag(wrapper, "mutable list slot is missing", span)
                })?;
                let bytes = storage
                    .get(current_storage)
                    .map(FfiStorage::as_bytes)
                    .ok_or_else(|| ffi_diag(wrapper, "mutable list storage is missing", span))?;
                let mut values = Vec::with_capacity(slot.len);
                for element_index in 0..slot.len {
                    let start = element_index.checked_mul(element_size).ok_or_else(|| {
                        ffi_diag(
                            wrapper,
                            "mutable list length overflowed its C layout",
                            span,
                        )
                    })?;
                    let end = start.checked_add(element_size).ok_or_else(|| {
                        ffi_diag(wrapper, "mutable list element range overflowed", span)
                    })?;
                    let element = bytes.get(start..end).ok_or_else(|| {
                        ffi_diag(
                            wrapper,
                            "mutable list storage is shorter than its count",
                            span,
                        )
                    })?;
                    values.push(decode_bytes(
                        element,
                        inner,
                        &state.records,
                        wrapper,
                        span,
                    )?);
                }
                writebacks.push(AmbientMirExternWriteback {
                    parameter: index,
                    value: MirRuntimeValue::List(values)
                });
            }
            ParamAbi::String => {
                let current_storage = storage_index;
                storage_index += 1;
                if *access != MirAccess::Write {
                    continue;
                }
                let bytes = storage
                    .get(current_storage)
                    .map(FfiStorage::as_bytes)
                    .ok_or_else(|| ffi_diag(wrapper, "mutable String storage is missing", span))?;
                let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
                let value = String::from_utf8(bytes[..end].to_vec())
                    .map_err(|_| ffi_diag(wrapper, "native mutable String is not UTF-8", span))?;
                writebacks.push(AmbientMirExternWriteback {
                    parameter: index,
                    value: MirRuntimeValue::String(value)
                });
            }
            ParamAbi::Int | ParamAbi::Float | ParamAbi::Bool
                if *access == MirAccess::Write =>
            {
                let current_storage = storage_index;
                storage_index += 1;
                let bytes = storage
                    .get(current_storage)
                    .map(FfiStorage::as_bytes)
                    .ok_or_else(|| ffi_diag(wrapper, "mutable scalar storage is missing", span))?;
                writebacks.push(AmbientMirExternWriteback {
                    parameter: index,
                    value: decode_bytes(bytes, ty, &state.records, wrapper, span)?
                });
            }
            ParamAbi::Int | ParamAbi::Float | ParamAbi::Bool => {}
            ParamAbi::Handle => {
                if *access == MirAccess::Write {
                    let value = args.get(index).cloned().ok_or_else(|| {
                        ffi_diag(wrapper, "mutable handle argument is missing", span)
                    })?;
                    if !matches!(value, MirRuntimeValue::Int(_)) {
                        return Err(ffi_diag(
                            wrapper,
                            "mutable handle argument is not an integer token carrier",
                            span,
                        ));
                    }
                    // A writable opaque handle lends its native pointee. The
                    // native call cannot replace the managed token; return
                    // the original carrier so ownership and identity remain
                    // unchanged while the pointee mutation stays in place.
                    writebacks.push(AmbientMirExternWriteback {
                        parameter: index,
                        value
                    });
                }
            }
            ParamAbi::Callback => {
                if *access == MirAccess::Write {
                    return Err(ffi_diag(
                        wrapper,
                        "managed callback write parameter has no checked carrier",
                        span,
                    ));
                }
            }
        }
    }
    Ok(writebacks)
}

fn call_runtime_inner(
    wrapper: &str,
    args: &[MirRuntimeValue],
    ret_f32: bool,
    span: Span,
) -> Result<AmbientMirExternResult, Diagnostic> {
    let state = FFI_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = state.as_ref() else {
        return Err(ffi_diag(wrapper, "has no prepared bridge", span));
    };
    let Some(entry) = state.by_wrapper.get(wrapper) else {
        return Err(ffi_diag(
            wrapper,
            "is not bound in the prepared bridge",
            span,
        ));
    };
    if args.len() != entry.params.len() {
        return Err(ffi_diag(
            wrapper,
            format!("argc {} != {}", args.len(), entry.params.len()),
            span,
        ));
    }
    let (slots, storage) =
        prepare_runtime_call(args, entry, &state.records, wrapper, span)?;
    let mut out = FfiSlot {
        value: 0,
        ptr: std::ptr::null_mut(),
        len: 0
    };
    call_uniform(state, wrapper, entry, &slots, &mut out, span)?;
    let value = decode_runtime_result(state, entry, &out, wrapper, span, ret_f32)?;
    let writebacks =
        decode_runtime_writebacks(state, entry, args, &slots, &storage, wrapper, span)?;
    Ok(AmbientMirExternResult { value, writebacks })
}
fn managed_callback_type(ty: &MirType) -> bool {
    let MirTypeKind::Fn(signature) = ty.kind() else {
        return false;
    };
    signature.params.len() == 1
        && signature.ret.is_none()
        && matches!(
            signature.params[0].kind(),
            MirTypeKind::Apply { name, args }
                if name.name == "FfiCallbackEvent"
                    && args.len() == 1
                    && matches!(args[0].kind(), MirTypeKind::Int)
        )
}

fn mir_param_abi(
    ty: &MirType,
    is_handle: bool,
    is_record: bool,
    is_scalar_enum: bool,
) -> Option<ParamAbi> {
    if managed_callback_type(ty) {
        return Some(ParamAbi::Callback);
    }
    if is_record {
        return Some(ParamAbi::Record);
    }
    if is_scalar_enum {
        return Some(ParamAbi::Int);
    }
    if is_handle && matches!(ty.kind(), MirTypeKind::Apply { .. }) {
        return Some(ParamAbi::Handle);
    }
    if ty.is_string() {
        return Some(ParamAbi::String);
    }
    match ty.kind() {
        MirTypeKind::List(_) => Some(ParamAbi::List),
        _ => match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer) => Some(ParamAbi::Int),
            MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => Some(ParamAbi::Float),
            MirAbi::Scalar(MirScalarKind::Bool) => Some(ParamAbi::Bool),
            MirAbi::Scalar(MirScalarKind::Char)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic
            | MirAbi::Never => None
        }
    }
}

fn mir_ret_abi(
    ty: Option<&MirType>,
    is_handle: bool,
    is_record: bool,
    is_scalar_enum: bool,
) -> Option<RetAbi> {
    let Some(ty) = ty else {
        return Some(RetAbi::Unit);
    };
    if is_record {
        return Some(RetAbi::Record);
    }
    if is_scalar_enum {
        return Some(RetAbi::Int);
    }
    if is_handle && matches!(ty.kind(), MirTypeKind::Apply { .. }) {
        return Some(RetAbi::Handle);
    }
    match ty.kind() {
        MirTypeKind::List(_) => Some(RetAbi::List),
        _ => match ty.layout.abi {
            MirAbi::Scalar(MirScalarKind::Int | MirScalarKind::Pointer) => Some(RetAbi::Int),
            MirAbi::Scalar(MirScalarKind::Float | MirScalarKind::Float32) => Some(RetAbi::Float),
            MirAbi::Scalar(MirScalarKind::Bool) => Some(RetAbi::Bool),
            MirAbi::Scalar(MirScalarKind::Char)
            | MirAbi::Aggregate
            | MirAbi::Sequence
            | MirAbi::Function
            | MirAbi::Nominal
            | MirAbi::Dynamic
            | MirAbi::Never => None
        }
    }
}

fn validate_mir_bridge(foreign: &MirForeign, span: Span) -> Result<bool, Diagnostic> {
    let wrapper = bridge_wrapper_name(foreign);
    if !foreign.target_applicability.interpreter {
        return Err(ffi_diag(
            &wrapper,
            "is not applicable to the interpreter target",
            span,
        ));
    }
    if foreign.path.is_empty() || foreign.symbol.is_empty() {
        return Err(ffi_diag(
            &wrapper,
            "has no checked bridge identity",
            span,
        ));
    }
    if !matches!(
        foreign.foreign_language,
        MirForeignLanguage::C | MirForeignLanguage::Cpp | MirForeignLanguage::Assembly
    ) || !matches!(
        foreign.foreign_abi,
        MirForeignAbi::C | MirForeignAbi::CUnwind
    ) {
        return Err(ffi_diag(
            &wrapper,
            "has no supported native bridge ABI",
            span,
        ));
    }
    let state = FFI_STATE.lock().unwrap_or_else(|error| error.into_inner());
    let Some(state) = state.as_ref() else {
        return Err(ffi_diag(&wrapper, "has no prepared bridge", span));
    };
    let Some(entry) = state.by_wrapper.get(&wrapper) else {
        return Err(ffi_diag(
            &wrapper,
            "is not bound in the prepared bridge",
            span,
        ));
    };
    let expected_params = foreign
        .params
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            mir_param_abi(
                &parameter.ty,
                entry
                    .param_handles
                    .get(index)
                    .copied()
                    .flatten()
                    .is_some(),
                !foreign.bridge_eligible && mir_record_type(&parameter.ty, &state.records),
                mir_scalar_enum_type(&parameter.ty, &state.records),
            )
            .ok_or_else(|| {
                ffi_diag(
                    &wrapper,
                    format!("parameter `{}` has no supported bridge carrier", parameter.name),
                    span,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected_ret = mir_ret_abi(
        foreign.return_type.as_ref(),
        entry.handle.is_some()
            && foreign
                .return_type
                .as_ref()
                .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Apply { .. })),
        !foreign.bridge_eligible
            && foreign
                .return_type
                .as_ref()
                .is_some_and(|ty| mir_record_type(ty, &state.records)),
        foreign
            .return_type
            .as_ref()
            .is_some_and(|ty| mir_scalar_enum_type(ty, &state.records)),
    )
    .ok_or_else(|| {
        ffi_diag(
            &wrapper,
            "return type has no supported bridge carrier",
            span,
        )
    })?;
    let expected_access = foreign
        .params
        .iter()
        .map(|parameter| parameter.access)
        .collect::<Vec<_>>();
    let same_types = entry
        .param_types
        .iter()
        .zip(foreign.params.iter().map(|parameter| &parameter.ty))
        .all(|(left, right)| left.same_checked_type(right));
    let same_ret_type = match (entry.ret_type.as_ref(), foreign.return_type.as_ref()) {
        (None, None) => true,
        (Some(left), Some(right)) => left.same_checked_type(right),
        _ => false
    };
    if entry.params != expected_params
        || entry.param_access != expected_access
        || !same_types
        || entry.ret != expected_ret
        || !same_ret_type
        || entry.param_handles.len() != foreign.params.len()
    {
        return Err(ffi_diag(
            &wrapper,
            "does not match the checked bridge signature",
            span,
        ));
    }
    Ok(foreign
        .return_type
        .as_ref()
        .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Float32)))
}

/// Ambient provider for canonical MIR foreign calls made by the interpreter:
/// call the exact typed bridge selected by the canonical MIR foreign row.
pub(crate) fn ambient_mir_extern_call(
    foreign: &MirForeign,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<AmbientMirExternResult, Diagnostic>> {
    Some((|| {
        let wrapper = bridge_wrapper_name(foreign);
        let ret_f32 = validate_mir_bridge(foreign, span)?;
        if args.len() != foreign.params.len() {
            return Err(ffi_diag(
                &wrapper,
                format!("argc {} != {}", args.len(), foreign.params.len()),
                span,
            ));
        }
        call_runtime_inner(&wrapper, &args, ret_f32, span)
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    unsafe extern "C" fn test_clear_panic_hook() {}

    unsafe extern "C" fn test_take_failure() -> i8 {
        0
    }

    fn test_state() -> FfiState {
        FfiState {
            handle: std::ptr::null_mut(),
            clear_panic_hook_fn: test_clear_panic_hook,
            free_fn: None,
            take_failure_fn: test_take_failure,
            artifact_identity: String::new(),
            by_wrapper: HashMap::new(),
            records: HashMap::new(),
            arrow_provider_lease: None
        }
    }

    fn test_entry() -> FfiEntry {
        let int = MirType::from_kind(MirTypeKind::Int);
        let list = MirType::from_kind(MirTypeKind::List(Box::new(int.clone())));
        FfiEntry {
            params: vec![
                ParamAbi::Int,
                ParamAbi::Int,
                ParamAbi::List,
                ParamAbi::String,
                ParamAbi::Handle,
            ],
            param_handles: vec![None; 5],
            param_types: vec![
                int.clone(),
                int.clone(),
                list,
                MirType::from_kind(MirTypeKind::String),
                int,
            ],
            param_access: vec![
                MirAccess::Read,
                MirAccess::Write,
                MirAccess::Write,
                MirAccess::Write,
                MirAccess::Write,
            ],
            ret: RetAbi::Unit,
            ret_type: None,
            handle: None,
            close_handle: None,
            ptr: std::ptr::null()
        }
    }

    unsafe extern "C" fn test_uniform_writeback(
        args: *const FfiSlot,
        argc: usize,
        out: *mut FfiSlot,
    ) -> i32 {
        let args = unsafe { std::slice::from_raw_parts(args, argc) };
        unsafe {
            (args[1].ptr as *mut i64).write(23);
            for (index, value) in [4i64, 5].iter().copied().enumerate() {
                (args[2].ptr as *mut i64).add(index).write(value);
            }
            std::ptr::copy_nonoverlapping(b"new\0".as_ptr(), args[3].ptr, 4);
            (*out).value = 0;
            (*out).ptr = std::ptr::null_mut();
            (*out).len = 0;
        }
        0
    }

    #[test]
    fn native_uniform_abi_invocation_returns_indexed_writebacks() {
        let mut state = test_state();
        let mut entry = test_entry();
        entry.ptr = test_uniform_writeback as *const ();
        state
            .by_wrapper
            .insert("test_generated_out".to_string(), entry);
        *FFI_STATE.lock().unwrap_or_else(|error| error.into_inner()) = Some(state);
        let result = call_runtime_inner(
            "test_generated_out",
            &[
                MirRuntimeValue::Int(3),
                MirRuntimeValue::Int(7),
                MirRuntimeValue::List(vec![
                    MirRuntimeValue::Int(1),
                    MirRuntimeValue::Int(2),
                ]),
                MirRuntimeValue::String("old".to_string()),
                MirRuntimeValue::Int(42),
            ],
            false,
            Span::new(0, 0),
        );
        *FFI_STATE.lock().unwrap_or_else(|error| error.into_inner()) = None;
        let result = result.expect("uniform native ABI fixture must return");
        assert_eq!(
            result.writebacks,
            vec![
                AmbientMirExternWriteback {
                    parameter: 1,
                    value: MirRuntimeValue::Int(23)
                },
                AmbientMirExternWriteback {
                    parameter: 2,
                    value: MirRuntimeValue::List(vec![
                        MirRuntimeValue::Int(4),
                        MirRuntimeValue::Int(5),
                    ])
                },
                AmbientMirExternWriteback {
                    parameter: 3,
                    value: MirRuntimeValue::String("new".to_string())
                },
                AmbientMirExternWriteback {
                    parameter: 4,
                    value: MirRuntimeValue::Int(42)
                },
            ]
        );
    }

    #[test]
    fn native_writeback_decode_covers_scalar_list_string_and_indexed_handle_carriers() {
        let state = test_state();
        let entry = test_entry();
        {
            let handle_arg = 42;
            let args = vec![
                MirRuntimeValue::Int(3),
                MirRuntimeValue::Int(7),
                MirRuntimeValue::List(vec![
                    MirRuntimeValue::Int(1),
                    MirRuntimeValue::Int(2),
                ]),
                MirRuntimeValue::String("old".to_string()),
                MirRuntimeValue::Int(handle_arg),
            ];
            let (slots, mut storage) = prepare_runtime_call(
                &args,
                &entry,
                &state.records,
                "test_writeback",
                Span::new(0, 0),
            )
            .expect("test arguments have checked carriers");
            assert_eq!(storage.len(), 3);
            storage[0].as_bytes_mut()[..8].copy_from_slice(&23i64.to_ne_bytes());

            for (index, value) in [4i64, 5].iter().copied().enumerate() {
                let start = index * 8;
                storage[1].as_bytes_mut()[start..start + 8]
                    .copy_from_slice(&value.to_ne_bytes());
            }
            storage[2].as_bytes_mut().copy_from_slice(b"new\0");

            let writebacks = decode_runtime_writebacks(
                &state,
                &entry,
                &args,
                &slots,
                &storage,
                "test_writeback",
                Span::new(0, 0),
            )
            .expect("native storage mutations decode losslessly");
            assert_eq!(
                writebacks,
                vec![
                    AmbientMirExternWriteback {
                        parameter: 1,
                        value: MirRuntimeValue::Int(23)
                    },
                    AmbientMirExternWriteback {
                        parameter: 2,
                        value: MirRuntimeValue::List(vec![
                            MirRuntimeValue::Int(4),
                            MirRuntimeValue::Int(5),
                        ])
                    },
                    AmbientMirExternWriteback {
                        parameter: 3,
                        value: MirRuntimeValue::String("new".to_string())
                    },
                    AmbientMirExternWriteback {
                        parameter: 4,
                        value: MirRuntimeValue::Int(handle_arg)
                    },
                ]
            );
        }
    }
}

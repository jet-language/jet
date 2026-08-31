#[cfg(target_os = "windows")]
mod jet_process_windows_sandbox {
    //! Native Windows AppContainer backend for build actions.
    //!
    //! The child is created with a default-deny AppContainer token, only the
    //! declared capability set, a per-run ACL projection, and a Job Object. It is
    //! created suspended, joined to the Job Object, and resumed only after every
    //! boundary is ready. Any preparation failure is returned before the build
    //! tool can execute.

    use std::collections::{BTreeMap, HashSet};
    use std::ffi::c_void;
    use std::fs::{self, File};
    use std::io::{self, Read};
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::os::windows::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::{ExitStatus, Output};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::ReadOnlyMount;

    const MECHANISM: &str = "windows-appcontainer";
    const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
    const PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES: usize = 0x0002_0009;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
    const INFINITE: u32 = 0xffff_ffff;
    const WAIT_FAILED: u32 = 0xffff_ffff;
    const INVALID_THREAD_RESUME: u32 = 0xffff_ffff;
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const GENERIC_EXECUTE: u32 = 0x2000_0000;
    const DELETE: u32 = 0x0001_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const WRITE_OWNER: u32 = 0x0008_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9;
    const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
    const JOB_OBJECT_LIMIT_PROCESS_MEMORY: u32 = 0x0000_0100;
    const JOB_OBJECT_LIMIT_JOB_MEMORY: u32 = 0x0000_0200;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const ACTIVE_PROCESS_LIMIT: u32 = 256;
    const MEMORY_LIMIT: usize = 2 * 1024 * 1024 * 1024;
    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const SET_ACCESS: u32 = 2;
    const DENY_ACCESS: u32 = 3;
    const REVOKE_ACCESS: u32 = 4;
    const NO_MULTIPLE_TRUSTEE: u32 = 0;
    const TRUSTEE_IS_SID: u32 = 0;
    const TRUSTEE_IS_UNKNOWN: u32 = 0;
    const SUB_CONTAINERS_AND_OBJECTS_INHERIT: u32 = 0x0000_0003;
    const SE_GROUP_ENABLED: u32 = 0x0000_0004;
    const PROFILE_ALREADY_EXISTS: i32 = 0x8007_00b7_u32 as i32;

    type Handle = *mut c_void;
    type Sid = *mut c_void;

    fn null_handle() -> Handle {
        std::ptr::null_mut()
    }

    fn invalid_handle(handle: Handle) -> bool {
        handle.is_null() || handle == (-1isize as Handle)
    }

    #[repr(C)]
    struct SecurityAttributes {
        n_length: u32,
        lp_security_descriptor: *mut c_void,
        b_inherit_handle: i32,
    }

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        lp_reserved: *mut u16,
        lp_desktop: *mut u16,
        lp_title: *mut u16,
        dw_x: u32,
        dw_y: u32,
        dw_x_size: u32,
        dw_y_size: u32,
        dw_x_count_chars: u32,
        dw_y_count_chars: u32,
        dw_fill_attribute: u32,
        dw_flags: u32,
        w_show_window: u16,
        cb_reserved2: u16,
        lp_reserved2: *mut u8,
        h_std_input: Handle,
        h_std_output: Handle,
        h_std_error: Handle,
    }

    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        attribute_list: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        h_process: Handle,
        h_thread: Handle,
        dw_process_id: u32,
        dw_thread_id: u32,
    }

    #[repr(C)]
    struct SidAndAttributes {
        sid: Sid,
        attributes: u32,
    }

    #[repr(C)]
    struct SecurityCapabilities {
        app_container_sid: Sid,
        capabilities: *mut SidAndAttributes,
        capability_count: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct IoCounters {
        read_operations: u64,
        write_operations: u64,
        other_operations: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }

    #[repr(C)]
    struct ExtendedLimitInformation {
        basic: BasicLimitInformation,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[repr(C)]
    struct TrusteeW {
        multiple_trustee: *mut TrusteeW,
        multiple_trustee_operation: u32,
        trustee_form: u32,
        trustee_type: u32,
        name: *mut u16,
    }

    #[repr(C)]
    struct ExplicitAccessW {
        access_permissions: u32,
        access_mode: u32,
        inheritance: u32,
        trustee: TrusteeW,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn CreateFileW(
            name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut SecurityAttributes,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: Handle,
        ) -> Handle;
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn CreatePipe(
            read_pipe: *mut Handle,
            write_pipe: *mut Handle,
            attributes: *mut SecurityAttributes,
            size: u32,
        ) -> i32;
        fn CreateProcessW(
            application_name: *const u16,
            command_line: *mut u16,
            process_attributes: *mut c_void,
            thread_attributes: *mut c_void,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *mut c_void,
            current_directory: *const u16,
            startup_info: *mut StartupInfoW,
            process_information: *mut ProcessInformation,
        ) -> i32;
        fn DeleteProcThreadAttributeList(attribute_list: *mut c_void);
        fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> i32;
        fn InitializeProcThreadAttributeList(
            attribute_list: *mut c_void,
            attribute_count: u32,
            flags: u32,
            size: *mut usize,
        ) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
        fn ResumeThread(thread: Handle) -> u32;
        fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> i32;
        fn SetInformationJobObject(
            job: Handle,
            information_class: u32,
            information: *mut c_void,
            length: u32,
        ) -> i32;
        fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
        fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
        fn UpdateProcThreadAttribute(
            attribute_list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *const c_void,
            size: usize,
            previous_value: *mut c_void,
            return_size: *mut usize,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSidToSidW(string_sid: *const u16, sid: *mut Sid) -> i32;
        fn FreeSid(sid: Sid) -> Sid;
        fn GetNamedSecurityInfoW(
            object_name: *const u16,
            object_type: u32,
            security_info: u32,
            owner: *mut Sid,
            group: *mut Sid,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            security_descriptor: *mut *mut c_void,
        ) -> u32;
        fn SetEntriesInAclW(
            count: u32,
            entries: *const ExplicitAccessW,
            old_acl: *mut c_void,
            new_acl: *mut *mut c_void,
        ) -> u32;
        fn SetNamedSecurityInfoW(
            object_name: *mut u16,
            object_type: u32,
            security_info: u32,
            owner: Sid,
            group: Sid,
            dacl: *mut c_void,
            sacl: *mut c_void,
        ) -> u32;
    }

    #[link(name = "userenv")]
    unsafe extern "system" {
        fn CreateAppContainerProfile(
            profile_name: *const u16,
            display_name: *const u16,
            description: *const u16,
            capabilities: *mut SidAndAttributes,
            capability_count: u32,
            app_container_sid: *mut Sid,
        ) -> i32;
        fn DeriveAppContainerSidFromAppContainerName(
            profile_name: *const u16,
            app_container_sid: *mut Sid,
        ) -> i32;
        fn DeleteAppContainerProfile(profile_name: *const u16) -> i32;
    }

    static PROFILE_COUNTER: AtomicU64 = AtomicU64::new(1);

    pub(crate) struct BackendStatus {
        pub available: bool,
        pub mechanism: String,
        pub policy: String,
        pub reason: String,
    }

    pub(crate) struct BackendOutput {
        pub output: Output,
        pub mechanism: String,
        pub policy: String,
        pub timed_out: bool,
        pub pid: i64,
    }

    #[derive(Debug)]
    pub(crate) enum WindowsSandboxError {
        Unsupported(String),
        Io(String),
    }

    fn wide(value: &str) -> io::Result<Vec<u16>> {
        if value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows sandbox path or argument contains NUL",
            ));
        }
        Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
    }

    fn hresult_error(name: &str, status: i32) -> io::Error {
        io::Error::new(
            io::ErrorKind::Other,
            format!("{name} failed with HRESULT 0x{:08x}", status as u32),
        )
    }

    struct HandleGuard(Handle);

    impl HandleGuard {
        fn new(handle: Handle) -> io::Result<Self> {
            if invalid_handle(handle) {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(handle))
            }
        }

        fn raw(&self) -> Handle {
            self.0
        }

        fn into_raw(mut self) -> RawHandle {
            let handle = self.0;
            self.0 = null_handle();
            handle as RawHandle
        }
    }

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !invalid_handle(self.0) {
                // SAFETY: the guard owns this live Win32 handle and closes it once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct AppContainer {
        name: Vec<u16>,
        sid: Sid,
    }

    impl AppContainer {
        fn create(capability: Option<&mut SidAndAttributes>) -> io::Result<Self> {
            let id = PROFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = wide(&format!("JetpackBuild-{}-{id}", std::process::id()))?;
            let display = wide("Jetpack package")?;
            let description = wide("One-shot Jetpack package isolation")?;
            let mut sid = null_handle();
            let (capabilities, capability_count) = capability
                .map_or((std::ptr::null_mut(), 0), |value| {
                    (value as *mut SidAndAttributes, 1)
                });
            // SAFETY: all strings and the optional capability live through the
            // call; Windows allocates the returned profile SID.
            let status = unsafe {
                CreateAppContainerProfile(
                    name.as_ptr(),
                    display.as_ptr(),
                    description.as_ptr(),
                    capabilities,
                    capability_count,
                    &mut sid,
                )
            };
            if status == PROFILE_ALREADY_EXISTS {
                // A killed build can leave its profile behind. Reuse its SID by
                // name instead of treating that recoverable state as "sandbox
                // unavailable".
                let derive_status =
                    unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
                if derive_status != 0 {
                    return Err(hresult_error(
                        "DeriveAppContainerSidFromAppContainerName",
                        derive_status,
                    ));
                }
            } else if status != 0 {
                return Err(hresult_error("CreateAppContainerProfile", status));
            }
            if sid.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "CreateAppContainerProfile returned no AppContainer SID",
                ));
            }
            Ok(Self { name, sid })
        }
    }

    impl Drop for AppContainer {
        fn drop(&mut self) {
            // SAFETY: the profile and SID were created by this guard and are
            // released after ACLs and child handles have been dropped.
            unsafe {
                DeleteAppContainerProfile(self.name.as_ptr());
                FreeSid(self.sid);
            }
        }
    }

    struct CapabilitySid(Sid);

    impl CapabilitySid {
        fn internet_client() -> io::Result<Self> {
            let text = wide("S-1-15-3-1")?;
            let mut sid = null_handle();
            // SAFETY: Windows allocates the SID and writes only the output slot.
            if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(sid))
        }
    }

    impl Drop for CapabilitySid {
        fn drop(&mut self) {
            // SAFETY: ConvertStringSidToSidW allocated this SID with LocalAlloc.
            unsafe { LocalFree(self.0) };
        }
    }

    struct AttributeList {
        list: *mut c_void,
        storage: Vec<u8>,
    }

    impl AttributeList {
        fn create() -> io::Result<Self> {
            let mut size = 0;
            // SAFETY: this is the documented size probe.
            unsafe {
                InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut size);
            }
            if size == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut storage = vec![0_u8; size];
            let list = storage.as_mut_ptr().cast::<c_void>();
            // SAFETY: storage is the exact size requested by Windows and remains
            // live while the process is created.
            if unsafe { InitializeProcThreadAttributeList(list, 2, 0, &mut size) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { list, storage })
        }

        fn update(
            &mut self,
            attribute: usize,
            value: *const c_void,
            size: usize,
        ) -> io::Result<()> {
            // SAFETY: `value` points to a live, correctly sized value for this
            // call; Windows copies the attribute metadata into the list.
            if unsafe {
                UpdateProcThreadAttribute(
                    self.list,
                    0,
                    attribute,
                    value,
                    size,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for AttributeList {
        fn drop(&mut self) {
            // SAFETY: the list was initialized successfully and storage remains
            // live until this Drop call returns.
            unsafe { DeleteProcThreadAttributeList(self.list) };
            let _ = &self.storage;
        }
    }

    #[derive(Clone, Copy)]
    struct Grant {
        access: u32,
        inheritance: u32,
        mode: u32,
        sid: Sid,
    }

    struct AclProjection {
        grants: Vec<(PathBuf, Grant)>,
        sid: Sid,
    }

    impl AclProjection {
        fn create(
            sid: Sid,
            source_dir: &Path,
            output_dir: Option<&Path>,
            executable: &Path,
            source_readable: bool,
            source_writable: bool,
            mounts: &[PathBuf],
        ) -> io::Result<Self> {
            let mut projection = Self {
                grants: Vec::new(),
                sid,
            };
            let mut seen = HashSet::new();
            projection.add_parent(source_dir, &mut seen)?;
            if !source_writable {
                projection.add_with_inheritance(
                    source_dir,
                    (if source_readable { 0 } else { GENERIC_READ })
                        | GENERIC_WRITE
                        | DELETE
                        | WRITE_DAC
                        | WRITE_OWNER,
                    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                    DENY_ACCESS,
                    &mut seen,
                )?;
            }
            projection.add_with_inheritance(
                source_dir,
                if source_writable {
                    GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE
                } else if source_readable {
                    GENERIC_READ | GENERIC_EXECUTE
                } else {
                    GENERIC_EXECUTE
                },
                SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                SET_ACCESS,
                &mut seen,
            )?;
            if let Some(output_dir) = output_dir {
                projection.add_parent(output_dir, &mut seen)?;
                projection.add_with_inheritance(
                    output_dir,
                    GENERIC_READ | GENERIC_WRITE | GENERIC_EXECUTE,
                    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                    SET_ACCESS,
                    &mut seen,
                )?;
            }
            if !is_system_path(executable) {
                if let Some(parent) = executable.parent() {
                    projection.add_parent(parent, &mut seen)?;
                    projection.add_with_inheritance(
                        parent,
                        GENERIC_READ | GENERIC_EXECUTE,
                        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                        SET_ACCESS,
                        &mut seen,
                    )?;
                }
            }
            for mount in mounts {
                projection.add_parent(mount, &mut seen)?;
                projection.add_with_inheritance(
                    mount,
                    GENERIC_WRITE | DELETE | WRITE_DAC | WRITE_OWNER,
                    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                    DENY_ACCESS,
                    &mut seen,
                )?;
                projection.add_with_inheritance(
                    mount,
                    GENERIC_READ | GENERIC_EXECUTE,
                    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
                    SET_ACCESS,
                    &mut seen,
                )?;
            }
            Ok(projection)
        }

        fn add_parent(
            &mut self,
            path: &Path,
            seen: &mut HashSet<(PathBuf, u32, u32, u32)>,
        ) -> io::Result<()> {
            let user_profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
            let mut parent = path.parent();
            while let Some(current) = parent {
                if is_system_path(current) {
                    break;
                }
                self.add_with_inheritance(current, GENERIC_EXECUTE, 0, SET_ACCESS, seen)?;
                if user_profile
                    .as_deref()
                    .is_some_and(|root| same_windows_path(current, root))
                {
                    break;
                }
                let Some(next) = current.parent() else {
                    break;
                };
                if next == current || next.parent().is_none() {
                    break;
                }
                parent = Some(next);
            }
            Ok(())
        }

        fn add_with_inheritance(
            &mut self,
            path: &Path,
            access: u32,
            inheritance: u32,
            mode: u32,
            seen: &mut HashSet<(PathBuf, u32, u32, u32)>,
        ) -> io::Result<()> {
            let key = (path.to_path_buf(), access, inheritance, mode);
            if !seen.insert(key) {
                return Ok(());
            }
            let mut object_name = wide(&path.to_string_lossy())?;
            let mut old_dacl = null_handle();
            let mut security_descriptor = null_handle();
            // SAFETY: output pointers and the NUL-terminated object name are live
            // for the duration of the call. Windows allocates the descriptor.
            let status = unsafe {
                GetNamedSecurityInfoW(
                    object_name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut old_dacl,
                    std::ptr::null_mut(),
                    &mut security_descriptor,
                )
            };
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            let entry = ExplicitAccessW {
                access_permissions: access,
                access_mode: mode,
                inheritance,
                trustee: TrusteeW {
                    multiple_trustee: std::ptr::null_mut(),
                    multiple_trustee_operation: NO_MULTIPLE_TRUSTEE,
                    trustee_form: TRUSTEE_IS_SID,
                    trustee_type: TRUSTEE_IS_UNKNOWN,
                    name: self.sid.cast(),
                },
            };
            let mut new_acl = null_handle();
            // SAFETY: the old ACL came from Windows and the trustee SID remains
            // live for the call.
            let status = unsafe { SetEntriesInAclW(1, &entry, old_dacl, &mut new_acl) };
            if status != 0 {
                unsafe { LocalFree(security_descriptor) };
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            // SAFETY: Windows consumes the merged ACL for this named object.
            let status = unsafe {
                SetNamedSecurityInfoW(
                    object_name.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_handle(),
                    null_handle(),
                    new_acl,
                    null_handle(),
                )
            };
            unsafe {
                LocalFree(new_acl);
                LocalFree(security_descriptor);
            }
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            self.grants.push((
                path.to_path_buf(),
                Grant {
                    access,
                    inheritance,
                    mode,
                    sid: self.sid,
                },
            ));
            Ok(())
        }
    }

    impl Drop for AclProjection {
        fn drop(&mut self) {
            for (path, grant) in self.grants.iter().rev() {
                let Ok(mut object_name) = wide(&path.to_string_lossy()) else {
                    continue;
                };
                let mut old_dacl = null_handle();
                let mut security_descriptor = null_handle();
                // SAFETY: this cleanup only targets the unique SID inserted by
                // this guard. A failed cleanup cannot widen the child boundary.
                if unsafe {
                    GetNamedSecurityInfoW(
                        object_name.as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut old_dacl,
                        std::ptr::null_mut(),
                        &mut security_descriptor,
                    )
                } != 0
                {
                    continue;
                }
                let entry = ExplicitAccessW {
                    access_permissions: grant.access,
                    access_mode: REVOKE_ACCESS,
                    inheritance: grant.inheritance,
                    trustee: TrusteeW {
                        multiple_trustee: std::ptr::null_mut(),
                        multiple_trustee_operation: NO_MULTIPLE_TRUSTEE,
                        trustee_form: TRUSTEE_IS_SID,
                        trustee_type: TRUSTEE_IS_UNKNOWN,
                        name: grant.sid.cast(),
                    },
                };
                let mut new_acl = null_handle();
                if unsafe { SetEntriesInAclW(1, &entry, old_dacl, &mut new_acl) } == 0 {
                    unsafe {
                        SetNamedSecurityInfoW(
                            object_name.as_mut_ptr(),
                            SE_FILE_OBJECT,
                            DACL_SECURITY_INFORMATION,
                            null_handle(),
                            null_handle(),
                            new_acl,
                            null_handle(),
                        );
                        LocalFree(new_acl);
                    }
                }
                unsafe { LocalFree(security_descriptor) };
            }
        }
    }

    struct Job(HandleGuard);

    impl Job {
        fn create() -> io::Result<Self> {
            // SAFETY: null attributes/name create one private unnamed Job Object.
            let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
            let guard = HandleGuard::new(handle)?;
            let mut limits = ExtendedLimitInformation {
                basic: BasicLimitInformation {
                    per_process_user_time_limit: 0,
                    per_job_user_time_limit: 0,
                    limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                        | JOB_OBJECT_LIMIT_JOB_MEMORY,
                    minimum_working_set_size: 0,
                    maximum_working_set_size: 0,
                    active_process_limit: ACTIVE_PROCESS_LIMIT,
                    affinity: 0,
                    priority_class: 0,
                    scheduling_class: 0,
                },
                io: IoCounters {
                    read_operations: 0,
                    write_operations: 0,
                    other_operations: 0,
                    read_bytes: 0,
                    write_bytes: 0,
                    other_bytes: 0,
                },
                process_memory_limit: MEMORY_LIMIT,
                job_memory_limit: MEMORY_LIMIT,
                peak_process_memory_used: 0,
                peak_job_memory_used: 0,
            };
            // SAFETY: the structure and byte length match the documented Job
            // Object information class and remain live through the call.
            if unsafe {
                SetInformationJobObject(
                    guard.raw(),
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    (&mut limits as *mut ExtendedLimitInformation).cast(),
                    std::mem::size_of::<ExtendedLimitInformation>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(guard))
        }

        fn assign(&self, process: Handle) -> io::Result<()> {
            // SAFETY: both handles are live and owned by their guards.
            if unsafe { AssignProcessToJobObject(self.0.raw(), process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn terminate(&self) -> io::Result<()> {
            // SAFETY: this job handle is live and owns the complete child tree.
            if unsafe { TerminateJobObject(self.0.raw(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn create_pipe() -> io::Result<(HandleGuard, HandleGuard)> {
        let mut read_pipe = null_handle();
        let mut write_pipe = null_handle();
        let mut attributes = SecurityAttributes {
            n_length: std::mem::size_of::<SecurityAttributes>() as u32,
            lp_security_descriptor: std::ptr::null_mut(),
            b_inherit_handle: 1,
        };
        // SAFETY: output handles and security attributes are valid for this call.
        if unsafe { CreatePipe(&mut read_pipe, &mut write_pipe, &mut attributes, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let read_guard = HandleGuard::new(read_pipe)?;
        let write_guard = HandleGuard::new(write_pipe)?;
        // SAFETY: only the child write side belongs in the inherited handle list.
        if unsafe { SetHandleInformation(read_guard.raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((read_guard, write_guard))
    }

    fn create_stdin() -> io::Result<HandleGuard> {
        let name = wide("NUL")?;
        let mut attributes = SecurityAttributes {
            n_length: std::mem::size_of::<SecurityAttributes>() as u32,
            lp_security_descriptor: std::ptr::null_mut(),
            b_inherit_handle: 1,
        };
        // SAFETY: NUL is a system device and the returned inheritable handle is
        // owned by the guard.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &mut attributes,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_handle(),
            )
        };
        HandleGuard::new(handle)
    }

    fn quote_arg(value: &str) -> io::Result<Vec<u16>> {
        if value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows sandbox argument contains NUL",
            ));
        }
        let needs_quotes =
            value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('"');
        let mut text = String::new();
        if needs_quotes {
            text.push('"');
        }
        let mut slashes = 0;
        for character in value.chars() {
            if character == '\\' {
                slashes += 1;
                continue;
            }
            if character == '"' {
                text.extend(std::iter::repeat('\\').take(slashes * 2 + 1));
                text.push(character);
            } else {
                text.extend(std::iter::repeat('\\').take(slashes));
                text.push(character);
            }
            slashes = 0;
        }
        if needs_quotes {
            text.extend(std::iter::repeat('\\').take(slashes * 2));
            text.push('"');
        } else {
            text.extend(std::iter::repeat('\\').take(slashes));
        }
        wide(&text)
    }

    fn command_line(executable: &Path, args: &[String]) -> io::Result<Vec<u16>> {
        let mut line = quote_arg(&executable.to_string_lossy())?;
        line.pop();
        for arg in args {
            line.push(' ' as u16);
            let mut quoted = quote_arg(arg)?;
            quoted.pop();
            line.append(&mut quoted);
        }
        line.push(0);
        Ok(line)
    }

    fn environment_block(
        output_dir: Option<&Path>,
        env: &BTreeMap<String, String>,
    ) -> io::Result<Vec<u16>> {
        let mut values = env.clone();
        if let Some(output_dir) = output_dir {
            let output = output_dir.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Windows sandbox output path is not valid UTF-16 text",
                )
            })?;
            values.insert("JET_BUILD_OUTPUT".to_string(), output.to_string());
        }
        let mut block = Vec::new();
        for (name, value) in values {
            block.extend(wide(&format!("{name}={value}"))?);
        }
        if block.is_empty() {
            block.extend([0, 0]);
        } else {
            block.push(0);
        }
        Ok(block)
    }

    fn read_pipe(
        handle: RawHandle,
        limit: Option<usize>,
        budget: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
        limit_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> thread::JoinHandle<io::Result<(Vec<u8>, bool)>> {
        let handle = handle as usize;
        thread::spawn(move || {
            // SAFETY: ownership of the read handle moves into this File exactly
            // once after the parent closes its copy.
            let mut file = unsafe { File::from_raw_handle(handle as RawHandle) };
            let mut bytes = Vec::new();
            let mut exceeded = false;
            let mut chunk = [0_u8; 8192];
            loop {
                let count = file.read(&mut chunk)?;
                if count == 0 {
                    break;
                }
                let kept = match limit {
                    None => {
                        bytes.extend_from_slice(&chunk[..count]);
                        count
                    }
                    Some(limit) => {
                        let budget = budget
                            .as_ref()
                            .expect("bounded Windows sandbox output needs a shared budget");
                        let mut used = budget.load(std::sync::atomic::Ordering::Acquire);
                        loop {
                            let available = limit.saturating_sub(used);
                            let kept = available.min(count);
                            match budget.compare_exchange(
                                used,
                                used + kept,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            ) {
                                Ok(_) => {
                                    bytes.extend_from_slice(&chunk[..kept]);
                                    break kept;
                                }
                                Err(next) => used = next,
                            }
                        }
                    }
                };
                if kept < count {
                    exceeded = true;
                    limit_hit.store(true, std::sync::atomic::Ordering::Release);
                    // Stop draining once the shared budget is exhausted. The
                    // parent observes `limit_hit`, terminates the Job Object,
                    // and joins this reader; continuing here would let an
                    // untrusted child turn an output-limit failure into an
                    // unbounded parent allocation.
                    break;
                }
            }
            Ok((bytes, exceeded))
        })
    }

    fn is_system_path(path: &Path) -> bool {
        let normalized = normalized_windows_path(path);
        let root = std::env::var("WINDIR")
            .unwrap_or_else(|_| String::from(r"C:\Windows"))
            .replace('/', "\\")
            .to_ascii_lowercase();
        normalized == root
            || normalized
                .strip_prefix(&root)
                .is_some_and(|rest| rest.starts_with('\\'))
    }

    fn normalized_windows_path(path: &Path) -> String {
        let normalized = path
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        normalized
            .strip_prefix("\\\\?\\")
            .or_else(|| normalized.strip_prefix("\\\\.\\"))
            .unwrap_or(&normalized)
            .to_string()
    }

    fn same_windows_path(left: &Path, right: &Path) -> bool {
        normalized_windows_path(left) == normalized_windows_path(right)
    }

    fn backend_policy(
        output_dir: Option<&Path>,
        share_network: bool,
        source_readable: bool,
        source_writable: bool,
    ) -> String {
        format!(
        "filesystem={};process=appcontainer+job-kill-on-close+active-process=256;network={};environment=clear+declared;devices=appcontainer-default-deny;resources=memory-2GiB+active-process-256",
        match (source_readable, source_writable, output_dir.is_some()) {
            (true, true, false) => "private-workspace-readwrite",
            (true, false, true) => "source-readonly,output-readwrite",
            (true, false, false) => "source-readonly",
            (false, false, _) => "filesystem-none",
            (false, true, _) => "filesystem-write-only",
        },
        if share_network {
            "declared-internet-client"
        } else {
            "denied"
        },
    )
    }

    pub(crate) fn status() -> BackendStatus {
        if std::env::var_os("JETPACK_FAKE_SANDBOX").is_some() {
            return BackendStatus {
                available: false,
                mechanism: format!("{MECHANISM}-unavailable"),
                policy: "not-enforced".to_string(),
                reason: "test sandbox override prevents the native Windows backend probe"
                    .to_string(),
            };
        }
        let root = std::env::temp_dir().join(format!(
            ".jetpack-sandbox-probe-{}-{}",
            std::process::id(),
            PROFILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        let output = root.join("output");
        if let Err(error) = fs::create_dir_all(&source).and_then(|_| fs::create_dir_all(&output)) {
            let _ = fs::remove_dir_all(&root);
            return BackendStatus {
                available: false,
                mechanism: format!("{MECHANISM}-unavailable"),
                policy: "not-enforced".to_string(),
                reason: format!(
                    "native Windows sandbox probe workspace could not be created: {error}"
                ),
            };
        }
        let executable = PathBuf::from(
            std::env::var_os("WINDIR").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
        )
        .join("System32")
        .join("cmd.exe");
        let args = [String::from("/C"), String::from("exit"), String::from("0")];
        let probe = run(
            &executable,
            &args,
            &source,
            Some(&output),
            &BTreeMap::new(),
            false,
            true,
            false,
            None,
            None,
        );
        let _ = fs::remove_dir_all(&root);
        match probe {
        Ok(result) if result.output.status.success() => BackendStatus {
            available: true,
            mechanism: result.mechanism,
            policy: result.policy,
            reason: "native AppContainer launch, ACL projection, inherited-handle list, and Job Object probe succeeded".to_string(),
        },
        Ok(result) => BackendStatus {
            available: false,
            mechanism: format!("{MECHANISM}-unavailable"),
            policy: "not-enforced".to_string(),
            reason: format!(
                "native Windows sandbox probe exited with {}",
                result.output.status
            ),
        },
        Err(WindowsSandboxError::Unsupported(detail))
        | Err(WindowsSandboxError::Io(detail)) => BackendStatus {
            available: false,
            mechanism: format!("{MECHANISM}-unavailable"),
            policy: "not-enforced".to_string(),
            reason: format!("native Windows sandbox probe failed: {detail}"),
        },
    }
    }

    pub(crate) fn run(
        executable: &Path,
        args: &[String],
        source_dir: &Path,
        output_dir: Option<&Path>,
        env: &BTreeMap<String, String>,
        share_network: bool,
        source_readable: bool,
        source_writable: bool,
        timeout_ms: Option<i64>,
        output_limit: Option<i64>,
    ) -> Result<BackendOutput, WindowsSandboxError> {
        run_with_read_only_mounts(
            executable,
            args,
            source_dir,
            output_dir,
            env,
            share_network,
            source_readable,
            source_writable,
            &[],
            timeout_ms,
            output_limit,
        )
    }

    pub(crate) fn run_with_read_only_mounts(
        executable: &Path,
        args: &[String],
        source_dir: &Path,
        output_dir: Option<&Path>,
        env: &BTreeMap<String, String>,
        share_network: bool,
        source_readable: bool,
        source_writable: bool,
        mounts: &[ReadOnlyMount],
        timeout_ms: Option<i64>,
        output_limit: Option<i64>,
    ) -> Result<BackendOutput, WindowsSandboxError> {
        if std::env::var_os("JETPACK_FAKE_SANDBOX").is_some() {
            return Err(WindowsSandboxError::Unsupported(
                "test sandbox override prevents child execution".to_string(),
            ));
        }
        let executable = real_executable(executable)?;
        let source_dir = real_directory(source_dir)?;
        let output_dir = output_dir.map(real_directory).transpose()?;
        let mounts = mounts
            .iter()
            .map(|mount| {
                if !mount.destination.is_absolute()
                    || mount.destination.components().any(|component| {
                        matches!(
                            component,
                            std::path::Component::CurDir | std::path::Component::ParentDir
                        )
                    })
                {
                    return Err(WindowsSandboxError::Unsupported(format!(
                        "sandbox mount destination `{}` is not an absolute, normalized path",
                        mount.destination.display()
                    )));
                }
                real_directory(&mount.source)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let network_capability = if share_network {
            Some(CapabilitySid::internet_client().map_err(io_error)?)
        } else {
            None
        };
        let mut capability = network_capability.as_ref().map(|sid| SidAndAttributes {
            sid: sid.0,
            attributes: SE_GROUP_ENABLED,
        });
        let profile = AppContainer::create(capability.as_mut()).map_err(io_error)?;
        let _acl = AclProjection::create(
            profile.sid,
            &source_dir,
            output_dir.as_deref(),
            &executable,
            source_readable,
            source_writable,
            &mounts,
        )
        .map_err(io_error)?;
        let security = SecurityCapabilities {
            app_container_sid: profile.sid,
            capabilities: capability
                .as_mut()
                .map_or(std::ptr::null_mut(), |value| value as *mut SidAndAttributes),
            capability_count: if capability.is_some() { 1 } else { 0 },
            reserved: 0,
        };
        let job = Job::create().map_err(io_error)?;
        let stdin = create_stdin().map_err(io_error)?;
        let (stdout_read, stdout_write) = create_pipe().map_err(io_error)?;
        let (stderr_read, stderr_write) = create_pipe().map_err(io_error)?;
        let application_name = wide(&executable.to_string_lossy()).map_err(io_error)?;
        let current_directory = wide(&source_dir.to_string_lossy()).map_err(io_error)?;
        let mut command_line = command_line(&executable, args).map_err(io_error)?;
        let mut environment = environment_block(output_dir.as_deref(), env).map_err(io_error)?;
        let mut attributes = AttributeList::create().map_err(io_error)?;
        attributes
            .update(
                PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                (&security as *const SecurityCapabilities).cast(),
                std::mem::size_of::<SecurityCapabilities>(),
            )
            .map_err(io_error)?;
        let handles = [stdin.raw(), stdout_write.raw(), stderr_write.raw()];
        attributes
            .update(
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                handles.as_ptr().cast(),
                std::mem::size_of_val(&handles),
            )
            .map_err(io_error)?;
        let mut startup = StartupInfoExW {
            startup_info: StartupInfoW {
                cb: std::mem::size_of::<StartupInfoExW>() as u32,
                lp_reserved: std::ptr::null_mut(),
                lp_desktop: std::ptr::null_mut(),
                lp_title: std::ptr::null_mut(),
                dw_x: 0,
                dw_y: 0,
                dw_x_size: 0,
                dw_y_size: 0,
                dw_x_count_chars: 0,
                dw_y_count_chars: 0,
                dw_fill_attribute: 0,
                dw_flags: STARTF_USESTDHANDLES,
                w_show_window: 0,
                cb_reserved2: 0,
                lp_reserved2: std::ptr::null_mut(),
                h_std_input: stdin.raw(),
                h_std_output: stdout_write.raw(),
                h_std_error: stderr_write.raw(),
            },
            attribute_list: attributes.list,
        };
        let mut process_information = ProcessInformation {
            h_process: null_handle(),
            h_thread: null_handle(),
            dw_process_id: 0,
            dw_thread_id: 0,
        };
        // SAFETY: all pointers reference live values for the complete call; the
        // process is suspended until it has joined the Job Object.
        if unsafe {
            CreateProcessW(
                application_name.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
                environment.as_mut_ptr().cast(),
                current_directory.as_ptr(),
                &mut startup.startup_info,
                &mut process_information,
            )
        } == 0
        {
            return Err(io_error(io::Error::last_os_error()));
        }
        let process = HandleGuard::new(process_information.h_process).map_err(io_error)?;
        let thread_handle = match HandleGuard::new(process_information.h_thread) {
            Ok(handle) => handle,
            Err(error) => {
                unsafe { TerminateProcess(process.raw(), 1) };
                unsafe { WaitForSingleObject(process.raw(), INFINITE) };
                return Err(io_error(error));
            }
        };
        drop(attributes);
        drop(stdin);
        drop(stdout_write);
        drop(stderr_write);
        if let Err(error) = job.assign(process.raw()) {
            unsafe { TerminateProcess(process.raw(), 1) };
            unsafe { WaitForSingleObject(process.raw(), INFINITE) };
            return Err(io_error(error));
        }
        if unsafe { ResumeThread(thread_handle.raw()) } == INVALID_THREAD_RESUME {
            unsafe { TerminateProcess(process.raw(), 1) };
            unsafe { WaitForSingleObject(process.raw(), INFINITE) };
            return Err(io_error(io::Error::last_os_error()));
        }
        let output_limit = output_limit.map(|limit| limit.max(0) as usize);
        let output_budget =
            output_limit.map(|_| std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)));
        let output_limit_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stdout_thread = read_pipe(
            stdout_read.into_raw(),
            output_limit,
            output_budget.clone(),
            output_limit_hit.clone(),
        );
        let stderr_thread = read_pipe(
            stderr_read.into_raw(),
            output_limit,
            output_budget,
            output_limit_hit.clone(),
        );
        let deadline =
            timeout_ms.map(|timeout| Instant::now() + Duration::from_millis(timeout.max(0) as u64));
        let mut timed_out = false;
        let mut wait_error = None;
        let mut process_done = false;
        loop {
            let result = unsafe { WaitForSingleObject(process.raw(), 50) };
            if result == 0 {
                process_done = true;
                break;
            }
            if result == WAIT_FAILED {
                wait_error = Some(io::Error::last_os_error());
                break;
            }
            if output_limit_hit.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                timed_out = true;
                break;
            }
        }
        // Always close the process tree, including after the leader exits: a
        // descendant can keep the stdio pipes open. If TerminateJobObject
        // itself fails, dropping the handle invokes the Job Object's
        // kill-on-close rule before waiting, so this path cannot wait forever
        // on an escaped child.
        let mut job = Some(job);
        let terminate_error = if process_done {
            drop(job.take());
            None
        } else {
            match job.as_ref().expect("job guard exists").terminate() {
                Ok(()) => None,
                Err(error) => {
                    drop(job.take());
                    Some(error)
                }
            }
        };
        if !process_done && unsafe { WaitForSingleObject(process.raw(), INFINITE) } == WAIT_FAILED {
            wait_error.get_or_insert_with(io::Error::last_os_error);
        }
        let mut exit_code = 1;
        let exit_error = if unsafe { GetExitCodeProcess(process.raw(), &mut exit_code) } == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        let (stdout, stdout_exceeded) = stdout_thread
            .join()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "stdout reader panicked"))
            .and_then(|result| result)
            .map_err(io_error)?;
        let (stderr, stderr_exceeded) = stderr_thread
            .join()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "stderr reader panicked"))
            .and_then(|result| result)
            .map_err(io_error)?;
        if let Some(error) = wait_error.or(terminate_error).or(exit_error) {
            return Err(io_error(error));
        }
        if stdout_exceeded || stderr_exceeded {
            return Err(WindowsSandboxError::Io(
                "process output exceeded output_limit".to_string(),
            ));
        }
        Ok(BackendOutput {
            output: Output {
                status: ExitStatus::from_raw(exit_code),
                stdout,
                stderr,
            },
            mechanism: MECHANISM.to_string(),
            policy: backend_policy(
                output_dir.as_deref(),
                share_network,
                source_readable,
                source_writable,
            ),
            timed_out,
            pid: process_information.dw_process_id as i64,
        })
    }

    fn io_error(error: io::Error) -> WindowsSandboxError {
        WindowsSandboxError::Io(error.to_string())
    }

    fn reject_reparse_components(path: &Path, kind: &str) -> Result<(), WindowsSandboxError> {
        let mut current = path;
        loop {
            let metadata = fs::symlink_metadata(current).map_err(|error| {
                io_error(io::Error::new(
                    io::ErrorKind::Other,
                    format!("sandbox {kind} `{}`: {error}", current.display()),
                ))
            })?;
            if metadata.file_type().is_symlink()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io_error(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "sandbox {kind} `{}` contains a symlink or reparse point at `{}`",
                        path.display(),
                        current.display()
                    ),
                )));
            }
            let Some(parent) = current.parent() else {
                break;
            };
            if parent == current || parent.as_os_str().is_empty() {
                break;
            }
            current = parent;
        }
        Ok(())
    }

    fn real_directory(path: &Path) -> Result<PathBuf, WindowsSandboxError> {
        reject_reparse_components(path, "directory")?;
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            io_error(io::Error::new(
                io::ErrorKind::Other,
                format!("sandbox directory `{}`: {error}", path.display()),
            ))
        })?;
        if metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io_error(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "sandbox path `{}` is a symlink or reparse point",
                    path.display()
                ),
            )));
        }
        let canonical = path.canonicalize().map_err(|error| {
            io_error(io::Error::new(
                io::ErrorKind::Other,
                format!("sandbox directory `{}`: {error}", path.display()),
            ))
        })?;
        reject_reparse_components(&canonical, "directory")?;
        let canonical_metadata = fs::symlink_metadata(&canonical).map_err(|error| {
            io_error(io::Error::new(
                io::ErrorKind::Other,
                format!("sandbox directory `{}`: {error}", path.display()),
            ))
        })?;
        if canonical_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || !canonical_metadata.is_dir()
        {
            return Err(io_error(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("sandbox path `{}` is not a directory", path.display()),
            )));
        }
        Ok(canonical)
    }

    fn real_executable(path: &Path) -> Result<PathBuf, WindowsSandboxError> {
        reject_reparse_components(path, "executable")?;
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            io_error(io::Error::new(
                io::ErrorKind::Other,
                format!("sandbox executable `{}`: {error}", path.display()),
            ))
        })?;
        if metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io_error(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "sandbox executable `{}` is a symlink or reparse point",
                    path.display()
                ),
            )));
        }
        let canonical = path.canonicalize().map_err(|error| {
            io_error(io::Error::new(
                io::ErrorKind::Other,
                format!("sandbox executable `{}`: {error}", path.display()),
            ))
        })?;
        reject_reparse_components(&canonical, "executable")?;
        let canonical_metadata = fs::symlink_metadata(&canonical).map_err(|error| {
            io_error(io::Error::new(
                io::ErrorKind::Other,
                format!("sandbox executable `{}`: {error}", path.display()),
            ))
        })?;
        if canonical_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || !canonical_metadata.is_file()
        {
            return Err(io_error(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "sandbox executable `{}` is not a regular file",
                    path.display()
                ),
            )));
        }
        Ok(canonical)
    }
}

#[cfg(target_os = "windows")]
pub(crate) use jet_process_windows_sandbox::{
    run as windows_output, run_with_read_only_mounts as windows_output_with_read_only_mounts,
    status as windows_status, BackendOutput as WindowsSandboxOutput,
    BackendStatus as WindowsSandboxStatus, WindowsSandboxError,
};

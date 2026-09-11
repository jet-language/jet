    // ── D-SIMD2 / D-SIMD3 / D-LINALG1: built-in math value types ────────────────
    // The pinned stable rustc has no `std::simd`, so the portable lane family is
    // represented by fixed arrays. F64x4 has a private host-native carrier
    // behind the same value API; the shared Prelude owns both representations,
    // and the public surface does not depend on a target feature.

    // Private place access uses the same checked lane index as value reads.
    pub(crate) trait JetNativeLane {
        type Scalar;
        const NAME: &'static str;
        fn lanes(&self) -> &[Self::Scalar];
        fn lanes_mut(&mut self) -> &mut [Self::Scalar];
    }

    pub(crate) fn jet_native_lane_ref<'a, L: JetNativeLane>(value: &'a L, index: i64, file: &str, line: u32) -> &'a L::Scalar {
        let lanes = value.lanes();
        let index = crate::jet_simd_lane_index(index, L::NAME, lanes.len())
            .unwrap_or_else(|message| crate::jet_panic(file, line, &message));
        &lanes[index]
    }

    pub(crate) fn jet_native_lane_mut<'a, L: JetNativeLane>(value: &'a mut L, index: i64, file: &str, line: u32) -> &'a mut L::Scalar {
        let lanes = value.lanes_mut();
        let index = crate::jet_simd_lane_index(index, L::NAME, lanes.len())
            .unwrap_or_else(|message| crate::jet_panic(file, line, &message));
        &mut lanes[index]
    }

    macro_rules! jet_lane_type {
        (F64x4, $E:ty, $N:literal) => {
            #[repr(transparent)]
            #[derive(Clone, Copy)]
            pub struct F64x4(pub(crate) crate::JetF64x4);
            impl JetNativeLane for F64x4 {
                type Scalar = f64;
                const NAME: &'static str = "F64x4";
                fn lanes(&self) -> &[f64] {
                    // SAFETY: JetF64x4 is [f64; 4] or __m256d, the same four
                    // contiguous f64 lanes used by the native array bridge.
                    // Both carriers have sufficient alignment and no padding.
                    unsafe { &*((&self.0 as *const crate::JetF64x4).cast::<[f64; 4]>()) }
                }
                fn lanes_mut(&mut self) -> &mut [f64] {
                    // SAFETY: same representation as lanes(); this exclusive
                    // borrow is the sole access to the carrier for its lifetime.
                    unsafe { &mut *((&mut self.0 as *mut crate::JetF64x4).cast::<[f64; 4]>()) }
                }
            }
        };
        ($T:ident, $E:ty, $N:literal) => {
            #[repr(transparent)]
            #[derive(Clone, Copy, Debug, PartialEq)]
            pub struct $T(pub [$E; $N]);
            impl JetNativeLane for $T {
                type Scalar = $E;
                const NAME: &'static str = stringify!($T);
                fn lanes(&self) -> &[$E] { &self.0 }
                fn lanes_mut(&mut self) -> &mut [$E] { &mut self.0 }
            }
        };
    }

    jet_lane_type!(F32x4, f32, 4);
    jet_lane_type!(F64x2, f64, 2);
    jet_lane_type!(F32x8, f32, 8);
    jet_lane_type!(F64x4, f64, 4);
    jet_lane_type!(I8x16, i8, 16);
    jet_lane_type!(I16x8, i16, 8);
    jet_lane_type!(I32x4, i32, 4);
    jet_lane_type!(I64x2, i64, 2);
    jet_lane_type!(U8x16, u8, 16);
    jet_lane_type!(U16x8, u16, 8);
    jet_lane_type!(U32x4, u32, 4);
    jet_lane_type!(U64x2, u64, 2);
    jet_lane_type!(I8x32, i8, 32);
    jet_lane_type!(I16x16, i16, 16);
    jet_lane_type!(I32x8, i32, 8);
    jet_lane_type!(I64x4, i64, 4);
    jet_lane_type!(U8x32, u8, 32);
    jet_lane_type!(U16x16, u16, 16);
    jet_lane_type!(U32x8, u32, 8);
    jet_lane_type!(U64x4, u64, 4);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Vec2(pub [f64; 2]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Vec3(pub [f64; 3]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Vec4(pub [f64; 4]);
    // Column-major: element (row r, col c) is `.0[c * N + r]`.
    #[derive(Clone, Copy, PartialEq)]
    pub struct Mat3(pub [f64; 9]);
    #[derive(Clone, Copy, PartialEq)]
    pub struct Mat4(pub [f64; 16]);

    macro_rules! jet_lane_call {
        (ref, $op:path, $left:expr, $right:expr) => {
            $op(&$left, &$right)
        };
        (value, $op:path, $left:expr, $right:expr) => {
            $op($left, $right)
        };
    }

    macro_rules! jet_lane_ops {
        ($T:ident, $E:ty, $N:literal) => {
            jet_lane_ops!(
                $T,
                $E,
                $N,
                crate::jet_simd_add_array,
                crate::jet_simd_sub_array,
                crate::jet_simd_mul_array,
                crate::jet_simd_div_array,
                ref
            );
        };
        ($T:ident, $E:ty, $N:literal, $add:path, $sub:path, $mul:path, $div:path, $mode:ident) => {
            impl std::ops::Add for $T {
                type Output = $T;
                #[inline(always)]
                fn add(self, o: $T) -> $T {
                    $T(jet_lane_call!($mode, $add, self.0, o.0))
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                #[inline(always)]
                fn sub(self, o: $T) -> $T {
                    $T(jet_lane_call!($mode, $sub, self.0, o.0))
                }
            }
            impl std::ops::Mul for $T {
                type Output = $T;
                #[inline(always)]
                fn mul(self, o: $T) -> $T {
                    $T(jet_lane_call!($mode, $mul, self.0, o.0))
                }
            }
            impl std::ops::Div for $T {
                type Output = $T;
                #[inline(always)]
                fn div(self, o: $T) -> $T {
                    $T(jet_lane_call!($mode, $div, self.0, o.0))
                }
            }
            impl std::ops::AddAssign for $T {
                #[inline(always)]
                fn add_assign(&mut self, o: $T) {
                    *self = *self + o;
                }
            }
            impl std::ops::SubAssign for $T {
                #[inline(always)]
                fn sub_assign(&mut self, o: $T) {
                    *self = *self - o;
                }
            }
            impl std::ops::MulAssign for $T {
                #[inline(always)]
                fn mul_assign(&mut self, o: $T) {
                    *self = *self * o;
                }
            }
            impl std::ops::DivAssign for $T {
                #[inline(always)]
                fn div_assign(&mut self, o: $T) {
                    *self = *self / o;
                }
            }
        };
    }
    jet_lane_ops!(
        F32x4,
        f32,
        4,
        crate::jet_simd_f32x4_add_array,
        crate::jet_simd_f32x4_sub_array,
        crate::jet_simd_f32x4_mul_array,
        crate::jet_simd_f32x4_div_array,
        ref
    );
    jet_lane_ops!(
        F64x2,
        f64,
        2,
        crate::jet_simd_f64x2_add_array,
        crate::jet_simd_f64x2_sub_array,
        crate::jet_simd_f64x2_mul_array,
        crate::jet_simd_f64x2_div_array,
        ref
    );
    jet_lane_ops!(
        F32x8,
        f32,
        8,
        crate::jet_simd_f32x8_add_array,
        crate::jet_simd_f32x8_sub_array,
        crate::jet_simd_f32x8_mul_array,
        crate::jet_simd_f32x8_div_array,
        ref
    );
    jet_lane_ops!(
        F64x4,
        f64,
        4,
        crate::jet_simd_f64x4_add_native,
        crate::jet_simd_f64x4_sub_native,
        crate::jet_simd_f64x4_mul_native,
        crate::jet_simd_f64x4_div_native,
        value
    );
    // The array entry points remain available to portable marshaling adapters
    // (`crate::jet_simd_f64x4_add_value`, and its sibling operators); the
    // generated F64x4 value itself stays on the native carrier above.
    jet_lane_ops!(I8x16, i8, 16);
    jet_lane_ops!(I16x8, i16, 8);
    jet_lane_ops!(I32x4, i32, 4);
    jet_lane_ops!(I64x2, i64, 2);
    jet_lane_ops!(U8x16, u8, 16);
    jet_lane_ops!(U16x8, u16, 8);
    jet_lane_ops!(U32x4, u32, 4);
    jet_lane_ops!(U64x2, u64, 2);
    jet_lane_ops!(I8x32, i8, 32);
    jet_lane_ops!(I16x16, i16, 16);
    jet_lane_ops!(I32x8, i32, 8);
    jet_lane_ops!(I64x4, i64, 4);
    jet_lane_ops!(U8x32, u8, 32);
    jet_lane_ops!(U16x16, u16, 16);
    jet_lane_ops!(U32x8, u32, 8);
    jet_lane_ops!(U64x4, u64, 4);

    macro_rules! jet_vec_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            // `v * w` is element-wise (Hadamard); the dot/cross products are methods.
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..$N {
                        r[i] = self.0[i] * o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::AddAssign for $T {
                fn add_assign(&mut self, o: $T) {
                    *self = *self + o;
                }
            }
            impl std::ops::SubAssign for $T {
                fn sub_assign(&mut self, o: $T) {
                    *self = *self - o;
                }
            }
            impl std::ops::MulAssign for $T {
                fn mul_assign(&mut self, o: $T) {
                    *self = *self * o;
                }
            }
        };
    }
    jet_vec_ops!(Vec2, 2);
    jet_vec_ops!(Vec3, 3);
    jet_vec_ops!(Vec4, 4);
    
    macro_rules! jet_mat_ops {
        ($T:ident, $N:literal) => {
            impl std::ops::Add for $T {
                type Output = $T;
                fn add(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..($N * $N) {
                        r[i] = self.0[i] + o.0[i];
                    }
                    $T(r)
                }
            }
            impl std::ops::Sub for $T {
                type Output = $T;
                fn sub(self, o: $T) -> $T {
                    let mut r = self.0;
                    for i in 0..($N * $N) {
                        r[i] = self.0[i] - o.0[i];
                    }
                    $T(r)
                }
            }
            // `m * n` is matrix multiply (column-major).
            impl std::ops::Mul for $T {
                type Output = $T;
                fn mul(self, o: $T) -> $T {
                    let mut r = [0.0f64; $N * $N];
                    for c in 0..$N {
                        for row in 0..$N {
                            let mut acc = 0.0f64;
                            for k in 0..$N {
                                acc += self.0[k * $N + row] * o.0[c * $N + k];
                            }
                            r[c * $N + row] = acc;
                        }
                    }
                    $T(r)
                }
            }
            impl std::ops::AddAssign for $T {
                fn add_assign(&mut self, o: $T) {
                    *self = *self + o;
                }
            }
            impl std::ops::SubAssign for $T {
                fn sub_assign(&mut self, o: $T) {
                    *self = *self - o;
                }
            }
            impl std::ops::MulAssign for $T {
                fn mul_assign(&mut self, o: $T) {
                    *self = *self * o;
                }
            }
        };
    }
    jet_mat_ops!(Mat3, 3);
    jet_mat_ops!(Mat4, 4);

    // `Mat * Vec` transforms the vector (column-major).
    impl std::ops::Mul<Vec3> for Mat3 {
        type Output = Vec3;
        fn mul(self, v: Vec3) -> Vec3 {
            let mut r = [0.0f64; 3];
            for row in 0..3 {
                let mut a = 0.0f64;
                for k in 0..3 {
                    a += self.0[k * 3 + row] * v.0[k];
                }
                r[row] = a;
            }
            Vec3(r)
        }
    }
    impl std::ops::Mul<Vec4> for Mat4 {
        type Output = Vec4;
        fn mul(self, v: Vec4) -> Vec4 {
            let mut r = [0.0f64; 4];
            for row in 0..4 {
                let mut a = 0.0f64;
                for k in 0..4 {
                    a += self.0[k * 4 + row] * v.0[k];
                }
                r[row] = a;
            }
            Vec4(r)
        }
    }

macro_rules! jet_lane_show {
    ($T:ident) => {
        impl super::JetShow for $T {
            fn jet_show(&self) -> String {
                format!("{}({:?})", stringify!($T), self.0)
            }
        }
        impl super::JetDebug for $T {
            fn jet_debug(&self) -> String {
                format!("{}({:?})", stringify!($T), self.0)
            }
        }
        impl super::JetDisplay for $T {
            fn jet_display(&self) -> String {
                self.jet_show()
            }
        }
    };
}
    jet_lane_show!(F32x4);
    jet_lane_show!(F64x2);
    jet_lane_show!(F32x8);
    impl std::fmt::Debug for F64x4 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("F64x4")
                .field(&crate::jet_simd_f64x4_to_array_native(self.0))
                .finish()
        }
    }
    impl PartialEq for F64x4 {
        fn eq(&self, other: &Self) -> bool {
            crate::jet_simd_f64x4_to_array_native(self.0)
                == crate::jet_simd_f64x4_to_array_native(other.0)
        }
    }
    impl super::JetShow for F64x4 {
        fn jet_show(&self) -> String {
            format!("{self:?}")
        }
    }
    impl super::JetDebug for F64x4 {
        fn jet_debug(&self) -> String {
            format!("{self:?}")
        }
    }
    impl super::JetDisplay for F64x4 {
        fn jet_display(&self) -> String {
            self.jet_show()
        }
    }
    jet_lane_show!(I8x16);
    jet_lane_show!(I16x8);
    jet_lane_show!(I32x4);
    jet_lane_show!(I64x2);
    jet_lane_show!(U8x16);
    jet_lane_show!(U16x8);
    jet_lane_show!(U32x4);
    jet_lane_show!(U64x2);
    jet_lane_show!(I8x32);
    jet_lane_show!(I16x16);
    jet_lane_show!(I32x8);
    jet_lane_show!(I64x4);
    jet_lane_show!(U8x32);
    jet_lane_show!(U16x16);
    jet_lane_show!(U32x8);
    jet_lane_show!(U64x4);
    macro_rules! jet_math_debug {
        ($type:ident, $($field:literal => $index:literal),+ $(,)?) => {
            impl std::fmt::Debug for $type {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let mut debug = f.debug_struct(stringify!($type));
                    $(debug.field($field, &self.0[$index]);)+
                    debug.finish()
                }
            }
            impl super::JetShow for $type {
                fn jet_show(&self) -> String {
                    format!("{self:?}")
                }
            }
            impl super::JetDebug for $type {
                fn jet_debug(&self) -> String {
                    format!("{self:?}")
                }
            }
            impl super::JetDisplay for $type {
                fn jet_display(&self) -> String {
                    self.jet_show()
                }
            }
        };
    }
    jet_math_debug!(Vec2, "x" => 0, "y" => 1);
    jet_math_debug!(Vec3, "x" => 0, "y" => 1, "z" => 2);
    jet_math_debug!(Vec4, "x" => 0, "y" => 1, "z" => 2, "w" => 3);
    jet_math_debug!(
        Mat3,
        "m00" => 0, "m10" => 1, "m20" => 2,
        "m01" => 3, "m11" => 4, "m21" => 5,
        "m02" => 6, "m12" => 7, "m22" => 8,
    );
    jet_math_debug!(
        Mat4,
        "m00" => 0, "m10" => 1, "m20" => 2, "m30" => 3,
        "m01" => 4, "m11" => 5, "m21" => 6, "m31" => 7,
        "m02" => 8, "m12" => 9, "m22" => 10, "m32" => 11,
        "m03" => 12, "m13" => 13, "m23" => 14, "m33" => 15,
    );

    struct JetTaskState<T: Send + 'static> {
        handle: std::sync::Mutex<Option<super::JetSchedulerJoin<T>>>,
        control: std::sync::Arc<super::JetTaskControl>,
        // Typed operations such as AsyncEvent convert their inherited deadline
        // into their own result value. Re-checking the caller deadline after
        // join would replace that value with E3003 and violate the typed API.
        skip_join_deadline: bool,
    }

    trait JetTaskGroupChild: Send + Sync {
        fn cancel(&self);
        fn join(&self);
    }

    impl<T: Send + 'static> JetTaskGroupChild for JetTaskState<T> {
        fn cancel(&self) {
            if self.handle.lock().unwrap().is_some() {
                self.control.cancel();
            }
        }

        fn join(&self) {
            if let Some(handle) = self.handle.lock().unwrap().take() {
                // Group cleanup is not a parent wait point. The shared drain
                // consumes the child completion even while the parent is
                // unwinding from cancellation.
                handle.drain();
            }
        }
    }

    /// D-CONC-SPAWN1=D: the internal runtime identity shared by a lexical
    /// `task.group` and every named helper that receives it.
    pub struct JetTaskGroup {
        children: std::sync::Arc<JetTaskGroupRuntime<std::sync::Arc<dyn JetTaskGroupChild>>>,
        owner: bool,
    }

    impl Clone for JetTaskGroup {
        fn clone(&self) -> Self {
            Self {
                children: self.children.clone(),
                owner: false,
            }
        }
    }

    impl JetTaskGroup {
        pub fn new() -> Self {
            Self {
                children: std::sync::Arc::new(JetTaskGroupRuntime::new_defaulted(None)),
                owner: true,
            }
        }

        pub fn with_limit(limit: i64) -> Self {
            Self {
                children: std::sync::Arc::new(JetTaskGroupRuntime::new_defaulted(Some(limit))),
                owner: true,
            }
        }

        /// D-TASKBORROW1=A: one canonical group spawn path covers both owned
        /// and sema-proven borrowed captures. The lexical group closes the
        /// loan by joining every registered child before it drops.
        pub fn spawn<'env, F, T>(&self, f: F) -> JetTask<T>
        where
            F: FnOnce() -> T + Send + 'env,
            T: Send + 'static,
        {
            self.spawn_at(0, "", f)
        }

        pub fn spawn_at<'env, F, T>(
            &self,
            spawn_site: usize,
            label: &str,
            f: F,
        ) -> JetTask<T>
        where
            F: FnOnce() -> T + Send + 'env,
            T: Send + 'static,
        {
            let boxed: Box<dyn FnOnce() -> T + Send + 'env> = Box::new(f);
            // JET_VETTED_UNSAFE_BEGIN: jet_taskgroup_borrowed_spawn
            let erased: Box<dyn FnOnce() -> T + Send + 'static> =
                unsafe { std::mem::transmute(boxed) };
            // JET_VETTED_UNSAFE_END: jet_taskgroup_borrowed_spawn
            let waiter = super::ParkSlot::new();
            let permit = self
                .children
                .acquire_with(waiter, |waiter| {
                    super::jet_scheduler_task_group_wait(waiter);
                    Ok::<(), ()>(())
                })
                .expect("task-group admission wait cannot fail");
            let task = JetTask::spawn_at(spawn_site, label, move || {
                let _permit = permit;
                erased()
            });
            self.children.register(task.state.clone());
            task
        }

        pub fn close(&self) {
            if jet_task_deadline_pending() {
                self.children
                    .close_with_cancel(|child| child.cancel(), |child| child.join());
            } else {
                self.children.close_with(|child| child.join());
            }
        }
    }

    impl Drop for JetTaskGroup {
        fn drop(&mut self) {
            if self.owner {
                self.close();
            }
        }
    }

    pub struct JetTask<T: Send + 'static> {
        state: std::sync::Arc<JetTaskState<T>>,
    }

    impl<T: Send + 'static> Default for JetTask<T> {
        fn default() -> Self {
            JetTask {
                state: std::sync::Arc::new(JetTaskState {
                    handle: std::sync::Mutex::new(None),
                    control: super::JetTaskControl::new(),
                    skip_join_deadline: false,
                }),
            }
        }
    }
    impl<T: Send + 'static> JetTask<T> {
        pub fn spawn<F: FnOnce() -> T + Send + 'static>(f: F) -> JetTask<T> {
            Self::spawn_at(0, "", f)
        }

        pub fn spawn_at<F: FnOnce() -> T + Send + 'static>(
            spawn_site: usize,
            label: &str,
            f: F,
        ) -> JetTask<T> {
            let inherited_deadline = super::jet_ctx_deadline_ms();
            let control = super::JetTaskControl::new();
            JetTask {
                state: std::sync::Arc::new(JetTaskState {
                    handle: std::sync::Mutex::new(Some(
                        super::jet_scheduler_spawn_blocking_with_control_at(
                            spawn_site,
                            label,
                            move || {
                                let _deadline_guard =
                                    inherited_deadline.map(super::jet_ctx_push_deadline);
                                f()
                            },
                            control.clone(),
                        ),
                    )),
                    control,
                    skip_join_deadline: false,
                }),
            }
        }
        pub(crate) fn spawn_typed_deadline<F: FnOnce() -> T + Send + 'static>(
            f: F,
            control: std::sync::Arc<super::JetTaskControl>,
        ) -> JetTask<T> {
            Self::spawn_typed_deadline_at(0, "", f, control)
        }

        pub(crate) fn spawn_typed_deadline_at<F: FnOnce() -> T + Send + 'static>(
            spawn_site: usize,
            label: &str,
            f: F,
            control: std::sync::Arc<super::JetTaskControl>,
        ) -> JetTask<T> {
            let inherited_deadline = super::jet_ctx_deadline_ms();
            JetTask {
                state: std::sync::Arc::new(JetTaskState {
                    handle: std::sync::Mutex::new(Some(
                        super::jet_scheduler_spawn_blocking_with_control_at(
                            spawn_site,
                            label,
                            move || {
                                let _deadline_guard =
                                    inherited_deadline.map(super::jet_ctx_push_deadline);
                                let _typed_deadline_boundary =
                                    super::JetTypedDeadlineBoundary::enter();
                                f()
                            },
                            control.clone(),
                        ),
                    )),
                    control,
                    skip_join_deadline: true,
                }),
            }
        }
        // D-COROUTINE1=A: control-plane hooks on the M:N scheduler substrate.
        pub fn pause(&self) {
            self.pause_with_mode(0);
        }
        /// D-TASK-PAUSE-TIER1=E: `mode` 0 = WaitPoints, 1 = CheckLoops.
        pub fn pause_with_mode(&self, mode: i64) {
            let mode = if mode == 1 { 1 } else { 0 };
            self.state.control.pause_with_mode(mode);
        }
        pub fn resume(&self) {
            self.state.control.resume();
        }
        pub fn cancel(&self) {
            self.state.control.cancel();
        }
        /// Status-only probe for heterogeneous `task.all`. The probe caches
        /// the typed completion in the scheduler join; `join()` still owns and
        /// returns that concrete value later.
        pub fn poll(&self) -> super::JetSchedulerTaskPoll {
            self.state
                .handle
                .lock()
                .unwrap()
                .as_ref()
                .expect("task already joined")
                .poll()
        }
        /// D-CONC-FAIL1=A: child cancellation, deadline, and panic are values
        /// in the one TaskFailure rail; the scheduler owns only the wait-point
        /// adapter for cancellation of the joining parent.
        pub fn join(self) -> Result<T, JetTaskFailure> {
            let state = self.state;
            let skip_join_deadline = state.skip_join_deadline;
            if !skip_join_deadline {
                super::jet_task_join_deadline_check();
            }
            let result = {
                let mut handle = state.handle.lock().unwrap();
                let result = handle
                    .as_mut()
                    .expect("task already joined")
                    .join();
                let _ = handle.take();
                result
            };
            if !skip_join_deadline {
                super::jet_task_join_deadline_check();
            }
            result
        }
        /// Drain a completed or cancelled child without observing the parent
        /// wait policy. The heterogeneous `task.all` adapter uses this after
        /// fail-fast cancellation so cleanup cannot re-raise the parent's
        /// cancellation while consuming child completions.
        pub fn drain(self) {
            let state = self.state;
            let handle = { state.handle.lock().unwrap().take() };
            if let Some(handle) = handle {
                handle.drain();
            }
        }
        pub fn detach(self) {
            let _ = self.state.handle.lock().unwrap().take();
        }
    }

    fn jet_task_entries<T: Send + 'static>(
        tasks: Vec<JetTask<T>>,
        operation: &str,
    ) -> Vec<(super::JetSchedulerJoin<T>, std::sync::Arc<super::JetTaskControl>)> {
        tasks
            .into_iter()
            .map(|task| {
                (
                    task.state
                        .handle
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap_or_else(|| panic!("{operation}: task already joined")),
                    task.state.control.clone(),
                )
            })
            .collect()
    }

    /// D-CONC-SPAWN1=D: `task.all` joins every child; fail fast and cancel siblings on error.
    pub fn jet_task_all<T: Send + 'static>(
        tasks: Vec<JetTask<T>>,
    ) -> Result<Vec<T>, JetTaskFailure> {
        super::jet_scheduler_all(jet_task_entries(tasks, "all"))
    }

    /// D-CONC-SPAWN1=D: `task.race` returns the first successful result and cancels siblings.
    pub fn jet_task_race<T: Send + 'static>(
        tasks: Vec<JetTask<T>>,
    ) -> Result<T, JetTaskFailure> {
        super::jet_scheduler_race(jet_task_entries(tasks, "race"))
    }

    /// D-CONC-SPAWN1=D: `task.any` returns the first completed result.
    pub fn jet_task_any<T: Send + 'static>(
        tasks: Vec<JetTask<T>>,
    ) -> Result<T, JetTaskFailure> {
        super::jet_scheduler_any(jet_task_entries(tasks, "any"))
    }

    /// D-CONC-FAIL1=A: a propagating task body returns a private `Result`
    /// carrier. Flatten it at the task boundary before `join()` exposes the
    /// public `TaskFailure` rail.
    pub fn jet_task_join_result<T, E>(
        task: JetTask<Result<T, E>>,
    ) -> Result<T, JetTaskFailure>
    where
        T: Send + 'static,
        E: Send + 'static + std::fmt::Debug,
    {
        jet_task_flatten_result(task.join())
    }

    pub fn jet_task_all_result<T, E>(
        tasks: Vec<JetTask<Result<T, E>>>,
    ) -> Result<Vec<T>, JetTaskFailure>
    where
        T: Send + 'static,
        E: Send + 'static + std::fmt::Debug,
    {
        jet_task_flatten_results(jet_task_all(tasks))
    }

    pub fn jet_task_race_result<T, E>(
        tasks: Vec<JetTask<Result<T, E>>>,
    ) -> Result<T, JetTaskFailure>
    where
        T: Send + 'static,
        E: Send + 'static + std::fmt::Debug,
    {
        jet_task_flatten_result(jet_task_race(tasks))
    }

    pub fn jet_task_any_result<T, E>(
        tasks: Vec<JetTask<Result<T, E>>>,
    ) -> Result<T, JetTaskFailure>
    where
        T: Send + 'static,
        E: Send + 'static + std::fmt::Debug,
    {
        jet_task_flatten_result(jet_task_any(tasks))
    }

    /// Cooperative yield — park at a wait point with a zero timeout.
    pub fn jet_task_yield() {
        super::jet_scheduler_yield_now();
    }

    /// Control-plane trace of the currently running task (or the idle defaults).
    pub fn jet_task_current_trace() -> String {
        super::jet_scheduler_current_task_trace()
    }

    /// D-CONC-CHAN1: wait on plain endpoints and return the selected arm plus
    /// its receive payload. Timer arms carry no value, so the option is absent.
    pub fn jet_select_wait_tagged<T: Send + 'static>(
        recvs: &[JetReceiver<T>],
        after_ns: Vec<i64>,
    ) -> (i64, Option<T>) {
        let inners: Vec<_> = recvs.iter().map(|receiver| receiver.inner.select_inner()).collect();
        let after_ms = after_ns
            .into_iter()
            .map(|ns| super::jet_task_delay_ms_defaulted(super::jet_std_time_duration_to_millis(ns)))
            .collect();
        match super::jet_scheduler_select(inners, after_ms) {
            super::JetSelectOutcome::Recv { arm, value } => (arm as i64, Some(value)),
            super::JetSelectOutcome::After { arm } => ((recvs.len() + arm) as i64, None),
            super::JetSelectOutcome::Closed => super::jet_scheduler_fatal("select closed"),
        }
    }

    /// D-CONC-CHAN2=D: probe a readiness table with an `else` arm. `-1` means
    /// that no receive or timer arm is ready, including a closed-only table.
    pub fn jet_select_try_wait_tagged<T: Send + 'static>(
        recvs: &[JetReceiver<T>],
        after_ns: Vec<i64>,
    ) -> (i64, Option<T>) {
        let inners: Vec<_> = recvs.iter().map(|receiver| receiver.inner.select_inner()).collect();
        let after_ms = after_ns
            .into_iter()
            .map(|ns| super::jet_task_delay_ms_defaulted(super::jet_std_time_duration_to_millis(ns)))
            .collect();
        match super::jet_scheduler_try_select(inners, after_ms) {
            Some(super::JetSelectOutcome::Recv { arm, value }) => (arm as i64, Some(value)),
            Some(super::JetSelectOutcome::After { arm }) => ((recvs.len() + arm) as i64, None),
            Some(super::JetSelectOutcome::Closed) | None => (-1, None),
        }
    }

    /// D-TUPLE-DESTRUCT1: `channel<T>()` — mirrors Rust's `mpsc::channel()`:
    /// returns the `(Sender<T>, Receiver<T>)` pair directly (no combined "Channel"
    /// handle, and no `.sender()` method — a second sender is `tx.clone()`).
    pub fn channel<T: Send>() -> (JetSender<T>, JetReceiver<T>) {
        let inner = super::JetSchedulerChannel::new();
        let tx = inner.sender();
        (JetSender { tx }, JetReceiver { inner })
    }

    /// D-TASKRUNTIME1=A: bounded channel; `capacity` is a real memory/backpressure bound.
    pub fn channel_bounded<T: Send>(capacity: i64) -> (JetSender<T>, JetReceiver<T>) {
        let inner = super::JetSchedulerChannel::bounded(capacity);
        let tx = inner.sender();
        (JetSender { tx }, JetReceiver { inner })
    }

    // D-CONC-STREAM1=A / D-CANCELMODEL1=C: the canonical pull/cancellation
    // protocol lives in the shared Prelude scheduler. This module only
    // exposes it under the `jet_std` namespace used by emitted AOT code.
    pub use super::{jet_stream_task, JetStream, JetStreamSender};

    /// D-TASKRUNTIME1=A: one-shot timer channel; wakes through the scheduler timer wheel.
    pub fn after(ms: i64) -> JetReceiver<()> {
        let (tx, rx) = channel::<()>();
        let delay = super::jet_task_delay_ms_defaulted(ms);
        super::jet_scheduler_spawn_detached_timer(move || {
            super::jet_scheduler_sleep_ms(delay);
            tx.send(());
        });
        rx
    }

    /// D-TASKRUNTIME1=A: one-shot typed timer channel for select timeout values.
    pub fn after_value<T: Send + 'static>(ms: i64, value: T) -> JetReceiver<T> {
        let (tx, rx) = channel::<T>();
        let delay = super::jet_task_delay_ms_defaulted(ms);
        super::jet_scheduler_spawn_detached_timer(move || {
            super::jet_scheduler_sleep_ms(delay);
            tx.send(value);
        });
        rx
    }

    /// D-TASKRUNTIME1=A: interval timer channel; sends 1, 2, ... until process exit.
    pub fn interval(ms: i64) -> JetReceiver<i64> {
        let (tx, rx) = channel::<i64>();
        let delay = super::jet_task_interval_ms_defaulted(ms);
        super::jet_scheduler_spawn_detached_timer(move || {
            let mut tick = 1i64;
            loop {
                super::jet_scheduler_sleep_ms(delay);
                if !tx.tx.send(tick) {
                    break;
                }
                tick += 1;
            }
        });
        rx
    }

    pub struct JetReceiver<T> {
        inner: super::JetSchedulerChannel<T>,
    }
    // D-TUPLE-DESTRUCT1: the tuple-destructure bind convention clones each
    // extracted field (`(tx, rx) :: channel<T>()` clones `rx` off the
    // synthesized `(Sender<T>, Receiver<T>)` struct, same as `Sender` below). The
    // underlying scheduler channel is `Arc`-backed and already supports concurrent
    // receivers (the same substrate races multiple receive arms
    // against), so cloning a `Receiver` is a cheap, sound pointer copy — not a
    // single-consumer `std::sync::mpsc::Receiver`.
    impl<T> Clone for JetReceiver<T> {
        fn clone(&self) -> Self {
            JetReceiver {
                inner: self.inner.clone(),
            }
        }
    }
    impl<T: Send> JetReceiver<T> {
        pub fn receive(&self) -> Result<T, Closed> {
            // D-CANCELMODEL1=C: cancellation is handled preemptively inside
            // `inner.receive()` — a cancelled recv unwinds at the wait point rather
            // than returning a cooperative `Closed`. No pre-check sentinel here.
            // Deadline uses the same raise door; cleanup must defer it too.
            if !super::jet_scheduler_shielded() {
                if let Some(remaining) = super::jet_deadline_remaining_ms() {
                    if remaining <= 0 {
                        super::jet_deadline_exceeded("channel receive");
                    }
                }
            }
            match self.inner.receive() {
                Some(v) => {
                    super::jet_deadline_check("channel receive");
                    Ok(v)
                }
                None => Err(Closed::Closed),
            }
        }

        /// Explicitly close the channel (wakes waiters); same as dropping the last sender.
        pub fn close(&self) {
            self.inner.close();
        }
    }

    pub struct JetSender<T> {
        tx: super::JetSchedulerSender<T>,
    }
    impl<T: Send> JetSender<T> {
        pub fn send(&self, value: T) {
            let _ = self.tx.send(value);
        }

        /// Explicitly close the channel from the send side.
        pub fn close(&self) {
            self.tx.close();
        }
    }
    impl<T> Clone for JetSender<T> {
        fn clone(&self) -> Self {
            JetSender {
                tx: self.tx.clone(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Closed {
        Closed
    }

    /// D-SHARED-REVISION1=A: a failed optimistic publication has a typed
    /// reason. Stale tickets are not failures: they return `Ok(false)` so a
    /// caller can distinguish a lost race from a malformed ticket or an
    /// exhausted generation.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum JetSharedRevisionError {
        WrongOwner,
        GenerationExhausted,
    }

    impl super::JetShow for JetSharedRevisionError {
        fn jet_show(&self) -> String {
            match self {
                Self::WrongOwner => "SharedRevisionError.WrongOwner".to_string(),
                Self::GenerationExhausted => {
                    "SharedRevisionError.GenerationExhausted".to_string()
                }
            }
        }
    }
    impl super::JetDisplay for JetSharedRevisionError {
        fn jet_display(&self) -> String {
            self.jet_show()
        }
    }
    impl super::JetDebug for JetSharedRevisionError {
        fn jet_debug(&self) -> String {
            self.jet_show()
        }
    }

    // D-SHARED-REVISION1=A: the owner and generation are intentionally private.
    // The copied projection is ordinary data, but the ticket cannot be forged,
    // decoded, cloned, or reused after it is moved to `try_replace`.
    pub struct JetSharedSnapshot<T: 'static, U> {
        owner: std::sync::Arc<JetSharedCell<T>>,
        revision: u64,
        valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
        consumed: std::sync::Arc<std::sync::atomic::AtomicBool>,
        value: U,
    }

    // JET_VETTED_UNSAFE_BEGIN: jet_shared_cell
    // AUDIT: JetSharedCell's UnsafeCell, permit-gated Send/Sync, and raw
    // projections implement the shared-cell lease. Safe Rust cannot express
    // that a runtime read or exclusive permit protects a dynamically selected
    // field projection across the callback. The invariant is that every
    // dereference holds its matching permit, projections are sema-validated
    // disjoint fields, and a revision ticket is consumed at most once.
    // Violating those conditions would create an aliasing/data race or a
    // dangling reference and is undefined behavior.

    // jet:shared-guard-internal-begin
    struct JetSharedCell<T> {
        protocol: std::sync::Arc<crate::JetSharedProtocol>,
        scalar: Option<crate::JetSharedAtomic>,
        revision: std::sync::atomic::AtomicU64,
        value: std::cell::UnsafeCell<T>,
    }
    // jet:shared-guard-internal-end

    // jet:shared-guard-internal-begin
    fn jet_shared_scalar_read_as<T: 'static, U: Copy + 'static>(value: &T) -> U {
        debug_assert_eq!(
            std::any::TypeId::of::<T>(),
            std::any::TypeId::of::<U>()
        );
        // SAFETY: callers select U only after matching T's TypeId. Both
        // values therefore have the same size and valid representation.
        unsafe { std::mem::transmute_copy(value) }
    }

    fn jet_shared_scalar_write_as<T: 'static, U: Copy + 'static>(value: U) -> T {
        debug_assert_eq!(
            std::any::TypeId::of::<T>(),
            std::any::TypeId::of::<U>()
        );
        // SAFETY: callers select U only after matching T's TypeId. Both
        // values therefore have the same size and valid representation.
        unsafe { std::mem::transmute_copy(&value) }
    }

    fn jet_shared_scalar_bits<T: 'static>(
        value: &T,
        kind: crate::JetSharedScalarKind,
    ) -> u64 {
        match kind {
            crate::JetSharedScalarKind::I8 => {
                jet_shared_scalar_read_as::<T, i8>(value) as i64 as u64
            }
            crate::JetSharedScalarKind::U8 => {
                jet_shared_scalar_read_as::<T, u8>(value) as u64
            }
            crate::JetSharedScalarKind::I16 => {
                jet_shared_scalar_read_as::<T, i16>(value) as i64 as u64
            }
            crate::JetSharedScalarKind::U16 => {
                jet_shared_scalar_read_as::<T, u16>(value) as u64
            }
            crate::JetSharedScalarKind::I32 => {
                jet_shared_scalar_read_as::<T, i32>(value) as i64 as u64
            }
            crate::JetSharedScalarKind::U32 => {
                jet_shared_scalar_read_as::<T, u32>(value) as u64
            }
            crate::JetSharedScalarKind::I64 => {
                jet_shared_scalar_read_as::<T, i64>(value) as u64
            }
            crate::JetSharedScalarKind::U64 => {
                jet_shared_scalar_read_as::<T, u64>(value)
            }
            crate::JetSharedScalarKind::F32 => {
                jet_shared_scalar_read_as::<T, f32>(value).to_bits() as u64
            }
            crate::JetSharedScalarKind::F64 => {
                jet_shared_scalar_read_as::<T, f64>(value).to_bits()
            }
            crate::JetSharedScalarKind::Bool => {
                if jet_shared_scalar_read_as::<T, bool>(value) {
                    1
                } else {
                    0
                }
            }
            crate::JetSharedScalarKind::Char => {
                jet_shared_scalar_read_as::<T, char>(value) as u32 as u64
            }
        }
    }

    fn jet_shared_scalar_from_bits<T: 'static>(
        bits: u64,
        kind: crate::JetSharedScalarKind,
    ) -> T {
        match kind {
            crate::JetSharedScalarKind::I8 => {
                jet_shared_scalar_write_as::<T, i8>(bits as i8)
            }
            crate::JetSharedScalarKind::U8 => {
                jet_shared_scalar_write_as::<T, u8>(bits as u8)
            }
            crate::JetSharedScalarKind::I16 => {
                jet_shared_scalar_write_as::<T, i16>(bits as i16)
            }
            crate::JetSharedScalarKind::U16 => {
                jet_shared_scalar_write_as::<T, u16>(bits as u16)
            }
            crate::JetSharedScalarKind::I32 => {
                jet_shared_scalar_write_as::<T, i32>(bits as i32)
            }
            crate::JetSharedScalarKind::U32 => {
                jet_shared_scalar_write_as::<T, u32>(bits as u32)
            }
            crate::JetSharedScalarKind::I64 => {
                jet_shared_scalar_write_as::<T, i64>(bits as i64)
            }
            crate::JetSharedScalarKind::U64 => {
                jet_shared_scalar_write_as::<T, u64>(bits)
            }
            crate::JetSharedScalarKind::F32 => {
                jet_shared_scalar_write_as::<T, f32>(f32::from_bits(bits as u32))
            }
            crate::JetSharedScalarKind::F64 => {
                jet_shared_scalar_write_as::<T, f64>(f64::from_bits(bits))
            }
            crate::JetSharedScalarKind::Bool => {
                jet_shared_scalar_write_as::<T, bool>(bits != 0)
            }
            crate::JetSharedScalarKind::Char => {
                let value = crate::jet_shared_guard_validate_char(bits as i32)
                    .expect(crate::JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED);
                jet_shared_scalar_write_as::<T, char>(value)
            }
        }
    }
    // jet:shared-guard-internal-end

    // jet:shared-guard-internal-begin
    impl<T: 'static> JetSharedCell<T> {
        fn scalar_snapshot(&self) -> Option<T> {
            self.scalar.as_ref().map(|scalar| {
                jet_shared_scalar_from_bits(scalar.load(), scalar.kind())
            })
        }

        fn store_scalar_value(&self, value: &T) {
            if let Some(scalar) = self.scalar.as_ref() {
                scalar.store(jet_shared_scalar_bits(value, scalar.kind()));
            }
        }

        fn with_read<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&T) -> R,
        {
            let _permit = crate::jet_shared_acquire(&self.protocol, false, || false)
                .expect("uncancelled Shared read acquires");
            if let Some(value) = self.scalar_snapshot() {
                return f(&value);
            }
            // jet:shared-guard-internal-begin
            // SAFETY: the read permit is held through the callback.
            f(unsafe { &*self.value.get() })
            // jet:shared-guard-internal-end
        }

        fn next_revision(&self) -> Result<u64, JetSharedRevisionError> {
            let current = self
                .revision
                .load(std::sync::atomic::Ordering::Acquire);
            current
                .checked_add(1)
                .ok_or(JetSharedRevisionError::GenerationExhausted)
        }

        fn commit_revision(&self) {
            let next = self
                .next_revision()
                .expect("Shared revision generation exhausted");
            self.revision
                .store(next, std::sync::atomic::Ordering::Release);
        }

        fn commit_guard(&self) {
            let next = self
                .next_revision()
                .expect("Shared revision generation exhausted");
            if self.scalar.is_some() {
                // SAFETY: the caller still holds the editable guard permit
                // while committing the guard's shadow value.
                let value = unsafe { &*self.value.get() };
                self.store_scalar_value(value);
            }
            self.revision
                .store(next, std::sync::atomic::Ordering::Release);
        }

        fn refresh_scalar_shadow(&self) {
            if let Some(value) = self.scalar_snapshot() {
                // SAFETY: the caller holds this cell's guard permit while
                // synchronizing the shadow used by the guard projection.
                unsafe { *self.value.get() = value };
            }
        }
    }
    // jet:shared-guard-internal-end

    // jet:shared-guard-internal-begin
    // SAFETY: JetSharedProtocol grants either shared read permits or one
    // exclusive edit permit before any structured payload reference is
    // created. Scalar callback paths use the atomic carrier and do not touch
    // the shadow.
    unsafe impl<T: Send> Send for JetSharedCell<T> {}
    unsafe impl<T: Send + Sync> Sync for JetSharedCell<T> {}
    // jet:shared-guard-internal-end

    pub struct JetShared<T>(std::sync::Arc<JetSharedCell<T>>);
    impl<T: 'static> JetShared<T> {
        pub fn new(value: T) -> Self {
            let kind = crate::jet_shared_scalar_kind::<T>();
            let scalar = kind.map(|kind| {
                crate::JetSharedAtomic::new(kind, jet_shared_scalar_bits(&value, kind))
            });
            JetShared(std::sync::Arc::new(JetSharedCell {
                protocol: crate::JetSharedProtocol::new(),
                scalar,
                revision: std::sync::atomic::AtomicU64::new(0),
                // jet:shared-guard-internal-begin
                value: std::cell::UnsafeCell::new(value),
                // jet:shared-guard-internal-end
            }))
        }

        /// Compatibility surface for a Cell binding promoted across an HTTP
        /// handler boundary. The carrier is still the canonical lock-ordered
        /// Shared value; these methods preserve the ordinary Cell operations
        /// while the checked binding fact changes the storage type.
        pub fn get(&self) -> T
        where
            T: Clone,
        {
            self.read(Clone::clone)
        }

        pub fn set(&self, value: T) {
            let _permit = crate::jet_shared_acquire(&self.0.protocol, true, || false)
                .expect("uncancelled Shared set acquires");
            let next = self
                .0
                .next_revision()
                .expect("Shared revision generation exhausted");
            if self.0.scalar.is_some() {
                self.0.store_scalar_value(&value);
            } else {
                // jet:shared-guard-internal-begin
                // SAFETY: the exclusive permit is held for the whole write.
                unsafe { *self.0.value.get() = value };
                // jet:shared-guard-internal-end
            }
            self.0
                .revision
                .store(next, std::sync::atomic::Ordering::Release);
        }

        pub fn replace(&self, value: T) -> T {
            let _permit = crate::jet_shared_acquire(&self.0.protocol, true, || false)
                .expect("uncancelled Shared replace acquires");
            let next = self
                .0
                .next_revision()
                .expect("Shared revision generation exhausted");
            let old = if let Some(scalar) = self.0.scalar.as_ref() {
                let old = scalar.swap(jet_shared_scalar_bits(&value, scalar.kind()));
                jet_shared_scalar_from_bits(old, scalar.kind())
            } else {
                // jet:shared-guard-internal-begin
                // SAFETY: the exclusive permit is held for the whole replace.
                unsafe { std::mem::replace(&mut *self.0.value.get(), value) }
                // jet:shared-guard-internal-end
            };
            self.0
                .revision
                .store(next, std::sync::atomic::Ordering::Release);
            old
        }

        pub fn read<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&T) -> R,
        {
            self.0.with_read(f)
        }
        /// Return the canonical publication revision without copying the value.
        ///
        /// Readers that need an optimistic ticket must still use `capture`; this
        /// accessor is metadata only and deliberately does not create a second
        /// revision mechanism in callers.
        pub(crate) fn revision(&self) -> u64 {
            self.0
                .revision
                .load(std::sync::atomic::Ordering::Acquire)
        }


        /// Capture one value/revision pair under the Shared read permit.
        pub fn capture(&self) -> JetSharedSnapshot<T, T>
        where
            T: Clone,
        {
            self.capture_with(Clone::clone)
        }

        /// Capture a pure owned projection under the same read permit.
        pub fn capture_with<F, U>(&self, project: F) -> JetSharedSnapshot<T, U>
        where
            F: FnOnce(&T) -> U,
        {
            let owner = self.0.clone();
            let valid = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let consumed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (revision, value) = owner.with_read(|value| {
                (
                    owner
                        .revision
                        .load(std::sync::atomic::Ordering::Acquire),
                    project(value),
                )
            });
            JetSharedSnapshot {
                owner,
                revision,
                valid,
                consumed,
                value,
            }
        }

        /// Transaction-local capture. Reads the current working value when
        /// this participant has a staged edit, and predicts the one revision
        /// that the outermost commit will publish. Abort invalidates the
        /// ticket; a later local write invalidates earlier tickets.
        pub fn capture_txn<F, U>(
            &self,
            stm: &mut super::jet_stm::Guard,
            project: F,
        ) -> JetSharedSnapshot<T, U>
        where
            F: FnOnce(&T) -> U,
        {
            let protocol = self.0.protocol.clone();
            stm.touch(protocol.clone());
            let snapshot = if let Some(staged) = stm.staged_value::<T>(&protocol) {
                let owner = self.0.clone();
                let valid = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                let consumed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let revision = stm
                    .snapshot_revision(
                        &protocol,
                        owner.revision.load(std::sync::atomic::Ordering::Acquire),
                    )
                    .expect("SharedRevisionError.GenerationExhausted");
                let value = project(&staged.borrow());
                JetSharedSnapshot {
                    owner,
                    revision,
                    valid,
                    consumed,
                    value,
                }
            } else {
                self.capture_with(project)
            };
            stm.record_snapshot(protocol, snapshot.valid.clone());
            snapshot
        }

        /// Consume an owner-bound ticket. The write and revision comparison
        /// happen while holding the same exclusive permit, so a race has one
        /// winner and a stale ticket never mutates the payload.
        pub fn try_replace<U>(
            &self,
            snapshot: JetSharedSnapshot<T, U>,
            value: T,
        ) -> Result<bool, JetSharedRevisionError> {
            let _permit = crate::jet_shared_acquire(&self.0.protocol, true, || false)
                .expect("uncancelled Shared try_replace acquires");
            if !std::sync::Arc::ptr_eq(&self.0, &snapshot.owner) {
                return Err(JetSharedRevisionError::WrongOwner);
            }
            if !snapshot
                .valid
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Ok(false);
            }
            let current = self
                .0
                .revision
                .load(std::sync::atomic::Ordering::Acquire);
            if current != snapshot.revision {
                return Ok(false);
            }
            let next = self.0.next_revision()?;
            if snapshot
                .consumed
                .swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                return Ok(false);
            }
            if self.0.scalar.is_some() {
                self.0.store_scalar_value(&value);
            } else {
                // jet:shared-guard-internal-begin
                // SAFETY: the exclusive permit is held for the whole write.
                unsafe { *self.0.value.get() = value };
                // jet:shared-guard-internal-end
            }
            self.0
                .revision
                .store(next, std::sync::atomic::Ordering::Release);
            snapshot
                .valid
                .store(false, std::sync::atomic::Ordering::Release);
            Ok(true)
        }

        /// Register a read participant with the current Shared transaction,
        /// then read its transaction-local working value when one exists.
        pub fn read_txn<F, R>(&self, stm: &mut super::jet_stm::Guard, f: F) -> R
        where
            F: FnOnce(&T) -> R,
        {
            let protocol = self.0.protocol.clone();
            stm.touch(protocol.clone());
            if let Some(staged) = stm.staged_value::<T>(&protocol) {
                f(&staged.borrow())
            } else {
                self.read(f)
            }
        }


        pub fn edit<F, R>(&self, f: F) -> R
        where
            F: FnOnce(&mut T) -> R,
        {
            let _permit = crate::jet_shared_acquire(&self.0.protocol, true, || false)
                .expect("uncancelled Shared edit acquires");
            let next = self
                .0
                .next_revision()
                .expect("Shared revision generation exhausted");
            if self.0.scalar.is_some() {
                let mut value = self
                    .0
                    .scalar_snapshot()
                    .expect("scalar Shared payload disappeared");
                let result = f(&mut value);
                self.0.store_scalar_value(&value);
                self.0
                    .revision
                    .store(next, std::sync::atomic::Ordering::Release);
                return result;
            }
            // jet:shared-guard-internal-begin
            // SAFETY: the exclusive permit is held through the callback.
            let result = f(unsafe { &mut *self.0.value.get() });
            // jet:shared-guard-internal-end
            self.0
                .revision
                .store(next, std::sync::atomic::Ordering::Release);
            result
        }

        pub fn guard_read(&self) -> JetSharedGuard<T> {
            JetSharedGuard::read(self.0.clone())
        }
        pub fn guard_edit(&self) -> JetSharedGuard<T> {
            JetSharedGuard::edit(self.0.clone())
        }

        // D-STM1=A (ratified 2026-07-12, card #506): the Shared plane of
        // `#Transact`. Edits run once against a private transaction-local
        // working value; commit publishes that value and one revision.
        pub fn edit_txn<F>(&self, stm: &mut super::jet_stm::Guard, f: F)
        where
            F: FnOnce(&mut T) + 'static,
            T: Clone + 'static,
        {
            let cell = self.0.clone();
            let protocol = cell.protocol.clone();
            let staged = stm.stage_value(protocol.clone(), || cell.with_read(Clone::clone));
            stm.mark_write(protocol.clone());
            f(&mut staged.borrow_mut());
            let commit_cell = cell.clone();
            let commit_staged = staged.clone();
            stm.record_edit_with_commit(
                protocol,
                Box::new(|| {}),
                Box::new(move || {
                    let value = commit_staged.borrow().clone();
                    let next = commit_cell
                        .next_revision()
                        .expect("Shared revision generation exhausted");
                    if commit_cell.scalar.is_some() {
                        commit_cell.store_scalar_value(&value);
                    } else {
                        // jet:shared-guard-internal-begin
                        // SAFETY: the transaction commit owns the participant
                        // permit while publishing the staged value.
                        unsafe { *commit_cell.value.get() = value };
                        // jet:shared-guard-internal-end
                    }
                    commit_cell
                        .revision
                        .store(next, std::sync::atomic::Ordering::Release);
                }),
            );
        }
    }
    impl<T: 'static, U: Clone> JetSharedSnapshot<T, U> {
        /// Return a fresh ordinary copy of the captured projection. This
        /// exposes no owner or revision authority.
        pub fn value(&self) -> U {
            self.value.clone()
        }
        /// Return the captured canonical revision for an internal atomic
        /// publication handoff.
        pub(crate) fn revision(&self) -> u64 {
            self.revision
        }
    }

    impl<T: 'static, U: super::JetShow> super::JetShow for JetSharedSnapshot<T, U> {
        fn jet_show(&self) -> String {
            format!("SharedSnapshot({})", self.value.jet_show())
        }
    }
    impl<T: 'static, U: super::JetShow> super::JetDisplay for JetSharedSnapshot<T, U> {
        fn jet_display(&self) -> String {
            self.jet_show()
        }
    }
    impl<T: 'static, U: super::JetShow> super::JetDebug for JetSharedSnapshot<T, U> {
        fn jet_debug(&self) -> String {
            self.jet_show()
        }
    }

    impl<T> Clone for JetShared<T> {
        fn clone(&self) -> Self {
            JetShared(self.0.clone())
        }
    }
    impl<T: 'static> JetShared<T> {
        /// D-SHARED-CYCLE1=C: expert weak edge. Strong Shared cycles are
        /// rejected in sema (E0221); intentional graphs use Weak back-edges.
        pub fn downgrade(&self) -> JetSharedWeak<T> {
            JetSharedWeak(std::sync::Arc::downgrade(&self.0))
        }
        pub fn strong_count(&self) -> i64 {
            std::sync::Arc::strong_count(&self.0) as i64
        }
    }
    // D-MEM1 S6: an opaque-handle placeholder, mirroring `JetTCPListener`'s
    // `JetShow` (Prelude/CoreLib.rs) — `Shared<T>`'s point is the lock-guarded
    // access methods, not a direct print of the handle itself.
    impl<T> super::JetShow for JetShared<T> {
        fn jet_show(&self) -> String {
            "Shared(..)".to_string()
        }
    }

    /// D-SHARED-CYCLE1=C: weak Shared handle (`Shared.Weak<T>`). Does not keep
    /// the payload alive; `upgrade` restores a strong handle when still live.
    pub struct JetSharedWeak<T>(std::sync::Weak<JetSharedCell<T>>);
    impl<T: 'static> JetSharedWeak<T> {
        pub fn upgrade(&self) -> JetOutcome<JetShared<T>, JetAbsent> {
            jet_outcome_of(self.0.upgrade().map(JetShared))
        }
    }
    impl<T> Clone for JetSharedWeak<T> {
        fn clone(&self) -> Self {
            JetSharedWeak(self.0.clone())
        }
    }
    impl<T> PartialEq for JetSharedWeak<T> {
        fn eq(&self, other: &Self) -> bool {
            self.0.ptr_eq(&other.0)
        }
    }
    impl<T> Eq for JetSharedWeak<T> {}
    impl<T> std::fmt::Debug for JetSharedWeak<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Shared.Weak(..)")
        }
    }
    impl<T> super::JetShow for JetSharedWeak<T> {
        fn jet_show(&self) -> String {
            "Shared.Weak(..)".to_string()
        }
    }

    /// A guard keeps the active protocol projection state separately from the
    /// root lease. Mapping consumes that state, while every mapped/split child
    /// retains the same root permit and scalar-shadow commit owner.
    pub struct JetSharedGuard<T: 'static> {
        state: std::sync::Arc<crate::JetSharedGuardState>,
        lease: std::rc::Rc<dyn JetSharedLease>,
        project: std::rc::Rc<dyn Fn(*mut ()) -> *mut T>,
        editable: bool,
    }

    trait JetSharedLease {
        fn root_ptr(&self) -> *mut ();
    }

    struct JetSharedRootLease<T: 'static> {
        permit: std::sync::Arc<crate::JetSharedPermit>,
        editable: bool,
        cell: std::sync::Arc<JetSharedCell<T>>,
    }

    impl<T: 'static> JetSharedLease for JetSharedRootLease<T> {
        fn root_ptr(&self) -> *mut () {
            assert!(self.permit.held(), "SharedGuard lease is released");
            self.cell.value.get().cast::<()>()
        }
    }

    impl<T: 'static> Drop for JetSharedRootLease<T> {
        fn drop(&mut self) {
            if self.editable && self.permit.held() {
                self.cell.commit_guard();
            }
        }
    }

    impl<T: 'static> JetSharedGuard<T> {
        fn read(cell: std::sync::Arc<JetSharedCell<T>>) -> Self {
            let state = crate::jet_shared_guard_acquire(&cell.protocol, false, || false)
                .unwrap_or_else(|| {
                    super::jet_runtime_stop_with_context(
                        "E3001",
                        "",
                        0,
                        "",
                        "",
                        crate::JET_SHARED_GUARD_INVALID,
                    )
                });
            cell.refresh_scalar_shadow();
            let permit = state.permit_arc();
            Self {
                state,
                lease: std::rc::Rc::new(JetSharedRootLease {
                    permit,
                    editable: false,
                    cell,
                }),
                project: std::rc::Rc::new(|root| root.cast::<T>()),
                editable: false,
            }
        }

        fn edit(cell: std::sync::Arc<JetSharedCell<T>>) -> Self {
            let state = crate::jet_shared_guard_acquire(&cell.protocol, true, || false)
                .unwrap_or_else(|| {
                    super::jet_runtime_stop_with_context(
                        "E3001",
                        "",
                        0,
                        "",
                        "",
                        crate::JET_SHARED_GUARD_INVALID,
                    )
                });
            cell.refresh_scalar_shadow();
            let permit = state.permit_arc();
            Self {
                state,
                lease: std::rc::Rc::new(JetSharedRootLease {
                    permit,
                    editable: true,
                    cell,
                }),
                project: std::rc::Rc::new(|root| root.cast::<T>()),
                editable: true,
            }
        }

        pub fn map_read<U: 'static, F>(self, field: i64, project: F) -> JetSharedGuard<U>
        where
            F: FnOnce(&T) -> &U,
        {
            let mapped = crate::jet_shared_guard_map(&self.state, field, false).unwrap_or_else(
                |message| {
                    super::jet_runtime_stop_with_context(
                        "E3001",
                        "",
                        0,
                        "",
                        "",
                        message,
                    )
                },
            );
            let root = (self.project)(self.lease.root_ptr());
            // jet:shared-guard-internal-begin
            // SAFETY: the lease keeps the root alive and holds a read or edit
            // permit; sema proved this closure is one stored-field projection.
            let projected = unsafe { project(&*root) as *const U as *mut U };
            // jet:shared-guard-internal-end
            JetSharedGuard {
                state: mapped,
                lease: self.lease.clone(),
                project: std::rc::Rc::new(move |_| projected),
                editable: false,
            }
        }

        pub fn map_edit<U: 'static, F>(self, field: i64, project: F) -> JetSharedGuard<U>
        where
            F: FnOnce(&mut T) -> &mut U,
        {
            if !self.editable {
                super::jet_runtime_stop_with_context(
                    "E3001",
                    "",
                    0,
                    "",
                    "",
                    crate::JET_SHARED_GUARD_EDIT_REQUIRED,
                );
            }
            let mapped = crate::jet_shared_guard_map(&self.state, field, true).unwrap_or_else(
                |message| {
                    super::jet_runtime_stop_with_context(
                        "E3001",
                        "",
                        0,
                        "",
                        "",
                        message,
                    )
                },
            );
            let root = (self.project)(self.lease.root_ptr());
            // jet:shared-guard-internal-begin
            // SAFETY: the editable lease holds the exclusive permit, and sema
            // proved this closure is one stored-field projection.
            let projected = unsafe { project(&mut *root) as *mut U };
            // jet:shared-guard-internal-end
            JetSharedGuard {
                state: mapped,
                lease: self.lease.clone(),
                project: std::rc::Rc::new(move |_| projected),
                editable: true,
            }
        }

        pub fn split_read<A: 'static, B: 'static, F>(
            self,
            first_field: i64,
            second_field: i64,
            project: F,
        ) -> (JetSharedGuard<A>, JetSharedGuard<B>)
        where
            F: FnOnce(&T) -> (&A, &B),
        {
            let (first_state, second_state) =
                crate::jet_shared_guard_split(&self.state, first_field, second_field, false)
                    .unwrap_or_else(|message| {
                        super::jet_runtime_stop_with_context(
                            "E3001",
                            "",
                            0,
                            "",
                            "",
                            message,
                        )
                    });
            let root = (self.project)(self.lease.root_ptr());
            // jet:shared-guard-internal-begin
            // SAFETY: the lease holds a read or edit permit. Sema proved both
            // projections are stored and disjoint.
            let (first, second) = unsafe { project(&*root) };
            let first = first as *const A as *mut A;
            let second = second as *const B as *mut B;
            // jet:shared-guard-internal-end
            (
                JetSharedGuard {
                    state: first_state,
                    lease: self.lease.clone(),
                    project: std::rc::Rc::new(move |_| first),
                    editable: false,
                },
                JetSharedGuard {
                    state: second_state,
                    lease: self.lease.clone(),
                    project: std::rc::Rc::new(move |_| second),
                    editable: false,
                },
            )
        }

        pub fn split_edit<A: 'static, B: 'static, F>(
            self,
            first_field: i64,
            second_field: i64,
            project: F,
        ) -> (JetSharedGuard<A>, JetSharedGuard<B>)
        where
            F: FnOnce(&mut T) -> (&mut A, &mut B),
        {
            if !self.editable {
                super::jet_runtime_stop_with_context(
                    "E3001",
                    "",
                    0,
                    "",
                    "",
                    crate::JET_SHARED_GUARD_EDIT_REQUIRED,
                );
            }
            let (first_state, second_state) =
                crate::jet_shared_guard_split(&self.state, first_field, second_field, true)
                    .unwrap_or_else(|message| {
                        super::jet_runtime_stop_with_context(
                            "E3001",
                            "",
                            0,
                            "",
                            "",
                            message,
                        )
                    });
            let root = (self.project)(self.lease.root_ptr());
            // jet:shared-guard-internal-begin
            // SAFETY: the editable lease holds the exclusive permit. One
            // closure creates both references, so Rust verifies disjointness.
            let (first, second) = unsafe { project(&mut *root) };
            let first = first as *mut A;
            let second = second as *mut B;
            // jet:shared-guard-internal-end
            (
                JetSharedGuard {
                    state: first_state,
                    lease: self.lease.clone(),
                    project: std::rc::Rc::new(move |_| first),
                    editable: true,
                },
                JetSharedGuard {
                    state: second_state,
                    lease: self.lease.clone(),
                    project: std::rc::Rc::new(move |_| second),
                    editable: true,
                },
            )
        }

        pub fn wait<F>(&mut self, condition: &JetCondition, ready: F) -> Result<(), String>
        where
            F: Fn(&T) -> bool,
        {
            let state = self.state.clone();
            if !state.held() {
                return Err(crate::JET_SHARED_GUARD_INVALID.to_string());
            }
            crate::jet_shared_guard_require_edit_capability(self.editable, state.permit())
                .map_err(|message| message.to_string())?;
            loop {
                if ready(self) {
                    return Ok(());
                }
                let waiter: std::sync::Arc<dyn crate::JetConditionWaiter> =
                    std::sync::Arc::new(JetSchedulerConditionWaiter {
                        slot: super::ParkSlot::new(),
                    });
                match crate::jet_shared_guard_wait_once(
                    Some(state.as_ref()),
                    Some(&condition.inner),
                    waiter,
                ) {
                    Ok(()) => {}
                    Err(error) => return Err(error.message().to_string()),
                }
            }
        }
    }

    impl<T: 'static> std::ops::Deref for JetSharedGuard<T> {
        type Target = T;
        fn deref(&self) -> &T {
            let value = (self.project)(self.lease.root_ptr());
            // jet:shared-guard-internal-begin
            // SAFETY: the lease holds the matching read or write lock while
            // this reference exists; the projection is sema-validated.
            unsafe { &*value }
            // jet:shared-guard-internal-end
        }
    }

    impl<T: 'static> std::ops::DerefMut for JetSharedGuard<T> {
        fn deref_mut(&mut self) -> &mut T {
            assert!(self.editable, "read SharedGuard used for edit");
            let value = (self.project)(self.lease.root_ptr());
            // jet:shared-guard-internal-begin
            // SAFETY: an editable lease holds the exclusive lock.
            unsafe { &mut *value }
            // jet:shared-guard-internal-end
        }
    }
    // JET_VETTED_UNSAFE_END: jet_shared_cell

    struct JetSchedulerConditionWaiter {
        slot: std::sync::Arc<super::ParkSlot>,
    }

    impl crate::JetConditionWaiter for JetSchedulerConditionWaiter {
        fn park(&self) -> Result<(), ()> {
            super::jet_scheduler_yield("Shared condition", &self.slot, None);
            Ok(())
        }

        fn wake(&self) {
            super::jet_scheduler_wake(&self.slot);
        }

        fn interrupted(&self) -> bool {
            super::jet_scheduler_wait_point_interrupted()
        }
    }

    #[derive(Clone)]
    pub struct JetCondition {
        inner: std::sync::Arc<crate::JetConditionProtocol>,
    }

    impl JetCondition {
        pub fn new() -> Self {
            Self {
                inner: crate::JetConditionProtocol::new(),
            }
        }

        pub fn notify_one(&self) {
            crate::jet_shared_condition_notify_one(&self.inner);
        }

        pub fn notify_all(&self) {
            crate::jet_shared_condition_notify_all(&self.inner);
        }
    }

    impl<T> super::JetShow for JetCell<T> {
        fn jet_show(&self) -> String {
            "Cell(..)".to_string()
        }
    }

    // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` — a generational arena. `Id<T>` is
    // a lightweight index+generation handle: plain data, `Copy`, comparable,
    // regardless of whether `T` itself is (it never touches `T` at runtime —
    // hand-written impls below, not `#[derive]`, so no `T: Copy`/`Clone`/`Eq`
    // bound leaks onto every `Id<T>`).
    enum JetPoolSlot<T> {
        Occupied(u32, T),
        Vacant(u32),
    }

    pub struct JetPool<T> {
        slots: Vec<JetPoolSlot<T>>,
        free: Vec<usize>,
    }

    impl<T> JetPool<T> {
        pub fn new() -> Self {
            JetPool {
                slots: Vec::new(),
                free: Vec::new(),
            }
        }

        pub fn add(&mut self, value: T) -> JetId<T> {
            if let Some(idx) = self.free.pop() {
                let gen = match self.slots[idx] {
                    JetPoolSlot::Vacant(g) => g,
                    JetPoolSlot::Occupied(..) => {
                        unreachable!("a free-list slot is always Vacant")
                    }
                };
                self.slots[idx] = JetPoolSlot::Occupied(gen, value);
                return JetId::new(idx as u32, gen);
            }
            let idx = self.slots.len();
            self.slots.push(JetPoolSlot::Occupied(0, value));
            JetId::new(idx as u32, 0)
        }

        /// D-POOLID-API1=A: removes the slot `id` names, bumping its generation
        /// so any other copy of `id` becomes stale — mirrors the Jet optional
        /// carrier convention (a miss returns an absent outcome, not a panic).
        pub fn remove(&mut self, id: JetId<T>) -> JetOutcome<T, JetAbsent> {
            let idx = id.index as usize;
            let occupied = matches!(
                self.slots.get(idx),
                Some(JetPoolSlot::Occupied(g, _)) if *g == id.generation
            );
            if !occupied {
                return Err(JetAbsent);
            }
            let next_gen = id.generation.wrapping_add(1);
            let old = std::mem::replace(&mut self.slots[idx], JetPoolSlot::Vacant(next_gen));
            self.free.push(idx);
            match old {
                JetPoolSlot::Occupied(_, v) => Ok(v),
                JetPoolSlot::Vacant(_) => unreachable!("just checked Occupied above"),
            }
        }

        /// A snapshot `Vec` of every live id — small, `Copy` elements, so a
        /// fresh allocation per call is the simplest correct thing (D-MEM1 S6
        /// notes deferred a genuine lazy `Iterator` as unneeded polish).
        pub fn ids(&self) -> Vec<JetId<T>> {
            self.slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| match s {
                    JetPoolSlot::Occupied(g, _) => Some(JetId::new(i as u32, *g)),
                    JetPoolSlot::Vacant(_) => None,
                })
                .collect()
        }
        /// Return the live value named by `id`, without changing the pool's
        /// ownership. All checked readers and writers share this generation
        /// validation seam.
        pub fn checked_get(&self, id: JetId<T>) -> Option<&T> {
            match self.slots.get(id.index as usize) {
                Some(JetPoolSlot::Occupied(generation, value))
                    if *generation == id.generation =>
                {
                    Some(value)
                }
                _ => None,
            }
        }

        /// Mutable counterpart to [`Self::checked_get`].
        pub fn checked_get_mut(&mut self, id: JetId<T>) -> Option<&mut T> {
            match self.slots.get_mut(id.index as usize) {
                Some(JetPoolSlot::Occupied(generation, value))
                    if *generation == id.generation =>
                {
                    Some(value)
                }
                _ => None,
            }
        }

    }
    // D-MEM1 S6: an opaque-handle placeholder, same rationale as `JetShared`'s
    // `JetShow` just above.
    impl<T> super::JetShow for JetPool<T> {
        fn jet_show(&self) -> String {
            format!("Pool({} slots)", self.slots.len())
        }
    }

    pub struct JetId<T> {
        index: u32,
        generation: u32,
        _marker: std::marker::PhantomData<fn() -> T>,
    }
    impl<T> JetId<T> {
        fn new(index: u32, generation: u32) -> Self {
            JetId {
                index,
                generation,
                _marker: std::marker::PhantomData,
            }
        }

        /// Encode this handle for the native/JIT ABI: low word is index+1,
        /// high word is the generation. Zero is never a valid handle.
        pub fn to_word(self) -> i64 {
            (((u64::from(self.generation)) << 32) | (u64::from(self.index) + 1)) as i64
        }

        /// Decode the native/JIT handle representation.
        pub fn from_word(word: i64) -> Option<Self> {
            let raw = word as u64;
            let index = (raw as u32).checked_sub(1)?;
            Some(Self::new(index, (raw >> 32) as u32))
        }
    }
    impl<T> Clone for JetId<T> {
        fn clone(&self) -> Self {
            *self
        }
    }
    impl<T> Copy for JetId<T> {}
    impl<T> PartialEq for JetId<T> {
        fn eq(&self, other: &Self) -> bool {
            self.index == other.index && self.generation == other.generation
        }
    }
    impl<T> Eq for JetId<T> {}
    impl<T> PartialOrd for JetId<T> {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl<T> Ord for JetId<T> {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            (self.index, self.generation).cmp(&(other.index, other.generation))
        }
    }
    impl<T> super::__jet_Equatable for JetId<T> {
        fn equal(&self, rhs: &Self) -> bool {
            self == rhs
        }
    }
    impl<T> std::hash::Hash for JetId<T> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.index.hash(state);
            self.generation.hash(state);
        }
    }
    impl<T> std::fmt::Debug for JetId<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Id(#{}@{})", self.index, self.generation)
        }
    }
    // D-MEM1 S6: print/interpolation/derived-Debug support — `Id<T>` shows up as
    // an ordinary struct field (`parent: Id<Node>?`), whose containing struct's
    // generated `jet_debug()` calls `.jet_debug()` on every field.
    impl<T> super::JetShow for JetId<T> {
        fn jet_show(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }
    impl<T> super::JetDisplay for JetId<T> {
        fn jet_display(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }
    impl<T> super::JetDebug for JetId<T> {
        fn jet_debug(&self) -> String {
            format!("Id(#{}@{})", self.index, self.generation)
        }
    }

    /// `pool[id]` read (`Expr::Index`, `IndexKind::Pool`): a generation-checked
    /// clone of `T`. Panics naming the stale-access class on a mismatched or
    /// vacant slot, mirroring the array-out-of-bounds panic precedent
    /// (`jet_index_vec`) — a runtime panic, not a new diagnostic code. The
    /// generated caller supplies its Jet function and source-line facts so this
    /// shared Prelude stop renders the same context on every execution tier.
    pub fn jet_pool_get<T: Clone>(
        pool: &JetPool<T>,
        id: JetId<T>,
        file: &str,
        line: u32,
        fn_name: &str,
        src_line: &str,
    ) -> T {
        jet_pool_get_ref(pool, id, file, line, fn_name, src_line).clone()
    }

    /// Borrow the generation-checked stored value without requiring `Clone`.
    pub fn jet_pool_get_ref<'a, T>(
        pool: &'a JetPool<T>,
        id: JetId<T>,
        file: &str,
        line: u32,
        fn_name: &str,
        src_line: &str,
    ) -> &'a T {
        match pool.checked_get(id) {
            Some(value) => value,
            None => super::jet_runtime_stop_with_context(
                "E3001",
                file,
                line,
                fn_name,
                src_line,
                super::jet_pool_stale_message(),
            ),
        }
    }

    /// `pool[id] = v` / `pool[id].field = v` (`LValue::Index` / `LValue::Field`
    /// nested on a `Pool` index): a genuine mutable place, not a value
    /// round-trip — a nested field write edits the real slot. Same stale-access
    /// panic as `jet_pool_get`, with the same source context.
    pub fn jet_pool_get_mut<'a, T>(
        pool: &'a mut JetPool<T>,
        id: JetId<T>,
        file: &str,
        line: u32,
        fn_name: &str,
        src_line: &str,
    ) -> &'a mut T {
        match pool.checked_get_mut(id) {
            Some(value) => value,
            None => super::jet_runtime_stop_with_context(
                "E3001",
                file,
                line,
                fn_name,
                src_line,
                super::jet_pool_stale_message(),
            ),
        }
    }
    /// `pool[id] = value` as an owned-place write. Generation validation is
    /// identical to `jet_pool_get` and `jet_pool_get_mut`; stale or vacant
    /// handles stop through the shared runtime context kernel.
    pub fn jet_pool_set<T>(
        pool: &mut JetPool<T>,
        id: JetId<T>,
        value: T,
        file: &str,
        line: u32,
        fn_name: &str,
        src_line: &str,
    ) {
        if let Some(slot) = pool.checked_get_mut(id) {
            *slot = value;
            return;
        }
        super::jet_runtime_stop_with_context(
            "E3001",
            file,
            line,
            fn_name,
            src_line,
            super::jet_pool_stale_message(),
        );
    }

//! Resident adapters for the shared compute Prelude.
//!
//! The JIT owns only handles and list marshalling. Tensor construction,
//! copying, view bounds, and writes stay in the same Prelude source used by
//! AOT and the interpreter (I9).

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use crate::runtime_host::{jit_callable_parts, bind_jit_callable_handle, JitCallableSlot};
use crate::JitRuntime;
use crate::Marshal::{result_err_msg, result_ok};
use super::Concurrency;

#[allow(dead_code, unused_imports)]
mod semantics {
    use crate::JetShow;
    use jet_foundation::Outcome::jet_list_bounds_message;
    use jet_foundation::StructuralDebug::jet_debug_range;

    fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
        panic!("{message}");
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct JetRange {
        start: i64,
        end: i64,
        exclusive: bool,
    }

    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
    include!("../../jet-codegen/src/Prelude/Core/ViewAccess.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Compute.rs");

    #[cfg(test)]
    mod cpu_simd_tests {
        use super::*;
        use std::time::{Duration, Instant};

        fn detected_backend() -> JetComputeSimdBackend {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                jet_compute_simd_backend_for_features(
                    is_x86_feature_detected!("avx2"),
                    is_x86_feature_detected!("sse2"),
                )
            }
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            {
                jet_compute_simd_backend_for_features(false, false)
            }
        }

        fn sample_lanes(length: usize) -> (Vec<f32>, Vec<f32>) {
            let left = (0..length)
                .map(|index| (index % 37) as f32 * 0.03125 - 0.5)
                .collect();
            let right = (0..length)
                .map(|index| (index % 29) as f32 * 0.0625 - 0.875)
                .collect();
            (left, right)
        }

        fn f32_tensor(shape: Vec<i64>, values: Vec<f32>) -> JetTensor {
            let mut tensor =
                jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu).unwrap();
            tensor.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
            tensor.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
            tensor.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
                .iter()
                .map(|ability| (*ability).to_string())
                .collect();
            jet_compute_validate_tensor(&tensor).unwrap();
            tensor
        }

        fn measure(iterations: usize, operation: impl Fn() -> f32) -> Duration {
            let start = Instant::now();
            let mut total = 0.0_f32;
            for _ in 0..iterations {
                total += std::hint::black_box(operation());
            }
            std::hint::black_box(total);
            start.elapsed()
        }

        #[test]
        fn runtime_dispatch_prefers_avx2_then_sse2_then_scalar() {
            assert_eq!(
                jet_compute_simd_backend_for_features(true, true),
                JetComputeSimdBackend::Avx2
            );
            assert_eq!(
                jet_compute_simd_backend_for_features(false, true),
                JetComputeSimdBackend::Sse2
            );
            assert_eq!(
                jet_compute_simd_backend_for_features(false, false),
                JetComputeSimdBackend::Scalar
            );
            assert_eq!(jet_compute_simd_backend(), detected_backend());
        }

        #[test]
        fn simd_dot_paths_match_scalar_bit_for_bit_including_tail() {
            let (left, right) = sample_lanes(4099);
            let scalar = jet_compute_f32_dot_scalar(&left, &right);
            for backend in [JetComputeSimdBackend::Sse2, JetComputeSimdBackend::Avx2] {
                if !jet_compute_simd_backend_available(backend) {
                    continue;
                }
                let actual = jet_compute_f32_dot(backend, &left, &right).unwrap();
                assert_eq!(
                    actual.to_bits(),
                    scalar.to_bits(),
                    "{} path changed ordered f32 reduction",
                    backend.name()
                );
            }
        }

        #[test]
        fn tiled_matmul_matches_f32_scalar_oracle_bit_for_bit() {
            let left_values = (0..39)
                .map(|index| (index * 7 % 23) as f32 - 11.0)
                .collect::<Vec<_>>();
            let right_values = (0..91)
                .map(|index| (index * 5 % 19) as f32 - 9.0)
                .collect::<Vec<_>>();
            let mut scalar_values = Vec::with_capacity(3 * 7);
            for row in 0..3 {
                for column in 0..7 {
                    let mut sum = 0.0_f32;
                    for inner in 0..13 {
                        sum += left_values[row * 13 + inner] * right_values[inner * 7 + column];
                    }
                    scalar_values.push(f64::from(sum));
                }
            }
            let left = f32_tensor(vec![3, 13], left_values);
            let right = f32_tensor(vec![13, 7], right_values);
            let tiled = jet_compute_matmul_f32_tile(&left, &right).unwrap();
            let tiled_values = jet_compute_tensor_values(&tiled);
            assert_eq!(
                scalar_values
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                tiled_values
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
        }

        #[test]
        fn selected_simd_path_beats_scalar_and_records_speedup() {
            let backend = jet_compute_simd_backend();
            if backend == JetComputeSimdBackend::Scalar {
                eprintln!("SKIP compute_simd_speedup: no runtime SIMD feature");
                return;
            }
            let (left, right) = sample_lanes(1 << 18);
            let scalar = (0..3)
                .map(|_| measure(4, || jet_compute_f32_dot_scalar(&left, &right)))
                .min()
                .unwrap();
            let simd = (0..3)
                .map(|_| measure(4, || jet_compute_f32_dot(backend, &left, &right).unwrap()))
                .min()
                .unwrap();
            assert!(
                simd < scalar,
                "{} SIMD path did not beat scalar: scalar={:?}, simd={:?}",
                backend.name(),
                scalar,
                simd
            );
            let speedup = scalar.as_secs_f64() / simd.as_secs_f64();
            eprintln!(
                "compute_simd_speedup backend={} scalar_ns={} simd_ns={} speedup={speedup:.2}x",
                backend.name(),
                scalar.as_nanos(),
                simd.as_nanos(),
            );
        }
    }

    #[cfg(test)]
    mod metal_tests {
        use super::*;

        fn f32_tensor(shape: Vec<i64>, values: Vec<f32>) -> JetTensor {
            let mut tensor =
                jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu).unwrap();
            tensor.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
            tensor.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
            tensor.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
                .iter()
                .map(|ability| (*ability).to_string())
                .collect();
            jet_compute_validate_tensor(&tensor).unwrap();
            tensor
        }

        #[test]
        fn explicit_metal_is_real_when_available_and_fails_closed_when_not() {
            let cpu = f32_tensor(vec![1], vec![2.0]);
            let result = jet_compute_on_device(&cpu, JetComputeDevice::Metal);
            if !jet_compute_metal::available() {
                let error = result.expect_err("unavailable Metal must not fall back to CPU");
                assert!(
                    matches!(&error, JetComputeError::Unsupported(_) | JetComputeError::Device(_)),
                    "unexpected unavailable Metal error: {error:?}"
                );
                return;
            }

            let metal = result.expect("system Metal device should accept F32 placement");
            assert_eq!(metal.device, JetComputeDevice::Metal);
            assert_eq!(metal.last_placement.backend, METAL_BACKEND);
            assert_eq!(metal.last_placement.profile, CPU_ORACLE_F32_PROFILE);

            let doubled = jet_compute_binary("add", &metal, &metal).unwrap();
            assert_eq!(jet_compute_tensor_values(&doubled), vec![4.0]);
            let downloaded = jet_compute_transfer(&doubled, JetComputeDevice::Cpu).unwrap();
            assert_eq!(jet_compute_tensor_values(&downloaded), vec![4.0]);
            assert_eq!(downloaded.last_transfer.unwrap().bytes, 4);

            let shaped = jet_compute_on_device(
                &f32_tensor(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
                JetComputeDevice::Metal,
            )
            .unwrap();
            let transposed = jet_compute_transpose(&shaped).unwrap();
            let downloaded = jet_compute_transfer(&transposed, JetComputeDevice::Cpu).unwrap();
            assert_eq!(jet_compute_tensor_values(&downloaded), vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
            assert_eq!(downloaded.strides, vec![2, 1]);

            let stream = jet_compute_stream_new_on_device(JetComputeDevice::Metal).unwrap();
            jet_compute_stream_sync(&stream).unwrap();

            let unsupported = jet_compute_det(&metal).unwrap_err();
            assert!(matches!(unsupported, JetComputeError::Unsupported(_)));
        }

        #[test]
        fn metal_mse_vjp_and_jvp_use_the_device_kernel() {
            if !jet_compute_metal::available() {
                eprintln!("SKIP metal autodiff: no system Metal device");
                return;
            }
            let prediction = jet_compute_on_device(
                &f32_tensor(vec![1], vec![3.0]),
                JetComputeDevice::Metal,
            )
            .unwrap();
            let target = jet_compute_on_device(
                &f32_tensor(vec![1], vec![1.0]),
                JetComputeDevice::Metal,
            )
            .unwrap();
            let (tape, traced) = jet_compute_trace_inputs(vec![prediction, target]);
            let loss = jet_compute_mse_loss(&traced[0], &traced[1]).unwrap();
            let state = jet_compute_vjp_begin(loss, tape);
            let seed = jet_compute_gradient_seed(&state).unwrap();
            let gradients = jet_compute_vjp_pull(&state, &seed, &[0, 1]).unwrap();
            assert_eq!(jet_compute_tensor_values(&gradients[0]), vec![4.0]);
            assert_eq!(jet_compute_tensor_values(&gradients[1]), vec![-4.0]);
            let tangent_prediction = jet_compute_on_device(
                &f32_tensor(vec![1], vec![1.0]),
                JetComputeDevice::Metal,
            )
            .unwrap();
            let tangent_target = jet_compute_on_device(
                &f32_tensor(vec![1], vec![0.0]),
                JetComputeDevice::Metal,
            )
            .unwrap();
            let tangent = jet_compute_jvp(&state, vec![tangent_prediction, tangent_target]).unwrap();
            assert_eq!(jet_compute_tensor_values(&tangent), vec![4.0]);
        }
    }

    #[cfg(test)]
    mod cuda_tests {
        use super::*;

        fn f32_tensor(shape: Vec<i64>, values: Vec<f32>) -> JetTensor {
            let mut tensor =
                jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu).unwrap();
            tensor.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
            tensor.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
            tensor.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
                .iter()
                .map(|ability| (*ability).to_string())
                .collect();
            jet_compute_validate_tensor(&tensor).unwrap();
            tensor
        }

        #[test]
        fn explicit_cuda_is_real_when_available_and_fails_closed_when_not() {
            let cpu = f32_tensor(vec![1], vec![2.0]);
            let result = jet_compute_on_device(&cpu, JetComputeDevice::Cuda);
            if !jet_compute_cuda::available() {
                let error = result.expect_err("unavailable CUDA must not fall back to CPU");
                assert!(
                    matches!(&error, JetComputeError::Unsupported(_) | JetComputeError::Device(_)),
                    "unexpected unavailable CUDA error: {error:?}"
                );
                return;
            }

            let cuda = result.expect("CUDA device should accept F32 placement");
            assert_eq!(cuda.device, JetComputeDevice::Cuda);
            assert_eq!(cuda.last_placement.backend, CUDA_BACKEND);
            assert_eq!(cuda.last_placement.profile, CPU_ORACLE_F32_PROFILE);

            let cpu_doubled = jet_compute_binary("add", &cpu, &cpu).unwrap();
            let doubled = jet_compute_binary("add", &cuda, &cuda).unwrap();
            assert_eq!(
                jet_compute_tensor_values(&doubled),
                jet_compute_tensor_values(&cpu_doubled)
            );
            let square_root = jet_compute_unary("sqrt", &doubled).unwrap();
            assert_eq!(jet_compute_tensor_values(&square_root), vec![2.0]);
            let downloaded = jet_compute_transfer(&doubled, JetComputeDevice::Cpu).unwrap();
            assert_eq!(jet_compute_tensor_values(&downloaded), vec![4.0]);
            assert_eq!(downloaded.last_transfer.unwrap().bytes, 4);

            let left = jet_compute_on_device(
                &f32_tensor(vec![1, 2], vec![1.0, 2.0]),
                JetComputeDevice::Cuda,
            )
            .unwrap();
            let right = jet_compute_on_device(
                &f32_tensor(vec![2, 1], vec![3.0, 4.0]),
                JetComputeDevice::Cuda,
            )
            .unwrap();
            let cpu_left = f32_tensor(vec![1, 2], vec![1.0, 2.0]);
            let cpu_right = f32_tensor(vec![2, 1], vec![3.0, 4.0]);
            let cpu_product = jet_compute_matmul(&cpu_left, &cpu_right).unwrap();
            let product = jet_compute_matmul(&left, &right).unwrap();
            assert_eq!(
                jet_compute_tensor_values(&product),
                jet_compute_tensor_values(&cpu_product)
            );
            let reduced = jet_compute_sum_axis(&product, 0).unwrap();
            assert_eq!(jet_compute_tensor_values(&reduced), vec![11.0]);

            let stream = jet_compute_stream_new_on_device(JetComputeDevice::Cuda).unwrap();
            jet_compute_stream_sync(&stream).unwrap();

            let unsupported = jet_compute_det(&cuda).unwrap_err();
            assert!(matches!(unsupported, JetComputeError::Unsupported(_)));
        }

        #[test]
        fn cuda_mse_vjp_and_jvp_use_the_device_kernels() {
            if !jet_compute_cuda::available() {
                eprintln!("SKIP cuda autodiff: no CUDA device");
                return;
            }
            let prediction = jet_compute_on_device(
                &f32_tensor(vec![1], vec![3.0]),
                JetComputeDevice::Cuda,
            )
            .unwrap();
            let target = jet_compute_on_device(
                &f32_tensor(vec![1], vec![1.0]),
                JetComputeDevice::Cuda,
            )
            .unwrap();
            let (tape, traced) = jet_compute_trace_inputs(vec![prediction, target]);
            let loss = jet_compute_mse_loss(&traced[0], &traced[1]).unwrap();
            let state = jet_compute_vjp_begin(loss, tape);
            let seed = jet_compute_gradient_seed(&state).unwrap();
            let gradients = jet_compute_vjp_pull(&state, &seed, &[0, 1]).unwrap();
            assert_eq!(jet_compute_tensor_values(&gradients[0]), vec![4.0]);
            assert_eq!(jet_compute_tensor_values(&gradients[1]), vec![-4.0]);
            let tangent_prediction = jet_compute_on_device(
                &f32_tensor(vec![1], vec![1.0]),
                JetComputeDevice::Cuda,
            )
            .unwrap();
            let tangent_target = jet_compute_on_device(
                &f32_tensor(vec![1], vec![0.0]),
                JetComputeDevice::Cuda,
            )
            .unwrap();
            let tangent = jet_compute_jvp(&state, vec![tangent_prediction, tangent_target]).unwrap();
            assert_eq!(jet_compute_tensor_values(&tangent), vec![4.0]);
        }
    }

    #[cfg(test)]
    mod vulkan_tests {
        use super::*;

        fn f32_tensor(shape: Vec<i64>, values: Vec<f32>) -> JetTensor {
            let mut tensor =
                jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu).unwrap();
            tensor.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
            tensor.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
            tensor.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
                .iter()
                .map(|ability| (*ability).to_string())
                .collect();
            jet_compute_validate_tensor(&tensor).unwrap();
            tensor
        }

        #[test]
        fn explicit_vulkan_is_real_when_available_and_fails_closed_when_not() {
            let cpu = f32_tensor(vec![2], vec![2.0, 3.0]);
            let result = jet_compute_on_device(&cpu, JetComputeDevice::Vulkan);
            if !jet_compute_vulkan::available() {
                let error = result.expect_err("unavailable Vulkan must not fall back to CPU");
                assert!(
                    matches!(&error, JetComputeError::Unsupported(_) | JetComputeError::Device(_)),
                    "unexpected unavailable Vulkan error: {error:?}"
                );
                return;
            }

            let vulkan = result.expect("system Vulkan device should accept F32 placement");
            assert_eq!(vulkan.device, JetComputeDevice::Vulkan);
            assert_eq!(vulkan.last_placement.backend, VULKAN_BACKEND);
            assert_eq!(vulkan.last_placement.profile, CPU_ORACLE_F32_PROFILE);

            let cpu_doubled = jet_compute_binary("add", &cpu, &cpu).unwrap();
            let doubled = jet_compute_binary("add", &vulkan, &vulkan).unwrap();
            assert_eq!(jet_compute_tensor_values(&doubled), vec![4.0, 6.0]);
            assert_eq!(
                jet_compute_tensor_values(&doubled),
                jet_compute_tensor_values(&cpu_doubled)
            );

            let left = jet_compute_on_device(
                &f32_tensor(vec![1, 2], vec![1.0, 2.0]),
                JetComputeDevice::Vulkan,
            )
            .unwrap();
            let right = jet_compute_on_device(
                &f32_tensor(vec![2, 1], vec![3.0, 4.0]),
                JetComputeDevice::Vulkan,
            )
            .unwrap();
            let product = jet_compute_matmul(&left, &right).unwrap();
            assert_eq!(jet_compute_tensor_values(&product), vec![11.0]);
            let reduced = jet_compute_sum_axis(&product, 0).unwrap();
            assert_eq!(jet_compute_tensor_values(&reduced), vec![11.0]);

            let downloaded = jet_compute_transfer(&doubled, JetComputeDevice::Cpu).unwrap();
            assert_eq!(jet_compute_tensor_values(&downloaded), vec![4.0, 6.0]);
            assert_eq!(downloaded.last_transfer.unwrap().bytes, 8);

            let stream = jet_compute_stream_new_on_device(JetComputeDevice::Vulkan).unwrap();
            jet_compute_stream_sync(&stream).unwrap();

            let unsupported = jet_compute_det(&vulkan).unwrap_err();
            assert!(matches!(unsupported, JetComputeError::Unsupported(_)));
        }

        #[test]
        fn vulkan_mse_vjp_and_jvp_use_the_device_kernel() {
            if !jet_compute_vulkan::available() {
                eprintln!("SKIP Vulkan autodiff: no Vulkan device");
                return;
            }
            let prediction = jet_compute_on_device(
                &f32_tensor(vec![1], vec![3.0]),
                JetComputeDevice::Vulkan,
            )
            .unwrap();
            let target = jet_compute_on_device(
                &f32_tensor(vec![1], vec![1.0]),
                JetComputeDevice::Vulkan,
            )
            .unwrap();
            let (tape, traced) = jet_compute_trace_inputs(vec![prediction, target]);
            let loss = jet_compute_mse_loss(&traced[0], &traced[1]).unwrap();
            let state = jet_compute_vjp_begin(loss, tape);
            let seed = jet_compute_gradient_seed(&state).unwrap();
            let gradients = jet_compute_vjp_pull(&state, &seed, &[0, 1]).unwrap();
            assert_eq!(jet_compute_tensor_values(&gradients[0]), vec![4.0]);
            assert_eq!(jet_compute_tensor_values(&gradients[1]), vec![-4.0]);
            let tangent_prediction = jet_compute_on_device(
                &f32_tensor(vec![1], vec![1.0]),
                JetComputeDevice::Vulkan,
            )
            .unwrap();
            let tangent_target = jet_compute_on_device(
                &f32_tensor(vec![1], vec![0.0]),
                JetComputeDevice::Vulkan,
            )
            .unwrap();
            let tangent = jet_compute_jvp(&state, vec![tangent_prediction, tangent_target]).unwrap();
            assert_eq!(jet_compute_tensor_values(&tangent), vec![4.0]);
        }

        #[test]
        fn webgpu_native_path_fails_closed_without_cpu_drift() {
            let cpu = f32_tensor(vec![1], vec![2.0]);
            let result = jet_compute_on_device(&cpu, JetComputeDevice::WebGpu);
            assert!(
                matches!(result, Err(JetComputeError::Unsupported(_)) | Err(JetComputeError::Device(_))),
                "native WebGPU must report its missing browser provider"
            );
        }
    }

    pub(super) type Tensor = JetTensor;
    pub(super) type Device = JetComputeDevice;
    pub(super) type Stream = JetComputeStream;
    pub(super) type Sparse = JetSparseCsr;
    pub(super) type Tape = std::sync::Arc<std::sync::Mutex<JetComputeTape>>;
    pub(super) type VjpState = JetComputeVjpState;

    pub(super) enum TransformResult {
        Gradient(Vec<Tensor>),
        ValueAndGradient {
            value: Tensor,
            gradients: Vec<Tensor>,
        },
        Vjp {
            value: Tensor,
            state: VjpState,
        },
        Jvp {
            value: Tensor,
            tangent: Tensor,
        },
    }

    pub(super) fn trace_inputs(inputs: Vec<Tensor>) -> (Tape, Vec<Tensor>) {
        jet_compute_trace_inputs(inputs)
    }

    pub(super) fn vjp_begin(value: Tensor, tape: Tape) -> VjpState {
        jet_compute_vjp_begin(value, tape)
    }

    pub(super) fn transform(
        method: &str,
        state: &VjpState,
        tangents: &[Tensor],
        targets: &[i64],
    ) -> Result<TransformResult, String> {
        match jet_compute_transform(method, state, tangents, targets) {
            Ok(JetComputeTransformResult::Gradient(values)) => {
                Ok(TransformResult::Gradient(values))
            }
            Ok(JetComputeTransformResult::ValueAndGradient { value, gradients }) => {
                Ok(TransformResult::ValueAndGradient { value, gradients })
            }
            Ok(JetComputeTransformResult::Vjp { value, state }) => {
                Ok(TransformResult::Vjp { value, state })
            }
            Ok(JetComputeTransformResult::Jvp { value, tangent }) => {
                Ok(TransformResult::Jvp { value, tangent })
            }
            Err(error) => Err(error.jet_show()),
        }
    }

    pub(super) fn error_message(error: &JetComputeError) -> String {
        error.jet_show()
    }

    pub(super) fn nested_gradient(
        states: &[VjpState],
        targets: &[i64],
    ) -> Result<Vec<Vec<Tensor>>, String> {
        jet_compute_nested_gradient(states, targets).map_err(|error| error.jet_show())
    }

    pub(super) fn vjp_pull(
        state: &VjpState,
        seed: &Tensor,
        targets: &[i64],
    ) -> Result<Vec<Tensor>, String> {
        jet_compute_vjp_pull(state, seed, targets).map_err(|error| error.jet_show())
    }

    pub(super) fn vjp_gradient(
        state: &VjpState,
        targets: &[i64],
    ) -> Result<Vec<Tensor>, String> {
        let seed = jet_compute_gradient_seed(state).map_err(|error| error.jet_show())?;
        vjp_pull(state, &seed, targets)
    }

    pub(super) fn from_list(values: &[f64]) -> Result<Tensor, String> {
        jet_compute_from_list(&values.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn matrix(rows: i64, cols: i64, fill: f64) -> Result<Tensor, String> {
        jet_compute_matrix(rows, cols, fill).map_err(|error| error.jet_show())
    }

    pub(super) fn zeros(shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_zeros(&shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn ones(shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_ones(&shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn full(shape: &[i64], value: f64) -> Result<Tensor, String> {
        jet_compute_full(&shape.to_vec(), value).map_err(|error| error.jet_show())
    }

    pub(super) fn eye(size: i64) -> Result<Tensor, String> {
        jet_compute_eye(size).map_err(|error| error.jet_show())
    }

    pub(super) fn vec(len: i64, fill: f64) -> Result<Tensor, String> {
        jet_compute_vec(len, fill).map_err(|error| error.jet_show())
    }

    pub(super) fn reshape(tensor: &Tensor, shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_reshape(tensor, &shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn device_cpu() -> Device {
        jet_compute_device_cpu()
    }

    pub(super) fn device_auto() -> Device {
        jet_compute_device_auto()
    }

    pub(super) fn device_metal() -> Device {
        jet_compute_device_metal()
    }

    pub(super) fn device_cuda() -> Device {
        jet_compute_device_cuda()
    }

    pub(super) fn device_vulkan() -> Device {
        jet_compute_device_vulkan()
    }

    pub(super) fn device_webgpu() -> Device {
        jet_compute_device_webgpu()
    }

    pub(super) fn device_word(device: Device) -> i64 {
        match device {
            JetComputeDevice::Auto => 0,
            JetComputeDevice::Cpu => 1,
            JetComputeDevice::Metal => 2,
            JetComputeDevice::Cuda => 3,
            JetComputeDevice::Vulkan => 4,
            JetComputeDevice::WebGpu => 5,
        }
    }

    pub(super) fn device_from_word(word: i64) -> Option<Device> {
        match word {
            0 => Some(JetComputeDevice::Auto),
            1 => Some(JetComputeDevice::Cpu),
            2 => Some(JetComputeDevice::Metal),
            3 => Some(JetComputeDevice::Cuda),
            4 => Some(JetComputeDevice::Vulkan),
            5 => Some(JetComputeDevice::WebGpu),
            _ => None,
        }
    }

    pub(super) fn on_device(tensor: &Tensor, device: Device) -> Result<Tensor, String> {
        jet_compute_on_device(tensor, device).map_err(|error| error.jet_show())
    }

    pub(super) fn broadcast_to(tensor: &Tensor, shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_broadcast_to(tensor, &shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn transpose(tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_transpose(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn det(tensor: &Tensor) -> Result<f64, String> {
        jet_compute_det(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn inv(tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_inv(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn solve(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_solve(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn fft(tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_fft(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn stream_new() -> Stream {
        jet_compute_stream_new()
    }

    pub(super) fn stream_new_on(device: Device) -> Result<Stream, String> {
        jet_compute_stream_new_on_device(device).map_err(|error| error.jet_show())
    }

    pub(super) fn stream_sync(stream: &Stream) -> Result<(), String> {
        jet_compute_stream_sync(stream).map_err(|error| error.jet_show())
    }

    pub(super) fn stream_show(stream: &Stream) -> String {
        jet_compute_stream_show(stream)
    }

    pub(super) fn transfer(tensor: &Tensor, device: Device) -> Result<Tensor, String> {
        jet_compute_transfer(tensor, device).map_err(|error| error.jet_show())
    }

    pub(super) fn transfer_show(tensor: &Tensor) -> String {
        jet_compute_transfer_show(tensor)
    }

    pub(super) fn kernel_bounds_ok(shape: &[i64], indices: &[i64]) -> Result<bool, String> {
        jet_compute_kernel_bounds_ok(shape, indices).map_err(|error| error.jet_show())
    }

    pub(super) fn to_sparse(tensor: &Tensor) -> Result<Sparse, String> {
        jet_compute_to_sparse(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn sparse_nnz(sparse: &Sparse) -> i64 {
        jet_compute_sparse_nnz(sparse)
    }

    pub(super) fn sparse_mv(sparse: &Sparse, vector: &Tensor) -> Result<Tensor, String> {
        jet_compute_sparse_mv(sparse, vector).map_err(|error| error.jet_show())
    }

    pub(super) fn sparse_show(sparse: &Sparse) -> String {
        jet_compute_sparse_show(sparse)
    }

    pub(super) fn add(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_add(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn mul(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_mul(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn sub(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("sub", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn div(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("div", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn maximum(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("maximum", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn minimum(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("minimum", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn unary(op: &str, tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_unary(op, tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn matmul(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_matmul(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn sum_axis(tensor: &Tensor, axis: i64) -> Result<Tensor, String> {
        jet_compute_sum_axis(tensor, axis).map_err(|error| error.jet_show())
    }

    pub(super) fn mse_loss(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_mse_loss(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn sgd_step(param: &Tensor, grad: &Tensor, learning_rate: f64) -> Result<Tensor, String> {
        jet_compute_sgd_step(param, grad, learning_rate).map_err(|error| error.jet_show())
    }

    pub(super) fn serialize(tensor: &Tensor) -> Result<String, String> {
        jet_compute_serialize(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn deserialize(payload: &str) -> Result<Tensor, String> {
        jet_compute_deserialize(&payload.to_string()).map_err(|error| error.jet_show())
    }

    pub(super) fn matmul_f32_tile(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_matmul_f32_tile(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn set(tensor: &mut Tensor, indices: &[i64], value: f64) -> Result<(), String> {
        jet_compute_set(tensor, indices, value).map_err(|error| error.jet_show())
    }

    pub(super) fn get(tensor: &Tensor, indices: &[i64]) -> Result<f64, String> {
        jet_compute_get(tensor, indices).map_err(|error| error.jet_show())
    }

    pub(super) fn tensor_shape(tensor: &Tensor) -> Vec<i64> {
        jet_compute_tensor_shape(tensor)
    }

    pub(super) fn tensor_rank(tensor: &Tensor) -> i64 {
        jet_compute_tensor_rank(tensor)
    }

    pub(super) fn tensor_numel(tensor: &Tensor) -> i64 {
        jet_compute_tensor_numel(tensor)
    }

    pub(super) fn tensor_device(tensor: &Tensor) -> String {
        jet_compute_tensor_device(tensor)
    }

    pub(super) fn tensor_placement(tensor: &Tensor) -> String {
        jet_compute_tensor_placement(tensor)
    }

    pub(super) fn profile_show() -> String {
        jet_compute_profile_show()
    }

    pub(super) fn copy(tensor: &Tensor) -> Tensor {
        jet_compute_copy(tensor)
    }

    pub(super) fn clone(tensor: &Tensor) -> Tensor {
        tensor.clone()
    }

    pub(super) fn tensor_values(tensor: &Tensor) -> Result<Vec<f64>, String> {
        jet_compute_validate_tensor(tensor).map_err(|error| error.jet_show())?;
        Ok(jet_compute_tensor_to_list(tensor))
    }

    /// Marshal the logical storage projection without applying the user-facing
    /// `to_list` trace rule. Resident slots must retain traced tensors while a
    /// Prelude transform is running; the heap list is only an adapter mirror.
    pub(super) fn marshal_values(tensor: &Tensor) -> Result<Vec<f64>, String> {
        jet_compute_validate_tensor(tensor).map_err(|error| error.jet_show())?;
        Ok(jet_compute_tensor_values(tensor))
    }

    pub(super) fn slice(
        tensor: &Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Result<Tensor, String> {
        jet_compute_slice_checked(tensor, start, end, exclusive)
            .map_err(|error| error.jet_show())
    }

    pub(super) fn view_values(
        tensor: &Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Result<Vec<f64>, String> {
        jet_compute_view_checked(tensor, start, end, exclusive)
            .map(|view| view.to_vec())
            .map_err(|error| error.jet_show())
    }

    pub(super) fn window_get(
        tensor: &Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
        index: i64,
    ) -> Result<f64, String> {
        jet_compute_window_get(tensor, start, end, exclusive, index)
    }

    pub(super) fn validate_mut(
        tensor: &mut Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Result<(), String> {
        jet_compute_view_mut_checked(tensor, start, end, exclusive)
            .map(|_| ())
            .map_err(|error| error.jet_show())
    }

    /// Marshal one mutable Tensor-window write into the shared Prelude seam.
    pub(super) fn window_set(
        tensor: &mut Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
        index: i64,
        value: f64,
    ) -> Result<(), String> {
        jet_compute_window_set(tensor, start, end, exclusive, index, value)
    }
}

#[derive(Default)]
pub(crate) struct ComputeState {
    slots: Vec<Option<TensorSlot>>,
    windows: Vec<TensorWindowSlot>,
    streams: Vec<Option<semantics::Stream>>,
    sparse: Vec<Option<semantics::Sparse>>,
    vjp_states: Vec<Option<semantics::VjpState>>,
    curried_handles: Vec<i64>,
}

impl ComputeState {
    pub(crate) fn clear(&mut self) {
        self.windows.clear();
        self.slots.clear();
        self.streams.clear();
        self.sparse.clear();
        self.vjp_states.clear();
        for handle in self.curried_handles.drain(..) {
            semantics::jet_compute_curried_drop(handle);
        }
    }
}

struct TensorSlot {
    tensor: semantics::Tensor,
    /// The flattened list handle used by the ordinary ViewMut JIT record.
    list: i64,
}

#[derive(Clone, Copy)]
struct TensorWindowSlot {
    /// The materialized list used by generic view operations.
    list: i64,
    /// The owning Tensor handle and its original Prelude window facts.
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: bool,
}

fn tensor_index(handle: i64) -> Option<usize> {
    usize::try_from(handle).ok()?.checked_sub(1)
}

fn stream<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a semantics::Stream> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.streams.get(index))
        .and_then(Option::as_ref)
}

fn sparse<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a semantics::Sparse> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.sparse.get(index))
        .and_then(Option::as_ref)
}

fn vjp_state<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a semantics::VjpState> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.vjp_states.get(index))
        .and_then(Option::as_ref)
}

fn slot<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a TensorSlot> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.slots.get(index))
        .and_then(Option::as_ref)
}

fn slot_mut<'a>(runtime: &'a mut JitRuntime, handle: i64) -> Option<&'a mut TensorSlot> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.slots.get_mut(index))
        .and_then(Option::as_mut)
}

fn trap(runtime: &mut JitRuntime, message: &str) -> i64 {
    runtime.set_trap(message);
    0
}

fn read_float_list(runtime: &JitRuntime, handle: i64) -> Option<Vec<f64>> {
    let len = runtime.heap.list_len(handle)?;
    (0..len)
        .map(|index| runtime.heap.list_get_float(handle, index))
        .collect()
}

fn alloc_float_list(runtime: &mut JitRuntime, values: &[f64]) -> i64 {
    let list = runtime.heap.alloc_empty_list();
    for &value in values {
        if runtime.heap.list_push_float(list, value).is_none() {
            runtime.set_trap("JIT compute list allocation failed");
            return 0;
        }
    }
    list
}

fn alloc_int_list(runtime: &mut JitRuntime, values: &[i64]) -> i64 {
    let list = runtime.heap.alloc_empty_list();
    for &value in values {
        if runtime.heap.list_push_int(list, value).is_none() {
            runtime.set_trap("JIT compute integer-list allocation failed");
            return 0;
        }
    }
    list
}

fn read_int_list(runtime: &JitRuntime, handle: i64) -> Option<Vec<i64>> {
    let len = runtime.heap.list_len(handle)?;
    (0..len)
        .map(|index| runtime.heap.list_get_int(handle, index))
        .collect()
}

fn sync_float_list(runtime: &mut JitRuntime, list: i64, values: &[f64]) -> bool {
    for (index, &value) in values.iter().enumerate() {
        let Ok(index) = i64::try_from(index) else {
            runtime.set_trap("JIT compute list index overflow");
            return false;
        };
        if runtime.heap.list_set_float(list, index, value).is_none() {
            runtime.set_trap("JIT compute list mirror is invalid");
            return false;
        }
    }
    true
}

fn alloc_tensor(runtime: &mut JitRuntime, tensor: semantics::Tensor) -> i64 {
    let Ok(values) = semantics::marshal_values(&tensor) else {
        runtime.set_trap("JIT compute tensor validation failed");
        return 0;
    };
    let list = alloc_float_list(runtime, &values);
    runtime.compute.slots.push(Some(TensorSlot { tensor, list }));
    runtime.compute.slots.len() as i64
}

fn alloc_record_words(runtime: &mut JitRuntime, values: &[i64], context: &str) -> i64 {
    let record = runtime.heap.alloc_record(values.len());
    for (index, &value) in values.iter().enumerate() {
        let Ok(index) = i64::try_from(index) else {
            runtime.set_trap(&format!("{context} field index overflow"));
            return 0;
        };
        if runtime.heap.record_set_int(record, index, value).is_none() {
            runtime.set_trap(&format!("{context} record allocation failed"));
            return 0;
        }
    }
    record
}

fn alloc_tensor_record(
    runtime: &mut JitRuntime,
    tensors: &[semantics::Tensor],
    context: &str,
) -> i64 {
    let handles = tensors
        .iter()
        .cloned()
        .map(|tensor| alloc_tensor(runtime, tensor))
        .collect::<Vec<_>>();
    if handles.iter().any(|handle| *handle == 0) {
        return 0;
    }
    alloc_record_words(runtime, &handles, context)
}

fn read_tensor_list(runtime: &JitRuntime, handle: i64) -> Option<Vec<i64>> {
    let values = read_int_list(runtime, handle)?;
    values
        .iter()
        .all(|value| slot(runtime, *value).is_some())
        .then_some(values)
}

fn read_record_words(runtime: &JitRuntime, handle: i64, count: usize) -> Option<Vec<i64>> {
    (0..count)
        .map(|index| runtime.heap.record_get_int(handle, index as i64))
        .collect()
}

fn invoke_callable(slot: JitCallableSlot, args: &[i64]) -> i64 {
    // SAFETY: the callable binder records the Cranelift signature shape and
    // this adapter only invokes Tensor-valued functions after sema has proved
    // their arity. The resident function-value ABI is C-compatible on the JIT
    // target, with the environment word prepended for captured values.
    unsafe {
        if slot.has_env {
            match args {
                [] => {
                    let callback: unsafe extern "C" fn(i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env)
                }
                [a] => {
                    let callback: unsafe extern "C" fn(i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a)
                }
                [a, b] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b)
                }
                [a, b, c] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c)
                }
                [a, b, c, d] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c, *d)
                }
                [a, b, c, d, e] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c, *d, *e)
                }
                [a, b, c, d, e, f] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c, *d, *e, *f)
                }
                _ => 0,
            }
        } else {
            match args {
                [] => {
                    let callback: unsafe extern "C" fn() -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback()
                }
                [a] => {
                    let callback: unsafe extern "C" fn(i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a)
                }
                [a, b] => {
                    let callback: unsafe extern "C" fn(i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b)
                }
                [a, b, c] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c)
                }
                [a, b, c, d] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c, *d)
                }
                [a, b, c, d, e] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c, *d, *e)
                }
                [a, b, c, d, e, f] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c, *d, *e, *f)
                }
                _ => 0,
            }
        }
    }
}

fn transform_failure(runtime: &mut JitRuntime, message: &str) -> i64 {
    runtime.set_trap(message);
    0
}

fn alloc_vjp_run(
    runtime: &mut JitRuntime,
    value: semantics::Tensor,
    state: semantics::VjpState,
    targets: &[i64],
    targets_handle: i64,
) -> i64 {
    let gradients = match semantics::vjp_gradient(&state, targets) {
        Ok(values) => values,
        Err(message) => return transform_failure(runtime, &message),
    };
    runtime.compute.vjp_states.push(Some(state));
    let state_handle = runtime.compute.vjp_states.len() as i64;
    let pull_env = alloc_record_words(
        runtime,
        &[state_handle, targets_handle],
        "core.compute.vjp.pull",
    );
    if pull_env == 0 {
        return 0;
    }
    let pull = bind_jit_callable_handle(
        runtime,
        crate::host_seam::guarded_addr(jet_jit_compute_vjp_pull) as i64,
        pull_env,
        true,
    );
    if pull == 0 {
        return 0;
    }
    let value_handle = alloc_tensor(runtime, value);
    let gradients_handle = alloc_tensor_record(runtime, &gradients, "core.compute.vjp.grads");
    if value_handle == 0 || gradients_handle == 0 {
        return 0;
    }
    let grads = bind_jit_callable_handle(
        runtime,
        crate::host_seam::guarded_addr(jet_jit_compute_vjp_grads_value) as i64,
        gradients_handle,
        true,
    );
    if grads == 0 {
        return 0;
    }
    alloc_record_words(
        runtime,
        &[value_handle, pull, grads],
        "core.compute.vjp",
    )
}

fn run_transform(
    runtime: &mut JitRuntime,
    base_handle: i64,
    inputs_handle: i64,
    targets_handle: i64,
    method: &str,
    base_arity: usize,
    result_fields: usize,
) -> i64 {
    let Some(callable) = jit_callable_parts(runtime, base_handle) else {
        return transform_failure(runtime, "core.compute transform received an invalid function");
    };
    let Some(inputs) = read_tensor_list(runtime, inputs_handle) else {
        return transform_failure(runtime, "core.compute transform expects Tensor arguments");
    };
    let Some(targets) = read_int_list(runtime, targets_handle) else {
        return transform_failure(runtime, "core.compute transform expects integer targets");
    };
    let expected_inputs = if method == "jvp" {
        base_arity.saturating_mul(2)
    } else {
        base_arity
    };
    if inputs.len() != expected_inputs {
        return transform_failure(runtime, "core.compute transform argument count mismatch");
    }
    if base_arity > 6 {
        return transform_failure(runtime, "core.compute transform function arity exceeds the resident ABI");
    }
    let primal_handles = &inputs[..base_arity];
    let tangent_handles = if method == "jvp" {
        &inputs[base_arity..]
    } else {
        &[]
    };
    let input_tensors = match primal_handles
        .iter()
        .map(|handle| slot(runtime, *handle).map(|slot| slot.tensor.clone()))
        .collect::<Option<Vec<_>>>()
    {
        Some(values) => values,
        None => return transform_failure(runtime, "core.compute transform received an invalid Tensor"),
    };
    let tangent_tensors = match tangent_handles
        .iter()
        .map(|handle| slot(runtime, *handle).map(|slot| slot.tensor.clone()))
        .collect::<Option<Vec<_>>>()
    {
        Some(values) => values,
        None => return transform_failure(runtime, "core.compute.jvp received an invalid tangent Tensor"),
    };
    let (tape, traced) = semantics::trace_inputs(input_tensors);
    let mut originals = Vec::with_capacity(primal_handles.len());
    for (handle, traced_tensor) in primal_handles.iter().zip(traced.iter()) {
        let Some(slot) = slot_mut(runtime, *handle) else {
            return transform_failure(runtime, "core.compute transform received an invalid Tensor");
        };
        originals.push((*handle, slot.tensor.clone()));
        slot.tensor = traced_tensor.clone();
    }
    let output_handle = invoke_callable(callable, primal_handles);
    let output_record = if output_handle != 0 && result_fields != 0 {
        read_record_words(runtime, output_handle, result_fields)
    } else {
        None
    };
    let output_tensor = if output_handle != 0 && result_fields == 0 {
        slot(runtime, output_handle).map(|slot| slot.tensor.clone())
    } else {
        None
    };
    for (handle, original) in originals {
        if let Some(slot) = slot_mut(runtime, handle) {
            slot.tensor = original;
        }
    }
    if output_handle == 0 {
        return transform_failure(runtime, "core.compute transform function returned no value");
    }
    let targets = targets.as_slice();
    if result_fields != 0 {
        if method != "gradient" {
            return transform_failure(runtime, "core.compute transform requires a Tensor result");
        }
        let Some(fields) = output_record else {
            return transform_failure(runtime, "core.compute.gradient returned an invalid tuple");
        };
        let states = match fields
            .iter()
            .map(|handle| slot(runtime, *handle).map(|slot| semantics::vjp_begin(slot.tensor.clone(), tape.clone())))
            .collect::<Option<Vec<_>>>()
        {
            Some(states) => states,
            None => return transform_failure(runtime, "core.compute.gradient tuple field is not a Tensor"),
        };
        let nested = match semantics::nested_gradient(&states, targets) {
            Ok(values) => values,
            Err(message) => return transform_failure(runtime, &message),
        };
        let mut outer_handles = Vec::with_capacity(nested.len());
        for values in nested {
            let handle = alloc_tensor_record(runtime, &values, "core.compute.gradient");
            if handle == 0 {
                return 0;
            }
            outer_handles.push(handle);
        }
        return alloc_record_words(runtime, &outer_handles, "core.compute.gradient");
    }
    let Some(output_tensor) = output_tensor else {
        return transform_failure(runtime, "core.compute transform returned an invalid Tensor");
    };
    let state = semantics::vjp_begin(output_tensor, tape);
    let transform = match semantics::transform(method, &state, &tangent_tensors, targets) {
        Ok(result) => result,
        Err(message) => return transform_failure(runtime, &message),
    };
    match transform {
        semantics::TransformResult::Gradient(values) => {
            alloc_tensor_record(runtime, &values, "core.compute.gradient")
        }
        semantics::TransformResult::ValueAndGradient { value, gradients } => {
            let value_handle = alloc_tensor(runtime, value);
            let gradients_handle = alloc_tensor_record(
                runtime,
                &gradients,
                "core.compute.value_and_gradient",
            );
            if value_handle == 0 || gradients_handle == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, gradients_handle],
                    "core.compute.value_and_gradient",
                )
            }
        }
        semantics::TransformResult::Vjp { value, state } => {
            alloc_vjp_run(runtime, value, state, targets, targets_handle)
        }
        semantics::TransformResult::Jvp { value, tangent } => {
            let value_handle = alloc_tensor(runtime, value);
            let tangent_handle = alloc_tensor(runtime, tangent);
            if value_handle == 0 || tangent_handle == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, tangent_handle],
                    "core.compute.jvp",
                )
            }
        }
    }
}

fn curried_base(
    base_handle: i64,
    base_arity: usize,
    result_fields: usize,
) -> semantics::JetComputeBase {
    semantics::JetComputeBase::new(base_arity, move |inputs| {
        let result = Concurrency::with_runtime_mut(|runtime| {
            let Some(callable) = jit_callable_parts(runtime, base_handle) else {
                return Some(Err(semantics::JetComputeError::Unsupported(
                    "core.compute transform received an invalid function".to_string(),
                )));
            };
            let handles = inputs
                .iter()
                .cloned()
                .map(|tensor| alloc_tensor(runtime, tensor))
                .collect::<Vec<_>>();
            if handles.iter().any(|handle| *handle == 0) {
                return Some(Err(semantics::JetComputeError::Unsupported(
                    "core.compute transform received an invalid Tensor".to_string(),
                )));
            }
            let output_handle = invoke_callable(callable, &handles);
            if output_handle == 0 {
                return Some(Err(semantics::JetComputeError::Unsupported(
                    "core.compute transform function returned no value".to_string(),
                )));
            }
            if result_fields == 0 {
                let Some(value) = slot(runtime, output_handle).map(|slot| slot.tensor.clone()) else {
                    return Some(Err(semantics::JetComputeError::Unsupported(
                        "core.compute transform returned an invalid Tensor".to_string(),
                    )));
                };
                return Some(Ok(semantics::JetComputeBaseResult::Tensor(value)));
            }
            let Some(fields) = read_record_words(runtime, output_handle, result_fields) else {
                return Some(Err(semantics::JetComputeError::Unsupported(
                    "core.compute.gradient returned an invalid tuple".to_string(),
                )));
            };
            let Some(values) = fields
                .iter()
                .map(|handle| slot(runtime, *handle).map(|slot| slot.tensor.clone()))
                .collect::<Option<Vec<_>>>()
            else {
                return Some(Err(semantics::JetComputeError::Unsupported(
                    "core.compute.gradient tuple field is not a Tensor".to_string(),
                )));
            };
            Some(Ok(semantics::JetComputeBaseResult::TensorTuple(values)))
        });
        result.unwrap_or_else(|| {
            Err(semantics::JetComputeError::Unsupported(
                "core.compute transform has no active resident runtime".to_string(),
            ))
        })
    })
}

fn alloc_curried_gradient(
    runtime: &mut JitRuntime,
    values: &[Vec<semantics::Tensor>],
    context: &str,
) -> i64 {
    let mut outer = Vec::with_capacity(values.len());
    for target in values {
        let handle = if target.len() == 1 {
            alloc_tensor(runtime, target[0].clone())
        } else {
            alloc_tensor_record(runtime, target, context)
        };
        if handle == 0 {
            return 0;
        }
        outer.push(handle);
    }
    alloc_record_words(runtime, &outer, context)
}

fn run_curried_handle(
    runtime: &mut JitRuntime,
    handle: i64,
    inputs: &[i64],
) -> i64 {
    let Some(tensors) = inputs
        .iter()
        .map(|handle| slot(runtime, *handle).map(|slot| slot.tensor.clone()))
        .collect::<Option<Vec<_>>>()
    else {
        return transform_failure(runtime, "core.compute transform received an invalid Tensor");
    };
    let result = match semantics::jet_compute_call_curried(
        handle,
        semantics::JetComputeInputPack::from_flat(tensors),
    ) {
        Ok(result) => result,
        Err(error) => return transform_failure(runtime, &semantics::error_message(&error)),
    };
    match result {
        semantics::JetComputeCurriedResult::Gradient(values) => {
            alloc_curried_gradient(runtime, &values, "core.compute.gradient")
        }
        semantics::JetComputeCurriedResult::ValueAndGradient { value, gradients } => {
            let value_handle = alloc_tensor(runtime, value);
            let gradients_handle = alloc_curried_gradient(
                runtime,
                &gradients,
                "core.compute.value_and_gradient",
            );
            if value_handle == 0 || gradients_handle == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, gradients_handle],
                    "core.compute.value_and_gradient",
                )
            }
        }
        semantics::JetComputeCurriedResult::Vjp { value, pull, grads } => {
            let value_handle = alloc_tensor(runtime, value);
            if value_handle == 0 {
                return 0;
            }
            runtime.compute.curried_handles.push(pull);
            runtime.compute.curried_handles.push(grads);
            let pull_callable = bind_jit_callable_handle(
                runtime,
                crate::host_seam::guarded_addr(jet_jit_compute_curried_pull) as i64,
                pull,
                true,
            );
            let grads_callable = bind_jit_callable_handle(
                runtime,
                crate::host_seam::guarded_addr(jet_jit_compute_curried_grads) as i64,
                grads,
                true,
            );
            if pull_callable == 0 || grads_callable == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, pull_callable, grads_callable],
                    "core.compute.vjp",
                )
            }
        }
        semantics::JetComputeCurriedResult::Jvp { value, tangent } => {
            let value_handle = alloc_tensor(runtime, value);
            let tangent_handle = alloc_tensor(runtime, tangent);
            if value_handle == 0 || tangent_handle == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, tangent_handle],
                    "core.compute.jvp",
                )
            }
        }
    }
}

/// Marshal one resident callable invocation into the single Prelude-owned
/// curried call. The record is adapter state only: the plan handle retains
/// transform policy, tape state, and continuation lifetime in the Prelude.
fn run_curried_call(runtime: &mut JitRuntime, env: i64, inputs: &[i64]) -> i64 {
    run_curried_handle(runtime, env, inputs)
}

fn run_curried_call_list(runtime: &mut JitRuntime, env: i64, inputs_handle: i64) -> i64 {
    let Some(inputs) = read_int_list(runtime, inputs_handle) else {
        return transform_failure(runtime, "core.compute transform inputs are invalid");
    };
    run_curried_call(runtime, env, &inputs)
}

/// The resident host call has one logical ABI for every curried transform.
/// Typed function-value adapters below only pack their arguments into a list
/// and enter this operation.
fn jet_jit_compute_call_curried(env: i64, inputs_handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_list(runtime, env, inputs_handle))
}

fn run_curried_call_adapter(runtime: &mut JitRuntime, env: i64, inputs: &[i64]) -> i64 {
    let inputs_handle = alloc_int_list(runtime, inputs);
    if inputs_handle == 0 {
        return 0;
    }
    run_curried_call_list(runtime, env, inputs_handle)
}

fn jet_jit_compute_curried_pull(env: i64, seed: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        run_curried_handle(
            runtime,
            env,
            &[seed],
        )
    })
}

fn jet_jit_compute_vjp_grads_value(env: i64) -> i64 {
    env
}

fn jet_jit_compute_curried_grads(env: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let result = semantics::jet_compute_call_curried(
            env,
            semantics::JetComputeInputPack::new(Vec::new(), Vec::new()),
        );
        let values = match result {
            Ok(semantics::JetComputeCurriedResult::Gradient(values)) => values,
            Ok(_) => return transform_failure(runtime, "core.compute.vjp.grads returned the wrong result"),
            Err(error) => return transform_failure(runtime, &semantics::error_message(&error)),
        };
        alloc_curried_gradient(runtime, &values, "core.compute.vjp.grads")
    })
}

fn jet_jit_compute_curried_call_0(env: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[]))
}

fn jet_jit_compute_curried_call_1(env: i64, a: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[a]))
}

fn jet_jit_compute_curried_call_2(env: i64, a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[a, b]))
}

fn jet_jit_compute_curried_call_3(env: i64, a: i64, b: i64, c: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[a, b, c]))
}

fn jet_jit_compute_curried_call_4(
    env: i64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[a, b, c, d]))
}

fn jet_jit_compute_curried_call_5(
    env: i64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[a, b, c, d, e]))
}

fn jet_jit_compute_curried_call_6(
    env: i64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    f: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_curried_call_adapter(runtime, env, &[a, b, c, d, e, f]))
}

fn jet_jit_compute_vjp_pull(env: i64, seed: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(fields) = read_record_words(runtime, env, 2) else {
            return transform_failure(runtime, "core.compute.vjp.pull environment is invalid");
        };
        let Some(state) = vjp_state(runtime, fields[0]).cloned() else {
            return transform_failure(runtime, "core.compute.vjp.pull state is invalid");
        };
        let Some(seed) = slot(runtime, seed).map(|slot| slot.tensor.clone()) else {
            return transform_failure(runtime, "core.compute.vjp.pull seed is invalid");
        };
        let Some(targets) = read_int_list(runtime, fields[1]) else {
            return transform_failure(runtime, "core.compute.vjp.pull targets are invalid");
        };
        let values = match semantics::vjp_pull(&state, &seed, &targets) {
            Ok(values) => values,
            Err(message) => return transform_failure(runtime, &message),
        };
        alloc_tensor_record(runtime, &values, "core.compute.vjp.pull")
    })
}

fn jet_jit_compute_transform(
    base: i64,
    inputs: i64,
    targets: i64,
    method: i64,
    base_arity: i64,
    result_fields: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(kind) = semantics::JetComputeTransformKind::from_i64(method) else {
            return transform_failure(runtime, "core.compute transform method is invalid");
        };
        let Ok(base_arity) = usize::try_from(base_arity) else {
            return transform_failure(runtime, "core.compute transform arity is invalid");
        };
        let Ok(result_fields) = usize::try_from(result_fields) else {
            return transform_failure(runtime, "core.compute transform result shape is invalid");
        };
        if inputs != 0 {
            return run_transform(
                runtime,
                base,
                inputs,
                targets,
                kind.name(),
                base_arity,
                result_fields,
            );
        }
        transform_failure(runtime, "core.compute curried transform needs its constructor seam")
    })
}

fn jet_jit_compute_curried_new(
    base: i64,
    targets: i64,
    method: i64,
    base_arity: i64,
    result_fields: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(kind) = semantics::JetComputeTransformKind::from_i64(method) else {
            return transform_failure(runtime, "core.compute transform method is invalid");
        };
        let Ok(base_arity) = usize::try_from(base_arity) else {
            return transform_failure(runtime, "core.compute transform arity is invalid");
        };
        let Ok(result_fields) = usize::try_from(result_fields) else {
            return transform_failure(runtime, "core.compute transform result shape is invalid");
        };
        let Some(targets) = read_int_list(runtime, targets) else {
            return transform_failure(runtime, "core.compute transform expects integer targets");
        };
        let plan = semantics::jet_compute_curried_new(
            curried_base(base, base_arity, result_fields),
            kind,
            &targets,
            if result_fields == 0 {
                semantics::JetComputeResultShape::Tensor
            } else {
                semantics::JetComputeResultShape::TensorTuple(result_fields)
            },
        );
        runtime.compute.curried_handles.push(plan);
        let adapter_arity = if kind.is_jvp() {
            base_arity.saturating_mul(2)
        } else {
            base_arity
        };
        // These adapters are handed to generated code as plain callable
        // addresses rather than as named `host_fns!` imports, so they need the
        // same no-unwind boundary explicitly (#1997, `host_seam.rs`).
        let fn_ptr = match adapter_arity {
            0 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_0) as i64,
            1 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_1) as i64,
            2 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_2) as i64,
            3 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_3) as i64,
            4 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_4) as i64,
            5 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_5) as i64,
            6 => crate::host_seam::guarded_addr(jet_jit_compute_curried_call_6) as i64,
            _ => return transform_failure(runtime, "core.compute transform function arity exceeds the resident ABI"),
        };
        bind_jit_callable_handle(runtime, fn_ptr, plan, true)
    })
}

fn jet_jit_compute_drop_tensor(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(index) = tensor_index(tensor) else {
            return 0;
        };
        let released = runtime
            .compute
            .slots
            .get_mut(index)
            .and_then(Option::take)
            .is_some();
        if released {
            runtime
                .compute
                .windows
                .retain(|window| window.tensor != tensor);
        }
        0
    })
}

fn jet_jit_compute_drop_window(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        runtime.compute.windows.retain(|window| window.list != list);
        0
    })
}

fn jet_jit_compute_from_list(values: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(values) = read_float_list(runtime, values) else {
            return trap(runtime, "core.compute.from_list expects a float list");
        };
        match semantics::from_list(&values) {
            Ok(tensor) => {
                let handle = alloc_tensor(runtime, tensor);
                result_ok(handle as u64)
            }
            Err(message) => trap(runtime, &message),
        }
    })
}

fn jet_jit_compute_matrix(rows: i64, cols: i64, fill: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match semantics::matrix(rows, cols, fill) {
        Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
        Err(message) => result_err_msg(&message),
    })
}

fn jet_jit_compute_zeros(shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.zeros expects an integer shape list");
        };
        match semantics::zeros(&shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_ones(shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.ones expects an integer shape list");
        };
        match semantics::ones(&shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_full(shape: i64, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.full expects an integer shape list");
        };
        match semantics::full(&shape, value) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_eye(size: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match semantics::eye(size) {
        Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
        Err(message) => result_err_msg(&message),
    })
}

fn jet_jit_compute_vec(len: i64, fill: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match semantics::vec(len, fill) {
        Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
        Err(message) => result_err_msg(&message),
    })
}

fn jet_jit_compute_reshape(tensor: i64, shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.reshape expects an integer shape list");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.reshape received an invalid Tensor handle");
        };
        match semantics::reshape(&tensor, &shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_device_cpu() -> i64 {
    semantics::device_word(semantics::device_cpu())
}

fn jet_jit_compute_device_auto() -> i64 {
    semantics::device_word(semantics::device_auto())
}

fn jet_jit_compute_device_metal() -> i64 {
    semantics::device_word(semantics::device_metal())
}

fn jet_jit_compute_device_cuda() -> i64 {
    semantics::device_word(semantics::device_cuda())
}

fn jet_jit_compute_device_vulkan() -> i64 {
    semantics::device_word(semantics::device_vulkan())
}

fn jet_jit_compute_device_webgpu() -> i64 {
    semantics::device_word(semantics::device_webgpu())
}

fn jet_jit_compute_on_device(tensor: i64, device: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(device) = semantics::device_from_word(device) else {
            return result_err_msg("core.compute.on_device received an invalid device");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.on_device received an invalid Tensor handle");
        };
        match semantics::on_device(&tensor, device) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_broadcast_to(tensor: i64, shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.broadcast_to expects an integer shape list");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.broadcast_to received an invalid Tensor handle");
        };
        match semantics::broadcast_to(&tensor, &shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_transpose(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.transpose received an invalid Tensor handle");
        };
        match semantics::transpose(&tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_det(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.det received an invalid Tensor handle");
        };
        match semantics::det(&tensor) {
            Ok(value) => result_ok(value.to_bits()),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_inv(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.inv received an invalid Tensor handle");
        };
        match semantics::inv(&tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_solve(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.solve received an invalid Tensor handle");
        };
        match semantics::solve(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_fft(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.fft received an invalid Tensor handle");
        };
        match semantics::fft(&tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_stream_new() -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        runtime.compute.streams.push(Some(semantics::stream_new()));
        runtime.compute.streams.len() as i64
    })
}

fn jet_jit_compute_stream_new_on_device(device: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(device) = semantics::device_from_word(device) else {
            return result_err_msg("core.compute.stream_new_on received an invalid device");
        };
        match semantics::stream_new_on(device) {
            Ok(stream) => {
                runtime.compute.streams.push(Some(stream));
                result_ok(runtime.compute.streams.len() as u64)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_stream_sync(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(stream) = stream(runtime, handle) else {
            return result_err_msg("core.compute.stream_sync received an invalid stream handle");
        };
        match semantics::stream_sync(stream) {
            Ok(()) => result_ok(0),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_stream_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(stream) = stream(runtime, handle) else {
            runtime.set_trap("core.compute.stream_show received an invalid stream handle");
            return 0;
        };
        runtime.heap.alloc_string(semantics::stream_show(stream))
    })
}

fn jet_jit_compute_transfer(tensor: i64, device: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(device) = semantics::device_from_word(device) else {
            return result_err_msg("core.compute.transfer received an invalid device");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.transfer received an invalid Tensor handle");
        };
        match semantics::transfer(&tensor, device) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_transfer_show(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            runtime.set_trap("core.compute.transfer_show received an invalid Tensor handle");
            return 0;
        };
        runtime.heap.alloc_string(semantics::transfer_show(&tensor))
    })
}

fn jet_jit_compute_kernel_bounds_ok(shape: i64, indices: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.kernel_bounds_ok expects an integer shape list");
        };
        let Some(indices) = read_int_list(runtime, indices) else {
            return result_err_msg("core.compute.kernel_bounds_ok expects an integer index list");
        };
        match semantics::kernel_bounds_ok(&shape, &indices) {
            Ok(value) => result_ok(u64::from(value)),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_to_sparse(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.to_sparse received an invalid Tensor handle");
        };
        match semantics::to_sparse(&tensor) {
            Ok(sparse) => {
                runtime.compute.sparse.push(Some(sparse));
                result_ok(runtime.compute.sparse.len() as u64)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_sparse_nnz(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match sparse(runtime, handle) {
        Some(sparse) => semantics::sparse_nnz(sparse),
        None => trap(runtime, "core.compute.sparse_nnz received an invalid sparse handle"),
    })
}

fn jet_jit_compute_sparse_mv(sparse_handle: i64, vector: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(sparse) = sparse(runtime, sparse_handle).cloned() else {
            return result_err_msg("core.compute.sparse_mv received an invalid sparse handle");
        };
        let Some(vector) = slot(runtime, vector).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.sparse_mv received an invalid Tensor handle");
        };
        match semantics::sparse_mv(&sparse, &vector) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_sparse_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(sparse) = sparse(runtime, handle) else {
            runtime.set_trap("core.compute.sparse_show received an invalid sparse handle");
            return 0;
        };
        runtime.heap.alloc_string(semantics::sparse_show(sparse))
    })
}

fn clone_tensor_pair(runtime: &JitRuntime, left: i64, right: i64) -> Option<(semantics::Tensor, semantics::Tensor)> {
    Some((slot(runtime, left)?.tensor.clone(), slot(runtime, right)?.tensor.clone()))
}

fn jet_jit_compute_add(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.add received an invalid Tensor handle");
        };
        match semantics::add(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_mul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.mul received an invalid Tensor handle");
        };
        match semantics::mul(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_sub(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.sub received an invalid Tensor handle");
        };
        match semantics::sub(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn compute_binary_op(
    left: i64,
    right: i64,
    name: &str,
    op: fn(&semantics::Tensor, &semantics::Tensor) -> Result<semantics::Tensor, String>,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg(&format!("core.compute.{name} received an invalid Tensor handle"));
        };
        match op(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_div(left: i64, right: i64) -> i64 {
    compute_binary_op(left, right, "div", semantics::div)
}

fn jet_jit_compute_maximum(left: i64, right: i64) -> i64 {
    compute_binary_op(left, right, "maximum", semantics::maximum)
}

fn jet_jit_compute_minimum(left: i64, right: i64) -> i64 {
    compute_binary_op(left, right, "minimum", semantics::minimum)
}

fn compute_unary_op(tensor: i64, op: &str, name: &str) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg(&format!("core.compute.{name} received an invalid Tensor handle"));
        };
        match semantics::unary(op, &tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_negate(tensor: i64) -> i64 {
    compute_unary_op(tensor, "negate", "negate")
}

fn jet_jit_compute_abs(tensor: i64) -> i64 {
    compute_unary_op(tensor, "abs", "abs")
}

fn jet_jit_compute_exp(tensor: i64) -> i64 {
    compute_unary_op(tensor, "exp", "exp")
}

fn jet_jit_compute_log(tensor: i64) -> i64 {
    compute_unary_op(tensor, "log", "log")
}

fn jet_jit_compute_sqrt(tensor: i64) -> i64 {
    compute_unary_op(tensor, "sqrt", "sqrt")
}

fn jet_jit_compute_matmul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.matmul received an invalid Tensor handle");
        };
        match semantics::matmul(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_sum_axis(tensor: i64, axis: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.sum_axis received an invalid Tensor handle");
        };
        match semantics::sum_axis(&tensor, axis) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_mse_loss(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.mse_loss received an invalid Tensor handle");
        };
        match semantics::mse_loss(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_sgd_step(param: i64, grad: i64, learning_rate: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((param, grad)) = clone_tensor_pair(runtime, param, grad) else {
            return result_err_msg("core.compute.sgd_step received an invalid Tensor handle");
        };
        match semantics::sgd_step(&param, &grad, learning_rate) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_serialize(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.serialize received an invalid Tensor handle");
        };
        match semantics::serialize(&tensor) {
            Ok(payload) => {
                let handle = runtime.heap.alloc_string(payload);
                result_ok(handle as u64)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_deserialize(payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(payload) = runtime.heap.clone_string(payload) else {
            return result_err_msg("core.compute.deserialize expects a string payload");
        };
        match semantics::deserialize(&payload) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_matmul_f32_tile(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg(
                "core.compute.matmul_f32_tile received an invalid Tensor handle",
            );
        };
        match semantics::matmul_f32_tile(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_get(tensor: i64, indices: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(indices) = read_int_list(runtime, indices) else {
            return result_err_msg("core.compute.get expects an integer index list");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.get received an invalid Tensor handle");
        };
        match semantics::get(&tensor, &indices) {
            Ok(value) => result_ok(value.to_bits()),
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_set(tensor: i64, indices: i64, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(indices) = read_int_list(runtime, indices) else {
            return result_err_msg("core.compute.set expects an integer index list");
        };
        let result = {
            let Some(slot) = slot_mut(runtime, tensor) else {
                return result_err_msg("core.compute.set received an invalid Tensor handle");
            };
            semantics::set(&mut slot.tensor, &indices, value)
        };
        match result {
            Ok(()) => {
                let values = match slot(runtime, tensor) {
                    Some(slot) => match semantics::tensor_values(&slot.tensor) {
                        Ok(values) => values,
                        Err(message) => return result_err_msg(&message),
                    },
                    None => return result_err_msg("core.compute.set received an invalid Tensor handle"),
                };
                let Some(list) = slot(runtime, tensor).map(|slot| slot.list) else {
                    return result_err_msg("core.compute.set received an invalid Tensor handle");
                };
                if !sync_float_list(runtime, list, &values) {
                    return 0;
                }
                result_ok(0)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

fn jet_jit_compute_shape(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return trap(runtime, "core.compute.shape received an invalid Tensor handle");
        };
        alloc_int_list(runtime, &semantics::tensor_shape(&tensor))
    })
}

fn jet_jit_compute_rank(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match slot(runtime, tensor) {
        Some(slot) => semantics::tensor_rank(&slot.tensor),
        None => trap(runtime, "core.compute.rank received an invalid Tensor handle"),
    })
}

fn jet_jit_compute_numel(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match slot(runtime, tensor) {
        Some(slot) => semantics::tensor_numel(&slot.tensor),
        None => trap(runtime, "core.compute.numel received an invalid Tensor handle"),
    })
}

fn jet_jit_compute_device(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return trap(runtime, "core.compute.device received an invalid Tensor handle");
        };
        runtime.heap.alloc_string(semantics::tensor_device(&tensor))
    })
}

fn jet_jit_compute_placement(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return trap(runtime, "core.compute.placement received an invalid Tensor handle");
        };
        runtime.heap.alloc_string(semantics::tensor_placement(&tensor))
    })
}

fn jet_jit_compute_profile_show() -> i64 {
    Concurrency::with_runtime_mut(|runtime| runtime.heap.alloc_string(semantics::profile_show()))
}

fn jet_jit_compute_copy(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let copied = match slot(runtime, tensor) {
            Some(slot) => semantics::copy(&slot.tensor),
            None => return trap(runtime, "core.compute.copy received an invalid Tensor handle"),
        };
        alloc_tensor(runtime, copied)
    })
}

fn jet_jit_compute_clone(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let cloned = match slot(runtime, tensor) {
            Some(slot) => semantics::clone(&slot.tensor),
            None => return trap(runtime, "core.compute.clone received an invalid Tensor handle"),
        };
        alloc_tensor(runtime, cloned)
    })
}

fn jet_jit_compute_tensor_to_list(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(slot) = slot(runtime, tensor) else {
            return trap(runtime, "core.compute.to_list received an invalid Tensor handle");
        };
        let values = match semantics::tensor_values(&slot.tensor) {
            Ok(values) => values,
            Err(message) => return trap(runtime, &message),
        };
        alloc_float_list(runtime, &values)
    })
}

fn jet_jit_compute_slice(
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(slot) = slot(runtime, tensor) else {
            return trap(runtime, "core.compute slice received an invalid Tensor handle");
        };
        let sliced = match semantics::slice(&slot.tensor, start, end, exclusive != 0) {
            Ok(tensor) => tensor,
            Err(message) => return trap(runtime, &message),
        };
        alloc_tensor(runtime, sliced)
    })
}

fn jet_jit_compute_view(
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(slot) = slot(runtime, tensor) else {
            return trap(runtime, "core.compute view received an invalid Tensor handle");
        };
        let values = match semantics::view_values(&slot.tensor, start, end, exclusive != 0) {
            Ok(values) => values,
            Err(message) => return trap(runtime, &message),
        };
        alloc_float_list(runtime, &values)
    })
}

fn jet_jit_compute_view_mut(
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let values = {
            let Some(slot) = slot_mut(runtime, tensor) else {
                return trap(runtime, "core.compute mutable view received an invalid Tensor handle");
            };
            if let Err(message) = semantics::validate_mut(
                &mut slot.tensor,
                start,
                end,
                exclusive != 0,
            ) {
                return trap(runtime, &message);
            }
            match semantics::view_values(
                &slot.tensor,
                start,
                end,
                exclusive != 0,
            ) {
                Ok(values) => values,
                Err(message) => return trap(runtime, &message),
            }
        };
        let list = alloc_float_list(runtime, &values);
        if list == 0 {
            return 0;
        }
        let Ok(view_len) = i64::try_from(values.len()) else {
            return trap(runtime, "Tensor view length is too large");
        };
        let view_end = view_len.checked_sub(1).unwrap_or(-1);
        runtime.compute.windows.push(TensorWindowSlot {
            list,
            tensor,
            start,
            end,
            exclusive: exclusive != 0,
        });
        let view = runtime.heap.alloc_record(3);
        let _ = runtime.heap.record_set_int(view, 0, list);
        // Generic ViewMut marshalling sees a self-contained relative list. It
        // performs no Tensor policy; the shared window seam below receives the
        // logical index and original window facts.
        let _ = runtime.heap.record_set_int(view, 1, 0);
        let _ = runtime.heap.record_set_int(view, 2, view_end);
        view
    })
}

/// Ordinary JIT list writes are the marshalling edge for a compute ViewMut.
/// Keep them on the canonical Prelude setter; ordinary list views continue
/// through Collections' existing list setter below this hook.
pub(crate) fn try_set_list_f64(
    runtime: &mut JitRuntime,
    list: i64,
    index: i64,
    value: f64,
) -> bool {
    let Some(window_index) = runtime
        .compute
        .windows
        .iter()
        .position(|window| window.list == list)
    else {
        return false;
    };
    let window = runtime.compute.windows[window_index];
    let result = {
        let Some(slot) = slot_mut(runtime, window.tensor) else {
            runtime.set_trap("core.compute mutable view received an invalid Tensor handle");
            return true;
        };
        match semantics::window_set(
            &mut slot.tensor,
            window.start,
            window.end,
            window.exclusive,
            index,
            value,
        ) {
            Ok(()) => {
                let owner_values = semantics::tensor_values(&slot.tensor);
                let view_values = semantics::view_values(
                    &slot.tensor,
                    window.start,
                    window.end,
                    window.exclusive,
                );
                owner_values.and_then(|owner_values| {
                    view_values.map(|view_values| (owner_values, view_values))
                })
            }
            Err(message) => Err(message),
        }
    };
    match result {
        Ok((owner_values, view_values)) => {
            let owner_list = slot(runtime, window.tensor).map(|slot| slot.list);
            if let Some(owner_list) = owner_list {
                let _ = sync_float_list(runtime, owner_list, &owner_values);
            }
            let _ = sync_float_list(runtime, list, &view_values);
        }
        Err(message) => runtime.set_trap(&message),
    }
    true
}

pub(crate) fn try_get_list_f64(
    runtime: &mut JitRuntime,
    list: i64,
    index: i64,
) -> Option<f64> {
    let window = runtime
        .compute
        .windows
        .iter()
        .find(|window| window.list == list)
        .copied()?;
    let result = slot(runtime, window.tensor).map_or_else(
        || Err("core.compute mutable view received an invalid Tensor handle".to_string()),
        |slot| {
            semantics::window_get(
                &slot.tensor,
                window.start,
                window.end,
                window.exclusive,
                index,
            )
        },
    );
    match result {
        Ok(value) => Some(value),
        Err(message) => {
            runtime.set_trap(&message);
            Some(0.0)
        }
    }
}

host_fns! {
    struct ComputeHostFns;
    register: register_compute_symbols;
    declare: declare_compute_host_fns(module) {
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;
        let mut sig_one = cranelift_codegen::ir::Signature::new(cc);
        sig_one.params.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        sig_one.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        let mut sig_zero = cranelift_codegen::ir::Signature::new(cc);
        sig_zero.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        let mut sig_two = cranelift_codegen::ir::Signature::new(cc);
        for _ in 0..2 {
            sig_two
                .params
                .push(cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::I64,
                ));
        }
        sig_two.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        let mut sig_three_f64 = cranelift_codegen::ir::Signature::new(cc);
        sig_three_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_three_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_three_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::F64,
            ));
        sig_three_f64
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_one_f64 = cranelift_codegen::ir::Signature::new(cc);
        sig_one_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_one_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::F64,
            ));
        sig_one_f64
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_two_f64 = cranelift_codegen::ir::Signature::new(cc);
        sig_two_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_two_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_two_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::F64,
            ));
        sig_two_f64
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_transform = cranelift_codegen::ir::Signature::new(cc);
        for _ in 0..6 {
            sig_transform
                .params
                .push(cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::I64,
                ));
        }
        sig_transform
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_curried_new = cranelift_codegen::ir::Signature::new(cc);
        for _ in 0..5 {
            sig_curried_new
                .params
                .push(cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::I64,
                ));
        }
        sig_curried_new
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_window = cranelift_codegen::ir::Signature::new(cc);
        for ty in [
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::I8,
        ] {
            sig_window
                .params
                .push(cranelift_codegen::ir::AbiParam::new(ty));
        }
        sig_window
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
    }
    from_list: "jet_compute_from_list" => jet_jit_compute_from_list: sig_one;
    matrix: "jet_compute_matrix" => jet_jit_compute_matrix: sig_three_f64;
    vec: "jet_compute_vec" => jet_jit_compute_vec: sig_one_f64;
    zeros: "jet_compute_zeros" => jet_jit_compute_zeros: sig_one;
    ones: "jet_compute_ones" => jet_jit_compute_ones: sig_one;
    full: "jet_compute_full" => jet_jit_compute_full: sig_one_f64;
    eye: "jet_compute_eye" => jet_jit_compute_eye: sig_one;
    reshape: "jet_compute_reshape" => jet_jit_compute_reshape: sig_two;
    add: "jet_compute_add" => jet_jit_compute_add: sig_two;
    mul: "jet_compute_mul" => jet_jit_compute_mul: sig_two;
    sub: "jet_compute_sub" => jet_jit_compute_sub: sig_two;
    div: "jet_compute_div" => jet_jit_compute_div: sig_two;
    maximum: "jet_compute_maximum" => jet_jit_compute_maximum: sig_two;
    minimum: "jet_compute_minimum" => jet_jit_compute_minimum: sig_two;
    negate: "jet_compute_negate" => jet_jit_compute_negate: sig_one;
    abs: "jet_compute_abs" => jet_jit_compute_abs: sig_one;
    exp: "jet_compute_exp" => jet_jit_compute_exp: sig_one;
    log: "jet_compute_log" => jet_jit_compute_log: sig_one;
    sqrt: "jet_compute_sqrt" => jet_jit_compute_sqrt: sig_one;
    matmul: "jet_compute_matmul" => jet_jit_compute_matmul: sig_two;
    sum_axis: "jet_compute_sum_axis" => jet_jit_compute_sum_axis: sig_two;
    mse_loss: "jet_compute_mse_loss" => jet_jit_compute_mse_loss: sig_two;
    sgd_step: "jet_compute_sgd_step" => jet_jit_compute_sgd_step: sig_three_f64;
    serialize: "jet_compute_serialize" => jet_jit_compute_serialize: sig_one;
    deserialize: "jet_compute_deserialize" => jet_jit_compute_deserialize: sig_one;
    matmul_f32_tile: "jet_compute_matmul_f32_tile" => jet_jit_compute_matmul_f32_tile: sig_two;
    get: "jet_compute_get" => jet_jit_compute_get: sig_two;
    set: "jet_compute_set" => jet_jit_compute_set: sig_two_f64;
    shape: "jet_compute_tensor_shape" => jet_jit_compute_shape: sig_one;
    rank: "jet_compute_tensor_rank" => jet_jit_compute_rank: sig_one;
    numel: "jet_compute_tensor_numel" => jet_jit_compute_numel: sig_one;
    device: "jet_compute_tensor_device" => jet_jit_compute_device: sig_one;
    placement: "jet_compute_tensor_placement" => jet_jit_compute_placement: sig_one;
    device_cpu: "jet_compute_device_cpu" => jet_jit_compute_device_cpu: sig_zero;
    device_auto: "jet_compute_device_auto" => jet_jit_compute_device_auto: sig_zero;
    device_metal: "jet_compute_device_metal" => jet_jit_compute_device_metal: sig_zero;
    device_cuda: "jet_compute_device_cuda" => jet_jit_compute_device_cuda: sig_zero;
    device_vulkan: "jet_compute_device_vulkan" => jet_jit_compute_device_vulkan: sig_zero;
    device_webgpu: "jet_compute_device_webgpu" => jet_jit_compute_device_webgpu: sig_zero;
    on_device: "jet_compute_on_device" => jet_jit_compute_on_device: sig_two;
    broadcast_to: "jet_compute_broadcast_to" => jet_jit_compute_broadcast_to: sig_two;
    transpose: "jet_compute_transpose" => jet_jit_compute_transpose: sig_one;
    det: "jet_compute_det" => jet_jit_compute_det: sig_one;
    inv: "jet_compute_inv" => jet_jit_compute_inv: sig_one;
    solve: "jet_compute_solve" => jet_jit_compute_solve: sig_two;
    fft: "jet_compute_fft" => jet_jit_compute_fft: sig_one;
    stream_new: "jet_compute_stream_new" => jet_jit_compute_stream_new: sig_zero;
    stream_new_on: "jet_compute_stream_new_on_device" => jet_jit_compute_stream_new_on_device: sig_one;
    stream_sync: "jet_compute_stream_sync" => jet_jit_compute_stream_sync: sig_one;
    stream_show: "jet_compute_stream_show" => jet_jit_compute_stream_show: sig_one;
    transfer: "jet_compute_transfer" => jet_jit_compute_transfer: sig_two;
    transfer_show: "jet_compute_transfer_show" => jet_jit_compute_transfer_show: sig_one;
    kernel_bounds_ok: "jet_compute_kernel_bounds_ok" => jet_jit_compute_kernel_bounds_ok: sig_two;
    to_sparse: "jet_compute_to_sparse" => jet_jit_compute_to_sparse: sig_one;
    sparse_nnz: "jet_compute_sparse_nnz" => jet_jit_compute_sparse_nnz: sig_one;
    sparse_mv: "jet_compute_sparse_mv" => jet_jit_compute_sparse_mv: sig_two;
    sparse_show: "jet_compute_sparse_show" => jet_jit_compute_sparse_show: sig_one;
    profile_f32_strict: "jet_compute_profile_f32_strict" => jet_jit_compute_profile_show: sig_zero;
    profile_show: "jet_compute_profile_show" => jet_jit_compute_profile_show: sig_zero;
    copy: "jet_compute_copy" => jet_jit_compute_copy: sig_one;
    clone: "jet_compute_clone" => jet_jit_compute_clone: sig_one;
    drop_tensor: "jet_jit_compute_drop_tensor" => jet_jit_compute_drop_tensor: sig_one;
    drop_window: "jet_jit_compute_drop_window" => jet_jit_compute_drop_window: sig_one;
    tensor_to_list: "jet_compute_tensor_to_list" => jet_jit_compute_tensor_to_list: sig_one;
    slice: "jet_jit_compute_slice" => jet_jit_compute_slice: sig_window;
    view: "jet_jit_compute_view" => jet_jit_compute_view: sig_window;
    view_mut: "jet_jit_compute_view_mut" => jet_jit_compute_view_mut: sig_window;
    transform: "jet_compute_transform" => jet_jit_compute_transform: sig_transform;
    curried_new: "jet_compute_curried_new" => jet_jit_compute_curried_new: sig_curried_new;
    call_curried: "jet_compute_call_curried" => jet_jit_compute_call_curried: sig_two;
}

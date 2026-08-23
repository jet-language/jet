// ── D-COMPUTE1=D / D-COMPUTE-TYPE1=D / D-COMPUTE-PLACE1=D (#443) ─────────────
// One Core compute family. `Tensor` owns ranked multidimensional storage on the
// selected CPU-oracle or explicit accelerator ability; views retain the backing allocation and its
// strides. Mutable access requires the sema-proved exclusive ViewMut path;
// shared writes fail closed instead of copying or pretending to update an alias.
// Explicit Tensor copies materialize logical values into fresh backing storage.
// Engines only marshal into these Prelude symbols (I9).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetComputeDevice {
    Auto,
    Cpu,
    Metal,
    Cuda,
    Vulkan,
    WebGpu,
}

const MAX_TENSOR_ELEMENTS: usize = 16 * 1024 * 1024;
const CPU_ORACLE_BACKEND: &str = "cpu-oracle";
const CPU_ORACLE_VERSION: &str = "builtin";
const CPU_ORACLE_CACHE: &str = "none";
const METAL_BACKEND: &str = "metal";
const METAL_VERSION: &str = "system";
const METAL_CACHE: &str = "runtime";
const CUDA_BACKEND: &str = "cuda";
const CUDA_VERSION: &str = "driver";
const CUDA_CACHE: &str = "runtime";
const VULKAN_BACKEND: &str = "vulkan";
const VULKAN_VERSION: &str = "system";
const VULKAN_CACHE: &str = "runtime";
const WEBGPU_BACKEND: &str = "webgpu";
const WEBGPU_VERSION: &str = "browser";
const WEBGPU_CACHE: &str = "runtime";
const CPU_ORACLE_F64_PROFILE: &str = "F64Strict+Reproducible";
const CPU_ORACLE_F32_PROFILE: &str = "F32Strict+Reproducible";
const CPU_ORACLE_F64_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "reproducible-reduction",
    "differential-oracle",
];
const CPU_ORACLE_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "cpu-simd-dispatch",
    "simd-tail",
    "blocked-matmul",
    "differential-oracle",
];
const METAL_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "reproducible-reduction",
    "elementwise",
    "matmul",
    "device-buffer",
    "stream",
    "differential-oracle",
];
const CUDA_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "reproducible-reduction",
    "elementwise",
    "matmul",
    "device-buffer",
    "stream",
    "differential-oracle",
];
const VULKAN_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "reproducible-reduction",
    "elementwise",
    "matmul",
    "device-buffer",
    "stream",
    "differential-oracle",
];
const WEBGPU_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "reproducible-reduction",
    "elementwise",
    "matmul",
    "device-buffer",
    "stream",
    "differential-oracle",
];

// Compiled from `compute_vulkan.comp` with glslangValidator. Keeping the
// words in the Prelude makes AOT, JIT, comptime, and interpreter hosts use the
// same checked kernel without a runtime shader compiler dependency.
const VULKAN_SHADER: &[u32] = &[
    119734787, 65536, 524299, 479, 0, 131089, 1, 393227, 1, 1280527431, 1685353262, 808793134,
    0, 196622, 0, 1, 393231, 5, 4, 1852399981, 0, 11, 393232, 4,
    17, 64, 1, 1, 196611, 2, 450, 262149, 4, 1852399981, 0, 196613,
    8, 25705, 524293, 11, 1197436007, 1633841004, 1986939244, 1952539503, 1231974249, 68, 262149, 19,
    1634885968, 29549, 327686, 19, 0, 1853189987, 116, 327686, 19, 1, 1937207154, 0,
    327686, 19, 2, 1701736041, 114, 327686, 19, 3, 1936486243, 0, 262150, 19,
    4, 28783, 327686, 19, 5, 1818321779, 29281, 196613, 21, 112, 262149, 60,
    1952867692, 0, 196613, 62, 65, 262150, 62, 0, 97, 196613, 64, 0,
    262149, 69, 1751607666, 116, 196613, 71, 66, 262150, 71, 0, 98, 196613,
    73, 0, 196613, 83, 67, 262150, 83, 0, 99, 196613, 85, 0,
    262149, 152, 1970037110, 101, 196613, 211, 7827314, 196613, 217, 7106403, 196613, 222,
    7173491, 196613, 224, 107, 196613, 271, 7173491, 262149, 272, 1701080681, 120, 196613,
    302, 7173491, 262149, 303, 1701080681, 120, 327685, 313, 1717987684, 1852142181, 25955, 327685,
    341, 1717987684, 1852142181, 25955, 196613, 374, 68, 262150, 374, 0, 100, 196613,
    376, 0, 196613, 407, 7173491, 262149, 408, 1701080681, 120, 196613, 430, 69,
    262150, 430, 0, 101, 196613, 432, 0, 196613, 476, 70, 262150, 476,
    0, 102, 196613, 478, 0, 262215, 11, 11, 28, 196679, 19, 2,
    327752, 19, 0, 35, 0, 327752, 19, 1, 35, 4, 327752, 19,
    2, 35, 8, 327752, 19, 3, 35, 12, 327752, 19, 4, 35,
    16, 327752, 19, 5, 35, 20, 262215, 21, 33, 6, 262215, 21,
    34, 0, 262215, 61, 6, 4, 196679, 62, 3, 262216, 62, 0,
    24, 327752, 62, 0, 35, 0, 196679, 64, 24, 262215, 64, 33,
    0, 262215, 64, 34, 0, 262215, 70, 6, 4, 196679, 71, 3,
    262216, 71, 0, 24, 327752, 71, 0, 35, 0, 196679, 73, 24,
    262215, 73, 33, 1, 262215, 73, 34, 0, 262215, 82, 6, 4,
    196679, 83, 3, 327752, 83, 0, 35, 0, 262215, 85, 33, 2,
    262215, 85, 34, 0, 262215, 373, 6, 4, 196679, 374, 3, 262216,
    374, 0, 24, 327752, 374, 0, 35, 0, 196679, 376, 24, 262215,
    376, 33, 3, 262215, 376, 34, 0, 262215, 429, 6, 4, 196679,
    430, 3, 262216, 430, 0, 24, 327752, 430, 0, 35, 0, 196679,
    432, 24, 262215, 432, 33, 4, 262215, 432, 34, 0, 262215, 474,
    11, 25, 262215, 475, 6, 4, 196679, 476, 3, 262216, 476, 0,
    24, 327752, 476, 0, 35, 0, 196679, 478, 24, 262215, 478, 33,
    5, 262215, 478, 34, 0, 131091, 2, 196641, 3, 2, 262165, 6,
    32, 0, 262176, 7, 7, 6, 262167, 9, 6, 3, 262176, 10,
    1, 9, 262203, 10, 11, 1, 262187, 6, 12, 0, 262176, 13,
    1, 6, 131092, 16, 196630, 18, 32, 524318, 19, 6, 6, 6,
    6, 6, 18, 262176, 20, 2, 19, 262203, 20, 21, 2, 262165,
    22, 32, 1, 262187, 22, 23, 0, 262176, 24, 2, 6, 262187,
    22, 30, 4, 262187, 6, 33, 12, 262187, 6, 40, 13, 262187,
    6, 47, 15, 262187, 6, 55, 5, 262176, 59, 7, 18, 196637,
    61, 18, 196638, 62, 61, 262176, 63, 2, 62, 262203, 63, 64,
    2, 262176, 66, 2, 18, 196637, 70, 18, 196638, 71, 70, 262176,
    72, 2, 71, 262203, 72, 73, 2, 196637, 82, 18, 196638, 83,
    82, 262176, 84, 2, 83, 262203, 84, 85, 2, 262187, 6, 94,
    1, 262187, 6, 106, 2, 262187, 6, 118, 3, 262187, 6, 130,
    4, 262187, 6, 148, 10, 262187, 6, 158, 6, 262187, 6, 169,
    7, 262187, 6, 180, 8, 262187, 6, 191, 9, 262187, 6, 207,
    11, 262187, 22, 213, 3, 262187, 18, 223, 0, 262187, 22, 231,
    2, 262187, 22, 255, 1, 262187, 6, 337, 14, 262187, 22, 342,
    5, 262187, 18, 367, 1073741824, 196637, 373, 18, 196638, 374, 373, 262176,
    375, 2, 374, 262203, 375, 376, 2, 262187, 6, 398, 16, 196637,
    429, 18, 196638, 430, 429, 262176, 431, 2, 430, 262203, 431, 432,
    2, 262187, 6, 451, 18, 262187, 6, 473, 64, 393260, 9, 474,
    473, 94, 94, 196637, 475, 18, 196638, 476, 475, 262176, 477, 2,
    476, 262203, 477, 478, 2, 327734, 2, 4, 0, 3, 131320, 5,
    262203, 7, 8, 7, 262203, 59, 60, 7, 262203, 59, 69, 7,
    262203, 59, 152, 7, 262203, 7, 211, 7, 262203, 7, 217, 7,
    262203, 59, 222, 7, 262203, 7, 224, 7, 262203, 59, 271, 7,
    262203, 7, 272, 7, 262203, 59, 302, 7, 262203, 7, 303, 7,
    262203, 59, 313, 7, 262203, 59, 341, 7, 262203, 59, 346, 7,
    262203, 59, 407, 7, 262203, 7, 408, 7, 327745, 13, 14, 11,
    12, 262205, 6, 15, 14, 196670, 8, 15, 262205, 6, 17, 8,
    327745, 24, 25, 21, 23, 262205, 6, 26, 25, 327854, 16, 27,
    17, 26, 196855, 29, 0, 262394, 27, 28, 29, 131320, 28, 327745,
    24, 31, 21, 30, 262205, 6, 32, 31, 327851, 16, 34, 32,
    33, 131321, 29, 131320, 29, 458997, 16, 35, 27, 5, 34, 28,
    196855, 37, 0, 262394, 35, 36, 37, 131320, 36, 327745, 24, 38,
    21, 30, 262205, 6, 39, 38, 327851, 16, 41, 39, 40, 131321,
    37, 131320, 37, 458997, 16, 42, 35, 29, 41, 36, 196855, 44,
    0, 262394, 42, 43, 44, 131320, 43, 327745, 24, 45, 21, 30,
    262205, 6, 46, 45, 327851, 16, 48, 46, 47, 131321, 44, 131320,
    44, 458997, 16, 49, 42, 37, 48, 43, 196855, 51, 0, 262394,
    49, 50, 51, 131320, 50, 65789, 131320, 51, 327745, 24, 53, 21,
    30, 262205, 6, 54, 53, 327858, 16, 56, 54, 55, 196855, 58,
    0, 262394, 56, 57, 145, 131320, 57, 262205, 6, 65, 8, 393281,
    66, 67, 64, 23, 65, 262205, 18, 68, 67, 196670, 60, 68,
    262205, 6, 74, 8, 393281, 66, 75, 73, 23, 74, 262205, 18,
    76, 75, 196670, 69, 76, 327745, 24, 77, 21, 30, 262205, 6,
    78, 77, 327850, 16, 79, 78, 12, 196855, 81, 0, 262394, 79,
    80, 91, 131320, 80, 262205, 6, 86, 8, 262205, 18, 87, 60,
    262205, 18, 88, 69, 327809, 18, 89, 87, 88, 393281, 66, 90,
    85, 23, 86, 196670, 90, 89, 131321, 81, 131320, 91, 327745, 24,
    92, 21, 30, 262205, 6, 93, 92, 327850, 16, 95, 93, 94,
    196855, 97, 0, 262394, 95, 96, 103, 131320, 96, 262205, 6, 98,
    8, 262205, 18, 99, 60, 262205, 18, 100, 69, 327813, 18, 101,
    99, 100, 393281, 66, 102, 85, 23, 98, 196670, 102, 101, 131321,
    97, 131320, 103, 327745, 24, 104, 21, 30, 262205, 6, 105, 104,
    327850, 16, 107, 105, 106, 196855, 109, 0, 262394, 107, 108, 115,
    131320, 108, 262205, 6, 110, 8, 262205, 18, 111, 60, 262205, 18,
    112, 69, 327811, 18, 113, 111, 112, 393281, 66, 114, 85, 23,
    110, 196670, 114, 113, 131321, 109, 131320, 115, 327745, 24, 116, 21,
    30, 262205, 6, 117, 116, 327850, 16, 119, 117, 118, 196855, 121,
    0, 262394, 119, 120, 127, 131320, 120, 262205, 6, 122, 8, 262205,
    18, 123, 60, 262205, 18, 124, 69, 327816, 18, 125, 123, 124,
    393281, 66, 126, 85, 23, 122, 196670, 126, 125, 131321, 121, 131320,
    127, 327745, 24, 128, 21, 30, 262205, 6, 129, 128, 327850, 16,
    131, 129, 130, 196855, 133, 0, 262394, 131, 132, 139, 131320, 132,
    262205, 6, 134, 8, 262205, 18, 135, 60, 262205, 18, 136, 69,
    458764, 18, 137, 1, 40, 135, 136, 393281, 66, 138, 85, 23,
    134, 196670, 138, 137, 131321, 133, 131320, 139, 262205, 6, 140, 8,
    262205, 18, 141, 60, 262205, 18, 142, 69, 458764, 18, 143, 1,
    37, 141, 142, 393281, 66, 144, 85, 23, 140, 196670, 144, 143,
    131321, 133, 131320, 133, 131321, 121, 131320, 121, 131321, 109, 131320, 109,
    131321, 97, 131320, 97, 131321, 81, 131320, 81, 131321, 58, 131320, 145,
    327745, 24, 146, 21, 30, 262205, 6, 147, 146, 327858, 16, 149,
    147, 148, 196855, 151, 0, 262394, 149, 150, 204, 131320, 150, 262205,
    6, 153, 8, 393281, 66, 154, 64, 23, 153, 262205, 18, 155,
    154, 196670, 152, 155, 327745, 24, 156, 21, 30, 262205, 6, 157,
    156, 327850, 16, 159, 157, 158, 196855, 161, 0, 262394, 159, 160,
    166, 131320, 160, 262205, 6, 162, 8, 262205, 18, 163, 152, 262271,
    18, 164, 163, 393281, 66, 165, 85, 23, 162, 196670, 165, 164,
    131321, 161, 131320, 166, 327745, 24, 167, 21, 30, 262205, 6, 168,
    167, 327850, 16, 170, 168, 169, 196855, 172, 0, 262394, 170, 171,
    177, 131320, 171, 262205, 6, 173, 8, 262205, 18, 174, 152, 393228,
    18, 175, 1, 4, 174, 393281, 66, 176, 85, 23, 173, 196670,
    176, 175, 131321, 172, 131320, 177, 327745, 24, 178, 21, 30, 262205,
    6, 179, 178, 327850, 16, 181, 179, 180, 196855, 183, 0, 262394,
    181, 182, 188, 131320, 182, 262205, 6, 184, 8, 262205, 18, 185,
    152, 393228, 18, 186, 1, 27, 185, 393281, 66, 187, 85, 23,
    184, 196670, 187, 186, 131321, 183, 131320, 188, 327745, 24, 189, 21,
    30, 262205, 6, 190, 189, 327850, 16, 192, 190, 191, 196855, 194,
    0, 262394, 192, 193, 199, 131320, 193, 262205, 6, 195, 8, 262205,
    18, 196, 152, 393228, 18, 197, 1, 28, 196, 393281, 66, 198,
    85, 23, 195, 196670, 198, 197, 131321, 194, 131320, 199, 262205, 6,
    200, 8, 262205, 18, 201, 152, 393228, 18, 202, 1, 31, 201,
    393281, 66, 203, 85, 23, 200, 196670, 203, 202, 131321, 194, 131320,
    194, 131321, 183, 131320, 183, 131321, 172, 131320, 172, 131321, 161, 131320,
    161, 131321, 151, 131320, 204, 327745, 24, 205, 21, 30, 262205, 6,
    206, 205, 327850, 16, 208, 206, 207, 196855, 210, 0, 262394, 208,
    209, 260, 131320, 209, 262205, 6, 212, 8, 327745, 24, 214, 21,
    213, 262205, 6, 215, 214, 327814, 6, 216, 212, 215, 196670, 211,
    216, 262205, 6, 218, 8, 327745, 24, 219, 21, 213, 262205, 6,
    220, 219, 327817, 6, 221, 218, 220, 196670, 217, 221, 196670, 222,
    223, 196670, 224, 12, 131321, 225, 131320, 225, 262390, 227, 228, 0,
    131321, 229, 131320, 229, 262205, 6, 230, 224, 327745, 24, 232, 21,
    231, 262205, 6, 233, 232, 327856, 16, 234, 230, 233, 262394, 234,
    226, 227, 131320, 226, 262205, 6, 235, 211, 327745, 24, 236, 21,
    231, 262205, 6, 237, 236, 327812, 6, 238, 235, 237, 262205, 6,
    239, 224, 327808, 6, 240, 238, 239, 393281, 66, 241, 64, 23,
    240, 262205, 18, 242, 241, 262205, 6, 243, 224, 327745, 24, 244,
    21, 213, 262205, 6, 245, 244, 327812, 6, 246, 243, 245, 262205,
    6, 247, 217, 327808, 6, 248, 246, 247, 393281, 66, 249, 73,
    23, 248, 262205, 18, 250, 249, 327813, 18, 251, 242, 250, 262205,
    18, 252, 222, 327809, 18, 253, 252, 251, 196670, 222, 253, 131321,
    228, 131320, 228, 262205, 6, 254, 224, 327808, 6, 256, 254, 255,
    196670, 224, 256, 131321, 225, 131320, 227, 262205, 6, 257, 8, 262205,
    18, 258, 222, 393281, 66, 259, 85, 23, 257, 196670, 259, 258,
    131321, 210, 131320, 260, 327745, 24, 261, 21, 30, 262205, 6, 262,
    261, 327850, 16, 263, 262, 33, 196855, 265, 0, 262394, 263, 264,
    291, 131320, 264, 262205, 6, 266, 8, 327851, 16, 267, 266, 12,
    196855, 269, 0, 262394, 267, 268, 269, 131320, 268, 65789, 131320, 269,
    196670, 271, 223, 196670, 272, 12, 131321, 273, 131320, 273, 262390, 275,
    276, 0, 131321, 277, 131320, 277, 262205, 6, 278, 272, 327745, 24,
    279, 21, 23, 262205, 6, 280, 279, 327856, 16, 281, 278, 280,
    262394, 281, 274, 275, 131320, 274, 262205, 6, 282, 272, 393281, 66,
    283, 64, 23, 282, 262205, 18, 284, 283, 262205, 18, 285, 271,
    327809, 18, 286, 285, 284, 196670, 271, 286, 131321, 276, 131320, 276,
    262205, 6, 287, 272, 327808, 6, 288, 287, 255, 196670, 272, 288,
    131321, 273, 131320, 275, 262205, 18, 289, 271, 393281, 66, 290, 85,
    23, 23, 196670, 290, 289, 131321, 265, 131320, 291, 327745, 24, 292,
    21, 30, 262205, 6, 293, 292, 327850, 16, 294, 293, 40, 196855,
    296, 0, 262394, 294, 295, 334, 131320, 295, 262205, 6, 297, 8,
    327851, 16, 298, 297, 12, 196855, 300, 0, 262394, 298, 299, 300,
    131320, 299, 65789, 131320, 300, 196670, 302, 223, 196670, 303, 12, 131321,
    304, 131320, 304, 262390, 306, 307, 0, 131321, 308, 131320, 308, 262205,
    6, 309, 303, 327745, 24, 310, 21, 23, 262205, 6, 311, 310,
    327856, 16, 312, 309, 311, 262394, 312, 305, 306, 131320, 305, 262205,
    6, 314, 303, 393281, 66, 315, 64, 23, 314, 262205, 18, 316,
    315, 262205, 6, 317, 303, 393281, 66, 318, 73, 23, 317, 262205,
    18, 319, 318, 327811, 18, 320, 316, 319, 196670, 313, 320, 262205,
    18, 321, 313, 262205, 18, 322, 313, 327813, 18, 323, 321, 322,
    262205, 18, 324, 302, 327809, 18, 325, 324, 323, 196670, 302, 325,
    131321, 307, 131320, 307, 262205, 6, 326, 303, 327808, 6, 327, 326,
    255, 196670, 303, 327, 131321, 304, 131320, 306, 262205, 18, 328, 302,
    327745, 24, 329, 21, 23, 262205, 6, 330, 329, 262256, 18, 331,
    330, 327816, 18, 332, 328, 331, 393281, 66, 333, 85, 23, 23,
    196670, 333, 332, 131321, 296, 131320, 334, 327745, 24, 335, 21, 30,
    262205, 6, 336, 335, 327850, 16, 338, 336, 337, 196855, 340, 0,
    262394, 338, 339, 381, 131320, 339, 327745, 66, 343, 21, 342, 262205,
    18, 344, 343, 327860, 16, 345, 344, 223, 196855, 348, 0, 262394,
    345, 347, 356, 131320, 347, 262205, 6, 349, 8, 393281, 66, 350,
    64, 23, 349, 262205, 18, 351, 350, 262205, 6, 352, 8, 393281,
    66, 353, 73, 23, 352, 262205, 18, 354, 353, 327811, 18, 355,
    351, 354, 196670, 346, 355, 131321, 348, 131320, 356, 262205, 6, 357,
    8, 393281, 66, 358, 73, 23, 357, 262205, 18, 359, 358, 262205,
    6, 360, 8, 393281, 66, 361, 64, 23, 360, 262205, 18, 362,
    361, 327811, 18, 363, 359, 362, 196670, 346, 363, 131321, 348, 131320,
    348, 262205, 18, 364, 346, 196670, 341, 364, 262205, 6, 365, 8,
    262205, 18, 366, 341, 327745, 24, 368, 21, 23, 262205, 6, 369,
    368, 262256, 18, 370, 369, 327816, 18, 371, 367, 370, 327813, 18,
    372, 366, 371, 393281, 66, 377, 376, 23, 23, 262205, 18, 378,
    377, 327813, 18, 379, 372, 378, 393281, 66, 380, 85, 23, 365,
    196670, 380, 379, 131321, 340, 131320, 381, 327745, 24, 382, 21, 30,
    262205, 6, 383, 382, 327850, 16, 384, 383, 47, 196855, 386, 0,
    262394, 384, 385, 395, 131320, 385, 262205, 6, 387, 8, 262205, 6,
    388, 8, 393281, 66, 389, 64, 23, 388, 262205, 18, 390, 389,
    327745, 66, 391, 21, 342, 262205, 18, 392, 391, 327813, 18, 393,
    390, 392, 393281, 66, 394, 85, 23, 387, 196670, 394, 393, 131321,
    386, 131320, 395, 327745, 24, 396, 21, 30, 262205, 6, 397, 396,
    327850, 16, 399, 397, 398, 196855, 401, 0, 262394, 399, 400, 448,
    131320, 400, 262205, 6, 402, 8, 327851, 16, 403, 402, 12, 196855,
    405, 0, 262394, 403, 404, 405, 131320, 404, 65789, 131320, 405, 196670,
    407, 223, 196670, 408, 12, 131321, 409, 131320, 409, 262390, 411, 412,
    0, 131321, 413, 131320, 413, 262205, 6, 414, 408, 327745, 24, 415,
    21, 23, 262205, 6, 416, 415, 327856, 16, 417, 414, 416, 262394,
    417, 410, 411, 131320, 410, 262205, 6, 418, 408, 393281, 66, 419,
    64, 23, 418, 262205, 18, 420, 419, 262205, 6, 421, 408, 393281,
    66, 422, 73, 23, 421, 262205, 18, 423, 422, 327811, 18, 424,
    420, 423, 327813, 18, 425, 367, 424, 262205, 6, 426, 408, 393281,
    66, 427, 376, 23, 426, 262205, 18, 428, 427, 262205, 6, 433,
    408, 393281, 66, 434, 432, 23, 433, 262205, 18, 435, 434, 327811,
    18, 436, 428, 435, 327813, 18, 437, 425, 436, 262205, 18, 438,
    407, 327809, 18, 439, 438, 437, 196670, 407, 439, 131321, 412, 131320,
    412, 262205, 6, 440, 408, 327808, 6, 441, 440, 255, 196670, 408,
    441, 131321, 409, 131320, 411, 262205, 18, 442, 407, 327745, 24, 443,
    21, 23, 262205, 6, 444, 443, 262256, 18, 445, 444, 327816, 18,
    446, 442, 445, 393281, 66, 447, 85, 23, 23, 196670, 447, 446,
    131321, 401, 131320, 448, 327745, 24, 449, 21, 30, 262205, 6, 450,
    449, 327850, 16, 452, 450, 451, 196855, 454, 0, 262394, 452, 453,
    467, 131320, 453, 262205, 6, 455, 8, 262205, 6, 456, 8, 393281,
    66, 457, 64, 23, 456, 262205, 18, 458, 457, 327745, 66, 459,
    21, 342, 262205, 18, 460, 459, 262205, 6, 461, 8, 393281, 66,
    462, 73, 23, 461, 262205, 18, 463, 462, 327813, 18, 464, 460,
    463, 327811, 18, 465, 458, 464, 393281, 66, 466, 85, 23, 455,
    196670, 466, 465, 131321, 454, 131320, 467, 262205, 6, 468, 8, 262205,
    6, 469, 8, 393281, 66, 470, 64, 23, 469, 262205, 18, 471,
    470, 393281, 66, 472, 85, 23, 468, 196670, 472, 471, 131321, 454,
    131320, 454, 131321, 401, 131320, 401, 131321, 386, 131320, 386, 131321, 340,
    131320, 340, 131321, 296, 131320, 296, 131321, 265, 131320, 265, 131321, 210,
    131320, 210, 131321, 151, 131320, 151, 131321, 58, 131320, 58, 65789, 65592,
];
// D-COMPUTE-BACKEND1=D / #1145: the Metal bridge is a Prelude-owned native
// adapter. It stages canonical F32 values into shared Metal buffers, launches
// checked kernels, and reads the result back into the Tensor owner. No host
// engine selects a kernel or supplies a fallback policy.
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod jet_compute_metal {
    use super::JetComputeError;
    use std::ffi::CString;

    type Obj = usize;

    const STATUS_COMPLETED: Obj = 4;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: usize,
        height: usize,
        depth: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Params {
        count: u32,
        rows: u32,
        inner: u32,
        cols: u32,
        op: u32,
        scalar: f32,
    }

    const SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;
#pragma clang fp contract(off)

struct JetParams {
    uint count;
    uint rows;
    uint inner;
    uint cols;
    uint op;
    float scalar;
};

kernel void jet_binary(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    float left = a[id];
    float right = b[id];
    switch (p.op) {
        case 0: out[id] = left + right; break;
        case 1: out[id] = left * right; break;
        case 2: out[id] = left - right; break;
        case 3: out[id] = left / right; break;
        case 4: out[id] = max(left, right); break;
        case 5: out[id] = min(left, right); break;
        default: out[id] = 0.0f; break;
    }
}

kernel void jet_unary(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    float value = a[id];
    switch (p.op) {
        case 0: out[id] = -value; break;
        case 1: out[id] = abs(value); break;
        case 2: out[id] = exp(value); break;
        case 3: out[id] = log(value); break;
        case 4: out[id] = sqrt(value); break;
        default: out[id] = 0.0f; break;
    }
}

kernel void jet_matmul(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    uint total = p.rows * p.cols;
    if (id >= total) return;
    uint row = id / p.cols;
    uint col = id % p.cols;
    float sum = 0.0f;
    for (uint inner = 0; inner < p.inner; inner++) {
        sum = sum + a[row * p.inner + inner] * b[inner * p.cols + col];
    }
    out[id] = sum;
}

kernel void jet_sum(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id != 0) return;
    float sum = 0.0f;
    for (uint index = 0; index < p.count; index++) sum = sum + a[index];
    out[0] = sum;
}

kernel void jet_mse(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    if (id != 0) return;
    float sum = 0.0f;
    for (uint index = 0; index < p.count; index++) {
        float difference = a[index] - b[index];
        sum = sum + difference * difference;
    }
    out[0] = sum / float(p.count);
}

kernel void jet_mse_grad(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device const float* cot [[buffer(2)]],
    device float* out [[buffer(3)]],
    constant JetParams& p [[buffer(4)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    float difference = p.op == 0 ? a[id] - b[id] : b[id] - a[id];
    out[id] = difference * (2.0f / float(p.count)) * cot[0];
}

kernel void jet_mse_jvp(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device const float* at [[buffer(2)]],
    device const float* bt [[buffer(3)]],
    device float* out [[buffer(4)]],
    constant JetParams& p [[buffer(5)]],
    uint id [[thread_position_in_grid]]) {
    if (id != 0) return;
    float sum = 0.0f;
    for (uint index = 0; index < p.count; index++) {
        sum = sum + 2.0f * (a[index] - b[index]) * (at[index] - bt[index]);
    }
    out[0] = sum / float(p.count);
}

kernel void jet_sgd(
    device const float* parameter [[buffer(0)]],
    device const float* gradient [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    out[id] = parameter[id] - p.scalar * gradient[id];
}

kernel void jet_scale(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    out[id] = a[id] * p.scalar;
}

kernel void jet_copy(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    out[id] = a[id];
}
"#;

// JET_VETTED_UNSAFE_BEGIN: jet_compute_metal
#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    fn MTLCreateSystemDefaultDevice() -> Obj;
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const i8) -> Obj;
    fn sel_registerName(name: *const i8) -> Obj;
    fn objc_msgSend(receiver: Obj, selector: Obj, ...) -> Obj;
}

struct Object(Obj);

impl Object {
    fn new(value: Obj, label: &str) -> Result<Self, JetComputeError> {
        if value == 0 {
            return Err(JetComputeError::Device(format!(
                "Metal {label} allocation or launch failed"
            )));
        }
        Ok(Self(value))
    }

    fn raw(&self) -> Obj {
        self.0
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            msg0(self.0, b"release\0");
        }
    }
}

unsafe fn selector(name: &[u8]) -> Obj {
    unsafe { sel_registerName(name.as_ptr().cast()) }
}

unsafe fn msg0(receiver: Obj, name: &[u8]) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name)) }
}

unsafe fn msg1(receiver: Obj, name: &[u8], first: Obj) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first) }
}

unsafe fn msg2(receiver: Obj, name: &[u8], first: Obj, second: Obj) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first, second) }
}

unsafe fn msg3(receiver: Obj, name: &[u8], first: Obj, second: Obj, third: Obj) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first, second, third) }
}

unsafe fn msg_size2(receiver: Obj, name: &[u8], first: Size, second: Size) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first, second) }
}

fn string(value: &str) -> Result<Object, JetComputeError> {
    let value = CString::new(value).map_err(|_| {
        JetComputeError::Unsupported("Metal source name contains a NUL byte".to_string())
    })?;
    unsafe {
        let class = objc_getClass(b"NSString\0".as_ptr().cast());
        if class == 0 {
            return Err(JetComputeError::Device(
                "Metal Objective-C NSString class is unavailable".to_string(),
            ));
        }
        let allocated = msg0(class, b"alloc\0");
        if allocated == 0 {
            return Err(JetComputeError::Device(
                "Metal could not allocate an Objective-C string".to_string(),
            ));
        }
        let result = msg1(allocated, b"initWithUTF8String:\0", value.as_ptr() as Obj);
        if result == 0 {
            msg0(allocated, b"release\0");
            return Err(JetComputeError::Device(
                "Metal could not create an Objective-C string".to_string(),
            ));
        }
        Object::new(result, "string")
    }
}

fn device() -> Result<Object, JetComputeError> {
    unsafe { Object::new(MTLCreateSystemDefaultDevice(), "device") }
}

pub fn available() -> bool {
    let Ok(device) = device() else {
        return false;
    };
    drop(device);
    true
}

fn object(value: Obj, label: &str) -> Result<Object, JetComputeError> {
    Object::new(value, label)
}

fn u32_value(value: usize, label: &str) -> Result<u32, JetComputeError> {
    u32::try_from(value).map_err(|_| {
        JetComputeError::InvalidShape(format!("Metal {label} exceeds the u32 kernel limit"))
    })
}

fn run(
    function_name: &str,
    inputs: &[&[f32]],
    output_len: usize,
    params: Params,
    output_index: usize,
    params_index: usize,
) -> Result<Vec<f32>, JetComputeError> {
    if output_len == 0 {
        return Ok(Vec::new());
    }
    let device = device()?;
    let source = string(SHADER)?;
    let function_name = string(function_name)?;
    let options_class = unsafe { objc_getClass(b"MTLCompileOptions\0".as_ptr().cast()) };
    if options_class == 0 {
        return Err(JetComputeError::Device(
            "Metal compile options are unavailable".to_string(),
        ));
    }
    let options = unsafe {
        let allocated = msg0(options_class, b"alloc\0");
        let initialized = msg0(allocated, b"init\0");
        let options = object(initialized, "compile options")?;
        msg1(options.raw(), b"setFastMathEnabled:\0", 0);
        options
    };
    let mut compile_error = 0;
    let library = unsafe {
        object(
            msg3(
                device.raw(),
                b"newLibraryWithSource:options:error:\0",
                source.raw(),
                options.raw(),
                (&mut compile_error as *mut Obj) as Obj,
            ),
            "library",
        )
        .map_err(|_| {
            JetComputeError::Unsupported(
                "Metal shader compilation rejected the requested kernel".to_string(),
            )
        })?
    };
    let function = unsafe {
        object(
            msg1(
                library.raw(),
                b"newFunctionWithName:\0",
                function_name.raw(),
            ),
            "kernel function",
        )
        .map_err(|_| {
            JetComputeError::Unsupported(
                "Metal shader does not contain the requested kernel".to_string(),
            )
        })?
    };
    let mut pipeline_error = 0;
    let pipeline = unsafe {
        object(
            msg2(
                device.raw(),
                b"newComputePipelineStateWithFunction:error:\0",
                function.raw(),
                (&mut pipeline_error as *mut Obj) as Obj,
            ),
            "compute pipeline",
        )
        .map_err(|_| {
            JetComputeError::Unsupported(
                "Metal rejected the requested kernel pipeline".to_string(),
            )
        })?
    };
    let queue = unsafe {
        object(msg0(device.raw(), b"newCommandQueue\0"), "command queue")?
    };
    let command = unsafe {
        object(msg0(queue.raw(), b"commandBuffer\0"), "command buffer")?
    };
    let encoder = unsafe {
        object(
            msg0(command.raw(), b"computeCommandEncoder\0"),
            "compute encoder",
        )?
    };
    let mut input_buffers = Vec::with_capacity(inputs.len());
    for values in inputs {
        let bytes = values
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| JetComputeError::InvalidShape("Metal buffer size overflow".to_string()))?;
        let buffer = unsafe {
            object(
                msg3(
                    device.raw(),
                    b"newBufferWithBytes:length:options:\0",
                    values.as_ptr() as Obj,
                    bytes,
                    0,
                ),
                "input buffer",
            )?
        };
        input_buffers.push(buffer);
    }
    let output_bytes = output_len
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| JetComputeError::InvalidShape("Metal output buffer size overflow".to_string()))?;
    let output = unsafe {
        object(
            msg2(
                device.raw(),
                b"newBufferWithLength:options:\0",
                output_bytes,
                0,
            ),
            "output buffer",
        )?
    };
    unsafe {
        msg1(
            encoder.raw(),
            b"setComputePipelineState:\0",
            pipeline.raw(),
        );
        for (index, buffer) in input_buffers.iter().enumerate() {
            msg3(
                encoder.raw(),
                b"setBuffer:offset:atIndex:\0",
                buffer.raw(),
                0,
                index,
            );
        }
        msg3(
            encoder.raw(),
            b"setBuffer:offset:atIndex:\0",
            output.raw(),
            0,
            output_index,
        );
        msg3(
            encoder.raw(),
            b"setBytes:length:atIndex:\0",
            (&params as *const Params) as Obj,
            std::mem::size_of::<Params>(),
            params_index,
        );
        msg_size2(
            encoder.raw(),
            b"dispatchThreads:threadsPerThreadgroup:\0",
            Size {
                width: output_len,
                height: 1,
                depth: 1,
            },
            Size {
                width: 1,
                height: 1,
                depth: 1,
            },
        );
        msg0(encoder.raw(), b"endEncoding\0");
        msg0(command.raw(), b"commit\0");
        msg0(command.raw(), b"waitUntilCompleted\0");
        if msg0(command.raw(), b"status\0") != STATUS_COMPLETED {
            return Err(JetComputeError::Device(
                "Metal command buffer failed or device was lost".to_string(),
            ));
        }
        let contents = msg0(output.raw(), b"contents\0");
        if contents == 0 {
            return Err(JetComputeError::Device(
                "Metal output buffer has no CPU-readable contents".to_string(),
            ));
        }
        let values = std::slice::from_raw_parts(contents as *const f32, output_len).to_vec();
        if values.iter().any(|value| !value.is_finite()) {
            return Err(JetComputeError::Arithmetic(
                "Metal kernel produced a non-finite F32 value".to_string(),
            ));
        }
        Ok(values)
    }
}

pub fn copy(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    let count = u32_value(values.len(), "copy count")?;
    run(
        "jet_copy",
        &[values],
        values.len(),
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        1,
        2,
    )
}

pub fn binary(op: u32, left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len() {
        return Err(JetComputeError::InvalidShape(
            "Metal binary inputs have different lengths".to_string(),
        ));
    }
    let count = u32_value(left.len(), "binary count")?;
    run(
        "jet_binary",
        &[left, right],
        left.len(),
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op,
            scalar: 0.0,
        },
        2,
        3,
    )
}

pub fn unary(op: u32, values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    let count = u32_value(values.len(), "unary count")?;
    run(
        "jet_unary",
        &[values],
        values.len(),
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op,
            scalar: 0.0,
        },
        1,
        2,
    )
}

pub fn matmul(
    left: &[f32],
    right: &[f32],
    rows: usize,
    inner: usize,
    cols: usize,
) -> Result<Vec<f32>, JetComputeError> {
    let count = rows
        .checked_mul(cols)
        .ok_or_else(|| JetComputeError::InvalidShape("Metal matmul output size overflow".to_string()))?;
    let params = Params {
        count: u32_value(count, "matmul output")?,
        rows: u32_value(rows, "matmul rows")?,
        inner: u32_value(inner, "matmul inner dimension")?,
        cols: u32_value(cols, "matmul columns")?,
        op: 0,
        scalar: 0.0,
    };
    run("jet_matmul", &[left, right], count, params, 2, 3)
}

pub fn sum(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    let count = u32_value(values.len(), "sum count")?;
    run(
        "jet_sum",
        &[values],
        1,
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        1,
        2,
    )
}

pub fn mse(left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len() || left.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "Metal MSE inputs must have the same non-empty length".to_string(),
        ));
    }
    run(
        "jet_mse",
        &[left, right],
        1,
        Params {
            count: u32_value(left.len(), "MSE count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        2,
        3,
    )
}

pub fn mse_grad(
    left: &[f32],
    right: &[f32],
    cot: &[f32],
    positive: bool,
) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len() || left.is_empty() || cot.len() != 1 {
        return Err(JetComputeError::InvalidShape(
            "Metal MSE gradient inputs have incompatible lengths".to_string(),
        ));
    }
    run(
        "jet_mse_grad",
        &[left, right, cot],
        left.len(),
        Params {
            count: u32_value(left.len(), "MSE gradient count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: u32::from(!positive),
            scalar: 0.0,
        },
        3,
        4,
    )
}

pub fn mse_jvp(
    left: &[f32],
    right: &[f32],
    left_tangent: &[f32],
    right_tangent: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len()
        || left.len() != left_tangent.len()
        || left.len() != right_tangent.len()
        || left.is_empty()
    {
        return Err(JetComputeError::InvalidShape(
            "Metal MSE JVP inputs have incompatible lengths".to_string(),
        ));
    }
    run(
        "jet_mse_jvp",
        &[left, right, left_tangent, right_tangent],
        1,
        Params {
            count: u32_value(left.len(), "MSE JVP count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        4,
        5,
    )
}

pub fn sgd(
    parameter: &[f32],
    gradient: &[f32],
    learning_rate: f32,
) -> Result<Vec<f32>, JetComputeError> {
    if parameter.len() != gradient.len() {
        return Err(JetComputeError::InvalidShape(
            "Metal SGD inputs have different lengths".to_string(),
        ));
    }
    run(
        "jet_sgd",
        &[parameter, gradient],
        parameter.len(),
        Params {
            count: u32_value(parameter.len(), "SGD count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: learning_rate,
        },
        2,
        3,
    )
}

pub fn scale(values: &[f32], scalar: f32) -> Result<Vec<f32>, JetComputeError> {
    run(
        "jet_scale",
        &[values],
        values.len(),
        Params {
            count: u32_value(values.len(), "scale count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar,
        },
        1,
        2,
    )
}
}
// JET_VETTED_UNSAFE_END: jet_compute_metal

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod jet_compute_metal {
    use super::JetComputeError;

    fn unavailable<T>() -> Result<T, JetComputeError> {
        Err(JetComputeError::Unsupported(
            "Metal backend is unavailable on this target".to_string(),
        ))
    }

    pub fn available() -> bool {
        false
    }

    pub fn copy(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn binary(_: u32, _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn unary(_: u32, _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn matmul(_: &[f32], _: &[f32], _: usize, _: usize, _: usize) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sum(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse(_: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_grad(_: &[f32], _: &[f32], _: &[f32], _: bool) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_jvp(_: &[f32], _: &[f32], _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sgd(_: &[f32], _: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn scale(_: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
}

// D-COMPUTE-BACKEND1=D / #1144: CUDA is a dynamically loaded Driver API
// bridge. The compiler and generated program need no CUDA toolkit or link-time
// stub. A request succeeds only when the installed driver exposes the complete
// checked launch surface; otherwise it fails closed without a CPU fallback.
// JET_VETTED_UNSAFE_BEGIN: jet_compute_cuda
#[cfg(target_os = "linux")]
mod jet_compute_cuda {
    use super::JetComputeError;
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::ptr;
    use std::sync::Arc;

    type CuDevice = c_int;
    type CuDevicePtr = u64;
    type CuContext = *mut c_void;
    type CuModule = *mut c_void;
    type CuFunction = *mut c_void;
    type CuStream = *mut c_void;
    type CuInit = unsafe extern "C" fn(u32) -> c_int;
    type CuDeviceGetCount = unsafe extern "C" fn(*mut c_int) -> c_int;
    type CuDeviceGet = unsafe extern "C" fn(*mut CuDevice, c_int) -> c_int;
    type CuCtxCreate = unsafe extern "C" fn(*mut CuContext, u32, CuDevice) -> c_int;
    type CuCtxDestroy = unsafe extern "C" fn(CuContext) -> c_int;
    type CuCtxSetCurrent = unsafe extern "C" fn(CuContext) -> c_int;
    type CuCtxSynchronize = unsafe extern "C" fn() -> c_int;
    type CuModuleLoadData = unsafe extern "C" fn(*mut CuModule, *const c_void) -> c_int;
    type CuModuleUnload = unsafe extern "C" fn(CuModule) -> c_int;
    type CuModuleGetFunction = unsafe extern "C" fn(
        *mut CuFunction,
        CuModule,
        *const c_char,
    ) -> c_int;
    type CuMemAlloc = unsafe extern "C" fn(*mut CuDevicePtr, usize) -> c_int;
    type CuMemFree = unsafe extern "C" fn(CuDevicePtr) -> c_int;
    type CuMemcpyHtoD = unsafe extern "C" fn(CuDevicePtr, *const c_void, usize) -> c_int;
    type CuMemcpyDtoH = unsafe extern "C" fn(*mut c_void, CuDevicePtr, usize) -> c_int;
    type CuLaunchKernel = unsafe extern "C" fn(
        CuFunction,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        CuStream,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> c_int;
    type CuStreamCreate = unsafe extern "C" fn(*mut CuStream, u32) -> c_int;
    type CuStreamDestroy = unsafe extern "C" fn(CuStream) -> c_int;
    type CuStreamSynchronize = unsafe extern "C" fn(CuStream) -> c_int;

    const CUDA_SUCCESS: c_int = 0;
    const RTLD_NOW: c_int = 2;
    const RTLD_LOCAL: c_int = 4;
    const BLOCK_SIZE: usize = 256;

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Params {
        count: u32,
        rows: u32,
        inner: u32,
        cols: u32,
        op: u32,
        scalar: f32,
    }

    const PTX: &str = concat!(
        r#".version 6.0
.target sm_50
.address_size 64

.visible .entry jet_copy(
    .param .u64 a,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<2>;
    .reg .b64 %rd<6>;
    .reg .f32 %f;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [out];
    ld.param.u64 %rd2, [params];
    ld.global.u32 %r1, [%rd2];
    mov.u32 %r0, %tid.x;
    setp.ge.u32 %p, %r0, %r1;
    @%p bra copy_done;
    mul.wide.u32 %rd3, %r0, 4;
    add.s64 %rd4, %rd0, %rd3;
    add.s64 %rd5, %rd1, %rd3;
    ld.global.f32 %f, [%rd4];
    st.global.f32 [%rd5], %f;
copy_done:
    ret;
}

.visible .entry jet_binary(
    .param .u64 a,
    .param .u64 b,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<3>;
    .reg .b64 %rd<8>;
    .reg .f32 %f<3>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [b];
    ld.param.u64 %rd2, [out];
    ld.param.u64 %rd3, [params];
    ld.global.u32 %r1, [%rd3];
    ld.global.u32 %r2, [%rd3+16];
    mov.u32 %r0, %tid.x;
    setp.ge.u32 %p, %r0, %r1;
    @%p bra binary_done;
    mul.wide.u32 %rd4, %r0, 4;
    add.s64 %rd5, %rd0, %rd4;
    add.s64 %rd6, %rd1, %rd4;
    add.s64 %rd7, %rd2, %rd4;
    ld.global.f32 %f0, [%rd5];
    ld.global.f32 %f1, [%rd6];
    setp.eq.u32 %p, %r2, 0;
    @%p bra binary_add;
    setp.eq.u32 %p, %r2, 1;
    @%p bra binary_mul;
    setp.eq.u32 %p, %r2, 2;
    @%p bra binary_sub;
    setp.eq.u32 %p, %r2, 3;
    @%p bra binary_div;
    setp.eq.u32 %p, %r2, 4;
    @%p bra binary_max;
    min.f32 %f2, %f0, %f1;
    bra binary_store;
binary_add:
    add.rn.f32 %f2, %f0, %f1;
    bra binary_store;
binary_mul:
    mul.rn.f32 %f2, %f0, %f1;
    bra binary_store;
binary_sub:
    sub.rn.f32 %f2, %f0, %f1;
    bra binary_store;
binary_div:
    div.rn.f32 %f2, %f0, %f1;
    bra binary_store;
binary_max:
    max.f32 %f2, %f0, %f1;
binary_store:
    st.global.f32 [%rd7], %f2;
binary_done:
    ret;
}

.visible .entry jet_unary(
    .param .u64 a,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<3>;
    .reg .b64 %rd<6>;
    .reg .f32 %f<4>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [out];
    ld.param.u64 %rd2, [params];
    ld.global.u32 %r1, [%rd2];
    ld.global.u32 %r2, [%rd2+16];
    mov.u32 %r0, %tid.x;
    setp.ge.u32 %p, %r0, %r1;
    @%p bra unary_done;
    mul.wide.u32 %rd3, %r0, 4;
    add.s64 %rd4, %rd0, %rd3;
    add.s64 %rd5, %rd1, %rd3;
    ld.global.f32 %f0, [%rd4];
    setp.eq.u32 %p, %r2, 0;
    @%p bra unary_negate;
    setp.eq.u32 %p, %r2, 1;
    @%p bra unary_abs;
    setp.eq.u32 %p, %r2, 2;
    @%p bra unary_exp;
    setp.eq.u32 %p, %r2, 3;
    @%p bra unary_log;
    sqrt.approx.f32 %f1, %f0;
    bra unary_store;
unary_negate:
    neg.f32 %f1, %f0;
    bra unary_store;
unary_abs:
    abs.f32 %f1, %f0;
    bra unary_store;
unary_exp:
    mul.f32 %f2, %f0, 1.4426950408889634;
    ex2.approx.f32 %f1, %f2;
    bra unary_store;
unary_log:
    lg2.approx.f32 %f1, %f0;
    mul.f32 %f1, %f1, 0.6931471805599453;
unary_store:
    st.global.f32 [%rd5], %f1;
unary_done:
    ret;
}

.visible .entry jet_matmul(
    .param .u64 a,
    .param .u64 b,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<10>;
    .reg .b64 %rd<10>;
    .reg .f32 %f<4>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [b];
    ld.param.u64 %rd2, [out];
    ld.param.u64 %rd3, [params];
    ld.global.u32 %r1, [%rd3+4];
    ld.global.u32 %r2, [%rd3+8];
    ld.global.u32 %r3, [%rd3+12];
    mov.u32 %r0, %tid.x;
    mul.lo.u32 %r4, %r1, %r3;
    setp.ge.u32 %p, %r0, %r4;
    @%p bra matmul_done;
    div.u32 %r5, %r0, %r3;
    rem.u32 %r6, %r0, %r3;
    mov.u32 %r7, 0;
    mov.f32 %f3, 0f00000000;
matmul_loop:
    setp.ge.u32 %p, %r7, %r2;
    @%p bra matmul_store;
    mul.lo.u32 %r8, %r5, %r2;
    add.u32 %r8, %r8, %r7;
    mul.wide.u32 %rd4, %r8, 4;
    add.s64 %rd5, %rd0, %rd4;
    mul.lo.u32 %r9, %r7, %r3;
    add.u32 %r9, %r9, %r6;
    mul.wide.u32 %rd6, %r9, 4;
    add.s64 %rd7, %rd1, %rd6;
    ld.global.f32 %f0, [%rd5];
    ld.global.f32 %f1, [%rd7];
    mul.rn.f32 %f2, %f0, %f1;
    add.rn.f32 %f3, %f3, %f2;
    add.u32 %r7, %r7, 1;
    bra matmul_loop;
matmul_store:
    mul.wide.u32 %rd8, %r0, 4;
    add.s64 %rd9, %rd2, %rd8;
    st.global.f32 [%rd9], %f3;
matmul_done:
    ret;
}

.visible .entry jet_sum(
    .param .u64 a,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<3>;
    .reg .b64 %rd<7>;
    .reg .f32 %f<3>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [out];
    ld.param.u64 %rd2, [params];
    ld.global.u32 %r1, [%rd2];
    mov.u32 %r0, %tid.x;
    setp.ne.u32 %p, %r0, 0;
    @%p bra sum_done;
    mov.u32 %r2, 0;
    mov.f32 %f0, 0f00000000;
sum_loop:
    setp.ge.u32 %p, %r2, %r1;
    @%p bra sum_store;
    mul.wide.u32 %rd3, %r2, 4;
    add.s64 %rd4, %rd0, %rd3;
    ld.global.f32 %f1, [%rd4];
    add.rn.f32 %f0, %f0, %f1;
    add.u32 %r2, %r2, 1;
    bra sum_loop;
sum_store:
    st.global.f32 [%rd1], %f0;
sum_done:
    ret;
}

.visible .entry jet_mse(
    .param .u64 a,
    .param .u64 b,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<3>;
    .reg .b64 %rd<9>;
    .reg .f32 %f<5>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [b];
    ld.param.u64 %rd2, [out];
    ld.param.u64 %rd3, [params];
    ld.global.u32 %r1, [%rd3];
    mov.u32 %r0, %tid.x;
    setp.ne.u32 %p, %r0, 0;
    @%p bra mse_done;
    mov.u32 %r2, 0;
    mov.f32 %f0, 0f00000000;
mse_loop:
    setp.ge.u32 %p, %r2, %r1;
    @%p bra mse_store;
    mul.wide.u32 %rd4, %r2, 4;
    add.s64 %rd5, %rd0, %rd4;
    add.s64 %rd6, %rd1, %rd4;
    ld.global.f32 %f1, [%rd5];
    ld.global.f32 %f2, [%rd6];
    sub.rn.f32 %f3, %f1, %f2;
    mul.rn.f32 %f4, %f3, %f3;
    add.rn.f32 %f0, %f0, %f4;
    add.u32 %r2, %r2, 1;
    bra mse_loop;
mse_store:
    cvt.rn.f32.u32 %f1, %r1;
    div.rn.f32 %f0, %f0, %f1;
    st.global.f32 [%rd2], %f0;
mse_done:
    ret;
}

.visible .entry jet_mse_grad(
    .param .u64 a,
    .param .u64 b,
    .param .u64 cot,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<4>;
    .reg .b64 %rd<11>;
    .reg .f32 %f<7>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [b];
    ld.param.u64 %rd2, [cot];
    ld.param.u64 %rd3, [out];
    ld.param.u64 %rd4, [params];
    ld.global.u32 %r1, [%rd4];
    ld.global.u32 %r2, [%rd4+16];
    mov.u32 %r0, %tid.x;
    setp.ge.u32 %p, %r0, %r1;
    @%p bra mse_grad_done;
    mul.wide.u32 %rd5, %r0, 4;
    add.s64 %rd6, %rd0, %rd5;
    add.s64 %rd7, %rd1, %rd5;
    add.s64 %rd8, %rd3, %rd5;
    ld.global.f32 %f0, [%rd6];
    ld.global.f32 %f1, [%rd7];
    setp.eq.u32 %p, %r2, 0;
    @%p bra mse_grad_positive;
    sub.rn.f32 %f2, %f1, %f0;
    bra mse_grad_factor;
mse_grad_positive:
    sub.rn.f32 %f2, %f0, %f1;
mse_grad_factor:
    mov.f32 %f3, 0f40000000;
    cvt.rn.f32.u32 %f4, %r1;
    div.rn.f32 %f3, %f3, %f4;
    ld.global.f32 %f5, [%rd2];
    mul.rn.f32 %f3, %f3, %f5;
    mul.rn.f32 %f6, %f2, %f3;
    st.global.f32 [%rd8], %f6;
mse_grad_done:
    ret;
}

.visible .entry jet_mse_jvp(
    .param .u64 a,
    .param .u64 b,
    .param .u64 at,
    .param .u64 bt,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<3>;
    .reg .b64 %rd<13>;
    .reg .f32 %f<8>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [b];
    ld.param.u64 %rd2, [at];
    ld.param.u64 %rd3, [bt];
    ld.param.u64 %rd4, [out];
    ld.param.u64 %rd5, [params];
    ld.global.u32 %r1, [%rd5];
    mov.u32 %r0, %tid.x;
    setp.ne.u32 %p, %r0, 0;
    @%p bra mse_jvp_done;
    mov.u32 %r2, 0;
    mov.f32 %f0, 0f00000000;
mse_jvp_loop:
    setp.ge.u32 %p, %r2, %r1;
    @%p bra mse_jvp_store;
    mul.wide.u32 %rd6, %r2, 4;
    add.s64 %rd7, %rd0, %rd6;
    add.s64 %rd8, %rd1, %rd6;
    add.s64 %rd9, %rd2, %rd6;
    add.s64 %rd10, %rd3, %rd6;
    ld.global.f32 %f1, [%rd7];
    ld.global.f32 %f2, [%rd8];
    ld.global.f32 %f3, [%rd9];
    ld.global.f32 %f4, [%rd10];
    sub.rn.f32 %f5, %f1, %f2;
    sub.rn.f32 %f6, %f3, %f4;
    mul.rn.f32 %f7, %f5, %f6;
    add.rn.f32 %f7, %f7, %f7;
    add.rn.f32 %f0, %f0, %f7;
    add.u32 %r2, %r2, 1;
    bra mse_jvp_loop;
mse_jvp_store:
    cvt.rn.f32.u32 %f1, %r1;
    div.rn.f32 %f0, %f0, %f1;
    st.global.f32 [%rd4], %f0;
mse_jvp_done:
    ret;
}

.visible .entry jet_sgd(
    .param .u64 parameter,
    .param .u64 gradient,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<2>;
    .reg .b64 %rd<8>;
    .reg .f32 %f<4>;
    ld.param.u64 %rd0, [parameter];
    ld.param.u64 %rd1, [gradient];
    ld.param.u64 %rd2, [out];
    ld.param.u64 %rd3, [params];
    ld.global.u32 %r1, [%rd3];
    ld.global.f32 %f0, [%rd3+20];
    mov.u32 %r0, %tid.x;
    setp.ge.u32 %p, %r0, %r1;
    @%p bra sgd_done;
    mul.wide.u32 %rd4, %r0, 4;
    add.s64 %rd5, %rd0, %rd4;
    add.s64 %rd6, %rd1, %rd4;
    add.s64 %rd7, %rd2, %rd4;
    ld.global.f32 %f1, [%rd5];
    ld.global.f32 %f2, [%rd6];
    mul.rn.f32 %f3, %f0, %f2;
    sub.rn.f32 %f3, %f1, %f3;
    st.global.f32 [%rd7], %f3;
sgd_done:
    ret;
}

.visible .entry jet_scale(
    .param .u64 a,
    .param .u64 out,
    .param .u64 params) {
    .reg .pred %p;
    .reg .b32 %r<2>;
    .reg .b64 %rd<6>;
    .reg .f32 %f<3>;
    ld.param.u64 %rd0, [a];
    ld.param.u64 %rd1, [out];
    ld.param.u64 %rd2, [params];
    ld.global.u32 %r1, [%rd2];
    ld.global.f32 %f0, [%rd2+20];
    mov.u32 %r0, %tid.x;
    setp.ge.u32 %p, %r0, %r1;
    @%p bra scale_done;
    mul.wide.u32 %rd3, %r0, 4;
    add.s64 %rd4, %rd0, %rd3;
    add.s64 %rd5, %rd1, %rd3;
    ld.global.f32 %f1, [%rd4];
    mul.rn.f32 %f2, %f1, %f0;
    st.global.f32 [%rd5], %f2;
scale_done:
    ret;
}
"#,
        "\0"
    );

    struct Api {
        init: CuInit,
        device_get_count: CuDeviceGetCount,
        device_get: CuDeviceGet,
        ctx_create: CuCtxCreate,
        ctx_destroy: CuCtxDestroy,
        ctx_set_current: CuCtxSetCurrent,
        ctx_synchronize: CuCtxSynchronize,
        module_load_data: CuModuleLoadData,
        module_unload: CuModuleUnload,
        module_get_function: CuModuleGetFunction,
        mem_alloc: CuMemAlloc,
        mem_free: CuMemFree,
        memcpy_htod: CuMemcpyHtoD,
        memcpy_dtoh: CuMemcpyDtoH,
        launch_kernel: CuLaunchKernel,
        stream_create: CuStreamCreate,
        stream_destroy: CuStreamDestroy,
        stream_synchronize: CuStreamSynchronize,
    }

    impl Api {
        fn load() -> Result<Arc<Self>, JetComputeError> {
            let handle = [b"libcuda.so.1\0".as_slice(), b"libcuda.so\0".as_slice()]
                .into_iter()
                .find_map(|path| {
                    let handle = unsafe { dlopen(path.as_ptr().cast(), RTLD_NOW | RTLD_LOCAL) };
                    (!handle.is_null()).then_some(handle)
                })
                .ok_or_else(|| {
                    JetComputeError::Unsupported(
                        "CUDA driver library is unavailable on this target".to_string(),
                    )
                })?;

            macro_rules! symbol {
                ($name:literal, $ty:ty) => {{
                    let name = concat!($name, "\0");
                    let pointer = unsafe { dlsym(handle, name.as_ptr().cast()) };
                    if pointer.is_null() {
                        return Err(JetComputeError::Unsupported(format!(
                            "CUDA driver symbol `{}` is unavailable",
                            $name
                        )));
                    }
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(pointer) }
                }};
            }

            let api = Arc::new(Self {
                init: symbol!("cuInit", CuInit),
                device_get_count: symbol!("cuDeviceGetCount", CuDeviceGetCount),
                device_get: symbol!("cuDeviceGet", CuDeviceGet),
                ctx_create: symbol!("cuCtxCreate_v2", CuCtxCreate),
                ctx_destroy: symbol!("cuCtxDestroy_v2", CuCtxDestroy),
                ctx_set_current: symbol!("cuCtxSetCurrent", CuCtxSetCurrent),
                ctx_synchronize: symbol!("cuCtxSynchronize", CuCtxSynchronize),
                module_load_data: symbol!("cuModuleLoadData", CuModuleLoadData),
                module_unload: symbol!("cuModuleUnload", CuModuleUnload),
                module_get_function: symbol!("cuModuleGetFunction", CuModuleGetFunction),
                mem_alloc: symbol!("cuMemAlloc_v2", CuMemAlloc),
                mem_free: symbol!("cuMemFree_v2", CuMemFree),
                memcpy_htod: symbol!("cuMemcpyHtoD_v2", CuMemcpyHtoD),
                memcpy_dtoh: symbol!("cuMemcpyDtoH_v2", CuMemcpyDtoH),
                launch_kernel: symbol!("cuLaunchKernel", CuLaunchKernel),
                stream_create: symbol!("cuStreamCreate", CuStreamCreate),
                stream_destroy: symbol!("cuStreamDestroy_v2", CuStreamDestroy),
                stream_synchronize: symbol!("cuStreamSynchronize", CuStreamSynchronize),
            });
            check(unsafe { (api.init)(0) }, "driver initialization")?;
            Ok(api)
        }
    }

    fn check(code: c_int, operation: &str) -> Result<(), JetComputeError> {
        if code == CUDA_SUCCESS {
            Ok(())
        } else {
            Err(JetComputeError::Device(format!(
                "CUDA {operation} failed (driver status {code})"
            )))
        }
    }

    struct Context {
        api: Arc<Api>,
        raw: CuContext,
    }

    impl Context {
        fn new(api: Arc<Api>) -> Result<Self, JetComputeError> {
            let mut device = 0;
            check(
                unsafe { (api.device_get)(&mut device, 0) },
                "device selection",
            )?;
            let mut raw = ptr::null_mut();
            check(
                unsafe { (api.ctx_create)(&mut raw, 0, device) },
                "context creation",
            )?;
            Ok(Self { api, raw })
        }

        fn make_current(&self) -> Result<(), JetComputeError> {
            check(
                unsafe { (self.api.ctx_set_current)(self.raw) },
                "context selection",
            )
        }

        fn synchronize(&self) -> Result<(), JetComputeError> {
            self.make_current()?;
            check(
                unsafe { (self.api.ctx_synchronize)() },
                "kernel synchronization",
            )
        }
    }

    impl Drop for Context {
        fn drop(&mut self) {
            unsafe {
                (self.api.ctx_destroy)(self.raw);
            }
        }
    }

    struct Module {
        api: Arc<Api>,
        raw: CuModule,
    }

    impl Module {
        fn load(context: &Context) -> Result<Self, JetComputeError> {
            context.make_current()?;
            let mut raw = ptr::null_mut();
            check(
                unsafe {
                    (context.api.module_load_data)(
                        &mut raw,
                        PTX.as_ptr().cast::<c_void>(),
                    )
                },
                "PTX module loading",
            )?;
            Ok(Self {
                api: context.api.clone(),
                raw,
            })
        }

        fn function(&self, name: &str) -> Result<CuFunction, JetComputeError> {
            let name = CString::new(name).map_err(|_| {
                JetComputeError::Unsupported("CUDA kernel name contains a NUL byte".to_string())
            })?;
            let mut raw = ptr::null_mut();
            check(
                unsafe { (self.api.module_get_function)(&mut raw, self.raw, name.as_ptr()) },
                "kernel lookup",
            )?;
            Ok(raw)
        }
    }

    impl Drop for Module {
        fn drop(&mut self) {
            unsafe {
                (self.api.module_unload)(self.raw);
            }
        }
    }

    struct Buffer {
        api: Arc<Api>,
        raw: CuDevicePtr,
    }

    impl Buffer {
        fn new(context: &Context, bytes: usize) -> Result<Self, JetComputeError> {
            context.make_current()?;
            let mut raw = 0;
            check(
                unsafe { (context.api.mem_alloc)(&mut raw, bytes) },
                "device allocation",
            )?;
            Ok(Self {
                api: context.api.clone(),
                raw,
            })
        }

        fn from_f32(context: &Context, values: &[f32]) -> Result<Self, JetComputeError> {
            let bytes = values
                .len()
                .checked_mul(std::mem::size_of::<f32>())
                .ok_or_else(|| JetComputeError::InvalidShape("CUDA buffer size overflow".to_string()))?;
            context.make_current()?;
            let buffer = Self::new(context, bytes)?;
            check(
                unsafe {
                    (buffer.api.memcpy_htod)(
                        buffer.raw,
                        values.as_ptr().cast::<c_void>(),
                        bytes,
                    )
                },
                "host-to-device transfer",
            )?;
            Ok(buffer)
        }

        fn copy_from<T>(&self, context: &Context, value: &T) -> Result<(), JetComputeError> {
            let bytes = std::mem::size_of::<T>();
            context.make_current()?;
            check(
                unsafe {
                    (self.api.memcpy_htod)(
                        self.raw,
                        (value as *const T).cast::<c_void>(),
                        bytes,
                    )
                },
                "parameter transfer",
            )
        }

        fn copy_to_f32(&self, context: &Context, values: &mut [f32]) -> Result<(), JetComputeError> {
            let bytes = values
                .len()
                .checked_mul(std::mem::size_of::<f32>())
                .ok_or_else(|| JetComputeError::InvalidShape("CUDA output size overflow".to_string()))?;
            context.make_current()?;
            check(
                unsafe {
                    (self.api.memcpy_dtoh)(
                        values.as_mut_ptr().cast::<c_void>(),
                        self.raw,
                        bytes,
                    )
                },
                "device-to-host transfer",
            )
        }

        fn raw(&self) -> CuDevicePtr {
            self.raw
        }
    }

    impl Drop for Buffer {
        fn drop(&mut self) {
            unsafe {
                (self.api.mem_free)(self.raw);
            }
        }
    }

    fn u32_value(value: usize, label: &str) -> Result<u32, JetComputeError> {
        u32::try_from(value).map_err(|_| {
            JetComputeError::InvalidShape(format!("CUDA {label} exceeds the u32 kernel limit"))
        })
    }

    fn run(
        function_name: &str,
        inputs: &[&[f32]],
        output_len: usize,
        params: Params,
    ) -> Result<Vec<f32>, JetComputeError> {
        if output_len == 0 {
            return Ok(Vec::new());
        }
        let api = Api::load()?;
        let context = Context::new(api.clone())?;
        let module = Module::load(&context)?;
        let function = module.function(function_name)?;
        let input_buffers = inputs
            .iter()
            .map(|values| Buffer::from_f32(&context, values))
            .collect::<Result<Vec<_>, _>>()?;
        let output = Buffer::new(
            &context,
            output_len
                .checked_mul(std::mem::size_of::<f32>())
                .ok_or_else(|| JetComputeError::InvalidShape("CUDA output size overflow".to_string()))?,
        )?;
        let parameter = Buffer::new(&context, std::mem::size_of::<Params>())?;
        parameter.copy_from(&context, &params)?;
        let mut argument_values = input_buffers
            .iter()
            .map(Buffer::raw)
            .collect::<Vec<_>>();
        argument_values.push(output.raw());
        argument_values.push(parameter.raw());
        let mut arguments = argument_values
            .iter_mut()
            .map(|value| (value as *mut u64).cast::<c_void>())
            .collect::<Vec<_>>();
        let grid = output_len
            .checked_add(BLOCK_SIZE - 1)
            .and_then(|value| value.checked_div(BLOCK_SIZE))
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| JetComputeError::InvalidShape("CUDA launch grid is too large".to_string()))?;
        check(
            unsafe {
                (api.launch_kernel)(
                    function,
                    grid,
                    1,
                    1,
                    BLOCK_SIZE as u32,
                    1,
                    1,
                    0,
                    ptr::null_mut(),
                    arguments.as_mut_ptr(),
                    ptr::null_mut(),
                )
            },
            "kernel launch",
        )?;
        context.synchronize()?;
        let mut values = vec![0.0_f32; output_len];
        output.copy_to_f32(&context, &mut values)?;
        if values.iter().any(|value| !value.is_finite()) {
            return Err(JetComputeError::Arithmetic(
                "CUDA kernel produced a non-finite F32 value".to_string(),
            ));
        }
        Ok(values)
    }

    pub fn available() -> bool {
        let Ok(api) = Api::load() else {
            return false;
        };
        let mut count = 0;
        unsafe { (api.device_get_count)(&mut count) == CUDA_SUCCESS && count > 0 }
    }

    pub fn copy(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        run(
            "jet_copy",
            &[values],
            values.len(),
            Params {
                count: u32_value(values.len(), "copy count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 0,
                scalar: 0.0,
            },
        )
    }

    pub fn binary(op: u32, left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len() {
            return Err(JetComputeError::InvalidShape(
                "CUDA binary inputs have different lengths".to_string(),
            ));
        }
        run(
            "jet_binary",
            &[left, right],
            left.len(),
            Params {
                count: u32_value(left.len(), "binary count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op,
                scalar: 0.0,
            },
        )
    }

    pub fn unary(op: u32, values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        run(
            "jet_unary",
            &[values],
            values.len(),
            Params {
                count: u32_value(values.len(), "unary count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op,
                scalar: 0.0,
            },
        )
    }

    pub fn matmul(
        left: &[f32],
        right: &[f32],
        rows: usize,
        inner: usize,
        cols: usize,
    ) -> Result<Vec<f32>, JetComputeError> {
        let count = rows
            .checked_mul(cols)
            .ok_or_else(|| JetComputeError::InvalidShape("CUDA matmul output size overflow".to_string()))?;
        run(
            "jet_matmul",
            &[left, right],
            count,
            Params {
                count: u32_value(count, "matmul output")?,
                rows: u32_value(rows, "matmul rows")?,
                inner: u32_value(inner, "matmul inner dimension")?,
                cols: u32_value(cols, "matmul columns")?,
                op: 0,
                scalar: 0.0,
            },
        )
    }

    pub fn sum(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        run(
            "jet_sum",
            &[values],
            1,
            Params {
                count: u32_value(values.len(), "sum count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 0,
                scalar: 0.0,
            },
        )
    }

    pub fn mse(left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len() || left.is_empty() {
            return Err(JetComputeError::InvalidShape(
                "CUDA MSE inputs must have the same non-empty length".to_string(),
            ));
        }
        run(
            "jet_mse",
            &[left, right],
            1,
            Params {
                count: u32_value(left.len(), "MSE count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 0,
                scalar: 0.0,
            },
        )
    }

    pub fn mse_grad(
        left: &[f32],
        right: &[f32],
        cot: &[f32],
        positive: bool,
    ) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len() || left.is_empty() || cot.len() != 1 {
            return Err(JetComputeError::InvalidShape(
                "CUDA MSE gradient inputs have incompatible lengths".to_string(),
            ));
        }
        run(
            "jet_mse_grad",
            &[left, right, cot],
            left.len(),
            Params {
                count: u32_value(left.len(), "MSE gradient count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: u32::from(!positive),
                scalar: 0.0,
            },
        )
    }

    pub fn mse_jvp(
        left: &[f32],
        right: &[f32],
        left_tangent: &[f32],
        right_tangent: &[f32],
    ) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len()
            || left.len() != left_tangent.len()
            || left.len() != right_tangent.len()
            || left.is_empty()
        {
            return Err(JetComputeError::InvalidShape(
                "CUDA MSE JVP inputs have incompatible lengths".to_string(),
            ));
        }
        run(
            "jet_mse_jvp",
            &[left, right, left_tangent, right_tangent],
            1,
            Params {
                count: u32_value(left.len(), "MSE JVP count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 0,
                scalar: 0.0,
            },
        )
    }

    pub fn sgd(
        parameter: &[f32],
        gradient: &[f32],
        learning_rate: f32,
    ) -> Result<Vec<f32>, JetComputeError> {
        if parameter.len() != gradient.len() {
            return Err(JetComputeError::InvalidShape(
                "CUDA SGD inputs have different lengths".to_string(),
            ));
        }
        run(
            "jet_sgd",
            &[parameter, gradient],
            parameter.len(),
            Params {
                count: u32_value(parameter.len(), "SGD count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 0,
                scalar: learning_rate,
            },
        )
    }

    pub fn scale(values: &[f32], scalar: f32) -> Result<Vec<f32>, JetComputeError> {
        run(
            "jet_scale",
            &[values],
            values.len(),
            Params {
                count: u32_value(values.len(), "scale count")?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 0,
                scalar,
            },
        )
    }

    pub struct StreamHandle {
        context: Context,
        stream: CuStream,
    }

    pub fn stream_new() -> Result<StreamHandle, JetComputeError> {
        let api = Api::load()?;
        let context = Context::new(api)?;
        let mut stream = ptr::null_mut();
        check(
            unsafe { (context.api.stream_create)(&mut stream, 0) },
            "stream creation",
        )?;
        Ok(StreamHandle { context, stream })
    }

    pub fn stream_sync(stream: &StreamHandle) -> Result<(), JetComputeError> {
        stream.context.make_current()?;
        check(
            unsafe { (stream.context.api.stream_synchronize)(stream.stream) },
            "stream synchronization",
        )
    }

    impl Drop for StreamHandle {
        fn drop(&mut self) {
            unsafe {
                (self.context.api.stream_destroy)(self.stream);
            }
        }
    }
}
// JET_VETTED_UNSAFE_END: jet_compute_cuda

#[cfg(not(target_os = "linux"))]
mod jet_compute_cuda {
    use super::JetComputeError;

    pub struct StreamHandle;

    fn unavailable<T>() -> Result<T, JetComputeError> {
        Err(JetComputeError::Unsupported(
            "CUDA backend is unavailable on this target".to_string(),
        ))
    }

    pub fn available() -> bool { false }
    pub fn copy(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn binary(_: u32, _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn unary(_: u32, _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn matmul(_: &[f32], _: &[f32], _: usize, _: usize, _: usize) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sum(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse(_: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_grad(_: &[f32], _: &[f32], _: &[f32], _: bool) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_jvp(_: &[f32], _: &[f32], _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sgd(_: &[f32], _: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn scale(_: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn stream_new() -> Result<StreamHandle, JetComputeError> { unavailable() }
    pub fn stream_sync(_: &StreamHandle) -> Result<(), JetComputeError> { unavailable() }
}

// D-COMPUTE-BACKEND1=D / #1146: Vulkan is a dynamically loaded native
// bridge. It owns instance/device/queue/pipeline setup and launches the
// embedded SPIR-V kernel. The Prelude keeps the CPU oracle and accelerator
// contracts in one place; this module only marshals buffers and commands.
#[cfg(target_os = "linux")]
mod jet_compute_vulkan {
    use super::{JetComputeError, VULKAN_SHADER};
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::ptr;
    use std::sync::Arc;

    type VkResult = i32;
    type VkInstance = *mut c_void;
    type VkPhysicalDevice = *mut c_void;
    type VkDevice = *mut c_void;
    type VkQueue = *mut c_void;
    type VkCommandBuffer = *mut c_void;
    type VkBuffer = u64;
    type VkDeviceMemory = u64;
    type VkShaderModule = u64;
    type VkDescriptorSetLayout = u64;
    type VkPipelineLayout = u64;
    type VkPipeline = u64;
    type VkDescriptorPool = u64;
    type VkDescriptorSet = u64;
    type VkCommandPool = u64;
    type VkFence = u64;

    const VK_SUCCESS: VkResult = 0;
    const VK_STRUCTURE_TYPE_APPLICATION_INFO: u32 = 0;
    const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: u32 = 1;
    const VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO: u32 = 2;
    const VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO: u32 = 3;
    const VK_STRUCTURE_TYPE_SUBMIT_INFO: u32 = 4;
    const VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO: u32 = 5;
    const VK_STRUCTURE_TYPE_FENCE_CREATE_INFO: u32 = 8;
    const VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO: u32 = 12;
    const VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO: u32 = 16;
    const VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO: u32 = 18;
    const VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO: u32 = 29;
    const VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO: u32 = 30;
    const VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO: u32 = 32;
    const VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO: u32 = 33;
    const VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO: u32 = 34;
    const VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET: u32 = 35;
    const VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO: u32 = 39;
    const VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO: u32 = 40;
    const VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO: u32 = 42;
    const VK_QUEUE_COMPUTE_BIT: u32 = 0x00000002;
    const VK_BUFFER_USAGE_STORAGE_BUFFER_BIT: u32 = 0x00000020;
    const VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT: u32 = 0x00000010;
    const VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT: u32 = 0x00000002;
    const VK_MEMORY_PROPERTY_HOST_COHERENT_BIT: u32 = 0x00000004;
    const VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER: u32 = 6;
    const VK_DESCRIPTOR_TYPE_STORAGE_BUFFER: u32 = 7;
    const VK_SHADER_STAGE_COMPUTE_BIT: u32 = 0x00000020;
    const VK_PIPELINE_BIND_POINT_COMPUTE: u32 = 1;
    const VK_COMMAND_BUFFER_LEVEL_PRIMARY: u32 = 0;
    const RTLD_NOW: c_int = 2;
    const RTLD_LOCAL: c_int = 4;

    #[repr(C)]
    struct ApplicationInfo {
        s_type: u32,
        p_next: *const c_void,
        p_application_name: *const c_char,
        application_version: u32,
        p_engine_name: *const c_char,
        engine_version: u32,
        api_version: u32,
    }

    #[repr(C)]
    struct InstanceCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        p_application_info: *const ApplicationInfo,
        enabled_layer_count: u32,
        pp_enabled_layer_names: *const *const c_char,
        enabled_extension_count: u32,
        pp_enabled_extension_names: *const *const c_char,
    }

    #[repr(C)]
    struct DeviceQueueCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        queue_family_index: u32,
        queue_count: u32,
        p_queue_priorities: *const f32,
    }

    #[repr(C)]
    struct DeviceCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        queue_create_info_count: u32,
        p_queue_create_infos: *const DeviceQueueCreateInfo,
        enabled_layer_count: u32,
        pp_enabled_layer_names: *const *const c_char,
        enabled_extension_count: u32,
        pp_enabled_extension_names: *const *const c_char,
        p_enabled_features: *const c_void,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct QueueFamilyProperties {
        queue_flags: u32,
        queue_count: u32,
        timestamp_valid_bits: u32,
        min_image_transfer_granularity: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MemoryType {
        property_flags: u32,
        heap_index: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MemoryHeap {
        size: u64,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MemoryProperties {
        memory_type_count: u32,
        memory_types: [MemoryType; 32],
        memory_heap_count: u32,
        memory_heaps: [MemoryHeap; 16],
    }

    #[repr(C)]
    struct BufferCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        size: u64,
        usage: u32,
        sharing_mode: u32,
        queue_family_index_count: u32,
        p_queue_family_indices: *const u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct MemoryRequirements {
        size: u64,
        alignment: u64,
        memory_type_bits: u32,
    }

    #[repr(C)]
    struct MemoryAllocateInfo {
        s_type: u32,
        p_next: *const c_void,
        allocation_size: u64,
        memory_type_index: u32,
    }

    #[repr(C)]
    struct ShaderModuleCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        code_size: usize,
        p_code: *const u32,
    }

    #[repr(C)]
    struct DescriptorSetLayoutBinding {
        binding: u32,
        descriptor_type: u32,
        descriptor_count: u32,
        stage_flags: u32,
        p_immutable_samplers: *const u64,
    }

    #[repr(C)]
    struct DescriptorSetLayoutCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        binding_count: u32,
        p_bindings: *const DescriptorSetLayoutBinding,
    }

    #[repr(C)]
    struct PipelineLayoutCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        set_layout_count: u32,
        p_set_layouts: *const VkDescriptorSetLayout,
        push_constant_range_count: u32,
        p_push_constant_ranges: *const c_void,
    }

    #[repr(C)]
    struct PipelineShaderStageCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        stage: u32,
        module: VkShaderModule,
        p_name: *const c_char,
        p_specialization_info: *const c_void,
    }

    #[repr(C)]
    struct ComputePipelineCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        stage: PipelineShaderStageCreateInfo,
        layout: VkPipelineLayout,
        base_pipeline_handle: VkPipeline,
        base_pipeline_index: i32,
    }

    #[repr(C)]
    struct DescriptorPoolSize {
        descriptor_type: u32,
        descriptor_count: u32,
    }

    #[repr(C)]
    struct DescriptorPoolCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        max_sets: u32,
        pool_size_count: u32,
        p_pool_sizes: *const DescriptorPoolSize,
    }

    #[repr(C)]
    struct DescriptorSetAllocateInfo {
        s_type: u32,
        p_next: *const c_void,
        descriptor_pool: VkDescriptorPool,
        descriptor_set_count: u32,
        p_set_layouts: *const VkDescriptorSetLayout,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct DescriptorBufferInfo {
        buffer: VkBuffer,
        offset: u64,
        range: u64,
    }

    #[repr(C)]
    struct WriteDescriptorSet {
        s_type: u32,
        p_next: *const c_void,
        dst_set: VkDescriptorSet,
        dst_binding: u32,
        dst_array_element: u32,
        descriptor_count: u32,
        descriptor_type: u32,
        p_image_info: *const c_void,
        p_buffer_info: *const DescriptorBufferInfo,
        p_texel_buffer_view: *const u64,
    }

    #[repr(C)]
    struct CommandPoolCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        queue_family_index: u32,
    }

    #[repr(C)]
    struct CommandBufferAllocateInfo {
        s_type: u32,
        p_next: *const c_void,
        command_pool: VkCommandPool,
        level: u32,
        command_buffer_count: u32,
    }

    #[repr(C)]
    struct CommandBufferBeginInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
        p_inheritance_info: *const c_void,
    }

    #[repr(C)]
    struct SubmitInfo {
        s_type: u32,
        p_next: *const c_void,
        wait_semaphore_count: u32,
        p_wait_semaphores: *const u64,
        p_wait_dst_stage_mask: *const u32,
        command_buffer_count: u32,
        p_command_buffers: *const VkCommandBuffer,
        signal_semaphore_count: u32,
        p_signal_semaphores: *const u64,
    }

    #[repr(C)]
    struct FenceCreateInfo {
        s_type: u32,
        p_next: *const c_void,
        flags: u32,
    }

    type CreateInstance = unsafe extern "C" fn(
        *const InstanceCreateInfo,
        *const c_void,
        *mut VkInstance,
    ) -> VkResult;
    type DestroyInstance = unsafe extern "C" fn(VkInstance, *const c_void);
    type EnumeratePhysicalDevices = unsafe extern "C" fn(
        VkInstance,
        *mut u32,
        *mut VkPhysicalDevice,
    ) -> VkResult;
    type GetQueueFamilyProperties = unsafe extern "C" fn(
        VkPhysicalDevice,
        *mut u32,
        *mut QueueFamilyProperties,
    );
    type GetPhysicalDeviceMemoryProperties = unsafe extern "C" fn(
        VkPhysicalDevice,
        *mut MemoryProperties,
    );
    type CreateDevice = unsafe extern "C" fn(
        VkPhysicalDevice,
        *const DeviceCreateInfo,
        *const c_void,
        *mut VkDevice,
    ) -> VkResult;
    type DestroyDevice = unsafe extern "C" fn(VkDevice, *const c_void);
    type GetDeviceQueue = unsafe extern "C" fn(VkDevice, u32, u32, *mut VkQueue);
    type DeviceWaitIdle = unsafe extern "C" fn(VkDevice) -> VkResult;
    type CreateBuffer = unsafe extern "C" fn(
        VkDevice,
        *const BufferCreateInfo,
        *const c_void,
        *mut VkBuffer,
    ) -> VkResult;
    type DestroyBuffer = unsafe extern "C" fn(VkDevice, VkBuffer, *const c_void);
    type GetBufferMemoryRequirements = unsafe extern "C" fn(
        VkDevice,
        VkBuffer,
        *mut MemoryRequirements,
    );
    type AllocateMemory = unsafe extern "C" fn(
        VkDevice,
        *const MemoryAllocateInfo,
        *const c_void,
        *mut VkDeviceMemory,
    ) -> VkResult;
    type FreeMemory = unsafe extern "C" fn(VkDevice, VkDeviceMemory, *const c_void);
    type BindBufferMemory = unsafe extern "C" fn(
        VkDevice,
        VkBuffer,
        VkDeviceMemory,
        u64,
    ) -> VkResult;
    type MapMemory = unsafe extern "C" fn(
        VkDevice,
        VkDeviceMemory,
        u64,
        u64,
        u32,
        *mut *mut c_void,
    ) -> VkResult;
    type UnmapMemory = unsafe extern "C" fn(VkDevice, VkDeviceMemory);
    type CreateShaderModule = unsafe extern "C" fn(
        VkDevice,
        *const ShaderModuleCreateInfo,
        *const c_void,
        *mut VkShaderModule,
    ) -> VkResult;
    type DestroyShaderModule = unsafe extern "C" fn(VkDevice, VkShaderModule, *const c_void);
    type CreateDescriptorSetLayout = unsafe extern "C" fn(
        VkDevice,
        *const DescriptorSetLayoutCreateInfo,
        *const c_void,
        *mut VkDescriptorSetLayout,
    ) -> VkResult;
    type DestroyDescriptorSetLayout = unsafe extern "C" fn(
        VkDevice,
        VkDescriptorSetLayout,
        *const c_void,
    );
    type CreatePipelineLayout = unsafe extern "C" fn(
        VkDevice,
        *const PipelineLayoutCreateInfo,
        *const c_void,
        *mut VkPipelineLayout,
    ) -> VkResult;
    type DestroyPipelineLayout = unsafe extern "C" fn(VkDevice, VkPipelineLayout, *const c_void);
    type CreateComputePipelines = unsafe extern "C" fn(
        VkDevice,
        u64,
        u32,
        *const ComputePipelineCreateInfo,
        *const c_void,
        *mut VkPipeline,
    ) -> VkResult;
    type DestroyPipeline = unsafe extern "C" fn(VkDevice, VkPipeline, *const c_void);
    type CreateDescriptorPool = unsafe extern "C" fn(
        VkDevice,
        *const DescriptorPoolCreateInfo,
        *const c_void,
        *mut VkDescriptorPool,
    ) -> VkResult;
    type DestroyDescriptorPool = unsafe extern "C" fn(VkDevice, VkDescriptorPool, *const c_void);
    type AllocateDescriptorSets = unsafe extern "C" fn(
        VkDevice,
        *const DescriptorSetAllocateInfo,
        *mut VkDescriptorSet,
    ) -> VkResult;
    type UpdateDescriptorSets = unsafe extern "C" fn(
        VkDevice,
        u32,
        *const WriteDescriptorSet,
        u32,
        *const c_void,
    );
    type CreateCommandPool = unsafe extern "C" fn(
        VkDevice,
        *const CommandPoolCreateInfo,
        *const c_void,
        *mut VkCommandPool,
    ) -> VkResult;
    type DestroyCommandPool = unsafe extern "C" fn(VkDevice, VkCommandPool, *const c_void);
    type AllocateCommandBuffers = unsafe extern "C" fn(
        VkDevice,
        *const CommandBufferAllocateInfo,
        *mut VkCommandBuffer,
    ) -> VkResult;
    type FreeCommandBuffers = unsafe extern "C" fn(
        VkDevice,
        VkCommandPool,
        u32,
        *const VkCommandBuffer,
    );
    type BeginCommandBuffer = unsafe extern "C" fn(
        VkCommandBuffer,
        *const CommandBufferBeginInfo,
    ) -> VkResult;
    type EndCommandBuffer = unsafe extern "C" fn(VkCommandBuffer) -> VkResult;
    type CmdBindPipeline = unsafe extern "C" fn(VkCommandBuffer, u32, VkPipeline);
    type CmdBindDescriptorSets = unsafe extern "C" fn(
        VkCommandBuffer,
        u32,
        VkPipelineLayout,
        u32,
        u32,
        *const VkDescriptorSet,
        u32,
        *const u32,
    );
    type CmdDispatch = unsafe extern "C" fn(VkCommandBuffer, u32, u32, u32);
    type CreateFence = unsafe extern "C" fn(
        VkDevice,
        *const FenceCreateInfo,
        *const c_void,
        *mut VkFence,
    ) -> VkResult;
    type DestroyFence = unsafe extern "C" fn(VkDevice, VkFence, *const c_void);
    type QueueSubmit = unsafe extern "C" fn(VkQueue, u32, *const SubmitInfo, VkFence) -> VkResult;
    type WaitForFences = unsafe extern "C" fn(
        VkDevice,
        u32,
        *const VkFence,
        u32,
        u64,
    ) -> VkResult;

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    struct Api {
        handle: *mut c_void,
        create_instance: CreateInstance,
        destroy_instance: DestroyInstance,
        enumerate_physical_devices: EnumeratePhysicalDevices,
        get_queue_family_properties: GetQueueFamilyProperties,
        get_physical_device_memory_properties: GetPhysicalDeviceMemoryProperties,
        create_device: CreateDevice,
        destroy_device: DestroyDevice,
        get_device_queue: GetDeviceQueue,
        device_wait_idle: DeviceWaitIdle,
        create_buffer: CreateBuffer,
        destroy_buffer: DestroyBuffer,
        get_buffer_memory_requirements: GetBufferMemoryRequirements,
        allocate_memory: AllocateMemory,
        free_memory: FreeMemory,
        bind_buffer_memory: BindBufferMemory,
        map_memory: MapMemory,
        unmap_memory: UnmapMemory,
        create_shader_module: CreateShaderModule,
        destroy_shader_module: DestroyShaderModule,
        create_descriptor_set_layout: CreateDescriptorSetLayout,
        destroy_descriptor_set_layout: DestroyDescriptorSetLayout,
        create_pipeline_layout: CreatePipelineLayout,
        destroy_pipeline_layout: DestroyPipelineLayout,
        create_compute_pipelines: CreateComputePipelines,
        destroy_pipeline: DestroyPipeline,
        create_descriptor_pool: CreateDescriptorPool,
        destroy_descriptor_pool: DestroyDescriptorPool,
        allocate_descriptor_sets: AllocateDescriptorSets,
        update_descriptor_sets: UpdateDescriptorSets,
        create_command_pool: CreateCommandPool,
        destroy_command_pool: DestroyCommandPool,
        allocate_command_buffers: AllocateCommandBuffers,
        free_command_buffers: FreeCommandBuffers,
        begin_command_buffer: BeginCommandBuffer,
        end_command_buffer: EndCommandBuffer,
        cmd_bind_pipeline: CmdBindPipeline,
        cmd_bind_descriptor_sets: CmdBindDescriptorSets,
        cmd_dispatch: CmdDispatch,
        create_fence: CreateFence,
        destroy_fence: DestroyFence,
        queue_submit: QueueSubmit,
        wait_for_fences: WaitForFences,
    }

    impl Api {
        fn load() -> Result<Arc<Self>, JetComputeError> {
            unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &str) -> Result<T, JetComputeError> {
                let name = CString::new(name).map_err(|_| {
                    JetComputeError::Device("Vulkan symbol name contains a NUL byte".to_string())
                })?;
                let pointer = dlsym(handle, name.as_ptr());
                if pointer.is_null() {
                    return Err(JetComputeError::Unsupported(format!(
                        "Vulkan loader is missing `{name:?}`"
                    )));
                }
                Ok(std::mem::transmute_copy(&pointer))
            }

            let handle = unsafe {
                let first = CString::new("libvulkan.so.1").unwrap();
                let second = CString::new("libvulkan.so").unwrap();
                let handle = dlopen(first.as_ptr(), RTLD_NOW | RTLD_LOCAL);
                if handle.is_null() {
                    dlopen(second.as_ptr(), RTLD_NOW | RTLD_LOCAL)
                } else {
                    handle
                }
            };
            if handle.is_null() {
                return Err(JetComputeError::Unsupported(
                    "Vulkan loader is unavailable on this target".to_string(),
                ));
            }
            macro_rules! load {
                ($name:literal, $ty:ty) => {
                    match unsafe { symbol::<$ty>(handle, $name) } {
                        Ok(value) => value,
                        Err(error) => return Err(error),
                    }
                };
            }
            Ok(Arc::new(Self {
                handle,
                create_instance: load!("vkCreateInstance", CreateInstance),
                destroy_instance: load!("vkDestroyInstance", DestroyInstance),
                enumerate_physical_devices: load!(
                    "vkEnumeratePhysicalDevices",
                    EnumeratePhysicalDevices
                ),
                get_queue_family_properties: load!(
                    "vkGetPhysicalDeviceQueueFamilyProperties",
                    GetQueueFamilyProperties
                ),
                get_physical_device_memory_properties: load!(
                    "vkGetPhysicalDeviceMemoryProperties",
                    GetPhysicalDeviceMemoryProperties
                ),
                create_device: load!("vkCreateDevice", CreateDevice),
                destroy_device: load!("vkDestroyDevice", DestroyDevice),
                get_device_queue: load!("vkGetDeviceQueue", GetDeviceQueue),
                device_wait_idle: load!("vkDeviceWaitIdle", DeviceWaitIdle),
                create_buffer: load!("vkCreateBuffer", CreateBuffer),
                destroy_buffer: load!("vkDestroyBuffer", DestroyBuffer),
                get_buffer_memory_requirements: load!(
                    "vkGetBufferMemoryRequirements",
                    GetBufferMemoryRequirements
                ),
                allocate_memory: load!("vkAllocateMemory", AllocateMemory),
                free_memory: load!("vkFreeMemory", FreeMemory),
                bind_buffer_memory: load!("vkBindBufferMemory", BindBufferMemory),
                map_memory: load!("vkMapMemory", MapMemory),
                unmap_memory: load!("vkUnmapMemory", UnmapMemory),
                create_shader_module: load!("vkCreateShaderModule", CreateShaderModule),
                destroy_shader_module: load!("vkDestroyShaderModule", DestroyShaderModule),
                create_descriptor_set_layout: load!(
                    "vkCreateDescriptorSetLayout",
                    CreateDescriptorSetLayout
                ),
                destroy_descriptor_set_layout: load!(
                    "vkDestroyDescriptorSetLayout",
                    DestroyDescriptorSetLayout
                ),
                create_pipeline_layout: load!("vkCreatePipelineLayout", CreatePipelineLayout),
                destroy_pipeline_layout: load!("vkDestroyPipelineLayout", DestroyPipelineLayout),
                create_compute_pipelines: load!(
                    "vkCreateComputePipelines",
                    CreateComputePipelines
                ),
                destroy_pipeline: load!("vkDestroyPipeline", DestroyPipeline),
                create_descriptor_pool: load!("vkCreateDescriptorPool", CreateDescriptorPool),
                destroy_descriptor_pool: load!("vkDestroyDescriptorPool", DestroyDescriptorPool),
                allocate_descriptor_sets: load!("vkAllocateDescriptorSets", AllocateDescriptorSets),
                update_descriptor_sets: load!("vkUpdateDescriptorSets", UpdateDescriptorSets),
                create_command_pool: load!("vkCreateCommandPool", CreateCommandPool),
                destroy_command_pool: load!("vkDestroyCommandPool", DestroyCommandPool),
                allocate_command_buffers: load!("vkAllocateCommandBuffers", AllocateCommandBuffers),
                free_command_buffers: load!("vkFreeCommandBuffers", FreeCommandBuffers),
                begin_command_buffer: load!("vkBeginCommandBuffer", BeginCommandBuffer),
                end_command_buffer: load!("vkEndCommandBuffer", EndCommandBuffer),
                cmd_bind_pipeline: load!("vkCmdBindPipeline", CmdBindPipeline),
                cmd_bind_descriptor_sets: load!("vkCmdBindDescriptorSets", CmdBindDescriptorSets),
                cmd_dispatch: load!("vkCmdDispatch", CmdDispatch),
                create_fence: load!("vkCreateFence", CreateFence),
                destroy_fence: load!("vkDestroyFence", DestroyFence),
                queue_submit: load!("vkQueueSubmit", QueueSubmit),
                wait_for_fences: load!("vkWaitForFences", WaitForFences),
            }))
        }
    }

    impl Drop for Api {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { dlclose(self.handle) };
            }
        }
    }

    struct InstanceGuard {
        api: Arc<Api>,
        instance: VkInstance,
    }

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            if !self.instance.is_null() {
                unsafe { (self.api.destroy_instance)(self.instance, ptr::null()) };
            }
        }
    }

    // The shader's std430 Params block is intentionally six scalar words.
    // Keep this layout identical to the GLSL block and to the Metal/CUDA
    // adapters so the operation family has one precision contract.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Params {
        count: u32,
        rows: u32,
        inner: u32,
        cols: u32,
        op: u32,
        scalar: f32,
    }

    fn check(result: VkResult, operation: &str) -> Result<(), JetComputeError> {
        if result == VK_SUCCESS {
            Ok(())
        } else {
            Err(JetComputeError::Device(format!(
                "Vulkan {operation} failed with VkResult {result}"
            )))
        }
    }

    struct Context {
        api: Arc<Api>,
        instance: VkInstance,
        device: VkDevice,
        queue: VkQueue,
        queue_family: u32,
        memory: MemoryProperties,
        command_pool: VkCommandPool,
        descriptor_layout: VkDescriptorSetLayout,
        pipeline_layout: VkPipelineLayout,
        pipeline: VkPipeline,
        descriptor_pool: VkDescriptorPool,
    }

    impl Context {
        fn new(api: Arc<Api>) -> Result<Self, JetComputeError> {
            let application_name = CString::new("jet-compute").unwrap();
            let engine_name = CString::new("jet").unwrap();
            let application_info = ApplicationInfo {
                s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
                p_next: ptr::null(),
                p_application_name: application_name.as_ptr(),
                application_version: 1,
                p_engine_name: engine_name.as_ptr(),
                engine_version: 1,
                api_version: 0,
            };
            let instance_info = InstanceCreateInfo {
                s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                p_application_info: &application_info,
                enabled_layer_count: 0,
                pp_enabled_layer_names: ptr::null(),
                enabled_extension_count: 0,
                pp_enabled_extension_names: ptr::null(),
            };
            let mut instance = ptr::null_mut();
            check(
                unsafe { (api.create_instance)(&instance_info, ptr::null(), &mut instance) },
                "instance creation",
            )?;

            let mut guard = InstanceGuard {
                api: api.clone(),
                instance,
            };
            Self::with_instance(api, &mut guard)
        }

        fn with_instance(
            api: Arc<Api>,
            guard: &mut InstanceGuard,
        ) -> Result<Self, JetComputeError> {
            let instance = guard.instance;
            let mut physical_count = 0;
            check(
                unsafe {
                    (api.enumerate_physical_devices)(instance, &mut physical_count, ptr::null_mut())
                },
                "physical-device enumeration",
            )?;
            if physical_count == 0 {
                return Err(JetComputeError::Device(
                    "Vulkan reported no physical devices".to_string(),
                ));
            }
            let mut physical_devices = vec![ptr::null_mut(); physical_count as usize];
            check(
                unsafe {
                    (api.enumerate_physical_devices)(
                        instance,
                        &mut physical_count,
                        physical_devices.as_mut_ptr(),
                    )
                },
                "physical-device enumeration",
            )?;
            let mut selected = None;
            for physical in physical_devices.into_iter().take(physical_count as usize) {
                let mut family_count = 0;
                unsafe {
                    (api.get_queue_family_properties)(physical, &mut family_count, ptr::null_mut())
                };
                if family_count == 0 {
                    continue;
                }
                let mut families = vec![
                    QueueFamilyProperties {
                        queue_flags: 0,
                        queue_count: 0,
                        timestamp_valid_bits: 0,
                        min_image_transfer_granularity: [0; 3],
                    };
                    family_count as usize
                ];
                unsafe {
                    (api.get_queue_family_properties)(physical, &mut family_count, families.as_mut_ptr())
                };
                let Some((family_index, _)) = families
                    .iter()
                    .enumerate()
                    .find(|(_, family)| {
                        family.queue_count != 0 && family.queue_flags & VK_QUEUE_COMPUTE_BIT != 0
                    })
                else {
                    continue;
                };
                let mut memory = MemoryProperties {
                    memory_type_count: 0,
                    memory_types: [MemoryType {
                        property_flags: 0,
                        heap_index: 0,
                    }; 32],
                    memory_heap_count: 0,
                    memory_heaps: [MemoryHeap { size: 0, flags: 0 }; 16],
                };
                unsafe {
                    (api.get_physical_device_memory_properties)(physical, &mut memory)
                };
                selected = Some((physical, family_index as u32, memory));
                break;
            }
            let Some((physical, queue_family, memory)) = selected else {
                return Err(JetComputeError::Device(
                    "Vulkan reported no physical device with a compute queue".to_string(),
                ));
            };
            let priority = 1.0_f32;
            let queue_info = DeviceQueueCreateInfo {
                s_type: VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                queue_family_index: queue_family,
                queue_count: 1,
                p_queue_priorities: &priority,
            };
            let device_info = DeviceCreateInfo {
                s_type: VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                queue_create_info_count: 1,
                p_queue_create_infos: &queue_info,
                enabled_layer_count: 0,
                pp_enabled_layer_names: ptr::null(),
                enabled_extension_count: 0,
                pp_enabled_extension_names: ptr::null(),
                p_enabled_features: ptr::null(),
            };
            let mut device = ptr::null_mut();
            check(
                unsafe { (api.create_device)(physical, &device_info, ptr::null(), &mut device) },
                "logical-device creation",
            )?;
            let mut queue = ptr::null_mut();
            unsafe { (api.get_device_queue)(device, queue_family, 0, &mut queue) };
            if queue.is_null() {
                unsafe { (api.destroy_device)(device, ptr::null()) };
                return Err(JetComputeError::Device(
                    "Vulkan returned a null compute queue".to_string(),
                ));
            }
            let mut context = Self {
                api,
                instance,
                device,
                queue,
                queue_family,
                memory,
                command_pool: 0,
                descriptor_layout: 0,
                pipeline_layout: 0,
                pipeline: 0,
                descriptor_pool: 0,
            };
            guard.instance = ptr::null_mut();
            let command_pool_info = CommandPoolCreateInfo {
                s_type: VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                queue_family_index: context.queue_family,
            };
            check(
                unsafe {
                    (context.api.create_command_pool)(
                        context.device,
                        &command_pool_info,
                        ptr::null(),
                        &mut context.command_pool,
                    )
                },
                "command-pool creation",
            )?;
            context.create_pipeline()?;
            let pool_sizes = [
                DescriptorPoolSize {
                    descriptor_type: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                    descriptor_count: 6,
                },
                DescriptorPoolSize {
                    descriptor_type: VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                    descriptor_count: 1,
                },
            ];
            let pool_info = DescriptorPoolCreateInfo {
                s_type: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                max_sets: 1,
                pool_size_count: pool_sizes.len() as u32,
                p_pool_sizes: pool_sizes.as_ptr(),
            };
            check(
                unsafe {
                    (context.api.create_descriptor_pool)(
                        context.device,
                        &pool_info,
                        ptr::null(),
                        &mut context.descriptor_pool,
                    )
                },
                "descriptor-pool creation",
            )?;
            Ok(context)
        }

        fn create_pipeline(&mut self) -> Result<(), JetComputeError> {
            let bindings: [DescriptorSetLayoutBinding; 7] = std::array::from_fn(|binding| DescriptorSetLayoutBinding {
                binding: binding as u32,
                descriptor_type: if binding == 6 {
                    VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER
                } else {
                    VK_DESCRIPTOR_TYPE_STORAGE_BUFFER
                },
                descriptor_count: 1,
                stage_flags: VK_SHADER_STAGE_COMPUTE_BIT,
                p_immutable_samplers: ptr::null(),
            });
            let layout_info = DescriptorSetLayoutCreateInfo {
                s_type: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                binding_count: bindings.len() as u32,
                p_bindings: bindings.as_ptr(),
            };
            check(
                unsafe {
                    (self.api.create_descriptor_set_layout)(
                        self.device,
                        &layout_info,
                        ptr::null(),
                        &mut self.descriptor_layout,
                    )
                },
                "descriptor-set-layout creation",
            )?;
            let layouts = [self.descriptor_layout];
            let pipeline_layout_info = PipelineLayoutCreateInfo {
                s_type: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                set_layout_count: 1,
                p_set_layouts: layouts.as_ptr(),
                push_constant_range_count: 0,
                p_push_constant_ranges: ptr::null(),
            };
            check(
                unsafe {
                    (self.api.create_pipeline_layout)(
                        self.device,
                        &pipeline_layout_info,
                        ptr::null(),
                        &mut self.pipeline_layout,
                    )
                },
                "pipeline-layout creation",
            )?;
            let shader_info = ShaderModuleCreateInfo {
                s_type: VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                code_size: VULKAN_SHADER.len() * std::mem::size_of::<u32>(),
                p_code: VULKAN_SHADER.as_ptr(),
            };
            let mut shader = 0;
            check(
                unsafe {
                    (self.api.create_shader_module)(
                        self.device,
                        &shader_info,
                        ptr::null(),
                        &mut shader,
                    )
                },
                "SPIR-V shader-module creation",
            )?;
            let entry = b"main\0";
            let stage = PipelineShaderStageCreateInfo {
                s_type: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                stage: VK_SHADER_STAGE_COMPUTE_BIT,
                module: shader,
                p_name: entry.as_ptr().cast(),
                p_specialization_info: ptr::null(),
            };
            let pipeline_info = ComputePipelineCreateInfo {
                s_type: VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                stage,
                layout: self.pipeline_layout,
                base_pipeline_handle: 0,
                base_pipeline_index: -1,
            };
            let result = check(
                unsafe {
                    (self.api.create_compute_pipelines)(
                        self.device,
                        0,
                        1,
                        &pipeline_info,
                        ptr::null(),
                        &mut self.pipeline,
                    )
                },
                "compute-pipeline creation",
            );
            unsafe { (self.api.destroy_shader_module)(self.device, shader, ptr::null()) };
            result
        }

        fn find_memory_type(&self, type_bits: u32) -> Result<u32, JetComputeError> {
            (0..self.memory.memory_type_count as usize)
                .find(|index| {
                    type_bits & (1 << index) != 0
                        && self.memory.memory_types[*index].property_flags
                            & (VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT
                                | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)
                            == (VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT
                                | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT)
                })
                .map(|index| index as u32)
                .ok_or_else(|| {
                    JetComputeError::Device(
                        "Vulkan has no host-visible coherent memory type for compute buffers"
                            .to_string(),
                    )
                })
        }
    }

    impl Drop for Context {
        fn drop(&mut self) {
            unsafe {
                if !self.device.is_null() {
                    let _ = (self.api.device_wait_idle)(self.device);
                }
                if self.descriptor_pool != 0 {
                    (self.api.destroy_descriptor_pool)(self.device, self.descriptor_pool, ptr::null());
                }
                if self.pipeline != 0 {
                    (self.api.destroy_pipeline)(self.device, self.pipeline, ptr::null());
                }
                if self.pipeline_layout != 0 {
                    (self.api.destroy_pipeline_layout)(self.device, self.pipeline_layout, ptr::null());
                }
                if self.descriptor_layout != 0 {
                    (self.api.destroy_descriptor_set_layout)(
                        self.device,
                        self.descriptor_layout,
                        ptr::null(),
                    );
                }
                if self.command_pool != 0 {
                    (self.api.destroy_command_pool)(self.device, self.command_pool, ptr::null());
                }
                if !self.device.is_null() {
                    (self.api.destroy_device)(self.device, ptr::null());
                }
                if !self.instance.is_null() {
                    (self.api.destroy_instance)(self.instance, ptr::null());
                }
            }
        }
    }

    struct Buffer {
        api: Arc<Api>,
        device: VkDevice,
        buffer: VkBuffer,
        memory: VkDeviceMemory,
        bytes: u64,
    }

    impl Buffer {
        fn from_bytes(context: &Context, bytes: &[u8], usage: u32) -> Result<Self, JetComputeError> {
            let size = bytes.len().max(4) as u64;
            let info = BufferCreateInfo {
                s_type: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                size,
                usage,
                sharing_mode: 0,
                queue_family_index_count: 0,
                p_queue_family_indices: ptr::null(),
            };
            let mut handle = 0;
            check(
                unsafe {
                    (context.api.create_buffer)(
                        context.device,
                        &info,
                        ptr::null(),
                        &mut handle,
                    )
                },
                "buffer creation",
            )?;
            let mut requirements = MemoryRequirements::default();
            unsafe {
                (context.api.get_buffer_memory_requirements)(
                    context.device,
                    handle,
                    &mut requirements,
                )
            };
            let memory_type = match context.find_memory_type(requirements.memory_type_bits) {
                Ok(value) => value,
                Err(error) => {
                    unsafe { (context.api.destroy_buffer)(context.device, handle, ptr::null()) };
                    return Err(error);
                }
            };
            let allocation_info = MemoryAllocateInfo {
                s_type: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                p_next: ptr::null(),
                allocation_size: requirements.size.max(size),
                memory_type_index: memory_type,
            };
            let mut memory = 0;
            if let Err(error) = check(
                unsafe {
                    (context.api.allocate_memory)(
                        context.device,
                        &allocation_info,
                        ptr::null(),
                        &mut memory,
                    )
                },
                "buffer memory allocation",
            ) {
                unsafe { (context.api.destroy_buffer)(context.device, handle, ptr::null()) };
                return Err(error);
            }
            let output = Self {
                api: context.api.clone(),
                device: context.device,
                buffer: handle,
                memory,
                bytes: size,
            };
            if let Err(error) = check(
                unsafe { (output.api.bind_buffer_memory)(output.device, output.buffer, memory, 0) },
                "buffer memory binding",
            ) {
                return Err(error);
            }
            let mut mapped = ptr::null_mut();
            if let Err(error) = check(
                unsafe {
                    (output.api.map_memory)(
                        output.device,
                        output.memory,
                        0,
                        output.bytes,
                        0,
                        &mut mapped,
                    )
                },
                "buffer mapping",
            ) {
                return Err(error);
            }
            if !bytes.is_empty() {
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.cast(), bytes.len());
                }
            }
            unsafe { (output.api.unmap_memory)(output.device, output.memory) };
            Ok(output)
        }

        fn f32(context: &Context, values: &[f32], usage: u32) -> Result<Self, JetComputeError> {
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    values.as_ptr().cast::<u8>(),
                    values.len() * std::mem::size_of::<f32>(),
                )
            };
            Self::from_bytes(context, bytes, usage)
        }

        fn descriptor(&self) -> DescriptorBufferInfo {
            DescriptorBufferInfo {
                buffer: self.buffer,
                offset: 0,
                range: self.bytes,
            }
        }

        fn read_f32(&self, length: usize) -> Result<Vec<f32>, JetComputeError> {
            let bytes = length
                .checked_mul(std::mem::size_of::<f32>())
                .ok_or_else(|| JetComputeError::InvalidShape("Vulkan output size overflow".to_string()))?;
            if bytes as u64 > self.bytes {
                return Err(JetComputeError::InvalidShape(
                    "Vulkan output buffer is smaller than the requested result".to_string(),
                ));
            }
            let mut mapped = ptr::null_mut();
            check(
                unsafe {
                    (self.api.map_memory)(self.device, self.memory, 0, self.bytes, 0, &mut mapped)
                },
                "output mapping",
            )?;
            let mut output = vec![0.0_f32; length];
            if bytes != 0 {
                unsafe {
                    ptr::copy_nonoverlapping(
                        mapped.cast::<u8>(),
                        output.as_mut_ptr().cast::<u8>(),
                        bytes,
                    );
                }
            }
            unsafe { (self.api.unmap_memory)(self.device, self.memory) };
            Ok(output)
        }
    }

    impl Drop for Buffer {
        fn drop(&mut self) {
            unsafe {
                if self.buffer != 0 {
                    (self.api.destroy_buffer)(self.device, self.buffer, ptr::null());
                }
                if self.memory != 0 {
                    (self.api.free_memory)(self.device, self.memory, ptr::null());
                }
            }
        }
    }

    fn run(
        op: u32,
        inputs: &[&[f32]],
        output_len: usize,
        params: Params,
    ) -> Result<Vec<f32>, JetComputeError> {
        if output_len == 0 {
            return Ok(Vec::new());
        }
        if inputs.len() > 6 {
            return Err(JetComputeError::InvalidShape(
                "Vulkan kernels accept at most six input buffers".to_string(),
            ));
        }
        let api = Api::load()?;
        let context = Context::new(api)?;
        let dummy = [0.0_f32];
        let mut input_buffers = Vec::with_capacity(6);
        for index in 0..6 {
            let values = inputs.get(index).copied().unwrap_or(&dummy);
            input_buffers.push(Buffer::f32(
                &context,
                values,
                VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
            )?);
        }
        let output = Buffer::f32(
            &context,
            &vec![0.0_f32; output_len],
            VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
        )?;
        let params_bytes = unsafe {
            std::slice::from_raw_parts(
                (&params as *const Params).cast::<u8>(),
                std::mem::size_of::<Params>(),
            )
        };
        let params_buffer = Buffer::from_bytes(
            &context,
            params_bytes,
            VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT,
        )?;
        let infos = [
            input_buffers[0].descriptor(),
            input_buffers[1].descriptor(),
            output.descriptor(),
            input_buffers[3].descriptor(),
            input_buffers[4].descriptor(),
            input_buffers[5].descriptor(),
            params_buffer.descriptor(),
        ];
        let set_layouts = [context.descriptor_layout];
        let allocation_info = DescriptorSetAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
            p_next: ptr::null(),
            descriptor_pool: context.descriptor_pool,
            descriptor_set_count: 1,
            p_set_layouts: set_layouts.as_ptr(),
        };
        let mut descriptor_set = 0;
        check(
            unsafe {
                (context.api.allocate_descriptor_sets)(
                    context.device,
                    &allocation_info,
                    &mut descriptor_set,
                )
            },
            "descriptor-set allocation",
        )?;
        let writes = infos
            .iter()
            .enumerate()
            .map(|(binding, info)| WriteDescriptorSet {
                s_type: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                p_next: ptr::null(),
                dst_set: descriptor_set,
                dst_binding: binding as u32,
                dst_array_element: 0,
                descriptor_count: 1,
                descriptor_type: if binding == 6 {
                    VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER
                } else {
                    VK_DESCRIPTOR_TYPE_STORAGE_BUFFER
                },
                p_image_info: ptr::null(),
                p_buffer_info: info,
                p_texel_buffer_view: ptr::null(),
            })
            .collect::<Vec<_>>();
        unsafe {
            (context.api.update_descriptor_sets)(
                context.device,
                writes.len() as u32,
                writes.as_ptr(),
                0,
                ptr::null(),
            )
        };
        let pool_info = CommandBufferAllocateInfo {
            s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            p_next: ptr::null(),
            command_pool: context.command_pool,
            level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
            command_buffer_count: 1,
        };
        let mut command_buffer = ptr::null_mut();
        check(
            unsafe {
                (context.api.allocate_command_buffers)(
                    context.device,
                    &pool_info,
                    &mut command_buffer,
                )
            },
            "command-buffer allocation",
        )?;
        let begin_info = CommandBufferBeginInfo {
            s_type: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: ptr::null(),
            flags: 0,
            p_inheritance_info: ptr::null(),
        };
        check(
            unsafe { (context.api.begin_command_buffer)(command_buffer, &begin_info) },
            "command-buffer begin",
        )?;
        let descriptor_sets = [descriptor_set];
        unsafe {
            (context.api.cmd_bind_pipeline)(
                command_buffer,
                VK_PIPELINE_BIND_POINT_COMPUTE,
                context.pipeline,
            );
            (context.api.cmd_bind_descriptor_sets)(
                command_buffer,
                VK_PIPELINE_BIND_POINT_COMPUTE,
                context.pipeline_layout,
                0,
                1,
                descriptor_sets.as_ptr(),
                0,
                ptr::null(),
            );
            let work_items = if matches!(op, 12 | 13 | 16) { 1 } else { output_len };
            let groups = work_items
                .saturating_add(63)
                .checked_div(64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value != 0)
                .ok_or_else(|| {
                    JetComputeError::InvalidShape("Vulkan dispatch group count is too large".to_string())
                })?;
            (context.api.cmd_dispatch)(command_buffer, groups, 1, 1);
        }
        check(
            unsafe { (context.api.end_command_buffer)(command_buffer) },
            "command-buffer end",
        )?;
        let command_buffers = [command_buffer];
        let submit = SubmitInfo {
            s_type: VK_STRUCTURE_TYPE_SUBMIT_INFO,
            p_next: ptr::null(),
            wait_semaphore_count: 0,
            p_wait_semaphores: ptr::null(),
            p_wait_dst_stage_mask: ptr::null(),
            command_buffer_count: 1,
            p_command_buffers: command_buffers.as_ptr(),
            signal_semaphore_count: 0,
            p_signal_semaphores: ptr::null(),
        };
        let fence_info = FenceCreateInfo {
            s_type: VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
        };
        let mut fence = 0;
        check(
            unsafe { (context.api.create_fence)(context.device, &fence_info, ptr::null(), &mut fence) },
            "fence creation",
        )?;
        let submit_result = check(
            unsafe { (context.api.queue_submit)(context.queue, 1, &submit, fence) },
            "queue submission",
        );
        if let Err(error) = submit_result {
            unsafe { (context.api.destroy_fence)(context.device, fence, ptr::null()) };
            return Err(error);
        }
        let wait_result = check(
            unsafe { (context.api.wait_for_fences)(context.device, 1, &fence, 1, u64::MAX) },
            "queue completion",
        );
        unsafe { (context.api.destroy_fence)(context.device, fence, ptr::null()) };
        wait_result?;
        output.read_f32(output_len)
    }

    pub fn available() -> bool {
        Api::load()
            .and_then(Context::new)
            .is_ok()
    }

    pub fn copy(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        run(
            17,
            &[values],
            values.len(),
            Params {
                count: values.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan copy count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 17,
                scalar: 0.0,
            },
        )
    }

    pub fn binary(op: u32, left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len() {
            return Err(JetComputeError::InvalidShape(
                "Vulkan binary inputs have different lengths".to_string(),
            ));
        }
        Ok(run(
            op,
            &[left, right],
            left.len(),
            Params {
                count: left.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan binary count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op,
                scalar: 0.0,
            },
        )?)
    }

    pub fn unary(op: u32, values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        Ok(run(
            6 + op,
            &[values],
            values.len(),
            Params {
                count: values.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan unary count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 6 + op,
                scalar: 0.0,
            },
        )?)
    }

    pub fn matmul(
        left: &[f32],
        right: &[f32],
        rows: usize,
        inner: usize,
        cols: usize,
    ) -> Result<Vec<f32>, JetComputeError> {
        let count = rows.checked_mul(cols).ok_or_else(|| {
            JetComputeError::InvalidShape("Vulkan matmul output size overflow".to_string())
        })?;
        Ok(run(
            11,
            &[left, right],
            count,
            Params {
                count: count.try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan matmul output is too large".to_string()))?,
                rows: rows.try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan matmul rows are too large".to_string()))?,
                inner: inner.try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan matmul inner dimension is too large".to_string()))?,
                cols: cols.try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan matmul columns are too large".to_string()))?,
                op: 11,
                scalar: 0.0,
            },
        )?)
    }

    pub fn sum(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        Ok(run(
            12,
            &[values],
            1,
            Params {
                count: values.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan sum count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 12,
                scalar: 0.0,
            },
        )?)
    }

    pub fn mse(left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len() || left.is_empty() {
            return Err(JetComputeError::InvalidShape(
                "Vulkan MSE inputs must have the same non-empty length".to_string(),
            ));
        }
        Ok(run(
            13,
            &[left, right],
            1,
            Params {
                count: left.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan MSE count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 13,
                scalar: 0.0,
            },
        )?)
    }

    pub fn mse_grad(
        left: &[f32],
        right: &[f32],
        cot: &[f32],
        positive: bool,
    ) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len() || left.is_empty() || cot.len() != 1 {
            return Err(JetComputeError::InvalidShape(
                "Vulkan MSE gradient inputs have incompatible lengths".to_string(),
            ));
        }
        Ok(run(
            14,
            &[left, right, cot],
            left.len(),
            Params {
                count: left.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan MSE gradient count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 14,
                scalar: if positive { 0.0 } else { 1.0 },
            },
        )?)
    }

    pub fn mse_jvp(
        left: &[f32],
        right: &[f32],
        left_tangent: &[f32],
        right_tangent: &[f32],
    ) -> Result<Vec<f32>, JetComputeError> {
        if left.len() != right.len()
            || left.len() != left_tangent.len()
            || left.len() != right_tangent.len()
            || left.is_empty()
        {
            return Err(JetComputeError::InvalidShape(
                "Vulkan MSE JVP inputs have incompatible lengths".to_string(),
            ));
        }
        Ok(run(
            16,
            &[left, right, left_tangent, right_tangent],
            1,
            Params {
                count: left.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan MSE JVP count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 16,
                scalar: 0.0,
            },
        )?)
    }

    pub fn sgd(
        parameter: &[f32],
        gradient: &[f32],
        learning_rate: f32,
    ) -> Result<Vec<f32>, JetComputeError> {
        if parameter.len() != gradient.len() {
            return Err(JetComputeError::InvalidShape(
                "Vulkan SGD inputs have different lengths".to_string(),
            ));
        }
        Ok(run(
            18,
            &[parameter, gradient],
            parameter.len(),
            Params {
                count: parameter.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan SGD count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 18,
                scalar: learning_rate,
            },
        )?)
    }

    pub fn scale(values: &[f32], scalar: f32) -> Result<Vec<f32>, JetComputeError> {
        Ok(run(
            15,
            &[values],
            values.len(),
            Params {
                count: values.len().try_into().map_err(|_| JetComputeError::InvalidShape("Vulkan scale count is too large".to_string()))?,
                rows: 0,
                inner: 0,
                cols: 0,
                op: 15,
                scalar,
            },
        )?)
    }
}

#[cfg(not(target_os = "linux"))]
mod jet_compute_vulkan {
    use super::JetComputeError;

    fn unavailable<T>() -> Result<T, JetComputeError> {
        Err(JetComputeError::Unsupported(
            "Vulkan backend is unavailable on this target".to_string(),
        ))
    }

    pub fn available() -> bool { false }
    pub fn copy(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn binary(_: u32, _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn unary(_: u32, _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn matmul(_: &[f32], _: &[f32], _: usize, _: usize, _: usize) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sum(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse(_: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_grad(_: &[f32], _: &[f32], _: &[f32], _: bool) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_jvp(_: &[f32], _: &[f32], _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sgd(_: &[f32], _: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn scale(_: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
}

// WebGPU is a browser-owned provider. Native hosts do not expose a standard
// WebGPU ABI, so the production seam reports that fact instead of pretending
// that a CPU result came from a GPU. A browser host can replace this adapter
// at the platform boundary without changing the Core compute contract.
mod jet_compute_webgpu {
    use super::JetComputeError;

    fn unavailable<T>() -> Result<T, JetComputeError> {
        Err(JetComputeError::Unsupported(
            "WebGPU backend requires a browser WebGPU host".to_string(),
        ))
    }

    pub fn available() -> bool { false }
    pub fn copy(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn binary(_: u32, _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn unary(_: u32, _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn matmul(_: &[f32], _: &[f32], _: usize, _: usize, _: usize) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sum(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse(_: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_grad(_: &[f32], _: &[f32], _: &[f32], _: bool) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_jvp(_: &[f32], _: &[f32], _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sgd(_: &[f32], _: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn scale(_: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
}

fn jet_compute_registered_abilities(profile: &str) -> Option<&'static [&'static str]> {
    match profile {
        CPU_ORACLE_F64_PROFILE => Some(CPU_ORACLE_F64_CAPABILITIES),
        CPU_ORACLE_F32_PROFILE => Some(CPU_ORACLE_F32_CAPABILITIES),
        _ => None,
    }
}

fn jet_compute_registered_backend_abilities(
    backend: &str,
    profile: &str,
) -> Option<&'static [&'static str]> {
    match backend {
        CPU_ORACLE_BACKEND => jet_compute_registered_abilities(profile),
        METAL_BACKEND if profile == CPU_ORACLE_F32_PROFILE => Some(METAL_F32_CAPABILITIES),
        CUDA_BACKEND if profile == CPU_ORACLE_F32_PROFILE => Some(CUDA_F32_CAPABILITIES),
        VULKAN_BACKEND if profile == CPU_ORACLE_F32_PROFILE => Some(VULKAN_F32_CAPABILITIES),
        WEBGPU_BACKEND if profile == CPU_ORACLE_F32_PROFILE => Some(WEBGPU_F32_CAPABILITIES),
        _ => None,
    }
}

fn jet_compute_abilities_match(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| actual == expected)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputePlacementReceipt {
    requested: JetComputeDevice,
    selected: JetComputeDevice,
    backend: String,
    version: String,
    profile: String,
    cache: String,
    abilities: Vec<String>,
    reason: String,
}

#[derive(Clone)]
struct JetComputeTrace {
    // Traces are observations of a live per-call tape.  The transform state
    // owns the strong tape handle; graph values keep only a weak back-link so
    // nested tapes cannot retain one another through recorded values.
    tape: std::sync::Weak<std::sync::Mutex<JetComputeTape>>,
    node: usize,
    parent: Option<Box<JetComputeTrace>>,
}

impl std::fmt::Debug for JetComputeTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetComputeTrace")
            .field("node", &self.node)
            .field("parent", &self.parent)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct JetTensor {
    shape: Vec<i64>,
    strides: Vec<i64>,
    data: std::sync::Arc<Vec<f64>>,
    device: JetComputeDevice,
    last_placement: JetComputePlacementReceipt,
    last_transfer: Option<JetComputeTransferReceipt>,
    trace: Option<JetComputeTrace>,
}

/// A compiler-internal mutable Tensor window. Unlike an ordinary list view it
/// retains the owner and original range so every element write can use the
/// complete shared window policy.
pub struct JetComputeViewMut<'a> {
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
}

impl PartialEq for JetTensor {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.strides == other.strides
            && self.data == other.data
            && self.device == other.device
            && self.last_placement == other.last_placement
            && self.last_transfer == other.last_transfer
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetComputeError {
    InvalidShape(String),
    RankMismatch(String),
    OutOfBounds(String),
    Device(String),
    Unsupported(String),
    Arithmetic(String),
    Serialization(String),
}

impl JetShow for JetComputeError {
    fn jet_show(&self) -> String {
        match self {
            JetComputeError::InvalidShape(m)
            | JetComputeError::RankMismatch(m)
            | JetComputeError::OutOfBounds(m)
            | JetComputeError::Device(m)
            | JetComputeError::Unsupported(m)
            | JetComputeError::Arithmetic(m)
            | JetComputeError::Serialization(m) => m.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum JetComputeTapeRule {
    Add,
    Sub,
    Mul,
    Div,
    Maximum,
    Minimum,
    Matmul,
    MatmulF32Tile,
    MseLoss,
    SgdStep {
        learning_rate: f64,
    },
    Unary(String),
    Reshape {
        source_shape: Vec<i64>,
    },
    Broadcast {
        source_shape: Vec<i64>,
    },
    ReduceToShape {
        source_shape: Vec<i64>,
    },
    Transpose,
    SumAxis {
        axis: usize,
        source_shape: Vec<i64>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct JetComputeTapeNode {
    parents: Vec<Option<usize>>,
    rule: Option<JetComputeTapeRule>,
    values: Vec<JetTensor>,
    output: JetTensor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetComputeTape {
    nodes: Vec<JetComputeTapeNode>,
    inputs: Vec<JetTensor>,
}

#[derive(Clone, Debug)]
pub struct JetComputeVjpState {
    value: JetTensor,
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    output_node: Option<usize>,
}

enum JetComputeTransformResult {
    Gradient(Vec<JetTensor>),
    ValueAndGradient {
        value: JetTensor,
        gradients: Vec<JetTensor>,
    },
    Vjp {
        value: JetTensor,
        state: JetComputeVjpState,
    },
    Jvp {
        value: JetTensor,
        tangent: JetTensor,
    },
}

/// One transform meaning crosses every host boundary.  The numeric form is
/// also the resident host ABI; hosts marshal it but never select policy from
/// a method string.
#[repr(i64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetComputeTransformKind {
    Gradient = 0,
    ValueAndGradient = 1,
    Vjp = 2,
    Jvp = 3,
}

impl JetComputeTransformKind {
    pub fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Gradient),
            1 => Some(Self::ValueAndGradient),
            2 => Some(Self::Vjp),
            3 => Some(Self::Jvp),
            _ => None,
        }
    }

    pub fn is_jvp(self) -> bool {
        matches!(self, Self::Jvp)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gradient => "gradient",
            Self::ValueAndGradient => "value_and_gradient",
            Self::Vjp => "vjp",
            Self::Jvp => "jvp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetComputeResultShape {
    Tensor,
    TensorTuple(usize),
}

pub enum JetComputeBaseResult {
    Tensor(JetTensor),
    TensorTuple(Vec<JetTensor>),
}

pub struct JetComputeBase {
    arity: usize,
    invoke: std::rc::Rc<dyn Fn(&[JetTensor]) -> Result<JetComputeBaseResult, JetComputeError>>,
}

impl Clone for JetComputeBase {
    fn clone(&self) -> Self {
        Self {
            arity: self.arity,
            invoke: self.invoke.clone(),
        }
    }
}

impl JetComputeBase {
    pub fn new<F>(arity: usize, invoke: F) -> Self
    where
        F: Fn(&[JetTensor]) -> Result<JetComputeBaseResult, JetComputeError> + 'static,
    {
        Self {
            arity,
            invoke: std::rc::Rc::new(invoke),
        }
    }

    fn call(&self, inputs: &[JetTensor]) -> Result<JetComputeBaseResult, JetComputeError> {
        if inputs.len() != self.arity {
            return Err(JetComputeError::Unsupported(
                "autodiff callable received the wrong number of Tensor arguments".to_string(),
            ));
        }
        (self.invoke)(inputs)
    }
}

pub struct JetComputeInputPack {
    pub primals: Vec<JetTensor>,
    pub tangents: Vec<JetTensor>,
    flat: bool,
}

impl JetComputeInputPack {
    pub fn new(primals: Vec<JetTensor>, tangents: Vec<JetTensor>) -> Self {
        Self {
            primals,
            tangents,
            flat: false,
        }
    }

    /// Resident hosts receive one flat list from a typed function-value ABI.
    /// The plan owns the JVP split, so the host does not inspect transform
    /// policy while marshalling that list.
    pub fn from_flat(values: Vec<JetTensor>) -> Self {
        Self {
            primals: values,
            tangents: Vec::new(),
            flat: true,
        }
    }
}

pub enum JetComputeCurriedResult {
    Gradient(Vec<Vec<JetTensor>>),
    ValueAndGradient {
        value: JetTensor,
        gradients: Vec<Vec<JetTensor>>,
    },
    Vjp {
        value: JetTensor,
        pull: i64,
        grads: i64,
    },
    Jvp {
        value: JetTensor,
        tangent: JetTensor,
    },
}

#[derive(Clone, Copy)]
enum JetComputeCurriedContinuation {
    Pull,
    Grads,
}

#[derive(Clone)]
enum JetComputeCurriedEntry {
    Plan {
        base: JetComputeBase,
        kind: JetComputeTransformKind,
        targets: Vec<i64>,
        result_shape: JetComputeResultShape,
    },
    Continuation {
        state: JetComputeVjpState,
        targets: Vec<i64>,
        kind: JetComputeCurriedContinuation,
    },
}

struct JetComputeCurriedSlot {
    refs: usize,
    entry: JetComputeCurriedEntry,
}

thread_local! {
    static JET_COMPUTE_CURRIED_HANDLES:
        std::cell::RefCell<Vec<Option<JetComputeCurriedSlot>>> = const {
            std::cell::RefCell::new(Vec::new())
        };
}

fn jet_compute_curried_insert(entry: JetComputeCurriedEntry) -> i64 {
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let index = handles.len();
        handles.push(Some(JetComputeCurriedSlot { refs: 1, entry }));
        (index as i64).saturating_add(1)
    })
}

fn jet_compute_curried_entry(handle: i64) -> Option<JetComputeCurriedEntry> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(index)
            .and_then(Option::as_ref)
            .map(|slot| slot.entry.clone())
    })
}

pub fn jet_compute_curried_new(
    base: JetComputeBase,
    kind: JetComputeTransformKind,
    targets: &[i64],
    result_shape: JetComputeResultShape,
) -> i64 {
    jet_compute_curried_insert(JetComputeCurriedEntry::Plan {
        base,
        kind,
        targets: targets.to_vec(),
        result_shape,
    })
}

pub fn jet_compute_curried_clone(handle: i64) -> i64 {
    let Some(index) = usize::try_from(handle).ok().and_then(|value| value.checked_sub(1)) else {
        return 0;
    };
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let Some(slot) = handles.get_mut(index).and_then(Option::as_mut) else {
            return 0;
        };
        slot.refs = slot.refs.saturating_add(1);
        handle
    })
}

pub fn jet_compute_curried_drop(handle: i64) {
    let Some(index) = usize::try_from(handle).ok().and_then(|value| value.checked_sub(1)) else {
        return;
    };
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let remove = match handles.get_mut(index).and_then(Option::as_mut) {
            Some(slot) if slot.refs <= 1 => true,
            Some(slot) => {
                slot.refs -= 1;
                false
            }
            None => return,
        };
        if remove {
            handles[index] = None;
        }
    });
}

#[repr(transparent)]
pub struct JetComputeHandle(i64);

impl JetComputeHandle {
    pub fn new(raw: i64) -> Self {
        Self(raw)
    }

    pub fn raw(&self) -> i64 {
        self.0
    }
}

impl Clone for JetComputeHandle {
    fn clone(&self) -> Self {
        Self(jet_compute_curried_clone(self.0))
    }
}

impl Drop for JetComputeHandle {
    fn drop(&mut self) {
        jet_compute_curried_drop(self.0);
    }
}

fn jet_compute_curried_result_shape(
    result: JetComputeBaseResult,
    shape: JetComputeResultShape,
) -> Result<Vec<JetTensor>, JetComputeError> {
    let values = match (shape, result) {
        (JetComputeResultShape::Tensor, JetComputeBaseResult::Tensor(value)) => {
            vec![value]
        }
        (JetComputeResultShape::TensorTuple(expected), JetComputeBaseResult::TensorTuple(values)) => {
            if values.len() != expected {
                return Err(JetComputeError::Unsupported(format!(
                    "autodiff base returned {} tensors; expected {expected}",
                    values.len()
                )));
            }
            values
        }
        (JetComputeResultShape::Tensor, JetComputeBaseResult::TensorTuple(_))
        | (JetComputeResultShape::TensorTuple(_), JetComputeBaseResult::Tensor(_)) => {
            return Err(JetComputeError::Unsupported(
                "autodiff base returned the wrong result shape".to_string(),
            ));
        }
    };
    for value in &values {
        jet_compute_validate_tensor(value)?;
    }
    Ok(values)
}

fn jet_compute_curried_gradient_result(
    states: &[JetComputeVjpState],
    targets: &[i64],
) -> Result<Vec<Vec<JetTensor>>, JetComputeError> {
    let gradients = states
        .iter()
        .map(|state| {
            let seed = jet_compute_gradient_seed(state)?;
            jet_compute_vjp_pull(state, &seed, targets)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if gradients.len() == 1 {
        return Ok(gradients
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .map(|gradient| vec![gradient])
            .collect());
    }
    let mut result = Vec::with_capacity(targets.len());
    for target_index in 0..targets.len() {
        result.push(
            gradients
                .iter()
                .map(|values| values[target_index].clone())
                .collect(),
        );
    }
    Ok(result)
}

fn jet_compute_curried_call_plan(
    base: &JetComputeBase,
    kind: JetComputeTransformKind,
    targets: &[i64],
    result_shape: JetComputeResultShape,
    input: JetComputeInputPack,
) -> Result<JetComputeCurriedResult, JetComputeError> {
    for tensor in input.primals.iter().chain(input.tangents.iter()) {
        jet_compute_validate_tensor(tensor)?;
    }
    if matches!(result_shape, JetComputeResultShape::TensorTuple(_))
        && !matches!(kind, JetComputeTransformKind::Gradient)
    {
        return Err(JetComputeError::Unsupported(
            "only compute.gradient can transform a Tensor tuple".to_string(),
        ));
    }
    let (tape, tracked) = jet_compute_trace_inputs(input.primals);
    let result = base.call(&tracked)?;
    let values = jet_compute_curried_result_shape(result, result_shape)?;
    let states = values
        .iter()
        .cloned()
        .map(|value| jet_compute_vjp_begin(value, tape.clone()))
        .collect::<Vec<_>>();
    match kind {
        JetComputeTransformKind::Gradient => Ok(JetComputeCurriedResult::Gradient(
            jet_compute_curried_gradient_result(&states, targets)?,
        )),
        JetComputeTransformKind::ValueAndGradient => {
            let Some(state) = states.first() else {
                return Err(JetComputeError::Unsupported(
                    "value_and_gradient base returned no value".to_string(),
                ));
            };
            Ok(JetComputeCurriedResult::ValueAndGradient {
                value: jet_compute_remove_trace_level(&state.value, &state.tape),
                gradients: jet_compute_curried_gradient_result(&states, targets)?,
            })
        }
        JetComputeTransformKind::Vjp => {
            let Some(state) = states.first() else {
                return Err(JetComputeError::Unsupported(
                    "vjp base returned no value".to_string(),
                ));
            };
            let value = jet_compute_remove_trace_level(&state.value, &state.tape);
            let pull = jet_compute_curried_insert(JetComputeCurriedEntry::Continuation {
                state: state.clone(),
                targets: targets.to_vec(),
                kind: JetComputeCurriedContinuation::Pull,
            });
            let grads = jet_compute_curried_insert(JetComputeCurriedEntry::Continuation {
                state: state.clone(),
                targets: targets.to_vec(),
                kind: JetComputeCurriedContinuation::Grads,
            });
            Ok(JetComputeCurriedResult::Vjp { value, pull, grads })
        }
        JetComputeTransformKind::Jvp => {
            let Some(state) = states.first() else {
                return Err(JetComputeError::Unsupported(
                    "jvp base returned no value".to_string(),
                ));
            };
            Ok(JetComputeCurriedResult::Jvp {
                value: jet_compute_remove_trace_level(&state.value, &state.tape),
                tangent: jet_compute_jvp(state, input.tangents)?,
            })
        }
    }
}

fn jet_compute_curried_flat_input(
    entry: &JetComputeCurriedEntry,
    input: JetComputeInputPack,
) -> Result<JetComputeInputPack, JetComputeError> {
    if !input.flat {
        return Ok(input);
    }
    let JetComputeInputPack { primals, .. } = input;
    let JetComputeCurriedEntry::Plan { base, kind, .. } = entry else {
        return Ok(JetComputeInputPack::new(primals, Vec::new()));
    };
    if !kind.is_jvp() {
        return Ok(JetComputeInputPack::new(primals, Vec::new()));
    }
    let split = base.arity;
    if primals.len() != split.saturating_mul(2) {
        return Err(JetComputeError::Unsupported(
            "jvp needs one tangent for every primal".to_string(),
        ));
    }
    Ok(JetComputeInputPack::new(
        primals[..split].to_vec(),
        primals[split..].to_vec(),
    ))
}

pub fn jet_compute_call_curried(
    handle: i64,
    input: JetComputeInputPack,
) -> Result<JetComputeCurriedResult, JetComputeError> {
    let Some(entry) = jet_compute_curried_entry(handle) else {
        return Err(JetComputeError::Unsupported(
            "autodiff callable handle is invalid or expired".to_string(),
        ));
    };
    let input = jet_compute_curried_flat_input(&entry, input)?;
    match entry {
        JetComputeCurriedEntry::Plan {
            base,
            kind,
            targets,
            result_shape,
        } => {
            if kind.is_jvp() {
                if input.primals.len() != input.tangents.len() {
                    return Err(JetComputeError::Unsupported(
                        "jvp needs one tangent for every primal".to_string(),
                    ));
                }
            } else if !input.tangents.is_empty() {
                return Err(JetComputeError::Unsupported(
                    "non-JVP autodiff callable received tangent values".to_string(),
                ));
            }
            jet_compute_curried_call_plan(&base, kind, &targets, result_shape, input)
        }
        JetComputeCurriedEntry::Continuation {
            state,
            targets,
            kind,
        } => {
            if !input.tangents.is_empty() || input.primals.len() != usize::from(matches!(kind, JetComputeCurriedContinuation::Pull)) {
                return Err(JetComputeError::Unsupported(
                    "autodiff continuation received the wrong arguments".to_string(),
                ));
            }
            let gradients = match kind {
                JetComputeCurriedContinuation::Pull => {
                    jet_compute_vjp_pull(&state, &input.primals[0], &targets)?
                }
                JetComputeCurriedContinuation::Grads => {
                    let seed = jet_compute_gradient_seed(&state)?;
                    jet_compute_vjp_pull(&state, &seed, &targets)?
                }
            };
            Ok(JetComputeCurriedResult::Gradient(
                gradients.into_iter().map(|gradient| vec![gradient]).collect(),
            ))
        }
    }
}

pub fn jet_compute_call_curried_or_panic(
    handle: i64,
    input: JetComputeInputPack,
    context: &str,
) -> JetComputeCurriedResult {
    match jet_compute_call_curried(handle, input) {
        Ok(result) => result,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

struct JetComputeVjpRun<R> {
    pub value: JetTensor,
    pub pull: std::rc::Rc<dyn Fn(&JetTensor) -> R>,
    pub grads: std::rc::Rc<dyn Fn() -> R>,
}

impl<R: Clone> Clone for JetComputeVjpRun<R> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            pull: self.pull.clone(),
            grads: self.grads.clone(),
        }
    }
}

impl<R> JetComputeVjpRun<R> {
    fn grads_or_panic(&self) -> R {
        (self.grads)()
    }
}

fn jet_compute_untracked(tensor: &JetTensor) -> JetTensor {
    let mut value = tensor.clone();
    value.trace = None;
    value
}

fn jet_compute_trace_node_for_tape(
    trace: Option<&JetComputeTrace>,
    tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> Option<usize> {
    let trace = trace?;
    if std::sync::Weak::ptr_eq(&trace.tape, &std::sync::Arc::downgrade(tape)) {
        return Some(trace.node);
    }
    jet_compute_trace_node_for_tape(trace.parent.as_deref(), tape)
}

fn jet_compute_trace_lanes(
    trace: Option<&JetComputeTrace>,
) -> Vec<(
    std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    usize,
)> {
    let mut lanes = Vec::new();
    let mut current = trace;
    while let Some(trace) = current {
        if let Some(tape) = trace.tape.upgrade() {
            lanes.push((tape, trace.node));
        }
        current = trace.parent.as_deref();
    }
    lanes
}

fn jet_compute_remove_trace_level(
    tensor: &JetTensor,
    tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> JetTensor {
    fn remove(
        trace: JetComputeTrace,
        tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    ) -> (Option<JetComputeTrace>, bool) {
        if std::sync::Weak::ptr_eq(&trace.tape, &std::sync::Arc::downgrade(tape)) {
            return (trace.parent.map(|parent| *parent), true);
        }
        let (parent, removed) = match trace.parent {
            Some(parent) => {
                let (parent, removed) = remove(*parent, tape);
                (parent.map(Box::new), removed)
            }
            None => (None, false),
        };
        (
            Some(JetComputeTrace {
                tape: trace.tape,
                node: trace.node,
                parent,
            }),
            removed,
        )
    }

    let Some(trace) = tensor.trace.clone() else {
        return tensor.clone();
    };
    let (trace, _) = remove(trace, tape);
    let mut value = tensor.clone();
    value.trace = trace.and_then(|trace| jet_compute_prune_trace(Some(trace)));
    value
}

fn jet_compute_prune_trace(trace: Option<JetComputeTrace>) -> Option<JetComputeTrace> {
    let trace = trace?;
    let parent = trace
        .parent
        .and_then(|parent| jet_compute_prune_trace(Some(*parent)).map(Box::new));
    if trace.tape.upgrade().is_none() {
        return parent.map(|parent| *parent);
    }
    Some(JetComputeTrace {
        tape: trace.tape,
        node: trace.node,
        parent,
    })
}

fn jet_compute_tape_for_parents(
    parents: &[&JetTensor],
) -> Result<
    Vec<(
        std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
        Vec<Option<usize>>,
    )>,
    JetComputeError,
> {
    let nonempty = parents
        .iter()
        .map(|parent| jet_compute_trace_lanes(parent.trace.as_ref()))
        .filter(|lanes| !lanes.is_empty())
        .collect::<Vec<_>>();
    let Some(first) = nonempty.first() else {
        return Ok(Vec::new());
    };
    if nonempty.iter().skip(1).any(|lanes| {
        !lanes.iter().any(|(tape, _)| {
            first
                .iter()
                .any(|(first_tape, _)| std::sync::Arc::ptr_eq(first_tape, tape))
        })
    }) {
        return Err(JetComputeError::Unsupported(
            "autodiff values belong to different tapes".to_string(),
        ));
    }
    let mut tapes = Vec::new();
    for lanes in &nonempty {
        for (tape, _) in lanes {
            if !tapes
                .iter()
                .any(|existing| std::sync::Arc::ptr_eq(existing, tape))
            {
                tapes.push(tape.clone());
            }
        }
    }
    Ok(tapes
        .into_iter()
        .map(|tape| {
            let ids = parents
                .iter()
                .map(|parent| {
                    jet_compute_trace_node_for_tape(parent.trace.as_ref(), &tape)
                })
                .collect();
            (tape, ids)
        })
        .collect())
}

fn jet_compute_record(
    mut output: JetTensor,
    parents: &[&JetTensor],
    values: Vec<JetTensor>,
    rule: JetComputeTapeRule,
) -> Result<JetTensor, JetComputeError> {
    // Validate before attaching a trace.  A profile is part of Tensor
    // meaning, so a recorded F32 value must be canonical just like an eager
    // value; no engine may hide a precision mismatch in tape metadata.
    jet_compute_validate_tensor(&output)?;
    let tapes = jet_compute_tape_for_parents(parents)?;
    if tapes.is_empty() {
        return Ok(output);
    }
    let mut recorded = Vec::with_capacity(tapes.len());
    for (tape, parent_ids) in tapes {
        let mut tape_guard = tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        let node = tape_guard.nodes.len();
        tape_guard.nodes.push(JetComputeTapeNode {
            parents: parent_ids,
            rule: Some(rule.clone()),
            values: values
                .iter()
                .map(|value| jet_compute_remove_trace_level(value, &tape))
                .collect(),
            output: jet_compute_remove_trace_level(&output, &tape),
        });
        recorded.push((tape.clone(), node));
    }
    let mut trace = None;
    for (tape, node) in recorded.into_iter().rev() {
        trace = Some(Box::new(JetComputeTrace {
            tape: std::sync::Arc::downgrade(&tape),
            node,
            parent: trace,
        }));
    }
    output.trace = trace.map(|trace| *trace);
    Ok(output)
}

fn jet_compute_trace_inputs(
    inputs: Vec<JetTensor>,
) -> (
    std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    Vec<JetTensor>,
) {
    let values = inputs.clone();
    let tape = std::sync::Arc::new(std::sync::Mutex::new(JetComputeTape {
        nodes: Vec::new(),
        inputs: values.clone(),
    }));
    let mut tracked = Vec::with_capacity(values.len());
    let mut guard = tape
        .lock()
        .unwrap_or_else(|_| jet_panic("Compute.rs", line!(), "autodiff tape is poisoned"));
    for (index, value) in values.iter().enumerate() {
        let node = guard.nodes.len();
        guard.nodes.push(JetComputeTapeNode {
            parents: Vec::new(),
            rule: None,
            values: vec![value.clone()],
            output: value.clone(),
        });
        let mut input = value.clone();
        input.trace = Some(JetComputeTrace {
            tape: std::sync::Arc::downgrade(&tape),
            node,
            parent: value.trace.clone().map(Box::new),
        });
        tracked.push(input);
        debug_assert_eq!(index, node);
    }
    drop(guard);
    (tape, tracked)
}

fn jet_compute_empty_tape() -> std::sync::Arc<std::sync::Mutex<JetComputeTape>> {
    std::sync::Arc::new(std::sync::Mutex::new(JetComputeTape {
        nodes: Vec::new(),
        inputs: Vec::new(),
    }))
}

fn jet_compute_vjp_begin(
    value: JetTensor,
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> JetComputeVjpState {
    JetComputeVjpState {
        output_node: jet_compute_trace_node_for_tape(value.trace.as_ref(), &tape),
        value,
        tape,
    }
}

impl JetShow for JetComputeDevice {
    fn jet_show(&self) -> String {
        match self {
            JetComputeDevice::Auto => "Auto".to_string(),
            JetComputeDevice::Cpu => "CPU".to_string(),
            JetComputeDevice::Metal => "Metal".to_string(),
            JetComputeDevice::Cuda => "CUDA".to_string(),
            JetComputeDevice::Vulkan => "Vulkan".to_string(),
            JetComputeDevice::WebGpu => "WebGPU".to_string(),
        }
    }
}

impl JetShow for JetComputePlacementReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Placement(requested={}, selected={}, backend={}, version={}, profile={}, cache={}, abilities={:?}, reason={})",
            self.requested.jet_show(),
            self.selected.jet_show(),
            self.backend,
            self.version,
            self.profile,
            self.cache,
            self.abilities,
            self.reason
        )
    }
}

impl JetShow for JetTensor {
    fn jet_show(&self) -> String {
        format!(
            "Tensor(shape={:?}, device={}, len={})",
            self.shape,
            self.device.jet_show(),
            jet_compute_tensor_numel(self)
        )
    }
}

fn jet_compute_row_major_strides(shape: &[i64]) -> Result<Vec<i64>, JetComputeError> {
    if shape.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape must have at least one axis".to_string(),
        ));
    }
    if shape.iter().any(|d| *d < 0) {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape axes must be non-negative".to_string(),
        ));
    }
    if shape
        .iter()
        .any(|d| *d > i64::try_from(MAX_TENSOR_ELEMENTS).unwrap_or(i64::MAX))
    {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape axis exceeds the Core storage limit".to_string(),
        ));
    }
    let mut strides = vec![1i64; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        let next = shape[i + 1]
            .checked_mul(strides[i + 1])
            .ok_or_else(|| JetComputeError::InvalidShape("Tensor stride overflow".to_string()))?;
        strides[i] = next;
    }
    Ok(strides)
}

fn jet_compute_numel(shape: &[i64]) -> Result<i64, JetComputeError> {
    let mut n = 1i64;
    for &d in shape {
        if d < 0 {
            return Err(JetComputeError::InvalidShape(
                "Tensor shape axes must be non-negative".to_string(),
            ));
        }
        n = n
            .checked_mul(d)
            .ok_or_else(|| JetComputeError::InvalidShape("Tensor numel overflow".to_string()))?;
    }
    Ok(n)
}

fn jet_compute_storage_len(shape: &[i64]) -> Result<usize, JetComputeError> {
    // Validate strides even when an earlier zero axis makes the element count
    // zero. Otherwise a shape such as `[0, i64::MAX]` could look empty while
    // carrying an unrepresentable axis into later indexing code.
    jet_compute_row_major_strides(shape)?;
    let n = jet_compute_numel(shape)?;
    let len = usize::try_from(n).map_err(|_| {
        JetComputeError::InvalidShape("Tensor storage length is too large".to_string())
    })?;
    if len > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(format!(
            "Tensor storage exceeds the {}-element Core limit",
            MAX_TENSOR_ELEMENTS
        )));
    }
    Ok(len)
}

fn jet_compute_view_metadata(
    tensor: &JetTensor,
) -> Result<(&[i64], usize), JetComputeError> {
    let rank = tensor.shape.len();
    if tensor.strides.len() == rank {
        return Ok((&tensor.strides, 0));
    }
    if rank.checked_add(1) != Some(tensor.strides.len()) {
        return Err(JetComputeError::InvalidShape(
            "Tensor stride and view metadata disagree".to_string(),
        ));
    }
    let offset = usize::try_from(tensor.strides[rank]).map_err(|_| {
        JetComputeError::InvalidShape("Tensor view offset must be non-negative".to_string())
    })?;
    Ok((&tensor.strides[..rank], offset))
}

fn jet_compute_view_strides(
    shape: &[i64],
    offset: usize,
) -> Result<Vec<i64>, JetComputeError> {
    let mut strides = jet_compute_row_major_strides(shape)?;
    if offset != 0 {
        strides.push(i64::try_from(offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    Ok(strides)
}

fn jet_compute_tensor_view_bounds(
    tensor: &JetTensor,
    offset: usize,
    expected_len: usize,
) -> Result<std::ops::Range<usize>, JetComputeError> {
    let expected_strides = jet_compute_row_major_strides(&tensor.shape)?;
    let (strides, metadata_offset) = jet_compute_view_metadata(tensor)?;
    if strides != expected_strides || metadata_offset != offset {
        return Err(JetComputeError::Unsupported(
            "this operation requires a contiguous Tensor view".to_string(),
        ));
    }
    let end = offset.checked_add(expected_len).ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor view end overflows backing storage".to_string())
    })?;
    if end > tensor.data.len() {
        return Err(JetComputeError::InvalidShape(
            "Tensor view exceeds backing storage".to_string(),
        ));
    }
    Ok(offset..end)
}

fn jet_compute_view_storage_end(
    tensor: &JetTensor,
    strides: &[i64],
    offset: usize,
) -> Result<usize, JetComputeError> {
    let mut relative_end = 0usize;
    for (&dim, &stride) in tensor.shape.iter().zip(strides.iter()) {
        if dim == 0 {
            continue;
        }
        let dim = usize::try_from(dim).map_err(|_| {
            JetComputeError::InvalidShape("Tensor shape axis is too large".to_string())
        })?;
        let stride = usize::try_from(stride).map_err(|_| {
            JetComputeError::InvalidShape(
                "Tensor view strides must be non-negative and representable".to_string(),
            )
        })?;
        let extent = dim.checked_sub(1).and_then(|last| last.checked_mul(stride)).ok_or_else(|| {
            JetComputeError::InvalidShape("Tensor view extent overflows backing storage".to_string())
        })?;
        relative_end = relative_end.checked_add(extent).ok_or_else(|| {
            JetComputeError::InvalidShape("Tensor view extent overflows backing storage".to_string())
        })?;
    }
    offset.checked_add(relative_end).and_then(|end| end.checked_add(1)).ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor view end overflows backing storage".to_string())
    })
}

fn jet_compute_tensor_values(tensor: &JetTensor) -> Vec<f64> {
    let Ok(expected_len) = jet_compute_storage_len(&tensor.shape) else {
        return Vec::new();
    };
    let Ok((strides, offset)) = jet_compute_view_metadata(tensor) else {
        return Vec::new();
    };
    let data = tensor.data.as_ref();
    if expected_len == 0 {
        return Vec::new();
    }
    let mut values = Vec::with_capacity(expected_len);
    for flat in 0..expected_len {
        let mut remainder = flat;
        let mut relative_offset = 0usize;
        for axis in (0..tensor.shape.len()).rev() {
            let dim = match usize::try_from(tensor.shape[axis]) {
                Ok(dim) if dim != 0 => dim,
                _ => return Vec::new(),
            };
            let index = remainder % dim;
            remainder /= dim;
            let stride = match usize::try_from(strides[axis]) {
                Ok(stride) => stride,
                Err(_) => return Vec::new(),
            };
            let term = match index.checked_mul(stride) {
                Some(term) => term,
                None => return Vec::new(),
            };
            relative_offset = match relative_offset.checked_add(term) {
                Some(offset) => offset,
                None => return Vec::new(),
            };
        }
        let physical_offset = match offset.checked_add(relative_offset) {
            Some(offset) => offset,
            None => return Vec::new(),
        };
        let Some(value) = data.get(physical_offset).copied() else {
            return Vec::new();
        };
        values.push(value);
    }
    values
}

fn jet_compute_validate_placement(
    device: JetComputeDevice,
    receipt: &JetComputePlacementReceipt,
) -> Result<(), JetComputeError> {
    if device == JetComputeDevice::Auto || receipt.selected == JetComputeDevice::Auto {
        return Err(JetComputeError::Unsupported(
            "a Tensor must record the concrete backend selected by Auto placement".to_string(),
        ));
    }
    let Some(expected_abilities) =
        jet_compute_registered_backend_abilities(&receipt.backend, &receipt.profile)
    else {
        return Err(JetComputeError::Unsupported(format!(
            "compute backend `{}` does not register profile `{}`",
            receipt.backend, receipt.profile
        )));
    };
    let registered = match receipt.selected {
        JetComputeDevice::Cpu => {
            receipt.backend == CPU_ORACLE_BACKEND
                && receipt.version == CPU_ORACLE_VERSION
                && receipt.cache == CPU_ORACLE_CACHE
                && matches!(
                    (receipt.requested, receipt.selected),
                    (JetComputeDevice::Cpu, JetComputeDevice::Cpu)
                        | (JetComputeDevice::Auto, JetComputeDevice::Cpu)
                )
        }
        JetComputeDevice::Metal => {
            receipt.backend == METAL_BACKEND
                && receipt.version == METAL_VERSION
                && receipt.cache == METAL_CACHE
                && receipt.requested == JetComputeDevice::Metal
        }
        JetComputeDevice::Cuda => {
            receipt.backend == CUDA_BACKEND
                && receipt.version == CUDA_VERSION
                && receipt.cache == CUDA_CACHE
                && receipt.requested == JetComputeDevice::Cuda
        }
        JetComputeDevice::Vulkan => {
            receipt.backend == VULKAN_BACKEND
                && receipt.version == VULKAN_VERSION
                && receipt.cache == VULKAN_CACHE
                && receipt.requested == JetComputeDevice::Vulkan
        }
        JetComputeDevice::WebGpu => {
            receipt.backend == WEBGPU_BACKEND
                && receipt.version == WEBGPU_VERSION
                && receipt.cache == WEBGPU_CACHE
                && receipt.requested == JetComputeDevice::WebGpu
        }
        JetComputeDevice::Auto => false,
    };
    if device != receipt.selected
        || !registered
        || receipt.reason.is_empty()
        || receipt.reason.chars().any(char::is_control)
        || !jet_compute_abilities_match(&receipt.abilities, expected_abilities)
    {
        return Err(JetComputeError::Device(
            "Tensor placement receipt does not match a registered backend ability".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_validate_tensor(tensor: &JetTensor) -> Result<(), JetComputeError> {
    jet_compute_validate_placement(tensor.device, &tensor.last_placement)?;
    let expected_len = jet_compute_storage_len(&tensor.shape)?;
    let (strides, offset) = jet_compute_view_metadata(tensor)?;
    if strides.iter().any(|stride| *stride < 0) {
        return Err(JetComputeError::InvalidShape(
            "Tensor view strides must be non-negative".to_string(),
        ));
    }
    if strides
        .iter()
        .zip(tensor.shape.iter())
        .any(|(stride, dim)| *dim > 1 && *stride == 0)
    {
        return Err(JetComputeError::Unsupported(
            "zero-stride Tensor views are not writable aliases".to_string(),
        ));
    }
    if expected_len == 0 {
        if offset > tensor.data.len() {
            return Err(JetComputeError::InvalidShape(
                "empty Tensor view starts outside backing storage".to_string(),
            ));
        }
        return Ok(());
    }
    let storage_end = jet_compute_view_storage_end(tensor, strides, offset)?;
    if storage_end > tensor.data.len() {
        return Err(JetComputeError::InvalidShape(
            "Tensor view exceeds backing storage".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    if values.len() != expected_len {
        return Err(JetComputeError::InvalidShape(
            "Tensor view metadata does not address its logical storage".to_string(),
        ));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    for value in &values {
        jet_compute_validate_profile_value(
            &tensor.last_placement.profile,
            *value,
            "Tensor value",
        )?;
    }
    Ok(())
}

fn jet_compute_place_with_profile(
    requested: JetComputeDevice,
    profile: &str,
) -> Result<JetComputePlacementReceipt, JetComputeError> {
    let selected = match requested {
        JetComputeDevice::Auto | JetComputeDevice::Cpu => JetComputeDevice::Cpu,
        JetComputeDevice::Metal => {
            if profile != CPU_ORACLE_F32_PROFILE {
                return Err(JetComputeError::Unsupported(
                    "Metal backend supports only F32Strict+Reproducible; create an F32 Tensor first"
                        .to_string(),
                ));
            }
            if !jet_compute_metal::available() {
                return Err(JetComputeError::Device(
                    "Metal device is unavailable; no CPU fallback was selected".to_string(),
                ));
            }
            JetComputeDevice::Metal
        }
        JetComputeDevice::Cuda => {
            if profile != CPU_ORACLE_F32_PROFILE {
                return Err(JetComputeError::Unsupported(
                    "CUDA backend supports only F32Strict+Reproducible; create an F32 Tensor first"
                        .to_string(),
                ));
            }
            if !jet_compute_cuda::available() {
                return Err(JetComputeError::Device(
                    "CUDA device is unavailable; no CPU fallback was selected".to_string(),
                ));
            }
            JetComputeDevice::Cuda
        }
        JetComputeDevice::Vulkan => {
            if profile != CPU_ORACLE_F32_PROFILE {
                return Err(JetComputeError::Unsupported(
                    "Vulkan backend supports only F32Strict+Reproducible; create an F32 Tensor first"
                        .to_string(),
                ));
            }
            if !jet_compute_vulkan::available() {
                return Err(JetComputeError::Device(
                    "Vulkan device is unavailable; no CPU fallback was selected".to_string(),
                ));
            }
            JetComputeDevice::Vulkan
        }
        JetComputeDevice::WebGpu => {
            if profile != CPU_ORACLE_F32_PROFILE {
                return Err(JetComputeError::Unsupported(
                    "WebGPU backend supports only F32Strict+Reproducible; create an F32 Tensor first"
                        .to_string(),
                ));
            }
            if !jet_compute_webgpu::available() {
                return Err(JetComputeError::Device(
                    "WebGPU device is unavailable; no CPU fallback was selected".to_string(),
                ));
            }
            JetComputeDevice::WebGpu
        }
    };
    let backend = match selected {
        JetComputeDevice::Metal => METAL_BACKEND,
        JetComputeDevice::Cuda => CUDA_BACKEND,
        JetComputeDevice::Vulkan => VULKAN_BACKEND,
        JetComputeDevice::WebGpu => WEBGPU_BACKEND,
        _ => CPU_ORACLE_BACKEND,
    };
    let version = match selected {
        JetComputeDevice::Metal => METAL_VERSION,
        JetComputeDevice::Cuda => CUDA_VERSION,
        JetComputeDevice::Vulkan => VULKAN_VERSION,
        JetComputeDevice::WebGpu => WEBGPU_VERSION,
        _ => CPU_ORACLE_VERSION,
    };
    let cache = match selected {
        JetComputeDevice::Metal => METAL_CACHE,
        JetComputeDevice::Cuda => CUDA_CACHE,
        JetComputeDevice::Vulkan => VULKAN_CACHE,
        JetComputeDevice::WebGpu => WEBGPU_CACHE,
        _ => CPU_ORACLE_CACHE,
    };
    let profile = if matches!(
        selected,
        JetComputeDevice::Metal
            | JetComputeDevice::Cuda
            | JetComputeDevice::Vulkan
            | JetComputeDevice::WebGpu
    ) {
        CPU_ORACLE_F32_PROFILE
    } else {
        profile
    };
    let abilities = jet_compute_registered_backend_abilities(backend, profile)
        .ok_or_else(|| JetComputeError::Unsupported("compute profile is not registered".to_string()))?
        .iter()
        .map(|ability| (*ability).to_string())
        .collect();
    let ability = match selected {
        JetComputeDevice::Metal => "metal.f32",
        JetComputeDevice::Cuda => "cuda.f32",
        JetComputeDevice::Vulkan => "vulkan.f32",
        JetComputeDevice::WebGpu => "webgpu.f32",
        _ if profile == CPU_ORACLE_F32_PROFILE => "cpu-oracle.f32",
        _ => "cpu-oracle.f64",
    };
    Ok(JetComputePlacementReceipt {
        requested,
        selected,
        backend: backend.to_string(),
        version: version.to_string(),
        profile: profile.to_string(),
        cache: cache.to_string(),
        abilities,
        reason: if requested == JetComputeDevice::Auto {
            format!("policy=auto; selected=cpu; ability={ability}")
        } else if selected == JetComputeDevice::Metal {
            "policy=explicit; selected=metal; ability=metal.f32".to_string()
        } else if selected == JetComputeDevice::Cuda {
            "policy=explicit; selected=cuda; ability=cuda.f32".to_string()
        } else if selected == JetComputeDevice::Vulkan {
            "policy=explicit; selected=vulkan; ability=vulkan.f32".to_string()
        } else if selected == JetComputeDevice::WebGpu {
            "policy=explicit; selected=webgpu; ability=webgpu.f32".to_string()
        } else {
            format!("policy=explicit; selected=cpu; ability={ability}")
        },
    })
}

fn jet_compute_place(
    requested: JetComputeDevice,
) -> Result<JetComputePlacementReceipt, JetComputeError> {
    jet_compute_place_with_profile(requested, CPU_ORACLE_F64_PROFILE)
}

fn jet_compute_inherit_placement(mut tensor: JetTensor, source: &JetTensor) -> JetTensor {
    tensor.device = source.device;
    tensor.last_placement = source.last_placement.clone();
    tensor.last_transfer = None;
    tensor
}

fn jet_compute_tensor_from_shape_like(
    source: &JetTensor,
    shape: Vec<i64>,
    fill: f64,
) -> Result<JetTensor, JetComputeError> {
    Ok(jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(shape, fill, JetComputeDevice::Cpu)?,
        source,
    ))
}

fn jet_compute_metal_values(
    tensor: &JetTensor,
    context: &str,
) -> Result<Vec<f32>, JetComputeError> {
    if tensor.device != JetComputeDevice::Metal {
        return Err(JetComputeError::Device(format!(
            "{context} requires a Metal Tensor"
        )));
    }
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(format!(
            "Metal {context} supports only F32Strict+Reproducible"
        )));
    }
    if !jet_compute_metal::available() {
        return Err(JetComputeError::Device(
            "Metal device was lost before launch".to_string(),
        ));
    }
    jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, context))
        .collect()
}

fn jet_compute_metal_result_like(
    source: &JetTensor,
    shape: Vec<i64>,
    values: Vec<f32>,
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&shape)?;
    if values.len() != expected {
        return Err(JetComputeError::InvalidShape(
            "Metal kernel returned the wrong storage length".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape_like(source, shape.clone(), 0.0)?;
    output.strides = jet_compute_row_major_strides(&shape)?;
    output.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_metal_binary_values(
    op: &str,
    left: &[f32],
    right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "add" => 0,
        "mul" => 1,
        "sub" => 2,
        "div" => 3,
        "maximum" => 4,
        "minimum" => 5,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported Metal binary operation `{op}`"
            )))
        }
    };
    jet_compute_metal::binary(op, left, right)
}

fn jet_compute_metal_unary_values(
    op: &str,
    values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "negate" => 0,
        "abs" => 1,
        "exp" => 2,
        "log" => 3,
        "sqrt" => 4,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported Metal unary operation `{op}`"
            )))
        }
    };
    jet_compute_metal::unary(op, values)
}

fn jet_compute_is_accelerator(device: JetComputeDevice) -> bool {
    matches!(
        device,
        JetComputeDevice::Metal
            | JetComputeDevice::Cuda
            | JetComputeDevice::Vulkan
            | JetComputeDevice::WebGpu
    )
}

fn jet_compute_cuda_values(
    tensor: &JetTensor,
    context: &str,
) -> Result<Vec<f32>, JetComputeError> {
    if tensor.device != JetComputeDevice::Cuda {
        return Err(JetComputeError::Device(format!(
            "{context} requires a CUDA Tensor"
        )));
    }
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(format!(
            "CUDA {context} supports only F32Strict+Reproducible"
        )));
    }
    if !jet_compute_cuda::available() {
        return Err(JetComputeError::Device(
            "CUDA device was lost before launch".to_string(),
        ));
    }
    jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, context))
        .collect()
}

fn jet_compute_cuda_result_like(
    source: &JetTensor,
    shape: Vec<i64>,
    values: Vec<f32>,
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&shape)?;
    if values.len() != expected {
        return Err(JetComputeError::InvalidShape(
            "CUDA kernel returned the wrong storage length".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape_like(source, shape.clone(), 0.0)?;
    output.strides = jet_compute_row_major_strides(&shape)?;
    output.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_cuda_binary_values(
    op: &str,
    left: &[f32],
    right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "add" => 0,
        "mul" => 1,
        "sub" => 2,
        "div" => 3,
        "maximum" => 4,
        "minimum" => 5,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported CUDA binary operation `{op}`"
            )))
        }
    };
    jet_compute_cuda::binary(op, left, right)
}

fn jet_compute_cuda_unary_values(
    op: &str,
    values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "negate" => 0,
        "abs" => 1,
        "exp" => 2,
        "log" => 3,
        "sqrt" => 4,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported CUDA unary operation `{op}`"
            )))
        }
    };
    jet_compute_cuda::unary(op, values)
}

fn jet_compute_vulkan_values(
    tensor: &JetTensor,
    context: &str,
) -> Result<Vec<f32>, JetComputeError> {
    if tensor.device != JetComputeDevice::Vulkan {
        return Err(JetComputeError::Device(format!(
            "{context} requires a Vulkan Tensor"
        )));
    }
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(format!(
            "Vulkan {context} supports only F32Strict+Reproducible"
        )));
    }
    if !jet_compute_vulkan::available() {
        return Err(JetComputeError::Device(
            "Vulkan device was lost before launch".to_string(),
        ));
    }
    jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, context))
        .collect()
}

fn jet_compute_vulkan_result_like(
    source: &JetTensor,
    shape: Vec<i64>,
    values: Vec<f32>,
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&shape)?;
    if values.len() != expected {
        return Err(JetComputeError::InvalidShape(
            "Vulkan kernel returned the wrong storage length".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape_like(source, shape.clone(), 0.0)?;
    output.strides = jet_compute_row_major_strides(&shape)?;
    output.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_vulkan_binary_values(
    op: &str,
    left: &[f32],
    right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "add" => 0,
        "mul" => 1,
        "sub" => 2,
        "div" => 3,
        "maximum" => 4,
        "minimum" => 5,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported Vulkan binary operation `{op}`"
            )))
        }
    };
    jet_compute_vulkan::binary(op, left, right)
}

fn jet_compute_vulkan_unary_values(
    op: &str,
    values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "negate" => 0,
        "abs" => 1,
        "exp" => 2,
        "log" => 3,
        "sqrt" => 4,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported Vulkan unary operation `{op}`"
            )))
        }
    };
    jet_compute_vulkan::unary(op, values)
}

fn jet_compute_webgpu_values(
    tensor: &JetTensor,
    context: &str,
) -> Result<Vec<f32>, JetComputeError> {
    if tensor.device != JetComputeDevice::WebGpu {
        return Err(JetComputeError::Device(format!(
            "{context} requires a WebGPU Tensor"
        )));
    }
    Err(JetComputeError::Unsupported(
        "WebGPU backend requires a browser WebGPU host".to_string(),
    ))
}

fn jet_compute_webgpu_result_like(
    _source: &JetTensor,
    _shape: Vec<i64>,
    _values: Vec<f32>,
) -> Result<JetTensor, JetComputeError> {
    Err(JetComputeError::Unsupported(
        "WebGPU backend requires a browser WebGPU host".to_string(),
    ))
}

fn jet_compute_webgpu_binary_values(
    _op: &str,
    _left: &[f32],
    _right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    Err(JetComputeError::Unsupported(
        "WebGPU backend requires a browser WebGPU host".to_string(),
    ))
}

fn jet_compute_webgpu_unary_values(
    _op: &str,
    _values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    Err(JetComputeError::Unsupported(
        "WebGPU backend requires a browser WebGPU host".to_string(),
    ))
}

fn jet_compute_accelerator_values(
    tensor: &JetTensor,
    context: &str,
) -> Result<Vec<f32>, JetComputeError> {
    match tensor.device {
        JetComputeDevice::Metal => jet_compute_metal_values(tensor, context),
        JetComputeDevice::Cuda => jet_compute_cuda_values(tensor, context),
        JetComputeDevice::Vulkan => jet_compute_vulkan_values(tensor, context),
        JetComputeDevice::WebGpu => jet_compute_webgpu_values(tensor, context),
        _ => Err(JetComputeError::Device(format!(
            "{context} requires an accelerator Tensor"
        ))),
    }
}

fn jet_compute_accelerator_result_like(
    source: &JetTensor,
    shape: Vec<i64>,
    values: Vec<f32>,
) -> Result<JetTensor, JetComputeError> {
    match source.device {
        JetComputeDevice::Metal => jet_compute_metal_result_like(source, shape, values),
        JetComputeDevice::Cuda => jet_compute_cuda_result_like(source, shape, values),
        JetComputeDevice::Vulkan => jet_compute_vulkan_result_like(source, shape, values),
        JetComputeDevice::WebGpu => jet_compute_webgpu_result_like(source, shape, values),
        _ => Err(JetComputeError::Device(
            "accelerator result requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_binary_values(
    device: JetComputeDevice,
    op: &str,
    left: &[f32],
    right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal_binary_values(op, left, right),
        JetComputeDevice::Cuda => jet_compute_cuda_binary_values(op, left, right),
        JetComputeDevice::Vulkan => jet_compute_vulkan_binary_values(op, left, right),
        JetComputeDevice::WebGpu => jet_compute_webgpu_binary_values(op, left, right),
        _ => Err(JetComputeError::Device(
            "accelerator binary operation requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_unary_values(
    device: JetComputeDevice,
    op: &str,
    values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal_unary_values(op, values),
        JetComputeDevice::Cuda => jet_compute_cuda_unary_values(op, values),
        JetComputeDevice::Vulkan => jet_compute_vulkan_unary_values(op, values),
        JetComputeDevice::WebGpu => jet_compute_webgpu_unary_values(op, values),
        _ => Err(JetComputeError::Device(
            "accelerator unary operation requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_sum(
    device: JetComputeDevice,
    values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal::sum(values),
        JetComputeDevice::Cuda => jet_compute_cuda::sum(values),
        JetComputeDevice::Vulkan => jet_compute_vulkan::sum(values),
        JetComputeDevice::WebGpu => jet_compute_webgpu::sum(values),
        _ => Err(JetComputeError::Device(
            "accelerator sum requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_mse(
    device: JetComputeDevice,
    left: &[f32],
    right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal::mse(left, right),
        JetComputeDevice::Cuda => jet_compute_cuda::mse(left, right),
        JetComputeDevice::Vulkan => jet_compute_vulkan::mse(left, right),
        JetComputeDevice::WebGpu => jet_compute_webgpu::mse(left, right),
        _ => Err(JetComputeError::Device(
            "accelerator MSE requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_mse_grad(
    device: JetComputeDevice,
    left: &[f32],
    right: &[f32],
    cot: &[f32],
    positive: bool,
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal::mse_grad(left, right, cot, positive),
        JetComputeDevice::Cuda => jet_compute_cuda::mse_grad(left, right, cot, positive),
        JetComputeDevice::Vulkan => jet_compute_vulkan::mse_grad(left, right, cot, positive),
        JetComputeDevice::WebGpu => jet_compute_webgpu::mse_grad(left, right, cot, positive),
        _ => Err(JetComputeError::Device(
            "accelerator MSE gradient requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_mse_jvp(
    device: JetComputeDevice,
    left: &[f32],
    right: &[f32],
    left_tangent: &[f32],
    right_tangent: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => {
            jet_compute_metal::mse_jvp(left, right, left_tangent, right_tangent)
        }
        JetComputeDevice::Cuda => {
            jet_compute_cuda::mse_jvp(left, right, left_tangent, right_tangent)
        }
        JetComputeDevice::Vulkan => {
            jet_compute_vulkan::mse_jvp(left, right, left_tangent, right_tangent)
        }
        JetComputeDevice::WebGpu => {
            jet_compute_webgpu::mse_jvp(left, right, left_tangent, right_tangent)
        }
        _ => Err(JetComputeError::Device(
            "accelerator MSE JVP requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_sgd(
    device: JetComputeDevice,
    parameter: &[f32],
    gradient: &[f32],
    learning_rate: f32,
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal::sgd(parameter, gradient, learning_rate),
        JetComputeDevice::Cuda => jet_compute_cuda::sgd(parameter, gradient, learning_rate),
        JetComputeDevice::Vulkan => jet_compute_vulkan::sgd(parameter, gradient, learning_rate),
        JetComputeDevice::WebGpu => jet_compute_webgpu::sgd(parameter, gradient, learning_rate),
        _ => Err(JetComputeError::Device(
            "accelerator SGD requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_accelerator_scale(
    device: JetComputeDevice,
    values: &[f32],
    scalar: f32,
) -> Result<Vec<f32>, JetComputeError> {
    match device {
        JetComputeDevice::Metal => jet_compute_metal::scale(values, scalar),
        JetComputeDevice::Cuda => jet_compute_cuda::scale(values, scalar),
        JetComputeDevice::Vulkan => jet_compute_vulkan::scale(values, scalar),
        JetComputeDevice::WebGpu => jet_compute_webgpu::scale(values, scalar),
        _ => Err(JetComputeError::Device(
            "accelerator scaling requires an accelerator Tensor".to_string(),
        )),
    }
}

fn jet_compute_require_same_contract(
    left: &JetTensor,
    right: &JetTensor,
    operation: &str,
) -> Result<(), JetComputeError> {
    if left.device != right.device {
        return Err(JetComputeError::Device(format!(
            "{operation} tensors must use the same device"
        )));
    }
    if left.last_placement.profile != right.last_placement.profile {
        return Err(JetComputeError::Device(format!(
            "{operation} tensors must use the same precision profile"
        )));
    }
    Ok(())
}

fn jet_compute_tensor_from_shape(
    shape: Vec<i64>,
    fill: f64,
    requested: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    if !fill.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let strides = jet_compute_row_major_strides(&shape)?;
    let n = jet_compute_storage_len(&shape)?;
    let receipt = jet_compute_place(requested)?;
    Ok(JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(vec![fill; n]),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

fn jet_compute_zeros(shape: &Vec<i64>) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), 0.0, JetComputeDevice::Auto)
}

fn jet_compute_ones(shape: &Vec<i64>) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), 1.0, JetComputeDevice::Auto)
}

fn jet_compute_full(shape: &Vec<i64>, value: f64) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), value, JetComputeDevice::Auto)
}

fn jet_compute_from_list(values: &Vec<f64>) -> Result<JetTensor, JetComputeError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let len = i64::try_from(values.len()).map_err(|_| {
        JetComputeError::InvalidShape("Tensor list is too long".to_string())
    })?;
    let shape = vec![len];
    let strides = jet_compute_row_major_strides(&shape)?;
    let storage_len = jet_compute_storage_len(&shape)?;
    let receipt = jet_compute_place(JetComputeDevice::Auto)?;
    Ok(JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(values[..storage_len].to_vec()),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

fn jet_compute_copy_checked(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let shape = tensor.shape.clone();
    let copy = JetTensor {
        strides: jet_compute_row_major_strides(&shape)?,
        data: std::sync::Arc::new(jet_compute_tensor_values(tensor)),
        shape,
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: tensor.last_transfer.clone(),
        trace: tensor.trace.clone(),
    };
    jet_compute_validate_tensor(&copy)?;
    Ok(copy)
}

/// Replace the ambient list projection after a mutable view write-back. The
/// adapter supplies only the already-marshalled values; storage exclusivity,
/// mutation policy, and the resulting canonical row-major metadata remain in
/// the shared Prelude.
fn jet_compute_replace_data_checked(
    tensor: &mut JetTensor,
    values: Vec<f64>,
) -> Result<(), JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
        ));
    }
    let expected = jet_compute_storage_len(&tensor.shape)?;
    if values.len() != expected {
        return Err(JetComputeError::InvalidShape(
            "Tensor write-back length does not match its shape".to_string(),
        ));
    }
    let Some(data) = std::sync::Arc::get_mut(&mut tensor.data) else {
        return Err(JetComputeError::Unsupported(
            "Tensor mutable view requires exclusive backing storage".to_string(),
        ));
    };
    *data = values;
    tensor.strides = jet_compute_row_major_strides(&tensor.shape)?;
    jet_compute_validate_tensor(tensor)
}

fn jet_compute_copy(tensor: &JetTensor) -> JetTensor {
    match jet_compute_copy_checked(tensor) {
        Ok(copy) => copy,
        Err(error) => jet_panic("Compute.rs", line!(), &error.jet_show()),
    }
}

/// Return the flat storage range selected by a bracket range.  A Tensor range
/// selects rows on its first axis, so a rank-2 matrix window keeps complete
/// rows and a higher-rank window keeps complete first-axis slabs.  This is the
/// one-dimensional bracket surface ratified by D-SHAPE-PLACE1; the inner axes
/// remain part of the same contiguous storage projection.
fn jet_compute_window_bounds(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<std::ops::Range<usize>, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let (strides, offset) = jet_compute_view_metadata(tensor)?;
    let expected_strides = jet_compute_row_major_strides(&tensor.shape)?;
    if strides != expected_strides {
        return Err(JetComputeError::Unsupported(
            "Tensor window is non-contiguous; use the strided View projection".to_string(),
        ));
    }
    let axis_len = tensor.shape.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor shape must have at least one axis".to_string())
    })?;
    let Some((axis_start, axis_end)) = jet_range_bounds(start, end, exclusive, axis_len) else {
        return Err(JetComputeError::OutOfBounds(format!(
            "Tensor range {}{}{} is outside first axis of extent {}",
            start,
            if exclusive { "..<" } else { ".." },
            end,
            axis_len
        )));
    };
    // In row-major storage the first stride is the number of scalar values in
    // one first-axis slab.  For an empty first axis the selected range is also
    // empty, so the stride is still safe to use.
    let slab = strides.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor is missing its first-axis stride".to_string())
    })?;
    let flat_start = axis_start.checked_mul(slab).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view start overflows storage".to_string())
    })?;
    let flat_end = axis_end.checked_mul(slab).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view end overflows storage".to_string())
    })?;
    let start = usize::try_from(flat_start).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor view start is outside storage".to_string())
    })?;
    let end = usize::try_from(flat_end).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor view end is outside storage".to_string())
    })?;
    let start = offset.checked_add(start).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view start overflows storage".to_string())
    })?;
    let end = offset.checked_add(end).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view end overflows storage".to_string())
    })?;
    if end > tensor.data.len() || start > end {
        return Err(JetComputeError::OutOfBounds(
            "Tensor view is outside storage".to_string(),
        ));
    }
    Ok(start..end)
}

fn jet_compute_slice_checked(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor views are not differentiable; reshape or copy before transforming".to_string(),
        ));
    }
    let axis_len = tensor.shape.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor shape must have at least one axis".to_string())
    })?;
    let Some((axis_start, axis_end)) = jet_range_bounds(start, end, exclusive, axis_len) else {
        return Err(JetComputeError::OutOfBounds(format!(
            "Tensor range {}{}{} is outside first axis of extent {}",
            start,
            if exclusive { "..<" } else { ".." },
            end,
            axis_len
        )));
    };
    let mut shape = tensor.shape.clone();
    shape[0] = axis_end.checked_sub(axis_start).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor slice has a negative extent".to_string())
    })?;
    let (source_strides, base_offset) = jet_compute_view_metadata(tensor)?;
    let first_stride = source_strides.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor is missing its first-axis stride".to_string())
    })?;
    let first_stride = usize::try_from(first_stride).map_err(|_| {
        JetComputeError::InvalidShape("Tensor view stride is not representable".to_string())
    })?;
    let axis_start = usize::try_from(axis_start).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor slice start is not representable".to_string())
    })?;
    let start_offset = base_offset
        .checked_add(axis_start.checked_mul(first_stride).ok_or_else(|| {
            JetComputeError::OutOfBounds("Tensor slice start overflows storage".to_string())
        })?)
        .ok_or_else(|| JetComputeError::OutOfBounds("Tensor slice start overflows storage".to_string()))?;
    let mut view_strides = source_strides.to_vec();
    if start_offset != 0 {
        view_strides.push(i64::try_from(start_offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    // An owned bracket slice is an ownership conversion, not another view.
    // Read the selected logical values once, then give the result independent
    // row-major storage. Read-only and mutable view helpers above retain their
    // zero-copy backing; only this owned path detaches it.
    let view = JetTensor {
        shape,
        strides: view_strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: tensor.last_transfer.clone(),
        trace: tensor.trace.clone(),
    };
    let slice = JetTensor {
        shape: view.shape.clone(),
        strides: jet_compute_row_major_strides(&view.shape)?,
        data: std::sync::Arc::new(jet_compute_tensor_values(&view)),
        device: view.device,
        last_placement: view.last_placement.clone(),
        last_transfer: view.last_transfer.clone(),
        trace: view.trace.clone(),
    };
    jet_compute_validate_tensor(&slice)?;
    Ok(slice)
}

fn jet_compute_slice(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> JetTensor {
    match jet_compute_slice_checked(tensor, start, end, exclusive) {
        Ok(slice) => slice,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_slice_range(
    tensor: &JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> JetTensor {
    jet_compute_slice(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_view_checked<'a>(
    tensor: &'a JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<&'a [f64], JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor views are not differentiable; reshape or copy before transforming".to_string(),
        ));
    }
    let bounds = jet_compute_window_bounds(tensor, start, end, exclusive)?;
    Ok(&tensor.data[bounds])
}

fn jet_compute_view<'a>(
    tensor: &'a JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> &'a [f64] {
    match jet_compute_view_checked(tensor, start, end, exclusive) {
        Ok(view) => view,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_view_range<'a>(
    tensor: &'a JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a [f64] {
    jet_compute_view(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_view_mut_checked<'a>(
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<JetComputeViewMut<'a>, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
        ));
    }
    jet_compute_window_bounds(tensor, start, end, exclusive)?;
    if std::sync::Arc::get_mut(&mut tensor.data).is_none() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutable view requires exclusive backing storage".to_string(),
        ));
    }
    Ok(JetComputeViewMut {
        tensor,
        start,
        end,
        exclusive,
    })
}

fn jet_compute_view_mut<'a>(
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> JetComputeViewMut<'a> {
    match jet_compute_view_mut_checked(tensor, start, end, exclusive) {
        Ok(view) => view,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_view_mut_range<'a>(
    tensor: &'a mut JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> JetComputeViewMut<'a> {
    jet_compute_view_mut(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_window_set_view(
    view: &mut JetComputeViewMut<'_>,
    index: i64,
    value: f64,
) -> Result<(), String> {
    jet_compute_window_set(
        view.tensor,
        view.start,
        view.end,
        view.exclusive,
        index,
        value,
    )
}

fn jet_compute_window_get_view(
    view: &JetComputeViewMut<'_>,
    index: i64,
    file: &str,
    line: u32,
) -> f64 {
    match jet_compute_window_get(
        view.tensor,
        view.start,
        view.end,
        view.exclusive,
        index,
    ) {
        Ok(value) => value,
        Err(error) => jet_panic(file, line, &error),
    }
}

impl<'a> JetComputeViewMut<'a> {
    fn len(&self) -> i64 {
        match jet_compute_window_bounds(self.tensor, self.start, self.end, self.exclusive) {
            Ok(bounds) => i64::try_from(bounds.len()).unwrap_or(i64::MAX),
            Err(error) => jet_panic("Compute.rs", line!(), &error.jet_show()),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn to_vec(&self) -> Vec<f64> {
        match jet_compute_view_checked(self.tensor, self.start, self.end, self.exclusive) {
            Ok(view) => view.to_vec(),
            Err(error) => jet_panic("Compute.rs", line!(), &error.jet_show()),
        }
    }
}

/// The one mutable Tensor-window write seam. Every engine supplies the Tensor
/// handle, the original window bounds, and the logical element coordinate;
/// this Prelude operation owns trace policy, exclusive backing storage,
/// window bounds, element addressing, finite-value validation, mutation, and
/// their canonical errors.
fn jet_compute_window_set(
    tensor: &mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    index: i64,
    value: f64,
) -> Result<(), String> {
    if tensor.trace.is_some() {
        return Err("Tensor mutation is not differentiable; use a pure Tensor function".to_string());
    }
    if !value.is_finite() {
        return Err("Tensor values must be finite".to_string());
    }
    jet_compute_validate_profile_value(
        &tensor.last_placement.profile,
        value,
        "Tensor write value",
    )
    .map_err(|error| error.jet_show())?;
    let bounds = jet_compute_window_bounds(tensor, start, end, exclusive)
        .map_err(|error| error.jet_show())?;
    // Validate the logical element before exclusivity. A valid empty window is
    // still a view; an attempted element write must therefore report the
    // canonical element-bounds error even when the owner has another handle.
    let relative = jet_view_address(bounds.len(), index)?;
    let Some(data) = std::sync::Arc::get_mut(&mut tensor.data) else {
        return Err("Tensor mutable view requires exclusive backing storage".to_string());
    };
    let offset = bounds
        .start
        .checked_add(relative)
        .ok_or_else(|| "Tensor view index is outside storage".to_string())?;
    let Some(slot) = data.get_mut(offset) else {
        return Err("Tensor view index is outside storage".to_string());
    };
    *slot = value;
    Ok(())
}

/// The matching checked read for a Tensor window. It keeps element addressing
/// (including empty-window rejection) beside the mutable write seam.
fn jet_compute_window_get(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    index: i64,
) -> Result<f64, String> {
    let view = jet_compute_view_checked(tensor, start, end, exclusive)
        .map_err(|error| error.jet_show())?;
    jet_view_get_checked(view, index)
}

fn jet_compute_tensor_shape(tensor: &JetTensor) -> Vec<i64> {
    tensor.shape.clone()
}

fn jet_compute_tensor_rank(tensor: &JetTensor) -> i64 {
    i64::try_from(tensor.shape.len()).unwrap_or(i64::MAX)
}

fn jet_compute_tensor_numel(tensor: &JetTensor) -> i64 {
    i64::try_from(jet_compute_tensor_values(tensor).len()).unwrap_or(i64::MAX)
}

fn jet_compute_tensor_device(tensor: &JetTensor) -> String {
    tensor.device.jet_show()
}

fn jet_compute_tensor_placement(tensor: &JetTensor) -> String {
    tensor.last_placement.jet_show()
}

fn jet_compute_tensor_to_list(tensor: &JetTensor) -> Vec<f64> {
    if tensor.trace.is_some() {
        jet_panic(
            "Compute.rs",
            line!(),
            "Tensor value reads have no registered autodiff rule",
        );
    }
    jet_compute_tensor_values(tensor)
}

fn jet_compute_offset(tensor: &JetTensor, indices: &[i64]) -> Result<usize, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if indices.len() != tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(format!(
            "expected {} indices, got {}",
            tensor.shape.len(),
            indices.len()
        )));
    }
    let (strides, base_offset) = jet_compute_view_metadata(tensor)?;
    let mut relative_offset = 0i64;
    for (i, (&idx, (&dim, &stride))) in indices
        .iter()
        .zip(tensor.shape.iter().zip(strides.iter()))
        .enumerate()
    {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "index {} out of range for axis {} of extent {}",
                idx, i, dim
            )));
        }
        let term = idx.checked_mul(stride).ok_or_else(|| {
            JetComputeError::OutOfBounds("tensor index offset overflow".to_string())
        })?;
        relative_offset = relative_offset.checked_add(term).ok_or_else(|| {
            JetComputeError::OutOfBounds("tensor index offset overflow".to_string())
        })?;
    }
    usize::try_from(relative_offset)
        .ok()
        .and_then(|relative| base_offset.checked_add(relative))
        .filter(|index| *index < tensor.data.len())
        .ok_or_else(|| JetComputeError::OutOfBounds("tensor index is outside storage".to_string()))
}

fn jet_compute_get_raw(tensor: &JetTensor, indices: &[i64]) -> Result<f64, JetComputeError> {
    let offset = jet_compute_offset(tensor, indices)?;
    tensor.data.get(offset).ok_or_else(|| {
        JetComputeError::OutOfBounds("tensor index is outside storage".to_string())
    }).copied()
}

fn jet_compute_get(tensor: &JetTensor, indices: &[i64]) -> Result<f64, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor element reads have no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_get_raw(tensor, indices)
}

impl JetComputeSetTarget for JetTensor {
    type Error = JetComputeError;

    fn jet_compute_set_target(
        &mut self,
        indices: &[i64],
        value: f64,
    ) -> Result<(), JetComputeError> {
        if self.trace.is_some() {
            return Err(JetComputeError::Unsupported(
                "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
            ));
        }
        if !value.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "Tensor values must be finite".to_string(),
            ));
        }
        jet_compute_validate_profile_value(
            &self.last_placement.profile,
            value,
            "Tensor write value",
        )?;
        let offset = jet_compute_offset(self, indices)?;
        let Some(data) = std::sync::Arc::get_mut(&mut self.data) else {
            return Err(JetComputeError::Unsupported(
                "Tensor write requires an exclusive ViewMut borrow".to_string(),
            ));
        };
        let Some(slot) = data.get_mut(offset) else {
            return Err(JetComputeError::OutOfBounds(
                "tensor index is outside storage".to_string(),
            ));
        };
        *slot = value;
        Ok(())
    }
}

fn jet_compute_add(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_binary("add", a, b)
}

fn jet_compute_mul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_binary("mul", a, b)
}

fn jet_compute_reshape(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let n = jet_compute_storage_len(shape)?;
    let tensor_n = jet_compute_storage_len(&tensor.shape)?;
    if n != tensor_n {
        return Err(JetComputeError::InvalidShape(format!(
            "reshape numel {} does not match tensor numel {}",
            n,
            tensor_n
        )));
    }
    let (source_strides, offset) = jet_compute_view_metadata(tensor)?;
    let source_row_major = jet_compute_row_major_strides(&tensor.shape)?;
    let (strides, data) = if source_strides == source_row_major {
        (jet_compute_view_strides(shape, offset)?, tensor.data.clone())
    } else {
        // Reshape is an explicit rank/layout conversion. A non-contiguous view
        // cannot be relabeled as contiguous storage, so materialize its logical
        // order into a new owner.
        (
            jet_compute_row_major_strides(shape)?,
            std::sync::Arc::new(jet_compute_tensor_values(tensor)),
        )
    };
    let output = JetTensor {
        shape: shape.clone(),
        strides,
        data,
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Reshape {
            source_shape: tensor.shape.clone(),
        },
    )
}

/// Matrix alias: rank-2 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_matrix(rows: i64, cols: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if rows < 0 || cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "Matrix rows and cols must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![rows, cols], fill, JetComputeDevice::Cpu)
}

/// Vec alias: rank-1 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_vec(len: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if len < 0 {
        return Err(JetComputeError::InvalidShape(
            "Vec length must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![len], fill, JetComputeDevice::Cpu)
}

fn jet_compute_matmul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul requires rank-2 tensors".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_require_same_contract(a, b, "matmul")?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if m < 0 || k < 0 || k2 < 0 || n < 0 {
        return Err(JetComputeError::InvalidShape(
            "matmul dimensions must be non-negative".to_string(),
        ));
    }
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul inner dims {} and {} disagree",
            k, k2
        )));
    }
    if a.device == JetComputeDevice::Metal {
        let rows = usize::try_from(m)
            .map_err(|_| JetComputeError::InvalidShape("Metal matmul rows are too large".to_string()))?;
        let inner = usize::try_from(k)
            .map_err(|_| JetComputeError::InvalidShape("Metal matmul inner dimension is too large".to_string()))?;
        let cols = usize::try_from(n)
            .map_err(|_| JetComputeError::InvalidShape("Metal matmul columns are too large".to_string()))?;
        let left = jet_compute_metal_values(a, "matmul input")?;
        let right = jet_compute_metal_values(b, "matmul input")?;
        let data = jet_compute_metal::matmul(&left, &right, rows, inner, cols)?;
        return jet_compute_record(
            jet_compute_metal_result_like(a, vec![m, n], data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::Matmul,
        );
    }
    if a.device == JetComputeDevice::Cuda {
        let rows = usize::try_from(m)
            .map_err(|_| JetComputeError::InvalidShape("CUDA matmul rows are too large".to_string()))?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("CUDA matmul inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("CUDA matmul columns are too large".to_string())
        })?;
        let left = jet_compute_cuda_values(a, "matmul input")?;
        let right = jet_compute_cuda_values(b, "matmul input")?;
        let data = jet_compute_cuda::matmul(&left, &right, rows, inner, cols)?;
        return jet_compute_record(
            jet_compute_cuda_result_like(a, vec![m, n], data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::Matmul,
        );
    }
    if a.device == JetComputeDevice::Vulkan {
        let rows = usize::try_from(m).map_err(|_| {
            JetComputeError::InvalidShape("Vulkan matmul rows are too large".to_string())
        })?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("Vulkan matmul inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("Vulkan matmul columns are too large".to_string())
        })?;
        let left = jet_compute_vulkan_values(a, "matmul input")?;
        let right = jet_compute_vulkan_values(b, "matmul input")?;
        let data = jet_compute_vulkan::matmul(&left, &right, rows, inner, cols)?;
        return jet_compute_record(
            jet_compute_vulkan_result_like(a, vec![m, n], data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::Matmul,
        );
    }
    if a.device == JetComputeDevice::WebGpu {
        let rows = usize::try_from(m).map_err(|_| {
            JetComputeError::InvalidShape("WebGPU matmul rows are too large".to_string())
        })?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("WebGPU matmul inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("WebGPU matmul columns are too large".to_string())
        })?;
        let left = jet_compute_webgpu_values(a, "matmul input")?;
        let right = jet_compute_webgpu_values(b, "matmul input")?;
        let data = jet_compute_webgpu::matmul(&left, &right, rows, inner, cols)?;
        return jet_compute_record(
            jet_compute_webgpu_result_like(a, vec![m, n], data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::Matmul,
        );
    }
    let f32_profile = a.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let mut out = jet_compute_tensor_from_shape_like(a, vec![m, n], 0.0)?;
    for i in 0..m {
        for j in 0..n {
            let mut sum = 0.0;
            for t in 0..k {
                let av = jet_compute_get_raw(a, &vec![i, t])?;
                let bv = jet_compute_get_raw(b, &vec![t, j])?;
                sum = if f32_profile {
                    let av = jet_compute_f32_value(av, "matmul input")?;
                    let bv = jet_compute_f32_value(bv, "matmul input")?;
                    f64::from(av * bv + sum as f32)
                } else {
                    sum + av * bv
                };
                if !sum.is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "matmul accumulation produced a non-finite value".to_string(),
                    ));
                }
            }
            jet_compute_set(&mut out, &vec![i, j], sum)?;
        }
    }
    jet_compute_record(
        out,
        &[a, b],
        vec![a.clone(), b.clone()],
        JetComputeTapeRule::Matmul,
    )
}

fn jet_compute_device_cpu() -> JetComputeDevice {
    JetComputeDevice::Cpu
}

fn jet_compute_device_auto() -> JetComputeDevice {
    JetComputeDevice::Auto
}

fn jet_compute_device_metal() -> JetComputeDevice {
    JetComputeDevice::Metal
}

fn jet_compute_device_cuda() -> JetComputeDevice {
    JetComputeDevice::Cuda
}

fn jet_compute_device_vulkan() -> JetComputeDevice {
    JetComputeDevice::Vulkan
}

fn jet_compute_device_webgpu() -> JetComputeDevice {
    JetComputeDevice::WebGpu
}

fn jet_compute_metal_upload(tensor: &JetTensor) -> Result<(), JetComputeError> {
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(
            "Metal transfers support only F32Strict+Reproducible".to_string(),
        ));
    }
    if !jet_compute_metal::available() {
        return Err(JetComputeError::Device(
            "Metal device was lost before transfer".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "Metal transfer"))
        .collect::<Result<Vec<_>, _>>()?;
    jet_compute_metal::copy(&values).map(|_| ())
}

fn jet_compute_cuda_upload(tensor: &JetTensor) -> Result<(), JetComputeError> {
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(
            "CUDA transfers support only F32Strict+Reproducible".to_string(),
        ));
    }
    if !jet_compute_cuda::available() {
        return Err(JetComputeError::Device(
            "CUDA device was lost before transfer".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "CUDA transfer"))
        .collect::<Result<Vec<_>, _>>()?;
    jet_compute_cuda::copy(&values).map(|_| ())
}

fn jet_compute_vulkan_upload(tensor: &JetTensor) -> Result<(), JetComputeError> {
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(
            "Vulkan transfers support only F32Strict+Reproducible".to_string(),
        ));
    }
    if !jet_compute_vulkan::available() {
        return Err(JetComputeError::Device(
            "Vulkan device was lost before transfer".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "Vulkan transfer"))
        .collect::<Result<Vec<_>, _>>()?;
    jet_compute_vulkan::copy(&values).map(|_| ())
}

fn jet_compute_webgpu_upload(tensor: &JetTensor) -> Result<(), JetComputeError> {
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(
            "WebGPU transfers support only F32Strict+Reproducible".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "WebGPU transfer"))
        .collect::<Result<Vec<_>, _>>()?;
    jet_compute_webgpu::copy(&values).map(|_| ())
}

fn jet_compute_on_device(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let mut receipt = jet_compute_place_with_profile(device, &tensor.last_placement.profile)?;
    match receipt.selected {
        JetComputeDevice::Metal => jet_compute_metal_upload(tensor)?,
        JetComputeDevice::Cuda => jet_compute_cuda_upload(tensor)?,
        JetComputeDevice::Vulkan => jet_compute_vulkan_upload(tensor)?,
        JetComputeDevice::WebGpu => jet_compute_webgpu_upload(tensor)?,
        _ => {}
    }
    if receipt.selected == JetComputeDevice::Cpu
        && tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE
    {
        receipt.reason = if device == JetComputeDevice::Auto {
            "policy=auto; selected=cpu; ability=cpu-oracle.f32".to_string()
        } else {
            "policy=explicit; selected=cpu; ability=cpu-oracle.f32".to_string()
        };
    }
    Ok(JetTensor {
        shape: tensor.shape.clone(),
        strides: tensor.strides.clone(),
        data: tensor.data.clone(),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: tensor.trace.clone(),
    })
}

// ── D-COMPUTE1=D / #1136: ndarray broadcast, ufuncs, reductions ─────────────

fn jet_compute_broadcast_shape(
    a: &[i64],
    b: &[i64],
) -> Result<Vec<i64>, JetComputeError> {
    if a.is_empty() || b.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "broadcasting requires ranked tensors".to_string(),
        ));
    }
    if a.iter().chain(b.iter()).any(|dim| *dim < 0) {
        return Err(JetComputeError::InvalidShape(
            "broadcast shapes cannot contain negative axes".to_string(),
        ));
    }
    let rank = a.len().max(b.len());
    let mut out = vec![1i64; rank];
    for i in 0..rank {
        let da = if i < rank - a.len() {
            1
        } else {
            a[i - (rank - a.len())]
        };
        let db = if i < rank - b.len() {
            1
        } else {
            b[i - (rank - b.len())]
        };
        if da == db {
            out[i] = da;
        } else if da == 1 {
            // A singleton axis expands to the other extent, including zero.
            out[i] = db;
        } else if db == 1 {
            // A singleton axis expands to the other extent, including zero.
            out[i] = da;
        } else {
            return Err(JetComputeError::RankMismatch(format!(
                "cannot broadcast shapes {:?} and {:?}",
                a, b
            )));
        }
    }
    jet_compute_storage_len(&out)?;
    Ok(out)
}

fn jet_compute_materialize_broadcast(
    tensor: &JetTensor,
    shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let n = jet_compute_storage_len(shape)?;
    let strides = jet_compute_row_major_strides(shape)?;
    let src_rank = tensor.shape.len();
    let dst_rank = shape.len();
    if src_rank == 0 || src_rank > dst_rank {
        return Err(JetComputeError::RankMismatch(format!(
            "cannot broadcast rank {} into rank {}",
            src_rank, dst_rank
        )));
    }
    if jet_compute_broadcast_shape(&tensor.shape, shape)? != shape {
        return Err(JetComputeError::InvalidShape(format!(
            "broadcast target {:?} is incompatible with {:?}",
            shape, tensor.shape
        )));
    }
    // Empty output has no source element to read.  This also makes shapes such
    // as [0, 3] broadcast-safe instead of indexing an empty backing vector.
    if n == 0 {
        return jet_compute_tensor_from_shape_like(tensor, shape.to_vec(), 0.0);
    }
    let mut data = Vec::with_capacity(n);
    for flat in 0..n {
        let mut rem = i64::try_from(flat).map_err(|_| {
            JetComputeError::InvalidShape("broadcast index is too large".to_string())
        })?;
        let mut destination_coords = vec![0i64; dst_rank];
        for axis in (0..dst_rank).rev() {
            let dim = shape[axis];
            destination_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let rank_delta = dst_rank - src_rank;
        let source_coords = (0..src_rank)
            .map(|axis| {
                if tensor.shape[axis] == 1 {
                    0
                } else {
                    destination_coords[rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        data.push(jet_compute_get_raw(tensor, &source_coords)?);
    }
    let mut output = jet_compute_tensor_from_shape_like(tensor, shape.to_vec(), 0.0)?;
    output.strides = strides;
    output.data = std::sync::Arc::new(data);
    Ok(output)
}

fn jet_compute_broadcast_to(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    let out_shape = jet_compute_broadcast_shape(&tensor.shape, shape)?;
    if &out_shape != shape {
        return Err(JetComputeError::InvalidShape(format!(
            "broadcast target {:?} is incompatible with {:?}",
            shape, tensor.shape
        )));
    }
    let output = jet_compute_materialize_broadcast(tensor, shape)?;
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Broadcast {
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_transpose(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "transpose requires rank-2 tensor".to_string(),
        ));
    }
    let (source_strides, offset) = jet_compute_view_metadata(tensor)?;
    let mut strides = vec![source_strides[1], source_strides[0]];
    if offset != 0 {
        strides.push(i64::try_from(offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    let out = JetTensor {
        shape: vec![tensor.shape[1], tensor.shape[0]],
        strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_validate_tensor(&out)?;
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Transpose,
    )
}

fn jet_compute_sum_axis(tensor: &JetTensor, axis: i64) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let Some(axis) = usize::try_from(axis)
        .ok()
        .filter(|index| *index < tensor.shape.len())
    else {
        return Err(JetComputeError::OutOfBounds(format!(
            "sum_axis axis {} out of range for rank {}",
            axis,
            tensor.shape.len()
        )));
    };
    let mut out_shape = Vec::new();
    for (i, &d) in tensor.shape.iter().enumerate() {
        if i != axis {
            out_shape.push(d);
        }
    }
    if out_shape.is_empty() {
        out_shape.push(1);
    }
    if jet_compute_is_accelerator(tensor.device) {
        let mut out = jet_compute_tensor_from_shape_like(tensor, out_shape.clone(), 0.0)?;
        let axis_len = usize::try_from(tensor.shape[axis]).map_err(|_| {
            JetComputeError::InvalidShape("accelerator sum_axis extent is too large".to_string())
        })?;
        let out_n = usize::try_from(jet_compute_numel(&out_shape)?).map_err(|_| {
            JetComputeError::InvalidShape("accelerator sum_axis output is too large".to_string())
        })?;
        for flat in 0..out_n {
            let mut rem = flat as i64;
            let mut out_coords = vec![0i64; out_shape.len()];
            for index in (0..out_shape.len()).rev() {
                let dim = out_shape[index];
                out_coords[index] = if dim == 0 { 0 } else { rem % dim };
                rem = if dim == 0 { 0 } else { rem / dim };
            }
            let mut coords = vec![0i64; tensor.shape.len()];
            let mut out_index = 0;
            for index in 0..tensor.shape.len() {
                if index != axis {
                    coords[index] = out_coords[out_index];
                    out_index += 1;
                }
            }
            let mut values = Vec::with_capacity(axis_len);
            for value in 0..axis_len {
                coords[axis] = value as i64;
                values.push(jet_compute_f32_value(
                    jet_compute_get_raw(tensor, &coords)?,
                    "accelerator sum input",
                )?);
            }
            let sum = if values.is_empty() {
                0.0
            } else {
                jet_compute_accelerator_sum(tensor.device, &values)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        JetComputeError::Device(format!(
                            "{} sum returned no value",
                            tensor.device.jet_show()
                        ))
                    })?
            };
            jet_compute_set(&mut out, &out_coords, f64::from(sum))?;
        }
        return jet_compute_record(
            out,
            &[tensor],
            vec![tensor.clone()],
            JetComputeTapeRule::SumAxis {
                axis,
                source_shape: tensor.shape.clone(),
            },
        );
    }
    let f32_profile = tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let mut out = jet_compute_tensor_from_shape_like(tensor, out_shape.clone(), 0.0)?;
    let axis_len = tensor.shape[axis];
    let out_n = jet_compute_numel(&out_shape)?;
    for flat in 0..out_n {
        let mut coords = vec![0i64; tensor.shape.len()];
        let mut rem = flat;
        let mut out_coords = vec![0i64; out_shape.len()];
        for i in (0..out_shape.len()).rev() {
            let dim = out_shape[i];
            out_coords[i] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let mut o = 0usize;
        for i in 0..tensor.shape.len() {
            if i == axis {
                coords[i] = 0;
            } else {
                coords[i] = out_coords[o];
                o += 1;
            }
        }
        let mut sum = 0.0;
        for k in 0..axis_len {
            coords[axis] = k;
            let value = jet_compute_get_raw(tensor, &coords)?;
            sum = if f32_profile {
                let value = jet_compute_f32_value(value, "sum_axis input")?;
                f64::from(value + sum as f32)
            } else {
                sum + value
            };
            if !sum.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sum_axis accumulation produced a non-finite value".to_string(),
                ));
            }
        }
        jet_compute_set(&mut out, &out_coords, sum)?;
    }
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::SumAxis {
            axis,
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_unary(op: &str, tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if !matches!(op, "negate" | "abs" | "exp" | "log" | "sqrt") {
        return Err(JetComputeError::Unsupported(format!(
            "unsupported unary compute operation `{op}`"
        )));
    }
    if jet_compute_is_accelerator(tensor.device) {
        let values = jet_compute_accelerator_values(tensor, "unary input")?;
        if op == "log" && values.iter().any(|value| *value <= 0.0) {
            return Err(JetComputeError::Arithmetic(
                "log requires strictly positive values".to_string(),
            ));
        }
        if op == "sqrt" && values.iter().any(|value| *value < 0.0) {
            return Err(JetComputeError::Arithmetic(
                "sqrt requires non-negative values".to_string(),
            ));
        }
        let data = jet_compute_accelerator_unary_values(tensor.device, op, &values)?;
        return jet_compute_record(
            jet_compute_accelerator_result_like(tensor, tensor.shape.clone(), data)?,
            &[tensor],
            vec![tensor.clone()],
            JetComputeTapeRule::Unary(op.to_string()),
        );
    }
    let f32_profile = tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let values = jet_compute_tensor_values(tensor);
    let mut data = Vec::with_capacity(values.len());
    for value in values {
        let output = if f32_profile {
            let value = jet_compute_f32_value(value, "unary input")?;
            let output = match op {
                "negate" => -value,
                "abs" => value.abs(),
                "exp" => value.exp(),
                "log" if value > 0.0 => value.ln(),
                "log" => {
                    return Err(JetComputeError::Arithmetic(
                        "log requires strictly positive values".to_string(),
                    ));
                }
                "sqrt" if value >= 0.0 => value.sqrt(),
                "sqrt" => {
                    return Err(JetComputeError::Arithmetic(
                        "sqrt requires non-negative values".to_string(),
                    ));
                }
                _ => unreachable!("unvalidated unary operation"),
            };
            f64::from(output)
        } else {
            match op {
                "negate" => -value,
                "abs" => value.abs(),
                "exp" => value.exp(),
                "log" if value > 0.0 => value.ln(),
                "log" => {
                    return Err(JetComputeError::Arithmetic(
                        "log requires strictly positive values".to_string(),
                    ));
                }
                "sqrt" if value >= 0.0 => value.sqrt(),
                "sqrt" => {
                    return Err(JetComputeError::Arithmetic(
                        "sqrt requires non-negative values".to_string(),
                    ));
                }
                _ => unreachable!("unvalidated unary operation"),
            }
        };
        if !output.is_finite() {
            return Err(JetComputeError::Arithmetic(format!(
                "unary operation `{op}` produced a non-finite value"
            )));
        }
        data.push(output);
    }
    let mut output = jet_compute_tensor_from_shape_like(tensor, tensor.shape.clone(), 0.0)?;
    output.strides = jet_compute_row_major_strides(&tensor.shape)?;
    output.data = std::sync::Arc::new(data);
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Unary(op.to_string()),
    )
}

fn jet_compute_binary(
    op: &str,
    a: &JetTensor,
    b: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_require_same_contract(a, b, "compute operation")?;
    let shape = jet_compute_broadcast_shape(&a.shape, &b.shape)?;
    if !matches!(op, "sub" | "div" | "maximum" | "minimum" | "add" | "mul") {
        return Err(JetComputeError::Unsupported(format!(
            "unsupported binary compute operation `{op}`"
        )));
    }
    let rule = match op {
        "add" => JetComputeTapeRule::Add,
        "sub" => JetComputeTapeRule::Sub,
        "mul" => JetComputeTapeRule::Mul,
        "div" => JetComputeTapeRule::Div,
        "maximum" => JetComputeTapeRule::Maximum,
        "minimum" => JetComputeTapeRule::Minimum,
        _ => unreachable!("validated binary operation"),
    };
    if jet_compute_is_accelerator(a.device) {
        let left = jet_compute_materialize_broadcast(a, &shape)?;
        let right = jet_compute_materialize_broadcast(b, &shape)?;
        let left_values = jet_compute_accelerator_values(&left, "binary input")?;
        let right_values = jet_compute_accelerator_values(&right, "binary input")?;
        if op == "div" && right_values.iter().any(|value| *value == 0.0) {
            return Err(JetComputeError::Arithmetic(
                "division by zero in compute operation".to_string(),
            ));
        }
        let data = jet_compute_accelerator_binary_values(a.device, op, &left_values, &right_values)?;
        return jet_compute_record(
            jet_compute_accelerator_result_like(a, shape, data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            rule,
        );
    }
    // D-COMPUTE-FUSE1: broadcast indexing and the elementwise operation are
    // one eager Prelude loop. Do not materialize either broadcast operand;
    // this is the shared fusion path for AOT, comptime, and dev evaluation.
    let f32_profile = a.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let n = jet_compute_storage_len(&shape)?;
    let mut data = Vec::with_capacity(n);
    let rank = shape.len();
    let left_rank_delta = rank - a.shape.len();
    let right_rank_delta = rank - b.shape.len();
    for flat in 0..n {
        let mut rem = i64::try_from(flat).map_err(|_| {
            JetComputeError::InvalidShape("broadcast index is too large".to_string())
        })?;
        let mut output_coords = vec![0i64; rank];
        for axis in (0..rank).rev() {
            let dim = shape[axis];
            output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let left_coords = (0..a.shape.len())
            .map(|axis| {
                if a.shape[axis] == 1 {
                    0
                } else {
                    output_coords[left_rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        let right_coords = (0..b.shape.len())
            .map(|axis| {
                if b.shape[axis] == 1 {
                    0
                } else {
                    output_coords[right_rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        let x = jet_compute_get_raw(a, &left_coords)?;
        let y = jet_compute_get_raw(b, &right_coords)?;
        if op == "div" && y == 0.0 {
            return Err(JetComputeError::Arithmetic(
                "division by zero in compute operation".to_string(),
            ));
        }
        let output = if f32_profile {
            let x = jet_compute_f32_value(x, "binary operation input")?;
            let y = jet_compute_f32_value(y, "binary operation input")?;
            let output = match op {
                "sub" => x - y,
                "div" => x / y,
                "maximum" => x.max(y),
                "minimum" => x.min(y),
                "add" => x + y,
                "mul" => x * y,
                _ => unreachable!("unvalidated binary operation"),
            };
            f64::from(output)
        } else {
            match op {
                "sub" => x - y,
                "div" => x / y,
                "maximum" => x.max(y),
                "minimum" => x.min(y),
                "add" => x + y,
                "mul" => x * y,
                _ => unreachable!("unvalidated binary operation"),
            }
        };
        if !output.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "compute operation produced a non-finite value".to_string(),
            ));
        }
        data.push(output);
    }
    let strides = jet_compute_row_major_strides(&shape)?;
    let mut output = jet_compute_tensor_from_shape_like(a, shape, 0.0)?;
    output.strides = strides;
    output.data = std::sync::Arc::new(data);
    jet_compute_record(output, &[a, b], vec![a.clone(), b.clone()], rule)
}

// ── #1137 / D-COMPUTE1: dense linalg on the Tensor CPU oracle ───────────────

fn jet_compute_eye(n: i64) -> Result<JetTensor, JetComputeError> {
    if n < 0 {
        return Err(JetComputeError::InvalidShape(
            "eye size must be non-negative".to_string(),
        ));
    }
    let mut out = jet_compute_tensor_from_shape(vec![n, n], 0.0, JetComputeDevice::Cpu)?;
    for i in 0..n {
        jet_compute_set(&mut out, &vec![i, i], 1.0)?;
    }
    Ok(out)
}

fn jet_compute_det(tensor: &JetTensor) -> Result<f64, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "det has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "det requires a square rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if jet_compute_is_accelerator(tensor.device) {
        return Err(JetComputeError::Unsupported(
        "accelerator backend does not support det; transfer to CPU explicitly".to_string(),
        ));
    }
    let n = usize::try_from(tensor.shape[0]).map_err(|_| {
        JetComputeError::InvalidShape("det dimension is too large".to_string())
    })?;
    let matrix_len = n.checked_mul(n).ok_or_else(|| {
        JetComputeError::InvalidShape("det matrix storage length overflow".to_string())
    })?;
    let values = jet_compute_tensor_values(tensor);
    if matrix_len != values.len() {
        return Err(JetComputeError::InvalidShape(
            "det matrix storage is inconsistent".to_string(),
        ));
    }
    let mut a = values.to_vec();
    let mut det = 1.0;
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * n + i].abs() > a[pivot * n + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * n + i].abs() < 1e-15 {
            return Ok(0.0);
        }
        if pivot != i {
            for c in 0..n {
                a.swap(i * n + c, pivot * n + c);
            }
            det = -det;
        }
        let piv = a[i * n + i];
        det *= piv;
        if !det.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "det overflowed to a non-finite value".to_string(),
            ));
        }
        for r in (i + 1)..n {
            let factor = a[r * n + i] / piv;
            if !factor.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "det elimination produced a non-finite factor".to_string(),
                ));
            }
            for c in i..n {
                a[r * n + c] -= factor * a[i * n + c];
                if !a[r * n + c].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "det elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    Ok(det)
}

fn jet_compute_inv(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "inv has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "inv requires a square rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if jet_compute_is_accelerator(tensor.device) {
        return Err(JetComputeError::Unsupported(
        "accelerator backend does not support inv; transfer to CPU explicitly".to_string(),
        ));
    }
    let n = usize::try_from(tensor.shape[0]).map_err(|_| {
        JetComputeError::InvalidShape("inv dimension is too large".to_string())
    })?;
    let width = n.checked_mul(2).ok_or_else(|| {
        JetComputeError::InvalidShape("inv augmented width overflow".to_string())
    })?;
    let matrix_len = n.checked_mul(width).ok_or_else(|| {
        JetComputeError::InvalidShape("inv matrix storage length overflow".to_string())
    })?;
    if matrix_len > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(
            "inv workspace exceeds the Core storage limit".to_string(),
        ));
    }
    let mut a = vec![0.0; matrix_len];
    for i in 0..n {
        for j in 0..n {
            a[i * width + j] = jet_compute_get_raw(tensor, &vec![i as i64, j as i64])?;
            a[i * width + n + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * width + i].abs() > a[pivot * width + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * width + i].abs() < 1e-15 {
            return Err(JetComputeError::InvalidShape(
                "matrix is singular".to_string(),
            ));
        }
        if pivot != i {
            for c in 0..width {
                a.swap(i * width + c, pivot * width + c);
            }
        }
        let piv = a[i * width + i];
        for c in 0..width {
            a[i * width + c] /= piv;
            if !a[i * width + c].is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "inv normalization produced a non-finite value".to_string(),
                ));
            }
        }
        for r in 0..n {
            if r == i {
                continue;
            }
            let factor = a[r * width + i];
            for c in 0..width {
                a[r * width + c] -= factor * a[i * width + c];
                if !a[r * width + c].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "inv elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    let mut out = jet_compute_tensor_from_shape(
        vec![tensor.shape[0], tensor.shape[1]],
        0.0,
        JetComputeDevice::Cpu,
    )?;
    for i in 0..n {
        for j in 0..n {
            jet_compute_set(&mut out, &vec![i as i64, j as i64], a[i * width + n + j])?;
        }
    }
    Ok(out)
}

fn jet_compute_solve(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.trace.is_some() || b.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "solve has no registered autodiff rule".to_string(),
        ));
    }
    if a.shape.len() != 2 || a.shape[0] != a.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "solve requires a square rank-2 coefficient tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    if jet_compute_is_accelerator(a.device) || jet_compute_is_accelerator(b.device) {
        return Err(JetComputeError::Unsupported(
        "accelerator backend does not support solve; transfer inputs to CPU explicitly".to_string(),
        ));
    }
    let n = usize::try_from(a.shape[0])
        .map_err(|_| JetComputeError::InvalidShape("solve dimension is too large".to_string()))?;
    let rhs_cols = match b.shape.as_slice() {
        [rows] if *rows == a.shape[0] => 1,
        [rows, cols] if *rows == a.shape[0] && *cols >= 0 => usize::try_from(*cols).map_err(|_| {
            JetComputeError::InvalidShape("solve right-hand side is too large".to_string())
        })?,
        _ => {
            return Err(JetComputeError::RankMismatch(format!(
                "solve expects a length-{} vector or a matrix with {} rows",
                a.shape[0], a.shape[0]
            )))
        }
    };
    let width = n.checked_add(rhs_cols).ok_or_else(|| {
        JetComputeError::InvalidShape("solve augmented width overflow".to_string())
    })?;
    let workspace = n.checked_mul(width).ok_or_else(|| {
        JetComputeError::InvalidShape("solve workspace length overflow".to_string())
    })?;
    if workspace > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(
            "solve workspace exceeds the Core storage limit".to_string(),
        ));
    }
    let mut augmented = vec![vec![0.0; width]; n];
    for row in 0..n {
        for col in 0..n {
            augmented[row][col] = jet_compute_get_raw(a, &[row as i64, col as i64])?;
        }
        for col in 0..rhs_cols {
            augmented[row][n + col] = if b.shape.len() == 1 {
                jet_compute_get_raw(b, &[row as i64])?
            } else {
                jet_compute_get_raw(b, &[row as i64, col as i64])?
            };
        }
    }
    for pivot in 0..n {
        let mut best = pivot;
        for row in pivot..n {
            if augmented[row][pivot].abs() > augmented[best][pivot].abs() {
                best = row;
            }
        }
        if augmented[best][pivot].abs() < 1e-15 {
            return Err(JetComputeError::Arithmetic(
                "solve coefficient matrix is singular".to_string(),
            ));
        }
        augmented.swap(pivot, best);
        let divisor = augmented[pivot][pivot];
        for value in &mut augmented[pivot][pivot..] {
            *value /= divisor;
            if !value.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "solve normalization produced a non-finite value".to_string(),
                ));
            }
        }
        for row in 0..n {
            if row == pivot {
                continue;
            }
            let factor = augmented[row][pivot];
            if !factor.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "solve elimination produced a non-finite factor".to_string(),
                ));
            }
            for col in pivot..width {
                augmented[row][col] -= factor * augmented[pivot][col];
                if !augmented[row][col].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "solve elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    let output_shape = if b.shape.len() == 1 {
        vec![a.shape[0]]
    } else {
        vec![a.shape[0], b.shape[1]]
    };
    let mut out = jet_compute_tensor_from_shape(output_shape, 0.0, JetComputeDevice::Cpu)?;
    for row in 0..n {
        for col in 0..rhs_cols {
            let index = if b.shape.len() == 1 {
                vec![row as i64]
            } else {
                vec![row as i64, col as i64]
            };
            jet_compute_set(&mut out, &index, augmented[row][n + col])?;
        }
    }
    Ok(out)
}

/// Naive DFT on a rank-1 real tensor → interleaved [re, im, re, im, …] length 2n.
fn jet_compute_fft(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "fft has no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if jet_compute_is_accelerator(tensor.device) {
        return Err(JetComputeError::Unsupported(
        "accelerator backend does not support fft; transfer to CPU explicitly".to_string(),
        ));
    }
    if tensor.shape.len() != 1 {
        return Err(JetComputeError::RankMismatch(
            "fft requires a rank-1 tensor".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    let n = values.len();
    let output_len = n
        .checked_mul(2)
        .and_then(|length| i64::try_from(length).ok())
        .ok_or_else(|| JetComputeError::InvalidShape("fft output length overflow".to_string()))?;
    let mut out = jet_compute_tensor_from_shape(
        vec![output_len],
        0.0,
        JetComputeDevice::Cpu,
    )?;
    if n == 0 {
        return Ok(out);
    }
    for k in 0..n {
        let mut re = 0.0;
        let mut im = 0.0;
        for t in 0..n {
            let angle = -2.0 * std::f64::consts::PI * (k as f64) * (t as f64) / (n as f64);
            re += values[t] * angle.cos();
            im += values[t] * angle.sin();
        }
        jet_compute_set(&mut out, &vec![(2 * k) as i64], re)?;
        jet_compute_set(&mut out, &vec![(2 * k + 1) as i64], im)?;
    }
    Ok(out)
}

// ── #1138 / #1145: stream + transfer receipts ───────────────────────────────

#[derive(Clone)]
pub struct JetComputeStream {
    id: i64,
    device: JetComputeDevice,
    cuda: Option<std::sync::Arc<jet_compute_cuda::StreamHandle>>,
}

impl std::fmt::Debug for JetComputeStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetComputeStream")
            .field("id", &self.id)
            .field("device", &self.device)
            .finish()
    }
}

impl PartialEq for JetComputeStream {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.device == other.device
    }
}

impl Eq for JetComputeStream {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputeTransferReceipt {
    from: JetComputeDevice,
    to: JetComputeDevice,
    bytes: i64,
    fallback: String,
}

fn jet_compute_validate_transfer_receipt(
    tensor: &JetTensor,
    receipt: &JetComputeTransferReceipt,
) -> Result<(), JetComputeError> {
    if receipt.from == JetComputeDevice::Auto || receipt.to == JetComputeDevice::Auto {
        return Err(JetComputeError::Device(
            "transfer receipt must name concrete source and destination devices".to_string(),
        ));
    }
    if receipt.to != tensor.device {
        return Err(JetComputeError::Device(
            "transfer receipt destination does not match Tensor placement".to_string(),
        ));
    }
    let scalar_bytes = if tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        std::mem::size_of::<f32>()
    } else {
        std::mem::size_of::<f64>()
    };
    let logical_bytes = jet_compute_tensor_values(tensor)
        .len()
        .checked_mul(scalar_bytes)
        .and_then(|bytes| i64::try_from(bytes).ok())
        .ok_or_else(|| JetComputeError::Device("transfer byte count overflow".to_string()))?;
    let expected_bytes = if receipt.from == receipt.to {
        0
    } else {
        logical_bytes
    };
    if receipt.bytes != expected_bytes
        || (receipt.from == receipt.to && receipt.fallback != "none")
        || (receipt.from != receipt.to && receipt.fallback == "none")
    {
        return Err(JetComputeError::Device(
            "transfer receipt does not match the selected backend operation".to_string(),
        ));
    }
    Ok(())
}

impl JetShow for JetComputeStream {
    fn jet_show(&self) -> String {
        // Stream identity is runtime-local; exposing it would make AOT/JIT
        // output differ despite identical compute semantics.
        format!("ComputeStream(device={})", self.device.jet_show())
    }
}

impl JetShow for JetComputeTransferReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Transfer(from={}, to={}, bytes={}, fallback={})",
            self.from.jet_show(),
            self.to.jet_show(),
            self.bytes,
            self.fallback
        )
    }
}

fn jet_compute_stream_new() -> JetComputeStream {
    jet_compute_stream_new_on_device(JetComputeDevice::Cpu)
        .unwrap_or_else(|error| jet_panic("Compute.rs", line!(), &error.jet_show()))
}

fn jet_compute_stream_new_on_device(
    requested: JetComputeDevice,
) -> Result<JetComputeStream, JetComputeError> {
    let receipt = jet_compute_place_with_profile(requested, CPU_ORACLE_F32_PROFILE)?;
    static NEXT_STREAM_ID: std::sync::atomic::AtomicI64 =
        std::sync::atomic::AtomicI64::new(1);
    let cuda = if receipt.selected == JetComputeDevice::Cuda {
        Some(std::sync::Arc::new(jet_compute_cuda::stream_new()?))
    } else {
        None
    };
    Ok(JetComputeStream {
        id: NEXT_STREAM_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        device: receipt.selected,
        cuda,
    })
}

fn jet_compute_stream_sync(stream: &JetComputeStream) -> Result<(), JetComputeError> {
    if stream.id <= 0 {
        return Err(JetComputeError::Device(
            "cannot synchronize an invalid compute stream".to_string(),
        ));
    }
    match stream.device {
        JetComputeDevice::Metal if !jet_compute_metal::available() => {
            return Err(JetComputeError::Device(
                "Metal device was lost before stream synchronization".to_string(),
            ));
        }
        JetComputeDevice::Cuda => {
            let Some(cuda) = &stream.cuda else {
                return Err(JetComputeError::Device(
                    "CUDA stream has no live driver handle".to_string(),
                ));
            };
            jet_compute_cuda::stream_sync(cuda)?;
        }
        JetComputeDevice::Vulkan if !jet_compute_vulkan::available() => {
            return Err(JetComputeError::Device(
                "Vulkan device was lost before stream synchronization".to_string(),
            ));
        }
        JetComputeDevice::WebGpu if !jet_compute_webgpu::available() => {
            return Err(JetComputeError::Device(
                "WebGPU device was lost before stream synchronization".to_string(),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn jet_compute_stream_show(stream: &JetComputeStream) -> String {
    stream.jet_show()
}

fn jet_compute_transfer(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let scalar_bytes = if tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        std::mem::size_of::<f32>()
    } else {
        std::mem::size_of::<f64>()
    };
    let logical_byte_count = jet_compute_tensor_values(tensor)
        .len()
        .checked_mul(scalar_bytes)
        .and_then(|bytes| i64::try_from(bytes).ok())
        .ok_or_else(|| JetComputeError::Device("transfer byte count overflow".to_string()))?;
    let from = tensor.device;
    let mut out = jet_compute_on_device(tensor, device)?;
    if jet_compute_is_accelerator(from) && out.device == JetComputeDevice::Cpu {
        let values = jet_compute_accelerator_values(tensor, "download")?;
        let values = match from {
            JetComputeDevice::Metal => jet_compute_metal::copy(&values)?,
            JetComputeDevice::Cuda => jet_compute_cuda::copy(&values)?,
            JetComputeDevice::Vulkan => jet_compute_vulkan::copy(&values)?,
            JetComputeDevice::WebGpu => jet_compute_webgpu::copy(&values)?,
            _ => {
                return Err(JetComputeError::Device(
                    "download source is not an accelerator".to_string(),
                ))
            }
        };
        out.strides = jet_compute_row_major_strides(&out.shape)?;
        out.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
        jet_compute_validate_tensor(&out)?;
    }
    let (bytes, transfer_kind) = if from == out.device {
        (0, "no-op; same backend and allocation".to_string())
    } else {
        (logical_byte_count, "copy; selected backend".to_string())
    };
    out.last_placement.reason = format!(
        "transfer kind={transfer_kind} bytes={bytes} from={} to={}",
        from.jet_show(),
        out.device.jet_show()
    );
    let receipt = JetComputeTransferReceipt {
        from,
        to: out.device,
        bytes,
        fallback: if from == out.device {
            "none".to_string()
        } else {
            "not-applicable".to_string()
        },
    };
    jet_compute_validate_transfer_receipt(&out, &receipt)?;
    out.last_transfer = Some(receipt);
    Ok(out)
}

fn jet_compute_transfer_show(tensor: &JetTensor) -> String {
    tensor
        .last_transfer
        .as_ref()
        .map_or_else(|| tensor.last_placement.jet_show(), |receipt| receipt.jet_show())
}

// ── #1139 / #1140: safe kernel bounds + typed raw-kernel boundary ────────────

fn jet_compute_kernel_bounds_ok(
    shape: &[i64],
    indices: &[i64],
) -> Result<bool, JetComputeError> {
    if shape.len() != indices.len() {
        return Err(JetComputeError::RankMismatch(
            "kernel index rank must match tensor shape".to_string(),
        ));
    }
    jet_compute_storage_len(shape)?;
    if shape.iter().any(|dim| *dim < 0) {
        return Err(JetComputeError::InvalidShape(
            "kernel shape axes must be non-negative".to_string(),
        ));
    }
    for (i, (&idx, &dim)) in indices.iter().zip(shape.iter()).enumerate() {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "kernel index {idx} out of bounds for axis {i} (extent {dim})"
            )));
        }
    }
    Ok(true)
}

// ── #1141 / D-COMPUTE-AUTODIFF1: reverse-mode VJP + JVP for dense ops ────────

/// Reverse-mode broadcast rule: axes introduced by broadcasting and axes with
/// extent one are summed back into the operand's original shape.
fn jet_compute_reduce_to_shape(
    tensor: &JetTensor,
    target_shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if target_shape.is_empty() || target_shape.len() > tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(
            "gradient target must be a ranked tensor with no greater rank".to_string(),
        ));
    }
    if jet_compute_broadcast_shape(target_shape, &tensor.shape)? != tensor.shape {
        return Err(JetComputeError::RankMismatch(format!(
            "gradient shape {:?} is not broadcast-compatible with {:?}",
            target_shape, tensor.shape
        )));
    }
    let f32_profile = tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let mut out = jet_compute_tensor_from_shape_like(tensor, target_shape.to_vec(), 0.0)?;
    let rank_delta = tensor.shape.len() - target_shape.len();
    let values = jet_compute_tensor_values(tensor);
    if jet_compute_is_accelerator(tensor.device) {
        let output_len = jet_compute_storage_len(target_shape)?;
        let mut buckets = vec![Vec::<f32>::new(); output_len];
        for (flat, value) in values.iter().enumerate() {
            let mut rem = flat as i64;
            let mut output_coords = vec![0i64; tensor.shape.len()];
            for axis in (0..tensor.shape.len()).rev() {
                let dim = tensor.shape[axis];
                output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
                rem = if dim == 0 { 0 } else { rem / dim };
            }
            let mut target_coords = vec![0i64; target_shape.len()];
            for axis in 0..target_shape.len() {
                let source_axis = axis + rank_delta;
                target_coords[axis] = if target_shape[axis] == 1 {
                    0
                } else {
                    output_coords[source_axis]
                };
            }
            let target_offset = jet_compute_offset(&out, &target_coords)?;
            buckets[target_offset].push(jet_compute_f32_value(
                *value,
                "accelerator gradient accumulation",
            )?);
        }
        let mut sums = Vec::with_capacity(output_len);
        for bucket in buckets {
            let sum = if bucket.is_empty() {
                0.0
            } else {
                jet_compute_accelerator_sum(tensor.device, &bucket)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        JetComputeError::Device(format!(
                            "{} gradient reduction returned no value",
                            tensor.device.jet_show()
                        ))
                    })?
            };
            sums.push(sum);
        }
        let Some(storage) = std::sync::Arc::get_mut(&mut out.data) else {
            return Err(JetComputeError::Unsupported(
                "accelerator gradient accumulation requires exclusive output storage".to_string(),
            ));
        };
        for (slot, sum) in storage.iter_mut().zip(sums) {
            *slot = f64::from(sum);
        }
        return jet_compute_record(
            out,
            &[tensor],
            vec![tensor.clone()],
            JetComputeTapeRule::ReduceToShape {
                source_shape: tensor.shape.clone(),
            },
        );
    }
    for flat in 0..values.len() {
        let mut rem = flat as i64;
        let mut output_coords = vec![0i64; tensor.shape.len()];
        for axis in (0..tensor.shape.len()).rev() {
            let dim = tensor.shape[axis];
            output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let mut target_coords = vec![0i64; target_shape.len()];
        for axis in 0..target_shape.len() {
            let source_axis = axis + rank_delta;
            target_coords[axis] = if target_shape[axis] == 1 {
                0
            } else {
                output_coords[source_axis]
            };
        }
        let target_offset = jet_compute_offset(&out, &target_coords)?;
        let Some(data) = std::sync::Arc::get_mut(&mut out.data) else {
            return Err(JetComputeError::Unsupported(
                "gradient accumulation requires exclusive output storage".to_string(),
            ));
        };
        let Some(slot) = data.get_mut(target_offset) else {
            return Err(JetComputeError::OutOfBounds(
                "gradient accumulation is outside storage".to_string(),
            ));
        };
        *slot = if f32_profile {
            let previous = jet_compute_f32_value(*slot, "gradient accumulation")?;
            let value = jet_compute_f32_value(values[flat], "gradient accumulation")?;
            f64::from(previous + value)
        } else {
            *slot + values[flat]
        };
        if !slot.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "gradient accumulation produced a non-finite value".to_string(),
            ));
        }
    }
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::ReduceToShape {
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_zero_like(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    Ok(jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(tensor.shape.clone(), 0.0, JetComputeDevice::Cpu)?,
        tensor,
    ))
}

fn jet_compute_tensor_from_values_like(
    template: &JetTensor,
    values: &[f64],
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&template.shape)?;
    if values.len() != expected || values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "autodiff values do not match the Tensor shape".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape(
        template.shape.clone(),
        0.0,
        JetComputeDevice::Cpu,
    )?;
    let Some(storage) = std::sync::Arc::get_mut(&mut output.data) else {
        return Err(JetComputeError::Unsupported(
            "autodiff output requires exclusive storage".to_string(),
        ));
    };
    storage.clone_from_slice(values);
    let output = jet_compute_inherit_placement(output, template);
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_unary_vjp(
    op: &str,
    input: &JetTensor,
    output: &JetTensor,
    cot: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(input)?;
    jet_compute_validate_tensor(output)?;
    jet_compute_validate_tensor(cot)?;
    if input.shape != output.shape || output.shape != cot.shape {
        return Err(JetComputeError::RankMismatch(
            "unary cotangent shape must equal the unary output".to_string(),
        ));
    }
    jet_compute_require_same_contract(input, output, "unary cotangent")?;
    jet_compute_require_same_contract(output, cot, "unary cotangent")?;
    let input_values = jet_compute_tensor_values(input);
    match op {
        "negate" => jet_compute_unary("negate", cot),
        "abs" => {
            let signs = input_values
                .iter()
                .map(|value| {
                    if *value > 0.0 {
                        1.0
                    } else if *value < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                })
                .collect::<Vec<_>>();
            let signs = jet_compute_tensor_from_values_like(input, &signs)?;
            jet_compute_binary("mul", &signs, cot)
        }
        "exp" => jet_compute_binary("mul", output, cot),
        "log" => jet_compute_binary("div", cot, input),
        "sqrt" => {
            let two = jet_compute_full(&output.shape, 2.0)?;
            let denominator = jet_compute_binary("mul", &two, output)?;
            jet_compute_binary("div", cot, &denominator)
        }
        _ => Err(JetComputeError::Unsupported(format!(
            "unsupported unary derivative `{op}`"
        ))),
    }
}

fn jet_compute_rule_gradients(
    rule: &JetComputeTapeRule,
    values: &[JetTensor],
    output: &JetTensor,
    cot: &JetTensor,
    active_tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> Result<Vec<JetTensor>, JetComputeError> {
    let cot = jet_compute_remove_trace_level(cot, active_tape);
    match rule {
        JetComputeTapeRule::Add => {
            let a = jet_compute_reduce_to_shape(&cot, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&cot, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Sub => {
            let a = jet_compute_reduce_to_shape(&cot, &values[0].shape)?;
            let negative = jet_compute_unary("negate", &cot)?;
            let b = jet_compute_reduce_to_shape(&negative, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Mul => {
            let a_full = jet_compute_binary("mul", &values[1], &cot)?;
            let b_full = jet_compute_binary("mul", &values[0], &cot)?;
            let a = jet_compute_reduce_to_shape(&a_full, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&b_full, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Div => {
            let a_full = jet_compute_binary("div", &cot, &values[1])?;
            let denominator = jet_compute_binary("mul", &values[1], &values[1])?;
            let numerator = jet_compute_binary("mul", &values[0], &cot)?;
            let b_full = jet_compute_unary(
                "negate",
                &jet_compute_binary("div", &numerator, &denominator)?,
            )?;
            let a = jet_compute_reduce_to_shape(&a_full, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&b_full, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Maximum | JetComputeTapeRule::Minimum => {
            let maximum = matches!(rule, JetComputeTapeRule::Maximum);
            let output_values = jet_compute_tensor_values(output);
            let left_value = if values[0].shape == output.shape {
                values[0].clone()
            } else {
                jet_compute_materialize_broadcast(&values[0], &output.shape)?
            };
            let right_value = if values[1].shape == output.shape {
                values[1].clone()
            } else {
                jet_compute_materialize_broadcast(&values[1], &output.shape)?
            };
            let left_values = jet_compute_tensor_values(&left_value);
            let right_values = jet_compute_tensor_values(&right_value);
            let mut left_mask = Vec::with_capacity(output_values.len());
            let mut right_mask = Vec::with_capacity(output_values.len());
            for ((output, a), b) in output_values
                .iter()
                .zip(left_values.iter())
                .zip(right_values.iter())
            {
                if *a == *b {
                    return Err(JetComputeError::Unsupported(
                        "maximum/minimum has no derivative at a tie".to_string(),
                    ));
                }
                let left_slot = if (maximum && *a == *output) || (!maximum && *a == *output) {
                    1.0
                } else {
                    0.0
                };
                let right_slot = if (maximum && *b == *output) || (!maximum && *b == *output) {
                    1.0
                } else {
                    0.0
                };
                left_mask.push(left_slot);
                right_mask.push(right_slot);
            }
            let left_mask = jet_compute_tensor_from_values_like(output, &left_mask)?;
            let right_mask = jet_compute_tensor_from_values_like(output, &right_mask)?;
            let left = jet_compute_binary("mul", &left_mask, &cot)?;
            let right = jet_compute_binary("mul", &right_mask, &cot)?;
            Ok(vec![
                jet_compute_reduce_to_shape(&left, &values[0].shape)?,
                jet_compute_reduce_to_shape(&right, &values[1].shape)?,
            ])
        }
        JetComputeTapeRule::Matmul => {
            let (a, b) = jet_compute_vjp_matmul(&values[0], &values[1], &cot)?;
            Ok(vec![a, b])
        }
        JetComputeTapeRule::MatmulF32Tile => {
            let (a, b) = jet_compute_vjp_matmul_f32_tile(&values[0], &values[1], &cot)?;
            Ok(vec![a, b])
        }
        JetComputeTapeRule::MseLoss => Ok(vec![
            jet_compute_mse_vjp(&values[0], &values[1], &cot, true)?,
            jet_compute_mse_vjp(&values[0], &values[1], &cot, false)?,
        ]),
        JetComputeTapeRule::SgdStep { learning_rate } => {
            let (parameter, gradient) = jet_compute_sgd_vjp(
                &values[0],
                &values[1],
                &cot,
                *learning_rate,
            )?;
            Ok(vec![parameter, gradient])
        }
        JetComputeTapeRule::Unary(op) => Ok(vec![jet_compute_unary_vjp(
            op,
            &values[0],
            output,
            &cot,
        )?]),
        JetComputeTapeRule::Reshape { source_shape } => Ok(vec![jet_compute_reshape(
            &cot,
            &source_shape.clone(),
        )?]),
        JetComputeTapeRule::Broadcast { source_shape } => Ok(vec![
            jet_compute_reduce_to_shape(&cot, source_shape)?,
        ]),
        JetComputeTapeRule::ReduceToShape { source_shape } => Ok(vec![
            jet_compute_broadcast_to(&cot, source_shape)?,
        ]),
        JetComputeTapeRule::Transpose => Ok(vec![jet_compute_transpose(&cot)?]),
        JetComputeTapeRule::SumAxis { axis, source_shape } => {
            let mut reduced_shape = source_shape.clone();
            reduced_shape[*axis] = 1;
            let cot = jet_compute_reshape(&cot, &reduced_shape)?;
            Ok(vec![jet_compute_broadcast_to(&cot, source_shape)?])
        }
    }
}

fn jet_compute_reverse(
    state: &JetComputeVjpState,
    seed: &JetTensor,
) -> Result<Vec<JetTensor>, JetComputeError> {
    jet_compute_validate_tensor(&state.value)?;
    jet_compute_validate_tensor(seed)?;
    if state.value.shape != seed.shape {
        return Err(JetComputeError::RankMismatch(
            "VJP seed shape must equal the function output shape".to_string(),
        ));
    }
    jet_compute_require_same_contract(&state.value, seed, "VJP seed")?;
    let (nodes, inputs) = {
        let tape = state
            .tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        (tape.nodes.clone(), tape.inputs.clone())
    };
    let mut cotangents: Vec<Option<JetTensor>> = vec![None; nodes.len()];
    if let Some(output_node) = state.output_node {
        let Some(slot) = cotangents.get_mut(output_node) else {
            return Err(JetComputeError::Unsupported(
                "VJP output node is outside its tape".to_string(),
            ));
        };
        *slot = Some(jet_compute_untracked(seed));
    }
    // The input nodes are leaves. Keep their accumulated cotangents in place
    // for the final result; only reverse-propagate through operation nodes.
    for index in (inputs.len()..nodes.len()).rev() {
        let Some(cot) = cotangents[index].take() else {
            continue;
        };
        let node = &nodes[index];
        let Some(rule) = &node.rule else {
            continue;
        };
        let gradients = jet_compute_rule_gradients(
            rule,
            &node.values,
            &node.output,
            &cot,
            &state.tape,
        )?;
        for (parent, gradient) in node.parents.iter().zip(gradients) {
            let Some(parent) = parent else {
                continue;
            };
            let gradient = jet_compute_remove_trace_level(&gradient, &state.tape);
            let Some(slot) = cotangents.get_mut(*parent) else {
                return Err(JetComputeError::Unsupported(
                    "VJP parent node is outside its tape".to_string(),
                ));
            };
            *slot = Some(match slot.take() {
                Some(previous) => jet_compute_binary("add", &previous, &gradient)?,
                None => gradient,
            });
        }
    }
    let mut result = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let gradient = cotangents
            .get(index)
            .and_then(Option::as_ref)
            .cloned()
            .unwrap_or(jet_compute_zero_like(input)?);
        result.push(jet_compute_inherit_placement(gradient, input));
    }
    Ok(result)
}

fn jet_compute_select_gradients(
    all: Vec<JetTensor>,
    targets: &[i64],
) -> Result<Vec<JetTensor>, JetComputeError> {
    targets
        .iter()
        .map(|target| {
            let index = usize::try_from(*target).map_err(|_| {
                JetComputeError::Unsupported("negative autodiff target index".to_string())
            })?;
            all.get(index).cloned().ok_or_else(|| {
                JetComputeError::Unsupported("autodiff target index is outside the function signature".to_string())
            })
        })
        .collect()
}

fn jet_compute_gradient_seed(state: &JetComputeVjpState) -> Result<JetTensor, JetComputeError> {
    if jet_compute_tensor_numel(&state.value) != 1 {
        return Err(JetComputeError::RankMismatch(
            "compute.gradient requires a scalar Tensor output".to_string(),
        ));
    }
    Ok(jet_compute_inherit_placement(
        jet_compute_ones(&state.value.shape)?,
        &state.value,
    ))
}

fn jet_compute_vjp_pull(
    state: &JetComputeVjpState,
    seed: &JetTensor,
    targets: &[i64],
) -> Result<Vec<JetTensor>, JetComputeError> {
    jet_compute_select_gradients(jet_compute_reverse(state, seed)?, targets)
}

fn jet_compute_vjp_pull_or_panic(
    state: &JetComputeVjpState,
    seed: &JetTensor,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    match jet_compute_vjp_pull(state, seed, targets) {
        Ok(values) => values,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_gradient_or_panic(
    state: &JetComputeVjpState,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    let seed = match jet_compute_gradient_seed(state) {
        Ok(seed) => seed,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    };
    jet_compute_vjp_pull_or_panic(state, &seed, targets, context)
}

fn jet_compute_vjp_unit_grads_or_panic(
    state: &JetComputeVjpState,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    jet_compute_gradient_or_panic(state, targets, context)
}

/// The one transform dispatcher used by AOT and the interpreter.  Engines
/// marshal callable arguments and package the typed result; this function
/// owns transform selection, scalar seeding, value detachment, and lazy VJP
/// state creation.
fn jet_compute_transform(
    method: &str,
    state: &JetComputeVjpState,
    tangents: &[JetTensor],
    targets: &[i64],
) -> Result<JetComputeTransformResult, JetComputeError> {
    let value = jet_compute_remove_trace_level(&state.value, &state.tape);
    match method {
        "gradient" => Ok(JetComputeTransformResult::Gradient(
            jet_compute_vjp_pull(state, &jet_compute_gradient_seed(state)?, targets)?,
        )),
        "value_and_gradient" => Ok(JetComputeTransformResult::ValueAndGradient {
            value,
            gradients: jet_compute_vjp_pull(state, &jet_compute_gradient_seed(state)?, targets)?,
        }),
        "vjp" => Ok(JetComputeTransformResult::Vjp {
            value,
            state: state.clone(),
        }),
        "jvp" => Ok(JetComputeTransformResult::Jvp {
            value,
            tangent: jet_compute_jvp(state, tangents.to_vec())?,
        }),
        _ => Err(JetComputeError::Unsupported(format!(
            "unknown autodiff transform `{method}`"
        ))),
    }
}

fn jet_compute_transform_or_panic(
    method: &str,
    state: &JetComputeVjpState,
    tangents: &[JetTensor],
    targets: &[i64],
    context: &str,
) -> JetComputeTransformResult {
    match jet_compute_transform(method, state, tangents, targets) {
        Ok(result) => result,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_nested_gradient(
    states: &[JetComputeVjpState],
    targets: &[i64],
) -> Result<Vec<Vec<JetTensor>>, JetComputeError> {
    states
        .iter()
        .map(|state| {
            let result = jet_compute_transform("gradient", state, &[], targets)?;
            let JetComputeTransformResult::Gradient(values) = result else {
                return Err(JetComputeError::Unsupported(
                    "nested gradient did not return gradients".to_string(),
                ));
            };
            Ok(values)
        })
        .collect()
}

fn jet_compute_nested_gradient_or_panic(
    states: &[JetComputeVjpState],
    targets: &[i64],
    context: &str,
) -> Vec<Vec<JetTensor>> {
    match jet_compute_nested_gradient(states, targets) {
        Ok(values) => values,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_jvp_rule(
    rule: &JetComputeTapeRule,
    values: &[JetTensor],
    output: &JetTensor,
    tangents: &[JetTensor],
) -> Result<JetTensor, JetComputeError> {
    match rule {
        JetComputeTapeRule::Add => jet_compute_binary("add", &tangents[0], &tangents[1]),
        JetComputeTapeRule::Sub => jet_compute_binary("sub", &tangents[0], &tangents[1]),
        JetComputeTapeRule::Mul => {
            let left = jet_compute_binary("mul", &tangents[0], &values[1])?;
            let right = jet_compute_binary("mul", &values[0], &tangents[1])?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Div => {
            let left = jet_compute_binary("div", &tangents[0], &values[1])?;
            let numerator = jet_compute_binary("mul", &values[0], &tangents[1])?;
            let denominator = jet_compute_binary("mul", &values[1], &values[1])?;
            let right = jet_compute_binary("div", &numerator, &denominator)?;
            jet_compute_binary("sub", &left, &right)
        }
        JetComputeTapeRule::Maximum | JetComputeTapeRule::Minimum => {
            let maximum = matches!(rule, JetComputeTapeRule::Maximum);
            let output_values = jet_compute_tensor_values(output);
            let left_value = if values[0].shape == output.shape {
                values[0].clone()
            } else {
                jet_compute_materialize_broadcast(&values[0], &output.shape)?
            };
            let right_value = if values[1].shape == output.shape {
                values[1].clone()
            } else {
                jet_compute_materialize_broadcast(&values[1], &output.shape)?
            };
            let left_tangent = if tangents[0].shape == output.shape {
                tangents[0].clone()
            } else {
                jet_compute_broadcast_to(&tangents[0], &output.shape.to_vec())?
            };
            let right_tangent = if tangents[1].shape == output.shape {
                tangents[1].clone()
            } else {
                jet_compute_broadcast_to(&tangents[1], &output.shape.to_vec())?
            };
            let left_values = jet_compute_tensor_values(&left_value);
            let right_values = jet_compute_tensor_values(&right_value);
            let mut left_mask = Vec::with_capacity(output_values.len());
            let mut right_mask = Vec::with_capacity(output_values.len());
            for ((output, a), b) in output_values
                .iter()
                .zip(left_values.iter())
                .zip(right_values.iter())
            {
                if *a == *b {
                    return Err(JetComputeError::Unsupported(
                        "maximum/minimum has no JVP at a tie".to_string(),
                    ));
                }
                if (maximum && *a == *output) || (!maximum && *a == *output) {
                    left_mask.push(1.0);
                    right_mask.push(0.0);
                } else {
                    left_mask.push(0.0);
                    right_mask.push(1.0);
                }
            }
            let left_mask = jet_compute_tensor_from_values_like(output, &left_mask)?;
            let right_mask = jet_compute_tensor_from_values_like(output, &right_mask)?;
            let left = jet_compute_binary("mul", &left_mask, &left_tangent)?;
            let right = jet_compute_binary("mul", &right_mask, &right_tangent)?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Matmul => {
            let left = jet_compute_matmul(&tangents[0], &values[1])?;
            let right = jet_compute_matmul(&values[0], &tangents[1])?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::MatmulF32Tile => jet_compute_jvp_matmul_f32_tile(
            &values[0],
            &values[1],
            &tangents[0],
            &tangents[1],
        ),
        JetComputeTapeRule::MseLoss => jet_compute_mse_jvp(
            &values[0],
            &values[1],
            &tangents[0],
            &tangents[1],
        ),
        JetComputeTapeRule::SgdStep { learning_rate } => jet_compute_sgd_step(
            &tangents[0],
            &tangents[1],
            *learning_rate,
        ),
        JetComputeTapeRule::Unary(op) => jet_compute_unary_vjp(
            op,
            &values[0],
            output,
            &tangents[0],
        ),
        JetComputeTapeRule::Reshape { .. } => {
            jet_compute_reshape(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::Broadcast { .. } => {
            jet_compute_broadcast_to(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::ReduceToShape { .. } => {
            jet_compute_reduce_to_shape(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::Transpose => jet_compute_transpose(&tangents[0]),
        JetComputeTapeRule::SumAxis { axis, .. } => jet_compute_sum_axis(&tangents[0], *axis as i64),
    }
}

fn jet_compute_jvp(
    state: &JetComputeVjpState,
    input_tangents: Vec<JetTensor>,
) -> Result<JetTensor, JetComputeError> {
    let (nodes, inputs) = {
        let tape = state
            .tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        (tape.nodes.clone(), tape.inputs.clone())
    };
    if input_tangents.len() != inputs.len() {
        return Err(JetComputeError::RankMismatch(
            "JVP tangent count must equal the function input count".to_string(),
        ));
    }
    for (input, tangent) in inputs.iter().zip(input_tangents.iter()) {
        if input.shape != tangent.shape {
            return Err(JetComputeError::RankMismatch(
                "JVP tangent shapes must match their primal inputs".to_string(),
            ));
        }
        jet_compute_require_same_contract(input, tangent, "JVP tangent")?;
    }
    let mut tangents: Vec<Option<JetTensor>> = vec![None; nodes.len()];
    for (index, tangent) in input_tangents.into_iter().enumerate() {
        if let Some(slot) = tangents.get_mut(index) {
            *slot = Some(jet_compute_remove_trace_level(&tangent, &state.tape));
        }
    }
    for (index, node) in nodes.iter().enumerate().skip(inputs.len()) {
        let Some(rule) = &node.rule else {
            continue;
        };
        let mut node_tangents = Vec::with_capacity(node.parents.len());
        for (parent, value) in node.parents.iter().zip(node.values.iter()) {
            node_tangents.push(
                parent
                    .and_then(|parent| tangents.get(parent).and_then(Option::as_ref).cloned())
                    .unwrap_or(jet_compute_zero_like(value)?),
            );
        }
        tangents[index] = Some(jet_compute_jvp_rule(
            rule,
            &node.values,
            &node.output,
            &node_tangents,
        )?);
    }
    match state.output_node {
        Some(node) => tangents
            .get(node)
            .and_then(Option::clone)
            .ok_or_else(|| JetComputeError::Unsupported("JVP output tangent is unavailable".to_string())),
        None => jet_compute_zero_like(&state.value),
    }
}

fn jet_compute_jvp_or_panic(
    state: &JetComputeVjpState,
    input_tangents: Vec<JetTensor>,
    context: &str,
) -> JetTensor {
    match jet_compute_jvp(state, input_tangents) {
        Ok(value) => value,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_vjp_matmul(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(cot)?;
    if a.shape.len() != 2
        || b.shape.len() != 2
        || cot.shape.len() != 2
        || a.shape[1] != b.shape[0]
        || cot.shape[0] != a.shape[0]
        || cot.shape[1] != b.shape[1]
    {
        return Err(JetComputeError::RankMismatch(
            "matmul cotangent shape must equal the matmul output".to_string(),
        ));
    }
    jet_compute_require_same_contract(a, b, "matmul cotangent")?;
    jet_compute_require_same_contract(a, cot, "matmul cotangent")?;
    let b_t = jet_compute_transpose(b)?;
    let a_t = jet_compute_transpose(a)?;
    Ok((
        jet_compute_inherit_placement(jet_compute_matmul(cot, &b_t)?, a),
        jet_compute_inherit_placement(jet_compute_matmul(&a_t, cot)?, b),
    ))
}

fn jet_compute_f32_projection(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "f32 autodiff input"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut projected = jet_compute_tensor_from_shape_like(tensor, tensor.shape.clone(), 0.0)?;
    projected.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
    if !jet_compute_is_accelerator(projected.device) {
        projected.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
        projected.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
            .iter()
            .map(|ability| (*ability).to_string())
            .collect();
        projected.last_placement.reason = "autodiff f32 projection".to_string();
    }
    jet_compute_validate_tensor(&projected)?;
    Ok(projected)
}

fn jet_compute_vjp_matmul_f32_tile(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(cot)?;
    if a.shape.len() != 2
        || b.shape.len() != 2
        || cot.shape.len() != 2
        || a.shape[1] != b.shape[0]
        || cot.shape[0] != a.shape[0]
        || cot.shape[1] != b.shape[1]
    {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile cotangent shape must equal the matmul output".to_string(),
        ));
    }
    if a.device != b.device || a.device != cot.device {
        return Err(JetComputeError::Device(
            "matmul_f32_tile cotangent devices must match the inputs".to_string(),
        ));
    }
    let a32 = jet_compute_f32_projection(a)?;
    let b32 = jet_compute_f32_projection(b)?;
    let cot32 = jet_compute_f32_projection(cot)?;
    let b_transposed = jet_compute_transpose(&b32)?;
    let a_transposed = jet_compute_transpose(&a32)?;
    let a_gradient = jet_compute_matmul_f32_tile(&cot32, &b_transposed)?;
    let b_gradient = jet_compute_matmul_f32_tile(&a_transposed, &cot32)?;
    Ok((
        jet_compute_tensor_from_values_like(a, &jet_compute_tensor_values(&a_gradient))?,
        jet_compute_tensor_from_values_like(b, &jet_compute_tensor_values(&b_gradient))?,
    ))
}

fn jet_compute_jvp_matmul_f32_tile(
    a: &JetTensor,
    b: &JetTensor,
    a_tangent: &JetTensor,
    b_tangent: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(a_tangent)?;
    jet_compute_validate_tensor(b_tangent)?;
    if a.shape != a_tangent.shape || b.shape != b_tangent.shape {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile tangent shapes must match the inputs".to_string(),
        ));
    }
    if a.device != b.device
        || a.device != a_tangent.device
        || b.device != b_tangent.device
    {
        return Err(JetComputeError::Device(
            "matmul_f32_tile tangent devices must match the inputs".to_string(),
        ));
    }
    let a32 = jet_compute_f32_projection(a)?;
    let b32 = jet_compute_f32_projection(b)?;
    let a_tangent32 = jet_compute_f32_projection(a_tangent)?;
    let b_tangent32 = jet_compute_f32_projection(b_tangent)?;
    let left = jet_compute_matmul_f32_tile(&a_tangent32, &b32)?;
    let right = jet_compute_matmul_f32_tile(&a32, &b_tangent32)?;
    jet_compute_binary("add", &left, &right)
}

fn jet_compute_f32_value(value: f64, context: &str) -> Result<f32, JetComputeError> {
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return Err(JetComputeError::Arithmetic(format!(
            "{context} is outside the finite F32 range"
        )));
    }
    Ok(narrowed)
}

fn jet_compute_validate_profile_value(
    profile: &str,
    value: f64,
    context: &str,
) -> Result<(), JetComputeError> {
    if profile == CPU_ORACLE_F32_PROFILE {
        let narrowed = jet_compute_f32_value(value, context)?;
        if f64::from(narrowed) != value {
            return Err(JetComputeError::Arithmetic(format!(
                "{context} is not canonical for the F32 precision profile"
            )));
        }
    }
    Ok(())
}

fn jet_compute_validate_serialized_profile_values(
    profile: &str,
    values: &[f64],
) -> Result<(), JetComputeError> {
    if profile == CPU_ORACLE_F32_PROFILE {
        for value in values {
            let narrowed = jet_compute_f32_value(*value, "serialized Tensor value")?;
            if f64::from(narrowed) != *value {
                return Err(JetComputeError::Serialization(
                    "serialized Tensor value is not canonical for its F32 profile".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn jet_compute_wire_checksum(body: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in body.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    format!("{hash:016x}")
}

// ── #1142: ML training/inference + model serialization over the Tensor oracle ─

fn jet_compute_validate_mse_inputs(
    pred: &JetTensor,
    target: &JetTensor,
) -> Result<(Vec<f64>, Vec<f64>), JetComputeError> {
    jet_compute_validate_tensor(pred)?;
    jet_compute_validate_tensor(target)?;
    if pred.shape != target.shape {
        return Err(JetComputeError::RankMismatch(
            "mse_loss prediction and target shapes must match".to_string(),
        ));
    }
    if pred.device != target.device {
        return Err(JetComputeError::Device(
            "mse_loss prediction and target devices must match".to_string(),
        ));
    }
    if pred.last_placement.profile != target.last_placement.profile {
        return Err(JetComputeError::Device(
            "mse_loss prediction and target precision profiles must match".to_string(),
        ));
    }
    let pred_values = jet_compute_tensor_values(pred);
    let target_values = jet_compute_tensor_values(target);
    if pred_values.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "mse_loss requires a non-empty tensor".to_string(),
        ));
    }
    Ok((pred_values, target_values))
}

fn jet_compute_scalar_from_like(
    template: &JetTensor,
    value: f64,
) -> Result<JetTensor, JetComputeError> {
    if !value.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "compute scalar produced a non-finite value".to_string(),
        ));
    }
    let output = jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(vec![1], value, JetComputeDevice::Cpu)?,
        template,
    );
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_mse_value(
    pred: &JetTensor,
    pred_values: &[f64],
    target_values: &[f64],
) -> Result<f64, JetComputeError> {
    let loss = if pred.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let mut sum = 0.0_f32;
        for (pred_value, target_value) in pred_values.iter().zip(target_values.iter()) {
            let pred_value = jet_compute_f32_value(*pred_value, "mse_loss prediction")?;
            let target_value = jet_compute_f32_value(*target_value, "mse_loss target")?;
            let difference = pred_value - target_value;
            let next = sum + difference * difference;
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        f64::from(sum / pred_values.len() as f32)
    } else {
        let mut sum = 0.0_f64;
        for (pred_value, target_value) in pred_values.iter().zip(target_values.iter()) {
            let difference = *pred_value - *target_value;
            let next = sum + difference * difference;
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        sum / pred_values.len() as f64
    };
    if !loss.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "mse_loss produced a non-finite value".to_string(),
        ));
    }
    Ok(loss)
}

/// MSE is a scalar Tensor operation, not a host float reduction. Recording it
/// here keeps eager loss, transformed loss, and all execution tiers on one
/// VJP/JVP rule.
fn jet_compute_mse_loss(
    pred: &JetTensor,
    target: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    let (pred_values, target_values) = jet_compute_validate_mse_inputs(pred, target)?;
    if jet_compute_is_accelerator(pred.device) {
        let pred_values = jet_compute_accelerator_values(pred, "MSE input")?;
        let target_values = jet_compute_accelerator_values(target, "MSE input")?;
        let loss = jet_compute_accelerator_mse(pred.device, &pred_values, &target_values)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                JetComputeError::Device(format!("{} MSE returned no value", pred.device.jet_show()))
            })?;
        return jet_compute_record(
            jet_compute_scalar_from_like(pred, f64::from(loss))?,
            &[pred, target],
            vec![pred.clone(), target.clone()],
            JetComputeTapeRule::MseLoss,
        );
    }
    let loss = jet_compute_mse_value(pred, &pred_values, &target_values)?;
    jet_compute_record(
        jet_compute_scalar_from_like(pred, loss)?,
        &[pred, target],
        vec![pred.clone(), target.clone()],
        JetComputeTapeRule::MseLoss,
    )
}

fn jet_compute_mse_vjp(
    pred: &JetTensor,
    target: &JetTensor,
    cot: &JetTensor,
    positive: bool,
) -> Result<JetTensor, JetComputeError> {
    let (pred_values, target_values) = jet_compute_validate_mse_inputs(pred, target)?;
    jet_compute_validate_tensor(cot)?;
    if cot.shape != vec![1] {
        return Err(JetComputeError::RankMismatch(
            "mse_loss cotangent must be a scalar Tensor".to_string(),
        ));
    }
    jet_compute_require_same_contract(pred, cot, "mse_loss cotangent")?;
    let cot_value = jet_compute_tensor_values(cot)
        .first()
        .copied()
        .ok_or_else(|| JetComputeError::InvalidShape("mse_loss cotangent is empty".to_string()))?;
    if jet_compute_is_accelerator(pred.device) {
        let pred_values = jet_compute_accelerator_values(pred, "MSE gradient input")?;
        let target_values = jet_compute_accelerator_values(target, "MSE gradient input")?;
        let cot_value = jet_compute_f32_value(cot_value, "mse_loss cotangent")?;
        let data = jet_compute_accelerator_mse_grad(
            pred.device,
            &pred_values,
            &target_values,
            &[cot_value],
            positive,
        )?;
        return jet_compute_tensor_from_values_like(
            pred,
            &data.into_iter().map(f64::from).collect::<Vec<_>>(),
        );
    }
    let data = if pred.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let cot_value = jet_compute_f32_value(cot_value, "mse_loss cotangent")?;
        let factor = 2.0_f32 / pred_values.len() as f32 * cot_value;
        if !factor.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "mse_loss gradient factor is non-finite".to_string(),
            ));
        }
        pred_values
            .iter()
            .zip(target_values.iter())
            .map(|(pred_value, target_value)| {
                let pred_value = jet_compute_f32_value(*pred_value, "mse_loss prediction")?;
                let target_value = jet_compute_f32_value(*target_value, "mse_loss target")?;
                let difference = if positive {
                    pred_value - target_value
                } else {
                    target_value - pred_value
                };
                let value = difference * factor;
                value.is_finite().then_some(f64::from(value)).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "mse_loss gradient produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let factor = 2.0 / pred_values.len() as f64 * cot_value;
        if !factor.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "mse_loss gradient factor is non-finite".to_string(),
            ));
        }
        pred_values
            .iter()
            .zip(target_values.iter())
            .map(|(pred_value, target_value)| {
                let difference = if positive {
                    *pred_value - *target_value
                } else {
                    *target_value - *pred_value
                };
                let value = difference * factor;
                value.is_finite().then_some(value).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "mse_loss gradient produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    jet_compute_tensor_from_values_like(pred, &data)
}

fn jet_compute_mse_jvp(
    pred: &JetTensor,
    target: &JetTensor,
    pred_tangent: &JetTensor,
    target_tangent: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    let (pred_values, target_values) = jet_compute_validate_mse_inputs(pred, target)?;
    jet_compute_validate_tensor(pred_tangent)?;
    jet_compute_validate_tensor(target_tangent)?;
    if pred_tangent.shape != pred.shape || target_tangent.shape != target.shape {
        return Err(JetComputeError::RankMismatch(
            "mse_loss tangent shapes must match their primal tensors".to_string(),
        ));
    }
    if pred_tangent.last_placement.profile != pred.last_placement.profile
        || target_tangent.last_placement.profile != target.last_placement.profile
    {
        return Err(JetComputeError::Device(
            "mse_loss tangent precision profiles must match their primal tensors".to_string(),
        ));
    }
    jet_compute_require_same_contract(pred, pred_tangent, "mse_loss tangent")?;
    jet_compute_require_same_contract(target, target_tangent, "mse_loss tangent")?;
    let pred_tangent_values = jet_compute_tensor_values(pred_tangent);
    let target_tangent_values = jet_compute_tensor_values(target_tangent);
    if jet_compute_is_accelerator(pred.device) {
        let pred_values = jet_compute_accelerator_values(pred, "MSE JVP input")?;
        let target_values = jet_compute_accelerator_values(target, "MSE JVP input")?;
        let pred_tangent_values =
            jet_compute_accelerator_values(pred_tangent, "MSE JVP tangent")?;
        let target_tangent_values =
            jet_compute_accelerator_values(target_tangent, "MSE JVP tangent")?;
        let value = jet_compute_accelerator_mse_jvp(
            pred.device,
            &pred_values,
            &target_values,
            &pred_tangent_values,
            &target_tangent_values,
        )?
            .into_iter()
            .next()
            .ok_or_else(|| {
                JetComputeError::Device(format!("{} MSE JVP returned no value", pred.device.jet_show()))
            })?;
        return jet_compute_scalar_from_like(pred, f64::from(value));
    }
    let value = if pred.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let mut sum = 0.0_f32;
        for (((pred_value, target_value), pred_tangent), target_tangent) in pred_values
            .iter()
            .zip(target_values.iter())
            .zip(pred_tangent_values.iter())
            .zip(target_tangent_values.iter())
        {
            let difference = jet_compute_f32_value(*pred_value, "mse_loss prediction")?
                - jet_compute_f32_value(*target_value, "mse_loss target")?;
            let direction = jet_compute_f32_value(*pred_tangent, "mse_loss prediction tangent")?
                - jet_compute_f32_value(*target_tangent, "mse_loss target tangent")?;
            let next = sum + 2.0_f32 * difference * direction;
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss JVP accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        f64::from(sum / pred_values.len() as f32)
    } else {
        let mut sum = 0.0_f64;
        for (((pred_value, target_value), pred_tangent), target_tangent) in pred_values
            .iter()
            .zip(target_values.iter())
            .zip(pred_tangent_values.iter())
            .zip(target_tangent_values.iter())
        {
            let next = sum
                + 2.0
                    * (*pred_value - *target_value)
                    * (*pred_tangent - *target_tangent);
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss JVP accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        sum / pred_values.len() as f64
    };
    jet_compute_scalar_from_like(pred, value)
}

fn jet_compute_sgd_step(
    param: &JetTensor,
    grad: &JetTensor,
    lr: f64,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(param)?;
    jet_compute_validate_tensor(grad)?;
    if param.shape != grad.shape {
        return Err(JetComputeError::RankMismatch(
            "sgd parameter and gradient shapes must match".to_string(),
        ));
    }
    if param.device != grad.device {
        return Err(JetComputeError::Device(
            "sgd parameter and gradient devices must match".to_string(),
        ));
    }
    if param.last_placement.profile != grad.last_placement.profile {
        return Err(JetComputeError::Device(
            "sgd parameter and gradient precision profiles must match".to_string(),
        ));
    }
    if !lr.is_finite() || lr < 0.0 {
        return Err(JetComputeError::Arithmetic(
            "sgd learning rate must be finite and non-negative".to_string(),
        ));
    }
    if jet_compute_is_accelerator(param.device) {
        let parameter = jet_compute_accelerator_values(param, "SGD input")?;
        let gradient = jet_compute_accelerator_values(grad, "SGD input")?;
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        let data = jet_compute_accelerator_sgd(param.device, &parameter, &gradient, learning_rate)?;
        return jet_compute_record(
            jet_compute_accelerator_result_like(param, param.shape.clone(), data)?,
            &[param, grad],
            vec![param.clone(), grad.clone()],
            JetComputeTapeRule::SgdStep {
                learning_rate: lr,
            },
        );
    }

    let param_values = jet_compute_tensor_values(param);
    let grad_values = jet_compute_tensor_values(grad);
    let data = if param.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        param_values
            .iter()
            .zip(grad_values.iter())
            .map(|(param_value, grad_value)| {
                let param_value = jet_compute_f32_value(*param_value, "sgd parameter")?;
                let grad_value = jet_compute_f32_value(*grad_value, "sgd gradient")?;
                let next = param_value - learning_rate * grad_value;
                next.is_finite().then_some(f64::from(next)).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "sgd_step produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        param_values
            .iter()
            .zip(grad_values.iter())
            .map(|(param_value, grad_value)| {
                let next = *param_value - lr * *grad_value;
                next.is_finite().then_some(next).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "sgd_step produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let next = JetTensor {
        shape: param.shape.clone(),
        strides: jet_compute_row_major_strides(&param.shape)?,
        data: std::sync::Arc::new(data),
        device: param.device,
        last_placement: param.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_validate_tensor(&next)?;
    jet_compute_record(
        next,
        &[param, grad],
        vec![param.clone(), grad.clone()],
        JetComputeTapeRule::SgdStep {
            learning_rate: lr,
        },
    )
}

fn jet_compute_sgd_vjp(
    param: &JetTensor,
    grad: &JetTensor,
    cot: &JetTensor,
    lr: f64,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(param)?;
    jet_compute_validate_tensor(grad)?;
    jet_compute_validate_tensor(cot)?;
    if param.shape != grad.shape || param.shape != cot.shape {
        return Err(JetComputeError::RankMismatch(
            "sgd cotangent shape must equal the parameter and gradient shapes".to_string(),
        ));
    }
    if param.device != grad.device
        || param.device != cot.device
        || param.last_placement.profile != grad.last_placement.profile
        || param.last_placement.profile != cot.last_placement.profile
    {
        return Err(JetComputeError::Device(
            "sgd parameter, gradient, and cotangent devices and profiles must match".to_string(),
        ));
    }
    if !lr.is_finite() || lr < 0.0 {
        return Err(JetComputeError::Arithmetic(
            "sgd learning rate must be finite and non-negative".to_string(),
        ));
    }
    let cot_values = jet_compute_tensor_values(cot);
    if jet_compute_is_accelerator(param.device) {
        let cot_values = jet_compute_accelerator_values(cot, "SGD cotangent")?;
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        let gradients = jet_compute_accelerator_scale(param.device, &cot_values, -learning_rate)?;
        return Ok((
            jet_compute_tensor_from_values_like(
                param,
                &cot_values.into_iter().map(f64::from).collect::<Vec<_>>(),
            )?,
            jet_compute_tensor_from_values_like(
                grad,
                &gradients.into_iter().map(f64::from).collect::<Vec<_>>(),
            )?,
        ));
    }
    let (parameter_values, gradient_values) = if param.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        let mut parameters = Vec::with_capacity(cot_values.len());
        let mut gradients = Vec::with_capacity(cot_values.len());
        for cot_value in cot_values {
            let cot_value = jet_compute_f32_value(cot_value, "sgd cotangent")?;
            let gradient = -learning_rate * cot_value;
            if !gradient.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sgd gradient produced a non-finite value".to_string(),
                ));
            }
            parameters.push(f64::from(cot_value));
            gradients.push(f64::from(gradient));
        }
        (parameters, gradients)
    } else {
        let gradients = cot_values
            .iter()
            .map(|cot_value| {
                let value = -lr * *cot_value;
                value.is_finite().then_some(value).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "sgd gradient produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        (cot_values, gradients)
    };
    Ok((
        jet_compute_tensor_from_values_like(param, &parameter_values)?,
        jet_compute_tensor_from_values_like(grad, &gradient_values)?,
    ))
}

fn jet_compute_serialize(tensor: &JetTensor) -> Result<String, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor serialization does not accept traced tensors".to_string(),
        ));
    }
    if jet_compute_is_accelerator(tensor.device) {
        return Err(JetComputeError::Unsupported(
            "accelerator Tensor serialization requires an explicit transfer to CPU".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    let shape = tensor
        .shape
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let data = values
        .iter()
        // Debug formatting is Rust's shortest round-tripping f64 spelling.
        // Keep it stable across the AOT/JIT/interpreter Prelude boundary.
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(",");
    let profile = tensor.last_placement.profile.as_str();
    jet_compute_validate_serialized_profile_values(profile, &values)?;
    let body = format!("shape={shape};data={data};profile={profile}");
    let checksum = jet_compute_wire_checksum(&body);
    Ok(format!("{body};checksum={checksum}"))
}

fn jet_compute_deserialize(payload: &String) -> Result<JetTensor, JetComputeError> {
    let mut fields = payload.split(';');
    let Some(shape_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    let Some(data_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    let Some(profile_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    let Some(checksum_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    if fields.next().is_some()
        || !data_part.starts_with("data=")
        || !profile_part.starts_with("profile=")
        || !checksum_part.starts_with("checksum=")
    {
        return Err(JetComputeError::Serialization(
            "deserialize contains duplicate or unknown fields".to_string(),
        ));
    }
    let shape_str = shape_part
        .strip_prefix("shape=")
        .ok_or_else(|| JetComputeError::Serialization("missing shape=".to_string()))?;
    if shape_str.is_empty() {
        return Err(JetComputeError::Serialization(
            "serialized Tensor shape cannot be empty".to_string(),
        ));
    }
    let shape: Vec<i64> = shape_str
        .split(',')
        .map(|p| {
            if p.is_empty() {
                return Err(JetComputeError::Serialization(
                    "serialized Tensor shape contains an empty axis".to_string(),
                ));
            }
            let axis = p.parse::<i64>().map_err(|_| {
                JetComputeError::Serialization(format!("bad shape axis `{p}`"))
            })?;
            if axis.to_string() != p {
                return Err(JetComputeError::Serialization(format!(
                    "non-canonical shape axis `{p}`"
                )));
            }
            Ok(axis)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let data_str = data_part.strip_prefix("data=").unwrap_or("");
    let data: Vec<f64> = if data_str.is_empty() {
        Vec::new()
    } else {
        data_str
            .split(',')
            .map(|p| {
                if p.is_empty() {
                    return Err(JetComputeError::Serialization(
                        "serialized Tensor data contains an empty value".to_string(),
                    ));
                }
                let value = p.parse::<f64>().map_err(|_| {
                    JetComputeError::Serialization(format!("bad data value `{p}`"))
                })?;
                if !value.is_finite() {
                    return Err(JetComputeError::Serialization(
                        "serialized Tensor contains a non-finite value".to_string(),
                    ));
                }
                if format!("{value:?}") != p {
                    return Err(JetComputeError::Serialization(format!(
                        "non-canonical data value `{p}`"
                    )));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let profile = profile_part.strip_prefix("profile=").unwrap_or("");
    let Some(abilities) = jet_compute_registered_abilities(profile) else {
        return Err(JetComputeError::Serialization(format!(
            "unsupported Tensor precision profile `{profile}`"
        )));
    };
    jet_compute_validate_serialized_profile_values(profile, &data)?;
    let checksum = checksum_part.strip_prefix("checksum=").unwrap_or("");
    if checksum.len() != 16
        || !checksum
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(JetComputeError::Serialization(
            "serialized Tensor checksum is not canonical".to_string(),
        ));
    }
    let body = format!("{shape_part};{data_part};{profile_part}");
    if jet_compute_wire_checksum(&body) != checksum {
        return Err(JetComputeError::Serialization(
            "serialized Tensor checksum does not match its contents".to_string(),
        ));
    }
    let expected = jet_compute_storage_len(&shape)?;
    if expected != data.len() {
        return Err(JetComputeError::Serialization(format!(
            "deserialize storage length mismatch: shape wants {expected}, got {}",
            data.len()
        )));
    }
    let mut tensor = jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu)?;
    tensor.data = std::sync::Arc::new(data);
    tensor.last_placement.profile = profile.to_string();
    tensor.last_placement.abilities = abilities
        .iter()
        .map(|ability| (*ability).to_string())
        .collect();
    tensor.last_placement.reason = "deserialized canonical Tensor".to_string();
    jet_compute_validate_tensor(&tensor)?;
    Ok(tensor)
}

// ── #1137 sparse CSR + #1143 CPU SIMD tile + #1147 profile ──────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct JetSparseCsr {
    rows: i64,
    cols: i64,
    row_ptr: Vec<i64>,
    col_idx: Vec<i64>,
    values: Vec<f64>,
}

impl JetShow for JetSparseCsr {
    fn jet_show(&self) -> String {
        format!(
            "SparseCsr({}x{}, nnz={})",
            self.rows,
            self.cols,
            self.values.len()
        )
    }
}

fn jet_compute_to_sparse(tensor: &JetTensor) -> Result<JetSparseCsr, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "to_sparse has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "to_sparse requires a rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if jet_compute_is_accelerator(tensor.device) {
        return Err(JetComputeError::Unsupported(
            "accelerator backend does not support sparse conversion; transfer to CPU explicitly"
                .to_string(),
        ));
    }
    let rows = tensor.shape[0];
    let cols = tensor.shape[1];
    let mut row_ptr = vec![0i64];
    let mut col_idx = Vec::new();
    let mut values = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let v = jet_compute_get_raw(tensor, &vec![r, c])?;
            if v != 0.0 {
                col_idx.push(c);
                values.push(v);
            }
        }
        row_ptr.push(i64::try_from(values.len()).map_err(|_| {
            JetComputeError::InvalidShape("sparse nnz is too large".to_string())
        })?);
    }
    Ok(JetSparseCsr {
        rows,
        cols,
        row_ptr,
        col_idx,
        values,
    })
}

fn jet_compute_sparse_nnz(sparse: &JetSparseCsr) -> i64 {
    i64::try_from(sparse.values.len()).unwrap_or(i64::MAX)
}

fn jet_compute_sparse_mv(
    sparse: &JetSparseCsr,
    vector: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    if vector.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "sparse_mv has no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_validate_sparse(sparse)?;
    jet_compute_validate_tensor(vector)?;
    if jet_compute_is_accelerator(vector.device) {
        return Err(JetComputeError::Unsupported(
            "accelerator backend does not support sparse_mv; transfer the vector to CPU explicitly"
                .to_string(),
        ));
    }
    if vector.shape.len() != 1 || vector.shape[0] != sparse.cols {
        return Err(JetComputeError::RankMismatch(format!(
            "sparse_mv expects a length-{} vector",
            sparse.cols
        )));
    }
    let mut out = jet_compute_zeros(&vec![sparse.rows])?;
    for r in 0..sparse.rows {
        let row = usize::try_from(r).map_err(|_| {
            JetComputeError::InvalidShape("sparse row index is too large".to_string())
        })?;
        let start = usize::try_from(sparse.row_ptr[row]).map_err(|_| {
            JetComputeError::InvalidShape("sparse row pointer is invalid".to_string())
        })?;
        let end = usize::try_from(sparse.row_ptr[row + 1]).map_err(|_| {
            JetComputeError::InvalidShape("sparse row pointer is invalid".to_string())
        })?;
        let mut acc = 0.0;
        for k in start..end {
            let c = sparse.col_idx[k];
            acc += sparse.values[k] * jet_compute_get_raw(vector, &[c])?;
            if !acc.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sparse matrix-vector multiplication produced a non-finite value"
                        .to_string(),
                ));
            }
        }
        jet_compute_set(&mut out, &vec![r], acc)?;
    }
    Ok(out)
}

fn jet_compute_validate_sparse(sparse: &JetSparseCsr) -> Result<(), JetComputeError> {
    if sparse.rows < 0 || sparse.cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "sparse dimensions must be non-negative".to_string(),
        ));
    }
    let rows = usize::try_from(sparse.rows).map_err(|_| {
        JetComputeError::InvalidShape("sparse row count is too large".to_string())
    })?;
    if sparse.row_ptr.len() != rows.saturating_add(1)
        || sparse.col_idx.len() != sparse.values.len()
    {
        return Err(JetComputeError::InvalidShape(
            "sparse CSR arrays have inconsistent lengths".to_string(),
        ));
    }
    let nnz = i64::try_from(sparse.values.len()).map_err(|_| {
        JetComputeError::InvalidShape("sparse nnz is too large".to_string())
    })?;
    if sparse.row_ptr.first().copied() != Some(0)
        || sparse.row_ptr.last().copied() != Some(nnz)
        || sparse
            .row_ptr
            .windows(2)
            .any(|pair| pair[0] < 0 || pair[1] < pair[0] || pair[1] > nnz)
        || sparse
            .col_idx
            .iter()
            .any(|col| *col < 0 || *col >= sparse.cols)
        || sparse.values.iter().any(|value| !value.is_finite())
    {
        return Err(JetComputeError::InvalidShape(
            "sparse CSR invariants are invalid".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_sparse_show(sparse: &JetSparseCsr) -> String {
    sparse.jet_show()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetComputeSimdBackend {
    Avx2,
    Sse2,
    Scalar,
}

impl JetComputeSimdBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Avx2 => "avx2",
            Self::Sse2 => "sse2",
            Self::Scalar => "scalar",
        }
    }

    fn width(self) -> usize {
        match self {
            Self::Avx2 => 8,
            Self::Sse2 => 4,
            Self::Scalar => 1,
        }
    }
}

fn jet_compute_simd_backend() -> JetComputeSimdBackend {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        return jet_compute_simd_backend_for_features(
            is_x86_feature_detected!("avx2"),
            is_x86_feature_detected!("sse2"),
        );
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        jet_compute_simd_backend_for_features(false, false)
    }
}

fn jet_compute_simd_backend_for_features(avx2: bool, sse2: bool) -> JetComputeSimdBackend {
    if avx2 {
        return JetComputeSimdBackend::Avx2;
    }
    if sse2 {
        return JetComputeSimdBackend::Sse2;
    }
    JetComputeSimdBackend::Scalar
}

#[inline(never)]
fn jet_compute_f32_dot_scalar(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .fold(0.0_f32, |sum, (left, right)| sum + left * right)
}

fn jet_compute_simd_backend_available(backend: JetComputeSimdBackend) -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        match backend {
            JetComputeSimdBackend::Avx2 => is_x86_feature_detected!("avx2"),
            JetComputeSimdBackend::Sse2 => is_x86_feature_detected!("sse2"),
            JetComputeSimdBackend::Scalar => true,
        }
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        matches!(backend, JetComputeSimdBackend::Scalar)
    }
}

// JET_VETTED_UNSAFE_BEGIN: jet_compute_cpu_simd
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn jet_compute_f32_dot_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::{_mm256_loadu_ps, _mm256_mul_ps, _mm256_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 8 * 8;
    while index < limit {
        let left = _mm256_loadu_ps(a.as_ptr().add(index));
        let right = _mm256_loadu_ps(b.as_ptr().add(index));
        let product = _mm256_mul_ps(left, right);
        let mut lanes = [0.0_f32; 8];
        _mm256_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 8;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn jet_compute_f32_dot_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86::{_mm256_loadu_ps, _mm256_mul_ps, _mm256_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 8 * 8;
    while index < limit {
        let left = _mm256_loadu_ps(a.as_ptr().add(index));
        let right = _mm256_loadu_ps(b.as_ptr().add(index));
        let product = _mm256_mul_ps(left, right);
        let mut lanes = [0.0_f32; 8];
        _mm256_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 8;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn jet_compute_f32_dot_sse2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::{_mm_loadu_ps, _mm_mul_ps, _mm_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 4 * 4;
    while index < limit {
        let left = _mm_loadu_ps(a.as_ptr().add(index));
        let right = _mm_loadu_ps(b.as_ptr().add(index));
        let product = _mm_mul_ps(left, right);
        let mut lanes = [0.0_f32; 4];
        _mm_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 4;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "sse2")]
unsafe fn jet_compute_f32_dot_sse2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86::{_mm_loadu_ps, _mm_mul_ps, _mm_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 4 * 4;
    while index < limit {
        let left = _mm_loadu_ps(a.as_ptr().add(index));
        let right = _mm_loadu_ps(b.as_ptr().add(index));
        let product = _mm_mul_ps(left, right);
        let mut lanes = [0.0_f32; 4];
        _mm_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 4;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}
fn jet_compute_f32_dot(
    backend: JetComputeSimdBackend,
    a: &[f32],
    b: &[f32],
) -> Result<f32, JetComputeError> {
    if a.len() != b.len() {
        return Err(JetComputeError::InvalidShape(
            "SIMD dot-product inputs have different lengths".to_string(),
        ));
    }
    let value = match backend {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        JetComputeSimdBackend::Avx2 => unsafe { jet_compute_f32_dot_avx2(a, b) },
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        JetComputeSimdBackend::Sse2 => unsafe { jet_compute_f32_dot_sse2(a, b) },
        JetComputeSimdBackend::Scalar => jet_compute_f32_dot_scalar(a, b),
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        _ => jet_compute_f32_dot_scalar(a, b),
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(JetComputeError::Arithmetic(
            "f32 SIMD dot product overflowed".to_string(),
        ))
    }
}
// JET_VETTED_UNSAFE_END: jet_compute_cpu_simd

/// CPU-SIMD profile path (#1143): blocked matmul in f32 arithmetic. The dot
/// product uses runtime-dispatched safe intrinsics where available, then an
/// ordered scalar tail. Lane products are reduced in lane order so the SIMD
/// backend preserves the reproducible CPU-oracle reduction contract.
fn jet_compute_matmul_f32_tile(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile requires rank-2 tensors".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if m < 0 || k < 0 || k2 < 0 || n < 0 {
        return Err(JetComputeError::InvalidShape(
            "matmul_f32_tile dimensions must be non-negative".to_string(),
        ));
    }
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul_f32_tile inner dims {} and {} disagree",
            k, k2
        )));
    }
    if a.device == JetComputeDevice::Metal {
        jet_compute_require_same_contract(a, b, "matmul_f32_tile")?;
        let rows = usize::try_from(m).map_err(|_| {
            JetComputeError::InvalidShape("Metal f32 tile row count is too large".to_string())
        })?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("Metal f32 tile inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("Metal f32 tile column count is too large".to_string())
        })?;
        let left = jet_compute_metal_values(a, "f32 tile input")?;
        let right = jet_compute_metal_values(b, "f32 tile input")?;
        let data = jet_compute_metal::matmul(&left, &right, rows, inner, cols)?;
        let mut out = jet_compute_metal_result_like(a, vec![m, n], data)?;
        out.last_placement.reason =
            "algorithm=metal-matmul; arithmetic=f32; reduction=ordered; dispatch=metal".to_string();
        return jet_compute_record(
            out,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::MatmulF32Tile,
        );
    }
    if a.device == JetComputeDevice::Cuda {
        jet_compute_require_same_contract(a, b, "matmul_f32_tile")?;
        let rows = usize::try_from(m).map_err(|_| {
            JetComputeError::InvalidShape("CUDA f32 tile row count is too large".to_string())
        })?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("CUDA f32 tile inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("CUDA f32 tile column count is too large".to_string())
        })?;
        let left = jet_compute_cuda_values(a, "f32 tile input")?;
        let right = jet_compute_cuda_values(b, "f32 tile input")?;
        let data = jet_compute_cuda::matmul(&left, &right, rows, inner, cols)?;
        let mut out = jet_compute_cuda_result_like(a, vec![m, n], data)?;
        out.last_placement.reason =
            "algorithm=cuda-matmul; arithmetic=f32; reduction=ordered; dispatch=cuda".to_string();
        return jet_compute_record(
            out,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::MatmulF32Tile,
        );
    }
    if a.device == JetComputeDevice::Vulkan {
        jet_compute_require_same_contract(a, b, "matmul_f32_tile")?;
        let rows = usize::try_from(m).map_err(|_| {
            JetComputeError::InvalidShape("Vulkan f32 tile row count is too large".to_string())
        })?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("Vulkan f32 tile inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("Vulkan f32 tile column count is too large".to_string())
        })?;
        let left = jet_compute_vulkan_values(a, "f32 tile input")?;
        let right = jet_compute_vulkan_values(b, "f32 tile input")?;
        let data = jet_compute_vulkan::matmul(&left, &right, rows, inner, cols)?;
        let mut out = jet_compute_vulkan_result_like(a, vec![m, n], data)?;
        out.last_placement.reason =
            "algorithm=vulkan-matmul; arithmetic=f32; reduction=ordered; dispatch=vulkan".to_string();
        return jet_compute_record(
            out,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::MatmulF32Tile,
        );
    }
    if a.device == JetComputeDevice::WebGpu {
        return Err(JetComputeError::Unsupported(
            "WebGPU backend requires a browser WebGPU host".to_string(),
        ));
    }
    let output_shape = vec![m, n];
    let m = usize::try_from(m)
        .map_err(|_| JetComputeError::InvalidShape("f32 tile row count is too large".to_string()))?;
    let k = usize::try_from(k)
        .map_err(|_| JetComputeError::InvalidShape("f32 tile inner dimension is too large".to_string()))?;
    let n = usize::try_from(n)
        .map_err(|_| JetComputeError::InvalidShape("f32 tile column count is too large".to_string()))?;
    let a_values = jet_compute_tensor_values(a)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "f32 tile input"))
        .collect::<Result<Vec<_>, _>>()?;
    let b_values = jet_compute_tensor_values(b)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "f32 tile input"))
        .collect::<Result<Vec<_>, _>>()?;
    let packed_b_len = n.checked_mul(k).ok_or_else(|| {
        JetComputeError::InvalidShape("f32 tile packed-B storage length overflow".to_string())
    })?;
    let mut packed_b = vec![0.0_f32; packed_b_len];
    for row in 0..k {
        for column in 0..n {
            packed_b[column * k + row] = b_values[row * n + column];
        }
    }
    let output_len = m.checked_mul(n).ok_or_else(|| {
        JetComputeError::InvalidShape("f32 tile output storage length overflow".to_string())
    })?;
    let backend = jet_compute_simd_backend();
    let mut output = vec![0.0_f64; output_len];
    const TILE: usize = 8;
    for row_tile in (0..m).step_by(TILE) {
        let row_end = row_tile.saturating_add(TILE).min(m);
        for column_tile in (0..n).step_by(TILE) {
            let column_end = column_tile.saturating_add(TILE).min(n);
            for row in row_tile..row_end {
                let left = &a_values[row * k..(row + 1) * k];
                for column in column_tile..column_end {
                    let right = &packed_b[column * k..(column + 1) * k];
                    let value = jet_compute_f32_dot(backend, left, right)?;
                    output[row * n + column] = f64::from(value);
                }
            }
        }
    }
    let mut out = jet_compute_tensor_from_shape(
        output_shape,
        0.0,
        JetComputeDevice::Cpu,
    )?;
    out.data = std::sync::Arc::new(output);
    out.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
    out.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
        .iter()
        .map(|ability| (*ability).to_string())
        .collect();
    out.last_placement.reason = format!(
        "algorithm=blocked-matmul; tile={TILE}; arithmetic=f32; reduction=ordered; dispatch={}; vector_width={}; tail=scalar",
        backend.name(),
        backend.width(),
    );
    jet_compute_record(
        out,
        &[a, b],
        vec![a.clone(), b.clone()],
        JetComputeTapeRule::MatmulF32Tile,
    )
}

fn jet_compute_profile_f32_strict() -> String {
    let backend = jet_compute_simd_backend();
    format!(
        "backend={};version={};profile={};algorithm=blocked-matmul;tile=8;dispatch={};vector_width={};tail=scalar;cache={}",
        CPU_ORACLE_BACKEND,
        CPU_ORACLE_VERSION,
        CPU_ORACLE_F32_PROFILE,
        backend.name(),
        backend.width(),
        CPU_ORACLE_CACHE,
    )
}

fn jet_compute_profile_show() -> String {
    jet_compute_profile_f32_strict()
}

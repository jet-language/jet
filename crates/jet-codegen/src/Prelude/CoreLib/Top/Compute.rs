// ── D-COMPUTE1=D / D-COMPUTE-TYPE1=D / D-COMPUTE-PLACE1=D (#443) ─────────────
// One Core compute family. `Tensor` owns ranked multidimensional storage on the
// CPU oracle; borrowed views share the same substrate; placement emits receipts.
// Engines only marshal into these Prelude symbols (I9).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetComputeDevice {
    Auto,
    Cpu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputePlacementReceipt {
    requested: JetComputeDevice,
    selected: JetComputeDevice,
    reason: String,
}

#[derive(Clone, Debug, PartialEq)]
struct JetTensor {
    shape: Vec<i64>,
    strides: Vec<i64>,
    data: Vec<f64>,
    device: JetComputeDevice,
    last_placement: JetComputePlacementReceipt,
}

#[derive(Clone, Debug, PartialEq)]
enum JetComputeError {
    InvalidShape(String),
    RankMismatch(String),
    OutOfBounds(String),
    Device(String),
}

impl JetShow for JetComputeError {
    fn jet_show(&self) -> String {
        match self {
            JetComputeError::InvalidShape(m)
            | JetComputeError::RankMismatch(m)
            | JetComputeError::OutOfBounds(m)
            | JetComputeError::Device(m) => m.clone(),
        }
    }
}

impl JetShow for JetComputeDevice {
    fn jet_show(&self) -> String {
        match self {
            JetComputeDevice::Auto => "Auto".to_string(),
            JetComputeDevice::Cpu => "CPU".to_string(),
        }
    }
}

impl JetShow for JetComputePlacementReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Placement(requested={}, selected={}, reason={})",
            self.requested.jet_show(),
            self.selected.jet_show(),
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
            self.data.len()
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
    let mut strides = vec![1i64; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        let next = shape[i + 1].saturating_mul(strides[i + 1]);
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

fn jet_compute_place(requested: JetComputeDevice) -> JetComputePlacementReceipt {
    // D-COMPUTE-PLACE1=D: `.Auto` may select an accelerator later; CPU oracle is
    // the shipped default and the differential reference.
    let selected = JetComputeDevice::Cpu;
    let reason = match requested {
        JetComputeDevice::Auto => "Auto selected CPU oracle (default profile)".to_string(),
        JetComputeDevice::Cpu => "explicit CPU placement".to_string(),
    };
    JetComputePlacementReceipt {
        requested,
        selected,
        reason,
    }
}

fn jet_compute_tensor_from_shape(
    shape: Vec<i64>,
    fill: f64,
    requested: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    let strides = jet_compute_row_major_strides(&shape)?;
    let n = jet_compute_numel(&shape)?;
    let receipt = jet_compute_place(requested);
    Ok(JetTensor {
        shape,
        strides,
        data: vec![fill; n as usize],
        device: receipt.selected,
        last_placement: receipt,
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
    let shape = vec![values.len() as i64];
    let strides = jet_compute_row_major_strides(&shape)?;
    let receipt = jet_compute_place(JetComputeDevice::Auto);
    Ok(JetTensor {
        shape,
        strides,
        data: values.clone(),
        device: receipt.selected,
        last_placement: receipt,
    })
}

fn jet_compute_tensor_shape(tensor: &JetTensor) -> Vec<i64> {
    tensor.shape.clone()
}

fn jet_compute_tensor_rank(tensor: &JetTensor) -> i64 {
    tensor.shape.len() as i64
}

fn jet_compute_tensor_numel(tensor: &JetTensor) -> i64 {
    tensor.data.len() as i64
}

fn jet_compute_tensor_device(tensor: &JetTensor) -> String {
    tensor.device.jet_show()
}

fn jet_compute_tensor_placement(tensor: &JetTensor) -> String {
    tensor.last_placement.jet_show()
}

fn jet_compute_tensor_to_list(tensor: &JetTensor) -> Vec<f64> {
    tensor.data.clone()
}

fn jet_compute_offset(tensor: &JetTensor, indices: &[i64]) -> Result<usize, JetComputeError> {
    if indices.len() != tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(format!(
            "expected {} indices, got {}",
            tensor.shape.len(),
            indices.len()
        )));
    }
    let mut offset = 0i64;
    for (i, (&idx, (&dim, &stride))) in indices
        .iter()
        .zip(tensor.shape.iter().zip(tensor.strides.iter()))
        .enumerate()
    {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "index {} out of range for axis {} of extent {}",
                idx, i, dim
            )));
        }
        offset += idx * stride;
    }
    Ok(offset as usize)
}

fn jet_compute_get(tensor: &JetTensor, indices: &Vec<i64>) -> Result<f64, JetComputeError> {
    let offset = jet_compute_offset(tensor, indices)?;
    Ok(tensor.data[offset])
}

fn jet_compute_set(
    tensor: &mut JetTensor,
    indices: &Vec<i64>,
    value: f64,
) -> Result<(), JetComputeError> {
    let offset = jet_compute_offset(tensor, indices)?;
    tensor.data[offset] = value;
    Ok(())
}

fn jet_compute_same_shape(a: &JetTensor, b: &JetTensor) -> Result<(), JetComputeError> {
    if a.shape != b.shape {
        return Err(JetComputeError::RankMismatch(format!(
            "shape {:?} does not match {:?}",
            a.shape, b.shape
        )));
    }
    Ok(())
}

fn jet_compute_add(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_same_shape(a, b)?;
    let receipt = jet_compute_place(JetComputeDevice::Auto);
    let data = a
        .data
        .iter()
        .zip(b.data.iter())
        .map(|(x, y)| x + y)
        .collect();
    Ok(JetTensor {
        shape: a.shape.clone(),
        strides: a.strides.clone(),
        data,
        device: receipt.selected,
        last_placement: receipt,
    })
}

fn jet_compute_mul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_same_shape(a, b)?;
    let receipt = jet_compute_place(JetComputeDevice::Auto);
    let data = a
        .data
        .iter()
        .zip(b.data.iter())
        .map(|(x, y)| x * y)
        .collect();
    Ok(JetTensor {
        shape: a.shape.clone(),
        strides: a.strides.clone(),
        data,
        device: receipt.selected,
        last_placement: receipt,
    })
}

fn jet_compute_reshape(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    let n = jet_compute_numel(shape)?;
    if n != tensor.data.len() as i64 {
        return Err(JetComputeError::InvalidShape(format!(
            "reshape numel {} does not match tensor numel {}",
            n,
            tensor.data.len()
        )));
    }
    let strides = jet_compute_row_major_strides(shape)?;
    Ok(JetTensor {
        shape: shape.clone(),
        strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
    })
}

/// Matrix alias: rank-2 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_matrix(rows: i64, cols: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if rows < 0 || cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "Matrix rows and cols must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![rows, cols], fill, JetComputeDevice::Auto)
}

/// Vec alias: rank-1 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_vec(len: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if len < 0 {
        return Err(JetComputeError::InvalidShape(
            "Vec length must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![len], fill, JetComputeDevice::Auto)
}

fn jet_compute_matmul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul requires rank-2 tensors".to_string(),
        ));
    }
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul inner dims {} and {} disagree",
            k, k2
        )));
    }
    let mut out = jet_compute_tensor_from_shape(vec![m, n], 0.0, JetComputeDevice::Auto)?;
    for i in 0..m {
        for j in 0..n {
            let mut sum = 0.0;
            for t in 0..k {
                let av = jet_compute_get(a, &vec![i, t])?;
                let bv = jet_compute_get(b, &vec![t, j])?;
                sum += av * bv;
            }
            jet_compute_set(&mut out, &vec![i, j], sum)?;
        }
    }
    Ok(out)
}

fn jet_compute_device_cpu() -> JetComputeDevice {
    JetComputeDevice::Cpu
}

fn jet_compute_device_auto() -> JetComputeDevice {
    JetComputeDevice::Auto
}

fn jet_compute_on_device(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    let receipt = jet_compute_place(device);
    if receipt.selected != JetComputeDevice::Cpu {
        return Err(JetComputeError::Device(
            "only the CPU oracle is shipped in this Core compute slice".to_string(),
        ));
    }
    Ok(JetTensor {
        shape: tensor.shape.clone(),
        strides: tensor.strides.clone(),
        data: tensor.data.clone(),
        device: receipt.selected,
        last_placement: receipt,
    })
}

// ── D-COMPUTE1=D / #1136: ndarray broadcast, ufuncs, reductions ─────────────

fn jet_compute_broadcast_shape(
    a: &[i64],
    b: &[i64],
) -> Result<Vec<i64>, JetComputeError> {
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
        if da == db || da == 1 || db == 1 {
            out[i] = da.max(db);
        } else {
            return Err(JetComputeError::RankMismatch(format!(
                "cannot broadcast shapes {:?} and {:?}",
                a, b
            )));
        }
    }
    Ok(out)
}

fn jet_compute_materialize_broadcast(
    tensor: &JetTensor,
    shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    let n = jet_compute_numel(shape)?;
    let strides = jet_compute_row_major_strides(shape)?;
    let src_rank = tensor.shape.len();
    let dst_rank = shape.len();
    let mut aligned_strides = vec![0i64; dst_rank];
    for i in 0..src_rank {
        let dst_i = dst_rank - src_rank + i;
        aligned_strides[dst_i] = if tensor.shape[i] == 1 {
            0
        } else {
            tensor.strides[i]
        };
    }
    let mut data = Vec::with_capacity(n as usize);
    for flat in 0..n {
        let mut rem = flat;
        let mut offset = 0i64;
        for i in 0..dst_rank {
            let dim = shape[dst_rank - 1 - i];
            let idx = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
            let src_i = dst_rank - 1 - i;
            offset += idx * aligned_strides[src_i];
        }
        data.push(tensor.data[offset as usize]);
    }
    let receipt = jet_compute_place(JetComputeDevice::Auto);
    Ok(JetTensor {
        shape: shape.to_vec(),
        strides,
        data,
        device: receipt.selected,
        last_placement: receipt,
    })
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
    jet_compute_materialize_broadcast(tensor, shape)
}

fn jet_compute_transpose(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "transpose requires rank-2 tensor".to_string(),
        ));
    }
    let (rows, cols) = (tensor.shape[0], tensor.shape[1]);
    let mut out = jet_compute_tensor_from_shape(vec![cols, rows], 0.0, JetComputeDevice::Auto)?;
    for i in 0..rows {
        for j in 0..cols {
            let v = jet_compute_get(tensor, &vec![i, j])?;
            jet_compute_set(&mut out, &vec![j, i], v)?;
        }
    }
    Ok(out)
}

fn jet_compute_sum_axis(tensor: &JetTensor, axis: i64) -> Result<JetTensor, JetComputeError> {
    if axis < 0 || axis as usize >= tensor.shape.len() {
        return Err(JetComputeError::OutOfBounds(format!(
            "sum_axis axis {} out of range for rank {}",
            axis,
            tensor.shape.len()
        )));
    }
    let axis = axis as usize;
    let mut out_shape = Vec::new();
    for (i, &d) in tensor.shape.iter().enumerate() {
        if i != axis {
            out_shape.push(d);
        }
    }
    if out_shape.is_empty() {
        out_shape.push(1);
    }
    let mut out = jet_compute_tensor_from_shape(out_shape.clone(), 0.0, JetComputeDevice::Auto)?;
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
            sum += jet_compute_get(tensor, &coords)?;
        }
        jet_compute_set(&mut out, &out_coords, sum)?;
    }
    Ok(out)
}

fn jet_compute_unary(op: &str, tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    let receipt = jet_compute_place(JetComputeDevice::Auto);
    let data = tensor
        .data
        .iter()
        .map(|v| match op {
            "negate" => -*v,
            "abs" => v.abs(),
            "exp" => v.exp(),
            "log" => v.ln(),
            "sqrt" => v.sqrt(),
            _ => *v,
        })
        .collect();
    Ok(JetTensor {
        shape: tensor.shape.clone(),
        strides: tensor.strides.clone(),
        data,
        device: receipt.selected,
        last_placement: receipt,
    })
}

fn jet_compute_binary(
    op: &str,
    a: &JetTensor,
    b: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    let shape = jet_compute_broadcast_shape(&a.shape, &b.shape)?;
    let left = if a.shape == shape {
        a.clone()
    } else {
        jet_compute_materialize_broadcast(a, &shape)?
    };
    let right = if b.shape == shape {
        b.clone()
    } else {
        jet_compute_materialize_broadcast(b, &shape)?
    };
    let receipt = jet_compute_place(JetComputeDevice::Auto);
    let data = left
        .data
        .iter()
        .zip(right.data.iter())
        .map(|(x, y)| match op {
            "sub" => x - y,
            "div" => x / y,
            "maximum" => x.max(*y),
            "minimum" => x.min(*y),
            "add" => x + y,
            "mul" => x * y,
            _ => x + y,
        })
        .collect();
    let strides = jet_compute_row_major_strides(&shape)?;
    Ok(JetTensor {
        shape,
        strides,
        data,
        device: receipt.selected,
        last_placement: receipt,
    })
}

// ── #1137 / D-COMPUTE1: dense linalg on the Tensor CPU oracle ───────────────

fn jet_compute_eye(n: i64) -> Result<JetTensor, JetComputeError> {
    if n < 0 {
        return Err(JetComputeError::InvalidShape(
            "eye size must be non-negative".to_string(),
        ));
    }
    let mut out = jet_compute_tensor_from_shape(vec![n, n], 0.0, JetComputeDevice::Auto)?;
    for i in 0..n {
        jet_compute_set(&mut out, &vec![i, i], 1.0)?;
    }
    Ok(out)
}

fn jet_compute_det(tensor: &JetTensor) -> Result<f64, JetComputeError> {
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "det requires a square rank-2 tensor".to_string(),
        ));
    }
    let n = tensor.shape[0] as usize;
    let mut a = tensor.data.clone();
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
        for r in (i + 1)..n {
            let factor = a[r * n + i] / piv;
            for c in i..n {
                a[r * n + c] -= factor * a[i * n + c];
            }
        }
    }
    Ok(det)
}

fn jet_compute_inv(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "inv requires a square rank-2 tensor".to_string(),
        ));
    }
    let n = tensor.shape[0] as usize;
    let mut a = vec![0.0; n * n * 2];
    for i in 0..n {
        for j in 0..n {
            a[i * (2 * n) + j] = jet_compute_get(tensor, &vec![i as i64, j as i64])?;
            a[i * (2 * n) + n + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * (2 * n) + i].abs() > a[pivot * (2 * n) + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * (2 * n) + i].abs() < 1e-15 {
            return Err(JetComputeError::InvalidShape(
                "matrix is singular".to_string(),
            ));
        }
        if pivot != i {
            for c in 0..(2 * n) {
                a.swap(i * (2 * n) + c, pivot * (2 * n) + c);
            }
        }
        let piv = a[i * (2 * n) + i];
        for c in 0..(2 * n) {
            a[i * (2 * n) + c] /= piv;
        }
        for r in 0..n {
            if r == i {
                continue;
            }
            let factor = a[r * (2 * n) + i];
            for c in 0..(2 * n) {
                a[r * (2 * n) + c] -= factor * a[i * (2 * n) + c];
            }
        }
    }
    let mut out = jet_compute_tensor_from_shape(vec![n as i64, n as i64], 0.0, JetComputeDevice::Auto)?;
    for i in 0..n {
        for j in 0..n {
            jet_compute_set(&mut out, &vec![i as i64, j as i64], a[i * (2 * n) + n + j])?;
        }
    }
    Ok(out)
}

fn jet_compute_solve(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    let inv = jet_compute_inv(a)?;
    jet_compute_matmul(&inv, b)
}

/// Naive DFT on a rank-1 real tensor → interleaved [re, im, re, im, …] length 2n.
fn jet_compute_fft(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.shape.len() != 1 {
        return Err(JetComputeError::RankMismatch(
            "fft requires a rank-1 tensor".to_string(),
        ));
    }
    let n = tensor.data.len();
    let mut out = jet_compute_tensor_from_shape(vec![(n * 2) as i64], 0.0, JetComputeDevice::Auto)?;
    for k in 0..n {
        let mut re = 0.0;
        let mut im = 0.0;
        for t in 0..n {
            let angle = -2.0 * std::f64::consts::PI * (k as f64) * (t as f64) / (n as f64);
            re += tensor.data[t] * angle.cos();
            im += tensor.data[t] * angle.sin();
        }
        jet_compute_set(&mut out, &vec![(2 * k) as i64], re)?;
        jet_compute_set(&mut out, &vec![(2 * k + 1) as i64], im)?;
    }
    Ok(out)
}

// ── #1138: stream + transfer receipts (CPU oracle) ──────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputeStream {
    id: i64,
    device: JetComputeDevice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputeTransferReceipt {
    from: JetComputeDevice,
    to: JetComputeDevice,
    bytes: i64,
    fallback: String,
}

impl JetShow for JetComputeStream {
    fn jet_show(&self) -> String {
        format!("ComputeStream(id={}, device={})", self.id, self.device.jet_show())
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
    JetComputeStream {
        id: 1,
        device: JetComputeDevice::Cpu,
    }
}

fn jet_compute_stream_sync(_stream: &JetComputeStream) -> Result<(), JetComputeError> {
    Ok(())
}

fn jet_compute_stream_show(stream: &JetComputeStream) -> String {
    stream.jet_show()
}

fn jet_compute_transfer(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    let bytes = (tensor.data.len() * std::mem::size_of::<f64>()) as i64;
    let from = tensor.device;
    let mut out = jet_compute_on_device(tensor, device)?;
    let fallback = if from == out.device {
        "none".to_string()
    } else {
        "cpu-oracle-copy".to_string()
    };
    out.last_placement.reason = format!(
        "transfer bytes={bytes} fallback={fallback} from={} to={}",
        from.jet_show(),
        out.device.jet_show()
    );
    let _ = JetComputeTransferReceipt {
        from,
        to: out.device,
        bytes,
        fallback,
    };
    Ok(out)
}

fn jet_compute_transfer_show(tensor: &JetTensor) -> String {
    tensor.last_placement.jet_show()
}

// ── #1139 / #1140: safe kernel bounds + raw-kernel contract label ────────────

fn jet_compute_kernel_bounds_ok(
    shape: &[i64],
    indices: &[i64],
) -> Result<bool, JetComputeError> {
    if shape.len() != indices.len() {
        return Err(JetComputeError::RankMismatch(
            "kernel index rank must match tensor shape".to_string(),
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

/// Records a raw-kernel contract string under `#Unsafe` (D-COMPUTE-KERNEL1).
fn jet_compute_raw_kernel_contract(reason: String, arity: i64) -> Result<String, JetComputeError> {
    if reason.trim().is_empty() {
        return Err(JetComputeError::Device(
            "raw kernel contract requires a non-empty #Unsafe reason".to_string(),
        ));
    }
    if arity < 0 {
        return Err(JetComputeError::InvalidShape(
            "raw kernel arity must be non-negative".to_string(),
        ));
    }
    Ok(format!("RawKernel(reason={reason}, arity={arity})"))
}

// ── #1141 / D-COMPUTE-AUTODIFF1: reverse-mode VJP + JVP for dense ops ────────

#[derive(Clone, Debug, PartialEq)]
struct JetComputeGradTriple {
    value: JetTensor,
    grad_a: JetTensor,
    grad_b: JetTensor,
}

impl JetShow for JetComputeGradTriple {
    fn jet_show(&self) -> String {
        format!(
            "GradTriple(value={}, grad_a={}, grad_b={})",
            self.value.jet_show(),
            self.grad_a.jet_show(),
            self.grad_b.jet_show()
        )
    }
}

fn jet_compute_vjp_add(
    _a: &JetTensor,
    _b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    Ok((cot.clone(), cot.clone()))
}

fn jet_compute_vjp_mul(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    Ok((jet_compute_mul(b, cot)?, jet_compute_mul(a, cot)?))
}

fn jet_compute_vjp_matmul(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    let b_t = jet_compute_transpose(b)?;
    let a_t = jet_compute_transpose(a)?;
    Ok((jet_compute_matmul(cot, &b_t)?, jet_compute_matmul(&a_t, cot)?))
}

/// Forward-mode JVP of elementwise mul: `t_a * b + a * t_b`.
fn jet_compute_jvp_mul(
    a: &JetTensor,
    b: &JetTensor,
    t_a: &JetTensor,
    t_b: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    let left = jet_compute_mul(t_a, b)?;
    let right = jet_compute_mul(a, t_b)?;
    jet_compute_add(&left, &right)
}

/// Scalar-loss convenience: value + ∂/∂a + ∂/∂b of `sum(a * b)`.
fn jet_compute_value_and_grad_mul(
    a: &JetTensor,
    b: &JetTensor,
) -> Result<JetComputeGradTriple, JetComputeError> {
    let value = jet_compute_mul(a, b)?;
    let ones = jet_compute_ones(&value.shape)?;
    let (ga, gb) = jet_compute_vjp_mul(a, b, &ones)?;
    Ok(JetComputeGradTriple {
        value,
        grad_a: ga,
        grad_b: gb,
    })
}

fn jet_compute_grad_value(g: &JetComputeGradTriple) -> JetTensor {
    g.value.clone()
}

fn jet_compute_grad_a(g: &JetComputeGradTriple) -> JetTensor {
    g.grad_a.clone()
}

fn jet_compute_grad_b(g: &JetComputeGradTriple) -> JetTensor {
    g.grad_b.clone()
}

fn jet_compute_grad_show(g: &JetComputeGradTriple) -> String {
    g.jet_show()
}

// ── #1142: ML step + serialization over the Tensor oracle ───────────────────

fn jet_compute_mse_loss(pred: &JetTensor, target: &JetTensor) -> Result<f64, JetComputeError> {
    let diff = jet_compute_binary("sub", pred, target)?;
    let sq = jet_compute_mul(&diff, &diff)?;
    let n = jet_compute_numel(&sq.shape)? as f64;
    if n == 0.0 {
        return Err(JetComputeError::InvalidShape(
            "mse_loss requires a non-empty tensor".to_string(),
        ));
    }
    Ok(sq.data.iter().sum::<f64>() / n)
}

fn jet_compute_sgd_step(
    param: &JetTensor,
    grad: &JetTensor,
    lr: f64,
) -> Result<JetTensor, JetComputeError> {
    let scaled = jet_compute_full(&grad.shape, lr)?;
    let delta = jet_compute_mul(grad, &scaled)?;
    jet_compute_binary("sub", param, &delta)
}

fn jet_compute_serialize(tensor: &JetTensor) -> String {
    let shape = tensor
        .shape
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let data = tensor
        .data
        .iter()
        .map(|v| format!("{v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("shape={shape};data={data}")
}

fn jet_compute_deserialize(payload: &String) -> Result<JetTensor, JetComputeError> {
    let Some((shape_part, data_part)) = payload.split_once(";data=") else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…".to_string(),
        ));
    };
    let shape_str = shape_part
        .strip_prefix("shape=")
        .ok_or_else(|| JetComputeError::InvalidShape("missing shape=".to_string()))?;
    let shape: Vec<i64> = if shape_str.is_empty() {
        Vec::new()
    } else {
        shape_str
            .split(',')
            .map(|p| {
                p.parse::<i64>().map_err(|_| {
                    JetComputeError::InvalidShape(format!("bad shape axis `{p}`"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let data: Vec<f64> = if data_part.is_empty() {
        Vec::new()
    } else {
        data_part
            .split(',')
            .map(|p| {
                p.parse::<f64>().map_err(|_| {
                    JetComputeError::InvalidShape(format!("bad data value `{p}`"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let expected = jet_compute_numel(&shape)?;
    if expected != data.len() as i64 {
        return Err(JetComputeError::InvalidShape(format!(
            "deserialize numel mismatch: shape wants {expected}, got {}",
            data.len()
        )));
    }
    let mut tensor = jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu)?;
    tensor.data = data;
    Ok(tensor)
}

// ── #1137 sparse CSR + #1143 CPU SIMD tile + #1147 profile ──────────────────

#[derive(Clone, Debug, PartialEq)]
struct JetSparseCsr {
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
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "to_sparse requires a rank-2 tensor".to_string(),
        ));
    }
    let rows = tensor.shape[0];
    let cols = tensor.shape[1];
    let mut row_ptr = vec![0i64];
    let mut col_idx = Vec::new();
    let mut values = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let v = jet_compute_get(tensor, &vec![r, c])?;
            if v != 0.0 {
                col_idx.push(c);
                values.push(v);
            }
        }
        row_ptr.push(values.len() as i64);
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
    sparse.values.len() as i64
}

fn jet_compute_sparse_mv(
    sparse: &JetSparseCsr,
    vector: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    if vector.shape.len() != 1 || vector.shape[0] != sparse.cols {
        return Err(JetComputeError::RankMismatch(format!(
            "sparse_mv expects a length-{} vector",
            sparse.cols
        )));
    }
    let mut out = jet_compute_zeros(&vec![sparse.rows])?;
    for r in 0..sparse.rows {
        let start = sparse.row_ptr[r as usize] as usize;
        let end = sparse.row_ptr[(r + 1) as usize] as usize;
        let mut acc = 0.0;
        for k in start..end {
            let c = sparse.col_idx[k];
            acc += sparse.values[k] * vector.data[c as usize];
        }
        jet_compute_set(&mut out, &vec![r], acc)?;
    }
    Ok(out)
}

fn jet_compute_sparse_show(sparse: &JetSparseCsr) -> String {
    sparse.jet_show()
}

/// Named CPU-SIMD profile path; math matches scalar matmul (D-COMPUTE-BACKEND1).
fn jet_compute_matmul_f32_tile(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_matmul(a, b)
}

fn jet_compute_profile_f32_strict() -> String {
    "F32Strict+Reproducible".to_string()
}

fn jet_compute_profile_show() -> String {
    jet_compute_profile_f32_strict()
}

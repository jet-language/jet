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

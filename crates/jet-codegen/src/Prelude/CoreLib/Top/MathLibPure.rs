// Shared core.math helpers (I9). Included by AOT prelude, JIT math_rt, and comptime ambient.
// Keep std-only; no jet_std / host types.

/// D-TYPE2-IMAG1=A: the one runtime value for imaginary literals. The type and
/// all arithmetic stay in this shared Prelude source so AOT, JIT, comptime,
/// and web cannot grow different complex-number rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetComplex {
    pub real: f64,
    pub imaginary: f64,
}

impl JetComplex {
    pub fn from_parts(real: f64, imaginary: f64) -> Self {
        Self { real, imaginary }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self::from_parts(self.real + other.real, self.imaginary + other.imaginary)
    }

    pub fn sub(&self, other: &Self) -> Self {
        Self::from_parts(self.real - other.real, self.imaginary - other.imaginary)
    }

    pub fn mul(&self, other: &Self) -> Self {
        Self::from_parts(
            self.real * other.real - self.imaginary * other.imaginary,
            self.real * other.imaginary + self.imaginary * other.real,
        )
    }

    pub fn div(&self, other: &Self) -> Self {
        let denominator = other.real * other.real + other.imaginary * other.imaginary;
        Self::from_parts(
            (self.real * other.real + self.imaginary * other.imaginary) / denominator,
            (self.imaginary * other.real - self.real * other.imaginary) / denominator,
        )
    }

    pub fn abs(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    pub fn to_string_rep(&self) -> String {
        let sign = if self.imaginary.is_sign_negative() { '-' } else { '+' };
        format!("{} {} {}i", self.real, sign, self.imaginary.abs())
    }
}

impl std::fmt::Display for JetComplex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_rep())
    }
}

pub fn jet_complex_from_parts(real: f64, imaginary: f64) -> JetComplex {
    JetComplex::from_parts(real, imaginary)
}

pub fn jet_complex_add(left: &JetComplex, right: &JetComplex) -> JetComplex {
    left.add(right)
}

pub fn jet_complex_sub(left: &JetComplex, right: &JetComplex) -> JetComplex {
    left.sub(right)
}

pub fn jet_complex_mul(left: &JetComplex, right: &JetComplex) -> JetComplex {
    left.mul(right)
}

pub fn jet_complex_div(left: &JetComplex, right: &JetComplex) -> JetComplex {
    left.div(right)
}

pub fn jet_complex_abs(value: JetComplex) -> f64 {
    value.abs()
}

pub fn jet_complex_to_string(value: &JetComplex) -> String {
    value.to_string_rep()
}

pub fn jet_std_math_pi() -> f64 {
    std::f64::consts::PI
}

pub fn jet_std_math_abs_i64(value: i64) -> i64 {
    value.abs()
}

pub fn jet_std_math_abs_f64(value: f64) -> f64 {
    value.abs()
}

pub fn jet_std_math_abs_f32(value: f32) -> f32 {
    value.abs()
}

/// Scalar IEEE-754 predicates shared by AOT, resident JIT, and comptime.
pub fn jet_std_math_is_nan(value: f64) -> bool {
    value.is_nan()
}

pub fn jet_std_math_is_infinite(value: f64) -> bool {
    value.is_infinite()
}

pub fn jet_std_math_is_finite(value: f64) -> bool {
    value.is_finite()
}

pub fn jet_std_math_asinh(value: f64) -> f64 {
    value.asinh()
}

pub fn jet_std_math_acosh(value: f64) -> f64 {
    value.acosh()
}

pub fn jet_std_math_atanh(value: f64) -> f64 {
    value.atanh()
}

pub fn jet_std_math_cbrt(value: f64) -> f64 {
    value.cbrt()
}
pub fn jet_std_math_exp(value: f64) -> f64 {
    value.exp()
}


pub fn jet_std_math_exp2(value: f64) -> f64 {
    value.exp2()
}

pub fn jet_std_math_exp_m1(value: f64) -> f64 {
    value.exp_m1()
}

/// Compensated summation for Python-compatible `math.fsum` semantics.
pub fn jet_std_math_fsum(values: Vec<f64>) -> f64 {
    let mut total = 0.0;
    let mut compensation = 0.0;
    for value in values {
        let corrected = value - compensation;
        let next = total + corrected;
        compensation = (next - total) - corrected;
        total = next;
    }
    total
}
pub fn jet_stats_sum(values: Vec<f64>) -> f64 {
    jet_std_math_fsum(values)
}
pub fn jet_stats_count(values: Vec<f64>) -> i64 {
    values.len() as i64
}

pub fn jet_stats_prod(values: Vec<f64>) -> f64 {
    values.into_iter().product()
}

pub fn jet_stats_cumsum(values: Vec<f64>) -> Vec<f64> {
    let mut total = 0.0;
    values
        .into_iter()
        .map(|value| {
            total += value;
            total
        })
        .collect()
}

pub fn jet_stats_cumprod(values: Vec<f64>) -> Vec<f64> {
    let mut total = 1.0;
    values
        .into_iter()
        .map(|value| {
            total *= value;
            total
        })
        .collect()
}

pub fn jet_stats_diff(values: Vec<f64>) -> Vec<f64> {
    values
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect()
}

pub fn jet_stats_moving_average(values: Vec<f64>, window: i64) -> Vec<f64> {
    if window <= 0 {
        return Vec::new();
    }
    let window = window as usize;
    (0..values.len())
        .map(|index| {
            let start = index.saturating_add(1).saturating_sub(window);
            let slice = &values[start..=index];
            slice.iter().sum::<f64>() / slice.len() as f64
        })
        .collect()
}

pub fn jet_stats_ewma(values: Vec<f64>, alpha: f64) -> Vec<f64> {
    let Some((&first, rest)) = values.split_first() else {
        return Vec::new();
    };
    let alpha = alpha.clamp(0.0, 1.0);
    let mut total = first;
    let mut out = vec![total];
    out.extend(rest.iter().map(|value| {
        total = alpha * *value + (1.0 - alpha) * total;
        total
    }));
    out
}

pub fn jet_stats_rank(values: Vec<f64>) -> Vec<f64> {
    values
        .iter()
        .map(|value| {
            1.0 + values.iter().filter(|other| *other < value).count() as f64
        })
        .collect()
}

pub fn jet_stats_zscore(values: Vec<f64>) -> Vec<f64> {
    if values.is_empty() {
        return Vec::new();
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let delta = *value - mean;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    let deviation = variance.sqrt();
    if deviation == 0.0 {
        return Vec::new();
    }
    values
        .into_iter()
        .map(|value| (value - mean) / deviation)
        .collect()
}

pub fn jet_stats_clip(values: Vec<f64>, low: f64, high: f64) -> Vec<f64> {
    values
        .into_iter()
        .map(|value| value.clamp(low, high))
        .collect()
}
fn jet_stats_sorted(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    values
}

fn jet_stats_mean_value(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn jet_stats_quantile_value(values: &[f64], probability: f64) -> Option<f64> {
    let sorted = jet_stats_sorted(values.to_vec());
    let (&first, rest) = sorted.split_first()?;
    let last = rest.last().copied().unwrap_or(first);
    if probability <= 0.0 {
        return Some(first);
    }
    if probability >= 1.0 {
        return Some(last);
    }
    let position = probability * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return Some(sorted[lower]);
    }
    let fraction = position - lower as f64;
    Some(sorted[lower] + (sorted[upper] - sorted[lower]) * fraction)
}

pub fn jet_stats_mean(values: Vec<f64>) -> Option<f64> {
    jet_stats_mean_value(&values)
}

pub fn jet_stats_geometric_mean(values: Vec<f64>) -> Option<f64> {
    if values.is_empty() || values.iter().any(|value| *value <= 0.0) {
        return None;
    }
    Some((values.iter().map(|value| value.ln()).sum::<f64>() / values.len() as f64).exp())
}

pub fn jet_stats_harmonic_mean(values: Vec<f64>) -> Option<f64> {
    if values.is_empty() || values.iter().any(|value| *value == 0.0) {
        return None;
    }
    Some(values.len() as f64 / values.iter().map(|value| 1.0 / value).sum::<f64>())
}

pub fn jet_stats_median(values: Vec<f64>) -> Option<f64> {
    jet_stats_quantile_value(&values, 0.5)
}

pub fn jet_stats_median_grouped(values: Vec<f64>, interval: f64) -> Option<f64> {
    if values.is_empty() || interval <= 0.0 {
        return None;
    }
    let sorted = jet_stats_sorted(values);
    let middle = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    let lower = middle - interval / 2.0;
    let upper = lower + interval;
    let below = sorted.iter().filter(|value| **value < lower).count();
    let frequency = sorted
        .iter()
        .filter(|value| **value >= lower && **value < upper)
        .count();
    if frequency == 0 {
        Some(middle)
    } else {
        Some(lower + interval * (sorted.len() / 2 - below) as f64 / frequency as f64)
    }
}

pub fn jet_stats_median_low(values: Vec<f64>) -> Option<f64> {
    let sorted = jet_stats_sorted(values);
    sorted
        .get((sorted.len().saturating_sub(1)) / 2)
        .copied()
}

pub fn jet_stats_median_high(values: Vec<f64>) -> Option<f64> {
    let sorted = jet_stats_sorted(values);
    sorted.get(sorted.len() / 2).copied()
}

pub fn jet_stats_quantile(values: Vec<f64>, probability: f64) -> Option<f64> {
    jet_stats_quantile_value(&values, probability)
}

pub fn jet_stats_percentile(values: Vec<f64>, percent: f64) -> Option<f64> {
    jet_stats_quantile_value(&values, percent / 100.0)
}

pub fn jet_stats_mode(values: Vec<f64>) -> Option<f64> {
    let first = values.first().copied()?;
    let mut best = first;
    let mut best_count = 0usize;
    for value in &values {
        let count = values.iter().filter(|other| **other == *value).count();
        if count > best_count {
            best = *value;
            best_count = count;
        }
    }
    Some(best)
}

pub fn jet_stats_multimode(values: Vec<f64>) -> Vec<f64> {
    let Some(first) = values.first().copied() else {
        return Vec::new();
    };
    let best_count = values
        .iter()
        .map(|value| values.iter().filter(|other| **other == *value).count())
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    for value in std::iter::once(first).chain(values.iter().copied().skip(1)) {
        let count = values.iter().filter(|other| **other == value).count();
        if count == best_count && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

pub fn jet_stats_pvariance(values: Vec<f64>) -> Option<f64> {
    let mean = jet_stats_mean_value(&values)?;
    Some(values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / values.len() as f64)
}

pub fn jet_stats_variance(values: Vec<f64>) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = jet_stats_mean_value(&values)?;
    Some(values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64)
}

pub fn jet_stats_pstdev(values: Vec<f64>) -> Option<f64> {
    jet_stats_pvariance(values).map(f64::sqrt)
}

pub fn jet_stats_stdev(values: Vec<f64>) -> Option<f64> {
    jet_stats_variance(values).map(f64::sqrt)
}

pub fn jet_stats_covariance(left: Vec<f64>, right: Vec<f64>) -> Option<f64> {
    if left.len() < 2 || left.len() != right.len() {
        return None;
    }
    let left_mean = jet_stats_mean_value(&left)?;
    let right_mean = jet_stats_mean_value(&right)?;
    Some(
        left.iter()
            .zip(right.iter())
            .map(|(a, b)| (a - left_mean) * (b - right_mean))
            .sum::<f64>()
            / (left.len() - 1) as f64,
    )
}

pub fn jet_stats_covariance_population(left: Vec<f64>, right: Vec<f64>) -> Option<f64> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let left_mean = jet_stats_mean_value(&left)?;
    let right_mean = jet_stats_mean_value(&right)?;
    Some(
        left.iter()
            .zip(right.iter())
            .map(|(a, b)| (a - left_mean) * (b - right_mean))
            .sum::<f64>()
            / left.len() as f64,
    )
}

pub fn jet_stats_correlation(left: Vec<f64>, right: Vec<f64>) -> Option<f64> {
    let covariance = jet_stats_covariance(left.clone(), right.clone())?;
    let left_deviation = jet_stats_stdev(left)?;
    let right_deviation = jet_stats_stdev(right)?;
    (left_deviation != 0.0 && right_deviation != 0.0)
        .then_some(covariance / (left_deviation * right_deviation))
}

pub fn jet_stats_kde(values: Vec<f64>, bandwidth: f64) -> Vec<f64> {
    if values.is_empty() || bandwidth <= 0.0 {
        return Vec::new();
    }
    let scale = values.len() as f64 * bandwidth * (2.0 * std::f64::consts::PI).sqrt();
    values
        .iter()
        .map(|point| {
            values
                .iter()
                .map(|value| {
                    let z = (point - value) / bandwidth;
                    (-0.5 * z * z).exp()
                })
                .sum::<f64>()
                / scale
        })
        .collect()
}

pub fn jet_stats_kde_random(values: Vec<f64>, bandwidth: f64, count: i64) -> Vec<f64> {
    let density = jet_stats_kde(values, bandwidth);
    if density.is_empty() || count <= 0 {
        return Vec::new();
    }
    (0..count)
        .map(|index| density[index as usize % density.len()])
        .collect()
}

pub fn jet_stats_min(values: Vec<f64>) -> Option<f64> {
    values.into_iter().reduce(f64::min)
}

pub fn jet_stats_max(values: Vec<f64>) -> Option<f64> {
    values.into_iter().reduce(f64::max)
}

pub fn jet_stats_range(values: Vec<f64>) -> Option<f64> {
    Some(jet_stats_max(values.clone())? - jet_stats_min(values)?)
}

pub fn jet_stats_sumprod(left: Vec<f64>, right: Vec<f64>) -> Option<f64> {
    (left.len() == right.len()).then(|| left.into_iter().zip(right).map(|(a, b)| a * b).sum())
}

pub fn jet_stats_weighted_mean(values: Vec<f64>, weights: Vec<f64>) -> Option<f64> {
    if values.is_empty() || values.len() != weights.len() || weights.iter().any(|weight| *weight < 0.0) {
        return None;
    }
    let total_weight = weights.iter().sum::<f64>();
    (total_weight != 0.0).then(|| {
        values
            .into_iter()
            .zip(weights)
            .map(|(value, weight)| value * weight)
            .sum::<f64>()
            / total_weight
    })
}

pub fn jet_stats_quantiles(values: Vec<f64>, count: i64) -> Vec<f64> {
    if count <= 0 || values.is_empty() {
        return Vec::new();
    }
    (1..count)
        .filter_map(|index| jet_stats_quantile_value(&values, index as f64 / count as f64))
        .collect()
}

pub fn jet_stats_iqr(values: Vec<f64>) -> Option<f64> {
    Some(jet_stats_quantile_value(&values, 0.75)? - jet_stats_quantile_value(&values, 0.25)?)
}

pub fn jet_stats_mad(values: Vec<f64>) -> Option<f64> {
    let median = jet_stats_median(values.clone())?;
    jet_stats_median(values.into_iter().map(|value| (value - median).abs()).collect())
}

pub fn jet_stats_mean_abs_deviation(values: Vec<f64>) -> Option<f64> {
    let mean = jet_stats_mean_value(&values)?;
    Some(values.iter().map(|value| (value - mean).abs()).sum::<f64>() / values.len() as f64)
}

pub fn jet_stats_skew(values: Vec<f64>) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let mean = jet_stats_mean_value(&values)?;
    let deviation = jet_stats_stdev(values.clone())?;
    (deviation != 0.0).then(|| {
        values
            .iter()
            .map(|value| ((value - mean) / deviation).powi(3))
            .sum::<f64>()
            / values.len() as f64
    })
}

pub fn jet_stats_kurtosis(values: Vec<f64>) -> Option<f64> {
    if values.len() < 4 {
        return None;
    }
    let mean = jet_stats_mean_value(&values)?;
    let deviation = jet_stats_stdev(values.clone())?;
    (deviation != 0.0).then(|| {
        values
            .iter()
            .map(|value| ((value - mean) / deviation).powi(4))
            .sum::<f64>()
            / values.len() as f64
            - 3.0
    })
}

pub fn jet_stats_spearman(left: Vec<f64>, right: Vec<f64>) -> Option<f64> {
    jet_stats_correlation(jet_stats_rank(left), jet_stats_rank(right))
}

pub fn jet_stats_residuals(left: Vec<f64>, right: Vec<f64>) -> Vec<f64> {
    if left.len() < 2 || left.len() != right.len() {
        return Vec::new();
    }
    let mean_x = match jet_stats_mean_value(&left) {
        Some(value) => value,
        None => return Vec::new(),
    };
    let mean_y = match jet_stats_mean_value(&right) {
        Some(value) => value,
        None => return Vec::new(),
    };
    let numerator = left
        .iter()
        .zip(right.iter())
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum::<f64>();
    let denominator = left.iter().map(|x| (x - mean_x).powi(2)).sum::<f64>();
    if denominator == 0.0 {
        return Vec::new();
    }
    let slope = numerator / denominator;
    let intercept = mean_y - slope * mean_x;
    left.iter()
        .zip(right.iter())
        .map(|(x, y)| y - (slope * x + intercept))
        .collect()
}

pub fn jet_stats_histogram(values: Vec<f64>, bins: i64) -> Vec<i64> {
    if bins <= 0 || values.is_empty() {
        return Vec::new();
    }
    let Some(low) = jet_stats_min(values.clone()) else {
        return Vec::new();
    };
    let Some(high) = jet_stats_max(values.clone()) else {
        return Vec::new();
    };
    let mut out = vec![0i64; bins as usize];
    if high == low {
        out[0] = values.len() as i64;
        return out;
    }
    let span = high - low;
    for value in values {
        let mut index = ((value - low) / span * bins as f64) as i64;
        index = index.clamp(0, bins - 1);
        out[index as usize] += 1;
    }
    out
}

pub fn jet_stats_winsorize(values: Vec<f64>, probability: f64) -> Vec<f64> {
    let Some(low) = jet_stats_quantile_value(&values, probability) else {
        return values;
    };
    let Some(high) = jet_stats_quantile_value(&values, 1.0 - probability) else {
        return values;
    };
    values.into_iter().map(|value| value.clamp(low, high)).collect()
}

/// Integer product for `math.prod`'s default exact integer carrier.
pub fn jet_std_math_prod_int(values: Vec<i64>) -> i64 {
    values.iter().copied().product()
}

/// Dot product over the shorter input, matching `math.sumprod`'s core shape.
pub fn jet_std_math_sumprod(left: Vec<f64>, right: Vec<f64>) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(a, b)| a * b)
        .sum()
}

pub fn jet_std_math_ln_1p(value: f64) -> f64 {
    value.ln_1p()
}

pub fn jet_std_math_log(value: f64, base: f64) -> f64 {
    value.log(base)
}

pub fn jet_std_math_copysign(value: f64, sign: f64) -> f64 {
    value.copysign(sign)
}

pub fn jet_std_math_signum(value: f64) -> f64 {
    value.signum()
}

pub fn jet_std_math_fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}

pub fn jet_std_math_identity_f64(value: f64) -> f64 {
    value
}

pub fn jet_std_math_zero_f64(_: f64) -> f64 {
    0.0
}

pub fn jet_std_math_lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn jet_std_math_degrees(value: f64) -> f64 {
    value.to_degrees()
}

pub fn jet_std_math_radians(value: f64) -> f64 {
    value.to_radians()
}

pub fn jet_std_math_is_even(value: i64) -> bool {
    value % 2 == 0
}

pub fn jet_std_math_is_odd(value: i64) -> bool {
    value % 2 != 0
}

pub fn jet_std_math_checked_abs(value: i64) -> Option<i64> {
    value.checked_abs()
}
pub fn jet_std_math_checked_add(left: i64, right: i64) -> Option<i64> {
    left.checked_add(right)
}

pub fn jet_std_math_saturating_add(left: i64, right: i64) -> i64 {
    left.saturating_add(right)
}

pub fn jet_std_math_checked_neg(value: i64) -> Option<i64> {
    value.checked_neg()
}

pub fn jet_std_math_checked_div(left: i64, right: i64) -> Option<i64> {
    left.checked_div(right)
}

pub fn jet_std_math_checked_rem(left: i64, right: i64) -> Option<i64> {
    left.checked_rem(right)
}

pub fn jet_std_math_is_normal(value: f64) -> bool {
    value.is_normal()
}
pub fn jet_std_math_is_subnormal(value: f64) -> bool {
    value.is_subnormal()
}

pub fn jet_std_math_is_canonical(value: f64) -> bool {
    value.is_finite() || value.is_nan()
}

pub fn jet_std_math_is_signed(value: f64) -> bool {
    value.is_sign_negative()
}

pub fn jet_std_math_is_zero(value: f64) -> bool {
    value == 0.0
}

pub fn jet_std_math_is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

pub fn jet_std_math_sign_bit(value: f64) -> bool {
    value.is_sign_negative()
}
pub fn jet_std_math_next_up(value: f64) -> f64 {
    value.next_up()
}

pub fn jet_std_math_next_down(value: f64) -> f64 {
    value.next_down()
}

pub fn jet_std_math_radix(_value: f64) -> i64 {
    2
}

pub fn jet_std_math_zero() -> f64 {
    0.0
}

pub fn jet_std_math_copy(value: f64) -> f64 {
    value
}
pub fn jet_std_math_cot(value: f64) -> f64 {
    1.0 / value.tan()
}

pub fn jet_std_math_inv(value: f64) -> f64 {
    1.0 / value
}
pub fn jet_std_math_sin_cos(value: f64) -> (f64, f64) {
    value.sin_cos()
}

pub fn jet_std_math_modf(value: f64) -> (f64, f64) {
    (value.fract(), value.trunc())
}

pub fn jet_std_math_frexp(value: f64) -> (f64, i64) {
    let exponent = jet_std_math_ilogb(value).unwrap_or(0);
    let fraction = if value == 0.0 || !value.is_finite() {
        value
    } else {
        jet_std_math_ldexp(value, -exponent)
    };
    (fraction, exponent)
}

pub fn jet_std_math_div_mod(left: i64, right: i64) -> (i64, i64) {
    (left.div_euclid(right), left.rem_euclid(right))
}

pub fn jet_std_math_div_rem(left: i64, right: i64) -> (i64, i64) {
    (left / right, left % right)
}

/// IEEE-754 bit conversions share this Prelude symbol across AOT, resident
/// JIT, and the evaluator.  The integer carrier is intentionally word-sized;
/// the evaluator projects larger exact `Int` values modulo two's-complement
/// `u64` before calling this function.
pub fn jet_std_math_to_bits(value: f64) -> i64 {
    value.to_bits() as i64
}

pub fn jet_std_math_from_bits(bits: i64) -> f64 {
    f64::from_bits(bits as u64)
}

pub fn jet_std_math_min_i64(left: i64, right: i64) -> i64 {
    left.min(right)
}

pub fn jet_std_math_max_i64(left: i64, right: i64) -> i64 {
    left.max(right)
}

/// `round` uses nearest-integer ties away from zero, for both signs.
pub fn jet_std_math_round(value: f64) -> i64 {
    value.round() as i64
}

pub fn jet_std_math_clamp_i64(value: i64, low: i64, high: i64) -> i64 {
    value.clamp(low, high)
}

pub fn jet_std_math_min_f64(left: f64, right: f64) -> f64 {
    match (left.is_nan(), right.is_nan()) {
        (true, false) => right,
        (false, true) => left,
        _ => left.min(right),
    }
}

pub fn jet_std_math_max_f64(left: f64, right: f64) -> f64 {
    match (left.is_nan(), right.is_nan()) {
        (true, false) => right,
        (false, true) => left,
        _ => left.max(right),
    }
}

pub fn jet_std_math_clamp_f64(value: f64, low: f64, high: f64) -> f64 {
    value.clamp(low, high)
}

pub fn jet_std_math_min_f32(left: f32, right: f32) -> f32 {
    match (left.is_nan(), right.is_nan()) {
        (true, false) => right,
        (false, true) => left,
        _ => left.min(right),
    }
}

pub fn jet_std_math_max_f32(left: f32, right: f32) -> f32 {
    match (left.is_nan(), right.is_nan()) {
        (true, false) => right,
        (false, true) => left,
        _ => left.max(right),
    }
}

pub fn jet_std_math_clamp_f32(value: f32, low: f32, high: f32) -> f32 {
    value.clamp(low, high)
}

/// The largest whole number whose square is at most `value`, or absent when
/// there is none. A negative number has no whole square root.
pub fn jet_std_math_isqrt(value: i64) -> Option<i64> {
    if value < 0 {
        return None;
    }
    let mut root = (value as f64).sqrt() as i64;
    // The float square root can land either side on large values, so walk back
    // to the exact answer without saturating a product into a false equality.
    while root > 0 && root > value / root {
        root -= 1;
    }
    while root + 1 <= value / (root + 1) {
        root += 1;
    }
    Some(root)
}

#[cfg(test)]
mod fixed_isqrt_tests {
    #[test]
    fn fixed_isqrt_handles_maximum_and_square_boundaries() {
        assert_eq!(super::jet_std_math_isqrt(i64::MAX), Some(3037000499));
        for (value, expected) in [
            (-1, None), (0, Some(0)), (1, Some(1)),
            (15, Some(3)), (16, Some(4)), (17, Some(4)),
        ] {
            assert_eq!(super::jet_std_math_isqrt(value), expected);
        }
        let square = 3037000499i64 * 3037000499;
        assert_eq!(super::jet_std_math_isqrt(square - 1), Some(3037000498));
        assert_eq!(super::jet_std_math_isqrt(square), Some(3037000499));
    }
}

/// The product of every whole number from 1 to `value`, or absent when there is
/// no answer: a negative input, or a result past the range. 21 factorial is
/// already too big.
pub fn jet_std_math_factorial(value: i64) -> Option<i64> {
    if value < 0 {
        return None;
    }
    let mut total: i64 = 1;
    let mut step: i64 = 2;
    while step <= value {
        total = total.checked_mul(step)?;
        step += 1;
    }
    Some(total)
}

pub fn jet_std_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
pub fn jet_std_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_std_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

/// Ways to choose `k` items from `n`, or absent when the product leaves the
/// range or either side is negative.
pub fn jet_std_math_binomial(n: i64, k: i64) -> Option<i64> {
    if n < 0 || k < 0 || k > n {
        return None;
    }
    let k = k.min(n - k);
    let mut out: i64 = 1;
    let mut i: i64 = 1;
    while i <= k {
        out = out.checked_mul(n - k + i)?.checked_div(i)?;
        i += 1;
    }
    Some(out)
}
pub fn jet_std_math_perm(n: i64, k: i64) -> Option<i64> {
    if n < 0 || k < 0 || k > n {
        return None;
    }
    let mut out = 1i64;
    let mut i = 0i64;
    while i < k {
        out = out.checked_mul(n - i)?;
        i += 1;
    }
    Some(out)
}
pub fn jet_std_math_rising_factorial(n: i64, k: i64) -> Option<i64> {
    if k < 0 {
        return None;
    }
    let mut out = 1i64;
    let mut step = 0i64;
    while step < k {
        out = out.checked_mul(n.checked_add(step)?)?;
        step += 1;
    }
    Some(out)
}

pub fn jet_std_math_multinomial(parts: Vec<i64>) -> Option<i64> {
    let mut remaining = 0i64;
    for part in &parts {
        if *part < 0 {
            return None;
        }
        remaining = remaining.checked_add(*part)?;
    }
    let mut out = 1i64;
    for part in parts {
        let factor = jet_std_math_binomial(remaining, part)?;
        out = out.checked_mul(factor)?;
        remaining -= part;
    }
    Some(out)
}
pub fn jet_comb_chain(left: Vec<i64>, right: Vec<i64>) -> Vec<i64> {
    left.into_iter().chain(right).collect()
}

pub fn jet_comb_compress(items: Vec<i64>, mask: Vec<bool>) -> Vec<i64> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| mask.get(index).copied().filter(|selected| *selected).map(|_| value))
        .collect()
}

pub fn jet_comb_drop(items: Vec<i64>, count: i64) -> Vec<i64> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| (index as i64 >= count).then_some(value))
        .collect()
}

pub fn jet_comb_takewhile(items: Vec<i64>, pred_nonneg: bool) -> Vec<i64> {
    items
        .into_iter()
        .take_while(|value| (*value >= 0) == pred_nonneg)
        .collect()
}

pub fn jet_comb_dropwhile(items: Vec<i64>, pred_nonneg: bool) -> Vec<i64> {
    let mut iter = items.into_iter();
    while let Some(value) = iter.next() {
        if (value >= 0) != pred_nonneg {
            return std::iter::once(value).chain(iter).collect();
        }
    }
    Vec::new()
}

pub fn jet_comb_filterfalse(items: Vec<i64>, pred_nonneg: bool) -> Vec<i64> {
    items
        .into_iter()
        .filter(|value| (*value >= 0) != pred_nonneg)
        .collect()
}

pub fn jet_comb_islice(items: Vec<i64>, start: i64, stop: i64, step: i64) -> Vec<i64> {
    if step <= 0 {
        return Vec::new();
    }
    let start = start.max(0) as usize;
    let stop = stop.max(0) as usize;
    items
        .into_iter()
        .skip(start)
        .take(stop.saturating_sub(start))
        .step_by(step as usize)
        .collect()
}

pub fn jet_comb_unique(items: Vec<i64>) -> Vec<i64> {
    let mut out = Vec::new();
    for value in items {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

pub fn jet_comb_repeat(value: i64, times: i64) -> Vec<i64> {
    if times <= 0 {
        return Vec::new();
    }
    vec![value; times as usize]
}

pub fn jet_comb_count_from(start: i64, step: i64, count: i64) -> Vec<i64> {
    (0..count.max(0))
        .map(|index| start.saturating_add(step.saturating_mul(index)))
        .collect()
}

pub fn jet_comb_cycle(items: Vec<i64>, times: i64) -> Vec<i64> {
    if times <= 0 {
        return Vec::new();
    }
    items
        .iter()
        .copied()
        .cycle()
        .take(items.len().saturating_mul(times as usize))
        .collect()
}

pub fn jet_comb_accumulate(items: Vec<i64>) -> Vec<i64> {
    let mut total = 0i64;
    items
        .into_iter()
        .map(|value| {
            total = total.saturating_add(value);
            total
        })
        .collect()
}

pub fn jet_comb_reverse(mut items: Vec<i64>) -> Vec<i64> {
    items.reverse();
    items
}
pub fn jet_comb_permutations(items: Vec<i64>, k: i64) -> Vec<Vec<i64>> {
    if k < 0 || k as usize > items.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut used = vec![false; items.len()];
    let mut current = Vec::new();
    jet_comb_permutations_rec(&items, k as usize, &mut used, &mut current, &mut out);
    out
}

fn jet_comb_permutations_rec(
    items: &[i64],
    target: usize,
    used: &mut [bool],
    current: &mut Vec<i64>,
    out: &mut Vec<Vec<i64>>,
) {
    if current.len() == target {
        out.push(current.clone());
        return;
    }
    for (index, value) in items.iter().copied().enumerate() {
        if !used[index] {
            used[index] = true;
            current.push(value);
            jet_comb_permutations_rec(items, target, used, current, out);
            current.pop();
            used[index] = false;
        }
    }
}

pub fn jet_comb_combinations(items: Vec<i64>, k: i64) -> Vec<Vec<i64>> {
    if k < 0 || k as usize > items.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut current = Vec::new();
    jet_comb_combinations_rec(&items, k as usize, 0, &mut current, &mut out);
    out
}

fn jet_comb_combinations_rec(
    items: &[i64],
    target: usize,
    start: usize,
    current: &mut Vec<i64>,
    out: &mut Vec<Vec<i64>>,
) {
    if current.len() == target {
        out.push(current.clone());
        return;
    }
    for index in start..items.len() {
        current.push(items[index]);
        jet_comb_combinations_rec(items, target, index + 1, current, out);
        current.pop();
    }
}

pub fn jet_comb_combinations_with_replacement(items: Vec<i64>, k: i64) -> Vec<Vec<i64>> {
    if k < 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut current = Vec::new();
    jet_comb_combinations_replacement_rec(&items, k as usize, 0, &mut current, &mut out);
    out
}

fn jet_comb_combinations_replacement_rec(
    items: &[i64],
    target: usize,
    start: usize,
    current: &mut Vec<i64>,
    out: &mut Vec<Vec<i64>>,
) {
    if current.len() == target {
        out.push(current.clone());
        return;
    }
    for index in start..items.len() {
        current.push(items[index]);
        jet_comb_combinations_replacement_rec(items, target, index, current, out);
        current.pop();
    }
}

pub fn jet_comb_product(items: Vec<i64>, repeat: i64) -> Vec<Vec<i64>> {
    if repeat <= 0 {
        return vec![Vec::new()];
    }
    let mut out = vec![Vec::new()];
    for _ in 0..repeat {
        let mut next = Vec::new();
        for row in &out {
            for value in &items {
                let mut expanded = row.clone();
                expanded.push(*value);
                next.push(expanded);
            }
        }
        out = next;
    }
    out
}

pub fn jet_comb_cartesian(left: Vec<i64>, right: Vec<i64>) -> Vec<Vec<i64>> {
    left.into_iter()
        .flat_map(|a| right.iter().copied().map(move |b| vec![a, b]))
        .collect()
}

pub fn jet_comb_pairwise(items: Vec<i64>) -> Vec<Vec<i64>> {
    items
        .windows(2)
        .map(|window| window.to_vec())
        .collect()
}

pub fn jet_comb_batched(items: Vec<i64>, size: i64) -> Vec<Vec<i64>> {
    if size <= 0 {
        return Vec::new();
    }
    items
        .chunks(size as usize)
        .map(|chunk| chunk.to_vec())
        .collect()
}

pub fn jet_comb_groupby(items: Vec<i64>) -> Vec<Vec<i64>> {
    let mut out: Vec<Vec<i64>> = Vec::new();
    for value in items {
        if out.last().is_some_and(|group| group.last() == Some(&value)) {
            out.last_mut().expect("last group exists").push(value);
        } else {
            out.push(vec![value]);
        }
    }
    out
}

pub fn jet_comb_tee(items: Vec<i64>, copies: i64) -> Vec<Vec<i64>> {
    if copies <= 0 {
        return Vec::new();
    }
    (0..copies).map(|_| items.clone()).collect()
}

pub fn jet_comb_zip_longest(left: Vec<i64>, right: Vec<i64>, fill: i64) -> Vec<Vec<i64>> {
    let length = left.len().max(right.len());
    (0..length)
        .map(|index| {
            vec![
                left.get(index).copied().unwrap_or(fill),
                right.get(index).copied().unwrap_or(fill),
            ]
        })
        .collect()
}

pub fn jet_comb_powerset(items: Vec<i64>) -> Vec<Vec<i64>> {
    let mut out = vec![Vec::new()];
    for value in items {
        let mut next = out.clone();
        for mut row in out {
            row.push(value);
            next.push(row);
        }
        out = next;
    }
    out
}

pub fn jet_comb_windows(items: Vec<i64>, size: i64) -> Vec<Vec<i64>> {
    if size <= 0 || size as usize > items.len() {
        return Vec::new();
    }
    items
        .windows(size as usize)
        .map(|window| window.to_vec())
        .collect()
}

pub fn jet_comb_flatten(groups: Vec<Vec<i64>>) -> Vec<i64> {
    groups.into_iter().flatten().collect()
}

pub fn jet_comb_starmap(rows: Vec<Vec<i64>>) -> Vec<i64> {
    rows.into_iter()
        .map(|row| row.into_iter().fold(0i64, |total, value| total.saturating_add(value)))
        .collect()
}
fn jet_coll_sift_down(values: &mut [i64], mut index: usize) {
    loop {
        let left = index.saturating_mul(2).saturating_add(1);
        if left >= values.len() {
            return;
        }
        let mut smallest = index;
        if values[left] < values[smallest] {
            smallest = left;
        }
        let right = left + 1;
        if right < values.len() && values[right] < values[smallest] {
            smallest = right;
        }
        if smallest == index {
            return;
        }
        values.swap(index, smallest);
        index = smallest;
    }
}

fn jet_coll_sift_up(values: &mut [i64], mut index: usize) {
    while index > 0 {
        let parent = (index - 1) / 2;
        if values[index] >= values[parent] {
            return;
        }
        values.swap(index, parent);
        index = parent;
    }
}

pub fn jet_coll_heapify(values: &[i64]) -> Vec<i64> {
    let mut values = values.to_vec();
    if values.len() > 1 {
        let mut index = values.len() / 2;
        loop {
            jet_coll_sift_down(&mut values, index);
            if index == 0 {
                break;
            }
            index -= 1;
        }
    }
    values
}

pub fn jet_coll_heappush(values: &[i64], value: i64) -> Vec<i64> {
    let mut values = values.to_vec();
    values.push(value);
    let index = values.len() - 1;
    jet_coll_sift_up(&mut values, index);
    values
}
pub fn jet_coll_heappop_pure(values: &[i64]) -> (Vec<i64>, Option<i64>) {
    if values.is_empty() {
        return (Vec::new(), None);
    }
    if values.len() == 1 {
        return (Vec::new(), Some(values[0]));
    }
    let mut heap = Vec::with_capacity(values.len() - 1);
    heap.push(values[values.len() - 1]);
    heap.extend_from_slice(&values[1..values.len() - 1]);
    let value = values[0];
    jet_coll_sift_down(&mut heap, 0);
    (heap, Some(value))
}

pub fn jet_coll_heappushpop_pure(values: &[i64], value: i64) -> (Vec<i64>, i64) {
    if values.is_empty() || value <= values[0] {
        return (values.to_vec(), value);
    }
    let popped = values[0];
    let mut heap = Vec::with_capacity(values.len());
    heap.push(value);
    heap.extend_from_slice(&values[1..]);
    jet_coll_sift_down(&mut heap, 0);
    (heap, popped)
}

pub fn jet_coll_heapreplace_pure(values: &[i64], value: i64) -> (Vec<i64>, Option<i64>) {
    if values.is_empty() {
        return (vec![value], None);
    }
    let popped = values[0];
    let mut heap = Vec::with_capacity(values.len());
    heap.push(value);
    heap.extend_from_slice(&values[1..]);
    jet_coll_sift_down(&mut heap, 0);
    (heap, Some(popped))
}

pub fn jet_coll_bisect_left(values: &[i64], value: i64) -> i64 {
    values.partition_point(|item| *item < value) as i64
}

pub fn jet_coll_bisect_right(values: &[i64], value: i64) -> i64 {
    values.partition_point(|item| *item <= value) as i64
}

pub fn jet_coll_insort_left(values: &[i64], value: i64) -> Vec<i64> {
    let index = values.partition_point(|item| *item < value);
    let mut values = values.to_vec();
    values.insert(index, value);
    values
}

pub fn jet_coll_insort_right(values: &[i64], value: i64) -> Vec<i64> {
    let index = values.partition_point(|item| *item <= value);
    let mut values = values.to_vec();
    values.insert(index, value);
    values
}

pub fn jet_coll_merge_sorted(left: &[i64], right: &[i64]) -> Vec<i64> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        if left[left_index] <= right[right_index] {
            out.push(left[left_index]);
            left_index += 1;
        } else {
            out.push(right[right_index]);
            right_index += 1;
        }
    }
    out.extend_from_slice(&left[left_index..]);
    out.extend_from_slice(&right[right_index..]);
    out
}

pub fn jet_coll_nsmallest(n: i64, values: &[i64]) -> Vec<i64> {
    if n <= 0 {
        return Vec::new();
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    values.truncate(n as usize);
    values
}

pub fn jet_coll_nlargest(n: i64, values: &[i64]) -> Vec<i64> {
    if n <= 0 {
        return Vec::new();
    }
    let mut values = values.to_vec();
    values.sort_unstable_by(|left, right| right.cmp(left));
    values.truncate(n as usize);
    values
}


pub fn jet_std_math_fmod(value: f64, divisor: f64) -> f64 {
    value % divisor
}

pub fn jet_std_math_remainder(value: f64, divisor: f64) -> f64 {
    if !value.is_finite() || !divisor.is_finite() || divisor == 0.0 {
        return f64::NAN;
    }
    let quotient = value / divisor;
    let lower = quotient.floor();
    let fraction = quotient - lower;
    let nearest = if fraction < 0.5 {
        lower
    } else if fraction > 0.5 || lower % 2.0 != 0.0 {
        lower + 1.0
    } else {
        lower
    };
    value - nearest * divisor
}

pub fn jet_std_math_isclose(
    left: f64,
    right: f64,
    relative_tolerance: f64,
    absolute_tolerance: f64,
) -> bool {
    if relative_tolerance < 0.0 || absolute_tolerance < 0.0 {
        return false;
    }
    if left == right {
        return true;
    }
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    (left - right).abs()
        <= (relative_tolerance * left.abs().max(right.abs())).max(absolute_tolerance)
}

pub fn jet_std_math_dist(left: Vec<f64>, right: Vec<f64>) -> f64 {
    if left.len() != right.len() {
        return f64::NAN;
    }
    left.into_iter()
        .zip(right)
        .map(|(a, b)| {
            let delta = a - b;
            delta * delta
        })
        .sum::<f64>()
        .sqrt()
}


pub fn jet_std_math_digits(value: i64) -> i64 {
    if value == 0 {
        return 1;
    }
    let mut n = value.unsigned_abs();
    let mut count = 0i64;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

pub fn jet_std_math_leading_ones(value: i64) -> i64 {
    (value as u64).leading_ones() as i64
}

pub fn jet_std_math_trailing_ones(value: i64) -> i64 {
    (value as u64).trailing_ones() as i64
}

pub fn jet_std_math_cmp(a: f64, b: f64) -> i64 {
    match a.partial_cmp(&b) {
        Some(std::cmp::Ordering::Less) => -1,
        Some(std::cmp::Ordering::Equal) => 0,
        Some(std::cmp::Ordering::Greater) => 1,
        None => {
            // NaN sorts after every finite value, matching a stable total order
            // for audit output: NaN vs NaN is equal; NaN vs number is greater.
            if a.is_nan() && b.is_nan() {
                0
            } else if a.is_nan() {
                1
            } else {
                -1
            }
        }
    }
}

pub fn jet_std_math_ulp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    let x = x.abs();
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let bits = x.to_bits();
    let next = f64::from_bits(bits + 1);
    next - x
}

pub fn jet_std_math_significand(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 {
        // Subnormal: scale into the normal significand range.
        let mut v = x.abs();
        while v < 1.0 {
            v *= 2.0;
        }
        return if x.is_sign_negative() { -v } else { v };
    }
    let frac_bits = bits & ((1u64 << 52) - 1);
    let sig = f64::from_bits((0x3ffu64 << 52) | frac_bits);
    if x.is_sign_negative() {
        -sig
    } else {
        sig
    }
}

pub fn jet_std_math_ilogb(x: f64) -> Option<i64> {
    if x == 0.0 || !x.is_finite() {
        return None;
    }
    Some(x.abs().log2().floor() as i64)
}

pub fn jet_std_math_logb(x: f64) -> f64 {
    match jet_std_math_ilogb(x) {
        Some(e) => e as f64,
        None if x == 0.0 => f64::NEG_INFINITY,
        None => f64::INFINITY,
    }
}

pub fn jet_std_math_ldexp(x: f64, exp: i64) -> f64 {
    if !x.is_finite() || x == 0.0 || exp == 0 {
        return x;
    }
    x * 2f64.powi(exp.clamp(-2099, 2099) as i32)
}

pub fn jet_std_math_next_after(x: f64, toward: f64) -> f64 {
    if x.is_nan() || toward.is_nan() {
        return f64::NAN;
    }
    if x == toward {
        return toward;
    }
    if x == 0.0 {
        return if toward > 0.0 {
            f64::from_bits(1)
        } else {
            -f64::from_bits(1)
        };
    }
    let bits = x.to_bits();
    let next = if (toward > x) == x.is_sign_positive() {
        bits + 1
    } else {
        bits - 1
    };
    f64::from_bits(next)
}

/// Abramowitz & Stegun 7.1.26 — max error under 1.5e-7 on the real line.
pub fn jet_std_math_erf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.is_infinite() {
        return x.signum();
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let poly = t
        * (0.254829592
            + t * (-0.284496736
                + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    sign * (1.0 - poly * (-ax * ax).exp())
}

pub fn jet_std_math_erfc(x: f64) -> f64 {
    1.0 - jet_std_math_erf(x)
}

/// Lanczos approximation for Γ(x) on positive reals; reflected for (0,1).
pub fn jet_std_math_gamma(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x <= 0.0 {
        if x == x.floor() {
            return f64::NAN;
        }
        // Reflection: Γ(z)Γ(1−z) = π / sin(πz)
        return std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * jet_std_math_gamma(1.0 - x));
    }
    // Lanczos g=7, n=9
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.99999999999980993,
        676.5203681218851,
        -1259.1392167224028,
        771.32342877765313,
        -176.61502916214059,
        12.507343278686905,
        -0.13857109526572012,
        9.984369654078991e-6,
        1.5056327351493116e-7,
    ];
    let mut z = x;
    if z < 0.5 {
        return std::f64::consts::PI / ((std::f64::consts::PI * z).sin() * jet_std_math_gamma(1.0 - z));
    }
    z -= 1.0;
    let mut xacc = C[0];
    for i in 1..9 {
        xacc += C[i] / (z + i as f64);
    }
    let t = z + G + 0.5;
    (2.0 * std::f64::consts::PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * xacc
}

pub fn jet_std_math_lgamma(x: f64) -> f64 {
    let g = jet_std_math_gamma(x);
    if g.is_nan() || g <= 0.0 {
        f64::NAN
    } else {
        g.ln()
    }
}


pub fn jet_std_math_checked_sub(left: i64, right: i64) -> Option<i64> {
    left.checked_sub(right)
}

pub fn jet_std_math_checked_mul(left: i64, right: i64) -> Option<i64> {
    left.checked_mul(right)
}

pub fn jet_std_math_saturating_sub(left: i64, right: i64) -> i64 {
    left.saturating_sub(right)
}

pub fn jet_std_math_saturating_mul(left: i64, right: i64) -> i64 {
    left.saturating_mul(right)
}

pub fn jet_std_math_tau() -> f64 {
    std::f64::consts::TAU
}

pub fn jet_std_math_hypot3(a: f64, b: f64, c: f64) -> f64 {
    a.hypot(b).hypot(c)
}

pub fn jet_std_math_midpoint(a: f64, b: f64) -> f64 {
    a + (b - a) * 0.5
}

pub fn jet_std_math_gcd_many(values: &[i64]) -> i64 {
    values.iter().copied().fold(0, jet_std_math_gcd)
}

pub fn jet_std_math_lcm_many(values: &[i64]) -> i64 {
    let mut values = values.iter().copied();
    let Some(first) = values.next() else {
        return 0;
    };
    values.fold(first, jet_std_math_lcm)
}

pub fn jet_std_math_powmod(base: i64, exp: i64, modulus: i64) -> Option<i64> {
    if modulus <= 0 || exp < 0 {
        return None;
    }
    let mut result = 1i64 % modulus;
    let mut factor = base.rem_euclid(modulus);
    let mut power = exp;
    while power > 0 {
        if power & 1 == 1 {
            result = result.checked_mul(factor)?.rem_euclid(modulus);
        }
        factor = factor.checked_mul(factor)?.rem_euclid(modulus);
        power >>= 1;
    }
    Some(result)
}

pub fn jet_std_math_sum_int(values: &[i64]) -> i64 {
    values.iter().copied().sum()
}

pub fn jet_std_math_prod_int_ref(values: &[i64]) -> i64 {
    values.iter().copied().product()
}

pub fn jet_std_math_abs_diff(left: i64, right: i64) -> i64 {
    left.abs_diff(right) as i64
}

pub fn jet_std_math_in_range(value: i64, low: i64, high: i64) -> bool {
    value >= low && value < high
}

pub fn jet_std_math_xor(left: i64, right: i64) -> i64 {
    left ^ right
}

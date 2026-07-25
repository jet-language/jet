#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetBigInt {
    negative: bool,
    limbs: Vec<u32>, // little-endian base 10^9
}

const BI_BASE: u64 = 1_000_000_000;

impl JetBigInt {
    pub fn from_int(n: i64) -> Self {
        if n == 0 {
            return JetBigInt {
                negative: false,
                limbs: vec![0],
            };
        }
        let negative = n < 0;
        let mut v = if negative {
            (n as i128).wrapping_neg() as u64
        } else {
            n as u64
        };
        let mut limbs = Vec::new();
        while v > 0 {
            limbs.push((v % BI_BASE) as u32);
            v /= BI_BASE;
        }
        JetBigInt { negative, limbs }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        let t = s.trim();
        if t.is_empty() {
            return Err("empty BigInt string".to_string());
        }
        let (negative, body) = if let Some(rest) = t.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = t.strip_prefix('+') {
            (false, rest)
        } else {
            (false, t)
        };
        if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("invalid BigInt string `{s}`"));
        }
        let mut acc = JetBigInt {
            negative: false,
            limbs: vec![0],
        };
        for ch in body.chars() {
            let digit = ch.to_digit(10).unwrap() as u32;
            acc = acc.mul_small(10).add_small(digit);
        }
        acc.negative = negative && !(acc.limbs.len() == 1 && acc.limbs[0] == 0);
        Ok(acc)
    }

    fn normalize(mut self) -> Self {
        while self.limbs.len() > 1 && *self.limbs.last().unwrap() == 0 {
            self.limbs.pop();
        }
        if self.limbs.len() == 1 && self.limbs[0] == 0 {
            self.negative = false;
        }
        self
    }

    fn mul_small(&self, m: u32) -> Self {
        let mut carry = 0u64;
        let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
        for &limb in &self.limbs {
            let prod = limb as u64 * m as u64 + carry;
            limbs.push((prod % BI_BASE) as u32);
            carry = prod / BI_BASE;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }
        JetBigInt {
            negative: self.negative,
            limbs,
        }
        .normalize()
    }

    fn add_small(&self, n: u32) -> Self {
        self.add(&JetBigInt::from_int(n as i64))
    }

    pub fn add(&self, other: &JetBigInt) -> JetBigInt {
        if self.negative == other.negative {
            let mut carry = 0u64;
            let len = self.limbs.len().max(other.limbs.len());
            let mut limbs = Vec::with_capacity(len + 1);
            for i in 0..len {
                let a = *self.limbs.get(i).unwrap_or(&0) as u64;
                let b = *other.limbs.get(i).unwrap_or(&0) as u64;
                let sum = a + b + carry;
                limbs.push((sum % BI_BASE) as u32);
                carry = sum / BI_BASE;
            }
            if carry > 0 {
                limbs.push(carry as u32);
            }
            JetBigInt {
                negative: self.negative,
                limbs,
            }
            .normalize()
        } else {
            let cmp = self.cmp_abs(other);
            if cmp == 0 {
                JetBigInt::from_int(0)
            } else if cmp > 0 {
                self.sub_abs(other).with_sign(self.negative)
            } else {
                other.sub_abs(self).with_sign(other.negative)
            }
        }
    }

    fn with_sign(self, negative: bool) -> Self {
        JetBigInt {
            negative,
            limbs: self.limbs,
        }
    }

    pub fn sub(&self, other: &JetBigInt) -> JetBigInt {
        let mut neg_other = other.clone();
        neg_other.negative = !neg_other.negative;
        self.add(&neg_other)
    }

    fn sub_abs(&self, other: &JetBigInt) -> JetBigInt {
        let mut borrow = 0i64;
        let len = self.limbs.len();
        let mut limbs = Vec::with_capacity(len);
        for i in 0..len {
            let a = self.limbs[i] as i64;
            let b = *other.limbs.get(i).unwrap_or(&0) as i64;
            let mut cur = a - b - borrow;
            if cur < 0 {
                cur += BI_BASE as i64;
                borrow = 1;
            } else {
                borrow = 0;
            }
            limbs.push(cur as u32);
        }
        JetBigInt {
            negative: false,
            limbs,
        }
        .normalize()
    }

    fn cmp_abs(&self, other: &JetBigInt) -> i8 {
        match self.limbs.len().cmp(&other.limbs.len()) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => {
                for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
                    match a.cmp(b) {
                        std::cmp::Ordering::Greater => return 1,
                        std::cmp::Ordering::Less => return -1,
                        std::cmp::Ordering::Equal => {}
                    }
                }
                0
            }
        }
    }

    pub fn mul(&self, other: &JetBigInt) -> JetBigInt {
        let mut out = JetBigInt::from_int(0);
        for (i, &limb) in other.limbs.iter().enumerate() {
            if limb == 0 {
                continue;
            }
            let mut part = self.mul_small(limb);
            for _ in 0..i {
                part = part.mul_small(BI_BASE as u32);
            }
            out = out.add(&part);
        }
        JetBigInt {
            negative: self.negative != other.negative,
            limbs: out.limbs,
        }
        .normalize()
    }

    pub fn to_string_rep(&self) -> String {
        if self.limbs.len() == 1 && self.limbs[0] == 0 {
            return "0".to_string();
        }
        let mut s = String::new();
        let top = *self.limbs.last().unwrap();
        s.push_str(&top.to_string());
        for &limb in self.limbs.iter().rev().skip(1) {
            s.push_str(&format!("{:09}", limb));
        }
        if self.negative {
            format!("-{s}")
        } else {
            s
        }
    }
}

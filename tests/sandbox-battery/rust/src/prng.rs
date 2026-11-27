pub struct Prng(u64);

impl Prng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn bytes(&mut self, length: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(length + 8);
        while out.len() < length {
            out.extend_from_slice(&self.next_u64().to_le_bytes());
        }
        out.truncate(length);
        out
    }

    pub fn below(&mut self, limit: u64) -> u64 {
        self.next_u64() % limit.max(1)
    }
}

pub fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub count: usize,
    pub median: f64,
    pub p95: f64,
    pub slowest: f64,
    pub within: usize,
}

impl Stats {
    pub fn of(values: &[f64], limit: f64) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        Self {
            count: sorted.len(),
            median: percentile(&sorted, 0.5),
            p95: percentile(&sorted, 0.95),
            slowest: sorted.last().copied().unwrap_or(0.0),
            within: sorted.iter().filter(|value| **value <= limit).count(),
        }
    }
}

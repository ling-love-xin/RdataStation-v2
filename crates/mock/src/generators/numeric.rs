// ==================== 数值辅助函数 ====================

use super::*;

pub(crate) fn standard_normal(rng: &mut StdRng) -> f64 {
    // 取 1-U 而不是 U，避开 ln(0) 得到 -inf
    let u1 = 1.0 - rng.random::<f64>();
    let u2 = rng.random::<f64>();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Gamma 分布抽样（Marsaglia-Tsang 挤压法），要求 `shape > 0`。
///
/// Beta 分布由两个 Gamma 之比得到，所以必须支持 `shape < 1`：
/// 此时用提升变换 `x^(1/shape) · Γ(shape + 1)` 绕开主分支对 `shape ≥ 1` 的要求。
pub(crate) fn gamma_sample(shape: f64, rng: &mut StdRng) -> f64 {
    if shape < 1.0 {
        let u = 1.0 - rng.random::<f64>();
        let g = gamma_sample(shape + 1.0, rng);
        return g * u.powf(1.0 / shape);
    }
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = standard_normal(rng);
        let v = 1.0 + c * x;
        if v <= 0.0 {
            continue;
        }
        let v = v * v * v;
        let u: f64 = rng.random::<f64>();
        // 挤压判据：先试廉价的对数判据，落空再走完整判据
        if u < 1.0 - 0.0331 * x.powi(4) || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
            return d * v;
        }
    }
}

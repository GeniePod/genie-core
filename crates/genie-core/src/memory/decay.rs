/// Temporal decay functions for memory scoring.
///
/// Implements exponential decay: score(t) = score(0) * exp(-lambda * t)
/// where lambda = ln(2) / half_life_days.
///
/// Inspired by OpenClaw's temporal-decay.ts — clean-room Rust implementation.

/// Calculate exponential decay multiplier.
///
/// At half-life: multiplier = 0.5
/// At 2x half-life: multiplier = 0.25
/// Evergreen memories should pass age_days = 0 or skip decay entirely.
pub fn exponential_decay(age_days: f64, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 || !age_days.is_finite() {
        return 1.0;
    }

    let lambda = std::f64::consts::LN_2 / half_life_days;
    let age = age_days.max(0.0);

    (-lambda * age).exp()
}

/// Convert BM25 rank (negative = more relevant) to a 0-1 score.
///
/// SQLite FTS5 returns BM25 as negative rank:
///   rank = -10 → score = 10/11 = 0.91
///   rank = -1  → score = 1/2  = 0.50
///   rank = -0.1 → score = 0.1/1.1 = 0.09
pub fn bm25_rank_to_score(rank: f64) -> f64 {
    if !rank.is_finite() {
        return 0.001;
    }

    if rank < 0.0 {
        let relevance = -rank;
        relevance / (1.0 + relevance)
    } else {
        1.0 / (1.0 + rank)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_at_zero_is_one() {
        assert!((exponential_decay(0.0, 30.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn decay_at_half_life_is_half() {
        assert!((exponential_decay(30.0, 30.0) - 0.5).abs() < 0.001);
    }

    #[test]
    fn decay_at_double_half_life_is_quarter() {
        assert!((exponential_decay(60.0, 30.0) - 0.25).abs() < 0.001);
    }

    #[test]
    fn decay_curve_values() {
        // From OpenClaw's documentation:
        let hl = 30.0;
        assert!((exponential_decay(7.0, hl) - 0.851).abs() < 0.01);
        assert!((exponential_decay(14.0, hl) - 0.724).abs() < 0.01);
        assert!((exponential_decay(90.0, hl) - 0.125).abs() < 0.01);
        assert!((exponential_decay(180.0, hl) - 0.016).abs() < 0.01);
    }

    #[test]
    fn decay_with_zero_half_life_returns_one() {
        assert_eq!(exponential_decay(100.0, 0.0), 1.0);
    }

    #[test]
    fn bm25_negative_rank() {
        assert!((bm25_rank_to_score(-10.0) - 0.909).abs() < 0.01);
        assert!((bm25_rank_to_score(-1.0) - 0.5).abs() < 0.01);
        assert!((bm25_rank_to_score(-0.1) - 0.091).abs() < 0.01);
    }

    #[test]
    fn bm25_zero_rank() {
        assert!((bm25_rank_to_score(0.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn bm25_positive_rank() {
        assert!((bm25_rank_to_score(1.0) - 0.5).abs() < 0.01);
        assert!((bm25_rank_to_score(10.0) - 0.091).abs() < 0.01);
    }
}

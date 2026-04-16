use anyhow::Result;

use super::decay;
use super::{Memory, MemoryEntry};

/// Dreaming-inspired memory consolidation.
///
/// Three phases (inspired by OpenClaw's dreaming-phases.ts):
///
/// 1. Light: quick scan — identify frequently recalled memories
/// 2. Deep: promote high-scoring memories to permanent (evergreen)
/// 3. Prune: remove decayed memories below threshold
///
/// Called by genie-governor during night mode, or manually via CLI.

/// Promotion scoring weights (from OpenClaw's 6-component system).
/// Simplified to 4 components for V1 (no vector embeddings = no diversity/conceptual).
#[derive(Debug, Clone)]
pub struct PromotionWeights {
    pub frequency: f64,     // how often recalled
    pub relevance: f64,     // best single score
    pub recency: f64,       // how recently recalled
    pub consolidation: f64, // temporal spread of recalls
}

impl Default for PromotionWeights {
    fn default() -> Self {
        Self {
            frequency: 0.30,
            relevance: 0.35,
            recency: 0.20,
            consolidation: 0.15,
        }
    }
}

/// Scored promotion candidate.
#[derive(Debug, Clone)]
pub struct PromotionCandidate {
    pub entry: MemoryEntry,
    pub score: f64,
    pub frequency_score: f64,
    pub relevance_score: f64,
    pub recency_score: f64,
    pub consolidation_score: f64,
}

/// Run the dreaming consolidation cycle.
///
/// Returns: (promoted_count, pruned_count)
pub fn dream_cycle(
    memory: &Memory,
    weights: &PromotionWeights,
    min_score: f64,
    min_recalls: i64,
    max_promotions: usize,
    prune_threshold: f64,
) -> Result<(usize, usize)> {
    // Phase 1: Score candidates.
    let candidates = score_candidates(memory, weights, min_recalls)?;

    // Phase 2: Promote top candidates above threshold.
    let mut promoted = 0;
    for candidate in candidates.iter().take(max_promotions) {
        if candidate.score >= min_score {
            memory.mark_promoted(candidate.entry.id)?;
            promoted += 1;
            tracing::info!(
                id = candidate.entry.id,
                score = format!("{:.3}", candidate.score),
                recalls = candidate.entry.recall_count,
                content = &candidate.entry.content[..candidate.entry.content.len().min(60)],
                "memory promoted to permanent"
            );
        }
    }

    // Phase 3: Prune decayed memories.
    let pruned = memory.prune_decayed(prune_threshold)?;
    if pruned > 0 {
        tracing::info!(pruned, "decayed memories removed");
    }

    Ok((promoted, pruned))
}

/// Score all promotion candidates using weighted components.
pub fn score_candidates(
    memory: &Memory,
    weights: &PromotionWeights,
    min_recalls: i64,
) -> Result<Vec<PromotionCandidate>> {
    let entries = memory.promotion_candidates(min_recalls, 0.0, 1000)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64;

    let mut candidates: Vec<PromotionCandidate> = entries
        .into_iter()
        .map(|entry| {
            // Frequency: normalized recall count (asymptotic to 1.0).
            let frequency_score = (entry.recall_count as f64 / 10.0).min(1.0);

            // Relevance: best single score ever achieved.
            let relevance_score = entry.max_score.min(1.0);

            // Recency: exponential decay from last access.
            let days_since_access = (now_ms - entry.accessed_ms as f64) / 86_400_000.0;
            let recency_score = decay::exponential_decay(days_since_access, 14.0);

            // Consolidation: based on recall count spread (simplified).
            // More recalls = higher consolidation.
            let consolidation_score = consolidation_from_count(entry.recall_count);

            // Weighted sum.
            let score = weights.frequency * frequency_score
                + weights.relevance * relevance_score
                + weights.recency * recency_score
                + weights.consolidation * consolidation_score;

            PromotionCandidate {
                entry,
                score,
                frequency_score,
                relevance_score,
                recency_score,
                consolidation_score,
            }
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(candidates)
}

/// Consolidation score from recall count (log-scaled).
///
/// 1 recall → 0.0
/// 3 recalls → 0.50
/// 5 recalls → 0.80
/// 10+ recalls → 1.0
fn consolidation_from_count(recall_count: i64) -> f64 {
    if recall_count <= 1 {
        return 0.0;
    }
    let x = (recall_count - 1) as f64;
    (x.ln_1p() / 9.0_f64.ln_1p()).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let w = PromotionWeights::default();
        let sum = w.frequency + w.relevance + w.recency + w.consolidation;
        assert!((sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn consolidation_scaling() {
        assert_eq!(consolidation_from_count(0), 0.0);
        assert_eq!(consolidation_from_count(1), 0.0);
        assert!(consolidation_from_count(3) > 0.3);
        assert!(consolidation_from_count(5) > 0.6);
        assert!((consolidation_from_count(10) - 1.0).abs() < 0.1);
    }

    #[test]
    fn dream_cycle_integration() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static CTR: AtomicU32 = AtomicU32::new(0);
        let id = CTR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("geniepod-dream-{}-{}.db", std::process::id(), id));
        let _ = std::fs::remove_file(&path);
        let mem = Memory::open(&path).unwrap();

        // Store and recall a memory many times.
        mem.store("fact", "GeniePod uses Nemotron 4B").unwrap();
        for _ in 0..6 {
            mem.search("Nemotron", 10).unwrap();
        }

        let weights = PromotionWeights::default();
        let (promoted, _pruned) = dream_cycle(&mem, &weights, 0.1, 3, 10, 0.01).unwrap();

        assert!(promoted >= 1, "should promote frequently recalled memory");
        assert!(mem.promoted_count().unwrap() >= 1);
    }
}

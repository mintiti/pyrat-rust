//! Elo rating computation using Bradley-Terry MLE.
//!
//! Ports KataGo's Gauss-Newton optimizer to Rust with alpharat's simpler
//! interface types. Works in "strength" space internally (1 unit = e:1 odds),
//! converts to Elo at output.

use std::collections::HashMap;

use crate::GameResultRecord;

/// 400 * log10(e) — converts strength units to Elo points.
const ELO_PER_STRENGTH: f64 = 173.717_792_761_245_88;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Win/loss/draw record between two players.
#[derive(Debug, Clone)]
pub struct HeadToHead {
    pub player_a: String,
    pub player_b: String,
    pub wins_a: u32,
    pub wins_b: u32,
    pub draws: u32,
}

/// Single player's computed rating.
#[derive(Debug, Clone)]
pub struct EloRating {
    pub player_id: String,
    pub elo: f64,
    pub stderr: Option<f64>,
    pub effective_game_count: Option<f64>,
}

/// Full result of an Elo computation.
///
/// Stores the covariance matrix internally for pairwise queries (LOS,
/// difference stderr) without exposing it.
#[derive(Debug, Clone)]
pub struct EloResult {
    pub ratings: Vec<EloRating>,
    pub anchor: String,
    pub anchor_elo: f64,
    // Private: flat n×n covariance in Elo space, player index map
    elo_covariance: Option<Vec<f64>>,
    player_index: HashMap<String, usize>,
    n: usize,
}

impl EloResult {
    /// Elo difference (A - B).
    pub fn elo_difference(&self, a: &str, b: &str) -> Option<f64> {
        let ea = self.get_elo(a)?;
        let eb = self.get_elo(b)?;
        Some(ea - eb)
    }

    /// Approximate stderr on the Elo difference (A - B), from covariance.
    pub fn elo_difference_stderr(&self, a: &str, b: &str) -> Option<f64> {
        let cov = self.elo_covariance.as_ref()?;
        let &ia = self.player_index.get(a)?;
        let &ib = self.player_index.get(b)?;
        let n = self.n;
        let var = cov[ia * n + ia] - cov[ia * n + ib] - cov[ib * n + ia] + cov[ib * n + ib];
        Some(var.max(0.0).sqrt())
    }

    /// Probability that A is stronger than B (normal approximation).
    /// Returns None if uncertainty wasn't computed.
    pub fn likelihood_of_superiority(&self, a: &str, b: &str) -> Option<f64> {
        if a == b {
            return Some(0.5);
        }
        let diff = self.elo_difference(a, b)?;
        let stderr = self.elo_difference_stderr(a, b)?;
        if stderr <= 0.0 {
            return if diff > 0.0 {
                Some(1.0)
            } else if diff < 0.0 {
                Some(0.0)
            } else {
                Some(0.5)
            };
        }
        Some(normal_cdf(diff / stderr))
    }

    /// Expected win probability for A against B based on their Elo ratings.
    pub fn expected_score(&self, a: &str, b: &str) -> Option<f64> {
        let ea = self.get_elo(a)?;
        let eb = self.get_elo(b)?;
        Some(win_expectancy(ea, eb))
    }

    fn get_elo(&self, name: &str) -> Option<f64> {
        self.ratings
            .iter()
            .find(|r| r.player_id == name)
            .map(|r| r.elo)
    }
}

/// Configuration for Elo computation.
pub struct EloOptions {
    /// Player to fix at `anchor_elo`.
    pub anchor: String,
    /// Elo value for the anchor (default 1000.0).
    pub anchor_elo: f64,
    /// Whether to compute stderr / covariance.
    pub compute_uncertainty: bool,
    /// How to count draws: 0.5 means half-win each side (default).
    pub draw_weight: f64,
    /// Bayesian prior strength: virtual games at 50% vs anchor (default 2.0).
    pub prior_games: f64,
    /// Maximum optimizer iterations (default 1000).
    pub max_iterations: u32,
    /// Convergence tolerance in Elo units (default 0.001).
    pub tolerance: f64,
}

impl Default for EloOptions {
    fn default() -> Self {
        Self {
            anchor: String::new(),
            anchor_elo: 1000.0,
            compute_uncertainty: false,
            draw_weight: 0.5,
            prior_games: 2.0,
            max_iterations: 1000,
            tolerance: 0.001,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EloError {
    #[error("no game records provided")]
    NoRecords,
    #[error("need at least 2 players")]
    TooFewPlayers,
    #[error("anchor player not found: {0}")]
    AnchorNotFound(String),
    #[error("player graph is disconnected")]
    DisconnectedGraph,
    #[error("singular matrix in linear solve")]
    SingularMatrix,
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Compute Elo ratings from head-to-head records.
pub fn compute_elo(records: &[HeadToHead], options: &EloOptions) -> Result<EloResult, EloError> {
    if records.is_empty() {
        return Err(EloError::NoRecords);
    }

    // Build sorted player list and index
    let mut player_set = std::collections::BTreeSet::new();
    for r in records {
        player_set.insert(r.player_a.clone());
        player_set.insert(r.player_b.clone());
    }
    let players: Vec<String> = player_set.into_iter().collect();
    let n = players.len();
    if n < 2 {
        return Err(EloError::TooFewPlayers);
    }
    let player_idx: HashMap<String, usize> = players
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i))
        .collect();
    let anchor_idx = *player_idx
        .get(&options.anchor)
        .ok_or_else(|| EloError::AnchorNotFound(options.anchor.clone()))?;

    // Build likelihood data: each HeadToHead becomes up to 2 sigmoid terms,
    // plus prior terms for non-anchor players.
    let mut terms: Vec<SigmoidTerm> = Vec::new();

    for r in records {
        let ia = player_idx[&r.player_a];
        let ib = player_idx[&r.player_b];
        let total = r.wins_a + r.wins_b + r.draws;
        if total == 0 {
            continue;
        }
        // A's effective wins (winner side = A)
        let w_a = r.wins_a as f64 + options.draw_weight * r.draws as f64;
        if w_a > 0.0 {
            terms.push(SigmoidTerm {
                pos: ia,
                neg: ib,

                weight: w_a,
                gamecount: w_a,
            });
        }
        // B's effective wins (winner side = B)
        let w_b = r.wins_b as f64 + (1.0 - options.draw_weight) * r.draws as f64;
        if w_b > 0.0 {
            terms.push(SigmoidTerm {
                pos: ib,
                neg: ia,

                weight: w_b,
                gamecount: w_b,
            });
        }
    }

    // Check connectivity before adding priors
    let pairs: Vec<(usize, usize)> = records
        .iter()
        .filter(|r| r.wins_a + r.wins_b + r.draws > 0)
        .map(|r| (player_idx[&r.player_a], player_idx[&r.player_b]))
        .collect();
    if !check_connected(n, &pairs) {
        return Err(EloError::DisconnectedGraph);
    }

    // Per-player prior: virtual 50% games vs anchor (at strength 0).
    // This regularizes non-anchor players toward the anchor's strength.
    if options.prior_games > 0.0 {
        for i in 0..n {
            if i == anchor_idx {
                continue;
            }
            let half = 0.5 * options.prior_games;
            // "player i wins" half the virtual games
            terms.push(SigmoidTerm {
                pos: i,
                neg: usize::MAX,

                weight: half,
                gamecount: half,
            });
            // "player i loses" the other half
            terms.push(SigmoidTerm {
                pos: usize::MAX,
                neg: i,

                weight: half,
                gamecount: half,
            });
        }
    }

    // --- Gauss-Newton optimization in strength space ---
    let mut strengths = vec![0.0_f64; n];
    let mut loglikelihood = compute_loglikelihood(&terms, &strengths);

    // Pre-allocate buffers reused across iterations
    let mut g = vec![0.0_f64; n];
    let mut h = vec![0.0_f64; n * n];
    let mut new_strengths = vec![0.0_f64; n];

    let mut iters_since_big_change = 0u32;
    for _ in 0..options.max_iterations {
        // Reset and accumulate gradient and Hessian
        g.fill(0.0);
        h.fill(0.0);
        accum_gradient_hessian(&terms, &strengths, &mut g, &mut h);

        // Constrain anchor: zero gradient, identity row in Hessian.
        // This fixes the anchor at strength 0 throughout optimization.
        g[anchor_idx] = 0.0;
        constrain_anchor_hessian(&mut h, n, anchor_idx);

        // Newton step: solve (-H) * ascent = g
        // (H is negative semi-definite, -H is positive definite)
        for v in h.iter_mut() {
            *v = -*v;
        }
        let ascent = match solve_lu(&mut h, &g, n) {
            Some(x) => x,
            None => return Err(EloError::SingularMatrix),
        };

        // Line search
        let mut step = ascent;
        let mut improved = false;
        for _ in 0..30 {
            for (ns, (s, d)) in new_strengths.iter_mut().zip(strengths.iter().zip(&step)) {
                *ns = s + d;
            }
            let new_ll = compute_loglikelihood(&terms, &new_strengths);
            if new_ll > loglikelihood {
                let elo_change = step
                    .iter()
                    .map(|d| (d * ELO_PER_STRENGTH).abs())
                    .fold(0.0_f64, f64::max);
                std::mem::swap(&mut strengths, &mut new_strengths);
                loglikelihood = new_ll;
                improved = true;

                if elo_change > options.tolerance {
                    iters_since_big_change = 0;
                } else {
                    iters_since_big_change += 1;
                }
                break;
            }
            for v in step.iter_mut() {
                *v *= 0.6;
            }
        }
        if !improved {
            iters_since_big_change += 1;
        }
        if iters_since_big_change > 3 {
            break;
        }
    }

    // Convert to Elo: shift so anchor lands at anchor_elo
    let anchor_shift = options.anchor_elo - strengths[anchor_idx] * ELO_PER_STRENGTH;
    let elos: Vec<f64> = strengths
        .iter()
        .map(|s| s * ELO_PER_STRENGTH + anchor_shift)
        .collect();

    // Uncertainty
    let mut covariance: Option<Vec<f64>> = None;
    let mut stderrs: Vec<Option<f64>> = vec![None; n];
    let mut effective_counts: Vec<Option<f64>> = vec![None; n];

    if options.compute_uncertainty {
        // Recompute Hessian at final strengths (reuse buffers)
        g.fill(0.0);
        h.fill(0.0);
        accum_gradient_hessian(&terms, &strengths, &mut g, &mut h);

        // Constrain anchor: replace its row/col with identity so the
        // precision matrix is invertible and anchor gets zero variance.
        constrain_anchor_hessian(&mut h, n, anchor_idx);

        // Precision = -H (in strength space), convert to Elo space in-place
        let scale = ELO_PER_STRENGTH * ELO_PER_STRENGTH;
        for v in h.iter_mut() {
            *v = -*v / scale;
        }

        if let Some(cov) = invert_matrix(&h, n) {
            for i in 0..n {
                stderrs[i] = Some(cov[i * n + i].max(0.0).sqrt());
            }
            covariance = Some(cov);
        }

        // Effective game count per player (diagonal only)
        // ESS = (sum of |info_ii|)^2 / (sum of info_ii^2 / gamecount)
        let mut ess_num = vec![0.0_f64; n];
        let mut ess_den = vec![0.0_f64; n];
        for t in &terms {
            if t.gamecount <= 0.0 {
                continue;
            }
            let s_total = term_strength(t, &strengths);
            let cosh = (0.5 * s_total).cosh();
            let d2 = -t.weight / (4.0 * cosh * cosh);

            let (indices, len) = term_player_indices(t);
            for &(pi, ci) in &indices[..len] {
                // Only diagonal: pi == pj
                let x = ci * ci * d2;
                ess_num[pi] += x;
                ess_den[pi] += x * x / t.gamecount;
            }
        }
        for i in 0..n {
            effective_counts[i] = if ess_den[i].abs() > 1e-30 {
                Some(ess_num[i] * ess_num[i] / ess_den[i])
            } else {
                Some(0.0)
            };
        }
    }

    // Build ratings sorted by Elo descending
    let mut ratings: Vec<EloRating> = players
        .iter()
        .enumerate()
        .map(|(i, p)| EloRating {
            player_id: p.clone(),
            elo: elos[i],
            stderr: stderrs[i],
            effective_game_count: effective_counts[i],
        })
        .collect();
    ratings.sort_by(|a, b| {
        b.elo
            .partial_cmp(&a.elo)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(EloResult {
        ratings,
        anchor: options.anchor.clone(),
        anchor_elo: options.anchor_elo,
        elo_covariance: covariance,
        player_index: player_idx,
        n,
    })
}

/// Aggregate game results into head-to-head records.
/// Groups by (player1, player2) pair, classifies win/loss/draw from scores.
pub fn head_to_head_from_results(results: &[GameResultRecord]) -> Vec<HeadToHead> {
    let mut map: HashMap<(String, String), (u32, u32, u32)> = HashMap::new();

    for r in results {
        // Canonical ordering: smaller ID first
        let (a, b, a_score, b_score) = if r.player1_id <= r.player2_id {
            (
                &r.player1_id,
                &r.player2_id,
                r.player1_score,
                r.player2_score,
            )
        } else {
            (
                &r.player2_id,
                &r.player1_id,
                r.player2_score,
                r.player1_score,
            )
        };

        let entry = map.entry((a.clone(), b.clone())).or_insert((0, 0, 0));
        if (a_score - b_score).abs() < 1e-9 {
            entry.2 += 1; // draw
        } else if a_score > b_score {
            entry.0 += 1; // a wins
        } else {
            entry.1 += 1; // b wins
        }
    }

    let mut records: Vec<HeadToHead> = map
        .into_iter()
        .map(|((a, b), (wa, wb, d))| HeadToHead {
            player_a: a,
            player_b: b,
            wins_a: wa,
            wins_b: wb,
            draws: d,
        })
        .collect();
    records.sort_by(|a, b| (&a.player_a, &a.player_b).cmp(&(&b.player_a, &b.player_b)));
    records
}

/// Expected win probability given two Elo ratings.
pub fn win_expectancy(elo_a: f64, elo_b: f64) -> f64 {
    1.0 / (1.0 + 10.0_f64.powf((elo_b - elo_a) / 400.0))
}

/// Infer Elo from observed winrate against a known opponent.
pub fn elo_from_winrate(winrate: f64, opponent_elo: f64) -> Result<f64, EloError> {
    if winrate <= 0.0 || winrate >= 1.0 {
        return Err(EloError::NoRecords); // reuse error; winrate out of range
    }
    Ok(opponent_elo - 400.0 * (1.0 / winrate - 1.0).log10())
}

// ---------------------------------------------------------------------------
// Internal types and helpers
// ---------------------------------------------------------------------------

/// A single sigmoid likelihood term.
/// Represents: weight * log(σ(strength[pos] - strength[neg]))
/// Use usize::MAX as sentinel for "no player" (prior terms).
struct SigmoidTerm {
    pos: usize,
    neg: usize,
    weight: f64,
    gamecount: f64,
}

/// Zero anchor's row/col in the Hessian and set diagonal to large negative.
fn constrain_anchor_hessian(h: &mut [f64], n: usize, anchor_idx: usize) {
    for j in 0..n {
        h[anchor_idx * n + j] = 0.0;
        h[j * n + anchor_idx] = 0.0;
    }
    h[anchor_idx * n + anchor_idx] = -1e12;
}

fn term_strength(t: &SigmoidTerm, strengths: &[f64]) -> f64 {
    let mut s = 0.0;
    if t.pos < strengths.len() {
        s += strengths[t.pos];
    }
    if t.neg < strengths.len() {
        s -= strengths[t.neg];
    }
    s
}

/// Returns (player_index, coefficient) pairs for this term.
/// At most 2 entries; returns count and fixed-size array to avoid allocation.
fn term_player_indices(t: &SigmoidTerm) -> ([(usize, f64); 2], usize) {
    let mut buf = [(0, 0.0); 2];
    let mut len = 0;
    if t.pos != usize::MAX {
        buf[len] = (t.pos, 1.0);
        len += 1;
    }
    if t.neg != usize::MAX {
        buf[len] = (t.neg, -1.0);
        len += 1;
    }
    (buf, len)
}

fn compute_loglikelihood(terms: &[SigmoidTerm], strengths: &[f64]) -> f64 {
    let mut total = 0.0;
    for t in terms {
        let s = term_strength(t, strengths);
        total += t.weight * log_sigmoid(s);
    }
    total
}

fn accum_gradient_hessian(terms: &[SigmoidTerm], strengths: &[f64], g: &mut [f64], h: &mut [f64]) {
    let n = g.len();
    for t in terms {
        let s = term_strength(t, strengths);

        // Gradient: dL/ds_total = weight * σ(-s) = weight / (1 + exp(s))
        let sig_neg = sigmoid(-s);
        let dl = t.weight * sig_neg;

        // Hessian: d²L/ds_total² = -weight * σ(s) * σ(-s)
        let cosh = (0.5 * s).cosh();
        let d2l = -t.weight / (4.0 * cosh * cosh);

        let (indices, len) = term_player_indices(t);
        for &(pi, ci) in &indices[..len] {
            g[pi] += ci * dl;
            for &(pj, cj) in &indices[..len] {
                h[pi * n + pj] += ci * cj * d2l;
            }
        }
    }
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Numerically stable log(σ(x)).
fn log_sigmoid(x: f64) -> f64 {
    if x < -40.0 {
        x
    } else {
        -(1.0 + (-x).exp()).ln()
    }
}

/// BFS connectivity check.
fn check_connected(n: usize, pairs: &[(usize, usize)]) -> bool {
    if n <= 1 {
        return true;
    }
    let mut adj = vec![vec![]; n];
    for &(a, b) in pairs {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut visited = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    visited[0] = true;
    queue.push_back(0);
    let mut count = 1usize;
    while let Some(u) = queue.pop_front() {
        for &v in &adj[u] {
            if !visited[v] {
                visited[v] = true;
                count += 1;
                queue.push_back(v);
            }
        }
    }
    count == n
}

/// LU factorization with partial pivoting. Stores L and U in `a` in place.
/// Returns the permutation vector, or None if singular.
fn lu_factorize(a: &mut [f64], n: usize) -> Option<Vec<usize>> {
    let mut perm: Vec<usize> = (0..n).collect();

    for col in 0..n {
        // Partial pivoting: find max in column
        let mut max_val = a[perm[col] * n + col].abs();
        let mut max_row = col;
        for row in (col + 1)..n {
            let val = a[perm[row] * n + col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_val < 1e-30 {
            return None;
        }
        perm.swap(col, max_row);

        let pivot = a[perm[col] * n + col];
        for row in (col + 1)..n {
            let factor = a[perm[row] * n + col] / pivot;
            a[perm[row] * n + col] = factor; // store L
            for k in (col + 1)..n {
                let val = a[perm[col] * n + k];
                a[perm[row] * n + k] -= factor * val;
            }
        }
    }
    Some(perm)
}

/// Solve Ax = b using a pre-computed LU factorization and permutation.
fn lu_solve(lu: &[f64], perm: &[usize], b: &[f64], n: usize) -> Option<Vec<f64>> {
    // Forward substitution (apply L)
    let mut y = b.to_vec();
    for col in 0..n {
        for row in (col + 1)..n {
            let factor = lu[perm[row] * n + col];
            y[perm[row]] -= factor * y[perm[col]];
        }
    }

    // Back substitution (apply U)
    let mut result = vec![0.0; n];
    for i in (0..n).rev() {
        let mut sum = y[perm[i]];
        for j in (i + 1)..n {
            sum -= lu[perm[i] * n + j] * result[j];
        }
        let diag = lu[perm[i] * n + i];
        if diag.abs() < 1e-30 {
            return None;
        }
        result[i] = sum / diag;
    }
    Some(result)
}

/// Solve Ax = b via LU decomposition with partial pivoting.
/// Modifies `a` in place. Returns None if singular.
fn solve_lu(a: &mut [f64], b: &[f64], n: usize) -> Option<Vec<f64>> {
    let perm = lu_factorize(a, n)?;
    lu_solve(a, &perm, b, n)
}

/// Invert matrix by factoring LU once, then solving for each column.
fn invert_matrix(a: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut lu = a.to_vec();
    let perm = lu_factorize(&mut lu, n)?;

    let mut inv = vec![0.0; n * n];
    let mut e = vec![0.0; n];
    for col in 0..n {
        e.fill(0.0);
        e[col] = 1.0;
        let x = lu_solve(&lu, &perm, &e, n)?;
        for row in 0..n {
            inv[row * n + col] = x[row];
        }
    }
    Some(inv)
}

/// Normal CDF via rational approximation of erf.
fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Approximation of the error function (Abramowitz & Stegun 7.1.26).
fn erf(x: f64) -> f64 {
    let sign = if x >= 0.0 { 1.0 } else { -1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let t5 = t4 * t;
    let poly = 0.254_829_592 * t - 0.284_496_736 * t2 + 1.421_413_741 * t3 - 1.453_152_027 * t4
        + 1.061_405_429 * t5;
    sign * (1.0 - poly * (-x * x).exp())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(anchor: &str) -> EloOptions {
        EloOptions {
            anchor: anchor.to_string(),
            anchor_elo: 1000.0,
            compute_uncertainty: true,
            prior_games: 2.0,
            ..EloOptions::default()
        }
    }

    fn opts_no_prior(anchor: &str) -> EloOptions {
        EloOptions {
            anchor: anchor.to_string(),
            anchor_elo: 1000.0,
            compute_uncertainty: true,
            prior_games: 0.0,
            ..EloOptions::default()
        }
    }

    // ---------------------------------------------------------------
    // Primitive helpers
    // ---------------------------------------------------------------

    #[test]
    fn sigmoid_known_values() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-12);
        // σ(x) + σ(-x) = 1
        for &x in &[-5.0, -1.0, 0.5, 2.0, 10.0] {
            assert!((sigmoid(x) + sigmoid(-x) - 1.0).abs() < 1e-12, "x={x}");
        }
        // Large positive → 1, large negative → 0
        assert!(sigmoid(50.0) > 1.0 - 1e-15);
        assert!(sigmoid(-50.0) < 1e-15);
    }

    #[test]
    fn log_sigmoid_matches_ln_sigmoid() {
        for &x in &[-40.0, -10.0, -1.0, 0.0, 1.0, 10.0, 40.0] {
            let ls = log_sigmoid(x);
            let direct = sigmoid(x).ln();
            // For very negative x the direct route loses precision,
            // but log_sigmoid should stay accurate.
            if x > -30.0 {
                assert!(
                    (ls - direct).abs() < 1e-10,
                    "x={x}: ls={ls}, direct={direct}"
                );
            }
            // log_sigmoid is always ≤ 0
            assert!(ls <= 0.0, "x={x}");
        }
    }

    #[test]
    fn solve_lu_identity() {
        // I * x = b → x = b
        let mut a = vec![1.0, 0.0, 0.0, 1.0];
        let b = vec![3.0, 7.0];
        let x = solve_lu(&mut a, &b, 2).unwrap();
        assert!((x[0] - 3.0).abs() < 1e-12);
        assert!((x[1] - 7.0).abs() < 1e-12);
    }

    #[test]
    fn solve_lu_2x2() {
        // [[2, 1], [5, 3]] * x = [1, 2] → x = [1, -1]
        let mut a = vec![2.0, 1.0, 5.0, 3.0];
        let b = vec![1.0, 2.0];
        let x = solve_lu(&mut a, &b, 2).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-10, "x[0]={}", x[0]);
        assert!((x[1] - (-1.0)).abs() < 1e-10, "x[1]={}", x[1]);
    }

    #[test]
    fn solve_lu_singular_returns_none() {
        let mut a = vec![1.0, 2.0, 2.0, 4.0]; // rank 1
        let b = vec![1.0, 2.0];
        assert!(solve_lu(&mut a, &b, 2).is_none());
    }

    #[test]
    fn invert_matrix_2x2() {
        // [[4, 7], [2, 6]] → inv = [[0.6, -0.7], [-0.2, 0.4]]
        let a = vec![4.0, 7.0, 2.0, 6.0];
        let inv = invert_matrix(&a, 2).unwrap();
        assert!((inv[0] - 0.6).abs() < 1e-10);
        assert!((inv[1] - (-0.7)).abs() < 1e-10);
        assert!((inv[2] - (-0.2)).abs() < 1e-10);
        assert!((inv[3] - 0.4).abs() < 1e-10);
    }

    #[test]
    fn invert_matrix_roundtrip() {
        // A * A^{-1} = I for a 3×3
        let a = vec![2.0, 1.0, 1.0, 4.0, 3.0, 3.0, 8.0, 7.0, 9.0];
        let inv = invert_matrix(&a, 3).unwrap();
        // Check A * inv ≈ I
        for i in 0..3 {
            for j in 0..3 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += a[i * 3 + k] * inv[k * 3 + j];
                }
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((sum - expected).abs() < 1e-8, "A*inv [{i},{j}]={sum}");
            }
        }
    }

    #[test]
    fn erf_known_values() {
        // erf(0) ≈ 0
        assert!(erf(0.0).abs() < 1e-6);
        // erf is odd
        for &x in &[0.5, 1.0, 2.0] {
            assert!((erf(x) + erf(-x)).abs() < 1e-6, "erf should be odd, x={x}");
        }
        // Known values (NIST)
        assert!(
            (erf(0.5) - 0.520_500).abs() < 0.001,
            "erf(0.5)={}",
            erf(0.5)
        );
        assert!(
            (erf(1.0) - 0.842_701).abs() < 0.001,
            "erf(1.0)={}",
            erf(1.0)
        );
        assert!(
            (erf(2.0) - 0.995_322).abs() < 0.001,
            "erf(2.0)={}",
            erf(2.0)
        );
        // erf(∞) → 1
        assert!((erf(6.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normal_cdf_known_values() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-6);
        // Φ(1) ≈ 0.8413, Φ(-1) ≈ 0.1587
        assert!((normal_cdf(1.0) - 0.8413).abs() < 0.001);
        assert!((normal_cdf(-1.0) - 0.1587).abs() < 0.001);
        // Φ(x) + Φ(-x) = 1
        for &x in &[0.5, 1.0, 2.0, 3.0] {
            assert!(
                (normal_cdf(x) + normal_cdf(-x) - 1.0).abs() < 1e-6,
                "symmetry at x={x}"
            );
        }
        // Tails
        assert!(normal_cdf(3.0) > 0.9986);
        assert!(normal_cdf(-3.0) < 0.0014);
    }

    #[test]
    fn check_connected_basic() {
        assert!(check_connected(3, &[(0, 1), (1, 2)]));
        assert!(!check_connected(3, &[(0, 1)])); // node 2 isolated
        assert!(check_connected(1, &[]));
    }

    // ---------------------------------------------------------------
    // win_expectancy
    // ---------------------------------------------------------------

    #[test]
    fn win_expectancy_equal_ratings() {
        assert!((win_expectancy(1000.0, 1000.0) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn win_expectancy_400_gap() {
        // 400 points = 10:1 odds → P = 10/11 ≈ 0.90909
        let p = win_expectancy(1400.0, 1000.0);
        assert!((p - 10.0 / 11.0).abs() < 1e-10, "p={p}");
    }

    #[test]
    fn win_expectancy_symmetry() {
        // P(A > B) + P(B > A) = 1
        for &(a, b) in &[(1200.0, 1000.0), (800.0, 1300.0), (1000.0, 1000.0)] {
            let sum = win_expectancy(a, b) + win_expectancy(b, a);
            assert!((sum - 1.0).abs() < 1e-12, "a={a}, b={b}");
        }
    }

    #[test]
    fn win_expectancy_monotonic() {
        // Higher gap → higher probability
        let p200 = win_expectancy(1200.0, 1000.0);
        let p400 = win_expectancy(1400.0, 1000.0);
        let p800 = win_expectancy(1800.0, 1000.0);
        assert!(0.5 < p200);
        assert!(p200 < p400);
        assert!(p400 < p800);
        assert!(p800 < 1.0);
    }

    // ---------------------------------------------------------------
    // elo_from_winrate — inverse of win_expectancy
    // ---------------------------------------------------------------

    #[test]
    fn elo_from_winrate_50_percent() {
        let elo = elo_from_winrate(0.5, 1000.0).unwrap();
        assert!((elo - 1000.0).abs() < 0.01, "elo={elo}");
    }

    #[test]
    fn elo_from_winrate_75_percent() {
        // 75% → 400 * log10(3) ≈ 190.85 above opponent
        let expected = 1000.0 + 400.0 * 3.0_f64.log10();
        let elo = elo_from_winrate(0.75, 1000.0).unwrap();
        assert!(
            (elo - expected).abs() < 0.01,
            "elo={elo}, expected={expected}"
        );
    }

    #[test]
    fn elo_from_winrate_roundtrips_with_win_expectancy() {
        // For any winrate, elo_from_winrate inverts win_expectancy.
        for &wr in &[0.1, 0.25, 0.5, 0.75, 0.9] {
            let elo = elo_from_winrate(wr, 1000.0).unwrap();
            let recovered = win_expectancy(elo, 1000.0);
            assert!(
                (recovered - wr).abs() < 1e-10,
                "winrate={wr}, elo={elo}, recovered={recovered}"
            );
        }
    }

    #[test]
    fn elo_from_winrate_rejects_boundary() {
        assert!(elo_from_winrate(0.0, 1000.0).is_err());
        assert!(elo_from_winrate(1.0, 1000.0).is_err());
    }

    // ---------------------------------------------------------------
    // compute_elo — quantitative properties
    // ---------------------------------------------------------------

    /// Helper: 2-player MLE with no prior. The closed-form solution is
    /// diff = 400 * log10(wins_a / wins_b).
    fn two_player_expected_diff(wins_a: u32, wins_b: u32) -> f64 {
        400.0 * (wins_a as f64 / wins_b as f64).log10()
    }

    #[test]
    fn two_player_50_50_diff_is_zero() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 1000,
            wins_b: 1000,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap();
        assert!(diff.abs() < 0.1, "50-50 should give diff ≈ 0, got {diff}");
    }

    #[test]
    fn two_player_75_25() {
        // diff = 400 * log10(3) ≈ 190.85
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 750,
            wins_b: 250,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap();
        let expected = two_player_expected_diff(750, 250);
        assert!(
            (diff - expected).abs() < 1.0,
            "75/25 should give diff ≈ {expected}, got {diff}"
        );
    }

    #[test]
    fn two_player_90_10() {
        // diff = 400 * log10(9) ≈ 381.76
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 900,
            wins_b: 100,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap();
        let expected = two_player_expected_diff(900, 100);
        assert!(
            (diff - expected).abs() < 1.0,
            "90/10 should give diff ≈ {expected}, got {diff}"
        );
    }

    #[test]
    fn two_player_70_30() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 700,
            wins_b: 300,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap();
        let expected = two_player_expected_diff(700, 300);
        assert!(
            (diff - expected).abs() < 1.0,
            "70/30 should give diff ≈ {expected}, got {diff}"
        );
    }

    #[test]
    fn anchor_is_exact() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 7,
            wins_b: 3,
            draws: 0,
        }];
        for &anchor_elo in &[0.0, 500.0, 1000.0, 1500.0] {
            let opts = EloOptions {
                anchor: "B".into(),
                anchor_elo,
                ..opts("B")
            };
            let result = compute_elo(&records, &opts).unwrap();
            let actual = result.get_elo("B").unwrap();
            assert!(
                (actual - anchor_elo).abs() < 0.01,
                "anchor_elo={anchor_elo}, actual={actual}"
            );
        }
    }

    #[test]
    fn symmetry_of_player_order() {
        let r1 = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 7,
            wins_b: 3,
            draws: 0,
        }];
        let r2 = vec![HeadToHead {
            player_a: "B".into(),
            player_b: "A".into(),
            wins_a: 3,
            wins_b: 7,
            draws: 0,
        }];
        let d1 = compute_elo(&r1, &opts("B"))
            .unwrap()
            .elo_difference("A", "B")
            .unwrap();
        let d2 = compute_elo(&r2, &opts("B"))
            .unwrap()
            .elo_difference("A", "B")
            .unwrap();
        assert!((d1 - d2).abs() < 0.1, "d1={d1}, d2={d2}");
    }

    #[test]
    fn all_draws_gives_equal_ratings() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 0,
            wins_b: 0,
            draws: 1000,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap().abs();
        assert!(diff < 0.1, "all draws → diff ≈ 0, got {diff}");
    }

    #[test]
    fn expected_score_roundtrips_with_data() {
        // If A beats B at 70%, the fitted expected_score should be ≈ 0.7.
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 700,
            wins_b: 300,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let es = result.expected_score("A", "B").unwrap();
        assert!(
            (es - 0.7).abs() < 0.01,
            "expected_score should ≈ 0.7, got {es}"
        );
    }

    #[test]
    fn three_players_transitive_ordering() {
        let records = vec![
            HeadToHead {
                player_a: "A".into(),
                player_b: "B".into(),
                wins_a: 70,
                wins_b: 30,
                draws: 0,
            },
            HeadToHead {
                player_a: "B".into(),
                player_b: "C".into(),
                wins_a: 60,
                wins_b: 40,
                draws: 0,
            },
        ];
        let result = compute_elo(&records, &opts("C")).unwrap();
        let elo_a = result.get_elo("A").unwrap();
        let elo_b = result.get_elo("B").unwrap();
        let elo_c = result.get_elo("C").unwrap();
        assert!(elo_a > elo_b, "A should be above B");
        assert!(elo_b > elo_c, "B should be above C");
        // A-B gap should be larger than B-C gap (70/30 > 60/40)
        assert!(elo_a - elo_b > elo_b - elo_c);
    }

    #[test]
    fn three_players_differences_are_additive() {
        // In Bradley-Terry, Elo differences are additive:
        // diff(A,C) ≈ diff(A,B) + diff(B,C)
        let records = vec![
            HeadToHead {
                player_a: "A".into(),
                player_b: "B".into(),
                wins_a: 700,
                wins_b: 300,
                draws: 0,
            },
            HeadToHead {
                player_a: "B".into(),
                player_b: "C".into(),
                wins_a: 600,
                wins_b: 400,
                draws: 0,
            },
            HeadToHead {
                player_a: "A".into(),
                player_b: "C".into(),
                wins_a: 800,
                wins_b: 200,
                draws: 0,
            },
        ];
        let result = compute_elo(&records, &opts_no_prior("C")).unwrap();
        let ab = result.elo_difference("A", "B").unwrap();
        let bc = result.elo_difference("B", "C").unwrap();
        let ac = result.elo_difference("A", "C").unwrap();
        assert!(
            (ac - (ab + bc)).abs() < 1.0,
            "additivity: ac={ac}, ab+bc={}",
            ab + bc
        );
    }

    // ---------------------------------------------------------------
    // Prior effects
    // ---------------------------------------------------------------

    #[test]
    fn prior_shrinks_ratings_toward_anchor() {
        // 1 win, 0 losses. Without prior → extreme rating.
        // With strong prior → pulled back toward anchor.
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 1,
            wins_b: 0,
            draws: 0,
        }];
        let strong = EloOptions {
            anchor: "B".into(),
            prior_games: 100.0,
            ..opts("B")
        };
        let weak = EloOptions {
            anchor: "B".into(),
            prior_games: 1.0,
            ..opts("B")
        };
        let d_strong = compute_elo(&records, &strong)
            .unwrap()
            .elo_difference("A", "B")
            .unwrap();
        let d_weak = compute_elo(&records, &weak)
            .unwrap()
            .elo_difference("A", "B")
            .unwrap();
        assert!(d_strong < d_weak, "stronger prior should shrink diff more");
        assert!(
            d_strong < 20.0,
            "strong prior should keep diff small, got {d_strong}"
        );
    }

    #[test]
    fn prior_vanishes_with_many_games() {
        // With 10000 games, prior_games=2 should barely matter.
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 7000,
            wins_b: 3000,
            draws: 0,
        }];
        let with_prior = compute_elo(&records, &opts("B")).unwrap();
        let without = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let d1 = with_prior.elo_difference("A", "B").unwrap();
        let d2 = without.elo_difference("A", "B").unwrap();
        assert!(
            (d1 - d2).abs() < 1.0,
            "prior should be negligible with many games, d1={d1}, d2={d2}"
        );
    }

    // ---------------------------------------------------------------
    // Uncertainty properties
    // ---------------------------------------------------------------

    #[test]
    fn stderr_decreases_with_more_games() {
        let mk = |n: u32| -> f64 {
            let records = vec![HeadToHead {
                player_a: "A".into(),
                player_b: "B".into(),
                wins_a: n,
                wins_b: n,
                draws: 0,
            }];
            let result = compute_elo(&records, &opts("B")).unwrap();
            result
                .ratings
                .iter()
                .find(|r| r.player_id == "A")
                .unwrap()
                .stderr
                .unwrap()
        };
        let se50 = mk(50);
        let se200 = mk(200);
        let se1000 = mk(1000);
        assert!(se50 > se200, "se50={se50} > se200={se200}");
        assert!(se200 > se1000, "se200={se200} > se1000={se1000}");
    }

    #[test]
    fn stderr_scales_as_one_over_sqrt_n() {
        // For 50-50 games, Fisher info per game is constant, so
        // stderr ∝ 1/√n. Doubling games should halve stderr (×√2 ratio).
        let mk = |n: u32| -> f64 {
            let records = vec![HeadToHead {
                player_a: "A".into(),
                player_b: "B".into(),
                wins_a: n,
                wins_b: n,
                draws: 0,
            }];
            compute_elo(&records, &opts_no_prior("B"))
                .unwrap()
                .ratings
                .iter()
                .find(|r| r.player_id == "A")
                .unwrap()
                .stderr
                .unwrap()
        };
        let se100 = mk(500);
        let se400 = mk(2000);
        // ratio should be ≈ 2.0 (√4)
        let ratio = se100 / se400;
        assert!(
            (ratio - 2.0).abs() < 0.1,
            "stderr ratio for 4× games should be ≈ 2.0, got {ratio}"
        );
    }

    #[test]
    fn anchor_stderr_is_zero() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 50,
            wins_b: 50,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let b = result.ratings.iter().find(|r| r.player_id == "B").unwrap();
        assert!(b.stderr.unwrap() < 0.01, "anchor stderr should be ≈ 0");
    }

    #[test]
    fn diff_stderr_equals_individual_in_two_player() {
        // When B is the anchor (stderr=0, cov=0), diff_stderr(A,B) = stderr(A).
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 50,
            wins_b: 50,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let a_stderr = result
            .ratings
            .iter()
            .find(|r| r.player_id == "A")
            .unwrap()
            .stderr
            .unwrap();
        let diff_stderr = result.elo_difference_stderr("A", "B").unwrap();
        assert!(
            (diff_stderr - a_stderr).abs() < 0.1,
            "diff_stderr={diff_stderr}, a_stderr={a_stderr}"
        );
    }

    #[test]
    fn diff_stderr_benefits_from_covariance_in_three_player() {
        // With 3 players, diff_stderr(A,C) should be smaller than
        // sqrt(stderr(A)² + stderr(C)²) because covariance helps.
        let records = vec![
            HeadToHead {
                player_a: "A".into(),
                player_b: "B".into(),
                wins_a: 500,
                wins_b: 500,
                draws: 0,
            },
            HeadToHead {
                player_a: "B".into(),
                player_b: "C".into(),
                wins_a: 500,
                wins_b: 500,
                draws: 0,
            },
        ];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let se_a = result
            .ratings
            .iter()
            .find(|r| r.player_id == "A")
            .unwrap()
            .stderr
            .unwrap();
        let se_c = result
            .ratings
            .iter()
            .find(|r| r.player_id == "C")
            .unwrap()
            .stderr
            .unwrap();
        let naive = (se_a * se_a + se_c * se_c).sqrt();
        let actual = result.elo_difference_stderr("A", "C").unwrap();
        assert!(
            actual <= naive + 0.1,
            "diff_stderr should benefit from covariance: actual={actual}, naive={naive}"
        );
    }

    // ---------------------------------------------------------------
    // LOS (likelihood of superiority)
    // ---------------------------------------------------------------

    #[test]
    fn los_equal_players_is_half() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 500,
            wins_b: 500,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let los = result.likelihood_of_superiority("A", "B").unwrap();
        assert!(
            (los - 0.5).abs() < 0.05,
            "LOS for equal players ≈ 0.5, got {los}"
        );
    }

    #[test]
    fn los_dominant_player_near_one() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 900,
            wins_b: 100,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let los = result.likelihood_of_superiority("A", "B").unwrap();
        assert!(
            los > 0.99,
            "dominant player LOS should be near 1.0, got {los}"
        );
    }

    #[test]
    fn los_symmetry() {
        // LOS(A,B) + LOS(B,A) = 1
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 60,
            wins_b: 40,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let los_ab = result.likelihood_of_superiority("A", "B").unwrap();
        let los_ba = result.likelihood_of_superiority("B", "A").unwrap();
        assert!(
            (los_ab + los_ba - 1.0).abs() < 0.01,
            "LOS(A,B) + LOS(B,A) should = 1.0, got {}",
            los_ab + los_ba
        );
    }

    #[test]
    fn los_self_is_half() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 60,
            wins_b: 40,
            draws: 0,
        }];
        let result = compute_elo(&records, &opts("B")).unwrap();
        assert_eq!(result.likelihood_of_superiority("A", "A").unwrap(), 0.5);
    }

    // ---------------------------------------------------------------
    // Error conditions
    // ---------------------------------------------------------------

    #[test]
    fn error_no_records() {
        assert!(matches!(
            compute_elo(&[], &opts("A")),
            Err(EloError::NoRecords)
        ));
    }

    #[test]
    fn error_anchor_not_found() {
        let records = vec![HeadToHead {
            player_a: "A".into(),
            player_b: "B".into(),
            wins_a: 5,
            wins_b: 5,
            draws: 0,
        }];
        assert!(matches!(
            compute_elo(&records, &opts("Z")),
            Err(EloError::AnchorNotFound(_))
        ));
    }

    #[test]
    fn error_disconnected_graph() {
        let records = vec![
            HeadToHead {
                player_a: "A".into(),
                player_b: "B".into(),
                wins_a: 5,
                wins_b: 5,
                draws: 0,
            },
            HeadToHead {
                player_a: "C".into(),
                player_b: "D".into(),
                wins_a: 5,
                wins_b: 5,
                draws: 0,
            },
        ];
        assert!(matches!(
            compute_elo(&records, &opts("A")),
            Err(EloError::DisconnectedGraph)
        ));
    }

    // ---------------------------------------------------------------
    // head_to_head_from_results
    // ---------------------------------------------------------------

    #[test]
    fn h2h_classifies_wins_losses_draws() {
        let results = vec![
            GameResultRecord {
                id: 1,
                game_config_id: "c".into(),
                player1_id: "A".into(),
                player2_id: "B".into(),
                player1_score: 5.0,
                player2_score: 3.0,
                turns: 100,
                played_at: "2024-01-01".into(),
            },
            GameResultRecord {
                id: 2,
                game_config_id: "c".into(),
                player1_id: "B".into(),
                player2_id: "A".into(),
                player1_score: 4.0,
                player2_score: 4.0,
                turns: 100,
                played_at: "2024-01-02".into(),
            },
            GameResultRecord {
                id: 3,
                game_config_id: "c".into(),
                player1_id: "A".into(),
                player2_id: "B".into(),
                player1_score: 2.0,
                player2_score: 6.0,
                turns: 100,
                played_at: "2024-01-03".into(),
            },
        ];
        let h2h = head_to_head_from_results(&results);
        assert_eq!(h2h.len(), 1);
        assert_eq!(h2h[0].player_a, "A");
        assert_eq!(h2h[0].player_b, "B");
        assert_eq!(h2h[0].wins_a, 1);
        assert_eq!(h2h[0].wins_b, 1);
        assert_eq!(h2h[0].draws, 1);
    }

    #[test]
    fn h2h_groups_by_pair() {
        let results = vec![
            GameResultRecord {
                id: 1,
                game_config_id: "c".into(),
                player1_id: "A".into(),
                player2_id: "B".into(),
                player1_score: 5.0,
                player2_score: 3.0,
                turns: 100,
                played_at: "t".into(),
            },
            GameResultRecord {
                id: 2,
                game_config_id: "c".into(),
                player1_id: "A".into(),
                player2_id: "C".into(),
                player1_score: 4.0,
                player2_score: 4.0,
                turns: 100,
                played_at: "t".into(),
            },
        ];
        let h2h = head_to_head_from_results(&results);
        assert_eq!(h2h.len(), 2);
    }

    // ---------------------------------------------------------------
    // Integration
    // ---------------------------------------------------------------

    #[test]
    fn full_pipeline_results_to_elo() {
        let results = vec![
            GameResultRecord {
                id: 1,
                game_config_id: "c".into(),
                player1_id: "greedy".into(),
                player2_id: "random".into(),
                player1_score: 8.0,
                player2_score: 2.0,
                turns: 100,
                played_at: "2024-01-01".into(),
            },
            GameResultRecord {
                id: 2,
                game_config_id: "c".into(),
                player1_id: "greedy".into(),
                player2_id: "random".into(),
                player1_score: 7.0,
                player2_score: 3.0,
                turns: 100,
                played_at: "2024-01-02".into(),
            },
            GameResultRecord {
                id: 3,
                game_config_id: "c".into(),
                player1_id: "random".into(),
                player2_id: "greedy".into(),
                player1_score: 4.0,
                player2_score: 6.0,
                turns: 100,
                played_at: "2024-01-03".into(),
            },
        ];
        let h2h = head_to_head_from_results(&results);
        let result = compute_elo(&h2h, &opts("random")).unwrap();
        let greedy_elo = result.get_elo("greedy").unwrap();
        let random_elo = result.get_elo("random").unwrap();
        assert!(greedy_elo > random_elo, "greedy should be rated higher");
        assert!((random_elo - 1000.0).abs() < 0.01, "anchor should be exact");
    }
}

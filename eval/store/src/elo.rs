//! Elo rating computation using Bradley-Terry MLE.
//!
//! Ports KataGo's Gauss-Newton optimizer to Rust with alpharat's simpler
//! interface types. Works in "strength" space internally (1 unit = e:1 odds),
//! converts to Elo at output.

use std::collections::HashMap;

use serde::Serialize;
use statrs::distribution::{ContinuousCDF, StudentsT};

/// 400 * log10(e) — converts strength units to Elo points.
const ELO_PER_STRENGTH: f64 = 173.717_792_761_245_88;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Win/loss/draw record between two players.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadToHead {
    pub player_a: String,
    pub player_b: String,
    pub wins_a: u32,
    pub wins_b: u32,
    pub draws: u32,
}

impl HeadToHead {
    /// Create a record with no draws.
    pub fn new(a: impl Into<String>, b: impl Into<String>, wins_a: u32, wins_b: u32) -> Self {
        Self {
            player_a: a.into(),
            player_b: b.into(),
            wins_a,
            wins_b,
            draws: 0,
        }
    }

    /// Create a record with explicit draws.
    pub fn with_draws(
        a: impl Into<String>,
        b: impl Into<String>,
        wins_a: u32,
        wins_b: u32,
        draws: u32,
    ) -> Self {
        Self {
            player_a: a.into(),
            player_b: b.into(),
            wins_a,
            wins_b,
            draws,
        }
    }

    /// Total games in this record.
    pub fn total(&self) -> u32 {
        self.wins_a + self.wins_b + self.draws
    }
}

/// Single player's computed rating.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EloRating {
    pub player_id: String,
    pub elo: f64,
}

/// Full result of an Elo computation.
///
/// Methods on this type are always available (no uncertainty needed).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EloResult {
    pub ratings: Vec<EloRating>,
    pub anchor: String,
    pub anchor_elo: f64,
}

impl EloResult {
    /// Elo difference (A - B).
    pub fn elo_difference(&self, a: &str, b: &str) -> Option<f64> {
        let ea = self.get_elo(a)?;
        let eb = self.get_elo(b)?;
        Some(ea - eb)
    }

    /// Expected win probability for A against B based on their Elo ratings.
    pub fn win_expectancy(&self, a: &str, b: &str) -> Option<f64> {
        let ea = self.get_elo(a)?;
        let eb = self.get_elo(b)?;
        Some(win_expectancy(ea, eb))
    }

    pub fn get_rating(&self, name: &str) -> Option<&EloRating> {
        self.ratings.iter().find(|r| r.player_id == name)
    }

    pub fn get_elo(&self, name: &str) -> Option<f64> {
        self.get_rating(name).map(|r| r.elo)
    }
}

/// Uncertainty data from the Hessian at the MLE.
///
/// Only returned by `compute_elo_with_uncertainty`. Owns the covariance matrix
/// and player index needed for pairwise queries.
#[derive(Debug, Clone)]
pub struct EloUncertainty {
    stderrs: Vec<f64>,
    elo_covariance: Vec<f64>,
    effective_game_counts: Vec<f64>,
    player_index: HashMap<String, usize>,
}

impl EloUncertainty {
    /// Standard error on a player's Elo (conditional on the anchor).
    pub fn stderr(&self, name: &str) -> Option<f64> {
        let &i = self.player_index.get(name)?;
        Some(self.stderrs[i])
    }

    /// Effective number of games contributing to a player's rating.
    pub fn effective_game_count(&self, name: &str) -> Option<f64> {
        let &i = self.player_index.get(name)?;
        Some(self.effective_game_counts[i])
    }

    /// Approximate stderr on the Elo difference (A - B), from covariance.
    pub fn elo_difference_stderr(&self, a: &str, b: &str) -> Option<f64> {
        let &ia = self.player_index.get(a)?;
        let &ib = self.player_index.get(b)?;
        let n = self.player_index.len();
        let cov = &self.elo_covariance;
        let var = cov[ia * n + ia] - cov[ia * n + ib] - cov[ib * n + ia] + cov[ib * n + ib];
        Some(var.max(0.0).sqrt())
    }

    /// Probability that A is stronger than B (Student's t approximation).
    ///
    /// Takes `&EloResult` to get the Elo difference between A and B.
    ///
    /// The degrees of freedom use player A's effective game count only.
    /// This follows KataGo's implementation: since we're testing
    /// "is A stronger than B?", the df reflects the uncertainty in A's
    /// rating estimate. This creates a subtle asymmetry — LOS(A,B) and
    /// 1 - LOS(B,A) may differ slightly when effective game counts differ.
    /// In practice, the difference is negligible for reasonable sample sizes.
    pub fn likelihood_of_superiority(&self, result: &EloResult, a: &str, b: &str) -> Option<f64> {
        if a == b {
            return Some(0.5);
        }
        let diff = result.elo_difference(a, b)?;
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
        let &ia = self.player_index.get(a)?;
        let df = (self.effective_game_counts[ia] - 1.0).max(1.0);
        let t = StudentsT::new(0.0, 1.0, df).ok()?;
        Some(t.cdf(diff / stderr))
    }
}

/// Configuration for Elo computation.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct EloOptions {
    anchor: String,
    anchor_elo: f64,
    draw_weight: f64,
    prior_games: f64,
    max_iterations: u32,
    tolerance: f64,
}

impl EloOptions {
    pub fn new(anchor: impl Into<String>) -> Self {
        Self {
            anchor: anchor.into(),
            anchor_elo: 1000.0,
            draw_weight: 0.5,
            prior_games: 2.0,
            max_iterations: 1000,
            tolerance: 0.001,
        }
    }

    pub fn anchor_elo(mut self, v: f64) -> Self {
        self.anchor_elo = v;
        self
    }

    pub fn draw_weight(mut self, v: f64) -> Self {
        self.draw_weight = v;
        self
    }

    pub fn prior_games(mut self, v: f64) -> Self {
        self.prior_games = v;
        self
    }

    pub fn max_iterations(mut self, v: u32) -> Self {
        self.max_iterations = v;
        self
    }

    pub fn tolerance(mut self, v: f64) -> Self {
        self.tolerance = v;
        self
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
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
    #[error("winrate must be in (0, 1), got {0}")]
    InvalidWinrate(f64),
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Compute Elo ratings from head-to-head records (no uncertainty).
pub fn compute_elo(records: &[HeadToHead], options: &EloOptions) -> Result<EloResult, EloError> {
    let (result, _) = compute_elo_inner(records, options, false)?;
    Ok(result)
}

/// Compute Elo ratings with uncertainty (stderr, covariance, ESS).
pub fn compute_elo_with_uncertainty(
    records: &[HeadToHead],
    options: &EloOptions,
) -> Result<(EloResult, EloUncertainty), EloError> {
    let (result, uncertainty) = compute_elo_inner(records, options, true)?;
    Ok((result, uncertainty.expect("requested uncertainty")))
}

/// Shared implementation. When `with_uncertainty` is true, returns Some(EloUncertainty).
fn compute_elo_inner(
    records: &[HeadToHead],
    options: &EloOptions,
    with_uncertainty: bool,
) -> Result<(EloResult, Option<EloUncertainty>), EloError> {
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

    // Build likelihood terms
    let mut terms: Vec<SigmoidTerm> = Vec::new();

    for r in records {
        let ia = player_idx[&r.player_a];
        let ib = player_idx[&r.player_b];
        if r.total() == 0 {
            continue;
        }
        let w_a = r.wins_a as f64 + options.draw_weight * r.draws as f64;
        if w_a > 0.0 {
            terms.push(SigmoidTerm {
                pos: Some(ia),
                neg: Some(ib),
                weight: w_a,
                gamecount: w_a,
            });
        }
        let w_b = r.wins_b as f64 + (1.0 - options.draw_weight) * r.draws as f64;
        if w_b > 0.0 {
            terms.push(SigmoidTerm {
                pos: Some(ib),
                neg: Some(ia),
                weight: w_b,
                gamecount: w_b,
            });
        }
    }

    // Check connectivity before adding priors
    let pairs: Vec<(usize, usize)> = records
        .iter()
        .filter(|r| r.total() > 0)
        .map(|r| (player_idx[&r.player_a], player_idx[&r.player_b]))
        .collect();
    if !check_connected(n, &pairs) {
        return Err(EloError::DisconnectedGraph);
    }

    // Prior: virtual 50% games vs anchor
    if options.prior_games > 0.0 {
        for i in 0..n {
            if i == anchor_idx {
                continue;
            }
            let half = 0.5 * options.prior_games;
            terms.push(SigmoidTerm {
                pos: Some(i),
                neg: None,
                weight: half,
                gamecount: half,
            });
            terms.push(SigmoidTerm {
                pos: None,
                neg: Some(i),
                weight: half,
                gamecount: half,
            });
        }
    }

    // --- Gauss-Newton optimization ---
    let mut strengths = vec![0.0_f64; n];
    let mut loglikelihood = compute_loglikelihood(&terms, &strengths);
    let mut iters_since_big_change = 0u32;

    for _ in 0..options.max_iterations {
        let (new_strengths, new_ll) =
            line_search_ascend(&terms, &strengths, loglikelihood, anchor_idx, n)?;
        let elo_change = new_strengths
            .iter()
            .zip(&strengths)
            .map(|(ns, s)| ((ns - s) * ELO_PER_STRENGTH).abs())
            .fold(0.0_f64, f64::max);
        strengths = new_strengths;
        loglikelihood = new_ll;

        iters_since_big_change += 1;
        if elo_change > options.tolerance {
            iters_since_big_change = 0;
        }
        if iters_since_big_change > 3 {
            break;
        }
    }

    // Convert to Elo
    let anchor_shift = options.anchor_elo - strengths[anchor_idx] * ELO_PER_STRENGTH;
    let elos: Vec<f64> = strengths
        .iter()
        .map(|s| s * ELO_PER_STRENGTH + anchor_shift)
        .collect();

    // Uncertainty
    let uncertainty = if with_uncertainty {
        Some(compute_uncertainty_data(
            &terms,
            &strengths,
            anchor_idx,
            n,
            &player_idx,
        )?)
    } else {
        None
    };

    // Build ratings sorted by Elo descending
    let mut ratings: Vec<EloRating> = players
        .iter()
        .enumerate()
        .map(|(i, p)| EloRating {
            player_id: p.clone(),
            elo: elos[i],
        })
        .collect();
    ratings.sort_by(|a, b| {
        b.elo
            .partial_cmp(&a.elo)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let result = EloResult {
        ratings,
        anchor: options.anchor.clone(),
        anchor_elo: options.anchor_elo,
    };

    Ok((result, uncertainty))
}

/// Expected win probability given two Elo ratings.
pub fn win_expectancy(elo_a: f64, elo_b: f64) -> f64 {
    1.0 / (1.0 + 10.0_f64.powf((elo_b - elo_a) / 400.0))
}

/// Infer Elo from observed winrate against a known opponent.
pub fn elo_from_winrate(winrate: f64, opponent_elo: f64) -> Result<f64, EloError> {
    if winrate <= 0.0 || winrate >= 1.0 {
        return Err(EloError::InvalidWinrate(winrate));
    }
    Ok(opponent_elo - 400.0 * (1.0 / winrate - 1.0).log10())
}

// ---------------------------------------------------------------------------
// Internal types and helpers
// ---------------------------------------------------------------------------

/// A single sigmoid likelihood term.
/// Represents: weight * log(σ(strength[pos] - strength[neg]))
/// `None` means "anchor side" in prior terms (strength implicitly 0).
struct SigmoidTerm {
    pos: Option<usize>,
    neg: Option<usize>,
    weight: f64,
    /// Separate from `weight` for ESS computation (following KataGo).
    /// Currently always equal to `weight`, but would diverge with
    /// time-decay or other weighting schemes.
    gamecount: f64,
}

// ---------------------------------------------------------------------------
// Gauss-Newton sub-routines (matches KataGo decomposition)
// ---------------------------------------------------------------------------

/// Gradient + Hessian + anchor constraint + Newton solve.
fn find_ascent_vector(
    terms: &[SigmoidTerm],
    strengths: &[f64],
    anchor_idx: usize,
    n: usize,
) -> Result<Vec<f64>, EloError> {
    let mut g = vec![0.0_f64; n];
    let mut hessian = vec![0.0_f64; n * n];
    accum_gradient_hessian(terms, strengths, &mut g, &mut hessian);

    g[anchor_idx] = 0.0;
    constrain_anchor_hessian(&mut hessian, n, anchor_idx);

    // Newton step: solve (-H) * ascent = g
    let mut precision = hessian;
    for v in precision.iter_mut() {
        *v = -*v;
    }
    solve_lu(&mut precision, &g, n).ok_or(EloError::SingularMatrix)
}

/// Line search: try full Newton step, damp by 0.6 up to 30 times.
/// Returns (new_strengths, new_loglikelihood).
fn line_search_ascend(
    terms: &[SigmoidTerm],
    strengths: &[f64],
    cur_ll: f64,
    anchor_idx: usize,
    n: usize,
) -> Result<(Vec<f64>, f64), EloError> {
    let mut ascent = find_ascent_vector(terms, strengths, anchor_idx, n)?;
    for _ in 0..30 {
        let new_strengths: Vec<f64> = strengths.iter().zip(&ascent).map(|(s, d)| s + d).collect();
        let new_ll = compute_loglikelihood(terms, &new_strengths);
        if new_ll > cur_ll {
            return Ok((new_strengths, new_ll));
        }
        for v in ascent.iter_mut() {
            *v *= 0.6;
        }
    }
    Ok((strengths.to_vec(), cur_ll))
}

/// Hessian at final point → precision → stderrs + covariance + ESS.
fn compute_uncertainty_data(
    terms: &[SigmoidTerm],
    strengths: &[f64],
    anchor_idx: usize,
    n: usize,
    player_idx: &HashMap<String, usize>,
) -> Result<EloUncertainty, EloError> {
    let mut g = vec![0.0_f64; n];
    let mut hessian = vec![0.0_f64; n * n];
    accum_gradient_hessian(terms, strengths, &mut g, &mut hessian);
    constrain_anchor_hessian(&mut hessian, n, anchor_idx);

    // Precision = -H in Elo space
    let scale = ELO_PER_STRENGTH * ELO_PER_STRENGTH;
    let mut precision = hessian;
    for v in precision.iter_mut() {
        *v = -*v / scale;
    }

    // Stderrs from precision diagonal (conditional, matches KataGo)
    let stderrs: Vec<f64> = (0..n)
        .map(|i| {
            let p = precision[i * n + i];
            if p > 0.0 {
                (1.0 / p).sqrt()
            } else {
                0.0
            }
        })
        .collect();

    // Invert for covariance (needed for diff_stderr / LOS)
    let elo_covariance = invert_matrix(&precision, n).ok_or(EloError::SingularMatrix)?;

    let effective_game_counts = compute_effective_game_counts(terms, strengths, n);

    Ok(EloUncertainty {
        stderrs,
        elo_covariance,
        effective_game_counts,
        player_index: player_idx.clone(),
    })
}

/// Effective sample size per player (diagonal only).
fn compute_effective_game_counts(terms: &[SigmoidTerm], strengths: &[f64], n: usize) -> Vec<f64> {
    let mut ess_num = vec![0.0_f64; n];
    let mut ess_den = vec![0.0_f64; n];
    for t in terms {
        if t.gamecount <= 0.0 {
            continue;
        }
        let s_total = term_strength(t, strengths);
        let cosh = (0.5 * s_total).cosh();
        let d2 = -t.weight / (4.0 * cosh * cosh);

        let (indices, len) = term_player_indices(t);
        for &(pi, ci) in &indices[..len] {
            let x = ci * ci * d2;
            ess_num[pi] += x;
            ess_den[pi] += x * x / t.gamecount;
        }
    }
    (0..n)
        .map(|i| {
            if ess_den[i].abs() > 1e-30 {
                ess_num[i] * ess_num[i] / ess_den[i]
            } else {
                0.0
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

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
    if let Some(p) = t.pos {
        s += strengths[p];
    }
    if let Some(n) = t.neg {
        s -= strengths[n];
    }
    s
}

/// Returns (player_index, coefficient) pairs for this term.
/// At most 2 entries; returns count and fixed-size array to avoid allocation.
fn term_player_indices(t: &SigmoidTerm) -> ([(usize, f64); 2], usize) {
    let mut buf = [(0, 0.0); 2];
    let mut len = 0;
    if let Some(p) = t.pos {
        buf[len] = (p, 1.0);
        len += 1;
    }
    if let Some(n) = t.neg {
        buf[len] = (n, -1.0);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(anchor: &str) -> EloOptions {
        EloOptions::new(anchor)
    }

    fn opts_no_prior(anchor: &str) -> EloOptions {
        EloOptions::new(anchor).prior_games(0.0)
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
    fn log_sigmoid_extreme_negative() {
        // For very negative x, log_sigmoid(x) ≈ x.
        // The stable branch (x < -40) returns x directly.
        let x = -1000.0;
        let ls = log_sigmoid(x);
        assert!(
            (ls - x).abs() < 1e-10,
            "log_sigmoid({x}) should ≈ {x}, got {ls}"
        );
        // Near the branch point
        let x = -50.0;
        let ls = log_sigmoid(x);
        assert!(
            (ls - x).abs() < 1e-10,
            "log_sigmoid({x}) should ≈ {x}, got {ls}"
        );
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
    fn check_connected_basic() {
        assert!(check_connected(3, &[(0, 1), (1, 2)]));
        assert!(!check_connected(3, &[(0, 1)])); // node 2 isolated
        assert!(check_connected(1, &[]));
    }

    // ---------------------------------------------------------------
    // HeadToHead constructors
    // ---------------------------------------------------------------

    #[test]
    fn head_to_head_new_no_draws() {
        let h = HeadToHead::new("A", "B", 7, 3);
        assert_eq!(h.draws, 0);
        assert_eq!(h.total(), 10);
    }

    #[test]
    fn head_to_head_with_draws() {
        let h = HeadToHead::with_draws("A", "B", 5, 3, 2);
        assert_eq!(h.total(), 10);
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
        assert!(matches!(
            elo_from_winrate(0.0, 1000.0),
            Err(EloError::InvalidWinrate(_))
        ));
        assert!(matches!(
            elo_from_winrate(1.0, 1000.0),
            Err(EloError::InvalidWinrate(_))
        ));
    }

    // ---------------------------------------------------------------
    // compute_elo (no uncertainty) — the default path
    // ---------------------------------------------------------------

    #[test]
    fn compute_elo_without_uncertainty() {
        let records = vec![HeadToHead::new("A", "B", 7, 3)];
        let result = compute_elo(&records, &opts("B")).unwrap();
        assert!(result.get_elo("A").unwrap() > result.get_elo("B").unwrap());
        assert!((result.get_elo("B").unwrap() - 1000.0).abs() < 0.01);
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
        let records = vec![HeadToHead::new("A", "B", 1000, 1000)];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap();
        assert!(diff.abs() < 0.1, "50-50 should give diff ≈ 0, got {diff}");
    }

    #[test]
    fn two_player_75_25() {
        // diff = 400 * log10(3) ≈ 190.85
        let records = vec![HeadToHead::new("A", "B", 750, 250)];
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
        let records = vec![HeadToHead::new("A", "B", 900, 100)];
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
        let records = vec![HeadToHead::new("A", "B", 700, 300)];
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
        let records = vec![HeadToHead::new("A", "B", 7, 3)];
        for &anchor_elo in &[0.0, 500.0, 1000.0, 1500.0] {
            let opts = EloOptions::new("B").anchor_elo(anchor_elo);
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
        let r1 = vec![HeadToHead::new("A", "B", 7, 3)];
        let r2 = vec![HeadToHead::new("B", "A", 3, 7)];
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
        let records = vec![HeadToHead::with_draws("A", "B", 0, 0, 1000)];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap().abs();
        assert!(diff < 0.1, "all draws → diff ≈ 0, got {diff}");
    }

    #[test]
    fn win_expectancy_roundtrips_with_data() {
        // If A beats B at 70%, the fitted win_expectancy should be ≈ 0.7.
        let records = vec![HeadToHead::new("A", "B", 700, 300)];
        let result = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let es = result.win_expectancy("A", "B").unwrap();
        assert!(
            (es - 0.7).abs() < 0.01,
            "win_expectancy should ≈ 0.7, got {es}"
        );
    }

    #[test]
    fn three_players_transitive_ordering() {
        let records = vec![
            HeadToHead::new("A", "B", 70, 30),
            HeadToHead::new("B", "C", 60, 40),
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
            HeadToHead::new("A", "B", 700, 300),
            HeadToHead::new("B", "C", 600, 400),
            HeadToHead::new("A", "C", 800, 200),
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

    #[test]
    fn ratings_sorted_descending() {
        let records = vec![
            HeadToHead::new("A", "B", 70, 30),
            HeadToHead::new("B", "C", 60, 40),
        ];
        let result = compute_elo(&records, &opts("C")).unwrap();
        for w in result.ratings.windows(2) {
            assert!(
                w[0].elo >= w[1].elo,
                "ratings not sorted: {} ({}) before {} ({})",
                w[0].player_id,
                w[0].elo,
                w[1].player_id,
                w[1].elo
            );
        }
    }

    #[test]
    fn missing_player_lookups_return_none() {
        let records = vec![HeadToHead::new("A", "B", 7, 3)];
        let result = compute_elo(&records, &opts("B")).unwrap();
        assert!(result.get_elo("Z").is_none());
        assert!(result.get_rating("Z").is_none());
        assert!(result.elo_difference("A", "Z").is_none());
        assert!(result.win_expectancy("Z", "A").is_none());
    }

    // ---------------------------------------------------------------
    // Draw weight
    // ---------------------------------------------------------------

    #[test]
    fn draw_weight_behavior() {
        // All draws. With draw_weight=0.5 (default), equal ratings.
        // With draw_weight=0.7, A gets more credit → A rated higher.
        let records = vec![HeadToHead::with_draws("A", "B", 0, 0, 1000)];

        let equal = compute_elo(&records, &opts_no_prior("B")).unwrap();
        let diff_equal = equal.elo_difference("A", "B").unwrap();
        assert!(diff_equal.abs() < 0.1, "draw_weight=0.5 → diff ≈ 0");

        let biased = compute_elo(
            &records,
            &EloOptions::new("B").prior_games(0.0).draw_weight(0.7),
        )
        .unwrap();
        let diff_biased = biased.elo_difference("A", "B").unwrap();
        assert!(
            diff_biased > 10.0,
            "draw_weight=0.7 should give A higher rating, got diff={diff_biased}"
        );
    }

    // ---------------------------------------------------------------
    // Prior effects
    // ---------------------------------------------------------------

    #[test]
    fn prior_shrinks_ratings_toward_anchor() {
        // 1 win, 0 losses. Without prior → extreme rating.
        // With strong prior → pulled back toward anchor.
        let records = vec![HeadToHead::new("A", "B", 1, 0)];
        let strong = EloOptions::new("B").prior_games(100.0);
        let weak = EloOptions::new("B").prior_games(1.0);
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
        let records = vec![HeadToHead::new("A", "B", 7000, 3000)];
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
            let records = vec![HeadToHead::new("A", "B", n, n)];
            let (_, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
            unc.stderr("A").unwrap()
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
            let records = vec![HeadToHead::new("A", "B", n, n)];
            let (_, unc) = compute_elo_with_uncertainty(&records, &opts_no_prior("B")).unwrap();
            unc.stderr("A").unwrap()
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
        let records = vec![HeadToHead::new("A", "B", 50, 50)];
        let (_, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        assert!(
            unc.stderr("B").unwrap() < 0.01,
            "anchor stderr should be ≈ 0"
        );
    }

    #[test]
    fn diff_stderr_equals_individual_in_two_player() {
        // When B is the anchor (stderr=0, cov=0), diff_stderr(A,B) = stderr(A).
        let records = vec![HeadToHead::new("A", "B", 50, 50)];
        let (_, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        let a_stderr = unc.stderr("A").unwrap();
        let diff_stderr = unc.elo_difference_stderr("A", "B").unwrap();
        assert!(
            (diff_stderr - a_stderr).abs() < 0.1,
            "diff_stderr={diff_stderr}, a_stderr={a_stderr}"
        );
    }

    #[test]
    fn diff_stderr_benefits_from_covariance_in_three_player() {
        // With 3 players, diff_stderr(A,C) should be strictly smaller than
        // sqrt(stderr(A)² + stderr(C)²) because covariance helps.
        let records = vec![
            HeadToHead::new("A", "B", 500, 500),
            HeadToHead::new("B", "C", 500, 500),
        ];
        let (_, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        let se_a = unc.stderr("A").unwrap();
        let se_c = unc.stderr("C").unwrap();
        let naive = (se_a * se_a + se_c * se_c).sqrt();
        let actual = unc.elo_difference_stderr("A", "C").unwrap();
        assert!(
            actual < naive,
            "diff_stderr should strictly benefit from covariance: actual={actual}, naive={naive}"
        );
    }

    #[test]
    fn effective_game_count_scales_with_games() {
        let mk = |n: u32| -> f64 {
            let records = vec![HeadToHead::new("A", "B", n, n)];
            let (_, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
            unc.effective_game_count("A").unwrap()
        };
        let ess_100 = mk(50);
        let ess_1000 = mk(500);
        assert!(
            ess_1000 > ess_100,
            "more games → more effective games: ess_100={ess_100}, ess_1000={ess_1000}"
        );
        // Rough sanity: ESS should be in the same ballpark as actual games
        assert!(
            ess_100 > 10.0,
            "ESS for 100 games should be > 10, got {ess_100}"
        );
    }

    #[test]
    fn uncertainty_missing_player_returns_none() {
        let records = vec![HeadToHead::new("A", "B", 7, 3)];
        let (_, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        assert!(unc.stderr("Z").is_none());
        assert!(unc.effective_game_count("Z").is_none());
        assert!(unc.elo_difference_stderr("A", "Z").is_none());
    }

    // ---------------------------------------------------------------
    // LOS (likelihood of superiority)
    // ---------------------------------------------------------------

    #[test]
    fn los_equal_players_is_half() {
        let records = vec![HeadToHead::new("A", "B", 500, 500)];
        let (result, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        let los = unc.likelihood_of_superiority(&result, "A", "B").unwrap();
        assert!(
            (los - 0.5).abs() < 0.05,
            "LOS for equal players ≈ 0.5, got {los}"
        );
    }

    #[test]
    fn los_dominant_player_near_one() {
        let records = vec![HeadToHead::new("A", "B", 900, 100)];
        let (result, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        let los = unc.likelihood_of_superiority(&result, "A", "B").unwrap();
        assert!(
            los > 0.99,
            "dominant player LOS should be near 1.0, got {los}"
        );
    }

    #[test]
    fn los_symmetry() {
        // LOS(A,B) + LOS(B,A) ≈ 1
        let records = vec![HeadToHead::new("A", "B", 60, 40)];
        let (result, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        let los_ab = unc.likelihood_of_superiority(&result, "A", "B").unwrap();
        let los_ba = unc.likelihood_of_superiority(&result, "B", "A").unwrap();
        assert!(
            (los_ab + los_ba - 1.0).abs() < 0.01,
            "LOS(A,B) + LOS(B,A) should = 1.0, got {}",
            los_ab + los_ba
        );
    }

    #[test]
    fn los_self_is_half() {
        let records = vec![HeadToHead::new("A", "B", 60, 40)];
        let (result, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
        assert_eq!(
            unc.likelihood_of_superiority(&result, "A", "A").unwrap(),
            0.5
        );
    }

    #[test]
    fn los_monotonicity() {
        // Stronger win records → higher LOS
        let scenarios = [(60, 40), (70, 30), (90, 10)];
        let mut prev_los = 0.0;
        for (wa, wb) in scenarios {
            let records = vec![HeadToHead::new("A", "B", wa, wb)];
            let (result, unc) = compute_elo_with_uncertainty(&records, &opts("B")).unwrap();
            let los = unc.likelihood_of_superiority(&result, "A", "B").unwrap();
            assert!(
                los > prev_los,
                "LOS should increase: {wa}/{wb} gave {los}, prev was {prev_los}"
            );
            prev_los = los;
        }
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
    fn error_too_few_players() {
        // Self-play record: only one unique player
        let records = vec![HeadToHead::new("A", "A", 5, 5)];
        assert!(matches!(
            compute_elo(&records, &opts("A")),
            Err(EloError::TooFewPlayers)
        ));
    }

    #[test]
    fn error_anchor_not_found() {
        let records = vec![HeadToHead::new("A", "B", 5, 5)];
        assert!(matches!(
            compute_elo(&records, &opts("Z")),
            Err(EloError::AnchorNotFound(_))
        ));
    }

    #[test]
    fn error_disconnected_graph() {
        let records = vec![
            HeadToHead::new("A", "B", 5, 5),
            HeadToHead::new("C", "D", 5, 5),
        ];
        assert!(matches!(
            compute_elo(&records, &opts("A")),
            Err(EloError::DisconnectedGraph)
        ));
    }

    #[test]
    fn zero_total_games_skipped() {
        // A record with 0 wins, 0 losses, 0 draws should be ignored.
        // If it's the only record for some players, they won't appear.
        let records = vec![
            HeadToHead::new("A", "B", 7, 3),
            HeadToHead::new("A", "B", 0, 0), // zero-total, should be skipped
        ];
        let result = compute_elo(&records, &opts("B")).unwrap();
        let diff = result.elo_difference("A", "B").unwrap();
        // Should match the 7-3 result only
        let single = compute_elo(&[HeadToHead::new("A", "B", 7, 3)], &opts("B")).unwrap();
        let diff_single = single.elo_difference("A", "B").unwrap();
        assert!(
            (diff - diff_single).abs() < 0.01,
            "zero-total record should have no effect"
        );
    }
}

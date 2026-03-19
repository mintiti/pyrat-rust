//! Naive search bot: simultaneous-game tree search with iterative deepening and PV ordering.
//!
//! A starting point for search, not a competitive engine.
//!
//! Both players independently maximize their own score. Since the game is constant-sum
//! (cheese collected by one player is unavailable to the other), this is equivalent to
//! minimax. Maximizing our score and maximizing opponent's score produces the same
//! result as maximizing ours while minimizing theirs.
//!
//! Iterative deepening with no depth cap. The search cooperates with `should_stop()` and
//! deepens until time runs out. Tied PVs (principal variations) from the previous depth
//! guide move ordering at every level of the tree: preferred moves are explored first,
//! giving deeper searches better chances of finding strong lines early.
//!
//! What's here: iterative deepening, PV move ordering, effective_moves filtering.
//!
//! What's not:
//! - No evaluation heuristic: only raw scores at leaf nodes, no positional awareness
//! - No pruning: full tree explored at each depth
//! - No transposition table: same positions re-evaluated when reached via different paths
//! - No simultaneous-move equilibrium: sequential best-response, not Nash
//!
//! SDK features: GameSim, effective_moves, should_stop, send_info.

use pyrat_sdk::{Bot, Context, Direction, GameSim, GameState, InfoParams, Options, Player};
use rand::prelude::SliceRandom;

struct Search {
    am_player1: bool,
    nodes: u64,
}

impl Options for Search {}

impl Bot for Search {
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction {
        self.am_player1 = state.my_player() == Player::Player1;
        self.nodes = 0;
        let mut sim = state.to_sim();

        let mut best_move = Direction::Stay;
        let mut best_score = f32::NEG_INFINITY;
        let mut pvs: Vec<Vec<Direction>> = Vec::new();

        for depth in 1.. {
            if ctx.should_stop() {
                break;
            }

            let (dir, score, new_pvs) = self.search_root(&mut sim, depth, state, ctx, &pvs);

            if score > best_score {
                best_move = dir;
                best_score = score;
            }

            pvs = new_pvs;
        }

        best_move
    }
}

impl Search {
    fn search_root(
        &mut self,
        sim: &mut GameSim,
        depth: i32,
        state: &GameState,
        ctx: &Context,
        pvs: &[Vec<Direction>],
    ) -> (Direction, f32, Vec<Vec<Direction>>) {
        let mut best_move = Direction::Stay;
        let mut best_score = f32::NEG_INFINITY;
        let mut tied_pvs: Vec<Vec<Direction>> = Vec::new();

        let my_pos = if self.am_player1 {
            sim.player1_position()
        } else {
            sim.player2_position()
        };
        let opp_pos = if self.am_player1 {
            sim.player2_position()
        } else {
            sim.player1_position()
        };

        let my_moves = order_moves(&state.effective_moves(Some(my_pos)), pvs);
        let opp_moves = state.effective_moves(Some(opp_pos));

        for my_dir in &my_moves {
            if ctx.should_stop() {
                break;
            }

            let child_suffixes: Vec<&[Direction]> = if pvs.is_empty() {
                Vec::new()
            } else {
                pvs.iter()
                    .filter(|pv| pv.first() == Some(my_dir))
                    .map(|pv| &pv[1..])
                    .collect()
            };

            let (our_score_vs_opp_best, _, pv_vs_opp_best) = match self
                .opponent_best_response(sim, *my_dir, &opp_moves, depth, state, ctx, &child_suffixes)
            {
                Some(result) => result,
                None => break,
            };

            let mut pv = vec![*my_dir];
            pv.extend(pv_vs_opp_best);

            if our_score_vs_opp_best > best_score {
                best_score = our_score_vs_opp_best;
                best_move = *my_dir;
                ctx.send_info(&InfoParams {
                    multipv: 1,
                    depth: depth as u16,
                    nodes: self.nodes as u32,
                    score: Some(best_score),
                    pv: &pv,
                    message: &format!("depth {depth}: {best_move:?} ({best_score:.1})"),
                    ..InfoParams::for_player(state.my_player())
                });
                tied_pvs = vec![pv];
            } else if our_score_vs_opp_best == best_score {
                tied_pvs.push(pv.clone());
                ctx.send_info(&InfoParams {
                    multipv: tied_pvs.len() as u16,
                    depth: depth as u16,
                    nodes: self.nodes as u32,
                    score: Some(best_score),
                    pv: &pv,
                    message: &format!(
                        "depth {depth}: {:?} ({best_score:.1}) [pv {}]",
                        pv[0],
                        tied_pvs.len()
                    ),
                    ..InfoParams::for_player(state.my_player())
                });
            }
        }

        (best_move, best_score, tied_pvs)
    }

    fn search(
        &mut self,
        sim: &mut GameSim,
        depth: i32,
        state: &GameState,
        ctx: &Context,
        pv_suffixes: &[&[Direction]],
    ) -> Option<(f32, f32, Vec<Direction>)> {
        if depth == 0 || sim.check_game_over() {
            let (our, opp) = self.evaluate(sim);
            return Some((our, opp, Vec::new()));
        }

        if ctx.should_stop() {
            return None;
        }

        let my_pos = if self.am_player1 {
            sim.player1_position()
        } else {
            sim.player2_position()
        };
        let opp_pos = if self.am_player1 {
            sim.player2_position()
        } else {
            sim.player1_position()
        };

        let my_moves = order_moves(&state.effective_moves(Some(my_pos)), pv_suffixes);
        let opp_moves = state.effective_moves(Some(opp_pos));

        let mut best_our = f32::NEG_INFINITY;
        let mut best_opp_at_our_best = 0.0_f32;
        let mut best_pv = Vec::new();

        for my_dir in &my_moves {
            if ctx.should_stop() {
                return None;
            }

            let child_suffixes: Vec<&[Direction]> = if pv_suffixes.is_empty() {
                Vec::new()
            } else {
                pv_suffixes
                    .iter()
                    .filter(|s| s.first() == Some(my_dir))
                    .map(|s| &s[1..])
                    .collect()
            };

            let (our_when_opp_best, best_opp_score, pv_when_opp_best) = match self
                .opponent_best_response(sim, *my_dir, &opp_moves, depth, state, ctx, &child_suffixes)
            {
                Some(result) => result,
                None => return None,
            };

            if our_when_opp_best > best_our {
                best_our = our_when_opp_best;
                best_opp_at_our_best = best_opp_score;
                let mut pv = vec![*my_dir];
                pv.extend(pv_when_opp_best);
                best_pv = pv;
            }
        }

        Some((best_our, best_opp_at_our_best, best_pv))
    }

    fn opponent_best_response(
        &mut self,
        sim: &mut GameSim,
        my_dir: Direction,
        opp_moves: &[Direction],
        depth: i32,
        state: &GameState,
        ctx: &Context,
        child_suffixes: &[&[Direction]],
    ) -> Option<(f32, f32, Vec<Direction>)> {
        let mut best_opp_score = f32::NEG_INFINITY;
        let mut our_when_opp_best = f32::NEG_INFINITY;
        let mut pv_when_opp_best = Vec::new();

        for opp_dir in opp_moves {
            let (p1_dir, p2_dir) = self.assign_moves(my_dir, *opp_dir);
            let undo = sim.make_move(p1_dir, p2_dir);
            self.nodes += 1;

            let (our, opp, child_pv) = if depth <= 1 || sim.check_game_over() {
                let (our, opp) = self.evaluate(sim);
                (our, opp, Vec::new())
            } else {
                match self.search(sim, depth - 1, state, ctx, child_suffixes) {
                    Some(result) => result,
                    None => {
                        sim.unmake_move(undo);
                        return None;
                    },
                }
            };

            sim.unmake_move(undo);

            if opp > best_opp_score {
                best_opp_score = opp;
                our_when_opp_best = our;
                pv_when_opp_best = child_pv;
            }
        }

        Some((our_when_opp_best, best_opp_score, pv_when_opp_best))
    }

    fn evaluate(&self, sim: &GameSim) -> (f32, f32) {
        if self.am_player1 {
            (sim.player1_score(), sim.player2_score())
        } else {
            (sim.player2_score(), sim.player1_score())
        }
    }

    fn assign_moves(&self, my_dir: Direction, opp_dir: Direction) -> (Direction, Direction) {
        if self.am_player1 {
            (my_dir, opp_dir)
        } else {
            (opp_dir, my_dir)
        }
    }
}

fn order_moves<S: AsRef<[Direction]>>(moves: &[Direction], pv_suffixes: &[S]) -> Vec<Direction> {
    if pv_suffixes.is_empty() {
        let mut shuffled = moves.to_vec();
        shuffled.shuffle(&mut rand::rng());
        return shuffled;
    }

    let mut pv_firsts: Vec<Direction> = Vec::new();
    for suffix in pv_suffixes {
        if let Some(&first) = suffix.as_ref().first() {
            if !pv_firsts.contains(&first) {
                pv_firsts.push(first);
            }
        }
    }

    let mut preferred: Vec<Direction> = moves
        .iter()
        .copied()
        .filter(|d| pv_firsts.contains(d))
        .collect();
    let mut rest: Vec<Direction> = moves
        .iter()
        .copied()
        .filter(|d| !pv_firsts.contains(d))
        .collect();

    preferred.shuffle(&mut rand::rng());
    rest.shuffle(&mut rand::rng());

    preferred.extend(rest);
    preferred
}

fn main() {
    pyrat_sdk::run(
        Search {
            am_player1: false,
            nodes: 0,
        },
        "Search.rs",
        "mintiti",
    );
}

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
//! SDK features: GameSim, effective_moves, should_stop, send_info, send_provisional.

use pyrat_sdk::{Bot, Context, Direction, GameSim, GameState, InfoParams, Options, Player};
use rand::prelude::SliceRandom;

struct Search {
    am_player1: bool,
    nodes: u64,
    /// Saved from preprocess(), consumed on first think().
    prep: Option<PrepState>,
}

struct PrepState {
    best_move: Direction,
    best_score: f32,
    pvs: Vec<Vec<Direction>>,
    depth: i32,
}

impl Options for Search {}

impl Bot for Search {
    fn preprocess(&mut self, state: &GameState, ctx: &Context) {
        self.am_player1 = state.my_player() == Player::Player1;
        self.nodes = 0;
        let mut sim = state.to_sim();

        let (best_move, best_score, pvs, depth) = self.iterative_deepen(
            &mut sim,
            state,
            ctx,
            1,
            Direction::Stay,
            f32::NEG_INFINITY,
            Vec::new(),
        );

        self.prep = Some(PrepState {
            best_move,
            best_score,
            pvs,
            depth,
        });
    }

    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction {
        self.am_player1 = state.my_player() == Player::Player1;
        self.nodes = 0;
        let mut sim = state.to_sim();

        let (start_depth, best_move, best_score, pvs) = match self.prep.take() {
            Some(p) => (p.depth + 1, p.best_move, p.best_score, p.pvs),
            None => (1, Direction::Stay, f32::NEG_INFINITY, Vec::new()),
        };

        let (best_move, _, _, _) =
            self.iterative_deepen(&mut sim, state, ctx, start_depth, best_move, best_score, pvs);

        best_move
    }
}

impl Search {
    #[allow(clippy::too_many_arguments)]
    fn iterative_deepen(
        &mut self,
        sim: &mut GameSim,
        state: &GameState,
        ctx: &Context,
        start_depth: i32,
        mut best_move: Direction,
        mut best_score: f32,
        mut pvs: Vec<Vec<Direction>>,
    ) -> (Direction, f32, Vec<Vec<Direction>>, i32) {
        let mut last_depth = start_depth.saturating_sub(1);

        for depth in start_depth.. {
            if ctx.should_stop() {
                break;
            }

            let (dir, score, new_pvs) = self.search_root(sim, depth, state, ctx, &pvs);

            if score > best_score {
                best_move = dir;
                best_score = score;
            }

            pvs = new_pvs;
            last_depth = depth;
            ctx.send_provisional(best_move);
        }

        (best_move, best_score, pvs, last_depth)
    }

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
        let mut best_opp_score = 0.0_f32;
        // (our_pv, opp_pv) pairs
        let mut tied_pvs: Vec<(Vec<Direction>, Vec<Direction>)> = Vec::new();

        let my_player = state.my_player();
        let opp_player = if my_player == Player::Player1 {
            Player::Player2
        } else {
            Player::Player1
        };

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

            let (our_score_vs_opp_best, opp_score, pv_vs_opp_best, opp_pv) = match self
                .opponent_best_response(sim, *my_dir, &opp_moves, depth, state, ctx, &child_suffixes)
            {
                Some(result) => result,
                None => break,
            };

            let mut pv = vec![*my_dir];
            pv.extend(pv_vs_opp_best);

            if our_score_vs_opp_best > best_score {
                best_score = our_score_vs_opp_best;
                best_opp_score = opp_score;
                best_move = *my_dir;

                let (my_target, opp_target) =
                    find_targets(sim, &pv, &opp_pv, self.am_player1);

                ctx.send_info(&InfoParams {
                    multipv: 1,
                    depth: depth as u16,
                    nodes: self.nodes as u32,
                    score: Some(best_score),
                    pv: &pv,
                    target: my_target,
                    message: &format!("depth {depth}: {best_move:?} ({best_score:.1})"),
                    ..InfoParams::for_player(my_player)
                });
                ctx.send_info(&InfoParams {
                    multipv: 1,
                    depth: depth as u16,
                    nodes: self.nodes as u32,
                    score: Some(opp_score),
                    pv: &opp_pv,
                    target: opp_target,
                    message: &format!("depth {depth}: {:?} ({opp_score:.1})", opp_pv.first().unwrap_or(&Direction::Stay)),
                    ..InfoParams::for_player(opp_player)
                });

                tied_pvs = vec![(pv, opp_pv)];
            } else if our_score_vs_opp_best == best_score {
                let (my_target, opp_target) =
                    find_targets(sim, &pv, &opp_pv, self.am_player1);

                tied_pvs.push((pv.clone(), opp_pv.clone()));

                ctx.send_info(&InfoParams {
                    multipv: tied_pvs.len() as u16,
                    depth: depth as u16,
                    nodes: self.nodes as u32,
                    score: Some(best_score),
                    pv: &pv,
                    target: my_target,
                    message: &format!(
                        "depth {depth}: {:?} ({best_score:.1}) [pv {}]",
                        pv[0],
                        tied_pvs.len()
                    ),
                    ..InfoParams::for_player(my_player)
                });
                ctx.send_info(&InfoParams {
                    multipv: tied_pvs.len() as u16,
                    depth: depth as u16,
                    nodes: self.nodes as u32,
                    score: Some(best_opp_score),
                    pv: &opp_pv,
                    target: opp_target,
                    message: &format!(
                        "depth {depth}: {:?} ({best_opp_score:.1}) [pv {}]",
                        opp_pv.first().unwrap_or(&Direction::Stay),
                        tied_pvs.len()
                    ),
                    ..InfoParams::for_player(opp_player)
                });
            }
        }

        let our_pvs = tied_pvs.into_iter().map(|(our, _)| our).collect();
        (best_move, best_score, our_pvs)
    }

    fn search(
        &mut self,
        sim: &mut GameSim,
        depth: i32,
        state: &GameState,
        ctx: &Context,
        pv_suffixes: &[&[Direction]],
    ) -> Option<(f32, f32, Vec<Direction>, Vec<Direction>)> {
        if depth == 0 || sim.check_game_over() {
            let (our, opp) = self.evaluate(sim);
            return Some((our, opp, Vec::new(), Vec::new()));
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
        let mut best_opp_pv = Vec::new();

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

            let (our_when_opp_best, best_opp_score, pv_when_opp_best, opp_pv) = self
                .opponent_best_response(sim, *my_dir, &opp_moves, depth, state, ctx, &child_suffixes)?;

            if our_when_opp_best > best_our {
                best_our = our_when_opp_best;
                best_opp_at_our_best = best_opp_score;
                let mut pv = vec![*my_dir];
                pv.extend(pv_when_opp_best);
                best_pv = pv;
                best_opp_pv = opp_pv;
            }
        }

        Some((best_our, best_opp_at_our_best, best_pv, best_opp_pv))
    }

    #[allow(clippy::too_many_arguments)]
    fn opponent_best_response(
        &mut self,
        sim: &mut GameSim,
        my_dir: Direction,
        opp_moves: &[Direction],
        depth: i32,
        state: &GameState,
        ctx: &Context,
        child_suffixes: &[&[Direction]],
    ) -> Option<(f32, f32, Vec<Direction>, Vec<Direction>)> {
        let mut best_opp_score = f32::NEG_INFINITY;
        let mut our_when_opp_best = f32::NEG_INFINITY;
        let mut pv_when_opp_best = Vec::new();
        let mut opp_pv_when_opp_best = Vec::new();

        for opp_dir in opp_moves {
            let (p1_dir, p2_dir) = self.assign_moves(my_dir, *opp_dir);
            let undo = sim.make_move(p1_dir, p2_dir);
            self.nodes += 1;

            let (our, opp, child_pv, child_opp_pv) = if depth <= 1 || sim.check_game_over() {
                let (our, opp) = self.evaluate(sim);
                (our, opp, Vec::new(), Vec::new())
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
                let mut opv = vec![*opp_dir];
                opv.extend(child_opp_pv);
                opp_pv_when_opp_best = opv;
            }
        }

        Some((our_when_opp_best, best_opp_score, pv_when_opp_best, opp_pv_when_opp_best))
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

type Target = Option<(u8, u8)>;

fn find_targets(
    sim: &mut GameSim,
    our_pv: &[Direction],
    opp_pv: &[Direction],
    am_player1: bool,
) -> (Target, Target) {
    let mut clone = sim.clone();
    let len = our_pv.len().max(opp_pv.len());

    let mut our_target = None;
    let mut opp_target = None;

    let initial_our = if am_player1 { clone.player1_score() } else { clone.player2_score() };
    let initial_opp = if am_player1 { clone.player2_score() } else { clone.player1_score() };
    let mut prev_our = initial_our;
    let mut prev_opp = initial_opp;

    for i in 0..len {
        let our_dir = our_pv.get(i).copied().unwrap_or(Direction::Stay);
        let opp_dir = opp_pv.get(i).copied().unwrap_or(Direction::Stay);

        let (p1_dir, p2_dir) = if am_player1 {
            (our_dir, opp_dir)
        } else {
            (opp_dir, our_dir)
        };
        clone.make_move(p1_dir, p2_dir);

        let cur_our = if am_player1 { clone.player1_score() } else { clone.player2_score() };
        let cur_opp = if am_player1 { clone.player2_score() } else { clone.player1_score() };

        if our_target.is_none() && cur_our > prev_our {
            let pos = if am_player1 { clone.player1_position() } else { clone.player2_position() };
            our_target = Some((pos.x, pos.y));
        }
        if opp_target.is_none() && cur_opp > prev_opp {
            let pos = if am_player1 { clone.player2_position() } else { clone.player1_position() };
            opp_target = Some((pos.x, pos.y));
        }

        if our_target.is_some() && opp_target.is_some() {
            break;
        }

        prev_our = cur_our;
        prev_opp = cur_opp;
    }

    (our_target, opp_target)
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
            prep: None,
        },
        "Search.rs",
        "mintiti",
    );
}

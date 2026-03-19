//! Search bot — best-response tree search with iterative deepening.
//!
//! Uses GameSim (make_move / unmake_move) for efficient game-tree exploration.
//! Both players independently maximize their own score — the opponent doesn't
//! try to hurt us, they try to help themselves.

use pyrat_sdk::{Bot, Context, DeriveOptions, Direction, GameSim, GameState, InfoParams, Player};
use rand::prelude::SliceRandom;

#[derive(DeriveOptions)]
struct Search {
    #[spin(default = 6, min = 1, max = 12)]
    max_depth: i32,

    // Internal state — no option attributes, ignored by derive.
    am_player1: bool,
    nodes: u64,
}

impl Bot for Search {
    fn think(&mut self, state: &GameState, ctx: &Context) -> Direction {
        self.am_player1 = state.my_player() == Player::Player1;
        self.nodes = 0;
        let mut sim = state.simulate();

        let mut best_move = Direction::Stay;

        // IDDFS: only update best_move when a depth completes fully.
        for depth in 1..=self.max_depth {
            if ctx.should_stop() {
                break;
            }

            let result = self.search_root(&mut sim, depth, state, ctx);
            let Some((dir, score, pv)) = result else {
                break; // timed out mid-search — keep previous best
            };

            best_move = dir;

            ctx.send_info(&InfoParams {
                multipv: 1,
                depth: depth as u16,
                nodes: self.nodes as u32,
                score,
                pv: &pv,
                message: &format!("depth {depth}: {best_move:?} ({score:.1})"),
                ..InfoParams::for_player(state.my_player())
            });
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
    ) -> Option<(Direction, f32, Vec<Direction>)> {
        let mut best_move = Direction::Stay;
        let mut best_score = f32::NEG_INFINITY;
        let mut best_child_pv = Vec::new();

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

        let mut my_moves = state.effective_moves(Some(my_pos));
        my_moves.shuffle(&mut rand::rng());
        let opp_moves = state.effective_moves(Some(opp_pos));

        for my_dir in &my_moves {
            if ctx.should_stop() {
                return None;
            }

            // Opponent picks the move that maximizes THEIR score.
            let mut best_opp_score = f32::NEG_INFINITY;
            let mut our_score_vs_opp_best = f32::NEG_INFINITY;
            let mut pv_vs_opp_best = Vec::new();

            for opp_dir in &opp_moves {
                let (p1_dir, p2_dir) = self.assign_moves(*my_dir, *opp_dir);
                let undo = sim.make_move(p1_dir, p2_dir);
                self.nodes += 1;

                let (our, opp, child_pv) = if depth <= 1 || sim.is_game_over() {
                    let (our, opp) = self.evaluate(sim);
                    (our, opp, Vec::new())
                } else {
                    match self.search(sim, depth - 1, state, ctx) {
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
                    our_score_vs_opp_best = our;
                    pv_vs_opp_best = child_pv;
                }
            }

            if our_score_vs_opp_best > best_score {
                best_score = our_score_vs_opp_best;
                best_move = *my_dir;
                best_child_pv = pv_vs_opp_best;
            }
        }

        let mut pv = vec![best_move];
        pv.extend(best_child_pv);
        Some((best_move, best_score, pv))
    }

    fn search(
        &mut self,
        sim: &mut GameSim,
        depth: i32,
        state: &GameState,
        ctx: &Context,
    ) -> Option<(f32, f32, Vec<Direction>)> {
        if depth == 0 || sim.is_game_over() {
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

        let my_moves = state.effective_moves(Some(my_pos));
        let opp_moves = state.effective_moves(Some(opp_pos));

        let mut best_our = f32::NEG_INFINITY;
        let mut best_opp_at_our_best = 0.0_f32;
        let mut best_pv = Vec::new();

        for my_dir in &my_moves {
            if ctx.should_stop() {
                return None;
            }
            let mut best_opp_score = f32::NEG_INFINITY;
            let mut our_when_opp_best = f32::NEG_INFINITY;
            let mut pv_when_opp_best = Vec::new();

            for opp_dir in &opp_moves {
                let (p1_dir, p2_dir) = self.assign_moves(*my_dir, *opp_dir);
                let undo = sim.make_move(p1_dir, p2_dir);
                self.nodes += 1;

                let (our, opp, child_pv) = if depth <= 1 || sim.is_game_over() {
                    let (our, opp) = self.evaluate(sim);
                    (our, opp, Vec::new())
                } else {
                    match self.search(sim, depth - 1, state, ctx) {
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

fn main() {
    pyrat_sdk::run(
        Search {
            max_depth: 6,
            am_player1: false,
            nodes: 0,
        },
        "Search",
        "PyRat SDK",
    );
}

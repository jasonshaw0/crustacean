//! Iterative-deepening alpha-beta search with tactical leaf search and cancellation.
use crate::board::{Board, Color, Kind, Move, Outcome, PositionKey};
use crate::eval::evaluate;
#[cfg(target_arch = "wasm32")]
use crate::wasm::Instant;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

pub const MATE: i32 = 30_000;
pub const MAX_DEPTH: u8 = 64;
pub const DEFAULT_DEPTH: u8 = 6;
pub const DEFAULT_NODES: u64 = 2_000_000;
pub const MAX_PLY: usize = 128;
const MAX_QUIESCENCE: u8 = 24;
const STATIC_LIMIT: i32 = MATE - MAX_PLY as i32 - 1;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// None uses DEFAULT_DEPTH. Explicit depths are clamped to 1..=MAX_DEPTH.
    pub depth: Option<u8>,
    pub node_limit: u64,
    pub time_limit: Option<Duration>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            depth: None,
            node_limit: DEFAULT_NODES,
            time_limit: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Nodes,
    Time,
    Cancelled,
    PlyLimit,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    pub score: i32,
    pub planned_depth: u8,
    pub completed_depth: u8,
    pub nodes: u64,
    pub qnodes: u64,
    pub selective_depth: usize,
    pub elapsed: Duration,
    pub pv: Vec<Move>,
    pub stop_reason: Option<StopReason>,
    pub budget_exhausted: bool,
    pub outcome: Option<Outcome>,
}

pub fn planned_depth(config: Config) -> u8 {
    config.depth.unwrap_or(DEFAULT_DEPTH).clamp(1, MAX_DEPTH)
}

fn static_score(board: &Board) -> i32 {
    evaluate(board).clamp(-STATIC_LIMIT, STATIC_LIMIT)
}
fn terminal_score(outcome: Outcome, ply: usize) -> i32 {
    if matches!(outcome, Outcome::Checkmate(_)) {
        -MATE + ply as i32
    } else {
        0
    }
}
fn is_capture(board: &Board, m: Move) -> bool {
    board.squares[m.to].is_some()
        || (board.ep == Some(m.to) && board.squares[m.from].is_some_and(|p| p.kind == Kind::Pawn))
}
fn color_index(color: Color) -> usize {
    usize::from(color == Color::Black)
}

struct Search<'a> {
    nodes: u64,
    qnodes: u64,
    selective_depth: usize,
    config: Config,
    start: Instant,
    cancelled: &'a AtomicBool,
    pv: Vec<Vec<Move>>,
    previous_pv: Vec<Move>,
    killers: Vec<[Option<Move>; 2]>,
    history: [[[i32; 64]; 64]; 2],
}

impl Search<'_> {
    fn visit(&mut self, ply: usize, quiescence: bool) -> Result<(), StopReason> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(StopReason::Cancelled);
        }
        if self.nodes >= self.config.node_limit {
            return Err(StopReason::Nodes);
        }
        // Poll the clock frequently without requiring an OS timer per node.
        if self.nodes.is_multiple_of(64)
            && self
                .config
                .time_limit
                .is_some_and(|limit| self.start.elapsed() >= limit)
        {
            return Err(StopReason::Time);
        }
        self.nodes += 1;
        self.qnodes += u64::from(quiescence);
        self.selective_depth = self.selective_depth.max(ply);
        self.pv[ply].clear();
        Ok(())
    }

    fn order(&self, board: &Board, moves: &mut [Move], ply: usize) {
        moves.sort_by_key(|&m| {
            if self.previous_pv.get(ply) == Some(&m) {
                return -2_000_000;
            }
            let promotion = m.promotion.map(|k| k.value()).unwrap_or(0);
            let priority = if is_capture(board, m) {
                let victim = board.squares[m.to].map(|p| p.kind.value()).unwrap_or(100);
                1_000_000 + victim * 10 - board.squares[m.from].unwrap().kind.value() + promotion
            } else if promotion > 0 {
                900_000 + promotion
            } else if self.killers[ply][0] == Some(m) {
                800_000
            } else if self.killers[ply][1] == Some(m) {
                700_000
            } else {
                self.history[color_index(board.side)][m.from][m.to]
            };
            -priority
        });
    }

    fn update_pv(&mut self, ply: usize, m: Move) {
        let child = self.pv[ply + 1].clone();
        self.pv[ply].clear();
        self.pv[ply].push(m);
        self.pv[ply].extend(child);
    }

    fn record_cutoff(&mut self, board: &Board, m: Move, ply: usize, depth: u8) {
        if is_capture(board, m) || m.promotion.is_some() {
            return;
        }
        if self.killers[ply][0] != Some(m) {
            self.killers[ply][1] = self.killers[ply][0];
            self.killers[ply][0] = Some(m);
        }
        let value = &mut self.history[color_index(board.side)][m.from][m.to];
        *value = (*value + i32::from(depth).pow(2)).min(600_000);
    }

    fn negamax(
        &mut self,
        board: &Board,
        depth: u8,
        ply: usize,
        mut alpha: i32,
        beta: i32,
        history: &mut Vec<PositionKey>,
    ) -> Result<i32, StopReason> {
        if depth == 0 {
            return self.quiescence(board, ply, 0, alpha, beta, history);
        }
        self.visit(ply, false)?;
        let mut moves = board.legal_moves();
        if let Some(outcome) = board.outcome_with_moves(history, &moves) {
            return Ok(terminal_score(outcome, ply));
        }
        if ply + 1 >= MAX_PLY {
            return Err(StopReason::PlyLimit);
        }
        self.order(board, &mut moves, ply);
        let mut best = -MATE - 1;
        for m in moves {
            let child = board.after(m);
            history.push(child.key());
            let value = self.negamax(&child, depth - 1, ply + 1, -beta, -alpha, history);
            history.pop();
            let score = -value?;
            best = best.max(score);
            if score > alpha {
                alpha = score;
                self.update_pv(ply, m);
            }
            if alpha >= beta {
                self.record_cutoff(board, m, ply, depth);
                break;
            }
        }
        Ok(best)
    }

    fn quiescence(
        &mut self,
        board: &Board,
        ply: usize,
        qdepth: u8,
        mut alpha: i32,
        beta: i32,
        history: &mut Vec<PositionKey>,
    ) -> Result<i32, StopReason> {
        self.visit(ply, true)?;
        let mut moves = board.legal_moves();
        if let Some(outcome) = board.outcome_with_moves(history, &moves) {
            return Ok(terminal_score(outcome, ply));
        }
        let checked = board.in_check(board.side);
        let stand_pat = static_score(board);
        if qdepth >= MAX_QUIESCENCE || ply + 1 >= MAX_PLY {
            // Never use stand-pat to pretend a checked position is quiet.
            return if checked {
                Err(StopReason::PlyLimit)
            } else {
                Ok(stand_pat)
            };
        }
        let mut best = if checked { -MATE - 1 } else { stand_pat };
        if !checked {
            if stand_pat >= beta {
                return Ok(stand_pat);
            }
            alpha = alpha.max(stand_pat);
            moves.retain(|&m| is_capture(board, m) || m.promotion.is_some());
        }
        // In check, every evasion remains, including quiet blocks and king moves.
        self.order(board, &mut moves, ply);
        for m in moves {
            let child = board.after(m);
            history.push(child.key());
            let value = self.quiescence(&child, ply + 1, qdepth + 1, -beta, -alpha, history);
            history.pop();
            let score = -value?;
            best = best.max(score);
            if score > alpha {
                alpha = score;
                self.update_pv(ply, m);
            }
            if alpha >= beta {
                break;
            }
        }
        Ok(best)
    }
}

/// Analyze without external cancellation or progress callbacks.
pub fn search(board: &Board, config: Config, history: &[PositionKey]) -> SearchResult {
    search_with_control(board, config, history, &AtomicBool::new(false), |_| {})
}

/// History includes the root once per occurrence. Empty history inserts the root.
/// The callback runs after each completed iteration, on the calling thread.
pub fn search_with_control(
    board: &Board,
    config: Config,
    history: &[PositionKey],
    cancelled: &AtomicBool,
    mut on_iteration: impl FnMut(&SearchResult),
) -> SearchResult {
    let start = Instant::now();
    let target = planned_depth(config);
    let mut history = history.to_vec();
    let root_key = board.key();
    if history.last() != Some(&root_key) {
        history.push(root_key);
    }
    let mut moves = board.legal_moves();
    let outcome = board.outcome_with_moves(&history, &moves);
    let mut result = SearchResult {
        best_move: None,
        score: outcome.map(|o| terminal_score(o, 0)).unwrap_or(0),
        planned_depth: target,
        completed_depth: 0,
        nodes: 0,
        qnodes: 0,
        selective_depth: 0,
        elapsed: start.elapsed(),
        pv: Vec::new(),
        stop_reason: None,
        budget_exhausted: false,
        outcome,
    };
    if outcome.is_some() {
        return result;
    }
    let mut worker = Search {
        nodes: 0,
        qnodes: 0,
        selective_depth: 0,
        config,
        start,
        cancelled,
        pv: vec![Vec::new(); MAX_PLY],
        previous_pv: Vec::new(),
        killers: vec![[None; 2]; MAX_PLY],
        history: [[[0; 64]; 64]; 2],
    };
    worker.order(board, &mut moves, 0);
    result.best_move = moves.first().copied();
    result.score = static_score(board);
    result.pv = result.best_move.into_iter().collect();
    for depth in 1..=target {
        let mut best = None;
        let mut best_pv = Vec::new();
        let mut alpha = -MATE - 1;
        let mut complete = true;
        for &m in &moves {
            let child = board.after(m);
            history.push(child.key());
            let value = worker.negamax(&child, depth - 1, 1, -MATE - 1, -alpha, &mut history);
            history.pop();
            match value {
                Ok(value) => {
                    let score = -value;
                    if score > alpha {
                        alpha = score;
                        best = Some(m);
                        best_pv = std::iter::once(m)
                            .chain(worker.pv[1].iter().copied())
                            .collect();
                    }
                }
                Err(reason) => {
                    result.stop_reason = Some(reason);
                    complete = false;
                    break;
                }
            }
        }
        if !complete {
            break;
        }
        result.best_move = best;
        result.score = alpha;
        result.completed_depth = depth;
        result.pv = best_pv;
        result.nodes = worker.nodes;
        result.qnodes = worker.qnodes;
        result.selective_depth = worker.selective_depth;
        result.elapsed = start.elapsed();
        on_iteration(&result);
        worker.previous_pv = result.pv.clone();
        if let Some(best) = best {
            moves.sort_by_key(|m| *m != best);
        }
        if result.score.abs() >= MATE - MAX_PLY as i32 {
            break;
        }
    }
    result.nodes = worker.nodes;
    result.qnodes = worker.qnodes;
    result.selective_depth = worker.selective_depth;
    result.elapsed = start.elapsed();
    result.budget_exhausted = result.stop_reason == Some(StopReason::Nodes);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(stop: &AtomicBool) -> Search<'_> {
        Search {
            nodes: 0,
            qnodes: 0,
            selective_depth: 0,
            config: Config::default(),
            start: Instant::now(),
            cancelled: stop,
            pv: vec![Vec::new(); MAX_PLY],
            previous_pv: Vec::new(),
            killers: vec![[None; 2]; MAX_PLY],
            history: [[[0; 64]; 64]; 2],
        }
    }

    #[test]
    fn quiescence_searches_quiet_check_evasions() {
        let b = Board::from_fen("4r2k/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        assert!(b.in_check(b.side));
        assert!(b.legal_moves().iter().all(|&m| !is_capture(&b, m)));
        let stop = AtomicBool::new(false);
        let mut worker = worker(&stop);
        worker
            .quiescence(&b, 0, 0, -MATE - 1, MATE + 1, &mut vec![b.key()])
            .unwrap();
        assert!(worker.qnodes > 1);
        let m = worker.pv[0][0];
        assert!(b.legal_moves().contains(&m));
        assert!(!b.after(m).in_check(b.side));
    }

    #[test]
    fn quiescence_limit_never_uses_stand_pat_in_check() {
        let b = Board::from_fen("4r2k/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let stop = AtomicBool::new(false);
        let mut worker = worker(&stop);
        assert_eq!(
            worker.quiescence(
                &b,
                0,
                MAX_QUIESCENCE,
                -MATE - 1,
                MATE + 1,
                &mut vec![b.key()]
            ),
            Err(StopReason::PlyLimit)
        );
        let mate = Board::from_fen("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(
            worker.quiescence(
                &mate,
                0,
                MAX_QUIESCENCE,
                -MATE - 1,
                MATE + 1,
                &mut vec![mate.key()]
            ),
            Ok(-MATE)
        );
    }

    #[test]
    fn quiescence_includes_en_passant_and_quiet_promotions() {
        for (fen, expected) in [
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", "e5d6"),
            ("7k/P7/8/8/8/8/8/7K w - - 0 1", "a7a8q"),
        ] {
            let b = Board::from_fen(fen).unwrap();
            let stop = AtomicBool::new(false);
            let mut worker = worker(&stop);
            worker
                .quiescence(&b, 0, 0, -MATE - 1, MATE + 1, &mut vec![b.key()])
                .unwrap();
            assert_eq!(worker.pv[0][0].to_string(), expected);
        }
    }

    #[test]
    fn quiescence_honors_draw_history_and_shared_budget() {
        let b = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
        let stop = AtomicBool::new(false);
        let mut worker = worker(&stop);
        let key = b.key();
        assert_eq!(
            worker.quiescence(
                &b,
                0,
                0,
                -MATE - 1,
                MATE + 1,
                &mut vec![key.clone(), key.clone(), key]
            ),
            Ok(0)
        );
        worker.config.node_limit = worker.nodes;
        assert_eq!(
            worker.quiescence(&b, 0, 0, -MATE - 1, MATE + 1, &mut vec![b.key()]),
            Err(StopReason::Nodes)
        );
    }
}

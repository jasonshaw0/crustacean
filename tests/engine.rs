use crustacean::board::{Board, Color, Outcome, Piece};
use crustacean::eval::evaluate;
use crustacean::search::{
    planned_depth, search, search_with_control, Config, StopReason, DEFAULT_DEPTH, DEFAULT_NODES,
    MATE, MAX_DEPTH,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const TACTIC: &str = "1nbrkb1r/pp2qppp/2n5/3p4/8/8/PPP1PPPP/RNBQKBNR w KQk - 0 8";
fn fixed(depth: u8) -> Config {
    Config {
        depth: Some(depth),
        ..Config::default()
    }
}
fn mirror(board: &Board) -> Board {
    let mut result = board.clone();
    for (sq, piece) in board.squares.iter().enumerate() {
        result.squares[sq ^ 56] = piece.map(|p| Piece {
            color: p.color.other(),
            kind: p.kind,
        });
    }
    result.side = board.side.other();
    result.castling = ((board.castling & 3) << 2) | ((board.castling & 12) >> 2);
    result.ep = board.ep.map(|sq| sq ^ 56);
    Board::from_fen(&result.to_fen()).unwrap()
}

#[test]
fn default_depth_and_overrides() {
    assert_eq!(DEFAULT_DEPTH, 6);
    assert_eq!(Config::default().node_limit, DEFAULT_NODES);
    assert!(Config::default().node_limit > 100_000);
    assert_eq!(planned_depth(Config::default()), DEFAULT_DEPTH);
    for (requested, expected) in [(0, 1), (1, 1), (7, 7), (32, 32), (u8::MAX, MAX_DEPTH)] {
        assert_eq!(planned_depth(fixed(requested)), expected);
    }
}
#[test]
fn quiescence_avoids_defended_pawn_at_depth_one() {
    for fen in [TACTIC, "3rk3/pp3ppp/8/3p4/8/1N6/PPP2PPP/3QK3 w - - 0 40"] {
        let b = Board::from_fen(fen).unwrap();
        let result = search(&b, fixed(1), &[]);
        assert_eq!(result.completed_depth, 1);
        assert_ne!(result.best_move.unwrap().to_string(), "d1d5");
        assert!(result.qnodes > 0);
        let mut continuation = b.clone();
        continuation.play_uci("d1d5").unwrap();
        continuation.play_uci("d8d5").unwrap();
    }
}
#[test]
fn opening_move_is_sensible_and_deterministic() {
    let b = Board::default();
    let a = search(&b, fixed(3), &[]);
    let again = search(&b, fixed(3), &[]);
    assert_eq!(a.completed_depth, 3);
    assert_eq!(
        (a.best_move, a.score, a.nodes),
        (again.best_move, again.score, again.nodes)
    );
    assert!(["e2e4", "d2d4", "b1c3", "g1f3"].contains(&a.best_move.unwrap().to_string().as_str()));
}
#[test]
fn mate_in_one_is_found_at_the_horizon() {
    let b = Board::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 0 1").unwrap();
    let result = search(&b, fixed(1), &[]);
    assert_eq!(result.score, MATE - 1);
    assert_eq!(
        b.after(result.best_move.unwrap()).outcome(&[]),
        Some(Outcome::Checkmate(Color::White))
    );
}
#[test]
fn terminal_positions_return_no_move_and_correct_score() {
    for (fen, score) in [
        ("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1", -MATE),
        ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", 0),
        ("7k/8/8/8/8/8/8/4K3 w - - 0 1", 0),
    ] {
        let b = Board::from_fen(fen).unwrap();
        let r = search(&b, Config::default(), &[]);
        assert_eq!((r.best_move, r.score, r.nodes), (None, score, 0));
    }
}
#[test]
fn budgets_preserve_last_completed_iteration_or_legal_fallback() {
    let b = Board::default();
    for budget in [0, 1, 20, 21, 100] {
        let r = search(
            &b,
            Config {
                node_limit: budget,
                ..fixed(8)
            },
            &[],
        );
        assert!(r.nodes <= budget);
        assert_eq!(r.stop_reason, Some(StopReason::Nodes));
        assert!(r.budget_exhausted);
        assert!(b.legal_moves().contains(&r.best_move.unwrap()));
        if budget < 20 {
            assert_eq!(r.completed_depth, 0);
        } else {
            assert!(r.completed_depth >= 1);
        }
    }
    let one = search(
        &b,
        Config {
            node_limit: 20,
            ..fixed(1)
        },
        &[],
    );
    let aborted = search(
        &b,
        Config {
            node_limit: 21,
            ..fixed(3)
        },
        &[],
    );
    assert_eq!(
        (one.best_move, one.score, one.pv),
        (aborted.best_move, aborted.score, aborted.pv)
    );
}
#[test]
fn search_honors_repetition_at_root() {
    let b = Board::default();
    let key = b.key();
    let r = search(&b, Config::default(), &[key.clone(), key.clone(), key]);
    assert_eq!(r.outcome, Some(Outcome::Repetition));
    assert_eq!(r.best_move, None);
}
#[test]
fn material_has_positive_value_in_all_phases() {
    assert_eq!(evaluate(&Board::default()), 0);
    for (fen, minimum) in [
        (
            "rnbqkbnr/1ppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            100,
        ),
        ("7k/8/8/8/8/8/Q7/7K w - - 0 40", 800),
        ("7k/8/8/8/8/8/R7/7K w - - 0 40", 400),
        ("7k/8/8/8/8/8/P7/7K w - - 0 40", 90),
    ] {
        let b = Board::from_fen(fen).unwrap();
        assert!(evaluate(&b) >= minimum, "{fen}: {}", evaluate(&b));
        let opposite = Board {
            side: b.side.other(),
            ..b.clone()
        };
        assert_eq!(evaluate(&b), -evaluate(&opposite));
    }
}
#[test]
fn reduced_material_positions_take_free_heavy_pieces() {
    for (fen, minimum) in [
        ("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 40", 400),
        ("4k3/8/8/3r4/8/8/8/3QK3 w - - 0 40", 800),
    ] {
        let board = Board::from_fen(fen).unwrap();
        for depth in [1, 4] {
            let result = search(&board, fixed(depth), &[]);
            assert_eq!(result.best_move.unwrap().to_string(), "d1d5");
            assert!(result.score >= minimum);
            let black = mirror(&board);
            let result = search(&black, fixed(depth), &[]);
            assert_eq!(result.best_move.unwrap().to_string(), "d8d4");
            assert!(result.score >= minimum);
        }
    }
}
#[test]
fn endgame_king_centralization_and_pawn_advancement_are_rewarded() {
    let edge = Board::from_fen("7k/8/8/8/8/8/P7/4K3 w - - 0 40").unwrap();
    let center = Board::from_fen("7k/8/8/8/4K3/8/P7/8 w - - 0 40").unwrap();
    assert!(evaluate(&center) > evaluate(&edge));
    let advanced = Board::from_fen("7k/8/8/P7/8/8/8/4K3 w - - 0 40").unwrap();
    assert!(evaluate(&advanced) > evaluate(&edge));
}
#[test]
fn bounded_engine_self_play_produces_only_legal_moves() {
    let mut b = Board::default();
    let mut history = vec![b.key()];
    let mut played = 0;
    for _ in 0..60 {
        let r = search(
            &b,
            Config {
                node_limit: 3000,
                ..fixed(4)
            },
            &history,
        );
        if r.outcome.is_some() {
            break;
        }
        let m = r.best_move.unwrap();
        assert!(b.legal_moves().contains(&m));
        b = b.after(m);
        history.push(b.key());
        played += 1;
    }
    assert!(played >= 10);
}
#[test]
fn analysis_is_independent_of_fullmove_number() {
    let board = Board::from_fen(TACTIC).unwrap();
    let reference = search(&board, fixed(2), &[]);
    for fullmove in [1, 45, 137, u32::MAX] {
        let result = search(
            &Board {
                fullmove,
                ..board.clone()
            },
            fixed(2),
            &[],
        );
        assert_eq!(result.completed_depth, 2);
        assert_eq!(
            (result.best_move, result.score, result.nodes),
            (reference.best_move, reference.score, reference.nodes)
        );
    }
}
#[test]
fn search_preserves_root_and_history() {
    let mut board = Board::default();
    let mut history = vec![board.key()];
    for m in ["e2e4", "e7e5", "g1f3"] {
        board.play_uci(m).unwrap();
        history.push(board.key());
    }
    let fen = board.to_fen();
    let saved = history.clone();
    let result = search(
        &board,
        Config {
            node_limit: 5000,
            ..fixed(4)
        },
        &history,
    );
    assert!(board.legal_moves().contains(&result.best_move.unwrap()));
    assert_eq!(board.to_fen(), fen);
    assert_eq!(history, saved);
}
#[test]
fn evaluation_is_color_mirror_symmetric() {
    for fen in [
        TACTIC,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "7k/8/3p4/8/4P3/8/8/R3K3 w - - 0 42",
    ] {
        let b = Board::from_fen(fen).unwrap();
        assert_eq!(evaluate(&b), evaluate(&mirror(&b)));
    }
}
#[test]
fn standard_openings_receive_developing_replies() {
    for opening in ["e2e4", "d2d4", "c2c4", "g1f3"] {
        let mut b = Board::default();
        b.play_uci(opening).unwrap();
        let result = search(&b, fixed(3), &[]);
        assert_eq!(result.completed_depth, 3);
        let reply = result.best_move.unwrap();
        assert!(
            ["b8c6", "g8f6", "e7e5", "d7d5", "c7c5", "e7e6", "d7d6", "c7c6", "g7g6"]
                .contains(&reply.to_string().as_str()),
            "{opening}: {reply}"
        );
        b.play_uci(&reply.to_string()).unwrap();
    }
}
#[test]
fn sparse_position_can_search_beyond_six_plies() {
    let b = Board::from_fen("7k/8/8/8/8/8/P7/K7 w - - 0 1").unwrap();
    let result = search(&b, fixed(8), &[]);
    assert_eq!(result.completed_depth, 8);
    assert_eq!(result.stop_reason, None);
}
#[test]
fn zero_time_and_external_cancellation_keep_a_legal_move() {
    let b = Board::default();
    let timed = search(
        &b,
        Config {
            time_limit: Some(Duration::ZERO),
            ..fixed(8)
        },
        &[],
    );
    assert_eq!(timed.stop_reason, Some(StopReason::Time));
    assert_eq!(timed.completed_depth, 0);
    assert!(b.legal_moves().contains(&timed.best_move.unwrap()));
    let stop = AtomicBool::new(true);
    let cancelled = search_with_control(&b, fixed(8), &[], &stop, |_| {
        panic!("iteration should not complete")
    });
    assert_eq!(cancelled.stop_reason, Some(StopReason::Cancelled));
    assert!(b.legal_moves().contains(&cancelled.best_move.unwrap()));
}
#[test]
fn cancellation_preserves_completed_pv_and_score() {
    let b = Board::default();
    let stop = AtomicBool::new(false);
    let mut completed = Vec::new();
    let result = search_with_control(&b, fixed(8), &[], &stop, |r| {
        completed.push(r.clone());
        if r.completed_depth == 2 {
            stop.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(result.stop_reason, Some(StopReason::Cancelled));
    let last = completed.last().unwrap();
    assert_eq!(result.completed_depth, 2);
    assert_eq!(
        (result.best_move, result.score, &result.pv),
        (last.best_move, last.score, &last.pv)
    );
    let mut position = b;
    for m in result.pv {
        assert!(position.legal_moves().contains(&m));
        position = position.after(m);
    }
}
#[test]
fn draw_at_horizon_precedes_static_evaluation() {
    let b = Board::from_fen("7k/8/8/8/8/8/8/R3K3 w - - 99 1").unwrap();
    let result = search(&b, fixed(1), &[]);
    assert_eq!(result.score, 0);
    assert!(result.best_move.is_some());
}

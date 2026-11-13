use crustacean::board::{parse_square, perft, Board, Color, Kind, Outcome, Piece, START_FEN};

fn has(board: &Board, m: &str) -> bool {
    board
        .legal_moves()
        .iter()
        .any(|legal| legal.to_string() == m)
}
fn fen(s: &str) -> Board {
    Board::from_fen(s).unwrap()
}

#[test]
fn start_position_perft_through_four() {
    let b = Board::default();
    for (depth, count) in [(0, 1), (1, 20), (2, 400), (3, 8902), (4, 197281)] {
        assert_eq!(perft(&b, depth), count, "depth {depth}");
    }
}
#[test]
fn kiwipete_castling_and_pins() {
    let b = fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    for (depth, count) in [(1, 48), (2, 2039), (3, 97862)] {
        assert_eq!(perft(&b, depth), count, "depth {depth}");
    }
}
#[test]
fn endgame_perft_en_passant_and_rook_checks() {
    let b = fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1");
    for (depth, count) in [(1, 14), (2, 191), (3, 2812), (4, 43238)] {
        assert_eq!(perft(&b, depth), count, "depth {depth}");
    }
}
#[test]
fn promotions_and_evasions_perft() {
    let b = fen("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1");
    for (depth, count) in [(1, 6), (2, 264), (3, 9467)] {
        assert_eq!(perft(&b, depth), count, "depth {depth}");
    }
}
#[test]
fn promotion_and_castling_perft() {
    let b = fen("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8");
    for (depth, count) in [(1, 44), (2, 1486), (3, 62379)] {
        assert_eq!(perft(&b, depth), count, "depth {depth}");
    }
}
#[test]
fn fen_round_trip_and_clocks() {
    let mut b = Board::default();
    assert_eq!(b.to_fen(), START_FEN);
    b.play_uci("e2e4").unwrap();
    assert_eq!(
        b.to_fen(),
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
    );
    assert_eq!(fen(&b.to_fen()).to_fen(), b.to_fen());
    b.play_uci("g8f6").unwrap();
    assert_eq!((b.halfmove, b.fullmove, b.ep), (1, 2, None));
}
#[test]
fn malformed_fens_return_errors() {
    for input in [
        "",
        "8/8/8/8/8/8/8/8 w - - 0 1",
        "8/8/8/8/8/8/4k3/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 x - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w K - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - e6 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 0",
        "4k3/8/8/8/8/8/8/4K2P w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - - -1 1",
        "4k3/8/8/8/8/8/8/4K4 w - - 0 1",
    ] {
        assert!(Board::from_fen(input).is_err(), "accepted {input}");
    }
}
#[test]
fn malformed_coordinate_moves_do_not_mutate() {
    let mut b = Board::default();
    for input in ["e2e5", "e2e4q", "e9e4", "é2e4", "", "e2", "e2e4garbage"] {
        assert!(b.play_uci(input).is_err());
        assert_eq!(b.to_fen(), START_FEN);
    }
    assert!(parse_square("é1").is_err());
}
#[test]
fn castling_moves_rook_and_clears_rights() {
    for (m, king, rook) in [("e1g1", 6, 5), ("e1c1", 2, 3)] {
        let mut b = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        b.play_uci(m).unwrap();
        assert_eq!(
            b.squares[king],
            Some(Piece {
                color: Color::White,
                kind: Kind::King
            })
        );
        assert_eq!(
            b.squares[rook],
            Some(Piece {
                color: Color::White,
                kind: Kind::Rook
            })
        );
        assert_eq!(b.castling, 12);
    }
    let mut b = fen("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1");
    b.play_uci("e8c8").unwrap();
    assert_eq!(b.squares[59].unwrap().kind, Kind::Rook);
    assert_eq!(b.castling, 3);
}
#[test]
fn cannot_castle_through_check_or_out_of_check() {
    let b = fen("k4r2/8/8/8/8/8/8/4K2R w K - 0 1");
    assert!(!has(&b, "e1g1"));
    let b = fen("k3r3/8/8/8/8/8/8/4K2R w K - 0 1");
    assert!(!has(&b, "e1g1"));
    let b = fen("k5r1/8/8/8/8/8/8/4K2R w K - 0 1");
    assert!(!has(&b, "e1g1"));
}
#[test]
fn rook_move_and_capture_revoke_rights_permanently() {
    let mut b = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    b.play_uci("a1a8").unwrap();
    assert_eq!(b.castling, 5);
    let mut b = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    for m in ["h1h2", "h8h7", "h2h1", "h7h8"] {
        b.play_uci(m).unwrap();
    }
    assert_eq!(b.castling, 10);
}
#[test]
fn en_passant_removes_correct_pawn_and_expires() {
    let mut b = fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
    b.play_uci("e5d6").unwrap();
    assert!(b.squares[35].is_none());
    assert_eq!(b.squares[43].unwrap().kind, Kind::Pawn);
    assert_eq!(b.ep, None);
    let mut b = fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
    b.play_uci("e1f1").unwrap();
    assert_eq!(b.ep, None);
}
#[test]
fn en_passant_cannot_expose_king_to_rook() {
    let b = fen("k7/8/8/4KPpr/8/8/8/8 w - g6 0 1");
    assert!(!has(&b, "f5g6"));
    let no_ep = fen("k7/8/8/4KPpr/8/8/8/8 w - - 0 1");
    assert_eq!(b.key(), no_ep.key());
}
#[test]
fn all_four_promotions_and_underpromotion_application() {
    let mut b = fen("7k/P7/8/8/8/8/8/7K w - - 0 1");
    for suffix in ['q', 'r', 'b', 'n'] {
        assert!(has(&b, &format!("a7a8{suffix}")));
    }
    assert!(!has(&b, "a7a8"));
    b.play_uci("a7a8n").unwrap();
    assert_eq!(b.squares[56].unwrap().kind, Kind::Knight);
}
#[test]
fn pinned_piece_cannot_expose_king() {
    let b = fen("k3r3/8/8/8/8/8/4R3/4K3 w - - 0 1");
    assert!(!has(&b, "e2d2"));
    assert!(has(&b, "e2e8"));
}
#[test]
fn kings_cannot_touch() {
    let b = fen("8/8/8/8/8/4k3/8/4K3 w - - 0 1");
    assert!(!has(&b, "e1e2"));
}
#[test]
fn checkmate_and_stalemate_are_distinct() {
    let mate = fen("7k/6Q1/6K1/8/8/8/8/8 b - - 100 1");
    assert_eq!(mate.outcome(&[]), Some(Outcome::Checkmate(Color::White)));
    let stale = fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert_eq!(stale.outcome(&[]), Some(Outcome::Stalemate));
}
#[test]
fn fifty_move_and_conservative_material_draws() {
    let b = fen("7k/8/8/8/8/8/8/R3K3 w - - 100 1");
    assert_eq!(b.outcome(&[]), Some(Outcome::FiftyMove));
    for s in [
        "7k/8/8/8/8/8/8/4K3 w - - 0 1",
        "7k/8/8/8/8/8/8/2B1K3 w - - 0 1",
        "7k/8/8/8/8/8/8/2N1K3 w - - 0 1",
        "5b1k/8/8/8/8/8/8/2B1K3 w - - 0 1",
    ] {
        assert!(fen(s).insufficient_material());
    }
    assert!(!fen("7k/8/8/8/8/8/8/2NNK3 w - - 0 1").insufficient_material());
    assert!(!fen("6bk/8/8/8/8/8/8/2B1K3 w - - 0 1").insufficient_material());
}
#[test]
fn actual_repetition_history_triggers_draw() {
    let mut b = Board::default();
    let mut history = vec![b.key()];
    for _ in 0..2 {
        for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            b.play_uci(m).unwrap();
            history.push(b.key());
        }
    }
    assert_eq!(b.outcome(&history), Some(Outcome::Repetition));
    assert_eq!(b.outcome(&history[1..]), None);
}
#[test]
fn repetition_key_ignores_clocks_and_unusable_ep_but_not_legal_ep() {
    let a = fen("4k3/8/8/3p4/8/8/8/4K3 w - d6 0 2");
    let b = fen("4k3/8/8/3p4/8/8/8/4K3 w - - 7 9");
    assert_eq!(a.key(), b.key());
    let a = fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
    let b = fen("4k3/8/8/3pP3/8/8/8/4K3 w - - 0 2");
    assert_ne!(a.key(), b.key());
}
#[test]
fn deterministic_legal_playouts_preserve_fen_and_kings() {
    for seed in 0..8usize {
        let mut b = Board::default();
        let mut state = seed + 1;
        for _ in 0..120 {
            let moves = b.legal_moves();
            if moves.is_empty() {
                break;
            }
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let side = b.side;
            b = b.after(moves[state % moves.len()]);
            assert!(!b.in_check(side));
            assert_eq!(fen(&b.to_fen()).to_fen(), b.to_fen());
        }
    }
}

#[test]
fn middlegame_perft_reference_six() {
    let b = fen("r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10");
    for (depth, count) in [(1, 46), (2, 2079), (3, 89890)] {
        assert_eq!(perft(&b, depth), count, "depth {depth}");
    }
}

#[test]
fn black_en_passant_and_underpromotion() {
    let mut b = fen("4k3/8/8/8/3Pp3/8/8/4K3 b - d3 0 2");
    b.play_uci("e4d3").unwrap();
    assert!(b.squares[27].is_none());
    assert_eq!(
        b.squares[19],
        Some(Piece {
            color: Color::Black,
            kind: Kind::Pawn
        })
    );
    let mut b = fen("7k/8/8/8/8/8/p7/7K b - - 0 1");
    for suffix in ['q', 'r', 'b', 'n'] {
        assert!(has(&b, &format!("a2a1{suffix}")));
    }
    b.play_uci("a2a1b").unwrap();
    assert_eq!(b.squares[0].unwrap().kind, Kind::Bishop);
}

#[test]
fn double_check_allows_only_king_moves() {
    let b = fen("k3r3/8/8/8/1b6/8/3R4/4K3 w - - 0 1");
    assert!(b.in_check(Color::White));
    assert!(!b.legal_moves().is_empty());
    for m in b.legal_moves() {
        assert_eq!(b.squares[m.from].unwrap().kind, Kind::King);
    }
}

#[test]
fn captures_and_pawns_reset_fifty_move_clock() {
    let mut b = fen("4k3/8/8/8/8/8/p6P/R3K3 w - - 99 1");
    b.play_uci("a1a2").unwrap();
    assert_eq!(b.halfmove, 0);
    let mut b = fen("4k3/8/8/8/8/8/7P/R3K3 w - - 99 1");
    b.play_uci("h2h3").unwrap();
    assert_eq!(b.halfmove, 0);
}

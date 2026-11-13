//! Material and positional evaluation.
use crate::board::{Board, Color, Kind};

const PHASE_TOTAL: i32 = 24;

#[derive(Default)]
struct Score {
    middlegame: i32,
    endgame: i32,
}

impl Score {
    fn interpolate(&self, phase: i32) -> i32 {
        let phase = phase.clamp(0, PHASE_TOTAL);
        (self.middlegame * phase + self.endgame * (PHASE_TOTAL - phase)) / PHASE_TOTAL
    }
}

fn phase_weight(kind: Kind) -> i32 {
    match kind {
        Kind::Knight | Kind::Bishop => 1,
        Kind::Rook => 2,
        Kind::Queen => 4,
        Kind::Pawn | Kind::King => 0,
    }
}

/// Centipawns from the side-to-move perspective. Not a win probability.
pub fn evaluate(board: &Board) -> i32 {
    let mut score = Score::default();
    let mut phase = 0;
    for (sq, piece) in board.squares.iter().enumerate() {
        let Some(p) = piece else {
            continue;
        };
        let file = (sq % 8) as i32;
        let rank = if p.color == Color::White {
            sq / 8
        } else {
            7 - sq / 8
        } as i32;
        let center = 14 - (2 * file - 7).abs() - (2 * rank - 7).abs();
        let middlegame = match p.kind {
            Kind::Pawn => {
                rank * 4
                    + if (3..=4).contains(&file) && (3..=4).contains(&rank) {
                        24
                    } else {
                        0
                    }
            }
            Kind::Knight => center * 6,
            Kind::Bishop => center * 3 + rank * 2,
            Kind::Rook => rank,
            Kind::Queen => center - rank * 4,
            Kind::King => {
                -center * 3 - rank * 8
                    + if rank == 0 && (file == 2 || file == 6) {
                        30
                    } else {
                        0
                    }
            }
        };
        let endgame = match p.kind {
            Kind::Pawn => rank * rank * 4,
            Kind::Knight => center * 4,
            Kind::Bishop => center * 3,
            Kind::Rook => rank * 4,
            Kind::Queen => center * 2,
            Kind::King => center * 6,
        };
        let sign = if p.color == Color::White { 1 } else { -1 };
        score.middlegame += (p.kind.value() + middlegame) * sign;
        score.endgame += (p.kind.value() + endgame) * sign;
        phase += phase_weight(p.kind);
    }
    score.interpolate(phase) * if board.side == Color::White { 1 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_has_correct_endpoints_and_midpoint() {
        let score = Score {
            middlegame: 240,
            endgame: 480,
        };
        assert_eq!(score.interpolate(PHASE_TOTAL), 240);
        assert_eq!(score.interpolate(0), 480);
        assert_eq!(score.interpolate(PHASE_TOTAL / 2), 360);
        assert_eq!(score.interpolate(100), 240);
        assert_eq!(score.interpolate(-10), 480);
    }

    #[test]
    fn equal_phase_scores_stay_constant() {
        for value in [-900, -100, 0, 100, 900] {
            let score = Score {
                middlegame: value,
                endgame: value,
            };
            for phase in 0..=PHASE_TOTAL {
                assert_eq!(score.interpolate(phase), value);
            }
        }
    }
}

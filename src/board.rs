//! Readable mailbox board. a1 = 0, h1 = 7, a8 = 56.
//! Move generation applies each candidate to a copy and checks king safety.
use std::fmt;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const KNIGHT: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING: [(i8, i8); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];
const BISHOP: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const ROOK: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}
impl Color {
    pub fn other(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
    fn forward(self) -> i8 {
        if self == Self::White {
            1
        } else {
            -1
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}
impl Kind {
    pub fn value(self) -> i32 {
        match self {
            Self::Pawn => 100,
            Self::Knight => 320,
            Self::Bishop => 330,
            Self::Rook => 500,
            Self::Queen => 900,
            Self::King => 0,
        }
    }
    fn letter(self) -> char {
        match self {
            Self::Pawn => 'p',
            Self::Knight => 'n',
            Self::Bishop => 'b',
            Self::Rook => 'r',
            Self::Queen => 'q',
            Self::King => 'k',
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: Kind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: usize,
    pub to: usize,
    pub promotion: Option<Kind>,
}
impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", square_name(self.from), square_name(self.to))?;
        if let Some(k) = self.promotion {
            write!(f, "{}", k.letter())?;
        }
        Ok(())
    }
}
pub fn square_name(sq: usize) -> String {
    format!(
        "{}{}",
        (b'a' + (sq % 8) as u8) as char,
        (b'1' + (sq / 8) as u8) as char
    )
}
pub fn parse_square(s: &str) -> Result<usize, String> {
    let b = s.as_bytes();
    if b.len() != 2 || !(b'a'..=b'h').contains(&b[0]) || !(b'1'..=b'8').contains(&b[1]) {
        return Err(format!("invalid square: {s}"));
    }
    Ok((b[0] - b'a') as usize + 8 * (b[1] - b'1') as usize)
}
fn step(sq: usize, dx: i8, dy: i8) -> Option<usize> {
    let x = (sq % 8) as i8 + dx;
    let y = (sq / 8) as i8 + dy;
    if (0..8).contains(&x) && (0..8).contains(&y) {
        Some(y as usize * 8 + x as usize)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Board {
    pub squares: [Option<Piece>; 64],
    pub side: Color,
    /// Bits: White king/queen side, Black king/queen side.
    pub castling: u8,
    pub ep: Option<usize>,
    pub halfmove: u32,
    pub fullmove: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PositionKey {
    squares: [Option<Piece>; 64],
    side: Color,
    castling: u8,
    ep: Option<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Checkmate(Color),
    Stalemate,
    FiftyMove,
    Repetition,
    InsufficientMaterial,
}
impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Checkmate(winner) => write!(f, "checkmate: {winner:?} wins"),
            Self::Stalemate => write!(f, "draw: stalemate"),
            Self::FiftyMove => write!(f, "draw: fifty-move claim adjudicated automatically"),
            Self::Repetition => write!(f, "draw: threefold claim adjudicated automatically"),
            Self::InsufficientMaterial => write!(f, "draw: insufficient material"),
        }
    }
}
impl Default for Board {
    fn default() -> Self {
        Self::from_fen(START_FEN).expect("built-in start FEN")
    }
}
impl Board {
    /// Reject malformed and locally impossible positions; not a retrograde legality proof.
    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let fields: Vec<_> = fen.split_whitespace().collect();
        if fields.len() != 6 {
            return Err("FEN needs exactly six fields".into());
        }
        let mut b = Self {
            squares: [None; 64],
            side: Color::White,
            castling: 0,
            ep: None,
            halfmove: 0,
            fullmove: 1,
        };
        let ranks: Vec<_> = fields[0].split('/').collect();
        if ranks.len() != 8 {
            return Err("FEN needs eight ranks".into());
        }
        for (row, rank) in ranks.iter().enumerate() {
            let mut file = 0;
            for c in rank.chars() {
                if ('1'..='8').contains(&c) {
                    file += c.to_digit(10).unwrap() as usize;
                } else {
                    let kind = match c.to_ascii_lowercase() {
                        'p' => Kind::Pawn,
                        'n' => Kind::Knight,
                        'b' => Kind::Bishop,
                        'r' => Kind::Rook,
                        'q' => Kind::Queen,
                        'k' => Kind::King,
                        _ => return Err("invalid FEN piece".into()),
                    };
                    if file >= 8 {
                        return Err("FEN rank overflow".into());
                    }
                    if kind == Kind::Pawn && (row == 0 || row == 7) {
                        return Err("pawn on promotion rank".into());
                    }
                    b.squares[(7 - row) * 8 + file] = Some(Piece {
                        color: if c.is_ascii_uppercase() {
                            Color::White
                        } else {
                            Color::Black
                        },
                        kind,
                    });
                    file += 1;
                }
                if file > 8 {
                    return Err("FEN rank overflow".into());
                }
            }
            if file != 8 {
                return Err("FEN rank must contain eight squares".into());
            }
        }
        b.side = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            _ => return Err("invalid FEN side".into()),
        };
        if fields[2] != "-" {
            for c in fields[2].chars() {
                let bit = match c {
                    'K' => 1,
                    'Q' => 2,
                    'k' => 4,
                    'q' => 8,
                    _ => return Err("invalid castling rights".into()),
                };
                if b.castling & bit != 0 {
                    return Err("duplicate castling right".into());
                }
                b.castling |= bit;
            }
        }
        for (bit, king, rook, color) in [
            (1, 4, 7, Color::White),
            (2, 4, 0, Color::White),
            (4, 60, 63, Color::Black),
            (8, 60, 56, Color::Black),
        ] {
            if b.castling & bit != 0
                && (b.squares[king]
                    != Some(Piece {
                        color,
                        kind: Kind::King,
                    })
                    || b.squares[rook]
                        != Some(Piece {
                            color,
                            kind: Kind::Rook,
                        }))
            {
                return Err("castling right without home king and rook".into());
            }
        }
        b.halfmove = fields[4].parse().map_err(|_| "invalid halfmove clock")?;
        b.fullmove = fields[5].parse().map_err(|_| "invalid fullmove number")?;
        if b.fullmove == 0 {
            return Err("fullmove number starts at one".into());
        }
        if fields[3] != "-" {
            let sq = parse_square(fields[3])?;
            let expected_rank = if b.side == Color::White { 5 } else { 2 };
            let pawn = step(sq, 0, -b.side.forward()).ok_or("invalid en passant square")?;
            let origin = step(sq, 0, b.side.forward()).ok_or("invalid en passant square")?;
            if sq / 8 != expected_rank
                || b.squares[sq].is_some()
                || b.squares[origin].is_some()
                || b.squares[pawn]
                    != Some(Piece {
                        color: b.side.other(),
                        kind: Kind::Pawn,
                    })
                || b.halfmove != 0
            {
                return Err("inconsistent en passant target".into());
            }
            b.ep = Some(sq);
        }
        for color in [Color::White, Color::Black] {
            if b.squares
                .iter()
                .filter(|p| {
                    **p == Some(Piece {
                        color,
                        kind: Kind::King,
                    })
                })
                .count()
                != 1
            {
                return Err("exactly one king of each color required".into());
            }
        }
        if b.in_check(b.side.other()) {
            return Err("side that just moved left its king in check".into());
        }
        Ok(b)
    }
    pub fn to_fen(&self) -> String {
        let mut placement = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                if let Some(p) = self.squares[rank * 8 + file] {
                    if empty > 0 {
                        placement.push(char::from_digit(empty, 10).unwrap());
                        empty = 0;
                    }
                    let c = p.kind.letter();
                    placement.push(if p.color == Color::White {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    });
                } else {
                    empty += 1;
                }
            }
            if empty > 0 {
                placement.push(char::from_digit(empty, 10).unwrap());
            }
            if rank > 0 {
                placement.push('/');
            }
        }
        let rights: String = [(1, 'K'), (2, 'Q'), (4, 'k'), (8, 'q')]
            .iter()
            .filter(|(bit, _)| self.castling & bit != 0)
            .map(|(_, c)| c)
            .collect();
        format!(
            "{placement} {} {} {} {} {}",
            if self.side == Color::White { "w" } else { "b" },
            if rights.is_empty() { "-" } else { &rights },
            self.ep.map(square_name).unwrap_or_else(|| "-".into()),
            self.halfmove,
            self.fullmove
        )
    }
    pub fn attacked(&self, sq: usize, by: Color) -> bool {
        for dx in [-1, 1] {
            if step(sq, dx, -by.forward()).is_some_and(|s| {
                self.squares[s]
                    == Some(Piece {
                        color: by,
                        kind: Kind::Pawn,
                    })
            }) {
                return true;
            }
        }
        for (offsets, kind) in [(&KNIGHT[..], Kind::Knight), (&KING[..], Kind::King)] {
            for &(dx, dy) in offsets {
                if step(sq, dx, dy)
                    .is_some_and(|s| self.squares[s] == Some(Piece { color: by, kind }))
                {
                    return true;
                }
            }
        }
        for (offsets, kind) in [(&BISHOP[..], Kind::Bishop), (&ROOK[..], Kind::Rook)] {
            for &(dx, dy) in offsets {
                let mut next = step(sq, dx, dy);
                while let Some(s) = next {
                    if let Some(p) = self.squares[s] {
                        if p.color == by && (p.kind == kind || p.kind == Kind::Queen) {
                            return true;
                        }
                        break;
                    }
                    next = step(s, dx, dy);
                }
            }
        }
        false
    }
    pub fn in_check(&self, color: Color) -> bool {
        let king = self
            .squares
            .iter()
            .position(|p| {
                *p == Some(Piece {
                    color,
                    kind: Kind::King,
                })
            })
            .expect("valid board has a king");
        self.attacked(king, color.other())
    }
    fn pawn_move(moves: &mut Vec<Move>, from: usize, to: usize) {
        if to / 8 == 0 || to / 8 == 7 {
            for kind in [Kind::Queen, Kind::Rook, Kind::Bishop, Kind::Knight] {
                moves.push(Move {
                    from,
                    to,
                    promotion: Some(kind),
                });
            }
        } else {
            moves.push(Move {
                from,
                to,
                promotion: None,
            });
        }
    }
    fn pseudo_moves(&self) -> Vec<Move> {
        let mut moves = Vec::with_capacity(64);
        for from in 0..64 {
            let Some(p) = self.squares[from] else {
                continue;
            };
            if p.color != self.side {
                continue;
            }
            if p.kind == Kind::Pawn {
                let dy = p.color.forward();
                if let Some(to) = step(from, 0, dy) {
                    if self.squares[to].is_none() {
                        Self::pawn_move(&mut moves, from, to);
                        let home = if p.color == Color::White { 1 } else { 6 };
                        if from / 8 == home {
                            if let Some(two) = step(to, 0, dy) {
                                if self.squares[two].is_none() {
                                    Self::pawn_move(&mut moves, from, two);
                                }
                            }
                        }
                    }
                }
                for dx in [-1, 1] {
                    if let Some(to) = step(from, dx, dy) {
                        if self.squares[to]
                            .is_some_and(|q| q.color != p.color && q.kind != Kind::King)
                            || self.ep == Some(to)
                        {
                            Self::pawn_move(&mut moves, from, to);
                        }
                    }
                }
                continue;
            }
            let offsets: &[(i8, i8)] = match p.kind {
                Kind::Knight => &KNIGHT,
                Kind::Bishop => &BISHOP,
                Kind::Rook => &ROOK,
                _ => &KING,
            };
            for &(dx, dy) in offsets {
                let mut next = step(from, dx, dy);
                while let Some(to) = next {
                    let target = self.squares[to];
                    if target.is_some_and(|q| q.color == p.color || q.kind == Kind::King) {
                        break;
                    }
                    moves.push(Move {
                        from,
                        to,
                        promotion: None,
                    });
                    if target.is_some() || p.kind == Kind::King || p.kind == Kind::Knight {
                        break;
                    }
                    next = step(to, dx, dy);
                }
            }
            if p.kind == Kind::King {
                let (home, shift) = if p.color == Color::White {
                    (4, 0)
                } else {
                    (60, 2)
                };
                if from == home && !self.in_check(p.color) {
                    for (bit, to, rook, empty, transit) in [
                        (
                            1 << shift,
                            home + 2,
                            home + 3,
                            vec![home + 1, home + 2],
                            home + 1,
                        ),
                        (
                            2 << shift,
                            home - 2,
                            home - 4,
                            vec![home - 1, home - 2, home - 3],
                            home - 1,
                        ),
                    ] {
                        if self.castling & bit != 0
                            && self.squares[rook]
                                == Some(Piece {
                                    color: p.color,
                                    kind: Kind::Rook,
                                })
                            && empty.iter().all(|&s| self.squares[s].is_none())
                        {
                            // Test transit with the home king removed so it cannot shield a ray.
                            let mut passing = self.clone();
                            passing.squares[from] = None;
                            passing.squares[transit] = Some(p);
                            if !passing.in_check(p.color) {
                                moves.push(Move {
                                    from,
                                    to,
                                    promotion: None,
                                });
                            }
                        }
                    }
                }
            }
        }
        moves
    }
    pub fn legal_moves(&self) -> Vec<Move> {
        self.pseudo_moves()
            .into_iter()
            .filter(|&m| !self.after(m).in_check(self.side))
            .collect()
    }
    /// Internal fast path. Caller must supply a move from legal_moves().
    /// Public input should use play_uci(), which validates before applying.
    pub fn after(&self, m: Move) -> Self {
        let mut next = self.clone();
        let p = self.squares[m.from].expect("move source must contain a piece");
        let capture =
            self.squares[m.to].is_some() || (p.kind == Kind::Pawn && self.ep == Some(m.to));
        next.squares[m.from] = None;
        if p.kind == Kind::Pawn && self.ep == Some(m.to) && self.squares[m.to].is_none() {
            next.squares[step(m.to, 0, -p.color.forward()).unwrap()] = None;
        }
        next.squares[m.to] = Some(Piece {
            color: p.color,
            kind: m.promotion.unwrap_or(p.kind),
        });
        if p.kind == Kind::King {
            next.castling &= if p.color == Color::White { !3 } else { !12 };
            if m.from.abs_diff(m.to) == 2 {
                let (rook_from, rook_to) = if m.to > m.from {
                    (m.from + 3, m.from + 1)
                } else {
                    (m.from - 4, m.from - 1)
                };
                next.squares[rook_to] = next.squares[rook_from];
                next.squares[rook_from] = None;
            }
        }
        for (sq, bit) in [(7, 1), (0, 2), (63, 4), (56, 8)] {
            if m.from == sq || m.to == sq {
                next.castling &= !bit;
            }
        }
        next.ep = if p.kind == Kind::Pawn && m.from.abs_diff(m.to) == 16 {
            Some((m.from + m.to) / 2)
        } else {
            None
        };
        next.halfmove = if p.kind == Kind::Pawn || capture {
            0
        } else {
            self.halfmove.saturating_add(1)
        };
        next.fullmove = self
            .fullmove
            .saturating_add(u32::from(self.side == Color::Black));
        next.side = self.side.other();
        next
    }
    pub fn play_uci(&mut self, input: &str) -> Result<Move, String> {
        let m = self
            .legal_moves()
            .into_iter()
            .find(|m| m.to_string() == input)
            .ok_or_else(|| {
                format!("illegal move: {input}; use coordinates such as e2e4 or a7a8q")
            })?;
        *self = self.after(m);
        Ok(m)
    }
    /// FIDE repetition distinguishes EP only if an actual legal EP capture exists.
    pub fn key(&self) -> PositionKey {
        let ep = self.ep.filter(|&sq| {
            self.legal_moves().iter().any(|m| {
                m.to == sq
                    && m.from % 8 != sq % 8
                    && self.squares[m.from].is_some_and(|p| p.kind == Kind::Pawn)
            })
        });
        PositionKey {
            squares: self.squares,
            side: self.side,
            castling: self.castling,
            ep,
        }
    }
    pub fn insufficient_material(&self) -> bool {
        let pieces: Vec<_> = self
            .squares
            .iter()
            .enumerate()
            .filter_map(|(sq, p)| p.filter(|p| p.kind != Kind::King).map(|p| (sq, p)))
            .collect();
        if pieces.is_empty() {
            return true;
        }
        if pieces.len() == 1 {
            return matches!(pieces[0].1.kind, Kind::Bishop | Kind::Knight);
        }
        pieces.iter().all(|(_, p)| p.kind == Kind::Bishop)
            && pieces
                .iter()
                .all(|(sq, _)| (sq / 8 + sq % 8) % 2 == (pieces[0].0 / 8 + pieces[0].0 % 8) % 2)
    }
    /// History includes the current position exactly once. Mate takes precedence.
    pub fn outcome_with_moves(&self, history: &[PositionKey], moves: &[Move]) -> Option<Outcome> {
        if moves.is_empty() {
            return Some(if self.in_check(self.side) {
                Outcome::Checkmate(self.side.other())
            } else {
                Outcome::Stalemate
            });
        }
        if self.halfmove >= 100 {
            return Some(Outcome::FiftyMove);
        }
        if self.insufficient_material() {
            return Some(Outcome::InsufficientMaterial);
        }
        let key = self.key();
        if history.iter().filter(|&k| *k == key).count() >= 3 {
            return Some(Outcome::Repetition);
        }
        None
    }
    pub fn outcome(&self, history: &[PositionKey]) -> Option<Outcome> {
        self.outcome_with_moves(history, &self.legal_moves())
    }
}
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in (0..8).rev() {
            write!(f, "{} ", rank + 1)?;
            for file in 0..8 {
                let c = self.squares[rank * 8 + file]
                    .map(|p| {
                        if p.color == Color::White {
                            p.kind.letter().to_ascii_uppercase()
                        } else {
                            p.kind.letter()
                        }
                    })
                    .unwrap_or('.');
                write!(f, "{c} ")?;
            }
            writeln!(f)?;
        }
        writeln!(f, "  a b c d e f g h\n{:?} to move", self.side)
    }
}
/// Count legal move-tree leaves. Draw claims do not stop perft.
pub fn perft(board: &Board, depth: u8) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = board.legal_moves();
    if depth == 1 {
        return moves.len() as u64;
    }
    moves
        .into_iter()
        .map(|m| perft(&board.after(m), depth - 1))
        .sum()
}

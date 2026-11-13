use crate::board::{square_name, Board, Color, Kind, Move, Outcome};
use crate::search::SearchResult;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Player,
    Engines,
}

pub(super) struct Record {
    pub uci: String,
    pub san: String,
    pub number: u32,
    pub side: Color,
}

pub(super) struct Game {
    pub session: String,
    pub board: Board,
    pub positions: Vec<Board>,
    pub records: Vec<Record>,
    pub mode: Mode,
    pub human: Color,
    pub white_ms: u64,
    pub black_ms: u64,
    pub running: bool,
    pub step: bool,
    pub thinking: bool,
    pub version: u64,
    pub cancel: Arc<AtomicBool>,
    pub analysis: Option<(SearchResult, Color)>,
    pub outcome: Option<Outcome>,
}

impl Default for Game {
    fn default() -> Self {
        let board = Board::default();
        Self {
            #[cfg(target_arch = "wasm32")]
            session: "browser".into(),
            #[cfg(not(target_arch = "wasm32"))]
            session: format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            positions: vec![board.clone()],
            board,
            records: Vec::new(),
            mode: Mode::Player,
            human: Color::White,
            white_ms: 500,
            black_ms: 500,
            running: true,
            step: false,
            thinking: false,
            version: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            analysis: None,
            outcome: None,
        }
    }
}

impl Game {
    pub fn history(&self) -> Vec<crate::board::PositionKey> {
        self.positions.iter().map(Board::key).collect()
    }
    pub fn needs_engine(&self) -> bool {
        self.outcome.is_none()
            && (self.step
                || (self.running && (self.mode == Mode::Engines || self.board.side != self.human)))
    }
    pub fn invalidate(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.version = self.version.wrapping_add(1);
        self.thinking = false;
        self.step = false;
    }
    fn reset(&mut self, board: Board) {
        self.invalidate();
        self.positions = vec![board.clone()];
        self.board = board;
        self.records.clear();
        self.analysis = None;
        self.outcome = self.board.outcome(&self.history());
        self.running = self.mode == Mode::Player && self.outcome.is_none();
    }
    pub fn play(&mut self, m: Move) {
        let san = san(&self.board, m);
        self.records.push(Record {
            uci: m.to_string(),
            san,
            number: self.board.fullmove,
            side: self.board.side,
        });
        self.board = self.board.after(m);
        self.positions.push(self.board.clone());
        self.version = self.version.wrapping_add(1);
        self.outcome = self.board.outcome(&self.history());
        if self.outcome.is_some() {
            self.running = false;
            self.step = false;
        }
    }
    /// Errors are transactional; only move requests require an exact board version.
    pub fn action(&mut self, fields: &BTreeMap<String, String>) -> Result<(), (u16, String)> {
        let field = |key: &str| {
            fields
                .get(key)
                .map(String::as_str)
                .ok_or_else(|| (400, format!("Missing {key}")))
        };
        match field("action")? {
            "new" => {
                let mode = match field("mode")? {
                    "player" => Mode::Player,
                    "engines" => Mode::Engines,
                    _ => return Err((400, "Invalid game mode".into())),
                };
                let human = match field("human")? {
                    "white" => Color::White,
                    "black" => Color::Black,
                    _ => return Err((400, "Invalid player color".into())),
                };
                let time = |key| -> Result<u64, (u16, String)> {
                    let value = field(key)?
                        .parse::<u64>()
                        .map_err(|_| (400, "Invalid thinking time".into()))?;
                    if !(100..=10_000).contains(&value) {
                        return Err((400, "Thinking time must be 100–10000 ms".into()));
                    }
                    Ok(value)
                };
                let white_ms = time("white_ms")?;
                let black_ms = time("black_ms")?;
                self.mode = mode;
                self.human = human;
                self.white_ms = white_ms;
                self.black_ms = black_ms;
                self.reset(Board::default());
            }
            "load" => {
                let board = Board::from_fen(field("fen")?).map_err(|e| (400, e))?;
                self.reset(board);
            }
            "move" => {
                let version = field("version")?
                    .parse::<u64>()
                    .map_err(|_| (400, "Invalid position version".into()))?;
                if version != self.version {
                    return Err((409, "The position changed. Select your move again.".into()));
                }
                if self.mode != Mode::Player
                    || self.board.side != self.human
                    || !self.running
                    || self.outcome.is_some()
                {
                    return Err((409, "It is not your turn.".into()));
                }
                let input = field("move")?;
                let m = self
                    .board
                    .legal_moves()
                    .into_iter()
                    .find(|m| m.to_string() == input)
                    .ok_or_else(|| (400, "That move is not legal.".into()))?;
                self.analysis = None;
                self.play(m);
            }
            "pause" => {
                self.invalidate();
                self.running = false;
            }
            "resume" => {
                if self.outcome.is_some() {
                    return Err((409, "This game has ended. Start a new game.".into()));
                }
                self.invalidate();
                self.running = true;
            }
            "step" => {
                if self.mode != Mode::Engines
                    || self.running
                    || self.thinking
                    || self.step
                    || self.outcome.is_some()
                {
                    return Err((409, "Pause the engine match before stepping.".into()));
                }
                self.invalidate();
                self.step = true;
            }
            "undo" => {
                if self.records.is_empty() {
                    return Err((409, "There are no moves to undo.".into()));
                }
                let count = if self.mode == Mode::Player && self.board.side == self.human {
                    2.min(self.records.len())
                } else {
                    1
                };
                self.invalidate();
                self.running = false;
                for _ in 0..count {
                    self.records.pop();
                    self.positions.pop();
                }
                self.board = self.positions.last().unwrap().clone();
                self.analysis = None;
                self.outcome = self.board.outcome(&self.history());
            }
            _ => return Err((400, "Unknown action".into())),
        }
        Ok(())
    }

    pub fn json(&self) -> String {
        let legal = self.board.legal_moves();
        let fen = self.board.to_fen();
        let placement = fen.split_whitespace().next().unwrap();
        let status = match self.outcome {
            Some(Outcome::Checkmate(c)) => format!("{} wins by checkmate", color_title(c)),
            Some(Outcome::Stalemate) => "Draw by stalemate".into(),
            Some(Outcome::Repetition) => "Draw by threefold repetition".into(),
            Some(Outcome::FiftyMove) => "Draw by the fifty-move rule".into(),
            Some(Outcome::InsufficientMaterial) => "Draw by insufficient material".into(),
            None if !self.running && !self.thinking && !self.step => {
                if self.records.is_empty() && self.mode == Mode::Engines {
                    "Ready to watch".into()
                } else {
                    "Game paused".into()
                }
            }
            None if self.thinking || self.needs_engine() => format!(
                "{} is thinking",
                if self.mode == Mode::Engines {
                    format!("Crustacean · {}", color_title(self.board.side))
                } else {
                    "Crustacean".into()
                }
            ),
            None if self.board.in_check(self.board.side) => "You are in check".into(),
            None => "Your move".into(),
        };
        let result = match self.outcome {
            Some(Outcome::Checkmate(Color::White)) => "1-0",
            Some(Outcome::Checkmate(Color::Black)) => "0-1",
            Some(_) => "1/2-1/2",
            None => "*",
        };
        let king = if self.board.in_check(self.board.side) {
            self.board
                .squares
                .iter()
                .position(|p| p.is_some_and(|p| p.kind == Kind::King && p.color == self.board.side))
                .map(square_name)
        } else {
            None
        };
        let moves = self
            .records
            .iter()
            .map(|r| {
                format!(
                    "{{\"uci\":{},\"san\":{},\"number\":{},\"color\":{}}}",
                    quote(&r.uci),
                    quote(&r.san),
                    r.number,
                    quote(color_name(r.side))
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let analysis = self
            .analysis
            .as_ref()
            .map(|(r, c)| {
                object([
                    ("depth", r.completed_depth.to_string()),
                    ("nodes", r.nodes.to_string()),
                    ("elapsed_ms", r.elapsed.as_millis().to_string()),
                    (
                        "score_white",
                        (r.score * if *c == Color::White { 1 } else { -1 }).to_string(),
                    ),
                    (
                        "mate",
                        (r.score.abs() >= crate::search::MATE - crate::search::MAX_PLY as i32)
                            .to_string(),
                    ),
                    (
                        "pv",
                        quote(
                            &r.pv
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" "),
                        ),
                    ),
                ])
            })
            .unwrap_or_else(|| "null".into());
        object([
            ("session", quote(&self.session)),
            ("version", self.version.to_string()),
            (
                "mode",
                quote(if self.mode == Mode::Player {
                    "player"
                } else {
                    "engines"
                }),
            ),
            ("human", quote(color_name(self.human))),
            ("white_ms", self.white_ms.to_string()),
            ("black_ms", self.black_ms.to_string()),
            ("running", self.running.to_string()),
            ("thinking", (self.thinking || self.step).to_string()),
            ("fen", quote(&fen)),
            ("starting_fen", quote(&self.positions[0].to_fen())),
            ("placement", quote(placement)),
            ("side", quote(color_name(self.board.side))),
            (
                "legal_moves",
                format!(
                    "[{}]",
                    legal
                        .iter()
                        .map(|m| quote(&m.to_string()))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            ("moves", format!("[{moves}]")),
            ("status", quote(&status)),
            ("result", quote(result)),
            ("finished", self.outcome.is_some().to_string()),
            (
                "check_square",
                king.map(|s| quote(&s)).unwrap_or_else(|| "null".into()),
            ),
            ("analysis", analysis),
        ])
    }
}

pub(super) fn color_name(c: Color) -> &'static str {
    if c == Color::White {
        "white"
    } else {
        "black"
    }
}
fn color_title(c: Color) -> &'static str {
    if c == Color::White {
        "White"
    } else {
        "Black"
    }
}

pub(super) fn quote(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c < ' ' => {
                let _ = write!(result, "\\u{:04x}", c as u32);
            }
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

fn object<const N: usize>(fields: [(&str, String); N]) -> String {
    format!(
        "{{{}}}",
        fields
            .into_iter()
            .map(|(key, value)| format!("{}:{value}", quote(key)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Standard algebraic notation, derived only from legal engine moves.
fn san(board: &Board, m: Move) -> String {
    let p = board.squares[m.from].unwrap();
    let capture = board.squares[m.to].is_some() || (p.kind == Kind::Pawn && m.from % 8 != m.to % 8);
    let mut result = if p.kind == Kind::King && m.from.abs_diff(m.to) == 2 {
        if m.to > m.from {
            "O-O".into()
        } else {
            "O-O-O".into()
        }
    } else {
        let mut s = String::new();
        if p.kind != Kind::Pawn {
            s.push(letter(p.kind));
            let others: Vec<_> = board
                .legal_moves()
                .into_iter()
                .filter(|other| {
                    other.to == m.to
                        && other.from != m.from
                        && board.squares[other.from].is_some_and(|q| q.kind == p.kind)
                })
                .collect();
            if !others.is_empty() {
                if others.iter().all(|other| other.from % 8 != m.from % 8) {
                    s.push((b'a' + (m.from % 8) as u8) as char);
                } else if others.iter().all(|other| other.from / 8 != m.from / 8) {
                    s.push((b'1' + (m.from / 8) as u8) as char);
                } else {
                    s.push_str(&square_name(m.from));
                }
            }
        } else if capture {
            s.push((b'a' + (m.from % 8) as u8) as char);
        }
        if capture {
            s.push('x');
        }
        s.push_str(&square_name(m.to));
        if let Some(promotion) = m.promotion {
            s.push('=');
            s.push(letter(promotion));
        }
        s
    };
    let child = board.after(m);
    if child.in_check(child.side) {
        result.push(if child.legal_moves().is_empty() {
            '#'
        } else {
            '+'
        });
    }
    result
}
fn letter(k: Kind) -> char {
    match k {
        Kind::Pawn => 'P',
        Kind::Knight => 'N',
        Kind::Bishop => 'B',
        Kind::Rook => 'R',
        Kind::Queen => 'Q',
        Kind::King => 'K',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::START_FEN;
    fn fields(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    #[test]
    fn human_moves_are_legal_versioned_and_turn_bound() {
        let mut g = Game::default();
        let invalid = fields(&[("action", "move"), ("move", "e2e5"), ("version", "0")]);
        assert!(g.action(&invalid).is_err());
        assert_eq!(g.board.to_fen(), START_FEN);
        g.action(&fields(&[
            ("action", "move"),
            ("move", "e2e4"),
            ("version", "0"),
        ]))
        .unwrap();
        assert_eq!(g.records[0].san, "e4");
        assert!(g.needs_engine());
        assert!(g
            .action(&fields(&[
                ("action", "move"),
                ("move", "e7e5"),
                ("version", "1")
            ]))
            .is_err());
        assert_eq!(g.records.len(), 1);
    }
    #[test]
    fn pause_invalidates_search_and_undo_restores_history() {
        let mut g = Game::default();
        g.action(&fields(&[
            ("action", "move"),
            ("move", "e2e4"),
            ("version", "0"),
        ]))
        .unwrap();
        let flag = Arc::clone(&g.cancel);
        g.action(&fields(&[("action", "undo")])).unwrap();
        assert!(flag.load(Ordering::Relaxed));
        assert!(!g.running);
        assert!(!g.needs_engine());
        assert_eq!(g.board.to_fen(), START_FEN);
        assert_eq!(g.positions.len(), 1);
        assert!(g.records.is_empty());
    }
    #[test]
    fn engine_matches_start_paused_and_step_exactly_one_turn() {
        let mut g = Game::default();
        g.action(&fields(&[
            ("action", "new"),
            ("mode", "engines"),
            ("human", "white"),
            ("white_ms", "100"),
            ("black_ms", "200"),
        ]))
        .unwrap();
        assert!(!g.running);
        assert!(!g.needs_engine());
        g.action(&fields(&[("action", "step")])).unwrap();
        assert!(g.needs_engine());
        assert!(!g.running);
        g.action(&fields(&[("action", "pause")])).unwrap();
        assert!(!g.needs_engine());
    }
    #[test]
    fn invalid_settings_and_fen_are_transactional() {
        let mut g = Game::default();
        let before = g.json();
        assert!(g
            .action(&fields(&[
                ("action", "new"),
                ("mode", "engines"),
                ("human", "white"),
                ("white_ms", "0"),
                ("black_ms", "100")
            ]))
            .is_err());
        assert_eq!(g.json(), before);
        assert!(g
            .action(&fields(&[("action", "load"), ("fen", "invalid")]))
            .is_err());
        assert_eq!(g.json(), before);
    }
    #[test]
    fn notation_covers_castling_capture_promotion_check_and_ambiguity() {
        for (fen, input, expected) in [
            (START_FEN, "g1f3", "Nf3"),
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1", "O-O"),
            ("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", "e5d6", "exd6"),
            ("7k/P7/8/8/8/8/8/7K w - - 0 1", "a7a8q", "a8=Q+"),
            ("7k/8/5KQ1/8/8/8/8/8 w - - 0 1", "g6g7", "Qg7#"),
            ("7k/8/8/8/8/8/8/1N2KN2 w - - 0 1", "b1d2", "Nbd2"),
        ] {
            let b = Board::from_fen(fen).unwrap();
            let m = b
                .legal_moves()
                .into_iter()
                .find(|m| m.to_string() == input)
                .unwrap();
            assert_eq!(san(&b, m), expected);
        }
    }
    #[test]
    fn terminal_positions_stop_and_json_escapes_text() {
        let mut g = Game::default();
        g.action(&fields(&[
            ("action", "load"),
            ("fen", "7k/6Q1/6K1/8/8/8/8/8 b - - 0 1"),
        ]))
        .unwrap();
        assert!(g.outcome.is_some());
        assert!(!g.needs_engine());
        assert!(g.json().contains("\"result\":\"1-0\""));
        assert_eq!(quote("a\n\"\\\u{0001}"), "\"a\\n\\\"\\\\\\u0001\"");
    }
}

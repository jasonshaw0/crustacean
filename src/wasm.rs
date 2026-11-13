//! Small WebAssembly boundary. Rules and game state are shared with the native GUI.
use crate::board::{Board, Color};
use crate::form::form;
use crate::game::{quote, Game};
use crate::search::{search, Config, SearchResult, MAX_DEPTH};
use std::cell::RefCell;
use std::time::Duration;

#[link(wasm_import_module = "env")]
extern "C" {
    fn now_ms() -> f64;
}

#[derive(Clone, Copy)]
pub(crate) struct Instant(f64);
impl Instant {
    pub fn now() -> Self {
        // The host supplies a monotonic clock (performance.now).
        Self(unsafe { now_ms() })
    }
    pub fn elapsed(self) -> Duration {
        Duration::from_secs_f64(((Self::now().0 - self.0) / 1000.0).max(0.0))
    }
}

thread_local! {
    static GAME: RefCell<Game> = RefCell::new(Game::default());
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<String> = const { RefCell::new(String::new()) };
}

// The JS adapter fills this owned buffer, then calls request without retaining views.
#[no_mangle]
pub extern "C" fn input_buffer(len: usize) -> *mut u8 {
    INPUT.with_borrow_mut(|input| {
        input.resize(len.min(4 * 1024 * 1024), 0);
        input.as_mut_ptr()
    })
}

#[no_mangle]
pub extern "C" fn output_len() -> usize {
    OUTPUT.with_borrow(|output| output.len())
}

#[no_mangle]
pub extern "C" fn request(operation: u32) -> *const u8 {
    let result = INPUT.with_borrow(|input| {
        std::str::from_utf8(input)
            .map_err(|_| "Invalid text".to_string())
            .and_then(|input| dispatch(operation, input))
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = result.unwrap_or_else(|error| format!("{{\"error\":{}}}", quote(&error)));
        output.as_ptr()
    })
}

fn dispatch(operation: u32, input: &str) -> Result<String, String> {
    match operation {
        0 => Ok(GAME.with_borrow(Game::json)),
        1 => {
            let fields = form(input)?;
            GAME.with_borrow_mut(|game| {
                game.action(&fields).map_err(|(_, message)| message)?;
                Ok(game.json())
            })
        }
        // Snapshot includes every position so repetition stays correct in the worker.
        2 => Ok(GAME.with_borrow(|game| {
            let ms = if game.board.side == Color::White {
                game.white_ms
            } else {
                game.black_ms
            };
            format!(
                "{ms}\n{}",
                game.positions
                    .iter()
                    .map(Board::to_fen)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })),
        3 => {
            let mut lines = input.lines();
            let ms: u64 = lines
                .next()
                .ok_or("Missing time")?
                .parse()
                .map_err(|_| "Invalid time")?;
            let positions = lines.map(Board::from_fen).collect::<Result<Vec<_>, _>>()?;
            let board = positions.last().ok_or("Missing position")?;
            let history = positions.iter().map(Board::key).collect::<Vec<_>>();
            let result = search(
                board,
                Config {
                    depth: Some(MAX_DEPTH),
                    node_limit: u64::MAX,
                    time_limit: Some(Duration::from_millis(ms.clamp(100, 10_000))),
                },
                &history,
            );
            Ok(format!("{{\"move\":{},\"score\":{},\"depth\":{},\"nodes\":{},\"elapsed_ms\":{},\"pv\":{}}}",
                quote(&result.best_move.map(|m| m.to_string()).unwrap_or_default()), result.score,
                result.completed_depth, result.nodes, result.elapsed.as_millis(),
                quote(&result.pv.iter().map(ToString::to_string).collect::<Vec<_>>().join(" "))))
        }
        4 => {
            let fields = form(input)?;
            let field = |name: &str| {
                fields
                    .get(name)
                    .map(String::as_str)
                    .ok_or_else(|| format!("Missing {name}"))
            };
            GAME.with_borrow_mut(|game| {
                let version: u64 = field("version")?.parse().map_err(|_| "Invalid version")?;
                if version != game.version || !game.needs_engine() {
                    return Err("Search is obsolete".into());
                }
                let m = game
                    .board
                    .legal_moves()
                    .into_iter()
                    .find(|m| m.to_string() == field("move").unwrap_or(""))
                    .ok_or("Invalid engine move")?;
                let score = field("score")?.parse().map_err(|_| "Invalid score")?;
                let depth = field("depth")?.parse().map_err(|_| "Invalid depth")?;
                let nodes = field("nodes")?.parse().map_err(|_| "Invalid nodes")?;
                let ms = field("elapsed_ms")?
                    .parse()
                    .map_err(|_| "Invalid elapsed time")?;
                let mut board = game.board.clone();
                let mut pv = Vec::new();
                for uci in field("pv")?.split_whitespace() {
                    let next = board
                        .legal_moves()
                        .into_iter()
                        .find(|m| m.to_string() == uci)
                        .ok_or("Invalid variation")?;
                    board = board.after(next);
                    pv.push(next);
                }
                game.analysis = Some((
                    SearchResult {
                        best_move: Some(m),
                        score,
                        planned_depth: MAX_DEPTH,
                        completed_depth: depth,
                        nodes,
                        qnodes: 0,
                        selective_depth: 0,
                        elapsed: Duration::from_millis(ms),
                        pv,
                        stop_reason: None,
                        budget_exhausted: false,
                        outcome: None,
                    },
                    game.board.side,
                ));
                game.step = false;
                game.thinking = false;
                game.play(m);
                Ok(game.json())
            })
        }
        _ => Err("Unknown operation".into()),
    }
}

//! Minesweeper binding adapter for the generic AI runtime (ADR-0013).
//!
//! `ai_adapter` renders the player-visible side of a `core::Game` into the
//! #94 presentation forms (simple-text / emoji / full-coordinates / image),
//! builds the shared #94/#95 system prompt, and wires `Guide::suggest` — the
//! advisor's "ask the AI" entry point — to a `ai::agent::Agent` round trip
//! over a (mock, in this ticket) provider.
//!
//! Privacy hard constraint: the payload sent to the model contains only
//! player-visible state (hidden / flagged / revealed numbers). The Mine
//! layout is never read and never leaks. `BoardView::from_game` is that
//! privacy seam — it reads only the visible API of `core::Game`.
//!
//! This module depends on `core` + `ai`, never on `server`.

#![allow(dead_code)] // whole public surface is a seam awaiting the /ai/guide route (#117)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::ai::agent::{Agent, AgentError, Session, Tool};
use crate::ai::protocol::{ContentBlock, Message, ProviderError, ProviderErrorKind, StreamChunk};
use crate::core::{CellContent, CellState, CellView, Difficulty, Game, GameState, Position};

/// The #94 presentation forms of a board.
///
/// Wire serialization is kebab-case (`#[serde(rename_all = "kebab-case")]`),
/// aligned with the frontend `ai/api.ts` kebab literals: `SimpleText` →
/// `simple-text`, `FullCoordinates` → `full-coordinates`. It is a `POST
/// /ai/guide/:id` request-body field (sent back by the frontend), so it
/// carries `Deserialize` — together with [`GuideRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoardFormat {
    SimpleText,
    Emoji,
    FullCoordinates,
    Image,
}

/// The player-visible board the model is shown; **never carries the Mine
/// layout** (privacy hard constraint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardView {
    pub difficulty: Difficulty,
    pub state: GameState,
    pub rows: usize,
    pub cols: usize,
    /// The recipe constant = `difficulty.mine_count()`; fixed at creation.
    pub mine_count: usize,
    /// Can go negative (over-flagging).
    pub flags_remaining: i32,
    /// One entry per Cell, row-major; player-visible only.
    pub cells: Vec<CellView>,
}

impl BoardView {
    /// Builds the visible board from a `Game`, reading only the visible API
    /// (`cell_view` / `game_state` / `difficulty` / `size` /
    /// `flags_remaining`) — the privacy seam. It never reads the Mines.
    pub fn from_game(game: &Game) -> Self {
        let size = game.size();
        let rows = size.rows;
        let cols = size.cols;
        let mut cells = Vec::with_capacity(rows * cols);
        for row in 0..rows {
            for col in 0..cols {
                cells.push(game.cell_view(Position::new(row, col)));
            }
        }
        Self {
            difficulty: game.difficulty(),
            state: game.game_state(),
            rows,
            cols,
            mine_count: game.difficulty().mine_count(),
            flags_remaining: game.flags_remaining(),
            cells,
        }
    }
}

/// The shared #94/#95 system prompt: coordinates are 0-based, the model sees
/// only the player-visible board, and the reply must end with the `SUGGEST`
/// contract line (`{"row":N,"col":M}` / `null`). Pure.
pub fn system_prompt() -> String {
    "你是扫雷顾问。玩家给你看当前棋盘，你要推荐他下一步点哪格（或标哪格）。\n\
     坐标系（0-based）：\n\
     - 行和列都从 0 开始编号：row 0 是最顶行，col 0 是最左列；(0,0) 是左上角。\n\
     - 坐标一律用 0-based，不要输出 1-based。\n\
     \n\
     输入说明：\n\
     - 每次你会收到一个**头部** + 一份当前棋盘。\n\
     - 头部含：Difficulty（难度预设）、Rows/Cols（行列数）、Mine count（固定总雷数，始终等于开局 Flag Budget）、\n\
       Flags remaining（总雷数 - 已放旗数，为负表示玩家 over-flag）、Game state（Playing/Won/Lost）。\n\
     - 棋盘只含玩家可见状态：hidden、flagged、revealed 的数字。你**永远看不到真正的雷布局**。\n\
     - 请根据已揭数字 + Mine count 推理，不要臆测看不见的雷。\n\
     \n\
     输出契约：\n\
     - 先给一段简短、可读的推理（说明判断依据）。\n\
     - 然后在**末尾单独一行**给出建议格，格式必须精确如下：\n\
       SUGGEST {\"row\":<r>,\"col\":<c>}\n\
     - 建议格必须是 hidden 格（不要建议已 reveal 或已 flag 的格）。能保证安全就优先安全；\n\
       如果每格都只能靠猜，选概率最高的一格，并在推理里说明\"这是猜、有风险\"。\n\
     - 若棋盘已无法给出任何建议，写：SUGGEST null"
        .to_string()
}

/// A user-message body for a text form (A/B/C). Pure.
pub fn build_text_blocks(view: &BoardView, format: BoardFormat) -> Vec<ContentBlock> {
    let body = match format {
        BoardFormat::SimpleText => build_simple_text(view),
        BoardFormat::Emoji => build_emoji(view),
        BoardFormat::FullCoordinates => build_full_coordinates(view),
        BoardFormat::Image => panic!(
            "build_text_blocks must not be called with BoardFormat::Image; use build_image_blocks"
        ),
    };
    vec![ContentBlock::Text(body)]
}

/// A user-message body for the image form (D) = header text + the screenshot
/// data URL. Pure.
pub fn build_image_blocks(view: &BoardView, image_data_url: &str) -> Vec<ContentBlock> {
    let legend = "棋盘：下面是一张棋盘截图，图中每个格子就是棋盘一格；hidden=未翻开、flag=旗、数字=已翻开的邻雷数。";
    let geometry = "坐标请按你从图上看到的格子，0-based 换算（最顶行是 row 0，最左列是 col 0）。";
    let body = format!(
        "{}\n\n{}\n{}\n\n{}",
        header(view),
        legend,
        geometry,
        "我该点哪一格？"
    );
    vec![
        ContentBlock::Text(body),
        ContentBlock::ImageUrl(image_data_url.to_string()),
    ]
}

/// The termination reason (#97). Mirrored by the wire / frontend so the
/// backend decides the final state. `user_interrupt` comes from the user
/// cancelling (`CancellationToken`); the rest refract upstream failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptReason {
    UserInterrupt,
    RateLimit,
    Timeout,
    UpstreamError,
    Unknown,
}

/// A pre-flight failure (before any content streamed), bucketed (#97 ①) and
/// carried out as an HTTP status + structured error body.
#[derive(Debug)]
pub struct SuggestPreFlightError(AgentError);

impl SuggestPreFlightError {
    /// The underlying agent error.
    pub fn into_inner(self) -> AgentError {
        self.0
    }
}

/// The frontend's request: only `format` (+ an optional `image_data_url` for
/// the image form). The board for a text form is read by the backend from its
/// own `Game`; **no model** is sent (the backend picks the DeepSeek default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuideRequest {
    pub format: BoardFormat,
    /// Image form only: a frontend `html-to-image` PNG data URL (with the
    /// `data:image/png;base64,` prefix).
    #[serde(default)]
    pub image_data_url: Option<String>,
}

/// Default model (text forms); `Guide::suggest` sets it per format.
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
/// Multimodal (vision) model — format D (Image) switches to it.
const VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";

/// The one-shot advisor: inject a board, run one round, stream the result.
///
/// Shared across threads via `Arc<Mutex<Agent>>`; `&self` methods `lock()` to
/// get `&mut Agent` for `set_model`. No concurrency design beyond the lock:
/// the frontend's `GuidePhase.running` means at most one `suggest` runs at a
/// time, and every call's `Session` is local with the model set per format.
pub struct Guide {
    agent: Arc<Mutex<Agent>>,
}

impl Guide {
    /// Builds a `Guide` from a shared `Agent` (DeepSeek or mock).
    pub fn new(agent: Arc<Mutex<Agent>>) -> Self {
        Self { agent }
    }

    /// Injects the board, runs one round, and returns the streamed analysis.
    ///
    /// `Ok(StreamChunk)` advances the analysis, `Ok(Done)` closes it;
    /// a mid-stream break is `Err(InterruptReason)`; a pre-flight failure
    /// (before any content blocks stream) is `Err(SuggestPreFlightError)`.
    pub async fn suggest(
        &self,
        game: &Game,
        req: GuideRequest,
        cancel: CancellationToken,
    ) -> Result<
        impl Stream<Item = Result<StreamChunk, InterruptReason>> + Send,
        SuggestPreFlightError,
    > {
        let view = BoardView::from_game(game);
        let image = matches!(req.format, BoardFormat::Image);

        let mut agent = self.agent.lock().unwrap();
        if image {
            agent.set_model(VISION_MODEL.to_string(), None);
        } else {
            agent.set_model(DEFAULT_MODEL.to_string(), None);
        }

        let blocks = if image {
            // Persist the screenshot for audit; a failure never blocks sending.
            let url = req.image_data_url.clone().unwrap_or_default();
            let _ = persist_image(&url);
            build_image_blocks(&view, &url)
        } else {
            build_text_blocks(&view, req.format)
        };

        let mut session = Session::new(Message::System {
            content: system_prompt(),
        });
        session.push(Message::User { content: blocks });

        let stream = agent
            .stream(&session, cancel)
            .await
            .map_err(SuggestPreFlightError)?;

        Ok(stream.map(|item| match item {
            Ok(chunk) => Ok(chunk),
            Err(AgentError::Cancelled) => Err(InterruptReason::UserInterrupt),
            Err(AgentError::Provider(pe)) => Err(refract_provider_error(&pe)),
            Err(AgentError::NoProvider) => Err(InterruptReason::Unknown),
        }))
    }
}

/// A handle to a single game instance's visible state (ADR-0013: tool binding
/// is a parameter, not a hardcoded single `Game`, leaving the door open for a
/// future two-`Game` `AiPlayWithMe`). Today it wraps the single shared
/// `Arc<Mutex<Game>>`; details land when AiPlay does. The advisor binds no
/// tools.
pub struct GameHandle {
    #[allow(dead_code)]
    game: Arc<Mutex<Game>>,
}

impl GameHandle {
    /// Wraps the shared game handle.
    pub fn new(game: Arc<Mutex<Game>>) -> Self {
        Self { game }
    }
}

/// Binds the `ai::Tool`s for a mode (future AiPlay; the advisor passes an
/// empty set). Bound to a `GameHandle` so future two-`Game` modes don't swap
/// the adapter.
pub fn tools(_handle: &GameHandle) -> Vec<Arc<dyn Tool>> {
    Vec::new()
}

// --- Rendering helpers (private) ---

fn difficulty_label(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Beginner => "Beginner",
        Difficulty::Intermediate => "Intermediate",
        Difficulty::Expert => "Expert",
    }
}

fn state_label(s: GameState) -> &'static str {
    match s {
        GameState::Ready => "Ready",
        GameState::Playing => "Playing",
        GameState::Won => "Won",
        GameState::Lost => "Lost",
    }
}

/// The #94 header block, common to all four forms.
fn header(view: &BoardView) -> String {
    format!(
        "Difficulty: {}\nRows: {}, Cols: {}\nMine count: {}\nFlags remaining: {}\nGame state: {}",
        difficulty_label(view.difficulty),
        view.rows,
        view.cols,
        view.mine_count,
        view.flags_remaining,
        state_label(view.state),
    )
}

/// The simple-text representation of a Cell: `.` hidden, `F` flag,
/// `*` revealed mine (Lost only), `0-8` revealed neighbor count.
fn simple_char(cell: CellView) -> char {
    match cell.state {
        CellState::Hidden => '.',
        CellState::Flagged => 'F',
        CellState::Revealed => match cell.content {
            Some(CellContent::Mine) => '*',
            Some(CellContent::Number(n)) => char::from(b'0' + n),
            None => '?',
        },
    }
}

/// A keycap digit emoji (`1️⃣`-`8️⃣`), i.e. digit + VS16 + U+20E3.
fn keycap(n: u8) -> String {
    format!("{}\u{fe0f}\u{20e3}", n)
}

/// The emoji representation of a Cell: `⬛` hidden, `🚩` flag, `💣` revealed
/// mine (Lost only), `⬜` revealed 0, `1️⃣`-`8️⃣` revealed count.
fn emoji_cell(cell: CellView) -> String {
    match cell.state {
        CellState::Hidden => "⬛".to_string(),
        CellState::Flagged => "🚩".to_string(),
        CellState::Revealed => match cell.content {
            Some(CellContent::Mine) => "💣".to_string(),
            Some(CellContent::Number(0)) => "⬜".to_string(),
            Some(CellContent::Number(n)) => keycap(n),
            None => "?".to_string(),
        },
    }
}

/// Renders the board rows via `fmt(row, col, cell)`, joined by `sep`.
fn render_rows<F>(view: &BoardView, fmt: F, sep: &str) -> String
where
    F: Fn(usize, usize, CellView) -> String,
{
    let mut rows = Vec::with_capacity(view.rows);
    for row in 0..view.rows {
        let cells: Vec<String> = (0..view.cols)
            .map(|col| fmt(row, col, view.cells[row * view.cols + col]))
            .collect();
        rows.push(cells.join(sep));
    }
    rows.join("\n")
}

fn build_simple_text(view: &BoardView) -> String {
    let legend =
        "棋盘（Legend）：`.`=hidden，`*`=revealed mine（仅 Lost），`F`=flag，`0-8`=revealed 数字。";
    let geometry = format!(
        "每行代表一行：第 1 行是 row 0，最后一行是 row {}。行内每个字符代表一格：第 1 个字符是 col 0，最后一个是 col {}。",
        view.rows - 1,
        view.cols - 1,
    );
    let board = render_rows(view, |_, _, c| simple_char(c).to_string(), " ");
    format!(
        "{}\n\n{}\n{}\n\n{}\n\n{}",
        header(view),
        legend,
        geometry,
        board,
        "我该点哪一格？"
    )
}

fn build_emoji(view: &BoardView) -> String {
    let legend = "棋盘（Legend）：`⬛`=hidden，`💣`=revealed mine（仅 Lost），`🚩`=flag，`⬜`=revealed 无雷(0)，`1️⃣`-`8️⃣`=revealed 有雷。";
    let geometry = format!(
        "每个格子是一个 emoji（不是按字符数拆），每行从左到右第 1 个 emoji 是 col 0，最后一个是 col {}。",
        view.cols - 1,
    );
    let board = render_rows(view, |_, _, c| emoji_cell(c), "");
    format!(
        "{}\n\n{}\n{}\n\n{}\n\n{}",
        header(view),
        legend,
        geometry,
        board,
        "我该点哪一格？"
    )
}

fn build_full_coordinates(view: &BoardView) -> String {
    let legend = "棋盘：每个 cell 写成 `[row][col]:x`，x 取值同简单字符（`.`/`*`/`F`/`0-8`）。每格自报坐标，无需数行/列。";
    let board = render_rows(
        view,
        |r, c, cell| format!("[{}][{}]:{}", r, c, simple_char(cell)),
        " ",
    );
    format!(
        "{}\n\n{}\n\n{}\n\n{}",
        header(view),
        legend,
        board,
        "我该点哪一格？"
    )
}

// --- Interrupt refraction (private) ---

/// Maps a mid-stream `ProviderError` to the #97 reason kind. A rate limit
/// (`429`) is `RateLimit`; a transport failure (no HTTP code) or a `408` is
/// `Timeout`; an upstream `5xx` is `UpstreamError`; configuration errors
/// (which should have failed pre-flight, but may surface mid-stream) fall to
/// `Unknown`.
fn refract_provider_error(pe: &ProviderError) -> InterruptReason {
    if pe.code == Some(429) {
        return InterruptReason::RateLimit;
    }
    if pe.code == Some(408) || pe.code.is_none() {
        return InterruptReason::Timeout;
    }
    match pe.kind {
        ProviderErrorKind::Upstream => InterruptReason::UpstreamError,
        ProviderErrorKind::Config => InterruptReason::Unknown,
    }
}

// --- Image persistence (best-effort side effect) ---

static BASE64_SEQ: AtomicU64 = AtomicU64::new(0);

/// Persists a `data:image/png;base64,<payload>` data URL to
/// `<exe_dir>/base64_img/YYYYMMDD_<seed>_<seq>.png`, best-effort: a failure
/// returns `Err` and must never block the send. This is an internal side
/// effect of `Guide::suggest`, not a public interface.
fn persist_image(data_url: &str) -> Result<(), String> {
    let payload = data_url
        .split_once("base64,")
        .map(|(_, payload)| payload)
        .ok_or_else(|| "not a base64 data URL".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    let dir = exe_dir()?.join("base64_img");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let seed: u32 = rand::random();
    let seq = BASE64_SEQ.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{}_{}_{}.png", current_yyyymmdd(), seed, seq));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn exe_dir() -> Result<std::path::PathBuf, String> {
    std::env::current_exe()
        .map(|exe| exe.parent().map(|p| p.to_path_buf()).unwrap_or_default())
        .map_err(|e| e.to_string())
}

/// Today's date as `YYYYMMDD`, via the Proleptic Gregorian day count.
fn current_yyyymmdd() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let days = (now.as_secs() / 86_400) as i64;
    civil_from_days(days)
}

/// Howard Hinnant's `civil_from_days`: converts days since the Unix epoch to
/// a `YYYYMMDD` string.
fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    // Jan/Feb belong to the previous civil year (Hinnant's adjustment).
    let year = y + if m <= 2 { 1 } else { 0 };
    format!("{year:04}{m:02}{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::agent::ProviderSet;
    use crate::ai::protocol::ChatRequest;
    use crate::ai::provider::{MockProvider, Provider, ProviderStream};
    use crate::core::{Features, GameConfig};
    use async_trait::async_trait;
    use futures::stream;

    // --- a deterministic, player-built test view ---

    fn sample_view() -> BoardView {
        // 2x2: (0,0) revealed 0, (0,1) flagged, (1,0) revealed 2, (1,1) hidden.
        BoardView {
            difficulty: Difficulty::Beginner,
            state: GameState::Playing,
            rows: 2,
            cols: 2,
            mine_count: 10,
            flags_remaining: 9,
            cells: vec![
                CellView {
                    state: CellState::Revealed,
                    content: Some(CellContent::Number(0)),
                },
                CellView {
                    state: CellState::Flagged,
                    content: None,
                },
                CellView {
                    state: CellState::Revealed,
                    content: Some(CellContent::Number(2)),
                },
                CellView {
                    state: CellState::Hidden,
                    content: None,
                },
            ],
        }
    }

    fn fresh_game() -> Game {
        Game::with_config(GameConfig::new(Difficulty::Beginner, Features::NONE, None))
    }

    fn mock_agent() -> (Agent, MockProvider) {
        let mock = MockProvider::new();
        let mut set = ProviderSet::new();
        set.insert("mock".to_string(), Box::new(mock.clone()));
        let mut agent = Agent::new(set);
        agent.set_model("initial".to_string(), Some("mock"));
        (agent, mock)
    }

    struct FailingProvider {
        error: ProviderError,
    }

    impl FailingProvider {
        fn new(error: ProviderError) -> Self {
            Self { error }
        }
    }

    #[async_trait]
    impl Provider for FailingProvider {
        async fn stream_chat(
            &self,
            _req: ChatRequest,
            _cancel: CancellationToken,
        ) -> Result<ProviderStream, ProviderError> {
            let error = self.error.clone();
            Ok(Box::pin(stream::iter(vec![Err(error)])))
        }
    }

    // --- BoardView ---

    #[test]
    fn from_game_reflects_a_fresh_game() {
        let view = BoardView::from_game(&fresh_game());
        assert_eq!(view.difficulty, Difficulty::Beginner);
        assert_eq!(view.state, GameState::Ready);
        assert_eq!(view.rows, 9);
        assert_eq!(view.cols, 9);
        assert_eq!(view.mine_count, 10);
        assert_eq!(view.flags_remaining, 10);
        assert_eq!(view.cells.len(), 81);
        assert!(
            view.cells
                .iter()
                .all(|c| c.state == CellState::Hidden && c.content.is_none())
        );
    }

    #[test]
    fn from_game_flags_remaining_can_be_negative() {
        let mut game =
            Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
        // Over-flag: place more Flags than the 10-mine budget.
        for row in 0..2 {
            for col in 0..6 {
                game.toggle_flag(Position::new(row, col));
            }
        }
        let view = BoardView::from_game(&game);
        assert_eq!(view.flags_remaining, 10 - 12);
    }

    #[test]
    fn from_game_reads_only_visible_state() {
        // Reveal a non-mine so we're mid-game; the two Mines stay hidden.
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            Features::NONE,
            &[Position::new(0, 1), Position::new(1, 0)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Playing);
        let view = BoardView::from_game(&game);
        // The hidden Mine cells are visible as Hidden, not as Revealed Mine.
        assert_eq!(view.cells[0 * 9 + 1].state, CellState::Hidden);
        assert_eq!(view.cells[1 * 9 + 0].state, CellState::Hidden);
        assert_eq!(view.cells[0 * 9 + 0].state, CellState::Revealed);
        assert_eq!(view.cells[0 * 9 + 0].content, Some(CellContent::Number(2)));
    }

    // --- system_prompt ---

    #[test]
    fn system_prompt_contains_the_contract() {
        let p = system_prompt();
        assert!(p.contains("0-based"));
        assert!(p.contains("SUGGEST"));
        assert!(p.contains("\"row\""));
        assert!(p.contains("null"));
    }

    // --- build_text_blocks / build_image_blocks ---

    #[test]
    fn simple_text_body_has_header_legend_and_board() {
        let blocks = build_text_blocks(&sample_view(), BoardFormat::SimpleText);
        assert_eq!(blocks.len(), 1);
        let ContentBlock::Text(body) = &blocks[0] else {
            panic!("expected a text block");
        };
        let expected = "Difficulty: Beginner\n\
            Rows: 2, Cols: 2\n\
            Mine count: 10\n\
            Flags remaining: 9\n\
            Game state: Playing\n\
            \n\
            棋盘（Legend）：`.`=hidden，`*`=revealed mine（仅 Lost），`F`=flag，`0-8`=revealed 数字。\n\
            每行代表一行：第 1 行是 row 0，最后一行是 row 1。行内每个字符代表一格：第 1 个字符是 col 0，最后一个是 col 1。\n\
            \n\
            0 F\n\
            2 .\n\
            \n\
            我该点哪一格？";
        assert_eq!(body, expected);
    }

    #[test]
    fn emoji_body_uses_emoji_cells_without_spaces() {
        let blocks = build_text_blocks(&sample_view(), BoardFormat::Emoji);
        let ContentBlock::Text(body) = &blocks[0] else {
            panic!("expected a text block");
        };
        let expected = "Difficulty: Beginner\n\
            Rows: 2, Cols: 2\n\
            Mine count: 10\n\
            Flags remaining: 9\n\
            Game state: Playing\n\
            \n\
            棋盘（Legend）：`⬛`=hidden，`💣`=revealed mine（仅 Lost），`🚩`=flag，`⬜`=revealed 无雷(0)，`1️⃣`-`8️⃣`=revealed 有雷。\n\
            每个格子是一个 emoji（不是按字符数拆），每行从左到右第 1 个 emoji 是 col 0，最后一个是 col 1。\n\
            \n\
            ⬜🚩\n\
            2️⃣⬛\n\
            \n\
            我该点哪一格？";
        assert_eq!(body, expected);
    }

    #[test]
    fn full_coordinates_body_wraps_each_cell_in_its_coordinates() {
        let blocks = build_text_blocks(&sample_view(), BoardFormat::FullCoordinates);
        let ContentBlock::Text(body) = &blocks[0] else {
            panic!("expected a text block");
        };
        let expected = "Difficulty: Beginner\n\
            Rows: 2, Cols: 2\n\
            Mine count: 10\n\
            Flags remaining: 9\n\
            Game state: Playing\n\
            \n\
            棋盘：每个 cell 写成 `[row][col]:x`，x 取值同简单字符（`.`/`*`/`F`/`0-8`）。每格自报坐标，无需数行/列。\n\
            \n\
            [0][0]:0 [0][1]:F\n\
            [1][0]:2 [1][1]:.\n\
            \n\
            我该点哪一格？";
        assert_eq!(body, expected);
    }

    #[test]
    fn image_body_is_a_text_preamble_plus_the_data_url() {
        let blocks = build_image_blocks(&sample_view(), "data:image/png;base64,AAAA");
        assert_eq!(blocks.len(), 2);
        let ContentBlock::Text(body) = &blocks[0] else {
            panic!("expected text preamble");
        };
        let expected = "Difficulty: Beginner\n\
            Rows: 2, Cols: 2\n\
            Mine count: 10\n\
            Flags remaining: 9\n\
            Game state: Playing\n\
            \n\
            棋盘：下面是一张棋盘截图，图中每个格子就是棋盘一格；hidden=未翻开、flag=旗、数字=已翻开的邻雷数。\n\
            坐标请按你从图上看到的格子，0-based 换算（最顶行是 row 0，最左列是 col 0）。\n\
            \n\
            我该点哪一格？";
        assert_eq!(body, expected);
        assert_eq!(
            blocks[1],
            ContentBlock::ImageUrl("data:image/png;base64,AAAA".to_string())
        );
    }

    #[test]
    #[should_panic(expected = "build_text_blocks must not be called with BoardFormat::Image")]
    fn build_text_blocks_rejects_the_image_variant() {
        build_text_blocks(&sample_view(), BoardFormat::Image);
    }

    // --- wire serde ---

    #[test]
    fn board_format_serializes_kebab_case_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&BoardFormat::SimpleText).unwrap(),
            "\"simple-text\""
        );
        assert_eq!(
            serde_json::to_string(&BoardFormat::Emoji).unwrap(),
            "\"emoji\""
        );
        assert_eq!(
            serde_json::to_string(&BoardFormat::FullCoordinates).unwrap(),
            "\"full-coordinates\""
        );
        assert_eq!(
            serde_json::to_string(&BoardFormat::Image).unwrap(),
            "\"image\""
        );
        let parsed: BoardFormat = serde_json::from_str("\"full-coordinates\"").unwrap();
        assert_eq!(parsed, BoardFormat::FullCoordinates);
        let parsed: BoardFormat = serde_json::from_str("\"simple-text\"").unwrap();
        assert_eq!(parsed, BoardFormat::SimpleText);
    }

    #[test]
    fn guide_request_deserializes_format_and_optional_image() {
        let req: GuideRequest = serde_json::from_str(r#"{"format":"emoji"}"#).unwrap();
        assert_eq!(req.format, BoardFormat::Emoji);
        assert_eq!(req.image_data_url, None);
        let req: GuideRequest = serde_json::from_str(
            r#"{"format":"image","image_data_url":"data:image/png;base64,AAAA"}"#,
        )
        .unwrap();
        assert_eq!(req.format, BoardFormat::Image);
        assert_eq!(
            req.image_data_url.as_deref(),
            Some("data:image/png;base64,AAAA")
        );
    }

    #[test]
    fn interrupt_reason_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&InterruptReason::UserInterrupt).unwrap(),
            "\"user_interrupt\""
        );
        assert_eq!(
            serde_json::to_string(&InterruptReason::RateLimit).unwrap(),
            "\"rate_limit\""
        );
        assert_eq!(
            serde_json::to_string(&InterruptReason::Timeout).unwrap(),
            "\"timeout\""
        );
        assert_eq!(
            serde_json::to_string(&InterruptReason::UpstreamError).unwrap(),
            "\"upstream_error\""
        );
        assert_eq!(
            serde_json::to_string(&InterruptReason::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    // --- Guide::suggest ---

    #[tokio::test]
    async fn suggest_streams_reasoning_then_content_then_done() {
        let (agent, mock) = mock_agent();
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ReasoningDelta("Mock reasoning.".into())))
        );
        match stream.next().await {
            // The mock echoes the last user text — our board body — as the
            // content delta, so it must be the rendered board, not the prompt.
            Some(Ok(StreamChunk::ContentDelta(text))) => {
                assert!(text.starts_with("Difficulty: Beginner"));
                assert!(text.contains("我该点哪一格？"));
            }
            other => panic!("expected a content delta, got {other:?}"),
        }
        assert_eq!(stream.next().await, Some(Ok(StreamChunk::Done)));
        assert_eq!(stream.next().await, None);
        // The default model was selected for a text format.
        assert_eq!(mock.last_request().unwrap().model, DEFAULT_MODEL);
    }

    #[tokio::test]
    async fn image_format_selects_the_vision_model() {
        let (agent, mock) = mock_agent();
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::Image,
            // Deliberately not valid base64: persist fails, and must not block.
            image_data_url: Some("data:image/png;base64,not-valid!!!".to_string()),
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        assert_eq!(mock.last_request().unwrap().model, VISION_MODEL);
    }

    #[tokio::test]
    async fn cancelled_suggest_reports_user_interrupt() {
        let (agent, _mock) = mock_agent();
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        let mut stream = guide.suggest(&game, req, cancel).await.unwrap();
        assert_eq!(
            stream.next().await,
            Some(Err(InterruptReason::UserInterrupt))
        );
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn rate_limit_refracts_to_rate_limit_interrupt() {
        let mut set = ProviderSet::new();
        set.insert(
            "mock".to_string(),
            Box::new(FailingProvider::new(ProviderError {
                kind: ProviderErrorKind::Upstream,
                code: Some(429),
                message: "rate limited".into(),
            })),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(stream.next().await, Some(Err(InterruptReason::RateLimit)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn transport_failure_refracts_to_timeout() {
        let mut set = ProviderSet::new();
        set.insert(
            "mock".to_string(),
            Box::new(FailingProvider::new(ProviderError {
                kind: ProviderErrorKind::Upstream,
                code: None,
                message: "connect timeout".into(),
            })),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(stream.next().await, Some(Err(InterruptReason::Timeout)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn upstream_error_refracts_to_upstream_error() {
        let mut set = ProviderSet::new();
        set.insert(
            "mock".to_string(),
            Box::new(FailingProvider::new(ProviderError {
                kind: ProviderErrorKind::Upstream,
                code: Some(500),
                message: "boom".into(),
            })),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            stream.next().await,
            Some(Err(InterruptReason::UpstreamError))
        );
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn config_error_refracts_to_unknown() {
        let mut set = ProviderSet::new();
        set.insert(
            "mock".to_string(),
            Box::new(FailingProvider::new(ProviderError {
                kind: ProviderErrorKind::Config,
                code: Some(400),
                message: "bad request".into(),
            })),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(stream.next().await, Some(Err(InterruptReason::Unknown)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn no_provider_is_a_preflight_error() {
        let agent = Agent::new(ProviderSet::new());
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            format: BoardFormat::SimpleText,
            image_data_url: None,
        };
        // `impl Stream` has no `Debug`; match instead of `unwrap_err`.
        let err = match guide.suggest(&game, req, CancellationToken::new()).await {
            Err(e) => e,
            Ok(_) => panic!("expected a preflight failure"),
        };
        assert_eq!(err.into_inner(), AgentError::NoProvider);
    }

    // --- privacy ---

    #[tokio::test]
    async fn suggest_payload_does_not_leak_hidden_mines() {
        let (agent, mock) = mock_agent();
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            Features::NONE,
            &[Position::new(0, 1), Position::new(1, 0)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Playing);
        let req = GuideRequest {
            format: BoardFormat::FullCoordinates,
            image_data_url: None,
        };
        let mut stream = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().expect("mock recorded a request");
        let payload = serde_json::to_string(&req.messages).unwrap();
        // The two hidden Mines (0,1) and (1,0) appear as hidden '.' in
        // full-coordinates, never as a revealed mine '*'.
        assert!(payload.contains("[0][1]:."));
        assert!(!payload.contains("[0][1]:*"));
        assert!(payload.contains("[1][0]:."));
        assert!(!payload.contains("[1][0]:*"));
        // And (0,0) is the player-visible revealed number, present.
        assert!(payload.contains("[0][0]:2"));
    }

    // --- image persistence (best-effort) ---

    #[test]
    fn persist_image_rejects_non_data_urls() {
        assert!(persist_image("not-a-data-url").is_err());
        assert!(persist_image("data:text/plain,hello").is_err());
        assert!(persist_image("data:image/png;base64,!!!").is_err());
    }

    #[test]
    fn persist_image_writes_for_valid_base64() {
        // "AAAA" decodes to 3 bytes; we write them regardless of PNG validity.
        assert!(persist_image("data:image/png;base64,AAAA").is_ok());
    }

    #[test]
    fn civil_from_days_matches_the_unix_epoch() {
        assert_eq!(civil_from_days(0), "19700101");
    }
}

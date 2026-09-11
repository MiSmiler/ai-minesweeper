//! Minesweeper binding adapter for the generic AI runtime (ADR-0013).
//!
//! `ai_adapter` renders the player-visible side of a `core::Game` into the
//! board presentation (the [`InputMode`]), builds the system prompt (the
//! shared core plus the mode's own section), and wires `Guide::suggest` — the
//! advisor's "ask the AI" entry point — to a `ai::agent::Agent` round trip
//! over a (mock, in this ticket) provider.
//!
//! The user turn carries the board alone (ADR-0016): every rule — the
//! coordinate system, the symbol legend, the output contract — lives in the
//! system prompt. The model is deliberately told neither the Mine count nor
//! the Flag Budget; a tool that answers such questions on demand is future
//! work.
//!
//! Privacy hard constraint: the payload sent to the model contains only
//! player-visible state (hidden / flagged / revealed numbers). The Mine
//! layout is never read and never leaks. `BoardView::from_game` is that
//! privacy seam — it reads only the visible API of `core::Game`.
//!
//! This module depends on `core` + `ai`, never on `server`.

#![allow(dead_code)] // whole public surface is a seam awaiting the /ai/guide route (#117)

use std::sync::Arc;
use tokio::sync::Mutex;

use base64::Engine as _;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::ai::agent::{Agent, AgentError, Session, ThinkingLevel, Tool};
use crate::ai::protocol::{ContentBlock, Message, ProviderError, ProviderErrorKind, StreamChunk};
use crate::core::{CellContent, CellState, CellView, Difficulty, Game, GameState, Position};

/// How the board is put in front of the model: the rendering into the user
/// turn, the system-prompt section that describes it, and the model that
/// serves it. Not `core::PlayMode`, which is the player's view.
///
/// Wire serialization is kebab-case (`#[serde(rename_all = "kebab-case")]`),
/// aligned with the frontend `ai/api.ts` literals: `Plain` → `plain`,
/// `Emoji` → `emoji`, `Image` → `image`. It is a `POST /ai/guide/:id`
/// request-body field (sent back by the frontend), so it carries
/// `Deserialize` — together with [`GuideRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    Plain,
    Emoji,
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

/// The system-prompt core, shared by every [`InputMode`]: the role, the
/// coordinate system, the meaning of the Cell states, and the output
/// contract. [`InputMode::system_prompt`] appends the mode's own section.
///
/// Live text lives in `prompts/system.md` (a repo-level content file, embedded
/// at compile time via `include_str!`); the binary stays self-contained and
/// never reads a prompt file at runtime.
const SYSTEM_CORE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts/system.md"));

/// The [`InputMode::Plain`] system-prompt section: the character legend. In
/// `prompts/plain.md`.
const PLAIN: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts/plain.md"));
/// The [`InputMode::Emoji`] system-prompt section: the emoji legend. In
/// `prompts/emoji.md`.
const EMOJI: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts/emoji.md"));
/// The [`InputMode::Image`] system-prompt section: the screenshot legend. In
/// `prompts/image.md`.
const IMAGE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts/image.md"));

impl InputMode {
    /// The system prompt for this mode: the shared core plus the mode's own
    /// section, separated by a blank line. The prompt files carry a trailing
    /// newline, trimmed so the model sees exactly the contract text. Pure.
    pub fn system_prompt(self) -> String {
        let section = match self {
            Self::Plain => PLAIN,
            Self::Emoji => EMOJI,
            Self::Image => IMAGE,
        };
        format!("{}\n\n{}", SYSTEM_CORE.trim_end(), section.trim_end())
    }

    /// The user turn for this mode: the board, rendered into the mode's
    /// symbols. [`InputMode::Image`] is the screenshot alone and carries no
    /// text — the frontend renders its own copy of that screenshot in the
    /// player's dialog bubble. Pure.
    pub fn user_message(self, view: &BoardView, image_data_url: &str) -> Vec<ContentBlock> {
        match self {
            Self::Plain => vec![ContentBlock::Text(render_plain(view))],
            Self::Emoji => vec![ContentBlock::Text(render_emoji(view))],
            Self::Image => vec![ContentBlock::ImageUrl(image_data_url.to_string())],
        }
    }

    /// The model that serves this mode: the vision model for
    /// [`InputMode::Image`], the text default otherwise.
    pub fn model(self) -> &'static str {
        match self {
            Self::Image => VISION_MODEL,
            _ => DEFAULT_MODEL,
        }
    }
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

/// The frontend's request: only `input_mode` (+ an optional `image_data_url`
/// for the image mode). The board is read by the backend from its own `Game`;
/// **no model** is sent (the backend picks the DeepSeek default per mode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuideRequest {
    pub input_mode: InputMode,
    /// The reasoning depth; `low` default. `off` disables thinking mode.
    #[serde(default)]
    pub thinking_level: ThinkingLevel,
    /// Image form only: a frontend `html-to-image` PNG data URL (with the
    /// `data:image/png;base64,` prefix).
    #[serde(default)]
    pub image_data_url: Option<String>,
}

/// Default model (text modes); `InputMode::model` picks it for `Plain` and
/// `Emoji`.
pub(crate) const DEFAULT_MODEL: &str = "deepseek-v4-flash";
/// Multimodal (vision) model — `InputMode::Image` picks it.
const VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";

/// The one-shot advisor: inject a board, run one round, stream the result.
///
/// Shared across threads via `Arc<Mutex<Agent>>`; `&self` methods `lock()` to
/// get `&mut Agent` for `set_model`. A `tokio::sync::Mutex` is used so the
/// guard held across the `agent.stream(...).await` network round trip is
/// `Send` (a `std::sync::MutexGuard` is not), keeping `suggest`'s future
/// `Send`-compatible with axum handlers. No concurrency design beyond the
/// lock: the frontend's `GuidePhase.running` means at most one `suggest` runs
/// at a time, and every call's `Session` is local with the model set per
/// input mode.
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
    /// The first tuple element is the verbatim player message (the `role: user`
    /// turn) — the board in the chosen input mode — echoed back so the frontend
    /// can render the player's half of the exchange (issue #124). The second is the
    /// analysis stream: `Ok(StreamChunk)` advances it, `Ok(Done)` closes it;
    /// a mid-stream break is `Err(InterruptReason)`; a pre-flight failure
    /// (before any content blocks stream) is `Err(SuggestPreFlightError)`.
    pub async fn suggest(
        &self,
        game: &Game,
        req: GuideRequest,
        cancel: CancellationToken,
    ) -> Result<
        (
            String,
            impl Stream<Item = Result<StreamChunk, InterruptReason>> + Send + use<>,
        ),
        SuggestPreFlightError,
    > {
        let view = BoardView::from_game(game);
        let mode = req.input_mode;
        let url = req.image_data_url.clone().unwrap_or_default();

        let mut agent = self.agent.lock().await;
        agent.set_model(mode.model().to_string(), None);
        // Set the player's reasoning depth (issue #122); the agent translates
        // it onto the request's `reasoning_effort` / `thinking` fields.
        agent.set_thinking_level(Some(req.thinking_level));

        // Persist the screenshot for audit; a failure never blocks sending.
        if matches!(mode, InputMode::Image) {
            let _ = persist_image(&url);
        }
        let blocks = mode.user_message(&view, &url);

        // The verbatim player message: the board body. The image mode's user
        // turn is the screenshot alone, so its echo is empty — the frontend
        // renders its own captured copy in the player's bubble (issue #124).
        let user_text = match &blocks[..] {
            [ContentBlock::Text(t)] => t.clone(),
            _ => String::new(),
        };

        let mut session = Session::new(Message::System {
            content: mode.system_prompt(),
        });
        session.push(Message::User { content: blocks });

        let stream = agent
            .stream(&session, cancel)
            .await
            .map_err(SuggestPreFlightError)?;

        Ok((
            user_text,
            stream.map(|item| match item {
                Ok(chunk) => Ok(chunk),
                Err(AgentError::Cancelled) => Err(InterruptReason::UserInterrupt),
                Err(AgentError::Provider(pe)) => Err(refract_provider_error(&pe)),
                Err(AgentError::NoProvider) => Err(InterruptReason::Unknown),
            }),
        ))
    }
}

/// A handle to a single game instance's visible state (ADR-0013: tool binding
/// is a parameter, not a hardcoded single `Game`, leaving the door open for a
/// future two-`Game` `AiPlayWithMe`). Today it wraps the single shared
/// `Arc<Mutex<Game>>`; details land when AiPlay does. The advisor binds no
/// tools.
pub struct GameHandle {
    #[allow(dead_code)]
    game: Arc<std::sync::Mutex<Game>>,
}

impl GameHandle {
    /// Wraps the shared game handle.
    pub fn new(game: Arc<std::sync::Mutex<Game>>) -> Self {
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

/// The plain representation of a Cell: `.` hidden, `F` flag, `*` revealed
/// mine (Lost only), `0-8` revealed neighbor count.
fn plain_char(cell: CellView) -> char {
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

/// The [`InputMode::Plain`] user turn: one character per Cell, no separator.
fn render_plain(view: &BoardView) -> String {
    render_rows(view, |_, _, c| plain_char(c).to_string(), "")
}

/// The [`InputMode::Emoji`] user turn: one emoji per Cell, no separator.
fn render_emoji(view: &BoardView) -> String {
    render_rows(view, |_, _, c| emoji_cell(c), "")
}

// --- Interrupt refraction (private) ---

/// Maps a mid-stream `ProviderError` to the #97 reason kind. A rate limit
/// (`429`) is `RateLimit`; a transport failure (no HTTP code) or a `408` is
/// `Timeout`; an upstream `5xx` is `UpstreamError`.
///
/// `ProviderErrorKind` is *not* redundant (issue #123): it is a real consumer
/// signal on the pre-flight path, where `config` / `upstream` reach the
/// frontend intact (see `ai_routes::preflight_response`). Here, on the
/// mid-stream path, `Config` is a defensive fallback **only** — in practice
/// DeepSeek never streams one (unknown-model / serialization failures are
/// returned pre-flight from `validate_model`, and `SseState` only ever emits
/// `Upstream`). The `Config -> Unknown` arm below is tested
/// (`config_error_refracts_to_unknown`) and intentionally reports a
/// should-never-surface config error as `Unknown`; it is a contract, not dead
/// code.
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

/// Persists a `data:image/png;base64,<payload>` data URL to
/// `<exe_dir>/base64_img/YYYYMMDD_HHMMSS_<seed>.png`, best-effort: a failure
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
    let path = dir.join(format!("{}_{}.png", current_timestamp(), seed));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn exe_dir() -> Result<std::path::PathBuf, String> {
    std::env::current_exe()
        .map(|exe| exe.parent().map(|p| p.to_path_buf()).unwrap_or_default())
        .map_err(|e| e.to_string())
}

/// Today's date + time as `YYYYMMDD_HHMMSS`, via the Proleptic Gregorian day
/// count (date) and the seconds since epoch (clock time, local-independent).
fn current_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let date = civil_from_days((now / 86_400) as i64);
    let secs = now % 86_400;
    let h = secs / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    format!("{date}_{h:02}{m:02}{s:02}")
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
    use crate::ai::protocol::{ReasoningEffort, ThinkingMode, ThinkingToggle};
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

    /// A fresh Beginner board rendered in `plain`: nine hidden rows.
    fn fresh_beginner_board() -> String {
        ["........."; 9].join("\n")
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

    // --- InputMode::system_prompt ---

    #[test]
    fn system_prompt_is_the_trimmed_core_plus_the_modes_own_section() {
        for (mode, legend) in [
            (InputMode::Plain, "- `.`：未开"),
            (InputMode::Emoji, "- ⬛：未开"),
            (InputMode::Image, "扫雷布局将以棋盘截图的形式展现给你。"),
        ] {
            let p = mode.system_prompt();
            assert!(p.contains("坐标写作 (row,col)"), "core missing: {mode:?}");
            assert!(p.contains(legend), "own section missing: {mode:?}");
            assert!(!p.ends_with('\n'), "trailing newline: {mode:?}");
            assert!(!p.contains("\n\n\n"), "extra blank line: {mode:?}");
        }
    }

    #[test]
    fn a_mode_carries_only_its_own_legend() {
        let plain = InputMode::Plain.system_prompt();
        let emoji = InputMode::Emoji.system_prompt();
        let image = InputMode::Image.system_prompt();
        assert!(plain.contains("- `F`：插旗"));
        assert!(!plain.contains("- 🚩：插旗"));
        assert!(emoji.contains("- 🚩：插旗"));
        assert!(!emoji.contains("- `F`：插旗"));
        assert!(!image.contains("- `F`：插旗"));
        assert!(!image.contains("- 🚩：插旗"));
    }

    // --- InputMode::user_message ---

    #[test]
    fn plain_user_message_is_the_bare_board() {
        let blocks = InputMode::Plain.user_message(&sample_view(), "");
        assert_eq!(blocks, vec![ContentBlock::Text("0F\n2.".to_string())]);
    }

    #[test]
    fn emoji_user_message_is_the_bare_board() {
        let blocks = InputMode::Emoji.user_message(&sample_view(), "");
        assert_eq!(blocks, vec![ContentBlock::Text("⬜🚩\n2️⃣⬛".to_string())]);
    }

    #[test]
    fn image_user_message_is_the_screenshot_alone() {
        let blocks = InputMode::Image.user_message(&sample_view(), "data:image/png;base64,AAAA");
        assert_eq!(
            blocks,
            vec![ContentBlock::ImageUrl(
                "data:image/png;base64,AAAA".to_string()
            )]
        );
    }

    // --- wire serde ---

    #[test]
    fn input_mode_serializes_kebab_case_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&InputMode::Plain).unwrap(),
            "\"plain\""
        );
        assert_eq!(
            serde_json::to_string(&InputMode::Emoji).unwrap(),
            "\"emoji\""
        );
        assert_eq!(
            serde_json::to_string(&InputMode::Image).unwrap(),
            "\"image\""
        );
        let parsed: InputMode = serde_json::from_str("\"image\"").unwrap();
        assert_eq!(parsed, InputMode::Image);
        let parsed: InputMode = serde_json::from_str("\"plain\"").unwrap();
        assert_eq!(parsed, InputMode::Plain);
    }

    #[test]
    fn guide_request_deserializes_input_mode_and_optional_image() {
        let req: GuideRequest = serde_json::from_str(r#"{"input_mode":"emoji"}"#).unwrap();
        assert_eq!(req.input_mode, InputMode::Emoji);
        assert_eq!(req.image_data_url, None);
        let req: GuideRequest = serde_json::from_str(
            r#"{"input_mode":"image","image_data_url":"data:image/png;base64,AAAA"}"#,
        )
        .unwrap();
        assert_eq!(req.input_mode, InputMode::Image);
        assert_eq!(
            req.image_data_url.as_deref(),
            Some("data:image/png;base64,AAAA")
        );
    }

    #[test]
    fn guide_request_defaults_thinking_level_to_low() {
        // A wire body without `thinking_level` defaults to Low (issue #122).
        let req: GuideRequest = serde_json::from_str(r#"{"input_mode":"emoji"}"#).unwrap();
        assert_eq!(req.thinking_level, ThinkingLevel::Low);
        // And a present value parses.
        let req: GuideRequest =
            serde_json::from_str(r#"{"input_mode":"emoji","thinking_level":"off"}"#).unwrap();
        assert_eq!(req.thinking_level, ThinkingLevel::Off);
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (user_text, mut stream) = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        // The verbatim player message is the board body, not the system prompt.
        assert_eq!(user_text, fresh_beginner_board());
        assert_eq!(
            stream.next().await,
            Some(Ok(StreamChunk::ReasoningDelta("Mock reasoning.".into())))
        );
        match stream.next().await {
            // The mock echoes the last user text — our board body — as the
            // content delta, so it must be the rendered board, not the prompt.
            Some(Ok(StreamChunk::ContentDelta(text))) => {
                assert_eq!(text, fresh_beginner_board());
            }
            other => panic!("expected a content delta, got {other:?}"),
        }
        assert_eq!(stream.next().await, Some(Ok(StreamChunk::Done)));
        assert_eq!(stream.next().await, None);
        // The default model was selected for a text input mode.
        assert_eq!(mock.last_request().unwrap().model, DEFAULT_MODEL);
        // The default thinking level (Low) threads onto the request.
        let req = mock.last_request().unwrap();
        assert_eq!(req.reasoning_effort, Some(ReasoningEffort::Low));
        assert_eq!(
            req.thinking,
            Some(ThinkingToggle {
                r#type: ThinkingMode::Enabled
            })
        );
    }

    #[tokio::test]
    async fn suggest_threads_off_into_the_request() {
        let (agent, mock) = mock_agent();
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Off,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().expect("mock recorded a request");
        assert_eq!(req.reasoning_effort, None);
        assert_eq!(
            req.thinking,
            Some(ThinkingToggle {
                r#type: ThinkingMode::Disabled
            })
        );
    }

    #[tokio::test]
    async fn image_mode_selects_the_vision_model() {
        let (agent, mock) = mock_agent();
        let guide = Guide::new(Arc::new(Mutex::new(agent)));
        let game = fresh_game();
        let req = GuideRequest {
            input_mode: InputMode::Image,
            thinking_level: ThinkingLevel::Low,
            // Deliberately not valid base64: persist fails, and must not block.
            image_data_url: Some("data:image/png;base64,not-valid!!!".to_string()),
        };
        let (user_text, mut stream) = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        // The image user turn is the screenshot alone, so the echo is empty;
        // the frontend renders its own captured copy in the player's bubble.
        assert_eq!(user_text, "");
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide.suggest(&game, req, cancel).await.unwrap();
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
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
            input_mode: InputMode::Plain,
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        };
        let (_user_text, mut stream) = guide
            .suggest(&game, req, CancellationToken::new())
            .await
            .unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().expect("mock recorded a request");
        // The user turn is the plain board alone. The two hidden Mines (0,1)
        // and (1,0) render as hidden '.', never as a revealed mine '*'.
        let Message::User { content } = &req.messages[1] else {
            panic!("expected the user turn");
        };
        let ContentBlock::Text(board) = &content[0] else {
            panic!("expected a text block");
        };
        // Row 0 is `2........`: only (0,0) is revealed, a player-visible 2.
        let expected = format!("2........\n{}", ["........."; 8].join("\n"));
        assert_eq!(board, &expected);
        assert!(!board.contains('*'));
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

    #[test]
    fn current_timestamp_is_yyyymmdd_hhmmss() {
        let ts = current_timestamp();
        // `YYYYMMDD_HHMMSS` = 8 digits + '_' + 6 digits.
        assert_eq!(ts.len(), 15);
        assert!(
            ts.chars()
                .enumerate()
                .all(|(i, c)| { (i == 8 && c == '_') || (i != 8 && c.is_ascii_digit()) })
        );
    }
}

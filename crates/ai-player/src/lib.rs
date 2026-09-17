//! The ai-player context: the Minesweeper binding for the generic AI runtime
//! (ADR-0013).
//!
//! [`AiPlayer`] renders the player-visible side of a `game::Game` into the board
//! presentation (the [`InputMode`]), builds the system prompt (the shared core
//! plus the mode's own section), and wires `AiPlayer::send` — the AiPlayer's "ask
//! the AI" entry point — to one Send of an `agent::Agent`.
//!
//! The Session is the Agent's (ADR-0021): the AiPlayer never names one. What
//! lives here is policy — one Game drives one Agent, the InputMode chosen at
//! session creation and fixed for that Session's life, a New Game ending the
//! Session.
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
//! privacy seam — it reads only the visible API of `game::Game`.
//!
//! This crate depends on `game` + `agent`, never on the app (`server`).

use std::sync::{Arc, Mutex as StdMutex};

use base64::Engine as _;
use futures::Stream;
use serde::{Deserialize, Serialize};

use agent::{
    Agent, ContentBlock, DeepSeek, DeepSeekConfig, Message, ProviderError, ProviderSet,
    SendError as AgentSendError, StreamChunk, ThinkingLevel, Tool,
};
use game::{CellContent, CellState, CellView, Difficulty, Game, GameState, Position};

/// How the board is put in front of the model: the rendering into the user
/// turn and the system-prompt section that describes it. Not
/// `game::PlayMode`, which is the player's view.
///
/// Wire serialization is kebab-case (`#[serde(rename_all = "kebab-case")]`),
/// aligned with the frontend `ai/api.ts` literals: `Plain` → `plain`,
/// `Emoji` → `emoji`, `Image` → `image`. It is the `POST /ai/begin`
/// request-body field (sent by the frontend), so it carries `Deserialize`.
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
}

/// The frontend's request: the Send's per-call settings only. The board is
/// read by the backend from its own `Game`; **no model** is sent (the backend
/// picks the DeepSeek default). The InputMode is not here — each Session
/// carries the one it was created with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRequest {
    /// The reasoning depth; `low` default. `off` disables thinking mode.
    #[serde(default)]
    pub thinking_level: ThinkingLevel,
    /// Image form only: a frontend `html-to-image` PNG data URL (with the
    /// `data:image/png;base64,` prefix).
    #[serde(default)]
    pub image_data_url: Option<String>,
}

/// Why a Send did not start. The AiPlayer's own refusal precedes the Agent's:
/// the body it was handed is incomplete for the live Session's InputMode.
#[derive(Debug, PartialEq)]
pub enum SendError {
    /// The Session's InputMode sends a screenshot, and the request carries
    /// none.
    MissingScreenshot,
    /// The agent's own failure or refusal, at delivery or mid-stream.
    Agent(AgentSendError),
}

/// The DeepSeek model that serves every [`InputMode`]: the canonical
/// `deepseek-flash` name. The legacy `deepseek-v4-flash` /
/// `deepseek-v4-flash-vision-exp` names were retired with the V4.1-Flash
/// release and only route there temporarily.
/// <https://api-docs.deepseek.com/zh-cn/news/news260910>
pub(crate) const MODEL: &str = "deepseek-flash";

/// The name the product registers its DeepSeek provider under.
const PROVIDER: &str = "deepseek";

/// The AiPlayer: the Minesweeper binding for one `Agent`. It holds the Agent
/// (which owns the live Session) and the live Session's InputMode; it never
/// names a Session itself. Cloned into the `AppState`, so every field sits
/// behind an `Arc`.
#[derive(Clone)]
pub struct AiPlayer {
    agent: Arc<Agent>,
    /// The live Session's InputMode, chosen by `begin` and cleared by `end`;
    /// `None` exactly when no Session is live.
    mode: Arc<StdMutex<Option<InputMode>>>,
}

impl AiPlayer {
    /// Builds an `AiPlayer` from a shared `Agent` (DeepSeek or mock).
    pub fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            mode: Arc::new(StdMutex::new(None)),
        }
    }

    /// The assembly this product ships: DeepSeek from `DEEPSEEK_API_KEY` and
    /// [`MODEL`] for every Send. It never fails — an environment without a key
    /// yields an `AiPlayer` with no Provider, whose Load fails with a `config`
    /// [`ProviderError`], surfaced to the player at session creation rather
    /// than at startup.
    pub fn from_env() -> Self {
        let mut providers = ProviderSet::new();
        if let Some(config) = DeepSeekConfig::from_env() {
            providers.insert(PROVIDER.to_string(), Box::new(DeepSeek::new(config)));
        }
        let mut agent = Agent::new(providers);
        agent.set_model(MODEL.to_string(), Some(PROVIDER));
        Self::new(Arc::new(agent))
    }

    /// Begins a fresh AI Session under `mode`: `mode`'s system prompt opens the
    /// Session's history, and `mode` stays this Session's for its whole life. A
    /// Load failure leaves the live Session and the mode untouched.
    pub async fn begin(&self, mode: InputMode) -> Result<(), ProviderError> {
        self.agent.create_session(mode.system_prompt()).await?;
        *self.mode.lock().expect("mode lock poisoned") = Some(mode);
        Ok(())
    }

    /// Ends the live AI Session: cancels its in-flight Send, forgets it, and
    /// frees the InputMode for the next one. A no-op when there is no Session.
    pub fn end(&self) {
        self.agent.end_session();
        *self.mode.lock().expect("mode lock poisoned") = None;
    }

    /// Cancels the in-flight Send, if any; `false` when none is in flight. The
    /// stream frees the slot itself, so the Send is free as soon as the
    /// cancelled task notices.
    pub fn interrupt(&self) -> bool {
        self.agent.interrupt()
    }

    /// The `Agent` behind this binding. A caller reads the live Session through
    /// it ([`Agent::messages`]) rather than through a relaying method here: the
    /// Session and its message list are the Agent's (ADR-0021), and the binding
    /// keeps no copy to hand out. What stays here is policy — the InputMode the
    /// live Session was created under, and the board rendering a Send carries.
    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    /// Appends the current board to the live Session and starts the Send.
    ///
    /// The user message renders the board in the live Session's [`InputMode`],
    /// fixed when `begin` created it. The tuple element is the verbatim player
    /// message (the `role: user` turn) echoed back so the frontend can render
    /// the player's half of the exchange (issue #124). The stream advances on
    /// `Ok(StreamChunk)`; a mid-stream break is `Err(agent::SendError)` — the
    /// caller's Interrupt or the Provider's failure, passed through intact;
    /// `Ok(Done)` closes it. A refusal (`NoSession` / `Busy`, or a missing
    /// screenshot for the image mode) is an `Err`, never a stream item.
    pub async fn send(
        &self,
        game: &Game,
        req: SendRequest,
    ) -> Result<
        (
            String,
            impl Stream<Item = Result<StreamChunk, AgentSendError>> + Send + use<>,
        ),
        SendError,
    > {
        // No mode means no live Session — the same refusal the Agent reports.
        let mode = self
            .mode
            .lock()
            .expect("mode lock poisoned")
            .ok_or(SendError::Agent(AgentSendError::NoSession))?;

        if matches!(mode, InputMode::Image) && req.image_data_url.is_none() {
            return Err(SendError::MissingScreenshot);
        }

        let (user_text, user_message) = board_message(mode, game, &req);
        let stream = self
            .agent
            .send(vec![user_message], req.thinking_level)
            .await
            .map_err(SendError::Agent)?;
        Ok((user_text, stream))
    }
}

/// Renders the board into the mode's user message, alongside the verbatim text
/// the frontend shows as the player's own message (issue #124). The image
/// mode's message is the screenshot alone, so its echo is empty — the frontend
/// renders its own captured copy in the player's bubble.
fn board_message(mode: InputMode, game: &Game, req: &SendRequest) -> (String, Message) {
    let view = BoardView::from_game(game);
    let url = req.image_data_url.clone().unwrap_or_default();

    // Persist the screenshot for audit; a failure never blocks sending.
    if matches!(mode, InputMode::Image) {
        let _ = persist_image(&url);
    }

    let blocks = mode.user_message(&view, &url);
    let user_text = match &blocks[..] {
        [ContentBlock::Text(t)] => t.clone(),
        _ => String::new(),
    };
    (user_text, Message::User { content: blocks })
}

/// A handle to a single game instance's visible state (ADR-0013: tool binding
/// is a parameter, not a hardcoded single `Game`, leaving the door open for a
/// future two-`Game` `HumanVsAiPlay`). Today it wraps the single shared
/// `Arc<Mutex<Game>>`; details land when AiPlay does. The AiPlayer binds no
/// tools, so nothing constructs one yet.
#[allow(dead_code)]
pub struct GameHandle {
    #[allow(dead_code)]
    game: Arc<std::sync::Mutex<Game>>,
}

#[allow(dead_code)]
impl GameHandle {
    /// Wraps the shared game handle.
    pub fn new(game: Arc<std::sync::Mutex<Game>>) -> Self {
        Self { game }
    }
}

/// Binds the `ai::Tool`s for a mode (future AiPlay; today the AiPlayer binds an
/// empty set). Bound to a `GameHandle` so future two-`Game` modes don't swap
/// the adapter.
#[allow(dead_code)]
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

// --- Image persistence (best-effort side effect) ---

/// Persists a `data:image/png;base64,<payload>` data URL to
/// `<exe_dir>/base64_img/YYYYMMDD_HHMMSS_<seed>.png`, best-effort: a failure
/// returns `Err` and must never block the send. This is an internal side
/// effect of `AiPlayer::send`, not a public interface.
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
    use agent::ChatRequest;
    use agent::ProviderSet;
    use agent::provider::openai_api;
    use agent::{MockProvider, Provider, ProviderErrorKind, ProviderStream};
    use agent::{ReasoningEffort, ThinkingMode, ThinkingToggle};
    use async_trait::async_trait;
    use futures::{StreamExt, stream};
    use game::{Features, GameConfig};
    use tokio_util::sync::CancellationToken;

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
        agent.set_model(MODEL.to_string(), Some("mock"));
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
    fn send_request_deserializes_the_optional_screenshot() {
        let req: SendRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(req.image_data_url, None);
        let req: SendRequest =
            serde_json::from_str(r#"{"image_data_url":"data:image/png;base64,AAAA"}"#).unwrap();
        assert_eq!(
            req.image_data_url.as_deref(),
            Some("data:image/png;base64,AAAA")
        );
    }

    #[test]
    fn send_request_defaults_thinking_level_to_low() {
        // A wire body without `thinking_level` defaults to Low (issue #122).
        let req: SendRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(req.thinking_level, ThinkingLevel::Low);
        // And a present value parses.
        let req: SendRequest = serde_json::from_str(r#"{"thinking_level":"off"}"#).unwrap();
        assert_eq!(req.thinking_level, ThinkingLevel::Off);
    }

    // --- AiPlayer: session lifecycle ---

    fn ai_player_with_mock() -> (AiPlayer, MockProvider) {
        let (agent, mock) = mock_agent();
        (AiPlayer::new(Arc::new(agent)), mock)
    }

    /// A Send under the default thinking level. The InputMode is not part of
    /// the request: `begin` chose it when it created the Session.
    fn default_request() -> SendRequest {
        SendRequest {
            thinking_level: ThinkingLevel::Low,
            image_data_url: None,
        }
    }

    #[tokio::test]
    async fn send_without_a_live_session_is_no_session() {
        let (ai_player, _mock) = ai_player_with_mock();
        let Err(err) = ai_player.send(&fresh_game(), default_request()).await else {
            panic!("expected an error");
        };
        assert_eq!(err, SendError::Agent(AgentSendError::NoSession));
    }

    #[tokio::test]
    async fn send_streams_reasoning_content_and_done() {
        let (ai_player, mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        let (user_text, mut stream) = ai_player
            .send(&fresh_game(), default_request())
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
        // The model is the same for every input mode.
        assert_eq!(mock.last_request().unwrap().model, MODEL);
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
    async fn send_threads_off_into_the_request() {
        let (ai_player, mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        let req = SendRequest {
            thinking_level: ThinkingLevel::Off,
            image_data_url: None,
        };
        let (_user_text, mut stream) = ai_player.send(&fresh_game(), req).await.unwrap();
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
    async fn the_session_mode_decides_the_system_prompt() {
        let (ai_player, mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Emoji).await.unwrap();
        let (_user_text, mut stream) = ai_player
            .send(&fresh_game(), default_request())
            .await
            .unwrap();
        while stream.next().await.is_some() {}

        let req = mock.last_request().unwrap();
        assert_eq!(
            req.messages[0],
            openai_api::Message::System {
                content: InputMode::Emoji.system_prompt()
            }
        );
    }

    #[tokio::test]
    async fn a_second_send_carries_the_first_exchange() {
        let (ai_player, mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        let game = fresh_game();
        for _ in 0..2 {
            let (_user_text, mut stream) = ai_player.send(&game, default_request()).await.unwrap();
            while stream.next().await.is_some() {}
        }
        // The second request was taken before its own reply streamed, so it is
        // System, User, Assistant, User.
        let req = mock.last_request().unwrap();
        assert_eq!(req.messages.len(), 4);
        assert!(matches!(
            req.messages[0],
            openai_api::Message::System { .. }
        ));
        assert!(matches!(req.messages[1], openai_api::Message::User { .. }));
        assert!(matches!(
            req.messages[2],
            openai_api::Message::Assistant { .. }
        ));
        assert!(matches!(req.messages[3], openai_api::Message::User { .. }));
    }

    #[tokio::test]
    async fn an_interrupted_send_leaves_its_user_message_in_the_history() {
        let (ai_player, mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        let game = fresh_game();
        {
            let (_user_text, mut stream) = ai_player.send(&game, default_request()).await.unwrap();
            // The first chunk arrived; the player then interrupts.
            assert!(stream.next().await.is_some());
            assert!(ai_player.interrupt());
            while stream.next().await.is_some() {}
            // The log is the Agent's (ADR-0026): the player's message stayed,
            // and the Interrupt landed a marker of its own in place of a reply.
            assert!(matches!(
                ai_player.agent.messages().expect("a live session").last(),
                Some(Message::Assistant {
                    interrupt: true,
                    ..
                })
            ));
        }
        // The Interrupt cut only the assistant half: the next request carries
        // the player's message, and not the marker.
        let (_user_text, mut stream) = ai_player.send(&game, default_request()).await.unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().unwrap();
        assert_eq!(req.messages.len(), 3);
        assert!(matches!(
            req.messages[0],
            openai_api::Message::System { .. }
        ));
        assert!(matches!(req.messages[1], openai_api::Message::User { .. }));
        assert!(matches!(req.messages[2], openai_api::Message::User { .. }));
    }

    #[tokio::test]
    async fn send_while_a_send_is_in_flight_is_busy() {
        let (ai_player, _mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        let game = fresh_game();
        let (_user_text, stream) = ai_player.send(&game, default_request()).await.unwrap();
        let Err(err) = ai_player.send(&game, default_request()).await else {
            panic!("expected an error");
        };
        assert_eq!(err, SendError::Agent(AgentSendError::Busy));
        drop(stream);
    }

    #[tokio::test]
    async fn interrupt_without_an_in_flight_send_is_false() {
        let (ai_player, _mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        assert!(!ai_player.interrupt());
    }

    #[tokio::test]
    async fn begin_cancels_the_previous_send() {
        let (ai_player, _mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        let game = fresh_game();
        let (_user_text, mut stream) = ai_player.send(&game, default_request()).await.unwrap();
        ai_player.begin(InputMode::Plain).await.unwrap();
        // The displaced Send reports the caller's interrupt on its next poll...
        assert_eq!(stream.next().await, Some(Err(AgentSendError::Interrupted)));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn end_forgets_the_live_session() {
        let (ai_player, _mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Plain).await.unwrap();
        ai_player.end();
        let Err(err) = ai_player.send(&fresh_game(), default_request()).await else {
            panic!("expected an error");
        };
        assert_eq!(err, SendError::Agent(AgentSendError::NoSession));
    }

    #[tokio::test]
    async fn image_mode_sends_the_screenshot_turn() {
        let (ai_player, mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Image).await.unwrap();
        let req = SendRequest {
            thinking_level: ThinkingLevel::Low,
            // Deliberately not valid base64: persist fails, and must not block.
            image_data_url: Some("data:image/png;base64,not-valid!!!".to_string()),
        };
        let (user_text, mut stream) = ai_player.send(&fresh_game(), req).await.unwrap();
        // The image user turn is the screenshot alone, so the echo is empty;
        // the frontend renders its own captured copy in the player's bubble.
        assert_eq!(user_text, "");
        while stream.next().await.is_some() {}
        // The wired request carries the screenshot block itself, not just
        // `user_message`'s return value.
        let request = mock.last_request().expect("mock recorded a request");
        match request.messages.last() {
            Some(openai_api::Message::User { content }) => {
                let expected = vec![ContentBlock::ImageUrl(
                    "data:image/png;base64,not-valid!!!".to_string(),
                )];
                assert_eq!(content, &expected);
            }
            other => panic!("expected a user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn image_mode_without_a_screenshot_is_refused() {
        let (ai_player, _mock) = ai_player_with_mock();
        ai_player.begin(InputMode::Image).await.unwrap();
        let Err(err) = ai_player.send(&fresh_game(), default_request()).await else {
            panic!("expected an error");
        };
        assert_eq!(err, SendError::MissingScreenshot);
    }

    #[tokio::test]
    async fn begin_without_a_provider_is_a_config_error() {
        let agent = Agent::new(ProviderSet::new());
        let ai_player = AiPlayer::new(Arc::new(agent));
        let err = ai_player.begin(InputMode::Plain).await.unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::Config);
    }

    /// A provider whose `load` succeeds once then fails; pins that a failed
    /// load leaves the live session untouched.
    struct FlakyLoadProvider {
        mock: MockProvider,
        loads: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl Provider for FlakyLoadProvider {
        async fn stream_chat(
            &self,
            req: ChatRequest,
            cancel: CancellationToken,
        ) -> Result<ProviderStream, ProviderError> {
            self.mock.stream_chat(req, cancel).await
        }

        async fn load(&self, _model: &str) -> Result<(), ProviderError> {
            if self
                .loads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                == 0
            {
                Ok(())
            } else {
                Err(ProviderError {
                    kind: ProviderErrorKind::Config,
                    code: None,
                    message: "load failed".into(),
                })
            }
        }
    }

    #[tokio::test]
    async fn a_failed_load_keeps_the_previous_session_live() {
        let mut set = ProviderSet::new();
        set.insert(
            "flaky".to_string(),
            Box::new(FlakyLoadProvider {
                mock: MockProvider::new(),
                loads: std::sync::atomic::AtomicUsize::new(0),
            }),
        );
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("flaky"));
        let ai_player = AiPlayer::new(Arc::new(agent));

        ai_player.begin(InputMode::Plain).await.unwrap();
        let err = ai_player.begin(InputMode::Plain).await.unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::Config);
        // The failed load did not replace the live session: a Send is still
        // accepted (not `NoSession`).
        assert!(
            ai_player
                .send(&fresh_game(), default_request())
                .await
                .is_ok()
        );
    }

    // --- mid-stream provider failure (no refraction) ---

    fn ai_player_with_failing(error: ProviderError) -> AiPlayer {
        let mut set = ProviderSet::new();
        set.insert("mock".to_string(), Box::new(FailingProvider::new(error)));
        let mut agent = Agent::new(set);
        agent.set_model("m".to_string(), Some("mock"));
        AiPlayer::new(Arc::new(agent))
    }

    #[tokio::test]
    async fn a_mid_stream_provider_error_passes_through_intact() {
        let ai_player = ai_player_with_failing(ProviderError {
            kind: ProviderErrorKind::Upstream,
            code: Some(429),
            message: "rate limited".into(),
        });
        ai_player.begin(InputMode::Plain).await.unwrap();
        let (_user_text, mut stream) = ai_player
            .send(&fresh_game(), default_request())
            .await
            .unwrap();
        // The AiPlayer does not refract: the provider's own kind, code and
        // message reach the caller untouched (ADR-0022).
        match stream.next().await {
            Some(Err(AgentSendError::Provider(pe))) => {
                assert_eq!(pe.kind, ProviderErrorKind::Upstream);
                assert_eq!(pe.code, Some(429));
                assert_eq!(pe.message, "rate limited");
            }
            other => panic!("expected the provider error intact, got {other:?}"),
        }
        assert_eq!(stream.next().await, None);
    }

    // --- privacy ---

    #[tokio::test]
    async fn send_payload_does_not_leak_hidden_mines() {
        let (ai_player, mock) = ai_player_with_mock();
        let mut game = Game::with_mines(
            Difficulty::Beginner,
            Features::NONE,
            &[Position::new(0, 1), Position::new(1, 0)],
        );
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Playing);
        ai_player.begin(InputMode::Plain).await.unwrap();
        let (_user_text, mut stream) = ai_player.send(&game, default_request()).await.unwrap();
        while stream.next().await.is_some() {}
        let req = mock.last_request().expect("mock recorded a request");
        // The user turn is the plain board alone. The two hidden Mines (0,1)
        // and (1,0) render as hidden '.', never as a revealed mine '*'.
        let openai_api::Message::User { content } = &req.messages[1] else {
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

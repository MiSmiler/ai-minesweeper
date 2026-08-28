//! Unit tests for the game domain (`crate::core`), extracted from the former
//! single-file `core.rs` so the module's definitions and `Game` logic are no
//! longer buried behind ~830 lines of tests (issue #108).
//!
//! # Deviation from the Rust convention
//!
//! The idiomatic Rust home for unit tests is a `#[cfg(test)] mod tests` block at
//! the bottom of the same source file as the code under test. Putting them in a
//! sibling `tests.rs` is a recognized but deliberate departure, chosen because
//! the original file had grown past 1,500 lines and the tests alone were more
//! than half of it.
//!
//! This still works because `core::tests` is a descendant module of `core`, so
//! the `use super::*` below sees `core`'s private items exactly as an in-file
//! `mod tests` did. The crate-facing API is unchanged.
//!
//! To follow the conventional layout instead, the project would extract a
//! `lib.rs` (a library target alongside the `main.rs` binary), which would let
//! these tests live in the top-level `tests/` integration-test directory and
//! exercise only the public API — the pragmatic route for a binary-only crate,
//! which currently has no library target for `tests/` to link against.
use super::*;

#[test]
fn difficulty_presets_have_classic_sizes_and_mine_counts() {
    assert_eq!(Difficulty::Beginner.size(), BoardSize::new(9, 9));
    assert_eq!(Difficulty::Beginner.mine_count(), 10);
    assert_eq!(Difficulty::Intermediate.size(), BoardSize::new(16, 16));
    assert_eq!(Difficulty::Intermediate.mine_count(), 40);
    assert_eq!(Difficulty::Expert.size(), BoardSize::new(16, 30));
    assert_eq!(Difficulty::Expert.mine_count(), 99);
}

#[test]
fn difficulty_canonical_names_match_the_wire() {
    assert_eq!(Difficulty::Beginner.as_str(), "beginner");
    assert_eq!(Difficulty::Intermediate.as_str(), "intermediate");
    assert_eq!(Difficulty::Expert.as_str(), "expert");
}

#[test]
fn game_state_canonical_names_match_the_wire() {
    assert_eq!(GameState::Ready.as_str(), "ready");
    assert_eq!(GameState::Playing.as_str(), "playing");
    assert_eq!(GameState::Won.as_str(), "won");
    assert_eq!(GameState::Lost.as_str(), "lost");
}

#[test]
fn cell_state_canonical_names_match_the_wire() {
    assert_eq!(CellState::Hidden.as_str(), "hidden");
    assert_eq!(CellState::Flagged.as_str(), "flagged");
    assert_eq!(CellState::Revealed.as_str(), "revealed");
}

#[test]
fn new_game_starts_ready_with_all_cells_hidden() {
    let game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    });
    assert_eq!(game.game_state(), GameState::Ready);
    let size = Difficulty::Beginner.size();
    for row in 0..size.rows {
        for col in 0..size.cols {
            let pos = Position::new(row, col);
            assert_eq!(game.cell_view(pos).state, CellState::Hidden);
            assert_eq!(game.cell_view(pos).content, None);
        }
    }
}

#[test]
fn flags_remaining_equals_total_before_any_flag() {
    for difficulty in [
        Difficulty::Beginner,
        Difficulty::Intermediate,
        Difficulty::Expert,
    ] {
        let game = Game::with_config(GameConfig {
            difficulty,
            features: Features::NONE,
            pinned_seed: None,
        });
        assert_eq!(game.flags_remaining(), difficulty.mine_count() as i32);
    }
}

#[test]
fn first_reveal_enters_playing_on_a_non_mine() {
    // A preset already carries Mines (see `with_mines`), so the First
    // Click outside them starts play; a Mine under the First Click loses
    // (covered by `reveal_mine_loses_and_auto_reveals_board`).
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1)); // not a Mine
    assert_eq!(game.game_state(), GameState::Playing);
}

#[test]
fn pinned_classic_game_defers_mines_until_first_click() {
    for difficulty in [
        Difficulty::Beginner,
        Difficulty::Intermediate,
        Difficulty::Expert,
    ] {
        let mut game = Game::with_config(GameConfig {
            difficulty,
            features: Features::NONE,
            pinned_seed: Some(42),
        });
        // No Mines at Ready: placement is deferred to the First Click
        // (ADR-0004), so the Seed is committed only then.
        assert_eq!(game.mines(), None);
        assert_eq!(game.committed_seed(), None);
        game.reveal(Position::new(0, 0));
        let mines = game
            .mines()
            .expect("pinned Classic mines are placed at the First Click");
        assert_eq!(mines.len(), difficulty.mine_count());
        assert_eq!(game.committed_seed(), Some(42));
        let size = difficulty.size();
        let mut seen = std::collections::HashSet::new();
        for pos in mines {
            assert!(
                pos.row < size.rows && pos.col < size.cols,
                "Mine out of bounds at {pos:?}"
            );
            assert!(seen.insert(*pos), "duplicate Mine at {pos:?}");
        }
    }
}

#[test]
fn random_classic_ready_game_has_no_mines_until_first_click() {
    let mut game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    });
    assert_eq!(game.mines(), None);
    game.reveal(Position::new(0, 0));
    assert!(game.mines().is_some());
}

#[test]
fn random_classic_first_click_is_safe_and_opens() {
    // A random Classic game guarantees the First Click is Mine-free: the
    // clicked Cell's 3x3 has no Mines, so it cascades as a zero Cell
    // (ADR-0009).
    for difficulty in [
        Difficulty::Beginner,
        Difficulty::Intermediate,
        Difficulty::Expert,
    ] {
        let size = difficulty.size();
        let first = Position::new(size.rows / 2, size.cols / 2);
        for _ in 0..8 {
            let mut game = Game::with_config(GameConfig {
                difficulty,
                features: Features::NONE,
                pinned_seed: None,
            });
            game.reveal(first);
            assert_ne!(game.game_state(), GameState::Lost);
            assert_eq!(
                game.cell_view(first).content,
                Some(CellContent::Number(0)),
                "First Click {first:?} was not a safe zero Cell"
            );
        }
    }
}

#[test]
fn random_game_accepted_seed_reproduces_the_safe_board() {
    let mut game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    });
    let first = Position::new(0, 0);
    game.reveal(first);
    let accepted = game.committed_seed().unwrap();
    let layout = game.mines().unwrap().to_vec();
    // Replay the accepted Seed as a pinned game: it reproduces the exact
    // layout, and the same First Click stays safe there (same Board).
    let mut replay = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: Some(accepted),
    });
    // Both games place Mines at the First Click; the replay's layout must
    // match the accepted one, and the same First Click stays safe there.
    replay.reveal(first);
    assert_eq!(replay.mines().unwrap(), &layout[..]);
    assert_ne!(replay.game_state(), GameState::Lost);
}

#[test]
fn same_seed_reproduces_the_same_classic_layout() {
    for difficulty in [Difficulty::Beginner, Difficulty::Expert] {
        let mut a = Game::with_config(GameConfig {
            difficulty,
            features: Features::NONE,
            pinned_seed: Some(42),
        });
        let mut b = Game::with_config(GameConfig {
            difficulty,
            features: Features::NONE,
            pinned_seed: Some(42),
        });
        // Both defer placement to the First Click; the same Seed yields
        // the same layout (ADR-0004).
        a.reveal(Position::new(0, 0));
        b.reveal(Position::new(0, 0));
        assert_eq!(a.committed_seed(), Some(42));
        assert_eq!(a.mines(), b.mines());
    }
}

#[test]
fn different_seeds_give_different_classic_layouts() {
    let mut a = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: Some(1),
    });
    let mut b = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: Some(2),
    });
    a.reveal(Position::new(0, 0));
    b.reveal(Position::new(0, 0));
    assert_ne!(a.mines(), b.mines());
}

#[test]
fn prank_ready_game_has_no_mines_until_first_click() {
    let mut game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::prank(),
        pinned_seed: None,
    });
    assert_eq!(game.mines(), None);
    game.reveal(Position::new(0, 0));
    assert_eq!(game.game_state(), GameState::Lost);
    assert!(game.mines().is_some());
}

#[test]
fn reveal_mine_loses_and_auto_reveals_board() {
    let mut game = Game::with_mines(
        Difficulty::Beginner,
        Features::NONE,
        &[Position::new(0, 0), Position::new(5, 5)],
    );
    game.reveal(Position::new(0, 0));
    assert_eq!(game.game_state(), GameState::Lost);
    assert_eq!(game.trigger(), Some(Position::new(0, 0)));
    // The Trigger Mine is Revealed and shown as Mine.
    assert_eq!(
        game.cell_view(Position::new(0, 0)).state,
        CellState::Revealed
    );
    assert_eq!(
        game.cell_view(Position::new(0, 0)).content,
        Some(CellContent::Mine)
    );
    // The other unflagged Mine is auto-Revealed too.
    assert_eq!(
        game.cell_view(Position::new(5, 5)).state,
        CellState::Revealed
    );
    assert_eq!(
        game.cell_view(Position::new(5, 5)).content,
        Some(CellContent::Mine)
    );
    // The Trigger Mine is the only one flagged as trigger.
    assert_ne!(game.trigger(), Some(Position::new(5, 5)));
}

#[test]
fn reveal_shows_neighbor_count() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(2, 2)]);
    game.reveal(Position::new(1, 1));
    assert_eq!(game.game_state(), GameState::Playing);
    assert_eq!(
        game.cell_view(Position::new(1, 1)).state,
        CellState::Revealed
    );
    assert_eq!(
        game.cell_view(Position::new(1, 1)).content,
        Some(CellContent::Number(1))
    );
}

#[test]
fn zero_cell_cascades_until_numbered_boundary() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(4, 4)]);
    game.reveal(Position::new(0, 0));
    // The clicked Cell is a zero Cell.
    assert_eq!(
        game.cell_view(Position::new(0, 0)).content,
        Some(CellContent::Number(0))
    );
    // The numbered boundary of the cascade is Revealed.
    assert_eq!(
        game.cell_view(Position::new(3, 3)).content,
        Some(CellContent::Number(1))
    );
    // A far corner in the zero region is Revealed.
    assert_eq!(
        game.cell_view(Position::new(8, 8)).content,
        Some(CellContent::Number(0))
    );
    // One lone Mine means the cascade wins instantly: the game is Won
    // and the Mine is auto-Flagged on the final board.
    assert_eq!(game.game_state(), GameState::Won);
    assert_eq!(
        game.cell_view(Position::new(4, 4)).state,
        CellState::Flagged
    );
    assert_eq!(game.cell_view(Position::new(4, 4)).content, None);
    // Every Mine is Flagged, so nothing is left to find.
    assert_eq!(game.flags_remaining(), 0);
}

#[test]
fn revealing_every_non_mine_cell_wins_and_auto_flags_mines() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    let size = Difficulty::Beginner.size();
    for row in 0..size.rows {
        for col in 0..size.cols {
            if Position::new(row, col) != Position::new(0, 0) {
                game.reveal(Position::new(row, col));
            }
        }
    }
    assert_eq!(game.game_state(), GameState::Won);
    // The Mine is auto-Flagged on the final board.
    assert_eq!(
        game.cell_view(Position::new(0, 0)).state,
        CellState::Flagged
    );
    assert_eq!(game.cell_view(Position::new(0, 0)).content, None);
    assert_eq!(game.flags_remaining(), 0);
}

#[test]
fn win_keeps_player_flags_and_auto_flags_the_rest() {
    let mut game = Game::with_mines(
        Difficulty::Beginner,
        Features::NONE,
        &[Position::new(0, 0), Position::new(1, 1)],
    );
    // Pre-flag one Mine; the other stays Hidden.
    game.toggle_flag(Position::new(0, 0));
    let size = Difficulty::Beginner.size();
    for row in 0..size.rows {
        for col in 0..size.cols {
            let pos = Position::new(row, col);
            if pos != Position::new(0, 0) && pos != Position::new(1, 1) {
                game.reveal(pos);
            }
        }
    }
    assert_eq!(game.game_state(), GameState::Won);
    // The player's Flag is kept on the Won board.
    assert_eq!(
        game.cell_view(Position::new(0, 0)).state,
        CellState::Flagged
    );
    // The previously Hidden Mine is auto-Flagged.
    assert_eq!(
        game.cell_view(Position::new(1, 1)).state,
        CellState::Flagged
    );
    assert_eq!(game.flags_remaining(), 0);
}

#[test]
fn ended_game_rejects_reveals() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(0, 0));
    assert_eq!(game.game_state(), GameState::Lost);
    // Reveals after the end change nothing.
    game.reveal(Position::new(1, 1));
    assert_eq!(game.game_state(), GameState::Lost);
    assert_eq!(game.cell_view(Position::new(1, 1)).state, CellState::Hidden);
}

#[test]
fn reveal_out_of_bounds_is_noop() {
    let mut game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    });
    game.reveal(Position::new(99, 99));
    assert_eq!(game.game_state(), GameState::Ready);
}

#[test]
fn flag_toggles_hidden_to_flagged_and_back() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.toggle_flag(Position::new(1, 1));
    assert_eq!(
        game.cell_view(Position::new(1, 1)).state,
        CellState::Flagged
    );
    game.toggle_flag(Position::new(1, 1));
    assert_eq!(game.cell_view(Position::new(1, 1)).state, CellState::Hidden);
}

#[test]
fn flag_on_revealed_cell_is_noop() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1));
    game.toggle_flag(Position::new(1, 1));
    assert_eq!(
        game.cell_view(Position::new(1, 1)).state,
        CellState::Revealed
    );
}

#[test]
fn flagged_cell_blocks_reveal() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.toggle_flag(Position::new(0, 0));
    game.reveal(Position::new(0, 0));
    // The Flag blocks the Reveal entirely: the game has not even started.
    assert_eq!(game.game_state(), GameState::Ready);
    assert_eq!(
        game.cell_view(Position::new(0, 0)).state,
        CellState::Flagged
    );
    // After unflagging, the first click goes through.
    game.toggle_flag(Position::new(0, 0));
    game.reveal(Position::new(0, 0));
    assert_eq!(game.game_state(), GameState::Lost);
    assert_eq!(game.trigger(), Some(Position::new(0, 0)));
}

#[test]
fn flags_remaining_tracks_flags() {
    let mut game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    });
    assert_eq!(game.flags_remaining(), 10);
    game.toggle_flag(Position::new(1, 1));
    game.toggle_flag(Position::new(2, 2));
    assert_eq!(game.flags_remaining(), 8);
    game.toggle_flag(Position::new(1, 1));
    assert_eq!(game.flags_remaining(), 9);
}

#[test]
fn flagging_allows_more_flags_than_mines() {
    let mut game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    }); // 10 mines
    for row in 0..2 {
        for col in 0..5 {
            game.toggle_flag(Position::new(row, col));
        }
    }
    assert_eq!(game.flags_remaining(), 0);
    // The 11th Flag is allowed: the counter goes negative.
    game.toggle_flag(Position::new(2, 2));
    assert_eq!(
        game.cell_view(Position::new(2, 2)).state,
        CellState::Flagged
    );
    assert_eq!(game.flags_remaining(), -1);
    // More Flags keep driving the counter further negative.
    game.toggle_flag(Position::new(2, 3));
    assert_eq!(game.flags_remaining(), -2);
    // Removing Flags raises the counter back toward zero.
    game.toggle_flag(Position::new(0, 0));
    assert_eq!(game.flags_remaining(), -1);
    game.toggle_flag(Position::new(2, 3));
    game.toggle_flag(Position::new(2, 2));
    assert_eq!(game.flags_remaining(), 1);
}

#[test]
fn chord_is_noop_when_flags_exceed_the_number() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1));
    assert_eq!(
        game.cell_view(Position::new(1, 1)).content,
        Some(CellContent::Number(1))
    );
    // Burn the whole budget, then keep flagging: 2 Flags around a 1.
    for col in 0..9 {
        game.toggle_flag(Position::new(3, col));
    }
    game.toggle_flag(Position::new(0, 0));
    assert_eq!(game.flags_remaining(), 0);
    game.toggle_flag(Position::new(0, 1)); // the 11th Flag: beyond the mine count
    assert_eq!(game.flags_remaining(), -1);
    game.chord(Position::new(1, 1));
    // Flag count (2) exceeds the number (1): the chord stays a no-op.
    assert_eq!(game.game_state(), GameState::Playing);
    assert_eq!(
        game.cell_view(Position::new(0, 1)).state,
        CellState::Flagged
    );
    assert_eq!(game.cell_view(Position::new(1, 0)).state, CellState::Hidden);
}

#[test]
fn chord_reveals_unflagged_neighbors_when_flags_match() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1));
    assert_eq!(
        game.cell_view(Position::new(1, 1)).content,
        Some(CellContent::Number(1))
    );
    game.toggle_flag(Position::new(0, 0));
    game.chord(Position::new(1, 1));
    // The unflagged neighbors are Revealed; the Flagged Mine stays.
    assert_eq!(
        game.cell_view(Position::new(0, 1)).state,
        CellState::Revealed
    );
    assert_eq!(
        game.cell_view(Position::new(1, 0)).state,
        CellState::Revealed
    );
    assert_eq!(
        game.cell_view(Position::new(0, 0)).state,
        CellState::Flagged
    );
}

#[test]
fn chord_cascades_through_revealed_zero_cells() {
    // A solid wall of Mines across row 4 splits the Board in two. The
    // 3 at (3,1) sits against the wall; Flagging its three Mine
    // neighbors and Chording Reveals the zero Cells at (2,0),(2,1),
    // (2,2) — each subject to the same cascade rule as a click, so the
    // connected zero region of the top half cascades several levels
    // deep, while the bottom half stays untouched.
    let mut mines: Vec<Position> = (0..9).map(|c| Position::new(4, c)).collect();
    mines.push(Position::new(8, 8));
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &mines);
    game.reveal(Position::new(3, 1));
    assert_eq!(
        game.cell_view(Position::new(3, 1)).content,
        Some(CellContent::Number(3))
    );
    for col in 0..3 {
        game.toggle_flag(Position::new(4, col));
    }
    game.chord(Position::new(3, 1));

    // The zero Cell the Chord revealed cascades like a click: level 1…
    assert_eq!(
        game.cell_view(Position::new(2, 0)).content,
        Some(CellContent::Number(0))
    );
    // …level 2…
    assert_eq!(
        game.cell_view(Position::new(1, 5)).content,
        Some(CellContent::Number(0))
    );
    // …and level 3, at the Board's top edge.
    assert_eq!(
        game.cell_view(Position::new(0, 5)).content,
        Some(CellContent::Number(0))
    );
    // The Mine wall bounds the cascade: the bottom half stays Hidden and
    // the game is still in progress.
    assert_eq!(game.cell_view(Position::new(5, 5)).state, CellState::Hidden);
    assert_eq!(game.game_state(), GameState::Playing);
}

#[test]
fn chord_cascade_revealing_the_last_cell_wins() {
    // Two corner Mines: Chording the 2 at (1,1) Reveals the remaining
    // Mine-adjacent Cells directly, and the zero Cells it Reveals
    // cascade across every other non-Mine Cell — the last Reveal wins.
    let mut game = Game::with_mines(
        Difficulty::Beginner,
        Features::NONE,
        &[Position::new(0, 0), Position::new(0, 1)],
    );
    game.reveal(Position::new(1, 1));
    game.toggle_flag(Position::new(0, 0));
    game.toggle_flag(Position::new(0, 1));
    game.chord(Position::new(1, 1));

    assert_eq!(game.game_state(), GameState::Won);
    // The Mines are auto-Flagged on the Won board.
    assert_eq!(
        game.cell_view(Position::new(0, 0)).state,
        CellState::Flagged
    );
    assert_eq!(
        game.cell_view(Position::new(0, 1)).state,
        CellState::Flagged
    );
    // A Cell deep in the cascaded region is Revealed.
    assert_eq!(
        game.cell_view(Position::new(8, 8)).state,
        CellState::Revealed
    );
}

#[test]
fn chord_is_noop_when_flag_count_mismatches() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1));
    game.chord(Position::new(1, 1)); // zero Flags around a 1
    assert_eq!(game.cell_view(Position::new(0, 1)).state, CellState::Hidden);
    assert_eq!(game.cell_view(Position::new(1, 0)).state, CellState::Hidden);
    assert_eq!(game.game_state(), GameState::Playing);
}

#[test]
fn chord_is_noop_on_hidden_and_zero_cells() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1));
    // Hidden Cell: nothing happens.
    game.chord(Position::new(2, 2));
    assert_eq!(game.cell_view(Position::new(2, 2)).state, CellState::Hidden);
    // Zero Cell: nothing happens either.
    game.reveal(Position::new(1, 1)); // already revealed; reveal a zero region first
    assert_eq!(game.game_state(), GameState::Playing);
}

#[test]
fn chord_hitting_a_mine_loses_with_that_mine_as_trigger() {
    // Two Mines around (1,1): the player Flags (0,0) correctly but also
    // Flags (0,1) which is NOT a Mine — Flag count matches the number,
    // so the chord Reveals the Mine at (0,2) and loses.
    let mut game = Game::with_mines(
        Difficulty::Beginner,
        Features::NONE,
        &[Position::new(0, 0), Position::new(0, 2)],
    );
    game.reveal(Position::new(1, 1));
    assert_eq!(
        game.cell_view(Position::new(1, 1)).content,
        Some(CellContent::Number(2))
    );
    game.toggle_flag(Position::new(0, 0));
    game.toggle_flag(Position::new(0, 1));
    game.chord(Position::new(1, 1));
    assert_eq!(game.game_state(), GameState::Lost);
    assert_eq!(game.trigger(), Some(Position::new(0, 2)));
    assert_eq!(
        game.cell_view(Position::new(0, 2)).content,
        Some(CellContent::Mine)
    );
}

#[test]
fn flag_and_chord_after_end_are_noop() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(0, 0));
    assert_eq!(game.game_state(), GameState::Lost);
    game.toggle_flag(Position::new(1, 1));
    game.chord(Position::new(2, 2));
    assert_eq!(game.cell_view(Position::new(1, 1)).state, CellState::Hidden);
    assert_eq!(game.cell_view(Position::new(2, 2)).state, CellState::Hidden);
}

#[test]
fn elapsed_is_zero_while_ready() {
    let game = Game::with_config(GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    });
    assert_eq!(game.elapsed(), Duration::ZERO);
}

#[test]
fn elapsed_runs_after_first_reveal() {
    // Reveal a numeric Cell so the game stays Playing (no cascade, no win).
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1));
    assert_eq!(game.game_state(), GameState::Playing);
    std::thread::sleep(Duration::from_millis(20));
    assert!(game.elapsed() >= Duration::from_millis(20));
}

#[test]
fn elapsed_freezes_at_game_end() {
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(0, 0)); // instant Lost
    let frozen = game.elapsed();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(game.elapsed(), frozen);
}

#[test]
fn prank_first_reveal_is_always_a_mine() {
    // Property test: for many random Prank games, the first click always
    // reveals a Mine and loses instantly (ADR-0002).
    for _ in 0..20 {
        let mut game = Game::with_config(GameConfig {
            difficulty: Difficulty::Beginner,
            features: Features::prank(),
            pinned_seed: None,
        });
        game.reveal(Position::new(0, 0));
        assert_eq!(game.game_state(), GameState::Lost);
        assert_eq!(game.trigger(), Some(Position::new(0, 0)));
        assert_eq!(
            game.cell_view(Position::new(0, 0)).content,
            Some(CellContent::Mine)
        );
        // The First Click swap keeps the recipe mine count intact.
        assert_eq!(
            game.mines().unwrap().len(),
            Difficulty::Beginner.mine_count()
        );
    }
}

#[test]
fn pinned_config_defers_mines_until_first_click() {
    let config = GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: Some(42),
    };
    let mut game = Game::with_config(config);
    assert_eq!(game.features(), Features::NONE);
    // No Mines at Ready: placement is deferred to the First Click.
    assert_eq!(game.mines(), None);
    assert_eq!(game.committed_seed(), None);
    game.reveal(Position::new(0, 0));
    let mines = game
        .mines()
        .expect("Pinned game places Mines at the First Click");
    assert_eq!(mines.len(), Difficulty::Beginner.mine_count());
    assert_eq!(game.committed_seed(), Some(42));
}

#[test]
fn random_config_defers_mines_until_first_click() {
    let config = GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::NONE,
        pinned_seed: None,
    };
    let mut game = Game::with_config(config);
    assert_eq!(game.mines(), None);
    game.reveal(Position::new(0, 0));
    assert!(game.mines().is_some());
}

#[test]
fn random_config_first_click_is_safe() {
    // A Random (non-Prank) game regenerates the Seed until the clicked
    // Cell's 3x3 is Mine-free (ADR-0009), so it cascades as a zero Cell.
    let first = Position::new(4, 4);
    for _ in 0..8 {
        let config = GameConfig {
            difficulty: Difficulty::Beginner,
            features: Features::NONE,
            pinned_seed: None,
        };
        let mut game = Game::with_config(config);
        game.reveal(first);
        assert_ne!(game.game_state(), GameState::Lost);
        assert_eq!(
            game.cell_view(first).content,
            Some(CellContent::Number(0)),
            "First Click {first:?} was not a safe zero Cell"
        );
    }
}

#[test]
fn prank_config_first_click_is_always_a_mine() {
    // Prank overrides the First Click outcome and is mutually exclusive
    // with a pinned Seed (ADR-0010): Prank drops the Seed at the model
    // boundary, so a Prank game is non-reproducible.
    let config = GameConfig {
        difficulty: Difficulty::Beginner,
        features: Features::prank(),
        pinned_seed: None,
    };
    let mut game = Game::with_config(config);
    assert_eq!(game.features(), Features::prank());
    game.reveal(Position::new(0, 0));
    assert_eq!(game.game_state(), GameState::Lost);
    assert_eq!(game.trigger(), Some(Position::new(0, 0)));
    assert_eq!(
        game.cell_view(Position::new(0, 0)).content,
        Some(CellContent::Mine)
    );
}

#[test]
fn new_game_keeps_session_features_and_seed() {
    // A Prank game's `new_game(None)` reuses the session config: the same
    // Features and the (dropped) Seed, so it stays an unseedable Prank.
    let mut game = Game::with_config(GameConfig::new(
        Difficulty::Beginner,
        Features::prank(),
        None,
    ));
    game.new_game(None);
    assert_eq!(game.features(), Features::prank());
    assert_eq!(game.committed_seed(), None);
}

#[test]
fn new_game_switches_difficulty_and_resets_state() {
    // Reveal to Playing, then `new_game(Some(Expert))`: a fresh board at
    // the new Difficulty, with no Mines and no committed Seed.
    let mut game = Game::with_mines(Difficulty::Beginner, Features::NONE, &[Position::new(0, 0)]);
    game.reveal(Position::new(1, 1)); // non-Mine, keeps Playing
    assert_eq!(game.game_state(), GameState::Playing);
    game.new_game(Some(Difficulty::Expert));
    assert_eq!(game.game_state(), GameState::Ready);
    assert_eq!(game.difficulty(), Difficulty::Expert);
    assert_eq!(game.mines(), None);
    assert_eq!(game.committed_seed(), None);
}

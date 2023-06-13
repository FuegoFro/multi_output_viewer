use crate::vte_actions::{VteAction, VteActionParser};
use anyhow::{anyhow, Result};
use crossterm::cursor::{MoveDown, MoveRight, MoveToColumn, MoveUp};
use crossterm::queue;
use crossterm::style::{Color, Print, PrintStyledContent, Stylize};
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType::FromCursorDown;
use std::cmp::max;
use std::io::Write;
use std::time::Duration;

#[cfg(test)]
use mock_instant::Instant;

#[cfg(not(test))]
use std::time::Instant;

// TODO - Make this non-copy/clone?
#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub struct SecondaryOutputId(u32);

impl SecondaryOutputId {
    fn next_id(&mut self) -> Self {
        let id = self.0;
        self.0 += 1;
        SecondaryOutputId(id)
    }
}

struct SecondaryOutputState {
    id: SecondaryOutputId,
    title: String,
    start: Instant,
    expanded: bool,
    // If we don't end up using this, move the dep back to test-only
    buffer: vt100::Parser,
}

impl SecondaryOutputState {
    fn handle_bytes(&mut self, bytes: &[u8]) {
        self.buffer.process(bytes);
    }
}

pub struct State<'a, W: Write> {
    output: &'a mut W,

    primary_bytes: Vec<u8>,
    primary_output_parser: VteActionParser,
    /// Tracks how far from the left and bottom (respectively) of the output the cursor is.
    primary_output_final_cursor_offset: (u16, u16),

    secondary_output_max_lines: usize,
    secondary_output_next_id: SecondaryOutputId,
    secondary_output_reference_start_time: Instant,
    secondary_outputs: Vec<SecondaryOutputState>,
    secondary_output_selected_index: usize,

    previous_render_extra_lines: u16,
}

impl<'a, W: Write> State<'a, W> {
    pub fn new(output: &'a mut W, secondary_output_max_lines: usize) -> Self {
        Self {
            output,
            primary_bytes: Vec::new(),
            primary_output_parser: VteActionParser::new(),
            primary_output_final_cursor_offset: (0, 0),
            secondary_output_max_lines,
            secondary_output_next_id: Default::default(),
            secondary_output_reference_start_time: Instant::now(),
            secondary_outputs: Vec::new(),
            secondary_output_selected_index: 0,
            previous_render_extra_lines: 0,
        }
    }

    pub fn render(&mut self) -> Result<()> {
        // Reset if necessary
        let (mut x, mut y) = self.primary_output_final_cursor_offset;
        if self.previous_render_extra_lines > 0 {
            queue!(
                self.output,
                MoveToColumn(0),
                MoveUp(self.previous_render_extra_lines),
                Clear(FromCursorDown),
                MoveUp(y + 1),
                MoveRight(x),
            )?;
        }

        // Write out any pending primary bytes, update internal state tracking
        self.output.write_all(&self.primary_bytes)?;
        for action in self.primary_output_parser.parse_bytes(&self.primary_bytes) {
            match action {
                VteAction::Text(_) => x += 1,
                VteAction::Tab => x += 8 - (x % 8),
                VteAction::LineFeed => y = y.saturating_sub(1),
                VteAction::CarriageReturn => x = 0,
                VteAction::CursorUp(n) => y += n,
                VteAction::CursorDown(n) => y = y.saturating_sub(n),
                VteAction::CursorForward(n) => x += n,
                VteAction::CursorBackward(n) => x = x.saturating_sub(n),
                VteAction::CursorNextLine(n) => {
                    y = y.saturating_sub(n);
                    x = 0;
                }
                VteAction::CursorPreviousLine(n) => {
                    y += n;
                    x = 0;
                }
            }
        }
        self.primary_output_final_cursor_offset = (x, y);
        self.primary_bytes.clear();

        // Write out any secondary output
        self.previous_render_extra_lines = 0;
        if !self.secondary_outputs.is_empty() {
            queue!(self.output, MoveToColumn(0), MoveDown(y + 1),)?;
            let mut newline = || {
                self.previous_render_extra_lines += 1;
                Print("\r\n")
            };
            let now = Instant::now();
            for (i, secondary_state) in self.secondary_outputs.iter().enumerate() {
                let num_seconds = (now - secondary_state.start).as_secs();
                let cursor = if i == self.secondary_output_selected_index {
                    "> "
                } else {
                    "  "
                };
                let expanded_indicator = if secondary_state.expanded {
                    "+++".with(Color::Yellow)
                } else {
                    "---".with(Color::Green)
                };
                queue!(
                    self.output,
                    Print(cursor),
                    PrintStyledContent(expanded_indicator),
                    Print(format!(" {num_seconds: >3}s {}", secondary_state.title)),
                    newline()
                )?;
                if secondary_state.expanded {
                    let rows = secondary_state
                        .buffer
                        .screen()
                        .rows_formatted(0, u16::MAX)
                        .collect::<Vec<_>>();

                    let (cursor_row, cursor_col) =
                        secondary_state.buffer.screen().cursor_position();
                    // If we're at the beginning of the row, assume trailing newline, remove it
                    let cursor_row =
                        (cursor_row as usize).saturating_sub(if cursor_col == 0 { 1 } else { 0 });
                    // Probably don't technically need this since we're nominally not handling
                    // terminal control sequences
                    let last_non_empty_row = rows.iter().rposition(|row| !row.is_empty());
                    let end_idx = max(cursor_row, last_non_empty_row.unwrap_or(0));
                    if end_idx > 0 {
                        let start_idx = end_idx.saturating_sub(self.secondary_output_max_lines - 1);
                        for row in &rows[start_idx..=end_idx] {
                            self.output.write_all(row)?;
                            queue!(self.output, newline())?;
                        }
                    }
                }
            }
        }

        self.output.flush()?;
        Ok(())
    }

    pub fn handle_primary_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.primary_bytes.extend(bytes);
        self
    }

    pub fn new_secondary_output(&mut self, title: String) -> SecondaryOutputId {
        // Align start time to the reference start time so different outputs tick to the next
        // second together.
        let seconds_since_reference =
            (Instant::now() - self.secondary_output_reference_start_time).as_secs();
        let start = self.secondary_output_reference_start_time
            + Duration::from_secs(seconds_since_reference);
        let id = self.secondary_output_next_id.next_id();
        self.secondary_outputs.push(SecondaryOutputState {
            id,
            title,
            start,
            expanded: false,
            buffer: vt100::Parser::new(50, 50, self.secondary_output_max_lines * 3),
        });
        id
    }

    fn secondary_output_position(&self, id: &SecondaryOutputId) -> Result<usize> {
        self.secondary_outputs
            .iter()
            .position(|secondary_state| secondary_state.id == *id)
            // TODO - Use thiserror
            .ok_or_else(|| anyhow!("Invalid ID: {id:?}"))
    }

    pub fn remove_secondary_output(&mut self, id: SecondaryOutputId) -> Result<&mut Self> {
        // Note: Should use `drain_filter` once/if that's stabilized
        // https://github.com/rust-lang/rust/issues/43244
        let idx = self.secondary_output_position(&id)?;
        self.secondary_outputs.remove(idx);
        if self.secondary_output_selected_index > idx {
            self.secondary_output_selected_index -= 1;
        }
        Ok(self)
    }

    pub fn handle_secondary_bytes(
        &mut self,
        id: &SecondaryOutputId,
        bytes: &[u8],
    ) -> Result<&mut Self> {
        let idx = self.secondary_output_position(id)?;
        self.secondary_outputs[idx].handle_bytes(bytes);
        Ok(self)
    }

    pub fn move_cursor_down(&mut self) -> &mut Self {
        self.secondary_output_selected_index =
            (self.secondary_output_selected_index + 1).min(self.secondary_outputs.len() - 1);
        self
    }

    pub fn move_cursor_up(&mut self) -> &mut Self {
        self.secondary_output_selected_index =
            self.secondary_output_selected_index.saturating_sub(1);
        self
    }

    pub fn toggle_current_selection_expanded(&mut self) -> &mut Self {
        if let Some(secondary_state) = self
            .secondary_outputs
            .get_mut(self.secondary_output_selected_index)
        {
            secondary_state.expanded = !secondary_state.expanded;
        }
        self
    }
}

#[cfg(test)]
mod test {
    use crate::state::State;
    #[allow(unused_imports)] // IntelliJ gets confused here
    use insta::{assert_snapshot, with_settings};

    const TEST_SECONDARY_OUTPUT_MAX_LINES: usize = 3;

    macro_rules! assert_state_output {
        ($f:expr) => {
            let output = get_state_output($f);
            with_settings!({
                description => stringify!($f),
                omit_expression => true
            }, {
                assert_snapshot!(format!("# Rendered:\n```\n{}\n```\n\n\n# Raw:\n```\n{}\n```", rasterize_output(&output), output));
            });

        };
    }

    fn rasterize_output(output: &str) -> String {
        let mut parser = vt100::Parser::new(50, 50, 50);
        parser.process(output.as_bytes());
        parser.screen().contents()
    }

    fn get_state_output(f: impl FnOnce(&mut State<Vec<u8>>)) -> String {
        let mut output: Vec<u8> = Vec::new();
        {
            let mut state = State::new(&mut output, TEST_SECONDARY_OUTPUT_MAX_LINES);
            f(&mut state);
        }
        String::from_utf8(output).unwrap()
    }

    mod primary_output {
        use super::*;

        #[test]
        fn buffers_bytes() {
            assert_state_output!(|state| {
                state.handle_primary_bytes("should not render".as_bytes());
            });
        }

        #[test]
        fn passes_through_bytes() {
            assert_state_output!(|state| {
                state
                    .handle_primary_bytes("my test string\r\nhello hi".as_bytes())
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn only_renders_bytes_once() {
            assert_state_output!(|state| {
                state.handle_primary_bytes("no repeat ".as_bytes());
                state.render().unwrap();
                state.render().unwrap();
            });
        }

        #[test]
        fn draws_secondary_output_after_content_and_restores_cursor_position() {
            assert_state_output!(|state| {
                state.new_secondary_output("test secondary output".into());
                state
                    .handle_primary_bytes("abc\r\ndef\r\nghi\x1b[3D\x1b[1A\x1b[3C".as_bytes())
                    .render()
                    .unwrap();

                state
                    .handle_primary_bytes("123".as_bytes())
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn clears_secondary_output() {
            assert_state_output!(|state| {
                state.new_secondary_output("test secondary output".into());
                state
                    .handle_primary_bytes("abc".as_bytes())
                    .render()
                    .unwrap();

                state
                    .handle_primary_bytes("def\r\n123".as_bytes())
                    .render()
                    .unwrap();
            });
        }
    }

    mod secondary_output {
        use super::*;
        use mock_instant::MockClock;
        use std::time::Duration;

        #[test]
        fn shows_titles_and_durations() {
            assert_state_output!(|state| {
                state.new_secondary_output("first title".into());
                MockClock::advance(Duration::from_secs(1));
                state.new_secondary_output("second title".into());
                MockClock::advance(Duration::from_secs(1));
                state.render().unwrap();
            });
        }

        #[test]
        fn durations_change_at_same_time() {
            assert_state_output!(|state| {
                // Offset from any instant taken when the state was created.
                MockClock::advance(Duration::from_millis(250));
                state.new_secondary_output("first title".into());

                // Offset by a non-whole-number of seconds
                MockClock::advance(Duration::from_millis(500));
                state.new_secondary_output("second title".into());

                // Wait until just before the times should tick over; assumes they tick over at whole
                // numbers of seconds from when the state was initially created.
                MockClock::advance(Duration::from_millis(249));
                state.render().unwrap();

                // Have it tick over to the next second
                MockClock::advance(Duration::from_millis(1));
                state.render().unwrap();
            });
        }

        #[test]
        fn shows_cursor_at_selected_index() {
            assert_state_output!(|state| {
                state.new_secondary_output("one".into());
                state.new_secondary_output("two".into());
                state.new_secondary_output("three".into());
                state.new_secondary_output("four".into());
                state
                    .move_cursor_down()
                    .move_cursor_down()
                    .move_cursor_down()
                    .move_cursor_up()
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn clamps_cursor_down() {
            assert_state_output!(|state| {
                state.new_secondary_output("one".into());
                state.new_secondary_output("two".into());
                state
                    .move_cursor_down()
                    .move_cursor_down()
                    .move_cursor_down()
                    .move_cursor_down()
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn clamps_cursor_up() {
            assert_state_output!(|state| {
                state.new_secondary_output("one".into());
                state.new_secondary_output("two".into());
                state
                    .move_cursor_down()
                    .move_cursor_down()
                    .move_cursor_up()
                    .move_cursor_up()
                    .move_cursor_up()
                    .move_cursor_up()
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn changes_prefix_when_expanded() {
            assert_state_output!(|state| {
                // No-op if there's no outputs
                state.toggle_current_selection_expanded();
                state.new_secondary_output("one".into());
                state.new_secondary_output("two".into());
                state.new_secondary_output("three".into());
                state
                    // Expand "one"
                    .toggle_current_selection_expanded()
                    // Expand "two"
                    .move_cursor_down()
                    .toggle_current_selection_expanded()
                    // Collapse "one"
                    .move_cursor_up()
                    .toggle_current_selection_expanded()
                    // Expand "three"
                    .move_cursor_down()
                    .move_cursor_down()
                    .toggle_current_selection_expanded()
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn removing_output_preserves_order_and_selection() {
            assert_state_output!(|state| {
                state.new_secondary_output("one".into());
                let two_id = state.new_secondary_output("two".into());
                state.new_secondary_output("three".into());
                state
                    // Put the cursor on the item to be removed
                    .move_cursor_down()
                    .remove_secondary_output(two_id)
                    .unwrap()
                    .render()
                    .unwrap();

                // Can't call a second time, id is now invalid
                assert!(state.remove_secondary_output(two_id).is_err());
            });
        }

        #[test]
        fn removing_output_moves_selection_down() {
            assert_state_output!(|state| {
                state.new_secondary_output("one".into());
                let two_id = state.new_secondary_output("two".into());
                state.new_secondary_output("three".into());
                state
                    .move_cursor_down()
                    .move_cursor_down()
                    .remove_secondary_output(two_id)
                    .unwrap()
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn shows_lines_when_expanded() {
            assert_state_output!(|state| {
                let id = state.new_secondary_output("out".into());
                state
                    .handle_secondary_bytes(&id, b"a\r\nb\r\n")
                    .unwrap()
                    .render()
                    .unwrap();
                state.toggle_current_selection_expanded().render().unwrap();

                // Can't send bytes to unknown process
                state.remove_secondary_output(id).unwrap();
                assert!(state.handle_secondary_bytes(&id, b"").is_err());
            });
        }

        #[test]
        fn only_shows_most_recent_lines() {
            assert_state_output!(|state| {
                let id = state.new_secondary_output("out".into());
                state
                    .handle_secondary_bytes(&id, b"a\r\nb\r\nc\r\nd\r\n")
                    .unwrap()
                    .toggle_current_selection_expanded()
                    .render()
                    .unwrap();
            });
        }

        #[test]
        fn handles_trailing_blank_lines() {
            assert_state_output!(|state| {
                let id = state.new_secondary_output("with trailing".into());
                state.new_secondary_output("after".into());
                state
                    .handle_secondary_bytes(&id, b"a\r\n")
                    .unwrap()
                    .toggle_current_selection_expanded()
                    .render()
                    .unwrap();
                state
                    .handle_secondary_bytes(&id, b"\r\n\r\n")
                    .unwrap()
                    .render()
                    .unwrap();
            });
        }
    }

    /*
    Use thiserror
    Better secondary output columns
    Handle primary output different styling (reset style)
    Hide/show cursor, enter/exit raw mode when have/don't have secondary output
    Handle mode changes, eg for password entry (unclear if in state)

    Assuming secondary output isn't a TTY, don't need to handle terminal output
    - Don't handle secondary output cursor moving up
    - Don't handle secondary output different styling (reset style)

    Differences from vt100 for primary output:
    - Tracks cursor position (from end) and state, doesn't store cell content
    - For state, maybe just 1x1 vt100 grid?
    - Position info is complicated, we're being naive right now
    Differences from vt100 for secondary output:
    - Infinite column width, wrap on render
    */
}

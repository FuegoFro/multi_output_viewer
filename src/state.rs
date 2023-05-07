use crate::vte_actions::{VteAction, VteActionParser};
use anyhow::Result;
use crossterm::cursor::{MoveDown, MoveRight, MoveToColumn, MoveUp};
use crossterm::queue;
use crossterm::style::{Color, Print, PrintStyledContent, Stylize};
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType::FromCursorDown;
use std::io::Write;
use std::time::Duration;

#[cfg(test)]
use mock_instant::Instant;

#[cfg(not(test))]
use std::time::Instant;

pub struct State<'a, W: Write> {
    output: &'a mut W,

    primary_bytes: Vec<u8>,
    primary_output_parser: VteActionParser,
    /// Tracks how far from the left and bottom (respectively) of the output the cursor is.
    primary_output_final_cursor_offset: (u16, u16),

    secondary_output_reference_start_time: Instant,
    secondary_outputs: Vec<(String, Instant)>,
    secondary_output_selected_index: usize,

    previous_render_extra_lines: u16,
}

impl<'a, W: Write> State<'a, W> {
    pub fn new(output: &'a mut W) -> Self {
        Self {
            output,
            primary_bytes: Vec::new(),
            primary_output_parser: VteActionParser::new(),
            primary_output_final_cursor_offset: (0, 0),
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
            for (i, (title, start)) in self.secondary_outputs.iter().enumerate() {
                let num_seconds = (now - *start).as_secs();
                let cursor = if i == self.secondary_output_selected_index {
                    "> "
                } else {
                    "  "
                };
                queue!(
                    self.output,
                    Print(cursor),
                    PrintStyledContent("+++".with(Color::Yellow)),
                    Print(format!(" {num_seconds: >3}s {title}")),
                    newline()
                )?;
            }
        }

        self.output.flush()?;
        Ok(())
    }

    pub fn handle_primary_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.primary_bytes.extend(bytes);
        self
    }

    pub fn new_secondary_output(&mut self, title: String) -> &mut Self {
        // Align start time to the reference start time so different outputs tick to the next
        // second together.
        let seconds_since_reference =
            (Instant::now() - self.secondary_output_reference_start_time).as_secs();
        let start = self.secondary_output_reference_start_time
            + Duration::from_secs(seconds_since_reference);
        self.secondary_outputs.push((title, start));
        self
    }
}

#[cfg(test)]
mod test {
    use crate::state::State;
    use insta::assert_snapshot;
    use insta::with_settings;
    use mock_instant::MockClock;
    use std::time::Duration;

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
            let mut state = State::new(&mut output);
            f(&mut state);
        }
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn buffers_primary_bytes() {
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
    fn only_renders_primary_bytes_once() {
        assert_state_output!(|state| {
            state.handle_primary_bytes("no repeat ".as_bytes());
            state.render().unwrap();
            state.render().unwrap();
        });
    }

    #[test]
    fn draws_secondary_output_after_content_and_restores_cursor_position() {
        assert_state_output!(|state| {
            state
                .new_secondary_output("test secondary output".into())
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
            state
                .new_secondary_output("test secondary output".into())
                .handle_primary_bytes("abc".as_bytes())
                .render()
                .unwrap();

            state
                .handle_primary_bytes("def\r\n123".as_bytes())
                .render()
                .unwrap();
        });
    }

    #[test]
    fn shows_secondary_output_titles_and_durations() {
        assert_state_output!(|state| {
            state.new_secondary_output("first title".into());
            MockClock::advance(Duration::from_secs(1));
            state.new_secondary_output("second title".into());
            MockClock::advance(Duration::from_secs(1));
            state.render().unwrap();
        });
    }

    #[test]
    fn secondary_output_durations_change_at_same_time() {
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

    /*
    Show secondary titles and durations
    Change prefix when expanded
    Add to end, preserve order when one finishes
    Show most recent N lines
    Handle cursor moving up in secondary output
    Handle different styling of primary output (reset style)
    */
}

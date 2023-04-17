use crate::vte_actions::{VteAction, VteActionParser};
use anyhow::Result;
use crossterm::cursor::{MoveDown, MoveRight, MoveToColumn, MoveUp};
use crossterm::queue;
use crossterm::style::Print;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType::FromCursorDown;
use std::io::Write;

pub struct State<'a, W: Write> {
    output: &'a mut W,

    primary_bytes: Vec<u8>,
    primary_output_parser: VteActionParser,
    /// Tracks how far from the left and bottom (respectively) of the output the cursor is.
    primary_output_final_cursor_offset: (u16, u16),

    // TODO - Temp, to test primary output while secondary is active
    has_secondary_output: bool,

    previous_render_extra_lines: u16,
}

impl<'a, W: Write> State<'a, W> {
    pub fn new(output: &'a mut W) -> Self {
        Self {
            output,
            primary_bytes: Vec::new(),
            primary_output_parser: VteActionParser::new(),
            primary_output_final_cursor_offset: (0, 0),
            has_secondary_output: false,
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
        dbg!((x, y));
        self.primary_output_final_cursor_offset = (x, y);
        self.primary_bytes.clear();

        // Write out any secondary output
        if self.has_secondary_output {
            queue!(
                self.output,
                MoveToColumn(0),
                MoveDown(y + 1),
                Print("<secondary output placeholder>\r\n"),
            )?;
            self.previous_render_extra_lines = 1;
        } else {
            self.previous_render_extra_lines = 0;
        }

        self.output.flush()?;
        Ok(())
    }

    pub fn handle_primary_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.primary_bytes.extend(bytes);
        self
    }

    pub fn new_secondary_output(&mut self) -> &mut Self {
        self.has_secondary_output = true;
        self
    }
}

#[cfg(test)]
mod test {
    use crate::state::State;
    use insta::assert_snapshot;
    use insta::with_settings;

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
            state.handle_primary_bytes("some data".as_bytes());
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
                .new_secondary_output()
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
                .new_secondary_output()
                .handle_primary_bytes("abc".as_bytes())
                .render()
                .unwrap();

            state
                .handle_primary_bytes("def\r\n123".as_bytes())
                .render()
                .unwrap();
        });
    }

    /*
    Handle different styling of primary output
    */
}

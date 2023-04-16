/*
No-output secondary output
Handle move-up primary
Handle non-new-line primary
*/
use anyhow::Result;
use std::io::Write;

struct State<'a, W: Write> {
    output: &'a mut W,
    primary_bytes: Vec<u8>,
}

impl<'a, W: Write> State<'a, W> {
    fn new(output: &'a mut W) -> Self {
        Self {
            output,
            primary_bytes: Vec::new(),
        }
    }

    fn render(&mut self) -> Result<()> {
        self.output.write_all(&self.primary_bytes)?;
        self.primary_bytes.clear();
        Ok(())
    }

    fn handle_primary_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.primary_bytes.extend(bytes);
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
                description => stringify!($f)
            }, {
                assert_snapshot!(output);
            });

        };
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
                .handle_primary_bytes("my test string\nhello hi".as_bytes())
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
}

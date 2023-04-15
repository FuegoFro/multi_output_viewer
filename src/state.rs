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

    fn new_secondary_output(&mut self) -> &mut Self {
        self
    }
}

#[cfg(test)]
mod test {
    use crate::state::State;

    #[test]
    fn buffers_primary_bytes() {
        let mut output: Vec<u8> = Vec::new();
        State::new(&mut output).handle_primary_bytes("some data".as_bytes());
        assert_eq!(String::from_utf8(output).unwrap(), "");
    }

    #[test]
    fn passes_through_bytes() {
        let mut output: Vec<u8> = Vec::new();
        let data = "my test string\nhello hi";
        State::new(&mut output)
            .handle_primary_bytes(data.as_bytes())
            .render()
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), data);
    }

    #[test]
    fn only_renders_primary_bytes_once() {
        let mut output: Vec<u8> = Vec::new();
        let data = "no repeat ";
        {
            let mut state = State::new(&mut output);
            state.handle_primary_bytes(data.as_bytes());
            state.render().unwrap();
            state.render().unwrap();
        }
        assert_eq!(String::from_utf8(output).unwrap(), data);
    }
}

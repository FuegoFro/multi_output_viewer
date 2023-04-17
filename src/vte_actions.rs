use crate::vte_actions::VteAction::{
    CarriageReturn, CursorBackward, CursorDown, CursorForward, CursorNextLine, CursorPreviousLine,
    CursorUp, LineFeed, Tab, Text,
};
use vte::{Params, Parser, Perform};

/// The semantic actions that can be taken as a result of bytes sent to the terminal.
// TODO - Implement more actions to be complete here (as needed?)
#[derive(Debug)]
pub enum VteAction {
    Text(char),
    Tab,
    LineFeed,
    CarriageReturn,
    CursorUp(u16),
    CursorDown(u16),
    CursorForward(u16),
    CursorBackward(u16),
    CursorNextLine(u16),
    CursorPreviousLine(u16),
}

/// A wrapper over [Parser] and [Perform] which takes bytes in and exposes an iterator
/// of semantic actions. Stops short of actually tracking the rendered output as a grid
/// of cells.
pub struct VteActionParser {
    parser: Parser,
}

impl VteActionParser {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    pub fn parse_bytes(&mut self, bytes: &[u8]) -> Vec<VteAction> {
        let mut performer = Performer::new();
        for byte in bytes {
            self.parser.advance(&mut performer, *byte)
        }
        performer.actions
    }
}

// Private struct to hide this implementation detail
struct Performer {
    actions: Vec<VteAction>,
}

impl Performer {
    fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }
}

// Implementation largely inspired by vt100-rust:
// https://github.com/doy/vt100-rust/blob/main/src/perform.rs
impl Perform for Performer {
    fn print(&mut self, c: char) {
        self.actions.push(Text(c))
    }

    fn execute(&mut self, byte: u8) {
        let action = match byte {
            9 => Tab,
            10 => LineFeed,
            13 => CarriageReturn,
            _ => return,
        };
        self.actions.push(action);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, c: char) {
        if intermediates.is_empty() {
            let action = match c {
                'A' => CursorUp(params.canonicalize_1(1)),
                'B' => CursorDown(params.canonicalize_1(1)),
                'C' => CursorForward(params.canonicalize_1(1)),
                'D' => CursorBackward(params.canonicalize_1(1)),
                'E' => CursorNextLine(params.canonicalize_1(1)),
                'F' => CursorPreviousLine(params.canonicalize_1(1)),
                _ => return,
            };
            self.actions.push(action);
        }
    }
}

trait ParamsCanonicalize {
    fn canonicalize_1(&self, default: u16) -> u16;
}

impl ParamsCanonicalize for Params {
    fn canonicalize_1(&self, default: u16) -> u16 {
        self.iter()
            .next()
            .and_then(|x| x.first().copied())
            .filter(|x| *x != 0)
            .unwrap_or(default)
    }
}

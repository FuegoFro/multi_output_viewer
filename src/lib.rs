/*
Main output tee's to VTE to determine cursor position
Secondary outputs feed through VTE to update buffer of last N lines

Render:
    Record final position (distance from bottom, distance from left)
    move to first unoccupied line
    render secondary outputs
    move back to final position

State update thread.
Also handle render?
Maybe have some sort of COW for state and send cheap copy off to a render thread?

Testability is important, need to think that through
    Test logging???
    Ability to manually specify output???

State first
Then IO
    PTY
        Mirror termio changes somehow (just, uh, ignore Windows for now?)
        Output sent to state
    stdin
        forwarding without secondary output, processing with
    Secondary output server
        Handle new connections, send bytes to state


*/
mod state;
mod vte_actions;

pub use state::State;

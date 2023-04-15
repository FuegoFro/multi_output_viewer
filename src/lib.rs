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
    Probably inputs -> render -> output
    Test logging???
    Assert against bytes out? Visual output somehow?
    Ideally fixtures. Maybe captures both the bytes and the visual representation (using VTE?)
    Ability to manually specify output???

State first

*/
mod state;

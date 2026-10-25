#[expect(
    dead_code,
    reason = "the TUI that reads sessions comes in a later change"
)]
mod state;
#[expect(
    dead_code,
    reason = "the TUI that lists and focuses panes comes in a later change"
)]
mod terminal;

fn main() {
    println!("Hello, world!");
}

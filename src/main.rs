#[expect(
    dead_code,
    reason = "the TUI that reads sessions comes in a later change"
)]
mod state;

fn main() {
    println!("Hello, world!");
}

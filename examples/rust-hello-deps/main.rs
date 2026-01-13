// Example demonstrating external Rust crate usage in Buck2
//
// Uses the `itoa` crate for efficient integer-to-string conversion.
// Dependencies are managed via rust-deps.toml and the rustdeps cell.

fn main() {
    let mut buffer = itoa::Buffer::new();
    let answer = 42;
    let printed = buffer.format(answer);
    println!("Hello! The answer is: {}", printed);
}

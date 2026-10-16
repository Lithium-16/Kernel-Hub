#![allow(dead_code)]
mod config;
mod protocol;

fn main() {
    println!("petlink {}", env!("CARGO_PKG_VERSION"));
}

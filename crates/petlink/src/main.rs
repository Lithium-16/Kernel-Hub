#![allow(dead_code)]
mod config;
mod protocol;
mod sprites;

fn main() {
    println!("petlink {}", env!("CARGO_PKG_VERSION"));
}

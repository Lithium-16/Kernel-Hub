#![allow(dead_code)]
mod config;
mod pet;
mod protocol;
mod sprites;
mod world;

fn main() {
    println!("petlink {}", env!("CARGO_PKG_VERSION"));
}

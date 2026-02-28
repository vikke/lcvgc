mod ast;
mod cli;
mod error;
mod parser;

use clap::Parser;
use cli::Cli;

fn main() {
    let _cli = Cli::parse();
    println!("lcvgc - Live CV Gate Coder");
}

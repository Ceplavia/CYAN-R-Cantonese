mod database;
mod generator;

use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
        let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let output_path = parse_output_path()?.unwrap_or_else(|| manifest_directory.join("../rcantonese/ime.sqlite3"));

        generator::generate(&manifest_directory.join("res"), &output_path)?;
        println!("Created {}", output_path.canonicalize()?.display());
        Ok(())
}

fn parse_output_path() -> Result<Option<PathBuf>, Box<dyn Error>> {
        let mut arguments = env::args_os().skip(1);
        let Some(argument) = arguments.next() else {
                return Ok(None);
        };
        if argument == "--help" || argument == "-h" {
                println!("Usage: cargo run --release [-- <output-path>]");
                std::process::exit(0);
        }
        if arguments.next().is_some() {
                return Err("expected at most one output path".into());
        }
        Ok(Some(argument.into()))
}

use anyhow::Result;
use clap::Parser;
use pdftp_lib::Processor;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::BufWriter;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(short, long)]
    input_filename: String,
    #[arg(short, long)]
    output_filename: Option<String>,
    #[arg(short, long, value_name = "KEY=VALUE", value_parser = parse_key_val::<String, String>)]
    variable: Option<Vec<(String, String)>>,
}

/// Parse a single key-value pair
fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.output_filename {
        Some(output_filename) => {
            let output = BufWriter::new(File::create(output_filename)?);
            let mut processor = Processor::new(&args.input_filename, output, args.variable);
            processor.execute()?;
        }
        _ => {
            let output = BufWriter::new(io::stdout());
            let mut processor = Processor::new(&args.input_filename, output, args.variable);
            processor.execute()?;
        }
    };
    Ok(())
}

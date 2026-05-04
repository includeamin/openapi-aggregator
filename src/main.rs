use std::path::PathBuf;

use clap::Parser;
use openapi_aggregator::{aggregate_from_file, OutputFormat};

#[derive(Parser)]
#[command(name = "openapi-aggregator")]
#[command(version)]
#[command(about = "Aggregate and merge OpenAPI specifications from multiple sources")]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "openapi-aggregator.yaml")]
    config: PathBuf,

    /// Output file path (stdout if omitted)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format (overrides config)
    #[arg(short, long, value_parser = parse_format)]
    format: Option<OutputFormat>,
}

fn parse_format(s: &str) -> Result<OutputFormat, String> {
    match s.to_ascii_lowercase().as_str() {
        "yaml" | "yml" => Ok(OutputFormat::Yaml),
        "json" => Ok(OutputFormat::Json),
        other => Err(format!(
            "unknown format '{other}', expected 'yaml' or 'json'"
        )),
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let merged = aggregate_from_file(&cli.config).await?;

    let format = cli.format.unwrap_or(OutputFormat::Yaml);
    let text = match format {
        OutputFormat::Yaml => serde_yaml::to_string(&merged)?,
        OutputFormat::Json => serde_json::to_string_pretty(&merged)?,
    };

    match cli.output {
        Some(path) => {
            std::fs::write(&path, &text)?;
            eprintln!("wrote merged spec to {}", path.display());
        }
        None => print!("{text}"),
    }

    Ok(())
}

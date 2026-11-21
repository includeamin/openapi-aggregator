mod check;

use std::path::PathBuf;

use clap::Parser;
use openapi_aggregator::{aggregate_with_options, load_config, AggregateOptions, OutputFormat};

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

    /// Let external `$ref`s read files outside the config directory
    #[arg(long)]
    allow_outside_config_dir: bool,

    /// Validate the merged spec and exit non-zero on problems; with --output, also fail
    /// when that file is out of date. Writes nothing.
    #[arg(long)]
    check: bool,

    /// With --check, treat warnings as failures
    #[arg(long, requires = "check")]
    deny_warnings: bool,
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

    match run(cli).await {
        Ok(outcome) => std::process::exit(outcome.exit_code()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

async fn run(cli: Cli) -> Result<check::Outcome, Box<dyn std::error::Error>> {
    let config = load_config(&cli.config)?;
    let options = AggregateOptions {
        allow_outside_config_dir: cli.allow_outside_config_dir,
        config_dir: cli.config.parent().map(PathBuf::from),
    };
    let report = aggregate_with_options(&config, &options).await?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }

    if cli.check {
        let mut outcome = check::run_validation(&report, cli.deny_warnings);
        if let Some(path) = &cli.output {
            let format = cli.format.unwrap_or(config.output.format);
            let stale = check::check_output(path, &report.spec, &format);
            // A validation failure (exit 1) outranks a stale file (exit 2).
            if outcome == check::Outcome::Passed {
                outcome = stale;
            }
        }
        return Ok(outcome);
    }

    let format = cli.format.unwrap_or(config.output.format);
    let text = match format {
        OutputFormat::Yaml => serde_yaml::to_string(&report.spec)?,
        OutputFormat::Json => serde_json::to_string_pretty(&report.spec)? + "\n",
    };

    match cli.output {
        Some(path) => {
            std::fs::write(&path, &text)?;
            eprintln!("wrote merged spec to {}", path.display());
        }
        None => print!("{text}"),
    }

    Ok(check::Outcome::Passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_allow_outside_config_dir_flag() {
        let cli =
            Cli::try_parse_from(["openapi-aggregator", "--allow-outside-config-dir"]).unwrap();
        assert!(cli.allow_outside_config_dir);
        let cli = Cli::try_parse_from(["openapi-aggregator"]).unwrap();
        assert!(!cli.allow_outside_config_dir);
    }
}

use clap::Parser;

#[path = "../src/cli.rs"]
mod cli;

use cli::{Cli, Commands};

#[test]
fn reembed_cli_defaults_to_bounded_live_migration() {
    let cli = Cli::parse_from(["mag", "re-embed"]);

    match cli.command {
        Commands::ReEmbed {
            batch_size,
            dry_run,
        } => {
            assert_eq!(batch_size, 100);
            assert!(!dry_run);
        }
        _ => panic!("expected re-embed command"),
    }
}

#[test]
fn reembed_cli_accepts_batch_size_and_dry_run() {
    let cli = Cli::parse_from(["mag", "re-embed", "--batch-size", "25", "--dry-run"]);

    match cli.command {
        Commands::ReEmbed {
            batch_size,
            dry_run,
        } => {
            assert_eq!(batch_size, 25);
            assert!(dry_run);
        }
        _ => panic!("expected re-embed command"),
    }
}

#[test]
fn reembed_cli_rejects_zero_batch_size() {
    let error = Cli::try_parse_from(["mag", "re-embed", "--batch-size", "0"])
        .expect_err("zero batch size must be rejected by clap");

    assert!(
        error.to_string().contains("batch-size"),
        "unexpected clap error: {error}"
    );
}

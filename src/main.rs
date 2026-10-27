mod bluetooth;
mod reading;

use std::{io::Write, process::ExitCode, time::Duration};

use anyhow::{Result, anyhow};
use bluer::Address;
use bluetooth::Bluetooth;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    version,
    about,
    after_help = "Linux with BlueZ is required. No background service is installed."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Bluetooth adapter to use
    #[arg(long, global = true, default_value = "hci0")]
    adapter: String,

    /// Cuff address (required if more than one UA-651BLE matches)
    #[arg(long, global = true)]
    address: Option<Address>,

    /// Maximum time for the command, in seconds
    #[arg(long, global = true, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86400))]
    timeout: u64,
}

#[derive(Subcommand)]
enum Command {
    /// Pair a new cuff and set its clock. Existing pairings are left unchanged.
    #[command(
        long_about = "Pair a new cuff and set its clock. Hold START until Pr appears.\n\
        If the cuff is already paired, exit without connecting or changing its clock."
    )]
    Pair,
    /// Receive one transfer, then exit. Start this before taking a measurement.
    #[command(
        long_about = "Receive one transfer from a paired cuff, then exit when it disconnects.\n\
        Start this command before taking a measurement. All received readings are printed,\n\
        including any queued readings. Nothing is saved automatically. Run again for the next transfer."
    )]
    Measure {
        /// Output one JSON object per reading, one object per line
        #[arg(long)]
        json: bool,
    },
}

fn print_reading(reading: &reading::Reading, json: bool) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer(&mut stdout, reading)?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "{reading}")?;
    }
    stdout.flush()?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut failure = ExitCode::FAILURE;
    let result = async {
        let mut bluetooth = Bluetooth::new(&cli.adapter).await?;
        let operation = async {
            match cli.command {
                Command::Pair => bluetooth.pair(cli.address).await,
                Command::Measure { json } => {
                    bluetooth
                        .measure(cli.address, |r| print_reading(r, json))
                        .await
                }
            }
        };
        let result = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(cli.timeout), operation) =>
                result.unwrap_or_else(|_| Err(anyhow!("Timed out after {} seconds", cli.timeout))),
            signal = cancelled() => {
                failure = ExitCode::from(130);
                signal.map_err(anyhow::Error::from).and(Err(anyhow!("Interrupted")))
            }
        };
        let cleanup = bluetooth.disconnect().await;
        if let Err(error) = &cleanup {
            eprintln!("Cleanup failed: {error:#}");
        }
        result.and(cleanup)
    }
    .await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            failure
        }
    }
}

async fn cancelled() -> std::io::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

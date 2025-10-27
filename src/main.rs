use anyhow::Result;
use clap::Parser;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::{Semaphore, mpsc};

fn parse_status_codes(s: &str) -> Result<HashSet<u16>, String> {
    s.split(',')
        .map(|s| s.trim().parse::<u16>())
        .collect::<Result<HashSet<u16>, _>>()
        .map_err(|e| format!("Invalid status code: {}", e))
}

use dircrab::start_scan;

fn wordlist_path_parser(s: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(s);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("Wordlist file not found: {}", s))
    }
}

fn parse_concurrency(s: &str) -> Result<usize, String> {
    let concurrency = s
        .parse::<usize>()
        .map_err(|e| format!("Invalid concurrency value: {}", e))?;
    if concurrency == 0 {
        Err("Concurrency must be at least 1.".to_string())
    } else {
        Ok(concurrency)
    }
}

#[derive(Parser, Debug)]
#[clap(author = "Neosb", version, about = "A high-speed web content scanner")]
struct Cli {
    /// The base URL to scan (e.g., `http://testsite.com`)
    #[arg(short, long)]
    url: url::Url,

    /// The path to the text file (e.g., `~/wordlists/common.txt`)
    #[arg(short, long, value_parser = wordlist_path_parser)]
    wordlist: PathBuf,

    /// Maximum number of concurrent requests
    #[arg(short, long, default_value = "2", value_parser = parse_concurrency)]
    concurrency: usize,

    /// Exclude the following HTTP status codes (comma-separated)
    #[arg(long, value_parser = parse_status_codes)]
    exclude_status: Option<HashSet<u16>>,

    /// Include only the following HTTP status codes (comma-separated)
    #[arg(long, value_parser = parse_status_codes)]
    include_status: Option<HashSet<u16>>,

    /// Maximum recursion depth for directory scanning (0 for infinite, 1 for no recursion)
    #[arg(long, default_value = "1")]
    depth: usize,

    /// Optional delay between requests in milliseconds
    #[arg(long)]
    delay: Option<u64>,
}

async fn read_wordlist(path: PathBuf) -> Result<Vec<String>, io::Error> {
    let file = File::open(&path).await?;
    let reader = BufReader::new(file);
    let mut words = Vec::new();
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        if !line.trim().is_empty() {
            words.push(line.trim().to_string());
        }
    }
    Ok(words)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("# URL: {}", cli.url);
    println!("# Wordlist: {}", cli.wordlist.display());

    let words = read_wordlist(cli.wordlist).await?;
    println!("# Read {} words from wordlist.", words.len());

    let client = Client::builder()
        .timeout(Duration::from_secs(10)) // 10 second timeout for requests
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let (tx, mut rx) = mpsc::channel::<String>(100);
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));

    // Spawn a task to receive and print messages
    let printer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("{}", msg);
        }
    });

    start_scan(
        client,
        cli.url,
        words,
        tx,
        semaphore,
        cli.exclude_status,
        cli.include_status,
        cli.depth,
        cli.delay,
    )
    .await?;

    // Wait for the printer task to finish
    printer_handle.await?;

    Ok(())
}

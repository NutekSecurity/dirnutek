use clap::Parser;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};
use std::sync::Arc;

mod scanner;

fn wordlist_path_parser(s: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(s);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("Wordlist file not found: {}", s))
    }
}

#[derive(Parser, Debug)]
#[clap(
    author = "Neosb",
    version,
    about = "A high-speed web content scanner"
)]
struct Cli {
    /// The base URL to scan (e.g., `http://testsite.com`)
    #[arg(short, long)]
    url: url::Url,

    /// The path to the text file (e.g., `~/wordlists/common.txt`)
    #[arg(short, long, value_parser = wordlist_path_parser)]
    wordlist: PathBuf,

    /// Maximum number of concurrent requests
    #[arg(short, long, default_value = "50")]
    concurrency: usize,
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

    println!("URL: {}", cli.url);
    println!("Wordlist: {}", cli.wordlist.display());

    let words = read_wordlist(cli.wordlist).await?;
    println!("Read {} words from wordlist.", words.len());

    let client = Client::builder()
        .timeout(Duration::from_secs(10)) // 10 second timeout for requests
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let (tx, mut rx) = mpsc::channel::<String>(100);
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));

    let mut handles = Vec::new();

    for word in words {
        let client = client.clone();
        let base_url = cli.url.clone();
        let tx_clone = tx.clone();
        let semaphore_clone = semaphore.clone();
        let handle = tokio::spawn(async move {
            let _permit = semaphore_clone.acquire().await.expect("Failed to acquire semaphore permit");
            scanner::scan_url(&client, &base_url, &word, tx_clone).await
        });
        handles.push(handle);
    }

    // Drop the original tx to signal the receiver that no more messages will be sent
    drop(tx);

    // Spawn a task to receive and print messages
    let printer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("{}", msg);
        }
    });

    for handle in handles {
        handle.await??;
    }

    // Wait for the printer task to finish
    printer_handle.await?;

    Ok(())
}

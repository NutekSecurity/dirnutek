use anyhow::Result;
use clap::Parser;
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::{Mutex, Semaphore, mpsc};

fn parse_status_codes(s: &str) -> Result<HashSet<u16>, String> {
    s.split(',')
        .map(|s| s.trim().parse::<u16>())
        .collect::<Result<HashSet<u16>, _>>()
        .map_err(|e| format!("Invalid status code: {}", e))
}

use dircrab::{HttpMethod, start_scan};

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
#[clap(
    author,
    version,
    about = "A high-speed web content scanner",
    help_template = "{about}\n{author-with-newline}\n{usage}\n{all-args}"
)]
struct Cli {
    /// The base URL(s) to scan (e.g., `http://testsite.com`). Can be specified multiple times.
    #[arg(short, long, value_name = "URL")]
    urls: Vec<url::Url>,

    /// Path to a file containing a list of URLs to scan, one per line.
    #[arg(long, value_name = "FILE")]
    urls_file: Option<PathBuf>,

    /// Path to a file containing "own results" from which URLs will be extracted and scanned.
    #[arg(long, value_name = "FILE")]
    results_file: Option<PathBuf>,

    /// The path to the text file (e.g., `~/wordlists/common.txt`)
    #[arg(short, long, value_parser = wordlist_path_parser)]
    wordlist: PathBuf,

    /// Maximum number of concurrent requests
    #[arg(short, long, default_value = "2", value_parser = parse_concurrency)]
    concurrency: usize,

    /// HTTP method to use for requests
    #[arg(long, default_value = "get", value_enum)]
    method: HttpMethod,

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

    /// DANGER: Accept invalid TLS certificates (for development/testing only)
    #[arg(long)]
    danger_accept_invalid_certs: bool,

    /// Custom User-Agent header to use for requests
    #[arg(long, default_value = "dircrab/0.1.0")]
    user_agent: String,

    /// Filter: Exact word count in response body
    #[arg(long)]
    exact_words: Option<usize>,

    /// Filter: Exact character count in response body
    #[arg(long)]
    exact_chars: Option<usize>,

    /// Filter: Exact line count in response body
    #[arg(long)]
    exact_lines: Option<usize>,
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

    let mut target_urls_set: HashSet<url::Url> = HashSet::new();

    // Collect URLs from direct arguments
    if !cli.urls.is_empty() {
        target_urls_set.extend(cli.urls);
    }

    // Collect URLs from urls_file
    if let Some(urls_file_path) = cli.urls_file {
        println!("# Reading URLs from file: {}", urls_file_path.display());
        let file = File::open(&urls_file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() || trimmed_line.starts_with("#") {
                continue;
            }
            match url::Url::parse(trimmed_line) {
                Ok(parsed_url) => {
                    if parsed_url.scheme() == "http" || parsed_url.scheme() == "https" {
                        target_urls_set.insert(parsed_url);
                    } else {
                        eprintln!(
                            "Warning: Unsupported URL scheme for '{}' from file. Only http and https are supported.",
                            trimmed_line
                        );
                    }
                }
                Err(e) => eprintln!(
                    "Warning: Could not parse URL '{}' from file: {}",
                    trimmed_line, e
                ),
            }
        }
    }

    // Collect URLs from results_file
    if let Some(results_file_path) = cli.results_file {
        println!(
            "# Extracting URLs from results file: {}",
            results_file_path.display()
        );
        let file = File::open(&results_file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let url_regex = Regex::new(r"https?://[^\s]+").unwrap();

        while let Some(line) = lines.next_line().await? {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() || trimmed_line.starts_with("#") {
                continue;
            }
            for mat in url_regex.find_iter(trimmed_line) {
                let url_str = mat.as_str();
                match url::Url::parse(url_str) {
                    Ok(parsed_url) => {
                        if parsed_url.scheme() == "http" || parsed_url.scheme() == "https" {
                            target_urls_set.insert(parsed_url);
                        } else {
                            eprintln!(
                                "Warning: Unsupported URL scheme for '{}' from results file. Only http and https are supported.",
                                url_str
                            );
                        }
                    }
                    Err(e) => eprintln!(
                        "Warning: Could not parse URL '{}' from results file: {}",
                        url_str, e
                    ),
                }
            }
        }
    }

    let target_urls: Vec<url::Url> = target_urls_set.into_iter().collect();
    let mut processed_urls = Vec::new();

    for url in target_urls {
        let mut new_url = url;
        if !new_url.path().ends_with('/') {
            let mut path = new_url.path().to_string();
            path.push('/');
            new_url.set_path(&path);
        }
        processed_urls.push(new_url);
    }

    if processed_urls.is_empty() {
        eprintln!(
            "Error: No URLs provided for scanning. Use --url, --urls-file, or --results-file."
        );
        return Ok(());
    }

    println!("# Wordlist: {}", cli.wordlist.display());

    let words = read_wordlist(cli.wordlist).await?;
    println!("# Read {} words from wordlist.", words.len());

    let mut client_builder = Client::builder()
        .timeout(Duration::from_secs(10)) // 10 second timeout for requests
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(cli.user_agent);

    if cli.danger_accept_invalid_certs {
        client_builder = client_builder.danger_accept_invalid_certs(true);
    }

    let client = client_builder.build()?;

    let (tx, mut rx) = mpsc::channel::<String>(100);
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    // Add initial target URLs to the globally visited set
    {
        let mut visited = visited_urls.lock().await;
        for url in &processed_urls {
            visited.insert(url.clone());
        }
    }

    // Spawn a task to receive and print messages
    let printer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("{}", msg);
        }
    });

    for base_url in processed_urls {
        println!("# Starting scan for URL: {}", base_url);
        start_scan(
            client.clone(), // Clone client for each scan
            base_url,
            words.clone(),        // Clone words for each scan
            tx.clone(),           // Clone sender for each scan
            semaphore.clone(),    // Clone semaphore for each scan
            visited_urls.clone(), // Pass the shared visited_urls
            cli.method.clone(),
            cli.exclude_status.clone(),
            cli.include_status.clone(),
            cli.depth,
            cli.delay,
            cli.exact_words,
            cli.exact_chars,
            cli.exact_lines,
        )
        .await?;
    }

    // Drop the original tx to signal the printer_handle to finish
    drop(tx);

    // Wait for the printer task to finish
    printer_handle.await?;

    Ok(())
}

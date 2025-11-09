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

use dircrab::{FuzzMode, HttpMethod, start_scan};

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
    /// Use the `FUZZ` keyword to indicate where the wordlist should be inserted.
    /// Examples:
    /// - Path fuzzing: `http://example.com/FUZZ`
    /// - Subdomain fuzzing: `http://FUZZ.example.com`
    /// - Parameter fuzzing: `http://example.com/page?id=FUZZ`
    #[arg(short, long, value_name = "URL")]
    urls: Vec<String>,

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

    /// Custom headers to add to requests (e.g., "Authorization: Bearer <TOKEN>").
    /// Can be specified multiple times. If 'FUZZ' is present in the header value,
    /// it will be replaced by words from the wordlist.
    #[arg(short = 'H', long, value_name = "HEADER")]
    headers: Vec<String>,

    /// Filter: Exact word count(s) in response body (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exact_words: Option<Vec<usize>>,

    /// Filter: Exact character count(s) in response body (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exact_chars: Option<Vec<usize>>,

    /// Filter: Exact line count(s) in response body (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exact_lines: Option<Vec<usize>>,

    /// Filter: Exclude exact word count(s) in response body (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exclude_exact_words: Option<Vec<usize>>,

    /// Filter: Exclude exact character count(s) in response body (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exclude_exact_chars: Option<Vec<usize>>,

    /// Filter: Exclude exact line count(s) in response body (comma-separated)
    #[arg(long, value_delimiter = ',')]
    exclude_exact_lines: Option<Vec<usize>>,

    /// The request body for POST requests.
    /// If the `FUZZ` keyword is present, it will be replaced by words from the wordlist.
    /// Example: -d '{"username":"admin","password":"FUZZ"}'
    #[arg(short, long, value_name = "DATA")]
    data: Option<String>,
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

    let mut target_urls_with_modes: Vec<(url::Url, FuzzMode)> = Vec::new();

    // Helper function to determine FuzzMode and parse URL
    let parse_url_and_fuzz_mode = |url_str: &str| -> Result<(url::Url, FuzzMode), anyhow::Error> {
        let parsed_url = url::Url::parse(url_str)?;

        // Check for supported schemes
        let scheme = parsed_url.scheme();
        if scheme != "http" && scheme != "https" {
            anyhow::bail!("Unsupported URL scheme: {}. Only http and https are supported.", scheme);
        }

        let fuzz_mode = if url_str.contains("FUZZ") {
            if url_str.contains("FUZZ.") {
                FuzzMode::Subdomain
            } else if url_str.contains("?") && url_str.contains("FUZZ") {
                FuzzMode::Parameter
            } else {
                FuzzMode::Path
            }
        } else {
            FuzzMode::Path
        };
        Ok((parsed_url, fuzz_mode))
    };

    // Collect URLs from direct arguments
    for url_str in cli.urls {
        if let Ok(item) = parse_url_and_fuzz_mode(&url_str) {
            target_urls_with_modes.push(item);
        } else {
            eprintln!("Warning: Could not parse URL '{}'. Skipping.", url_str);
        }
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
            if let Ok(item) = parse_url_and_fuzz_mode(trimmed_line) {
                target_urls_with_modes.push(item);
            } else {
                eprintln!("Warning: Could not parse URL '{}' from file. Skipping.", trimmed_line);
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
                if let Ok(item) = parse_url_and_fuzz_mode(url_str) {
                    target_urls_with_modes.push(item);
                } else {
                    eprintln!("Warning: Could not parse URL '{}' from results file. Skipping.", url_str);
                }
            }
        }
    }

    let mut processed_urls_with_modes = Vec::new();

    for (url, fuzz_mode) in target_urls_with_modes {
        let mut new_url = url;
        if fuzz_mode == FuzzMode::Path && !new_url.path().ends_with('/') {
            let mut path = new_url.path().to_string();
            path.push('/');
            new_url.set_path(&path);
        }
        processed_urls_with_modes.push((new_url, fuzz_mode));
    }

    if processed_urls_with_modes.is_empty() {
        anyhow::bail!("No URLs provided for scanning. Use --url, --urls-file, or --results-file.");
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
        for (url, _) in &processed_urls_with_modes {
            visited.insert(url.clone());
        }
    }

    // Spawn a task to receive and print messages
    let printer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("{}", msg);
        }
    });

    for (base_url, fuzz_mode) in processed_urls_with_modes {
        println!("# Starting scan for URL: {} (FuzzMode: {:?})", base_url, fuzz_mode);
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
            cli.exact_words.clone(),
            cli.exact_chars.clone(),
            cli.exact_lines.clone(),
            cli.exclude_exact_words.clone(),
            cli.exclude_exact_chars.clone(),
            cli.exclude_exact_lines.clone(),
            fuzz_mode,
            cli.headers.clone(),
            cli.data.clone(), // Pass the data argument
        )
        .await?;
    }

    // Drop the original tx to signal the printer_handle to finish
    drop(tx);

    // Wait for the printer task to finish
    printer_handle.await?;

    Ok(())
}

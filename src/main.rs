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
use tokio::sync::{mpsc, broadcast, Mutex};
use tokio::signal;

mod tui;

use dircrab::{FuzzMode, HttpMethod, ScanEvent, ControlEvent};

fn parse_status_codes(s: &str) -> Result<HashSet<u16>, String> {
    s.split(',')
        .map(|s| s.trim().parse::<u16>()) 
        .collect::<Result<HashSet<u16>, _>>() 
        .map_err(|e| format!("Invalid status code: {}", e))
}

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

    /// Enable Terminal User Interface (TUI) mode
    #[arg(long, default_value = "false")]
    tui: bool,

    /// Enable verbose output, including request completion and error messages.
    #[arg(long, default_value = "false")]
    verbose: bool,
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
            anyhow::bail!(
                "Unsupported URL scheme: {}. Only http and https are supported.",
                scheme
            );
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
                eprintln!(
                    "Warning: Could not parse URL '{}' from file. Skipping.",
                    trimmed_line
                );
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
                    eprintln!(
                        "Warning: Could not parse URL '{}' from results file. Skipping.",
                        url_str
                    );
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

    let (tx_scan_events, mut rx_scan_events) = mpsc::channel::<ScanEvent>(100);
    let (tx_control, _rx_control_for_main) = broadcast::channel::<ControlEvent>(1); // Capacity 1 is enough for stop signal

    // Handle Ctrl-C for graceful shutdown
    let ctrl_c_handler_tx = tx_control.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl-C");
        eprintln!("\nCtrl-C received, attempting graceful shutdown...");
        if let Err(e) = ctrl_c_handler_tx.send(ControlEvent::Stop) {
            eprintln!("Error sending stop signal: {}", e);
        }
    });


    let rx_consumer_handle = if cli.tui {
        // Initialize TUI
        let mut terminal = tui::init()?;
        // Spawn TUI as a separate task, moving rx into it
        let tx_control_clone = tx_control.clone();
        tokio::spawn(async move {
            let result = tui::run_tui(&mut terminal, rx_scan_events, tx_control_clone).await;
            tui::restore().expect("Failed to restore terminal");
            // Convert io::Result to anyhow::Result
            result.map_err(anyhow::Error::from)
        })
    } else {
        // Spawn a task to receive and print messages, moving rx into it
        tokio::spawn(async move {
            while let Some(event) = rx_scan_events.recv().await {
                match event {
                    ScanEvent::ScanStarted { total_words } => {
                        println!("# Scan started with {} words.", total_words);
                    }
                    ScanEvent::ScanFinished => {
                        println!("# Scan finished.");
                    }
                    ScanEvent::ScanStopped => {
                        println!("# Scan stopped by user.");
                    }
                    ScanEvent::RequestCompleted => {
                        if cli.verbose {
                            eprintln!("Request completed.");
                        }
                    }
                    ScanEvent::ErrorOccurred(msg) => {
                        if cli.verbose {
                            eprintln!("Error occurred during scan: {}", msg);
                        }
                    }
                    ScanEvent::Warning(msg) => {
                        if cli.verbose {
                            eprintln!("Warning: {}", msg);
                        }
                    }
                    ScanEvent::FoundUrl(full_output) => {
                        let re = Regex::new(r"^\[\d+\]\s+(.*?)(?:\s+->.*)?\s+\[.*\]$").unwrap();
                        if let Some(captures) = re.captures(&full_output) {
                            if let Some(url) = captures.get(1) {
                                println!("{}", url.as_str());
                            } else {
                                eprintln!("Warning: Could not parse URL from '{}'", full_output);
                                println!("{}", full_output); // Fallback to printing full output
                            }
                        } else {
                            eprintln!("Warning: Could not parse URL from '{}'", full_output);
                            println!("{}", full_output); // Fallback to printing full output
                        }
                    }
                }
            }
            Ok(())
        })
    };

    let client_clone = client.clone();
    let words_clone = words.clone();
    let tx_scan_events_clone = tx_scan_events.clone();

    let cli_method_clone = cli.method.clone();
    let cli_exclude_status_clone = cli.exclude_status.clone();
    let cli_include_status_clone = cli.include_status.clone();
    let cli_depth = cli.depth;
    let cli_delay = cli.delay;
    let cli_exact_words_clone = cli.exact_words.clone();
    let cli_exact_chars_clone = cli.exact_chars.clone();
    let cli_exact_lines_clone = cli.exact_lines.clone();
    let cli_exclude_exact_words_clone = cli.exclude_exact_words.clone();
    let cli_exclude_exact_chars_clone = cli.exclude_exact_chars.clone();
    let cli_exclude_exact_lines_clone = cli.exclude_exact_lines.clone();
    let cli_headers_clone = cli.headers.clone();
    let cli_data_clone = cli.data.clone();
    let cli_concurrency = cli.concurrency;
    let cli_tui = cli.tui;
    let tx_control_orchestrator = tx_control.clone();

    let scan_orchestrator_handle = tokio::spawn(async move {
        let mut ctrl_rx_for_orchestrator = tx_control_orchestrator.subscribe(); // Orchestrator listens for control events

        for (base_url, fuzz_mode) in processed_urls_with_modes {
            // Get a resubscribed receiver for the current start_scan instance
            let current_scan_ctrl_rx = ctrl_rx_for_orchestrator.resubscribe(); 

            tokio::select! {
                _ = ctrl_rx_for_orchestrator.recv() => {
                    // Control signal received (e.g., Stop), break the loop
                    break;
                }
                _ = async {
                    // Only print this if TUI is not enabled
                    if !cli_tui {
                        println!(
                            "# Starting scan for URL: {} (FuzzMode: {:?})",
                            base_url, fuzz_mode
                        );
                    }
                    let visited_urls_arc = Arc::new(Mutex::new(HashSet::new()));
                    dircrab::start_scan(
                        client_clone.clone(), // Clone client for each scan
                        base_url,
                        words_clone.clone(),        // Clone words for each scan
                        tx_scan_events_clone.clone(),           // Clone sender for each scan
                        visited_urls_arc, // Pass the new visited_urls_arc
                        current_scan_ctrl_rx, // Pass the resubscribed receiver
                        cli_concurrency,
                        cli_method_clone.clone(),
                        cli_exclude_status_clone.clone(),
                        cli_include_status_clone.clone(),
                        cli_depth,
                        cli_delay,
                        cli_exact_words_clone.clone(),
                        cli_exact_chars_clone.clone(),
                        cli_exact_lines_clone.clone(),
                        cli_exclude_exact_words_clone.clone(),
                        cli_exclude_exact_chars_clone.clone(),
                        cli_exclude_exact_lines_clone.clone(),
                        fuzz_mode,
                        cli_headers_clone.clone(),
                        cli_data_clone.clone(), // Pass the data argument
                    )
                    .await?;
                    Ok::<(), anyhow::Error>(())
                } => {} // This arm does nothing, it's just to allow the select to proceed
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    // Drop the original tx for ScanEvents so rx_consumer_handle can complete
    drop(tx_scan_events);
    // Drop the original tx for ControlEvents to ensure all receivers will eventually see the channel closed
    drop(tx_control);

    // Wait for both the TUI/console consumer and the scan orchestrator to finish
    rx_consumer_handle.await??;
    scan_orchestrator_handle.await??;

    Ok(())
}

use dircrab::{FuzzMode, HttpMethod, start_scan};
use httptest::responders;
use httptest::{Expectation, Server, matchers::*};
use reqwest::Client;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, Semaphore};
use url::Url;

#[tokio::test]
async fn test_filter_by_exact_word_count() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_words"))
            .respond_with(responders::status_code(200).body("one two three")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/four_words"))
            .respond_with(responders::status_code(200).body("one two three four")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["three_words".to_string(), "four_words".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        Some(vec![3]), // exact_words
        None,          // exact_chars
        None,          // exact_lines
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("three_words"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("four_words"))
    );
}

#[tokio::test]
async fn test_filter_by_exact_word_count_no_match() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_words"))
            .respond_with(responders::status_code(200).body("one two three")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/four_words"))
            .respond_with(responders::status_code(200).body("one two three four")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["three_words".to_string(), "four_words".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        Some(vec![5]), // exact_words (no match for 3 or 4 words)
        None,          // exact_chars
        None,          // exact_lines
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("three_words"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("four_words"))
    );
}

#[tokio::test]
async fn test_filter_by_exact_char_count() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_chars"))
            .respond_with(responders::status_code(200).body("abc")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/four_chars"))
            .respond_with(responders::status_code(200).body("abcd")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["three_chars".to_string(), "four_chars".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        Some(vec![3]), // exact_chars
        None,          // exact_lines
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("three_chars"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("four_chars"))
    );
}

#[tokio::test]
async fn test_filter_by_exact_char_count_no_match() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_chars"))
            .respond_with(responders::status_code(200).body("abc")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/four_chars"))
            .respond_with(responders::status_code(200).body("abcd")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["three_chars".to_string(), "four_chars".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        Some(vec![5]), // exact_chars (no match for 3 or 4 chars)
        None,          // exact_lines
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("three_chars"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("four_chars"))
    );
}

#[tokio::test]
async fn test_filter_by_exact_line_count() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/two_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2\nline3")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["two_lines".to_string(), "three_lines".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,          // exclude_status
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        None,          // exact_chars
        Some(vec![2]), // exact_lines
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("two_lines"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("three_lines"))
    );
}

#[tokio::test]
async fn test_filter_by_exact_line_count_no_match() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/two_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2\nline3")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["two_lines".to_string(), "three_lines".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        None,          // exact_chars
        Some(vec![5]), // exact_lines (no match for 2 or 3 lines)
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("two_lines"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("three_lines"))
    );
}

#[tokio::test]
async fn test_filter_by_exact_combined() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/match"))
            .respond_with(responders::status_code(200).body("one two three\nfour five\nsix")),
    ); // 6 words, 27 chars, 3 lines
    server.expect(
        Expectation::matching(request::method_path("GET", "/no_match_words"))
            .respond_with(responders::status_code(200).body("one")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/no_match_chars"))
            .respond_with(responders::status_code(200).body("a")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/no_match_lines"))
            .respond_with(responders::status_code(200).body("line1")),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec![
        "match".to_string(),
        "no_match_words".to_string(),
        "no_match_chars".to_string(),
        "no_match_lines".to_string(),
    ];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,           // include_status
        0,              // max_depth
        None,           // delay
        Some(vec![6]),  // exact_words
        Some(vec![27]), // exact_chars
        Some(vec![3]),  // exact_lines
        None,           // exclude_exact_words
        None,           // exclude_exact_chars
        None,           // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(received_messages.iter().any(|msg| msg.contains("match")));
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("no_match_words"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("no_match_chars"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("no_match_lines"))
    );
}

#[tokio::test]
async fn test_filter_by_exclude_exact_word_count() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_words"))
            .respond_with(responders::status_code(200).body("one two three")),
    ); // 3 words
    server.expect(
        Expectation::matching(request::method_path("GET", "/four_words"))
            .respond_with(responders::status_code(200).body("one two three four")),
    ); // 4 words

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["three_words".to_string(), "four_words".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        None,          // exact_chars
        None,          // exact_lines
        Some(vec![3]), // exclude_exact_words (should exclude "three_words")
        None,          // exclude_exact_chars
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("three_words"))
    );
    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("four_words"))
    );
}

#[tokio::test]
async fn test_filter_by_exclude_exact_char_count() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_chars"))
            .respond_with(responders::status_code(200).body("abc")),
    ); // 3 chars
    server.expect(
        Expectation::matching(request::method_path("GET", "/four_chars"))
            .respond_with(responders::status_code(200).body("abcd")),
    ); // 4 chars

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["three_chars".to_string(), "four_chars".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        None,          // exact_chars
        None,          // exact_lines
        None,          // exclude_exact_words
        Some(vec![3]), // exclude_exact_chars (should exclude "three_chars")
        None,          // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("three_chars"))
    );
    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("four_chars"))
    );
}

#[tokio::test]
async fn test_filter_by_exclude_exact_line_count() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/two_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2")),
    ); // 2 lines
    server.expect(
        Expectation::matching(request::method_path("GET", "/three_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2\nline3")),
    ); // 3 lines

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["two_lines".to_string(), "three_lines".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        None,          // exact_chars
        None,          // exact_lines
        None,          // exclude_exact_words
        None,          // exclude_exact_chars
        Some(vec![2]), // exclude_exact_lines (should exclude "two_lines")
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("two_lines"))
    );
    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("three_lines"))
    );
}

#[tokio::test]
async fn test_filter_by_exclude_exact_combined() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/match_all"))
            .respond_with(responders::status_code(200).body("one two three\nfour five\nsix")),
    ); // 6 words, 27 chars, 3 lines
    server.expect(
        Expectation::matching(request::method_path("GET", "/exclude_words"))
            .respond_with(responders::status_code(200).body("one two three")),
    ); // 3 words
    server.expect(
        Expectation::matching(request::method_path("GET", "/exclude_chars"))
            .respond_with(responders::status_code(200).body("abc")),
    ); // 3 chars
    server.expect(
        Expectation::matching(request::method_path("GET", "/exclude_lines"))
            .respond_with(responders::status_code(200).body("line1\nline2")),
    ); // 2 lines

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec![
        "match_all".to_string(),
        "exclude_words".to_string(),
        "exclude_chars".to_string(),
        "exclude_lines".to_string(),
    ];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,          // include_status
        0,             // max_depth
        None,          // delay
        None,          // exact_words
        None,          // exact_chars
        None,          // exact_lines
        Some(vec![3]), // exclude_exact_words
        Some(vec![3]), // exclude_exact_chars
        Some(vec![2]), // exclude_exact_lines
        FuzzMode::Path,
        vec![],
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(
        received_messages
            .iter()
            .any(|msg| msg.contains("match_all"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("exclude_words"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("exclude_chars"))
    );
    assert!(
        !received_messages
            .iter()
            .any(|msg| msg.contains("exclude_lines"))
    );
}

#[tokio::test]
async fn test_start_scan_with_custom_headers() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::headers(contains(("x-custom-header", "test"))))
            .respond_with(responders::status_code(200)),
    );

    let client = Client::builder().build().unwrap();
    let base_url = Url::parse(&server.url("/").to_string()).unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    let semaphore = Arc::new(Semaphore::new(1));
    let words = vec!["test".to_string()];
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));
    let headers = vec!["X-Custom-Header: test".to_string()];

    start_scan(
        client,
        base_url,
        words,
        tx,
        semaphore,
        visited_urls.clone(),
        HttpMethod::GET,
        None,
        None,
        0,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        FuzzMode::Path,
        headers,
        None, // data
    )
    .await
    .unwrap();

    let mut received_messages = Vec::new();
    while let Some(msg) = rx.recv().await {
        received_messages.push(msg);
    }

    assert!(received_messages.iter().any(|msg| msg.contains("[200 OK]")));
}

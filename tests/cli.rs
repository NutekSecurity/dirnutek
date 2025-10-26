use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use httptest::{Server, matchers::*, responders, Expectation};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::Duration;

// Helper function to create a temporary wordlist file for tests
fn create_temp_wordlist(content: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes()).expect("Failed to write to temp file");
    file
}

#[test]
fn test_cli_valid_args() {
    let wordlist_file = create_temp_wordlist("word1\nword2\nword3");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", "http://example.com", "-w", wordlist_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("URL: http://example.com/"))
        .stdout(predicate::str::contains(format!("Wordlist: {}", wordlist_path)))
        .stdout(predicate::str::contains("Read 3 words from wordlist."));
}

#[test]
fn test_cli_invalid_url() {
    let wordlist_file = create_temp_wordlist("word1");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", "not-a-url", "-w", wordlist_path])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'not-a-url' for '--url <URL>'"));
}

#[test]
fn test_cli_non_existent_wordlist() {
    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", "http://example.com", "-w", "/path/to/non_existent_file.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Wordlist file not found"));
}

#[test]
fn test_read_empty_wordlist() {
    let wordlist_file = create_temp_wordlist("");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", "http://example.com", "-w", wordlist_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("Read 0 words from wordlist."));
}

#[test]
fn test_read_wordlist_with_empty_lines() {
    let wordlist_file = create_temp_wordlist("word1\n\nword2\n  \nword3");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", "http://example.com", "-w", wordlist_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("Read 3 words from wordlist."));
}

#[test]
fn test_cli_output_formatting() {
    let server = Server::run();

    server.expect(
        Expectation::matching(request::method_path("GET", "/found"))
            .respond_with(responders::status_code(200)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/moved"))
            .respond_with(responders::status_code(301).insert_header("Location", "/new_location")),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/forbidden"))
            .respond_with(responders::status_code(403)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/not_found"))
            .respond_with(responders::status_code(404)),
    );

    let wordlist_content = "found\nmoved\nforbidden\nnot_found\n";
    let wordlist_file = create_temp_wordlist(wordlist_content);
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let server_url = server.url("/").to_string();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", &server_url, "-w", wordlist_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("URL: ".to_owned() + &server_url))
        .stdout(predicate::str::contains(format!("Wordlist: {}", wordlist_path)))
        .stdout(predicate::str::contains("Read 4 words from wordlist."))
        .get_output()
        .stdout.clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    // Assertions for expected output
    assert!(stdout_str.contains(&("[200 OK] ".to_owned() + &server_url + "found")));
    assert!(stdout_str.contains(&("[301 Moved Permanently] ".to_owned() + &server_url + "moved -> /new_location")));
    assert!(stdout_str.contains(&("[403 Forbidden] ".to_owned() + &server_url + "forbidden")));

    // Assert that 404 is NOT in the output
    assert!(!stdout_str.contains("[404]"));
}

#[tokio::test]
async fn test_concurrency_limit() {
    let concurrency_limit = 2;
    let num_words = 5;
    let delay_ms = 100;

    let server = Server::run();
    let active_requests = Arc::new(AtomicUsize::new(0));
    let max_active_requests = Arc::new(AtomicUsize::new(0));

    let mut wordlist_content = String::new();
    for i in 0..num_words {
        let word = format!("word{}", i);
        wordlist_content.push_str(&word);
        wordlist_content.push_str("\n");

        let active_requests_clone = active_requests.clone();
        let max_active_requests_clone = max_active_requests.clone();

        server.expect(
            Expectation::matching(request::method_path("GET", format!("/word{}", i)))
                .respond_with(move || {
                    let current_active = active_requests_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active_requests_clone.fetch_max(current_active, Ordering::SeqCst);
                    
                    // Simulate work
                    std::thread::sleep(Duration::from_millis(delay_ms));

                    active_requests_clone.fetch_sub(1, Ordering::SeqCst);
                    responders::status_code(200)
                }),
        );
    }

    let wordlist_file = create_temp_wordlist(&wordlist_content);
    let wordlist_path = wordlist_file.path().to_str().unwrap();
    let server_url = server.url("/").to_string();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url,
            "-w",
            wordlist_path,
            "--concurrency",
            &concurrency_limit.to_string(),
        ])
        .assert()
        .success();

    assert!(max_active_requests.load(Ordering::SeqCst) <= concurrency_limit);
}
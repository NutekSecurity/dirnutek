use assert_cmd::Command;
use dircrab::HttpMethod;
use httptest::{Expectation, Server, matchers::*, responders};
use predicates::prelude::*;
use std::io::Write;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

// Helper function to create a temporary wordlist file for tests
fn create_temp_wordlist(content: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write to temp file");
    file
}

// Helper function to create a temporary file with URLs
fn create_temp_urls_file(content: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("Failed to create temp urls file");
    file.write_all(content.as_bytes())
        .expect("Failed to write to temp urls file");
    file
}

#[test]
fn test_cli_valid_args() {
    let wordlist_file = create_temp_wordlist("word1\nword2\nword3");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            "http://example.com",
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Starting scan for URL: http://example.com/",
        ))
        .stdout(predicate::str::contains(format!(
            "Wordlist: {}",
            wordlist_path
        )))
        .stdout(predicate::str::contains("Read 3 words from wordlist."));
}

#[test]
fn test_cli_invalid_url() {
    let wordlist_file = create_temp_wordlist("word1");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-u", "not-a-url", "-w", wordlist_path, "--method", "get"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "invalid value 'not-a-url' for '--urls <URL>'",
        ));
}

#[test]
fn test_cli_non_existent_wordlist() {
    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            "http://example.com",
            "-w",
            "/path/to/non_existent_file.txt",
            "--method",
            "get",
        ])
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
        .args(&[
            "-u",
            "http://example.com",
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
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
        .args(&[
            "-u",
            "http://example.com",
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
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
        .args(&["-u", &server_url, "-w", wordlist_path, "--method", "get"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Starting scan for URL: ".to_owned() + &server_url,
        ))
        .stdout(predicate::str::contains(format!(
            "Wordlist: {}",
            wordlist_path
        )))
        .stdout(predicate::str::contains("Read 4 words from wordlist."))
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);
    dbg!(&stdout_str);

    // Assertions for expected output
    assert!(stdout_str.contains(&("[200 OK] ".to_owned() + &server_url + "found [0W, 0C, 0L]")));
    assert!(stdout_str.contains(
        &("[301 Moved Permanently] ".to_owned()
            + &server_url
            + "moved -> /new_location [0W, 0C, 0L]")
    ));
    assert!(
        stdout_str
            .contains(&("[403 Forbidden] ".to_owned() + &server_url + "forbidden [0W, 0C, 0L]"))
    );

    // Assert that 404 is NOT in the output
    assert!(!stdout_str.contains("[404]"));
}

#[test]
fn test_cli_status_code_filtering() {
    let wordlist_content = "found\nmoved\nforbidden\nnot_found\n";
    let wordlist_file = create_temp_wordlist(wordlist_content);
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    // Test --exclude-status
    let server_exclude = Server::run();
    server_exclude.expect(
        Expectation::matching(request::method_path("GET", "/found"))
            .respond_with(responders::status_code(200)),
    );
    server_exclude.expect(
        Expectation::matching(request::method_path("GET", "/moved"))
            .respond_with(responders::status_code(301).insert_header("Location", "/new_location")),
    );
    server_exclude.expect(
        Expectation::matching(request::method_path("GET", "/forbidden"))
            .respond_with(responders::status_code(403)),
    );
    server_exclude.expect(
        Expectation::matching(request::method_path("GET", "/not_found"))
            .respond_with(responders::status_code(404)),
    );
    let server_url_exclude = server_exclude.url("/").to_string();

    let cmd_output_exclude = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url_exclude,
            "-w",
            wordlist_path,
            "--exclude-status",
            "200,404",
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str_exclude = String::from_utf8_lossy(&cmd_output_exclude);
    assert!(!stdout_str_exclude.contains("[200 OK]"));
    assert!(stdout_str_exclude.contains("[301 Moved Permanently]"));
    assert!(stdout_str_exclude.contains("[403 Forbidden]"));
    assert!(!stdout_str_exclude.contains("[404 Not Found]"));

    // Test --include-status
    let server_include = Server::run();
    server_include.expect(
        Expectation::matching(request::method_path("GET", "/found"))
            .respond_with(responders::status_code(200)),
    );
    server_include.expect(
        Expectation::matching(request::method_path("GET", "/moved"))
            .respond_with(responders::status_code(301).insert_header("Location", "/new_location")),
    );
    server_include.expect(
        Expectation::matching(request::method_path("GET", "/forbidden"))
            .respond_with(responders::status_code(403)),
    );
    server_include.expect(
        Expectation::matching(request::method_path("GET", "/not_found"))
            .respond_with(responders::status_code(404)),
    );
    let server_url_include = server_include.url("/").to_string();

    let cmd_output_include = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url_include,
            "-w",
            wordlist_path,
            "--include-status",
            "200,301",
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str_include = String::from_utf8_lossy(&cmd_output_include);
    assert!(stdout_str_include.contains("[200 OK]"));
    assert!(stdout_str_include.contains("[301 Moved Permanently]"));
    assert!(!stdout_str_include.contains("[403 Forbidden]"));
    assert!(!stdout_str_include.contains("[404 Not Found]"));

    // Test --exclude-status and --include-status (include should override exclude)
    let server_both = Server::run();
    server_both.expect(
        Expectation::matching(request::method_path("GET", "/found"))
            .respond_with(responders::status_code(200)),
    );
    server_both.expect(
        Expectation::matching(request::method_path("GET", "/moved"))
            .respond_with(responders::status_code(301).insert_header("Location", "/new_location")),
    );
    server_both.expect(
        Expectation::matching(request::method_path("GET", "/forbidden"))
            .respond_with(responders::status_code(403)),
    );
    server_both.expect(
        Expectation::matching(request::method_path("GET", "/not_found"))
            .respond_with(responders::status_code(404)),
    );
    let server_url_both = server_both.url("/").to_string();

    let cmd_output_both = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url_both,
            "-w",
            wordlist_path,
            "--exclude-status",
            "200",
            "--include-status",
            "200,403",
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str_both = String::from_utf8_lossy(&cmd_output_both);
    assert!(stdout_str_both.contains("[200 OK]"));
    assert!(!stdout_str_both.contains("[301 Moved Permanently]"));
    assert!(stdout_str_both.contains("[403 Forbidden]"));
    assert!(!stdout_str_both.contains("[404 Not Found]"));
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
            Expectation::matching(request::method_path("GET", format!("/word{}", i))).respond_with(
                move || {
                    let current_active = active_requests_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active_requests_clone.fetch_max(current_active, Ordering::SeqCst);

                    // Simulate work
                    std::thread::sleep(Duration::from_millis(delay_ms));

                    active_requests_clone.fetch_sub(1, Ordering::SeqCst);
                    responders::status_code(200)
                },
            ),
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
            "--method",
            "get",
        ])
        .assert()
        .success();

    assert!(max_active_requests.load(Ordering::SeqCst) <= concurrency_limit);
}

#[test]
fn test_cli_delay_option() {
    let num_words = 3;
    let delay_ms = 200;
    let expected_min_duration = Duration::from_millis((num_words * delay_ms) as u64);

    // Adjusted expected max_duration to account for potential test runner overhead or minor timing variances.
    let expected_max_duration = expected_min_duration + Duration::from_millis(3000); // Increased tolerance

    let server = Server::run();
    let mut wordlist_content = String::new();
    for i in 0..num_words {
        let word = format!("word{}", i);
        wordlist_content.push_str(&word);
        wordlist_content.push_str("\n");
        server.expect(
            Expectation::matching(request::method_path("GET", format!("/word{}", i)))
                .respond_with(responders::status_code(200)),
        );
    }

    let wordlist_file = create_temp_wordlist(&wordlist_content);
    let wordlist_path = wordlist_file.path().to_str().unwrap();
    let server_url = server.url("/").to_string();

    let start_time = std::time::Instant::now();
    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url,
            "-w",
            wordlist_path,
            "--delay",
            &delay_ms.to_string(),
            "--concurrency",
            "1", // Ensure requests are sequential for accurate delay measurement
            "--method",
            "get",
        ])
        .assert()
        .success();
    let duration = start_time.elapsed();

    // Allow for some overhead, but ensure the delay is respected
    assert!(
        duration >= expected_min_duration,
        "Expected duration {:?} to be at least {:?}",
        duration,
        expected_min_duration
    );
    // Also ensure it's not excessively long, e.g., less than 2x the expected duration
    assert!(
        duration < expected_max_duration,
        "Expected duration {:?} to be less than {:?}",
        duration,
        expected_max_duration
    );
}

#[test]
fn test_cli_multiple_urls() {
    let server1 = Server::run();
    server1.expect(
        Expectation::matching(request::path("/test1")).respond_with(responders::status_code(200)),
    );
    server1.expect(
        Expectation::matching(request::path("/test2")).respond_with(responders::status_code(200)),
    );
    let server_url1 = server1.url("/").to_string();

    let server2 = Server::run();
    server2.expect(
        Expectation::matching(request::path("/test1")).respond_with(responders::status_code(200)),
    );
    server2.expect(
        Expectation::matching(request::path("/test2")).respond_with(responders::status_code(200)),
    );
    let server_url2 = server2.url("/").to_string();

    let wordlist_file = create_temp_wordlist("test1\ntest2");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url1,
            "-u",
            &server_url2,
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}test1", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}test2", server_url1)));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}test1", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}test2", server_url2)));
}

#[test]
fn test_cli_urls_file() {
    let server1 = Server::run();
    server1.expect(
        Expectation::matching(request::path("/file_test1"))
            .respond_with(responders::status_code(200)),
    );
    server1.expect(
        Expectation::matching(request::path("/file_test2"))
            .respond_with(responders::status_code(200)),
    );
    let server_url1 = server1.url("/").to_string();

    let server2 = Server::run();
    server2.expect(
        Expectation::matching(request::path("/file_test1"))
            .respond_with(responders::status_code(200)),
    );
    server2.expect(
        Expectation::matching(request::path("/file_test2"))
            .respond_with(responders::status_code(200)),
    );
    let server_url2 = server2.url("/").to_string();

    let urls_content = format!("{}\n{}", server_url1, server_url2);
    let urls_file = create_temp_urls_file(&urls_content);
    let urls_file_path = urls_file.path().to_str().unwrap();

    let wordlist_file = create_temp_wordlist("file_test1\nfile_test2");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "--urls-file",
            urls_file_path,
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!("Reading URLs from file: {}", urls_file_path)));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}file_test1", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}file_test2", server_url1)));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}file_test1", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}file_test2", server_url2)));
}

#[test]
fn test_cli_results_file() {
    let server1 = Server::run();
    server1.expect(
        Expectation::matching(request::path("/result_test1"))
            .respond_with(responders::status_code(200)),
    );
    server1.expect(
        Expectation::matching(request::path("/result_test2"))
            .respond_with(responders::status_code(200)),
    );
    let server_url1 = server1.url("/").to_string();

    let server2 = Server::run();
    server2.expect(
        Expectation::matching(request::path("/result_test1"))
            .respond_with(responders::status_code(200)),
    );
    server2.expect(
        Expectation::matching(request::path("/result_test2"))
            .respond_with(responders::status_code(200)),
    );
    let server_url2 = server2.url("/").to_string();

    let results_content = format!(
        "Some text with a URL: {}\nAnother line with no URL.\nAnd here's another: {}\n",
        server_url1, server_url2
    );
    let results_file = create_temp_urls_file(&results_content); // Reusing helper, it's just a file
    let results_file_path = results_file.path().to_str().unwrap();

    let wordlist_file = create_temp_wordlist("result_test1\nresult_test2");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "--results-file",
            results_file_path,
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!(
        "Extracting URLs from results file: {}",
        results_file_path
    )));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}result_test1", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}result_test2", server_url1)));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}result_test1", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}result_test2", server_url2)));
}

#[test]
fn test_cli_no_urls_provided() {
    let wordlist_file = create_temp_wordlist("word");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&["-w", wordlist_path, "--method", "get"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Error: No URLs provided for scanning. Use --url, --urls-file, or --results-file.",
        ));
}

#[test]
fn test_cli_combination_urls() {
    let server1 = Server::run();
    server1.expect(
        Expectation::matching(request::path("/combo1")).respond_with(responders::status_code(200)),
    );
    server1.expect(
        Expectation::matching(request::path("/combo2")).respond_with(responders::status_code(200)),
    );
    server1.expect(
        Expectation::matching(request::path("/combo3")).respond_with(responders::status_code(200)),
    );
    let server_url1 = server1.url("/").to_string();

    let server2 = Server::run();
    server2.expect(
        Expectation::matching(request::path("/combo1")).respond_with(responders::status_code(200)),
    );
    server2.expect(
        Expectation::matching(request::path("/combo2")).respond_with(responders::status_code(200)),
    );
    server2.expect(
        Expectation::matching(request::path("/combo3")).respond_with(responders::status_code(200)),
    );
    let server_url2 = server2.url("/").to_string();

    let server3 = Server::run();
    server3.expect(
        Expectation::matching(request::path("/combo1")).respond_with(responders::status_code(200)),
    );
    server3.expect(
        Expectation::matching(request::path("/combo2")).respond_with(responders::status_code(200)),
    );
    server3.expect(
        Expectation::matching(request::path("/combo3")).respond_with(responders::status_code(200)),
    );
    let server_url3 = server3.url("/").to_string();

    let urls_file_content = format!("{}", server_url2);
    let urls_file = create_temp_urls_file(&urls_file_content);
    let urls_file_path = urls_file.path().to_str().unwrap();

    let results_file_content = format!("Found this: {}", server_url3);
    let results_file = create_temp_urls_file(&results_file_content);
    let results_file_path = results_file.path().to_str().unwrap();

    let wordlist_file = create_temp_wordlist("combo1\ncombo2\ncombo3");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url1,
            "--urls-file",
            urls_file_path,
            "--results-file",
            results_file_path,
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo1", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo2", server_url1)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo3", server_url1)));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo1", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo2", server_url2)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo3", server_url2)));
    assert!(stdout_str.contains(&format!("Starting scan for URL: {}", server_url3)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo1", server_url3)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo2", server_url3)));
    assert!(stdout_str.contains(&format!("[200 OK] {}combo3", server_url3)));
}

#[test]
fn test_cli_unsupported_scheme() {
    let wordlist_file = create_temp_wordlist("word");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let urls_content = "ftp://ftp.example.com\nhttp://example.com";
    let urls_file = create_temp_urls_file(&urls_content);
    let urls_file_path = urls_file.path().to_str().unwrap();

    Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "--urls-file", urls_file_path,
            "-w", wordlist_path,
            "--method", "get",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Warning: Unsupported URL scheme for 'ftp://ftp.example.com' from file. Only http and https are supported."))
        .stdout(predicate::str::contains("Starting scan for URL: http://example.com/"));
}

#[test]
fn test_cli_urls_file_with_comments() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::path("/test_url/word"))
            .respond_with(responders::status_code(200)),
    );
    let server_url = server.url("/").to_string();

    let urls_content = format!(
        "# This is a comment\n{}/\n# Another comment\nftp://ignored.com\n",
        server_url.clone() + "test_url"
    );
    let urls_file = create_temp_urls_file(&urls_content);
    let urls_file_path = urls_file.path().to_str().unwrap();

    let wordlist_file = create_temp_wordlist("word");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "--urls-file",
            urls_file_path,
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!("Starting scan for URL: {}test_url/", server_url)));
    assert!(!stdout_str.contains("ftp://ignored.com"));
    assert!(!stdout_str.contains("# This is a comment"));
}

#[test]
fn test_cli_results_file_with_comments() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::path("/test_result/word"))
            .respond_with(responders::status_code(200)),
    );
    let server_url = server.url("/").to_string();

    let results_content = format!(
        "# This is a comment in results\nFound URL: {}\n# Another comment in results\nInvalid URL: ftp://ignored.com\n",
        server_url.clone() + "test_result/"
    );
    let results_file = create_temp_urls_file(&results_content);
    let results_file_path = results_file.path().to_str().unwrap();

    let wordlist_file = create_temp_wordlist("word");
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "--results-file",
            results_file_path,
            "-w",
            wordlist_path,
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!(
        "Starting scan for URL: {}test_result/",
        server_url
    )));
    assert!(!stdout_str.contains("ftp://ignored.com"));
    assert!(!stdout_str.contains("# This is a comment in results"));
}

#[cfg(test)]
mod start_scan_tests {
    use crate::HttpMethod; // Import HttpMethod for this test module
    use dircrab::start_scan;
    use httptest::responders;
    use httptest::{Expectation, Server, matchers::*};
    use reqwest::Client;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::sync::{Mutex, Semaphore};
    use url::Url;

    #[tokio::test]
    async fn test_start_scan_no_recursion() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/admin/"))
                .respond_with(responders::status_code(200)),
        );
        server.expect(
            Expectation::matching(request::method_path("GET", "/test"))
                .respond_with(responders::status_code(200)),
        );
        server.expect(
            Expectation::matching(request::method_path("GET", "/users"))
                .respond_with(responders::status_code(200)),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(100);
        let semaphore = Arc::new(Semaphore::new(1));
        let words = vec![
            "admin/".to_string(),
            "test".to_string(),
            "users".to_string(),
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
            None, // exclude_status
            None, // include_status
            1,    // max_depth = 1 (no recursion)
            None, // delay
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await
        .unwrap();

        let mut received_messages = Vec::new();
        while let Some(msg) = rx.recv().await {
            received_messages.push(msg);
        }

        assert!(
            received_messages.contains(&format!("[200 OK] {} [0W, 0C, 0L]", server.url("/admin/")))
        );
        assert!(
            received_messages.contains(&format!("[200 OK] {} [0W, 0C, 0L]", server.url("/test")))
        );
        // Should not contain /admin/users as recursion depth is 1
        assert!(!received_messages.contains(&format!("[200 OK] {}", server.url("/admin/users"))));
    }

    #[tokio::test]
    async fn test_start_scan_no_infinite_loop() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::path("/a/"))
                .times(1)
                .respond_with(responders::status_code(200)),
        );
        server.expect(
            Expectation::matching(request::path("/a/a/"))
                .times(1)
                .respond_with(responders::status_code(200)),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(100);
        let semaphore = Arc::new(Semaphore::new(1));
        let words = vec!["a/".to_string()]; // The word that will be appended

        let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));
        let initial_base_url_clone = base_url.clone();
        visited_urls.lock().await.insert(initial_base_url_clone);

        let max_depth = 2; // Set max depth to 2 to limit recursion

        start_scan(
            client,
            base_url.clone(), // Clone base_url here
            words,
            tx,
            semaphore,
            visited_urls.clone(),
            HttpMethod::GET,
            None, // exclude_status
            None, // include_status
            max_depth, // max_depth
            None, // delay
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await
        .unwrap();

        let mut received_messages = Vec::new();
        for _ in 0..max_depth + 1 {
            // Expect messages for depth 0 and 1
            if let Some(msg) = rx.recv().await {
                received_messages.push(msg);
            } else {
                break;
            }
        }
        // So, we expect messages for /, /a/, /a/a/  etc. up to max_depth
        assert!(
            received_messages.contains(&format!("[200 OK] {} [0W, 0C, 0L]", server.url("/a/")))
        );
        // If depth was 2, we expect up to /a/a/
        assert!(
            received_messages.contains(&format!("[200 OK] {} [0W, 0C, 0L]", server.url("/a/a/")))
        );

        // We should not see /a/a/a/ or deeper if max_depth is 2
        assert!(
            !received_messages
                .contains(&format!("[200 OK] {} [0W, 0C, 0L]", server.url("/a/a/a/")))
        );

        // Verify that only the expected number of unique URLs are in visited_urls
        let final_visited = visited_urls.lock().await;
        // Expected visited URLs: /, /a/, /a/a/
        assert_eq!(
            final_visited.len(),
            max_depth + 1,
            "Visited URLs: {:?}",
            final_visited
        );
        assert!(final_visited.contains(&base_url));
        assert!(
            final_visited
                .contains(&Url::parse(&format!("{}{}", base_url.to_string(), "a/")).unwrap())
        );
        assert!(
            final_visited.contains(
                &Url::parse(&format!("{}{}{}", base_url.to_string(), "a/", "a/")).unwrap()
            )
        );

        // Also, ensure no more messages are received (indicating no infinite loop)
        tokio::time::sleep(Duration::from_millis(100)).await; // Give a moment for any delayed messages
        assert!(
            rx.try_recv().is_err(),
            "Should not receive further messages after scan completion"
        );
    }
}

#[test]
fn test_scan_deeper_on_file() {
    let server = Server::run();

    server.expect(
        Expectation::matching(request::method_path("GET", "/config.php"))
            .times(1)
            .respond_with(responders::status_code(200)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/config.php/admin"))
            .times(1)
            .respond_with(responders::status_code(200)),
    );
    // The scanner will also test for /admin at the root, and /config.php/config.php.
    // We need to expect these requests, but we don't care about the result for this test.
    server.expect(
        Expectation::matching(request::method_path("GET", "/admin"))
            .times(1..)
            .respond_with(responders::status_code(404)),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", "/config.php/config.php"))
            .times(1..)
            .respond_with(responders::status_code(404)),
    );

    let wordlist_content = "config.php\nadmin";
    let wordlist_file = create_temp_wordlist(wordlist_content);
    let wordlist_path = wordlist_file.path().to_str().unwrap();

    let server_url = server.url("/").to_string();

    let cmd_output = Command::cargo_bin("dircrab")
        .expect("Failed to find dircrab binary")
        .args(&[
            "-u",
            &server_url,
            "-w",
            wordlist_path,
            "--depth",
            "2",
            "--method",
            "get",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout_str = String::from_utf8_lossy(&cmd_output);

    assert!(stdout_str.contains(&format!("[200 OK] {}config.php", server_url)));
    assert!(stdout_str.contains(&format!("[200 OK] {}config.php/admin", server_url)));
}

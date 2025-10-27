use anyhow::Result;
use clap::ValueEnum;
use reqwest::Client;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore, mpsc::Sender};
use tokio::task::JoinSet;

#[derive(Debug, Clone, ValueEnum)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    HEAD,
    OPTIONS,
    PATCH,
}

pub async fn perform_scan(
    client: &Client,
    base_url: &url::Url,
    word: &str,
    tx: Sender<String>,
    http_method: &HttpMethod,
    exclude_status: &Option<HashSet<u16>>,
    include_status: &Option<HashSet<u16>>,
    _scan_delay: Option<u64>,
    exact_words: Option<Vec<usize>>,
    exact_chars: Option<Vec<usize>>,
    exact_lines: Option<Vec<usize>>,
    exclude_exact_words: Option<Vec<usize>>,
    exclude_exact_chars: Option<Vec<usize>>,
    exclude_exact_lines: Option<Vec<usize>>,
) -> Result<Option<url::Url>> {
    let mut url_string = base_url.to_string();
    if !url_string.ends_with('/') {
        url_string.push('/');
    }
    url_string.push_str(word);
    let target_url = url::Url::parse(&url_string)?;

    let res = match http_method {
        HttpMethod::GET => client.get(target_url.as_str()).send().await?,
        HttpMethod::POST => client.post(target_url.as_str()).send().await?,
        HttpMethod::PUT => client.put(target_url.as_str()).send().await?,
        HttpMethod::DELETE => client.delete(target_url.as_str()).send().await?,
        HttpMethod::HEAD => client.head(target_url.as_str()).send().await?,
        HttpMethod::OPTIONS => {
            client
                .request(reqwest::Method::OPTIONS, target_url.as_str())
                .send()
                .await?
        }
        HttpMethod::PATCH => client.patch(target_url.as_str()).send().await?,
    };
    let status = res.status();
    let status_code = status.as_u16();
    let url_str = target_url.to_string();

    let redirect_target = if status_code == 301 {
        res.headers()
            .get("Location")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown")
            .to_string()
    } else {
        String::new()
    };

    // Filtering logic: include_status takes precedence over exclude_status
    if let Some(include) = include_status {
        if !include.contains(&status_code) {
            return Ok(None);
        }
    } else if let Some(exclude) = exclude_status {
        if exclude.contains(&status_code) {
            return Ok(None);
        }
    } else if status_code == 404 {
        // Exclude 404 by default if no explicit filtering
        return Ok(None);
    }

    let body = res.text().await?;
    let (words, chars, lines) = if status_code == 301 {
        (0, 0, 0)
    } else {
        let w = body.split_whitespace().count();
        let c = body.chars().count();
        let l = body.lines().count();
        (w, c, l)
    };

    if let Some(exact_w_list) = exact_words {
        if !exact_w_list.contains(&words) {
            return Ok(None);
        }
    }
    if let Some(exact_c_list) = exact_chars {
        if !exact_c_list.contains(&chars) {
            return Ok(None);
        }
    }
    if let Some(exact_l_list) = exact_lines {
        if !exact_l_list.contains(&lines) {
            return Ok(None);
        }
    }

    if let Some(exclude_exact_w_list) = exclude_exact_words {
        if exclude_exact_w_list.contains(&words) {
            return Ok(None);
        }
    }
    if let Some(exclude_exact_c_list) = exclude_exact_chars {
        if exclude_exact_c_list.contains(&chars) {
            return Ok(None);
        }
    }
    if let Some(exclude_exact_l_list) = exclude_exact_lines {
        if exclude_exact_l_list.contains(&lines) {
            return Ok(None);
        }
    }

    let output = match status_code {
        301 => Some(format!(
            "[{}] {} -> {} [{}W, {}C, {}L]",
            status, url_str, redirect_target, words, chars, lines
        )),
        _ => Some(format!(
            "[{}] {} [{}W, {}C, {}L]",
            status, url_str, words, chars, lines
        )),
    };

    if let Some(msg) = output {
        tx.send(msg).await?;
    }

    // If the status is success, we've found something.
    // We'll return it as a potential base for the next level of scanning.
    if status.is_success() {
        let mut new_base_url = target_url;
        // Ensure the path ends with a '/' to allow for deeper scanning.
        if !new_base_url.path().ends_with('/') {
            let mut path = new_base_url.path().to_string();
            path.push('/');
            new_base_url.set_path(&path);
        }
        Ok(Some(new_base_url))
    } else if status.is_redirection() {
        // For redirects, we also consider it for further scanning.
        Ok(Some(target_url))
    } else {
        Ok(None)
    }
}

pub async fn start_scan(
    client: Client,
    base_url: url::Url,
    words: Vec<String>,
    tx: Sender<String>,
    semaphore: Arc<Semaphore>,
    visited_urls: Arc<Mutex<HashSet<url::Url>>>,
    http_method: HttpMethod,
    exclude_status: Option<HashSet<u16>>,
    include_status: Option<HashSet<u16>>,
    max_depth: usize,
    delay: Option<u64>,
    exact_words: Option<Vec<usize>>,
    exact_chars: Option<Vec<usize>>,
    exact_lines: Option<Vec<usize>>,
    exclude_exact_words: Option<Vec<usize>>,
    exclude_exact_chars: Option<Vec<usize>>,
    exclude_exact_lines: Option<Vec<usize>>,
) -> Result<()> {
    let scan_delay_for_loop = delay.clone();
    let scan_queue: Arc<Mutex<VecDeque<(url::Url, usize)>>> = Arc::new(Mutex::new(VecDeque::new()));
    let mut join_set: JoinSet<Result<()>> = JoinSet::new();

    // Initial push to the queue
    scan_queue.lock().await.push_back((base_url.clone(), 0));

    loop {
        // Dequeue a URL to scan if available
        let (current_url, current_depth) = {
            let mut queue = scan_queue.lock().await;
            if let Some(item) = queue.pop_front() {
                item
            } else if join_set.is_empty() {
                // If queue is empty and no active scans, we are done
                break;
            } else {
                // Queue is empty but scans are active, wait for one to complete
                // This allows new URLs to be added to the queue by other tasks
                drop(queue); // Release the lock before awaiting
                tokio::select! {
                    _ = join_set.join_next() => {},
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {},
                }
                continue;
            }
        };

        if max_depth != 0 && current_depth >= max_depth {
            continue;
        }

        for word in &words {
            let client_clone = client.clone();
            let current_url_clone = current_url.clone();
            let tx_clone = tx.clone();
            let semaphore_clone = semaphore.clone();
            let exclude_status_clone = exclude_status.clone();
            let include_status_clone = include_status.clone();
            let word_clone = word.clone();
            let visited_urls_clone = visited_urls.clone();
            let scan_queue_clone = scan_queue.clone();
            let scan_delay_clone = scan_delay_for_loop.clone();
            let http_method_clone = http_method.clone();
            let exact_words_clone = exact_words.clone();
            let exact_chars_clone = exact_chars.clone();
            let exact_lines_clone = exact_lines.clone();
            let exclude_exact_words_clone = exclude_exact_words.clone();
            let exclude_exact_chars_clone = exclude_exact_chars.clone();
            let exclude_exact_lines_clone = exclude_exact_lines.clone();

            join_set.spawn(async move {
                let _permit = semaphore_clone
                    .acquire()
                    .await
                    .expect("Failed to acquire semaphore permit");

                if let Some(d) = scan_delay_clone {
                    tokio::time::sleep(tokio::time::Duration::from_millis(d)).await;
                }

                let result = perform_scan(
                    &client_clone,
                    &current_url_clone,
                    &word_clone,
                    tx_clone,
                    &http_method_clone,
                    &exclude_status_clone,
                    &include_status_clone,
                    scan_delay_clone,
                    exact_words_clone,
                    exact_chars_clone,
                    exact_lines_clone,
                    exclude_exact_words_clone,
                    exclude_exact_chars_clone,
                    exclude_exact_lines_clone,
                )
                .await;

                if let Ok(Some(found_url)) = result {
                    if max_depth == 0 || current_depth < max_depth {
                        let mut visited = visited_urls_clone.lock().await;
                        if visited.insert(found_url.clone()) {
                            scan_queue_clone
                                .lock()
                                .await
                                .push_back((found_url, current_depth + 1));
                        }
                    }
                } else if let Err(e) = result {
                    eprintln!(
                        "Error from perform_scan for {} + {}: {:?}",
                        current_url_clone, word_clone, e
                    );
                }
                Ok(())
            });
        }
    }

    // Wait for any remaining tasks in the join_set to complete
    while let Some(res) = join_set.join_next().await {
        res??;
    }

    drop(tx);

    Ok(())
}

#[cfg(test)]
mod tests {
    use httptest::responders;
    use httptest::{Expectation, Server, matchers::*};
    use reqwest::Client; // Explicit import
    use std::collections::HashSet;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use url::Url; // Explicit import

    use crate::{HttpMethod, perform_scan}; // Import perform_scan explicitly

    #[tokio::test]
    async fn test_perform_scan_success() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/test_path"))
                .respond_with(responders::status_code(200)),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = perform_scan(
            &client,
            &base_url,
            "test_path",
            tx,
            &HttpMethod::GET,
            &None,
            &None,
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // scan_delay
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_perform_scan_not_found() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/non_existent"))
                .respond_with(responders::status_code(404)),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = perform_scan(
            &client,
            &base_url,
            "non_existent",
            tx,
            &HttpMethod::GET,
            &None,
            &None,
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // scan_delay
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await;
        assert!(result.is_ok()); // 404 is a valid HTTP response, not an error in reqwest
    }
    #[tokio::test]
    async fn test_perform_scan_timeout() {
        // Create a TCP listener that will accept a connection but not send any data
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            // Keep the connection open but don't send any data, causing the client to timeout
            // Sleep for a duration longer than the client's timeout
            tokio::time::sleep(Duration::from_secs(5)).await;
            // Optionally, close the socket after the sleep if needed, but for a timeout test,
            // simply not responding is sufficient.
            let _ = socket.shutdown().await;
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(1)) // Client timeout is 1 second
            .build()
            .unwrap();
        let base_url = Url::parse(&format!("http://{}", addr)).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = perform_scan(
            &client,
            &base_url,
            "timeout",
            tx,
            &HttpMethod::GET,
            &None,
            &None,
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // scan_delay
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await;
        assert!(result.is_err());
        let _err = result.unwrap_err(); // Fixed unused variable warning
    }

    #[tokio::test]
    async fn test_perform_scan_include_404_explicitly() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/not_found"))
                .respond_with(responders::status_code(404)),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        let mut include_status = HashSet::new();
        include_status.insert(404);

        let result = perform_scan(
            &client,
            &base_url,
            "not_found",
            tx,
            &HttpMethod::GET,
            &None,
            &Some(include_status),
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // scan_delay
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await;
        assert!(result.is_ok());

        // Ensure a message was sent for the 404 status
        let received_message = rx.recv().await.expect("Expected a message for 404 status");
        assert_eq!(
            received_message,
            format!("[404 Not Found] {} [0W, 0C, 0L]", server.url("/not_found"))
        );
    }

    #[tokio::test]
    async fn test_perform_scan_exclude_404_by_default() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/not_found"))
                .respond_with(responders::status_code(404)),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(1);

        let result = perform_scan(
            &client,
            &base_url,
            "not_found",
            tx,
            &HttpMethod::GET,
            &None,
            &None,
            None, // exact_words
            None, // exact_chars
            None, // exact_lines
            None, // scan_delay
            None, // exclude_exact_words
            None, // exclude_exact_chars
            None, // exclude_exact_lines
        )
        .await;
        assert!(result.is_ok());

        // Ensure no message was sent for the 404 status
        tokio::time::sleep(Duration::from_millis(10)).await; // Give some time for message to be sent if it were
        assert!(rx.try_recv().is_err()); // Should be empty
    }
}

#[cfg(test)]
mod start_scan_tests {
    use crate::{HttpMethod, start_scan}; // Import start_scan explicitly
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
            visited_urls.clone(), // Added visited_urls argument
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
        // Make the server respond with 200 OK to any GET request
        server.expect(
            Expectation::matching(request::method("GET"))
                .times(..)
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
            max_depth,
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
        for _ in 0..max_depth {
            // Expect messages for depth 0 and 1
            if let Some(msg) = rx.recv().await {
                received_messages.push(msg);
            } else {
                break;
            }
        }
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
        assert!(final_visited.contains(&Url::parse(&server.url("/a/").to_string()).unwrap()));
        assert!(final_visited.contains(&Url::parse(&server.url("/a/a/").to_string()).unwrap()));

        // Also, ensure no more messages are received (indicating no infinite loop)
        tokio::time::sleep(Duration::from_millis(100)).await; // Give a moment for any delayed messages
        assert!(
            rx.try_recv().is_err(),
            "Should not receive further messages after scan completion"
        );
    }
}

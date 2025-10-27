use anyhow::Result;
use reqwest::Client;
use tokio::sync::{mpsc::Sender, Semaphore, Mutex};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::task::JoinSet;
use clap::ValueEnum;

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
) -> Result<Option<url::Url>> {
    let mut target_url = base_url.clone();
    target_url.path_segments_mut().map_err(|_| anyhow::anyhow!("cannot be a base"))?.push(word);

    let res = match http_method {
        HttpMethod::GET => client.get(target_url.as_str()).send().await?,
        HttpMethod::POST => client.post(target_url.as_str()).send().await?,
        HttpMethod::PUT => client.put(target_url.as_str()).send().await?,
        HttpMethod::DELETE => client.delete(target_url.as_str()).send().await?,
        HttpMethod::HEAD => client.head(target_url.as_str()).send().await?,
        HttpMethod::OPTIONS => client.request(reqwest::Method::OPTIONS, target_url.as_str()).send().await?,
        HttpMethod::PATCH => client.patch(target_url.as_str()).send().await?,
    };
    let status = res.status();
    let status_code = status.as_u16();
    let url_str = target_url.to_string();

    // Filtering logic: include_status takes precedence over exclude_status
    if let Some(include) = include_status {
        if !include.contains(&status_code) {
            return Ok(None);
        }
    } else if let Some(exclude) = exclude_status {
        if exclude.contains(&status_code) {
            return Ok(None);
        }
    } else if status_code == 404 { // Exclude 404 by default if no explicit filtering
        return Ok(None);
    }

    let output = match status_code {
        301 => {
            let redirect_target = res.headers()
                .get(reqwest::header::LOCATION)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("unknown");
            Some(format!("[{}] {} -> {}", status, url_str, redirect_target))
        },
        _ => Some(format!("[{}] {}", status, url_str)),
    };

    if let Some(msg) = output {
        tx.send(msg).await?;
    }
    res.bytes().await?;

    // Check if it's a directory
    if status.is_success() && target_url.path().ends_with('/') {
        Ok(Some(target_url))
    } else if status.is_redirection() {
        // For redirects, we'll consider it a potential directory to be explored
        // The recursive scanner will handle following the redirect
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
    http_method: HttpMethod,
    exclude_status: Option<HashSet<u16>>,
    include_status: Option<HashSet<u16>>,
    max_depth: usize,
    delay: Option<u64>,
) -> Result<()> {
    let visited_urls: Arc<Mutex<HashSet<url::Url>>> = Arc::new(Mutex::new(HashSet::new()));
    let scan_queue: Arc<Mutex<VecDeque<(url::Url, usize)>>> = Arc::new(Mutex::new(VecDeque::new()));
    let mut join_set: JoinSet<Result<()>> = JoinSet::new();

    // Initial push to the queue
    scan_queue.lock().await.push_back((base_url.clone(), 0));
    visited_urls.lock().await.insert(base_url);

    loop {
        // Dequeue a URL to scan if available
        let (current_url, current_depth) = {
            let mut queue = scan_queue.lock().await;
            if let Some(item) = queue.pop_front() {
                item
            } else if join_set.is_empty() {
                // If queue is empty and no active scans, we are done
                break;
            }
            else {
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
            let delay_clone = delay.clone();
            let http_method_clone = http_method.clone();

            join_set.spawn(async move {
                let _permit = semaphore_clone
                    .acquire()
                    .await
                    .expect("Failed to acquire semaphore permit");

                if let Some(d) = delay_clone {
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
                )
                .await;

                if let Ok(Some(found_url)) = result {
                    if max_depth == 0 || current_depth + 1 < max_depth {
                        let mut visited = visited_urls_clone.lock().await;
                        if visited.insert(found_url.clone()) {
                            scan_queue_clone.lock().await.push_back((found_url, current_depth + 1));
                        }
                    }
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
    use httptest::{Server, matchers::*, Expectation};
    use httptest::responders;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::mpsc;
    use reqwest::Client; // Explicit import
    use url::Url; // Explicit import
    use std::collections::HashSet;

    use crate::{perform_scan, HttpMethod}; // Import perform_scan explicitly

    #[tokio::test]
    async fn test_perform_scan_success() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/test_path")).respond_with(responders::status_code(200)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build().unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = perform_scan(&client, &base_url, "test_path", tx, &HttpMethod::GET, &None, &None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_perform_scan_not_found() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/non_existent")).respond_with(responders::status_code(404)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build().unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = perform_scan(&client, &base_url, "non_existent", tx, &HttpMethod::GET, &None, &None).await;
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
            .build().unwrap();
        let base_url = Url::parse(&format!("http://{}", addr)).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = perform_scan(&client, &base_url, "timeout", tx, &HttpMethod::GET, &None, &None).await;
        assert!(result.is_err());
        let _err = result.unwrap_err(); // Fixed unused variable warning
    }

    #[tokio::test]
    async fn test_perform_scan_include_404_explicitly() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/not_found")).respond_with(responders::status_code(404)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build().unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        let mut include_status = HashSet::new();
        include_status.insert(404);

        let result = perform_scan(&client, &base_url, "not_found", tx, &HttpMethod::GET, &None, &Some(include_status)).await;
        assert!(result.is_ok());

        // Ensure a message was sent for the 404 status
        let received_message = rx.recv().await.expect("Expected a message for 404 status");
        assert_eq!(received_message, format!("[404 Not Found] {}", server.url("/not_found")));
    }

    #[tokio::test]
    async fn test_perform_scan_exclude_404_by_default() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/not_found")).respond_with(responders::status_code(404)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build().unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(1);

        let result = perform_scan(&client, &base_url, "not_found", tx, &HttpMethod::GET, &None, &None).await;
        assert!(result.is_ok());

        // Ensure no message was sent for the 404 status
        tokio::time::sleep(Duration::from_millis(10)).await; // Give some time for message to be sent if it were
        assert!(rx.try_recv().is_err()); // Should be empty
    }
}

#[cfg(test)]
mod start_scan_tests {
    use crate::{start_scan, HttpMethod}; // Import start_scan explicitly
     // Import perform_scan explicitly
    use httptest::{Server, matchers::*, Expectation};
    use httptest::responders;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use reqwest::Client;
    use std::sync::Arc;
    use tokio::sync::Semaphore;
    use url::Url;

    #[tokio::test]
    async fn test_start_scan_no_recursion() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/admin%2F")).respond_with(responders::status_code(200)));
        server.expect(Expectation::matching(request::method_path("GET", "/test")).respond_with(responders::status_code(200)));
        server.expect(Expectation::matching(request::method_path("GET", "/users")).respond_with(responders::status_code(200)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .redirect(reqwest::redirect::Policy::none())
            .build().unwrap();
        let base_url = Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, mut rx) = mpsc::channel(100);
        let semaphore = Arc::new(Semaphore::new(1));
        let words = vec!["admin/".to_string(), "test".to_string(), "users".to_string()];

        start_scan(
            client,
            base_url,
            words,
            tx,
            semaphore,
            HttpMethod::GET,
            None,
            None,
            1, // max_depth = 1 (no recursion)
            None, // delay
        ).await.unwrap();

        let mut received_messages = Vec::new();
        while let Some(msg) = rx.recv().await {
            received_messages.push(msg);
        }
        println!("Received messages: {:?}", received_messages);

        assert!(received_messages.contains(&format!("[200 OK] {}", server.url("/admin%2F"))));
        assert!(received_messages.contains(&format!("[200 OK] {}", server.url("/test"))));
        // Should not contain /admin/users as recursion depth is 1
        assert!(!received_messages.contains(&format!("[200 OK] {}", server.url("/admin/users"))));
    }
}
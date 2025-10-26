use anyhow::Result;
use reqwest::Client;
use tokio::sync::mpsc::Sender;
use std::collections::HashSet;

pub async fn scan_url(
    client: &Client,
    base_url: &url::Url,
    word: &str,
    tx: Sender<String>,
    exclude_status: &Option<HashSet<u16>>,
    include_status: &Option<HashSet<u16>>,
) -> Result<()> {
    let mut target_url = base_url.clone();
    target_url.path_segments_mut().map_err(|_| anyhow::anyhow!("cannot be a base"))?.push(word);

    let res = client.get(target_url.as_str()).send().await?;
    let status = res.status();
    let status_code = status.as_u16();
    let url_str = target_url.to_string();

    // Filtering logic: include_status takes precedence over exclude_status
    if let Some(include) = include_status {
        if !include.contains(&status_code) {
            return Ok(());
        }
    } else if let Some(exclude) = exclude_status {
        if exclude.contains(&status_code) {
            return Ok(());
        }
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
    res.bytes().await?; // Consume the response body to release the connection

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httptest::{Server, matchers::*, Expectation};
    use httptest::responders;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_scan_url_success() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/test_path")).respond_with(responders::status_code(200)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build().unwrap();
        let base_url = url::Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = scan_url(&client, &base_url, "test_path", tx, &None, &None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_scan_url_not_found() {
        let server = Server::run();
        server.expect(Expectation::matching(request::method_path("GET", "/non_existent")).respond_with(responders::status_code(404)));

        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            .build().unwrap();
        let base_url = url::Url::parse(&server.url("/").to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = scan_url(&client, &base_url, "non_existent", tx, &None, &None).await;
        assert!(result.is_ok()); // 404 is a valid HTTP response, not an error in reqwest
    }
    #[tokio::test]
    async fn test_scan_url_timeout() {
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
        let base_url = url::Url::parse(&format!("http://{}", addr)).unwrap();
        let (tx, _rx) = mpsc::channel(1);

        let result = scan_url(&client, &base_url, "timeout", tx, &None, &None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("timeout"));
    }
}

Okay, here's a perfect "chillout" project. It's fun, 100% safe to build and test, and a fantastic way to master asynchronous Rust.



It's a **blazing-fast, concurrent directory and file scanner** for web servers. Think of it as your own personal version of `gobuster` or `dirsearch`.



---



## Project Idea: `DirCrab` - A High-Speed Web Content Scanner



The core idea is to build a tool that takes a wordlist and a target URL, and then *very* quickly tries to find valid pages, directories, or files on that server.



Example: You run `dircrab -u http://example.com -w common.txt`, and it finds `http://example.com/admin` (a 403 Forbidden) and `http://example.com/robots.txt` (a 200 OK).



### Why This Is a Great "Chillout" Project 🏖️



* **No `unsafe` Code:** This is a 100% "safe" Rust project. It's all about high-level logic.

* **Masters Asynchronous Rust:** The *entire* project is a practical lesson in `async`/`await` and `tokio`. The goal is to make thousands of HTTP requests per second, which is `tokio`'s specialty.

* **Pure Networking:** It's all about sending HTTP requests and reading HTTP responses. You'll become an expert with the **`reqwest`** crate.

* **Fundamental Recon:** This is a *core* reconnaissance technique. Every single web penetration test starts with this.



---



### Core Features (The "MVP")



1. **CLI:** Use the **`clap`** crate to take command-line arguments:

    * `-u` or `--url`: The base URL to scan (e.g., `http://testsite.com`).

    * `-w` or `--wordlist`: The path to the text file (e.g., `~/wordlists/common.txt`).

2. **Core Logic:**

    * Read the wordlist file.

    * For *each* word in the file (e.g., "admin"), create a new URL (e.g., `http://testsite.com/admin`).

    * Create a `tokio` task to send an HTTP `GET` or `HEAD` request to that URL.

3.  **Concurrency:** Don't do this one by one! Use `tokio::spawn` to launch *hundreds* (or thousands) of these requests concurrently.

4.  **Response Handling:**

    *   Check the HTTP status code of the response.

    *   **Print interesting results:**

        *   `[200 OK] /index.html`

        *   `[301 Moved] /old-page -> /new-page`

        *   `[403 Forbidden] /admin` (Finding a "Forbidden" directory is a *win*!)

    *   **Ignore boring results:** By default, you'd ignore `404 Not Found`.



---



### "Show-Off" Bonus Features 🚀



*   **Recursive Scanning:** If you find a directory (e.g., `/api/`), automatically start a *new* scan inside that directory (e.g., `/api/users`, `/api/v1`, etc.).

*   **Status Code Filtering:** Add flags to let the user filter results.

    *   `--exclude-404` (Default)

    *   `--include-500` (To find server errors)

    *   `--only-200` (To only see valid pages)

*   **Concurrency Limiting:** Use a `tokio::sync::Semaphore` to limit concurrent requests to a specific number (e.g., 50) so you don't crash your machine or the target.

*   **TUI Dashboard:** Use **`ratatui`** to make a cool terminal dashboard showing:

    *   Requests per second (RPS).

    *   A live-updating list of "Found" results.

    *   Total progress (e.g., "Request 10,234 / 50,000").



---



## Implementation Plan: `DirCrab`



This plan outlines a structured approach to building `DirCrab`, starting with the Minimum Viable Product (MVP) and then progressively adding "Show-Off" bonus features.



### Phase 1: Project Setup & MVP



1.  ✅ **Project Initialization:**

    *   ✅ Create a new Rust project: `cargo new dircrab --bin`

    *   ✅ Add dependencies to `Cargo.toml`:

        ```toml

        [dependencies]

        clap = { version = "4.0", features = ["derive"] } # For CLI argument parsing

        tokio = { version = "1", features = ["full"] } # For asynchronous runtime

        reqwest = { version = "0.11", features = ["json", "rustls-tls"] } # For HTTP requests

        anyhow = "1.0" # For simplified error handling

        # Consider `url` crate for robust URL manipulation if needed

        ```

    *   ✅ **Consideration:** Start with a minimal set of features for `tokio` and `reqwest` and add more as needed to keep compile times down. `rustls-tls` is generally preferred for security over `native-tls`.



2.  ✅ **CLI Argument Parsing (`clap`):**

    *   ✅ Define a `struct` to hold command-line arguments (URL, wordlist path).

    *   ✅ Use `#[derive(Parser)]` and `#[clap(author, version, about)]` for metadata.

    *   ✅ Implement argument validation (e.g., URL format, wordlist file existence).

    *   ✅ **Consideration:** Provide clear help messages for all arguments.



3.  **Wordlist Loading:**

    *   ✅ Create an `async` function to read the wordlist file.

    *   ✅ Read the file line by line, storing words in a `Vec<String>`.

    *   ✅ Handle file I/O errors gracefully (e.g., file not found).

    *   **Consideration:** For very large wordlists, consider streaming words instead of loading all into memory to reduce memory footprint.



4.  **Asynchronous HTTP Requests (`tokio`, `reqwest`):**

    *   ✅ Create an `async` function, e.g., `scan_url(client: &reqwest::Client, base_url: &str, word: &str) -> Result<(), anyhow::Error>`.

    *   ✅ Inside `scan_url`, construct the full URL (e.g., `http://example.com/admin`).

    *   ✅ Use `reqwest::Client` to send `GET` or `HEAD` requests. `HEAD` requests are often faster as they don't download the body, but might not always reflect the true status for all servers. Start with `GET` for simplicity, then optimize to `HEAD` if appropriate.

    *   ✅ Spawn each `scan_url` call as a `tokio::spawn` task. Collect the `JoinHandle`s.

    *   ✅ **Consideration:** `reqwest::Client` should be created once and reused for all requests to benefit from connection pooling. Implement a timeout for requests to prevent hanging.



5.  **Response Processing & Output:**

    *   ✅ Inside `scan_url`, after receiving a response, check `response.status()`.

    *   ✅ Print results for interesting status codes (200 OK, 301 Moved, 403 Forbidden).

    *   ✅ By default, ignore 404 Not Found.

    *   ✅ Format output clearly, e.g., `[STATUS_CODE] /path -> redirect_target (if 301)`.

    *   ✅ **Consideration:** Use a `tokio::sync::mpsc` channel to send results from `scan_url` tasks to a main thread for printing, ensuring ordered output and avoiding interleaved prints.



### Phase 2: Bonus Features



1.  ✅ **Concurrency Limiting (`tokio::sync::Semaphore`):**

    *   ✅ Introduce a `tokio::sync::Semaphore` with a configurable maximum number of permits.

    *   ✅ Before spawning each `scan_url` task, acquire a permit from the semaphore.

    *   ✅ Ensure the permit is released when the `scan_url` task completes (e.g., using `_permit = semaphore.acquire().await;`).

    *   ✅ Add a CLI argument for `--concurrency` to set the semaphore limit.

    *   ✅ **Consideration:** Experiment with different concurrency limits to find an optimal balance between speed and resource usage.



2.  ✅ **Status Code Filtering:**

    *   ✅ Add CLI arguments like `--exclude-status <CODES>` and `--include-status <CODES>`.

    *   ✅ Parse these arguments into a `HashSet<u16>` for efficient lookup.

    *   ✅ Modify the response processing logic to filter based on these sets.

    *   ✅ **Consideration:** Define clear precedence rules if both `--exclude` and `--include` are used (e.g., `--include` overrides `--exclude`).



3.  ✅ **Recursive Scanning:**

    *   ✅ When a directory is found (e.g., a 200 OK for `/admin/` or a 301 redirect to a directory), add it to a queue of URLs to be scanned.

    *   ✅ Implement a mechanism to track already scanned URLs to prevent infinite loops.

    *   ✅ Add a CLI argument for `--recursive` or `--depth <N>` to control recursion.

    *   ✅ **Consideration:** Be mindful of the performance impact of recursive scanning, as it can significantly increase the number of requests. Implement a maximum recursion depth.



4.  **TUI Dashboard (`ratatui`):**

    *   This is a significant feature and might warrant its own module.

    *   Integrate `ratatui` to draw a terminal UI.

    *   Use `tokio::sync::mpsc` channels to send updates (RPS, new findings, progress) from the scanning tasks to the TUI rendering loop.

    *   Display:

        *   Requests per second (RPS) - calculate based on completed requests over time.

        *   A live-updating list of "Found" results.

        *   Total progress (e.g., "Request X / Y" or percentage).

    *   **Consideration:** `ratatui` requires careful state management and event handling. Start with a very basic display and incrementally add complexity. Ensure the TUI doesn't block the scanning process.



### Phase 3: Error Handling & Refinements



1.  **Robust Error Handling:**

    *   ✅ Use `anyhow::Result` for functions that can fail.

    *   ✅ Provide informative error messages to the user.

    *   Handle specific `reqwest` errors (e.g., network issues, DNS resolution failures).



2.  **Configuration:**

    *   Consider a configuration file (e.g., `dircrab.toml`) for default settings.



3.  **Performance Optimization:**

    *   Profile the application to identify bottlenecks.

    *   Experiment with `reqwest` client settings (e.g., `tcp_nodelay`, `connect_timeout`).



4.  **Documentation:**

    *   Add comprehensive comments to the code.

    *   Update the `README.md` with usage instructions and examples.



### General Considerations:



*   **Modularity:** Organize code into logical modules (e.g., `cli.rs`, `scanner.rs`, `tui.rs`).

*   ✅ **Testing:** Write unit tests for individual functions and integration tests for the overall flow.

*   **User Experience:** Ensure the CLI is intuitive and the output is clear and actionable.

*   ✅ **Rate Limiting:** Be aware that aggressive scanning can trigger rate limits or IP bans on target servers. Consider adding an optional delay between requests.

*   ✅ **HTTP Methods:** While the MVP focuses on `GET`/`HEAD`, consider adding support for other HTTP methods (e.g., `POST`) as a future enhancement.

*   ✅ **User-Agent:** Set a custom User-Agent header to identify your scanner.

*   ✅ **SSL/TLS:** `reqwest` handles this by default, but be aware of potential issues with self-signed certificates or older TLS versions.

This plan provides a roadmap for building `DirCrab`. Remember to iterate, test frequently, and enjoy the process of mastering asynchronous Rust!



## Notes

### Tokio Remarks

`tokio` is an asynchronous runtime for Rust, enabling concurrent operations without traditional threads. In this crate, it's primarily used to manage and execute non-blocking I/O operations and concurrent tasks efficiently.

**What `tokio` is used for:**

*   **Asynchronous Execution**: Running multiple operations (like network requests or file I/O) concurrently without blocking the main thread.
*   **Concurrency Control**: Managing shared resources and limiting the number of simultaneous tasks.
*   **Task Management**: Spawning, monitoring, and awaiting the completion of asynchronous tasks.
*   **Asynchronous I/O**: Performing file and network operations in a non-blocking manner.

**How and When `tokio` is used:**

1.  **`#[tokio::main]` (in `src/main.rs`)**:
    *   **How**: This attribute macro transforms the `main` function into an asynchronous entry point, setting up the `tokio` runtime to execute the async code.
    *   **When**: It's used once at the very beginning of the program to enable the entire application to run asynchronously.

2.  **`tokio::spawn` (in `src/main.rs` and `src/lib.rs`)**:
    *   **How**: Spawns a new asynchronous task that runs concurrently with other tasks on the `tokio` runtime.
    *   **When**: 
        *   In `src/main.rs`, it's used to spawn a dedicated "printer" task (`printer_handle`) that receives and prints messages from the scanning process.
        *   In `src/lib.rs` (within the `start_scan` function), it's used to spawn multiple `perform_scan` tasks, allowing the application to send many HTTP requests concurrently.

3.  **`tokio::sync::mpsc::Sender` (in `src/lib.rs` and `src/main.rs`)**:
    *   **How**: Provides an asynchronous, multi-producer, single-consumer channel for sending messages between tasks.
    *   **When**: The `Sender` (`tx`) is passed around to `perform_scan` tasks, allowing them to send formatted scan results (e.g., `"[200 OK] http://example.com [10W, 50C, 2L]"`) back to the central printer task in `main.rs`.

4.  **`tokio::sync::{Mutex, Semaphore}` (in `src/lib.rs` and `src/main.rs`)**:
    *   **How**: 
        *   `Mutex`: An asynchronous mutual exclusion primitive, similar to a standard mutex but designed for async contexts.
        *   `Semaphore`: A counting semaphore used to limit the number of concurrent operations.
    *   **When**: 
        *   `Mutex`: Used to protect shared data structures like `visited_urls` (to prevent rescanning) and `scan_queue` (to manage URLs to be scanned) from concurrent access by multiple tasks.
        *   `Semaphore`: Used in `start_scan` to limit the number of active `perform_scan` tasks (and thus concurrent HTTP requests), preventing the application from overwhelming the target server or its own resources.

5.  **`tokio::time::sleep` (in `src/lib.rs` and tests)**:
    *   **How**: Asynchronously pauses the execution of the current task for a specified duration without blocking the entire runtime.
    *   **When**: 
        *   In `perform_scan`, it's used to implement a `scan_delay` between requests if configured, to avoid hammering the target server.
        *   In `start_scan`, it's used within the `tokio::select!` block as a fallback to periodically check the queue if no tasks have completed.
        *   In tests, it's used to simulate delays or ensure proper timing for asynchronous operations.

6.  **`tokio::select!` (in `src/lib.rs`)**:
    *   **How**: Allows waiting on multiple asynchronous operations simultaneously and executing the branch corresponding to the first operation that completes.
    *   **When**: In `start_scan`, it's used to efficiently wait for either a spawned task to complete (potentially adding new URLs to the queue) or for a short timeout, preventing the main loop from busy-waiting when the queue is empty but tasks are still running.

7.  **`tokio::fs::File`, `tokio::io::{AsyncBufReadExt, BufReader}` (in `src/main.rs`)**:
    *   **How**: Provides asynchronous file I/O operations.
    *   **When**: In `main.rs`, these are used to asynchronously read the wordlist file, allowing the application to remain responsive while loading potentially large files.

8.  **`tokio::net::TcpListener` (in tests)**:
    *   **How**: Provides an asynchronous TCP listener for network connections.
    *   **When**: Used in tests (e.g., `test_perform_scan_timeout`) to set up a mock server that can simulate network behavior like timeouts, allowing for robust testing of the `perform_scan` function.

9.  **`tokio::task::JoinSet` (in `src/lib.rs`)**:
    *   **How**: A collection of spawned tasks that can be awaited. It allows for managing a dynamic set of tasks and waiting for any of them to complete.
    *   **When**: In `start_scan`, it's used to keep track of all the `perform_scan` tasks that have been spawned. This allows the `start_scan` loop to know if there are still active tasks and to await their completion before finishing.

### Reqwest Remarks

`reqwest` is a powerful and ergonomic HTTP client for Rust, built on top of `tokio`. In this crate, it's exclusively used for making HTTP requests to target URLs.

**What `reqwest` is used for:**

*   **Sending HTTP Requests**: The primary purpose is to send various types of HTTP requests (GET, POST, PUT, DELETE, HEAD, OPTIONS, PATCH) to web servers.
*   **Handling HTTP Responses**: Receiving and processing the responses from these requests, including status codes, headers, and body content.
*   **Client Configuration**: Configuring aspects of the HTTP client, such as timeouts and redirect policies.

**How and When `reqwest` is used:**

1.  **`reqwest::Client` (in `src/main.rs` and `src/lib.rs`)**:
    *   **How**: An instance of `reqwest::Client` is created using `Client::builder().build().unwrap()`. This client is designed to be reused across multiple requests for efficiency, as it manages connection pooling and other resources.
    *   **When**: 
        *   In `src/main.rs`, a `Client` is initialized once at the start of the program. This client is then passed to the `start_scan` function.
        *   In `src/lib.rs`, within the `perform_scan` function, the `Client` is used to execute the actual HTTP requests.
        *   In tests, a `Client` is created for each test case to make HTTP requests to the mock server.

2.  **Making HTTP Requests (in `src/lib.rs`)**:
    *   **How**: The `Client` instance provides methods corresponding to HTTP verbs (e.g., `client.get()`, `client.post()`, `client.head()`, etc.). These methods return a `RequestBuilder` which is then used to send the request with `.send().await?`. For `OPTIONS` requests, `client.request(reqwest::Method::OPTIONS, target_url.as_str())` is used to explicitly specify the method.
    *   **When**: Inside the `perform_scan` function, for each `target_url` and `http_method`, an appropriate `reqwest` method is called to send the HTTP request.

3.  **Configuring Redirect Behavior (in `src/main.rs` and `src/lib.rs` tests)**:
    *   **How**: The `Client::builder()` is configured with `.redirect(reqwest::redirect::Policy::none())`.
    *   **When**: This is crucial for a web scanner like `dircrab`. By default, `reqwest` would automatically follow HTTP redirects (like 301, 302). However, `dircrab` needs to explicitly observe these redirect status codes to report them and potentially use the redirect target for further scanning. Disabling automatic redirects ensures that the `perform_scan` function receives the initial response status code.

4.  **Handling Responses (in `src/lib.rs`)**:
    *   **How**: After sending a request, the `res` object (of type `reqwest::Response`) is used to extract information such as:
        *   `res.status()`: To get the HTTP status code.
        *   `res.headers()`: To access response headers (e.g., to get the `Location` header for redirects).
        *   `res.text().await?`: To asynchronously read the response body as text.
    *   **When**: In `perform_scan`, after an HTTP request is sent, the status code is checked for filtering and reporting. The response body is read to count words, characters, and lines, which are also used for filtering and output.

5.  **Error Handling**:
    *   `reqwest` operations return `Result` types, allowing for robust error handling. The `?` operator is used to propagate errors.
    *   The comment `// 404 is a valid HTTP response, not an error in reqwest` highlights that `reqwest` considers any valid HTTP response (even 4xx or 5xx status codes) as a successful *transport* of the response. The application logic then interprets these status codes to determine if the scan result is "interesting" or not.

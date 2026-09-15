//! Hands out a rotating, shuffled image path over HTTP so that any number of
//! simultaneous `fastfetch`/`neofetch` invocations each get a different picture.
//!
//! Usage: custom_neofetch_wallpapers [IMAGE_DIR]
//!   IMAGE_DIR defaults to $HOME/custom_neofetch_wallpapers/img
//!
//! Environment:
//!   NEOFETCH_WALLPAPER_PORT  listen port (default 7777)

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_PORT: u16 = 7777;
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif",
];
/// Cap on the request bytes we are willing to read before answering.
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    let dir = image_dir();

    // A rotating queue: every request pops the front and pushes it to the back,
    // so concurrent callers never collide on the same picture.
    let paths = scan_shuffled(&dir);
    if paths.is_empty() {
        eprintln!("warning: no images found in {}", dir.display());
    } else {
        eprintln!("serving {} images from {}", paths.len(), dir.display());
    }
    let rotation = Arc::new(Mutex::new(VecDeque::from(paths)));

    spawn_hourly_reshuffle(dir, Arc::clone(&rotation));

    let port: u16 = std::env::var("NEOFETCH_WALLPAPER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));

    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("fatal: cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("listening on http://{addr}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let rotation = Arc::clone(&rotation);
                // One short-lived thread per connection; each one finishes in
                // microseconds, so nothing accumulates.
                if let Err(e) = thread::Builder::new()
                    .stack_size(64 * 1024)
                    .spawn(move || handle(stream, &rotation))
                {
                    eprintln!("spawn failed: {e}");
                }
            }
            Err(e) => eprintln!("accept failed: {e}"),
        }
    }
}

/// Pops the next path and immediately re-queues it at the back.
fn next_path(rotation: &Mutex<VecDeque<String>>) -> Option<String> {
    // Recover rather than propagate if a previous handler poisoned the lock;
    // a half-rotated queue is still perfectly usable.
    let mut queue = rotation.lock().unwrap_or_else(|e| e.into_inner());
    let path = queue.pop_front()?;
    queue.push_back(path.clone());
    Some(path)
}

fn handle(mut stream: TcpStream, rotation: &Mutex<VecDeque<String>>) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_nodelay(true);

    if read_request(&mut stream).is_err() {
        return;
    }

    let response = match next_path(rotation) {
        Some(path) => http_response("200 OK", &path),
        None => http_response("503 Service Unavailable", ""),
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Drains the request head. We do not care what was asked for — any method on
/// any path gets the next picture — but the bytes have to be consumed before
/// we reply or the client may see a reset instead of the response.
fn read_request(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut seen = Vec::with_capacity(256);
    let mut chunk = [0u8; 512];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        seen.extend_from_slice(&chunk[..n]);
        if seen.windows(4).any(|w| w == b"\r\n\r\n") || seen.windows(2).any(|w| w == b"\n\n") {
            return Ok(());
        }
        if seen.len() >= MAX_REQUEST_BYTES {
            return Ok(());
        }
    }
}

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {len}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    )
}

fn image_dir() -> PathBuf {
    match std::env::args().nth(1) {
        Some(arg) => PathBuf::from(arg),
        None => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            Path::new(&home).join("custom_neofetch_wallpapers").join("img")
        }
    }
}

/// Lists the images in `dir` in random order. Hidden files (`.DS_Store` and
/// friends) and non-image files are skipped so nothing unrenderable is served.
fn scan_shuffled(dir: &Path) -> Vec<String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("cannot read {}: {e}", dir.display());
            return Vec::new();
        }
    };

    let mut paths: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter(|entry| is_image(&entry.path()))
        .map(|entry| entry.path().to_string_lossy().into_owned())
        .collect();

    shuffle(&mut paths);
    paths
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.as_str()))
}

/// Fisher-Yates driven by xorshift64*. Picking wallpapers needs no better
/// randomness than the clock, and this keeps the binary dependency-free.
fn shuffle<T>(items: &mut [T]) {
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        | 1;

    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    for i in (1..items.len()).rev() {
        items.swap(i, (next() % (i as u64 + 1)) as usize);
    }
}

/// Re-reads and re-shuffles the directory on every hour boundary, matching the
/// `@hourly` cron the Go version used, so newly added pictures show up.
fn spawn_hourly_reshuffle(dir: PathBuf, rotation: Arc<Mutex<VecDeque<String>>>) {
    let spawned = thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || loop {
            thread::sleep(until_next_hour());
            let paths = scan_shuffled(&dir);
            if paths.is_empty() {
                eprintln!("reshuffle found no images in {}; keeping current list", dir.display());
                continue;
            }
            let mut queue = rotation.lock().unwrap_or_else(|e| e.into_inner());
            *queue = VecDeque::from(paths);
        });
    if let Err(e) = spawned {
        eprintln!("could not start hourly reshuffle: {e}");
    }
}

fn until_next_hour() -> Duration {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Duration::from_secs(3600 - (secs % 3600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_cycles_without_repeating_until_exhausted() {
        let rotation = Mutex::new(VecDeque::from(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]));
        let first: Vec<String> = (0..3).filter_map(|_| next_path(&rotation)).collect();
        assert_eq!(first, vec!["a", "b", "c"]);
        // After a full lap the queue is back in its original order.
        assert_eq!(next_path(&rotation).as_deref(), Some("a"));
    }

    #[test]
    fn empty_rotation_yields_nothing() {
        let rotation = Mutex::new(VecDeque::new());
        assert_eq!(next_path(&rotation), None);
    }

    #[test]
    fn only_image_extensions_pass() {
        assert!(is_image(Path::new("/img/makima1.JPG")));
        assert!(is_image(Path::new("/img/lucy2.webp")));
        assert!(!is_image(Path::new("/img/.DS_Store")));
        assert!(!is_image(Path::new("/img/notes.txt")));
        assert!(!is_image(Path::new("/img/noextension")));
    }

    #[test]
    fn shuffle_preserves_every_element() {
        let mut items: Vec<u32> = (0..64).collect();
        shuffle(&mut items);
        items.sort_unstable();
        assert_eq!(items, (0..64).collect::<Vec<u32>>());
    }

    #[test]
    fn response_length_matches_body() {
        let response = http_response("200 OK", "/img/asa1.jpg");
        assert!(response.contains("Content-Length: 13\r\n"));
        assert!(response.ends_with("\r\n\r\n/img/asa1.jpg"));
    }

    #[test]
    fn next_hour_is_within_the_hour() {
        let d = until_next_hour().as_secs();
        assert!(d > 0 && d <= 3600);
    }
}

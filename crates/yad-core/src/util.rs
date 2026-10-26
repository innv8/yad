use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Wait before retry number `attempt` (1-based): 1, 2, 4, 8, 16 s, with up to 25% jitter.
pub fn backoff(attempt: u32) -> Duration {
    let base = 1u64 << attempt.saturating_sub(1).min(4);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64;
    let jitter_ms = nanos % (base * 250 + 1);
    Duration::from_millis(base * 1000 + jitter_ms)
}

pub fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string()))
}

/// Finds ffmpeg: `YAD_FFMPEG`, next to the running binary (bundled sidecar), the macOS app
/// bundle's Resources folder, then `PATH`.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = std::env::var_os("YAD_FFMPEG").map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join(exe_name), dir.join("../Resources").join(exe_name)] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // GUI apps on macOS start with a minimal PATH; Homebrew is the usual install location.
    dirs.extend([PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]);
    dirs.into_iter().map(|d| d.join(exe_name)).find(|p| p.is_file())
}

/// Token bucket shared by every connection. `None` means unlimited.
pub struct RateLimiter {
    state: Mutex<(Option<u64>, f64, Instant)>,
}

impl RateLimiter {
    pub fn new(rate: Option<u64>) -> Self {
        RateLimiter { state: Mutex::new((rate, 0.0, Instant::now())) }
    }

    pub fn set_rate(&self, rate: Option<u64>) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *s = (rate, 0.0, Instant::now());
    }

    /// Takes `n` bytes from the bucket and sleeps when it is overdrawn.
    pub async fn acquire(&self, n: usize) {
        let wait = {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(rate) = s.0 else { return };
            let rate = rate as f64;
            let now = Instant::now();
            let elapsed = now.duration_since(s.2).as_secs_f64();
            s.2 = now;
            // Allow at most a quarter second of burst.
            s.1 = (s.1 + elapsed * rate).min(rate / 4.0);
            s.1 -= n as f64;
            if s.1 < 0.0 {
                Duration::from_secs_f64(-s.1 / rate)
            } else {
                Duration::ZERO
            }
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        assert!(backoff(1) >= Duration::from_secs(1) && backoff(1) < Duration::from_millis(1300));
        assert!(backoff(3) >= Duration::from_secs(4));
        assert!(backoff(9) >= Duration::from_secs(16) && backoff(9) < Duration::from_secs(21));
    }

    #[tokio::test]
    async fn rate_limiter_slows_down() {
        let rl = RateLimiter::new(Some(100_000));
        let start = Instant::now();
        for _ in 0..10 {
            rl.acquire(10_000).await;
        }
        // 100 KB at 100 KB/s with a 25 KB burst allowance: at least ~0.7 s.
        assert!(start.elapsed() >= Duration::from_millis(650), "{:?}", start.elapsed());
        let free = RateLimiter::new(None);
        let start = Instant::now();
        free.acquire(10_000_000).await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Generic proactive rate limiter that enforces a minimum interval between requests.
/// Requests exceeding the rate are delayed (not dropped) via tokio::time::sleep.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    min_interval: Duration,
    last_request: Arc<Mutex<Instant>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the specified minimum interval between requests.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_request: Arc::new(Mutex::new(Instant::now() - min_interval)),
        }
    }

    /// Create a rate limiter allowing `requests_per_second` requests per second.
    /// Panics if `requests_per_second` is zero or negative.
    pub fn per_second(requests_per_second: f64) -> Self {
        assert!(
            requests_per_second > 0.0,
            "requests_per_second must be positive, got {requests_per_second}"
        );
        let interval_ms = (1000.0 / requests_per_second) as u64;
        Self::new(Duration::from_millis(interval_ms))
    }

    /// Acquire permission to make a request. Sleeps if needed to enforce the rate limit.
    pub async fn acquire(&self) {
        let mut last = self.last_request.lock().await;
        let elapsed = last.elapsed();

        if elapsed < self.min_interval {
            let wait = self.min_interval - elapsed;
            drop(last); // Release lock while sleeping
            tokio::time::sleep(wait).await;
            let mut last = self.last_request.lock().await;
            *last = Instant::now();
        } else {
            *last = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_first_acquire_does_not_delay() {
        let limiter = RateLimiter::new(Duration::from_secs(1));
        let start = Instant::now();
        limiter.acquire().await;
        // First acquire should be near-instant (min_interval already elapsed from init)
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_rapid_acquires_enforce_delay() {
        let limiter = RateLimiter::new(Duration::from_millis(100));
        limiter.acquire().await;
        let start = Instant::now();
        limiter.acquire().await;
        // Second acquire should wait ~100ms
        assert!(start.elapsed() >= Duration::from_millis(80));
    }

    #[tokio::test]
    async fn test_zero_interval_no_delay() {
        let limiter = RateLimiter::new(Duration::from_millis(0));
        let start = Instant::now();
        for _ in 0..10 {
            limiter.acquire().await;
        }
        // All 10 acquires should be near-instant
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_per_second_constructor() {
        let limiter = RateLimiter::per_second(10.0); // 100ms interval
        limiter.acquire().await;
        let start = Instant::now();
        limiter.acquire().await;
        // Should wait ~100ms
        assert!(start.elapsed() >= Duration::from_millis(80));
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let limiter = RateLimiter::new(Duration::from_millis(50));
        let limiter1 = limiter.clone();
        let limiter2 = limiter.clone();

        let start = Instant::now();
        let (_, _) = tokio::join!(async move { limiter1.acquire().await }, async move {
            limiter2.acquire().await
        },);
        // At least one of the two should have waited
        assert!(start.elapsed() >= Duration::from_millis(30));
    }
}

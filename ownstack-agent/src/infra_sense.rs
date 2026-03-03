//! InfraSense — System monitoring and resource tracking.
//!
//! Provides metrics about the host system (CPU, RAM, disk) for workspace
//! health monitoring. Uses the `sysinfo` crate for cross-platform support.
//!
//! Rust port of `ownstack-python/app/tools/sense.py` (Docker telemetry),
//! adapted for host-level monitoring without Docker dependency.

use std::path::Path;
use tracing::debug;

// ─── System Metrics ─────────────────────────────────────────────

/// Snapshot of system resource usage.
#[derive(Debug, Clone)]
pub struct SystemMetrics {
    /// Total system memory in bytes.
    pub memory_total: u64,
    /// Used system memory in bytes.
    pub memory_used: u64,
    /// Memory usage percentage (0.0 - 100.0).
    pub memory_percent: f64,
    /// Disk usage for the workspace partition.
    pub disk_available: u64,
    pub disk_total: u64,
    pub disk_percent: f64,
    /// Number of logical CPU cores.
    pub cpu_count: usize,
}

impl SystemMetrics {
    /// Check if the system is under memory pressure.
    pub fn is_memory_critical(&self) -> bool {
        self.memory_percent > 90.0
    }

    /// Check if disk space is low.
    pub fn is_disk_critical(&self) -> bool {
        self.disk_percent > 95.0
    }

    /// Get a compact summary string.
    pub fn summary(&self) -> String {
        format!(
            "RAM: {:.1}% ({}/{}MB) | Disk: {:.1}% ({}/{}GB) | CPUs: {}",
            self.memory_percent,
            self.memory_used / 1_048_576,
            self.memory_total / 1_048_576,
            self.disk_percent,
            (self.disk_total - self.disk_available) / 1_073_741_824,
            self.disk_total / 1_073_741_824,
            self.cpu_count,
        )
    }
}

// ─── InfraSense ─────────────────────────────────────────────────

/// Monitors system resources and workspace health.
pub struct InfraSense;

impl InfraSense {
    /// Collect current system metrics.
    ///
    /// Uses basic OS APIs. Falls back to reasonable defaults on error.
    pub fn collect_metrics(workspace: &Path) -> SystemMetrics {
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        // Try to get disk info for the workspace
        let (disk_total, disk_available) = get_disk_usage(workspace);
        let disk_percent = if disk_total > 0 {
            ((disk_total - disk_available) as f64 / disk_total as f64) * 100.0
        } else {
            0.0
        };

        // Memory: use platform-specific methods
        let (memory_total, memory_used) = get_memory_usage();
        let memory_percent = if memory_total > 0 {
            (memory_used as f64 / memory_total as f64) * 100.0
        } else {
            0.0
        };

        let metrics = SystemMetrics {
            memory_total,
            memory_used,
            memory_percent,
            disk_available,
            disk_total,
            disk_percent,
            cpu_count,
        };

        debug!("InfraSense: {}", metrics.summary());
        metrics
    }

    /// Quick health check — returns warnings if resources are critical.
    pub fn health_check(workspace: &Path) -> Vec<String> {
        let metrics = Self::collect_metrics(workspace);
        let mut warnings = Vec::new();

        if metrics.is_memory_critical() {
            warnings.push(format!(
                "⚠️ Memory critical: {:.1}% used ({} MB free)",
                metrics.memory_percent,
                (metrics.memory_total - metrics.memory_used) / 1_048_576
            ));
        }

        if metrics.is_disk_critical() {
            warnings.push(format!(
                "⚠️ Disk critical: {:.1}% used ({} GB free)",
                metrics.disk_percent,
                metrics.disk_available / 1_073_741_824
            ));
        }

        warnings
    }
}

// ─── Platform-specific helpers ──────────────────────────────────

#[cfg(target_os = "windows")]
fn get_disk_usage(path: &Path) -> (u64, u64) {
    use std::os::windows::ffi::OsStrExt;

    // Get the drive root
    let root = path
        .components()
        .next()
        .map(|c| {
            let mut s = c.as_os_str().to_os_string();
            s.push("\\");
            s
        })
        .unwrap_or_else(|| std::ffi::OsStr::new("C:\\").to_os_string());

    let wide_root: Vec<u16> = root.encode_wide().chain(std::iter::once(0)).collect();

    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_bytes_available: *mut u64,
            total_bytes: *mut u64,
            total_free_bytes: *mut u64,
        ) -> i32;
    }

    let mut free_bytes_available: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;

    let result = unsafe {
        GetDiskFreeSpaceExW(
            wide_root.as_ptr(),
            &mut free_bytes_available,
            &mut total_bytes,
            &mut total_free_bytes,
        )
    };

    if result != 0 {
        (total_bytes, free_bytes_available)
    } else {
        (0, 0)
    }
}

#[cfg(not(target_os = "windows"))]
fn get_disk_usage(path: &Path) -> (u64, u64) {
    // Unix: use statvfs
    use std::process::Command;
    let output = Command::new("df")
        .args(["-B1", "--output=size,avail"])
        .arg(path)
        .output();

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let last_line = text.lines().last().unwrap_or("");
            let parts: Vec<&str> = last_line.split_whitespace().collect();
            if parts.len() >= 2 {
                let total = parts[0].parse::<u64>().unwrap_or(0);
                let avail = parts[1].parse::<u64>().unwrap_or(0);
                return (total, avail);
            }
            (0, 0)
        }
        Err(_) => (0, 0),
    }
}

#[cfg(target_os = "windows")]
fn get_memory_usage() -> (u64, u64) {
    use std::mem;

    #[repr(C)]
    struct MemoryStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    extern "system" {
        fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut status: MemoryStatusEx = unsafe { mem::zeroed() };
    status.dw_length = mem::size_of::<MemoryStatusEx>() as u32;

    let result = unsafe { GlobalMemoryStatusEx(&mut status) };
    if result != 0 {
        let total = status.ull_total_phys;
        let used = total - status.ull_avail_phys;
        (total, used)
    } else {
        (0, 0)
    }
}

#[cfg(not(target_os = "windows"))]
fn get_memory_usage() -> (u64, u64) {
    use std::fs;
    // Parse /proc/meminfo on Linux
    match fs::read_to_string("/proc/meminfo") {
        Ok(content) => {
            let mut total: u64 = 0;
            let mut available: u64 = 0;
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    total = parse_meminfo_value(line);
                } else if line.starts_with("MemAvailable:") {
                    available = parse_meminfo_value(line);
                }
            }
            (total * 1024, (total - available) * 1024) // kB to bytes
        }
        Err(_) => (0, 0),
    }
}

#[cfg(not(target_os = "windows"))]
fn parse_meminfo_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_collect_metrics_runs() {
        let dir = tempdir().unwrap();
        let metrics = InfraSense::collect_metrics(dir.path());

        assert!(metrics.cpu_count >= 1);
        assert!(metrics.memory_percent >= 0.0);
        assert!(metrics.memory_percent <= 100.0);
    }

    #[test]
    fn test_health_check_no_panic() {
        let dir = tempdir().unwrap();
        let warnings = InfraSense::health_check(dir.path());
        // Just verify it runs without panic
        debug!("Health warnings: {:?}", warnings);
    }

    #[test]
    fn test_metrics_summary() {
        let metrics = SystemMetrics {
            memory_total: 16 * 1_073_741_824, // 16 GB
            memory_used: 8 * 1_073_741_824,   // 8 GB
            memory_percent: 50.0,
            disk_total: 500 * 1_073_741_824, // 500 GB
            disk_available: 250 * 1_073_741_824,
            disk_percent: 50.0,
            cpu_count: 8,
        };

        let summary = metrics.summary();
        assert!(summary.contains("RAM: 50.0%"));
        assert!(summary.contains("CPUs: 8"));
    }

    #[test]
    fn test_critical_thresholds() {
        let normal = SystemMetrics {
            memory_total: 16_000,
            memory_used: 8_000,
            memory_percent: 50.0,
            disk_total: 100,
            disk_available: 50,
            disk_percent: 50.0,
            cpu_count: 4,
        };
        assert!(!normal.is_memory_critical());
        assert!(!normal.is_disk_critical());

        let critical = SystemMetrics {
            memory_total: 16_000,
            memory_used: 15_000,
            memory_percent: 93.75,
            disk_total: 100,
            disk_available: 3,
            disk_percent: 97.0,
            cpu_count: 4,
        };
        assert!(critical.is_memory_critical());
        assert!(critical.is_disk_critical());
    }
}

// TRC-019: GPU monitor - some fields for future display

use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct GpuInfo {
    pub name: String,
    pub utilization: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub temperature: Option<u32>,
    pub power_draw: Option<f32>,
}

#[allow(dead_code)]
impl GpuInfo {
    pub fn memory_percent(&self) -> f32 {
        if self.memory_total_mb == 0 {
            0.0
        } else {
            (self.memory_used_mb as f32 / self.memory_total_mb as f32) * 100.0
        }
    }

    pub fn format_memory(&self) -> String {
        let used_gb = self.memory_used_mb as f32 / 1024.0;
        let total_gb = self.memory_total_mb as f32 / 1024.0;
        format!("{:.1}/{:.1}G", used_gb, total_gb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct GpuMonitor {
    gpus: Vec<GpuInfo>,
    vendor: GpuVendor,
    last_refresh: Instant,
    refresh_interval: Duration,
    available: bool,
}

impl Default for GpuMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl GpuMonitor {
    pub fn new() -> Self {
        let mut monitor = Self {
            gpus: Vec::new(),
            vendor: GpuVendor::Unknown,
            last_refresh: Instant::now() - Duration::from_secs(10),
            refresh_interval: Duration::from_secs(2),
            available: false,
        };
        
        monitor.detect_vendor();
        monitor.refresh();
        monitor
    }

    fn detect_vendor(&mut self) {
        if Command::new("nvidia-smi")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            self.vendor = GpuVendor::Nvidia;
            self.available = true;
        } else if Command::new("rocm-smi")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            self.vendor = GpuVendor::Amd;
            self.available = true;
        } else {
            self.vendor = GpuVendor::Unknown;
            self.available = false;
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn vendor(&self) -> GpuVendor {
        self.vendor
    }

    pub fn gpus(&self) -> &[GpuInfo] {
        &self.gpus
    }

    pub fn primary_gpu(&self) -> Option<&GpuInfo> {
        self.gpus.first()
    }

    pub fn should_refresh(&self) -> bool {
        self.last_refresh.elapsed() >= self.refresh_interval
    }

    pub fn refresh(&mut self) {
        if !self.available {
            return;
        }

        self.last_refresh = Instant::now();

        match self.vendor {
            GpuVendor::Nvidia => self.refresh_nvidia(),
            GpuVendor::Amd => self.refresh_amd(),
            GpuVendor::Unknown => {}
        }
    }

    fn refresh_nvidia(&mut self) {
        let output = Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
                "--format=csv,noheader,nounits",
            ])
            .output();

        let Ok(output) = output else {
            self.gpus.clear();
            return;
        };

        if !output.status.success() {
            self.gpus.clear();
            return;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.gpus = stdout
            .lines()
            .filter_map(Self::parse_nvidia_line)
            .collect();
    }

    fn parse_nvidia_line(line: &str) -> Option<GpuInfo> {
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 4 {
            return None;
        }

        let name = parts[0].to_string();
        let utilization = parts[1].parse::<f32>().unwrap_or(0.0);
        let memory_used_mb = parts[2].parse::<u64>().unwrap_or(0);
        let memory_total_mb = parts[3].parse::<u64>().unwrap_or(0);
        let temperature = parts.get(4).and_then(|s| s.parse::<u32>().ok());
        let power_draw = parts.get(5).and_then(|s| s.parse::<f32>().ok());

        Some(GpuInfo {
            name,
            utilization,
            memory_used_mb,
            memory_total_mb,
            temperature,
            power_draw,
        })
    }

    fn refresh_amd(&mut self) {
        let output = Command::new("rocm-smi")
            .args(["--showuse", "--showmeminfo", "vram", "--showtemp", "--csv"])
            .output();

        let Ok(output) = output else {
            self.gpus.clear();
            return;
        };

        if !output.status.success() {
            self.gpus.clear();
            return;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.gpus = Self::parse_amd_output(&stdout);
    }

    fn parse_amd_output(output: &str) -> Vec<GpuInfo> {
        let mut gpus = Vec::new();
        let lines: Vec<&str> = output.lines().collect();
        
        if lines.len() < 2 {
            return gpus;
        }

        for line in lines.iter().skip(1) {
            let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if parts.len() >= 4 {
                let gpu = GpuInfo {
                    name: format!("AMD GPU {}", parts.first().unwrap_or(&"0")),
                    utilization: parts.get(1).and_then(|s| s.trim_end_matches('%').parse().ok()).unwrap_or(0.0),
                    memory_used_mb: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                    memory_total_mb: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
                    temperature: parts.get(4).and_then(|s| s.parse().ok()),
                    power_draw: None,
                };
                gpus.push(gpu);
            }
        }

        gpus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_info_default() {
        let gpu = GpuInfo::default();
        assert!(gpu.name.is_empty());
        assert_eq!(gpu.utilization, 0.0);
        assert_eq!(gpu.memory_percent(), 0.0);
    }

    #[test]
    fn test_gpu_memory_percent() {
        let gpu = GpuInfo {
            name: "Test GPU".to_string(),
            utilization: 50.0,
            memory_used_mb: 4096,
            memory_total_mb: 8192,
            temperature: Some(65),
            power_draw: Some(150.0),
        };
        assert!((gpu.memory_percent() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_gpu_format_memory() {
        let gpu = GpuInfo {
            name: "Test GPU".to_string(),
            utilization: 50.0,
            memory_used_mb: 2048,
            memory_total_mb: 8192,
            temperature: None,
            power_draw: None,
        };
        assert_eq!(gpu.format_memory(), "2.0/8.0G");
    }

    #[test]
    fn test_parse_nvidia_line() {
        let line = "NVIDIA GeForce RTX 4060 Ti, 39, 1173, 16380, 45, 21.42";
        let gpu = GpuMonitor::parse_nvidia_line(line).unwrap();
        
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 4060 Ti");
        assert!((gpu.utilization - 39.0).abs() < 0.01);
        assert_eq!(gpu.memory_used_mb, 1173);
        assert_eq!(gpu.memory_total_mb, 16380);
        assert_eq!(gpu.temperature, Some(45));
        assert!((gpu.power_draw.unwrap() - 21.42).abs() < 0.01);
    }

    #[test]
    fn test_parse_nvidia_line_incomplete() {
        let line = "NVIDIA GPU, 50";
        assert!(GpuMonitor::parse_nvidia_line(line).is_none());
    }

    #[test]
    fn test_gpu_monitor_new() {
        let monitor = GpuMonitor::new();
        assert!(matches!(
            monitor.vendor(),
            GpuVendor::Nvidia | GpuVendor::Amd | GpuVendor::Unknown
        ));
    }

    #[test]
    fn test_gpu_monitor_should_refresh() {
        let mut monitor = GpuMonitor::new();
        monitor.last_refresh = Instant::now() - Duration::from_secs(5);
        assert!(monitor.should_refresh());
        
        monitor.last_refresh = Instant::now();
        assert!(!monitor.should_refresh());
    }
}

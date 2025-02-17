// Detects devices available to the machine
// Details such as CPU, GPU, number of cores and memory.

// TODO:
// [ ] - Verify this works correctly on different machines and OS's

use std::fmt;
use sysinfo::System;

use std::{
    // Remove unused imports
    // net::{SocketAddr, ToSocketAddrs},
    // sync::{
    //     // atomic::{AtomicBool, Ordering},
    //     // Remove unused import
    //     // Arc,
    // },
    // Remove unused import
    thread::available_parallelism,
    time::{Duration, Instant},
};

// Remove unused import
// use sp_core::U256;

use sha2::{Sha256, Digest};
use rand::{thread_rng, Rng};

#[derive(Debug, Clone)]
pub enum GpuPlatform {
    NVIDIA,
    AMD,
    Apple,
    Unknown,
}

#[derive(Debug)]
pub struct DetectedHardware {
    pub gpus: Vec<GpuDevice>,
    pub cpu_info: CpuInfo,
}

#[derive(Debug)]
pub struct GpuDevice {
    pub platform: GpuPlatform,
    pub name: String,
    pub compute_units: usize,
    pub memory_mb: usize,
    pub metal_version: String,
}

#[derive(Debug)]
pub struct CpuInfo {
    pub cores: usize,
    pub threads: usize,
    pub model_name: String,
    pub available_parallelism: u32,
    pub hashrate: f64,
}

impl fmt::Display for DetectedHardware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = available_parallelism().unwrap().get() as u32;

        writeln!(f, "\n🖥️  Hardware Information")?;
        
        // CPU Info
        writeln!(f, "\n📟 CPU")?;
        writeln!(f, "   Model: {}", self.cpu_info.model_name)?;
        writeln!(f, "   Physical Cores: {}", self.cpu_info.cores)?;
        writeln!(f, "   Logical Cores: {}", self.cpu_info.threads)?;
        writeln!(f, "   Available parallelism: {}", p)?;

        // Directly convert and print the hash rate in MH/s
        // let hashrate_mh = self.cpu_info.hashrate / 1_000_000.0; // Convert to MH/s
        // writeln!(f, "   Hash Rate: {:.2} MH/s", hashrate_mh)?; // Print in MH/s
        
        // GPU Info
        if !self.gpus.is_empty() {
            for gpu in &self.gpus {
                writeln!(f, "\n🎮 GPU")?;
                writeln!(f, "   Model: {}", gpu.name)?;
                writeln!(f, "   Architecture: {:?}", gpu.platform)?;
                writeln!(f, "   Compute Cores: {}", gpu.compute_units)?;
                writeln!(f, "   Memory: {} GB", gpu.memory_mb / 1024)?;
                writeln!(f, "   Metal Version: {}", gpu.metal_version)?;
            }
        } else {
            writeln!(f, "   No compatible GPUs found.")?;
        }
        
        Ok(())
    }
}

pub struct HardwareDetector;

impl HardwareDetector {
    pub fn detect() -> DetectedHardware {
        let mut sys = System::new();
        sys.refresh_all();

        DetectedHardware {
            gpus: Self::detect_gpus(),
            cpu_info: Self::detect_cpu(&sys),
        }
    }

    fn detect_cpu(sys: &System) -> CpuInfo {
        let available_parallelism = available_parallelism().unwrap().get() as u32;
        let hashrate = Self::measure_hashrate(5);

        CpuInfo {
            cores: sys.physical_core_count().unwrap_or(0),
            threads: sys.cpus().len(),
            model_name: sys.cpus().first()
                .map(|cpu| cpu.brand().to_string())
                .unwrap_or_else(|| "Unknown CPU".to_string()),
            available_parallelism,
            hashrate,
        }
    }

    #[cfg(target_os = "macos")]
    fn detect_gpus() -> Vec<GpuDevice> {
        let output = std::process::Command::new("system_profiler")
            .arg("SPDisplaysDataType")
            .output()
            .ok();
        
        if let Some(output) = output {
            let output_str = String::from_utf8_lossy(&output.stdout);
            
            // Parse compute cores
            let compute_units = output_str
                .lines()
                .find(|line| line.trim().starts_with("Total Number of Cores:"))
                .and_then(|line| line.split(':').nth(1))
                .and_then(|cores| cores.trim().parse().ok())
                .unwrap_or(0);

            // Parse Metal version
            let metal_version = output_str
                .lines()
                .find(|line| line.trim().starts_with("Metal Support:"))
                .and_then(|line| line.split(':').nth(1))
                .map(|v| v.trim().to_string())
                .unwrap_or_default();

            // Parse GPU name
            let gpu_name = output_str
                .lines()
                .find(|line| line.trim().starts_with("Chipset Model:"))
                .and_then(|line| line.split(':').nth(1))
                .map(|name| name.trim().to_string())
                .unwrap_or("Unknown Apple GPU".to_string());
            
            // Get memory from system_profiler SPMemoryDataType
            let mem_output = std::process::Command::new("system_profiler")
                .arg("SPMemoryDataType")
                .output()
                .ok();

            let memory_gb = if let Some(mem_output) = mem_output {
                let mem_str = String::from_utf8_lossy(&mem_output.stdout);
                mem_str
                    .lines()
                    .find(|line| line.trim().starts_with("Memory:") && line.contains("GB"))
                    .and_then(|line| line.split(':').nth(1))
                    .and_then(|mem| mem.trim().split_whitespace().next())
                    .and_then(|num| num.parse::<usize>().ok())
                    .unwrap_or(0)
            } else {
                0
            };

            vec![GpuDevice {
                platform: GpuPlatform::Apple,
                name: gpu_name,
                compute_units,
                memory_mb: memory_gb * 1024,
                metal_version,
            }]
        } else {
            Vec::new()
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn detect_gpus() -> Vec<GpuDevice> {
        Vec::new()
    }


    // Returns hashrate based on how fast the device hashes over the given duration
    fn measure_hashrate(duration_secs: u64) -> f64 {
        let mut share = Self::generate_random_80_byte_array();
        let start_time = Instant::now();
        let mut hashes: u64 = 0;
        let duration = Duration::from_secs(duration_secs);

        while start_time.elapsed() < duration {
            for _ in 0..10000 {
                Self::hash(&mut share);
                hashes += 1;
            }
        }

        let elapsed_secs = start_time.elapsed().as_secs_f64();

        hashes as f64 / elapsed_secs
    }

    fn hash(share: &mut [u8; 80]) -> Vec<u8> {
        let hash = Sha256::digest(&share).to_vec();
        hash
    }

    fn generate_random_80_byte_array() -> [u8; 80] {
        let mut rng = thread_rng();
        let mut arr = [0u8; 80];
        rng.fill(&mut arr[..]);
        arr
    }   
}


// TODO:
// [x] - check for miner.toml
// [x] - run a default based on devices detected when no miner.toml (tell user what will run)
// [x] - validate the miner.toml config schema - ALREADY DOES THIS?
// [x] - validate miner.toml config to machine

// Note:
// There can be more logical cores than physical cores. We may only want to run threads based on the number of physical cores for efficiency.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::device_detector::DetectedHardware;
use sc_service::Error as ServiceError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeneralConfig {
    pub max_memory_use: Option<usize>, // in MB
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CpuConfig {
    pub use_cpu: bool,
    pub threads: Option<usize>, // This should match the TOML structure
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GpuConfig {
    pub use_gpu: bool,
    pub intensity: u32, // Adjust based on available compute power
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MinerConfig {
    pub general: GeneralConfig,
    pub cpu: CpuConfig,
    pub gpu: GpuConfig,
}

impl MinerConfig {
    // Load from TOML file
    pub fn from_file(path: PathBuf) -> Result<Self, ServiceError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            ServiceError::Other(format!("Failed to read config file: {}", e))
        })?;
        
        let config: MinerConfig = toml::from_str(&content).map_err(|e| {
            ServiceError::Other(format!("Failed to parse TOML: {}", e))
        })?;
        
        Ok(config)
    }
    
    // Validate configuration against available hardware
    pub fn validate(&self, hardware: &DetectedHardware) -> Result<(), String> {
        // Print the detected hardware
        println!("✅ Detected hardware inside validate fn: {:#?}", hardware);

        // Validate CPU configuration
        if self.cpu.use_cpu {
            if let Some(threads) = self.cpu.threads {
                // Ensure threads do not exceed available logical cores
                if threads > hardware.cpu_info.threads {
                    return Err(format!(
                        "Requested {} CPU threads but only {} logical CPU cores detected",
                        threads,
                        hardware.cpu_info.threads
                    ));
                }
            } else {
                return Err("CPU threads must be specified when CPU mining is enabled.".to_string());
            }
        }
        
        // Validate GPU configuration
        if self.gpu.use_gpu {
            if hardware.gpus.is_empty() {
                return Err("GPU mining requested but no compatible GPUs found".to_string());
            }
        }
        
        // Print confirmation of successful validation
        println!("✅ Miner configuration has been validated successfully against the detected hardware.");
        println!("🔧 The miner is ready to run with the current configuration.");

        // Return Ok since validation was successful
        Ok(())
    }
    
    // Print the configuration
    pub fn print(&self) {
        println!("\n🛠️  Mining Configuration Loaded:");
        if let Some(mem) = self.general.max_memory_use {
            println!("   💾 Max Memory: {} MB", mem);
        } else {
            println!("   💾 Max Memory: Not specified");
        }
        println!("   📟 CPU Mining: {}", self.cpu.use_cpu);
        println!("   🧵 CPU Threads: {:?}", self.cpu.threads);
        println!("   🎮 GPU Mining: {}", self.gpu.use_gpu);
        println!("   🎮 GPU Intensity: {}", self.gpu.intensity);
    }

    // Function to create a default MinerConfig based on detected hardware
    pub fn create_default_config(hardware: &DetectedHardware) -> Result<MinerConfig, ServiceError> {
        // Print the detected hardware
        println!("Creating default configuration based on detected hardware: {:?}", hardware);

        // Create a default CPU configuration
        let cpu_config = if hardware.cpu_info.threads > 0 {
            CpuConfig {
                use_cpu: true,
                threads: Some(hardware.cpu_info.threads), // Use detected threads
            }
        } else {
            CpuConfig {
                use_cpu: false,
                threads: None,
            }
        };

        // Create a default GPU configuration
        let gpu_config = if !hardware.gpus.is_empty() {
            GpuConfig {
                use_gpu: true,
                intensity: 100, // Set a default intensity
            }
        } else {
            GpuConfig {
                use_gpu: false,
                intensity: 0,
            }
        };

        // Create a general configuration
        let general_config = GeneralConfig {
            max_memory_use: Some(8192), // Set a default max memory use
        };

        // Return the populated MinerConfig
        Ok(MinerConfig {
            general: general_config,
            cpu: cpu_config,
            gpu: gpu_config,
        })
    }
} 
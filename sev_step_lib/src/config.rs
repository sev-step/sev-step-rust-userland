use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CpufreqPinConfig {
    pub governor: String,
    pub frequency: usize,
}

#[derive(Deserialize)]
pub enum FixCpuFrequency {
    External,
    Cpufreq(CpufreqPinConfig),
}

#[derive(Deserialize)]
pub struct Config {
    /// cpu core to which the vm should be pinned
    pub vm_cpu_core: usize,
    /// ip:port where the "vm-server" binary is listening
    pub vm_server_address: String,
    /// ip:port where QEMU's qmp interface is reachable
    pub qemu_qmp_address: String,
    /// method for fixating the cpu frequncy of the vm core
    pub fix_cpu_frequency: FixCpuFrequency,
}

pub fn parse_config(config_file_path: &str) -> Result<Config> {
    let config = fs::read_to_string(config_file_path)
        .context(format!("failed to read config from {}", config_file_path))?;

    toml::from_str(&config).context("failed to parse config file")
}

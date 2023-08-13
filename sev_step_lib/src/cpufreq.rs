//!
//! Thin wrapper around the file based cpufreq interface exposed by the Linux kernel
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Return base file path of the cpufreq interface for the given cpu
/// # Arguments
/// * `cpu` logical cpu id
fn cpufreq_basepath(cpu: usize) -> PathBuf {
    PathBuf::from(Path::new(&format!(
        "/sys/devices/system/cpu/cpu{}/cpufreq/",
        cpu
    )))
}

enum Parameters {
    ScalingGovernor,
    ScalingMinFreq,
    ScalingMaxFreq,
}

impl ToString for Parameters {
    fn to_string(&self) -> String {
        match self {
            Parameters::ScalingGovernor => "scaling_governor".to_string(),
            Parameters::ScalingMinFreq => "scaling_min_freq".to_string(),
            Parameters::ScalingMaxFreq => "scaling_max_freq".to_string(),
        }
    }
}

/// Update the given paramter to the given value and check that the change was successful
/// # Arguments
/// * `basepath`: path to `cpufreq` directory, as obtained by [`cpufreq_basepath`]
/// * `p`: paramter that should be updated
/// * `value`: new value for `p`
fn write_param_and_check(basepath: &PathBuf, p: &Parameters, value: &str) -> Result<()> {
    let file_path = basepath.join(p.to_string());
    let mut file =
        File::open(&file_path).context(format!("failed config file {:?}", &file_path))?;

    //write config option
    file.write_all(value.as_bytes()).context(format!(
        "failed to write config value {} to {:?}",
        value, file_path
    ))?;

    //check if succesful by reading again
    let mut current_config_value = String::new();
    file.read_to_string(&mut current_config_value)
        .context(format!("failed to read from config file {:?}", file_path))?;
    if current_config_value.ne(value) {
        bail!(
            "error changing {} to value {}, value still stuck at {}",
            p.to_string(),
            value,
            current_config_value
        );
    }

    Ok(())
}

/// Pins the frequency of given cpu using the supplied governor
/// #Arguments
/// * `cpu`: logical cpu id
/// * `governor`: scaling governor
/// * `freq`: used as new value for `scaling_max_freq` as well as `scaling_min_freq`
pub fn pin_cpu_freq(cpu: usize, governor: &str, freq: &str) -> Result<()> {
    let p = cpufreq_basepath(cpu);
    if !p.exists() {
        bail!("{:?} does not exists. Either cpufreq is not available on this system or logical cpud id {} is out of bounds",p,cpu);
    }

    for (parameter, new_value) in [
        (Parameters::ScalingGovernor, governor),
        (Parameters::ScalingMinFreq, freq),
        (Parameters::ScalingMaxFreq, freq),
    ] {
        write_param_and_check(&p, &parameter, new_value).context(format!(
            "failed to update {} to {}",
            parameter.to_string(),
            new_value
        ))?;
    }
    Ok(())
}

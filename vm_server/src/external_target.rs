//!Provides abstraction for executing an external command, monitoring its input and output.
//! The command is expected to execute in two phases: a setup and a payload phase
//! See comments on `InitCustomTargetReq` for more details

use crate::assembly_target::RunnableTarget;
use anyhow::{anyhow, Context, Result};
use log::debug;
use nix::sys::signal;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::channel;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct ExternalTarget {
    key_value_pairs: HashMap<String, String>,
    child_stdin: ChildStdin,
    child_stdout_thread: JoinHandle<()>,
    child_process_id: u32,
}

impl ExternalTarget {
    /// line on stdout that marks the end of the setup phase
    const MAKER_END_SETUP: &'static str = "VMSERVER::SETUP_DONE";
    /// prefix on stdout that marks a <name> <value> pair
    const PREFIX_KEY_VALUE_PAIR: &'static str = "VMSERVER::VAR";
    /// line on stdin that marks the start of the payload phase
    const INPUT_CMD_START: &'static str = "VMSERVER::START";

    ///Starts the external program and waits for the end of the setup phase (see comment on `InitCustomTargetReq`)
    /// # Arguments
    /// - `working_dir`: working dir of the external program
    /// - `cmd`: path to the external program relative to `working_dir`
    /// - `args`: arguments passed to the external program. See https://doc.rust-lang.org/std/process/struct.Command.html#method.args
    /// formatting
    pub fn new(working_dir: String, cmd: String, args: Vec<String>) -> Result<ExternalTarget> {
        let cmd_absolute = Path::new(&working_dir).join(cmd);

        let child_process = Command::new(cmd_absolute)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .current_dir(working_dir)
            .args(args)
            .spawn()?;
        let child_id = child_process.id();
        debug!("pid of child process: {}", child_process.id());
        let stdin = child_process
            .stdin
            .ok_or(anyhow!("failed to capture stdin"))?;
        let stdout = BufReader::new(
            child_process
                .stdout
                .ok_or(anyhow!("failed to capture stdout"))?,
        );

        let start_timestamp = Instant::now();
        let max_setup_duration = Duration::from_secs(30);

        //monitor stdout of child for `ExternalTarget::MAKER_END_SETUP` and `ExternalTarget::PREFIX_KEY_VALUE_PAIR`

        let (key_value_sender, key_value_receiver) = channel();

        let stdout_thread = thread::spawn(move || {
            println!("starting background reading thread");
            let mut key_value_pairs = HashMap::new();
            let mut setup_successful = false;

            for line in stdout.lines() {
                if !setup_successful {
                    if start_timestamp.elapsed() > max_setup_duration {
                        key_value_sender
                            .send(Err(anyhow!(
                                "externalTarget timed out waiting for end of setup phase"
                            )))
                            .unwrap();
                    }

                    let line = line.expect("failed to read line");
                    if line.starts_with(ExternalTarget::MAKER_END_SETUP) {
                        setup_successful = true;
                        key_value_sender.send(Ok(key_value_pairs.clone())).unwrap();
                        continue;
                    } else if line.starts_with(ExternalTarget::PREFIX_KEY_VALUE_PAIR) {
                        let tokens: Vec<_> = line.split(" ").collect();
                        if tokens.len() != 3 {
                            panic!("expected 3 tokens, got \"{:?}\"", tokens);
                        }
                        key_value_pairs.insert(tokens[1].to_string(), tokens[2].to_string());
                    }
                } else {
                    //past setup phase, simply drain stdout
                    let line = line.expect("failed to read line");
                    debug!("process send line to stdout: {}", line);
                }
            }
        });

        println!("waiting for background thread to send start signal");
        let setup_phase_values = key_value_receiver.recv()?.context(format!(
            "external target terminated before \"{}\" marker value has been emitted",
            ExternalTarget::MAKER_END_SETUP
        ))?;
        println!("background thread send values");

        Ok(ExternalTarget {
            key_value_pairs: setup_phase_values,
            child_stdout_thread: stdout_thread,
            child_stdin: stdin,
            child_process_id: child_id,
        })
    }

    ///Name and content of the variables captured during the program's setup phase
    pub fn get_key_value_pairs(&self) -> &HashMap<String, String> {
        &self.key_value_pairs
    }
}

impl RunnableTarget for ExternalTarget {
    unsafe fn run(&mut self) -> Result<()> {
        //send start marker to child_process
        debug!("writing start marker on external target's stdin");
        self.child_stdin
            .write_fmt(format_args!("{}\n", ExternalTarget::INPUT_CMD_START))?;
        self.child_stdin.flush()?;

        Ok(())
    }
    unsafe fn stop(self) -> Result<()> {
        kill(Pid::from_raw(self.child_process_id as i32), signal::SIGKILL)?;
        self.child_stdout_thread
            .join()
            .expect("failed to join stdout thread. TODO: handle this cleanly");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_setup_phase() -> Result<()> {
        let mut p = ExternalTarget::new(
            "/home/luca/sev-step/victims/dummy_victim".to_string(),
            "./a.out".to_string(),
            vec![],
        )?;
        assert_eq!(p.key_value_pairs.get("var_1").unwrap(), "value_var_1");
        assert_eq!(p.key_value_pairs.get("var_2").unwrap(), "value_var_2");

        unsafe {
            p.run().unwrap();
        }

        Ok(())
    }
}

use std::{collections::HashSet, fmt::Display, str::FromStr};

use crate::SevStep;
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use crossbeam::channel::Receiver;
use log::debug;
use sev_step_lib::{
    single_stepper::{
        BuildStepHistogram, EventHandler, RetrackGPASet, SkipIfNotOnTargetGPAs,
        StopAfterNSingleStepsHandler, TargetedStepper,
    },
    types::kvm_page_track_mode,
    vmserver_client::{self, *},
};

pub trait Test {
    fn get_name(&self) -> String;
    fn get_description(&self) -> &str;
    fn run(&self) -> Result<()>;
}

///This enum describes all known tests
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestName {
    SetupTeardown,
    PageTrackPresent,
    PageTrackWrite,
    PageTrackExec,
    SingleStepNopSlide,
}

impl FromStr for TestName {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "SetupTeardown" => Ok(Self::SetupTeardown),
            "PageTrackPresent" => Ok(Self::PageTrackPresent),
            "PageTrackWrite" => Ok(Self::PageTrackWrite),
            "PageTrackExec" => Ok(Self::PageTrackExec),
            "SingleStepNopSlide" => Ok(Self::SingleStepNopSlide),
            _ => Err("invalid TestName value"),
        }
    }
}

impl TestName {
    pub fn instantiate(
        &self,
        abort_chan: Receiver<()>,
        server_addr: String,
        apic_timer_value: Option<u32>,
    ) -> Result<Box<dyn Test>> {
        match &self {
            TestName::SetupTeardown => Ok(Box::new(SetupTeardownTest::new(abort_chan))),
            TestName::PageTrackPresent => Ok(Box::new(CommonPageTrackTest::new(
                abort_chan,
                kvm_page_track_mode::KVM_PAGE_TRACK_ACCESS,
                server_addr,
            )?)),
            TestName::PageTrackWrite => Ok(Box::new(CommonPageTrackTest::new(
                abort_chan,
                kvm_page_track_mode::KVM_PAGE_TRACK_WRITE,
                server_addr,
            )?)),
            TestName::PageTrackExec => Ok(Box::new(CommonPageTrackTest::new(
                abort_chan,
                kvm_page_track_mode::KVM_PAGE_TRACK_EXEC,
                server_addr,
            )?)),
            TestName::SingleStepNopSlide => {
                let apic_timer_value = apic_timer_value.ok_or(anyhow!(
                    "SingleStepNopSlide requires apic_timer_value but got None"
                ))?;
                Ok(Box::new(SingleStepNopSlideTest::new(
                    abort_chan,
                    server_addr,
                    apic_timer_value,
                )))
            }
        }
    }
}

impl Display for TestName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestName::SetupTeardown => write!(f, "SetupTeardown"),
            TestName::PageTrackPresent => write!(f, "PageTrackPresent"),
            TestName::PageTrackWrite => write!(f, "PageTrackWrite"),
            TestName::PageTrackExec => write!(f, "PageTrackExec"),
            TestName::SingleStepNopSlide => write!(f, "SingleStepNopSlide"),
        }
    }
}

///Group similar tests together to easily test whole subsystems
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestGroup {
    ///This group contains tests
    All,
    ///This group contains test related to testing the kernel space to user space channel
    Basic,
    ///This group contains all page fault tracking related tests
    PageFault,
    ///This group contains all single stepping related tests
    SingleStepping,
}

impl Into<Vec<TestName>> for TestGroup {
    fn into(self) -> Vec<TestName> {
        match self {
            TestGroup::All => vec![
                TestName::SetupTeardown,
                TestName::PageTrackWrite,
                TestName::PageTrackPresent,
                TestName::PageTrackExec,
                TestName::SingleStepNopSlide,
            ],
            TestGroup::Basic => vec![TestName::SetupTeardown],
            TestGroup::PageFault => vec![
                TestName::PageTrackWrite,
                TestName::PageTrackPresent,
                TestName::PageTrackExec,
            ],
            TestGroup::SingleStepping => vec![TestName::SingleStepNopSlide],
        }
    }
}

impl FromStr for TestGroup {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "All" => Ok(Self::All),
            "Basic" => Ok(Self::Basic),
            "PageFault" => Ok(Self::PageFault),
            "SingleStepping" => Ok(Self::SingleStepping),
            _ => Err("invalid TestGroup value"),
        }
    }
}

impl Display for TestGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestGroup::All => write!(f, "All Tests"),
            TestGroup::Basic => write!(f, "Basic"),
            TestGroup::PageFault => write!(f, "PageFault"),
            TestGroup::SingleStepping => write!(f, "SingleStepping"),
        }
    }
}

pub struct SetupTeardownTest {
    name: TestName,
    description: String,
    abort_chan: Receiver<()>,
}

impl SetupTeardownTest {
    pub fn new(abort_chan: Receiver<()>) -> Self {
        SetupTeardownTest {
            name: TestName::SetupTeardown,
            description: "Repeatedly opens and closes an API connection".to_string(),
            abort_chan,
        }
    }
}

impl Test for SetupTeardownTest {
    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn get_description(&self) -> &str {
        &self.description
    }

    fn run(&self) -> Result<()> {
        for _i in 0..10 {
            let mut _sev_step = SevStep::new(false, self.abort_chan.clone())
                .context("failed to open API connection")?;
            drop(_sev_step);
        }
        Ok(())
    }
}

pub struct CommonPageTrackTest {
    track_type: kvm_page_track_mode,
    abort_chan: Receiver<()>,
    /// address at which the server inside vm is reachable. format: http://hostname:port
    server_addr: String,
    name: TestName,
    description: String,
}

impl CommonPageTrackTest {
    fn new(
        abort_chan: Receiver<()>,
        track_type: kvm_page_track_mode,
        server_addr: String,
    ) -> Result<Self> {
        let name = match track_type {
            kvm_page_track_mode::KVM_PAGE_TRACK_WRITE => TestName::PageTrackWrite,
            kvm_page_track_mode::KVM_PAGE_TRACK_ACCESS => TestName::PageTrackPresent,
            kvm_page_track_mode::KVM_PAGE_TRACK_EXEC => TestName::PageTrackExec,
            _ => bail!(format!(
                "CommonPageTrackTest does not support track mode {:?}",
                track_type
            )),
        };
        Ok(CommonPageTrackTest {
            track_type,
            abort_chan,
            server_addr,
            name,
            description:
                "Track read access to two pages that are accessed in an alternating manner"
                    .to_string(),
        })
    }
}

impl Test for CommonPageTrackTest {
    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn get_description(&self) -> &str {
        &self.description
    }

    fn run(&self) -> Result<()> {
        let init_args = PingPongPageTrackReq {
            access_type: self.track_type.try_into().context(format!(
                "failed to convert {:?} to AccessType",
                &self.track_type
            ))?,
        };

        const REPS: usize = 10;
        for _i in 0..REPS {
            debug!("iteration {}/{}", _i + 1, REPS);

            let sev_step = SevStep::new(false, self.abort_chan.clone())
                .context("failed to open API connection")?;
            debug!("Instantiated API connection");
            let victim_prog = vmserver_client::pagetrack_victim_init(&self.server_addr, &init_args)
                .context("failed to init pagetrack victim")?;
            debug!("Received PageTrackVictim description : {:?}", victim_prog);

            let mut retrack_gpas = RetrackGPASet::new(
                HashSet::from([victim_prog.gpa1, victim_prog.gpa2]),
                self.track_type,
                Some(victim_prog.iterations as usize),
            );
            let handler_chain: Vec<&mut dyn EventHandler> = vec![&mut retrack_gpas];

            let a = self.server_addr.clone();
            let handler = TargetedStepper::new(
                sev_step,
                handler_chain,
                self.track_type,
                vec![victim_prog.gpa1, victim_prog.gpa2],
                move || {
                    debug!("requesting page track victim start");
                    vmserver_client::pagetrack_victim_start(&a)
                        .context("failed to start page track victim in trigger fn")
                },
            );
            debug!("Calling handler.run()");
            handler.run()?;

            debug!("Requesting page track victim teardown");
            vmserver_client::pagetrack_victim_teardown(&self.server_addr)
                .context("failed to teardown pagetrack victim")?;
        }

        Ok(())
    }
}

pub struct SingleStepNopSlideTest {
    abort_chan: Receiver<()>,
    /// address at which the server inside vm is reachable. format: http://hostname:port
    server_addr: String,
    name: TestName,
    description: String,
    timer_value: u32,
}

impl SingleStepNopSlideTest {
    pub fn new(abort_chan: Receiver<()>, server_addr: String, timer_value: u32) -> Self {
        SingleStepNopSlideTest {
            abort_chan,
            server_addr,
            name: TestName::SingleStepNopSlide,
            description: "Use page fault tracking to figure out when NopSlide is executed. Then activate single stepping".to_string(),
            timer_value,
        }
    }
}

impl Test for SingleStepNopSlideTest {
    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn get_description(&self) -> &str {
        &self.description
    }

    fn run(&self) -> Result<()> {
        let mut _sev_step = SevStep::new(true, self.abort_chan.clone())?;

        let victim_prog = single_step_victim_init(&self.server_addr, SingleStepTarget::NopSlide)
            .context("failed to init NopSlide victim")?;

        let mut targetter = SkipIfNotOnTargetGPAs::new(
            &[victim_prog.gpa],
            kvm_page_track_mode::KVM_PAGE_TRACK_EXEC,
            self.timer_value,
        );
        let mut step_histogram = BuildStepHistogram::new();
        let expected_rip_values = victim_prog
            .expected_offsets
            .iter()
            .map(|v| v + victim_prog.vaddr)
            .collect();
        let mut stop_after = StopAfterNSingleStepsHandler::new(
            victim_prog.expected_offsets.len(),
            Some(expected_rip_values),
        );
        let handler_chain: Vec<&mut dyn EventHandler> =
            vec![&mut targetter, &mut step_histogram, &mut stop_after];

        let server_addr = self.server_addr.clone();
        let stepper = TargetedStepper::new(
            _sev_step,
            handler_chain,
            kvm_page_track_mode::KVM_PAGE_TRACK_ACCESS,
            vec![victim_prog.gpa],
            move || {
                vmserver_client::single_step_victim_start(&server_addr, SingleStepTarget::NopSlide)
            },
        );

        stepper.run()?;

        let step_sizes = step_histogram.get_values();

        if step_sizes.len() == 1 && step_sizes.contains_key(&1) {
            Ok(())
        } else if step_sizes.len() == 2
            && step_sizes.contains_key(&1)
            && step_sizes.contains_key(&0)
            && *step_sizes.get(&1).unwrap() >= victim_prog.expected_offsets.len() as u64
        {
            Ok(())
        } else {
            bail!(
                "Did not successfully single step target. Require {} single steps and NO multi steps. Step Histogram : {}",
                victim_prog.expected_offsets.len(),
                step_histogram
            )
        }
    }
}

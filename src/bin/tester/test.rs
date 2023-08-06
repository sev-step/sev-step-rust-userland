use std::{collections::HashSet, fmt::Display, str::FromStr};

use crate::SevStep;
use clap::ValueEnum;
use crossbeam::channel::Receiver;
use log::debug;
use rust_userland::{
    single_stepper::{EventHandler, RetrackGPASet, TargetedStepper},
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
}

impl FromStr for TestName {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "SetupTeardown" => Ok(Self::SetupTeardown),
            "PageTrackPresent" => Ok(Self::PageTrackPresent),
            "PageTrackWrite" => Ok(Self::PageTrackWrite),
            "PageTrackExec" => Ok(Self::PageTrackExec),
            _ => Err("invalid TestName value"),
        }
    }
}

impl TestName {
    pub fn instantiate(&self, abort_chan: Receiver<()>, server_addr: String) -> Box<dyn Test> {
        match &self {
            TestName::SetupTeardown => Box::new(SetupTeardownTest::new(abort_chan)),
            TestName::PageTrackPresent => {
                Box::new(PageTrackPresentTest::new(abort_chan, server_addr))
            }
            TestName::PageTrackWrite => todo!(),
            TestName::PageTrackExec => todo!(),
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
        }
    }
}
///Group similar tests together to easily test whole subsystems
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TestGroup {
    All,
    Basic,
    PageFault,
    SingleStepping,
}

impl Into<Vec<TestName>> for TestGroup {
    fn into(self) -> Vec<TestName> {
        match self {
            TestGroup::All => vec![TestName::SetupTeardown],
            TestGroup::Basic => vec![TestName::SetupTeardown],
            TestGroup::PageFault => vec![],
            TestGroup::SingleStepping => vec![],
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
    group: TestGroup,
    description: String,
    abort_chan: Receiver<()>,
}

impl SetupTeardownTest {
    pub fn new(abort_chan: Receiver<()>) -> Self {
        SetupTeardownTest {
            name: TestName::SetupTeardown,
            group: TestGroup::Basic,
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
}

impl CommonPageTrackTest {
    fn new(abort_chan: Receiver<()>, track_type: kvm_page_track_mode, server_addr: String) -> Self {
        CommonPageTrackTest {
            track_type,
            abort_chan,
            server_addr,
        }
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

pub struct PageTrackPresentTest {
    name: TestName,
    group: TestGroup,
    description: String,
    test: CommonPageTrackTest,
}

impl PageTrackPresentTest {
    pub fn new(abort_chan: Receiver<()>, server_addr: String) -> Self {
        PageTrackPresentTest {
            name: TestName::PageTrackPresent,
            group: TestGroup::PageFault,
            description:
                "Track read access to two pages that are accessed in an alternating manner"
                    .to_string(),
            test: CommonPageTrackTest::new(
                abort_chan,
                kvm_page_track_mode::KVM_PAGE_TRACK_ACCESS,
                server_addr,
            ),
        }
    }
}

impl Test for PageTrackPresentTest {
    fn get_name(&self) -> String {
        self.name.to_string()
    }

    fn get_description(&self) -> &str {
        &self.description
    }

    fn run(&self) -> Result<()> {
        self.test.run()
    }
}

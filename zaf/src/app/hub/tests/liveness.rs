//! What the liveness tests stand on: run folders on disk, a board with one claimed task, and the
//! gather and the redraw's read against a process table the test controls.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::SystemTime;

use super::super::*;
use crate::app::actions::SliceRecords;
use crate::app::report::PriceTable;
use crate::app::spawn::{self, runs};
use crate::domain::{Instance, Ticket, TicketId, TicketState};
use crate::fakes::{FakeBoard, FakeForge, FakeVcs};
use crate::ports::{Board, Holder};

const EPIC: u64 = 103;
const TASK: u64 = 143;
const START: &str = "1000";

/// The processes that have ended.
pub type Dead = RefCell<BTreeSet<u32>>;

pub fn instance(number: u32) -> Instance {
    Instance {
        role: "programmer".into(),
        epic: TicketId::from(EPIC),
        number: number.into(),
    }
}

/// A run folder for `instance`, held by process `pid`.
pub fn run_folder(state_dir: &Path, instance: &Instance, pid: u32) {
    let dir = state_dir.join("runs").join(spawn::slug(instance));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let record = runs::LaunchRecord {
        instance: instance.to_string(),
        tasks: Vec::new(),
        reserved_at: "2026-11-12T10:00:00Z".to_owned(),
        started_at: Some("2026-11-12T10:00:01Z".to_owned()),
    };
    std::fs::write(
        dir.join(runs::LAUNCH_FILE),
        serde_json::to_string(&record).expect("json"),
    )
    .expect("launch");
    std::fs::write(dir.join(runs::HOLDER_FILE), format!("{pid} {START}\n")).expect("pid");
}

/// A board with one epic and one task claimed by instance 7.
pub fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_challenged_epic(TicketId::from(EPIC), "Epic");
    board.add_ticket(Ticket {
        id: TicketId::from(TASK),
        epic: Some(TicketId::from(EPIC)),
        title: "A task".into(),
        body: "".into(),
        state: Some(TicketState::Ready),
        url: "".into(),
        kind: None,
    });
    board
        .claim(TicketId::from(TASK), &instance(7))
        .expect("claims");
    board
}

/// The gathered state, with the processes named in `dead` gone.
pub fn gathered(state_dir: &Path, board: &FakeBoard, dead: &Dead) -> Snapshot {
    let alive = |holder: &Holder| !dead.borrow().contains(&holder.pid);
    let (forge, vcs) = (FakeForge::default(), FakeVcs::default());
    let records = SliceRecords::new(state_dir);
    gather(&HubInputs {
        board,
        forge: &forge,
        vcs: &vcs,
        records: &records,
        state_dir,
        alive: &alive,
        slots_total: 2,
        ledger: &[],
        project: "ways",
        root: state_dir,
        prices: &PriceTable::default(),
        models: &BTreeMap::new(),
        now: SystemTime::UNIX_EPOCH,
        places: None,
    })
    .expect("gathers")
}

/// What the live hub does on a redraw: reads the process table against the snapshot's claims.
pub fn refresh(snapshot: &mut Snapshot, state_dir: &Path, dead: &Dead) -> Result<(), HubError> {
    let alive = |holder: &Holder| !dead.borrow().contains(&holder.pid);
    let live = Liveness::read(state_dir, 2, &snapshot.claims, &alive)?;
    snapshot.apply(&live);
    Ok(())
}

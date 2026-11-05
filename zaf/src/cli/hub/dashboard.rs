//! `zaf hub --port <n>`: the hub's frame as a web page on the LAN (docs/zaf-actions.md §5).
//!
//! The page's server is `app::hub`'s; this is the loop that feeds it. It looks at the reports
//! file's metadata, as the terminal does, and a new report is the only thing that reads the board
//! and publishes a frame. Between reports it reads neither the board nor the network, and it
//! publishes nothing: the page's clocks do not move.

use std::io::Write;
use std::net::IpAddr;
use std::path::Path;
use std::time::SystemTime;

use super::super::exit::{BACKEND, REFUSED};
use super::{DEFAULT_WIDTH, POLL, Pace};
use crate::app::hub::{BindError, Dashboard, HubError, Mark, Snapshot, draw, lan_url};

/// Binds every interface on `port`, or says why not on stderr. A port in use is a refusal of the
/// state; any other failure is the machine's.
pub fn bind(port: u16, path: &str) -> Result<Dashboard, u8> {
    Dashboard::bind(port).map_err(|error| {
        eprintln!("zaf {path}: {error}");
        match error {
            BindError::InUse { .. } => REFUSED,
            BindError::Failed { .. } => BACKEND,
        }
    })
}

/// Serves `dashboard` until `pace` says to stop: the first frame from `gather`, then one more for
/// each new report. The page's LAN URL is printed on `out` once the server is up.
pub fn serve<G, W>(
    out: &mut W,
    dashboard: Dashboard,
    address: IpAddr,
    mut gather: G,
    state_dir: &Path,
    pace: &mut dyn Pace,
) -> Result<(), String>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
    W: Write,
{
    let io = |error: std::io::Error| error.to_string();
    let width = pace.width();
    // The mark is taken before the first read of the board, so a report made during it is seen.
    let mut mark = Mark::read(state_dir);
    let first = gather(pace.now()).map_err(|error| error.to_string())?;
    let port = dashboard.port();
    let page = dashboard.start(&draw(&first, width));
    writeln!(out, "{}", lan_url(address, port)).map_err(io)?;
    out.flush().map_err(io)?;
    while pace.wait() {
        let seen = Mark::read(state_dir);
        if seen == mark {
            continue;
        }
        // The mark is taken first: a board that fails is tried again at the next report, not at
        // every look, and the page keeps the last frame.
        mark = seen;
        if let Ok(snapshot) = gather(pace.now()) {
            page.publish(&draw(&snapshot, width));
        }
    }
    Ok(())
}

/// The dashboard's wait: no keys, no terminal; it runs until the process is stopped.
pub struct Forever;

impl Pace for Forever {
    fn now(&mut self) -> SystemTime {
        SystemTime::now()
    }

    fn width(&mut self) -> usize {
        DEFAULT_WIDTH
    }

    fn wait(&mut self) -> bool {
        std::thread::sleep(POLL);
        true
    }
}

#[cfg(test)]
mod tests;

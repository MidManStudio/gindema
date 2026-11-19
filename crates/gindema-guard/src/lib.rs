// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "lib.rs"
// ============================================================================

//! Runtime anti-tamper watchdog: cheap timing and checksum checks that flip
//! a shared, one-way [`GuardState`] to "polluted" on detection, so that
//! dependent values read back a plausible decoy ([`Guarded`]) instead of
//! the program crashing or the detection being otherwise observable.
//! Applied selectively — to specific values worth protecting — never to a
//! whole game loop.
//!
//! This first pass is a real, working core: [`TimingWindow`] (elapsed-time
//! checks — the debugger/breakpoint signal), [`Watch`] (byte-region
//! checksum checks — the "did this change" signal), [`GuardState`] (the
//! shared, sticky, one-directional pollution flag both of the above write
//! to), and [`Guarded`] (the decoy-on-pollution read). See
//! `docs/gindema-guard.md`'s "Known limits" for what a determined,
//! informed attacker could still do against each of these, and for why
//! `gindema-ptr` isn't used by any of them yet.
//!
//! # Example
//!
//! ```
//! use gindema_guard::{GuardState, Guarded, TimingWindow, Watch};
//! use std::time::Duration;
//!
//! let state = GuardState::new();
//!
//! // Watch a region of save-critical bytes for tampering.
//! let save_data = vec![1u8, 2, 3, 4];
//! let watch = Watch::new(&save_data);
//! assert!(watch.check(&save_data, &state));
//!
//! // A value that reads back as a believable decoy once anything pollutes
//! // the shared state, instead of the program crashing.
//! let player_gold = Guarded::new(500i64, 500i64);
//! assert_eq!(player_gold.read(&state), 500);
//!
//! // A timing window around something that should be quick.
//! let window = TimingWindow::start();
//! assert!(!window.elapsed_at_least(Duration::from_secs(5)));
//! ```

mod checksum;
mod guarded;
mod state;
mod timing;
mod watch;

pub use checksum::checksum;
pub use guarded::Guarded;
pub use state::GuardState;
pub use timing::TimingWindow;
pub use watch::Watch;

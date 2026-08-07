//! Drives `.timer` units. Unlike `.device` (driven by external kernel events)
//! or `.service`/`.socket` (driven by forked processes / fds), a timer's only
//! job is to wake up at the right `Instant` and (re)activate its `Unit=`.
//!
//! This module owns a single background thread and a min-heap of
//! `(Instant, UnitId)` wakeups, guarded by a `Condvar` so the thread sleeps
//! exactly until the next wakeup instead of busy-polling (and can be woken
//! early if a sooner wakeup gets scheduled while it's already waiting).
//!
//! Lifecycle, mirroring `crate::device_events`:
//! - `start_timer_thread` must be called once, early at boot (before any
//!   `.timer` unit gets activated), so `boot_instant()` reflects "when
//!   lksystem started listening" and the scheduler thread exists to receive
//!   `register()`/`cancel()` calls.
//! - `TimerState::activate()` (see `crate::units::unit`) calls `register()`
//!   whenever a `.timer` unit transitions into `Started` -- either during the
//!   normal boot activation pass, or later via `lksystemctl start`/`restart`.
//! - `TimerState::deactivate()` calls `cancel()` to drop any pending wakeup.
//! - After a timer fires, if `OnUnitActiveSec=` is set, this module
//!   re-schedules itself -- that's what makes a timer repeat (e.g. an hourly
//!   backup job) without anyone else having to re-trigger it.

use crate::runtime_info::ArcMutRuntimeInfo;
use crate::units::*;
use chrono::Local;
use lksystem::ui;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScheduledKind {
    Fixed,
    Calendar(CalendarSpec),
}

#[derive(Clone, Eq, PartialEq)]
struct ScheduledEntry {
    at: Instant,
    unit_id: UnitId,
    kind: ScheduledKind,
}

impl Ord for ScheduledEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.at
            .cmp(&other.at)
            .then_with(|| self.unit_id.cmp(&other.unit_id))
    }
}
impl PartialOrd for ScheduledEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct Scheduler {
    // BinaryHeap is a max-heap; wrap in Reverse so the *soonest* wakeup sits
    // at the top.
    heap: Mutex<BinaryHeap<Reverse<ScheduledEntry>>>,
    condvar: Condvar,
}

static SCHEDULER: OnceLock<Scheduler> = OnceLock::new();
static BOOT_INSTANT: OnceLock<Instant> = OnceLock::new();

fn scheduler() -> &'static Scheduler {
    SCHEDULER.get_or_init(|| Scheduler {
        heap: Mutex::new(BinaryHeap::new()),
        condvar: Condvar::new(),
    })
}

/// The instant `OnBootSec=` is measured from. Set on the first call (which
/// `start_timer_thread` makes early during boot, before any timer unit gets
/// the chance to activate) and never changes after that.
pub fn boot_instant() -> Instant {
    *BOOT_INSTANT.get_or_init(Instant::now)
}

pub fn start_timer_thread(run_info: ArcMutRuntimeInfo) {
    boot_instant();
    let _ = scheduler();
    std::thread::spawn(move || run_scheduler_loop(run_info));
}

/// Called when a `.timer` unit transitions into `Started`. Computes and
/// enqueues the wakeup(s) implied by `OnBootSec=`/`OnActiveSec=`.
/// `OnUnitActiveSec=` is intentionally not scheduled here -- like systemd, it
/// only starts counting *after* the first firing (see `fire_timer` below),
/// so a timer that only has `OnUnitActiveSec=` set and nothing else will sit
/// `Started` but never fire on its own -- same as systemd would.
pub fn register(unit_id: &UnitId, conf: &TimerConfig) {
    let now = Instant::now();
    if let Some(on_boot) = conf.on_boot_sec {
        let at = boot_instant() + on_boot;
        // If OnBootSec= has already elapsed by the time this unit actually
        // got activated (e.g. it was waiting on slow dependencies), fire it
        // as soon as possible instead of never.
        let at = if at < now { now } else { at };
        schedule(unit_id.clone(), at, ScheduledKind::Fixed);
    }
    if let Some(on_active) = conf.on_active_sec {
        schedule(unit_id.clone(), now + on_active, ScheduledKind::Fixed);
    }
    if let Some(on_calendar) = &conf.on_calendar {
        if let Some(at) = calendar_target_instant(on_calendar) {
            schedule(
                unit_id.clone(),
                at,
                ScheduledKind::Calendar(on_calendar.clone()),
            );
        }
    }
}

/// Drop any wakeup(s) currently pending for this unit. Safe to call even if
/// none are pending (e.g. a timer with none of OnBootSec=/OnActiveSec=/
/// OnUnitActiveSec= set, or one that already fired and had nothing to
/// reschedule).
pub fn cancel(unit_id: &UnitId) {
    let sched = scheduler();
    let mut heap = sched.heap.lock().unwrap();
    let remaining: BinaryHeap<_> = heap
        .drain()
        .filter(|Reverse(entry)| &entry.unit_id != unit_id)
        .collect();
    *heap = remaining;
    // No need to notify: removing entries never makes the next wakeup sooner.
}

fn schedule(unit_id: UnitId, at: Instant, kind: ScheduledKind) {
    let sched = scheduler();
    {
        let mut heap = sched.heap.lock().unwrap();
        heap.push(Reverse(ScheduledEntry { at, unit_id, kind }));
    }
    // Wake the scheduler thread in case this new entry is sooner than
    // whatever it was already sleeping until (or in case it was sleeping
    // indefinitely on an empty heap).
    sched.condvar.notify_all();
}

fn run_scheduler_loop(run_info: ArcMutRuntimeInfo) {
    loop {
        let sched = scheduler();
        let mut heap = sched.heap.lock().unwrap();

        // Block here until the heap's earliest entry is due.
        loop {
            match heap.peek().cloned() {
                None => {
                    heap = sched.condvar.wait(heap).unwrap();
                }
                Some(Reverse(entry)) => {
                    if let ScheduledKind::Calendar(spec) = &entry.kind {
                        if let Some(desired_at) = calendar_target_instant(spec) {
                            if desired_at != entry.at {
                                heap.pop();
                                heap.push(Reverse(ScheduledEntry {
                                    at: desired_at,
                                    unit_id: entry.unit_id.clone(),
                                    kind: entry.kind.clone(),
                                }));
                                continue;
                            }
                        }
                    }

                    let now = Instant::now();
                    if entry.at <= now {
                        break;
                    }
                    // Copy the wait duration out before moving `heap` into
                    // wait_timeout below -- `entry` borrows from `*heap`, so
                    // its last use has to be strictly before that move.
                    let wait_for = entry.at - now;
                    let (new_heap, timeout) = sched.condvar.wait_timeout(heap, wait_for).unwrap();
                    heap = new_heap;
                    if timeout.timed_out() {
                        // Either really due now, or a spurious wakeup -- loop
                        // back to peek() and re-check either way.
                    }
                }
            }
        }

        // Drain every entry that's due (there can be more than one if
        // several timers land on the same tick).
        let now = Instant::now();
        let mut fired = Vec::new();
        loop {
            let due = match heap.peek() {
                Some(Reverse(entry)) => entry.at <= now,
                None => false,
            };
            if !due {
                break;
            }
            let Reverse(entry) = heap.pop().unwrap();
            fired.push(entry.unit_id);
        }
        drop(heap);

        for timer_id in fired {
            fire_timer(&timer_id, &run_info);
        }
    }
}

fn fire_timer(timer_id: &UnitId, run_info: &ArcMutRuntimeInfo) {
    let (target_id, on_unit_active_sec, on_calendar) = {
        let run_info_locked = run_info.read().unwrap();
        let unit = match run_info_locked.unit_table.get(timer_id) {
            // The timer unit could have been removed (e.g. `lksystemctl
            // remove`) between being scheduled and firing.
            None => return,
            Some(unit) => unit,
        };
        if let Specific::Timer(specific) = &unit.specific {
            specific.state.write().unwrap().last_trigger = Some(Instant::now());
            (
                specific.conf.unit.clone(),
                specific.conf.on_unit_active_sec,
                specific.conf.on_calendar.clone(),
            )
        } else {
            return;
        }
    };

    ui::log(format!(
        "Timer {} elapsed, activating {}",
        timer_id.name,
        target_id.name
    ));
    {
        let run_info_locked = run_info.read().unwrap();
        if let Err(e) = activate_unit(
            target_id.clone(),
            &*run_info_locked,
            ActivationSource::Regular,
        ) {
            // Not fatal or even unusual: e.g. the target might still be
            // starting from a previous firing, or have unmet dependencies of
            // its own. Only trace it, same as device_events does for its
            // dependents.
            ui::log(format!(
                "Timer {} fired but {} did not (yet) activate: {}",
                timer_id.name,
                target_id.name,
                e
            ));
        }
    }

    if let Some(on_calendar) = &on_calendar {
        if let Some(at) = calendar_target_instant(on_calendar) {
            schedule(
                timer_id.clone(),
                at,
                ScheduledKind::Calendar(on_calendar.clone()),
            );
        }
    }
    if let Some(interval) = on_unit_active_sec {
        schedule(timer_id.clone(), Instant::now() + interval, ScheduledKind::Fixed);
    }
}

fn calendar_target_instant(spec: &CalendarSpec) -> Option<Instant> {
    let at = crate::units::next_calendar_instant(spec)?;
    let local_now = Local::now();
    let duration = if at <= local_now {
        std::time::Duration::from_secs(0)
    } else {
        (at - local_now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(0))
    };
    Some(Instant::now() + duration)
}

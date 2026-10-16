//! Tick-based simulation of a Zerg economy.
//!
//! Workers mine on their own, the simulation puts workers on gas only while the
//! build still needs gas, and every queen injects its hatchery whenever it can.
//! The only decisions left to the search are what to start, and in which order.

use crate::data::{self, Id, N, Source};
use crate::plan::{self, Plan};
use std::rc::Rc;

pub const TICKS_PER_SEC: u32 = 4;
const DT: f64 = 1.0 / TICKS_PER_SEC as f64;

pub fn ticks(seconds: f64) -> u32 {
    (seconds * TICKS_PER_SEC as f64).round() as u32
}

/// `m:ss` game time.
pub fn clock(tick: u32) -> String {
    let secs = tick / TICKS_PER_SEC;
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub struct Config {
    /// Seconds a drone walks to the expansion; it mines nothing meanwhile.
    pub expand_travel: f64,
    pub max_expansions: u16,
    pub max_macro_hatcheries: u16,
    pub max_extractors: u16,
    pub max_queens: u16,
    /// Give up on any build that runs longer than this.
    pub horizon: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ready,
    Wait,
    Never,
}

#[derive(Clone)]
pub struct Hatch {
    larva: u8,
    larva_timer: u32,
    inject_done: Option<u32>,
    busy_until: u32,
}

pub struct Step {
    pub tick: u32,
    /// Supply counter as shown in game when the step starts (half units).
    pub supply: u32,
    pub id: Id,
    prev: Option<Rc<Step>>,
}

#[derive(Clone)]
pub struct State {
    pub tick: u32,
    pub minerals: f64,
    pub gas: f64,
    pub supply_used: u32,
    pub supply_cap: u32,
    // Counts are u8 to keep states small; the search holds millions of them.
    // `Plan::oversized` rejects goals that could overflow them.
    /// Completed and alive, per item (individual units).
    done: [u8; N],
    /// In production, per item (actions).
    pub pending: [u8; N],
    /// Actions ever started, per item.
    pub started: [u8; N],
    pub hatches: Vec<Hatch>,
    queens: Vec<f64>,
    /// Tick each production building frees up. Fits in u16: the search horizon
    /// is 20 minutes, well under u16::MAX ticks.
    building_busy: [u16; N],
    jobs: Vec<(u32, Id)>,
    gas_spent: u32,
    gas_budget: u32,
    expand_cost: f64,
    pub log: Option<Rc<Step>>,
}

impl State {
    pub fn start(plan: &Plan, cfg: &Config) -> State {
        let mut done = [0u8; N];
        for id in data::all() {
            done[id as usize] = plan::initial_count(id) as u8;
        }
        State {
            tick: 0,
            minerals: 50.0,
            gas: 0.0,
            supply_used: plan::START_DRONES as u32 * 2,
            supply_cap: data::def(Id::Hatchery).provides + data::def(Id::Overlord).provides,
            done,
            pending: [0; N],
            started: [0; N],
            hatches: vec![Hatch { larva: 3, larva_timer: 0, inject_done: None, busy_until: 0 }],
            queens: Vec::new(),
            building_busy: [0; N],
            jobs: Vec::new(),
            gas_spent: 0,
            gas_budget: plan.gas,
            // The drone stops mining while it walks to the expansion.
            expand_cost: cfg.expand_travel * data::MINERAL_RATE,
            log: None,
        }
    }

    pub fn count(&self, id: Id) -> u16 {
        self.done[id as usize] as u16
    }

    pub fn pending(&self, id: Id) -> u16 {
        self.pending[id as usize] as u16
    }

    pub fn started(&self, id: Id) -> u16 {
        self.started[id as usize] as u16
    }

    pub fn count_with_pending(&self, id: Id) -> u16 {
        self.count(id) + self.pending(id) * data::def(id).yields
    }

    pub fn bases(&self) -> u16 {
        self.count(Id::Hatchery)
    }

    pub fn pending_supply(&self) -> u32 {
        data::all()
            .map(|id| self.pending[id as usize] as u32 * data::def(id).provides)
            .sum()
    }

    /// Gas the build still has to mine, beyond what is banked.
    pub fn gas_still_needed(&self) -> f64 {
        self.gas_budget as f64 - self.gas_spent as f64 - self.gas
    }

    pub fn gas_workers(&self) -> u32 {
        if self.gas_still_needed() <= 0.0 {
            return 0;
        }
        let slots = self.count(Id::Extractor) as u32 * data::WORKERS_PER_GEYSER;
        slots.min(self.count(Id::Drone) as u32)
    }

    pub fn mineral_income(&self) -> f64 {
        let workers = self.count(Id::Drone) as u32 - self.gas_workers();
        let patches = self.bases() as u32 * data::PATCHES_PER_BASE;
        let full = workers.min(2 * patches);
        let third = (workers - full).min(patches);
        full as f64 * data::MINERAL_RATE + third as f64 * data::MINERAL_RATE_THIRD
    }

    pub fn gas_income(&self) -> f64 {
        self.gas_workers() as f64 * data::GAS_RATE
    }

    pub fn larva(&self) -> u32 {
        self.hatches.iter().map(|h| h.larva as u32).sum()
    }

    fn mineral_cost(&self, id: Id) -> f64 {
        let base = data::def(id).minerals as f64;
        if id == Id::Hatchery { base + self.expand_cost } else { base }
    }

    /// Advances the game by one tick.
    pub fn advance(&mut self) {
        self.minerals += self.mineral_income() * DT;
        self.gas += self.gas_income() * DT;
        self.tick += 1;

        let larva_interval = ticks(data::LARVA_INTERVAL);
        for h in &mut self.hatches {
            if h.larva < data::NATURAL_LARVA_CAP {
                h.larva_timer += 1;
                if h.larva_timer >= larva_interval {
                    h.larva += 1;
                    h.larva_timer = 0;
                }
            }
            if h.inject_done.is_some_and(|t| t <= self.tick) {
                h.larva = (h.larva + data::INJECT_LARVA).min(data::MAX_LARVA);
                h.inject_done = None;
            }
        }

        let hatch_count = self.hatches.len();
        for (i, energy) in self.queens.iter_mut().enumerate() {
            *energy = (*energy + data::QUEEN_ENERGY_REGEN * DT).min(data::QUEEN_MAX_ENERGY);
            let hatch = &mut self.hatches[i % hatch_count];
            if *energy >= data::INJECT_ENERGY && hatch.inject_done.is_none() {
                *energy -= data::INJECT_ENERGY;
                hatch.inject_done = Some(self.tick + ticks(data::INJECT_TIME));
            }
        }

        if self.jobs.iter().any(|&(t, _)| t <= self.tick) {
            let now = self.tick;
            let (finished, running): (Vec<_>, Vec<_>) = self.jobs.drain(..).partition(|&(t, _)| t <= now);
            self.jobs = running;
            for (_, id) in finished {
                self.complete(id);
            }
        }
    }

    fn complete(&mut self, id: Id) {
        let def = data::def(id);
        self.pending[id as usize] -= 1;
        self.done[id as usize] += def.yields as u8;
        self.supply_cap += def.provides;
        match id {
            Id::Hatchery | Id::MacroHatchery => self.hatches.push(Hatch {
                larva: data::NEW_HATCH_LARVA,
                larva_timer: 0,
                inject_done: None,
                busy_until: 0,
            }),
            Id::Queen => self.queens.push(data::QUEEN_START_ENERGY),
            _ => {}
        }
    }

    fn has(&self, id: Id) -> bool {
        self.done[id as usize] > 0
    }

    /// Whether `id` can start now, could start later without any other
    /// decision being made, or never will.
    pub fn status(&self, id: Id) -> Status {
        let def = data::def(id);
        let mut status = Status::Ready;
        let mut need = |s: Status| {
            if s == Status::Never || (s == Status::Wait && status == Status::Ready) {
                status = s;
            }
        };
        let waiting_on = |item: Id| {
            if self.pending[item as usize] > 0 { Status::Wait } else { Status::Never }
        };

        for &r in def.requires {
            if !self.has(r) {
                need(waiting_on(r));
            }
        }

        match def.source {
            Source::Larva => {
                if self.larva() == 0 {
                    need(Status::Wait);
                }
            }
            Source::Worker => {
                if !self.has(Id::Drone) {
                    need(waiting_on(Id::Drone));
                }
            }
            Source::Morph(src) => {
                if !self.has(src) {
                    need(waiting_on(src));
                }
            }
            Source::TownHall => {
                if !self.hatches.iter().any(|h| h.busy_until <= self.tick) {
                    need(Status::Wait);
                }
            }
            Source::Building(b) => {
                if !self.has(b) {
                    need(waiting_on(b));
                } else if self.building_busy[b as usize] as u32 > self.tick {
                    need(Status::Wait);
                }
            }
        }

        if def.supply > 0 {
            let used = self.supply_used + def.supply;
            if used > self.supply_cap.min(data::MAX_SUPPLY) {
                if used <= (self.supply_cap + self.pending_supply()).min(data::MAX_SUPPLY) {
                    need(Status::Wait);
                } else {
                    need(Status::Never);
                }
            }
        }

        if self.minerals < self.mineral_cost(id) {
            if self.count(Id::Drone) > 0 { need(Status::Wait) } else { need(Status::Never) }
        }
        if self.gas < def.gas as f64 {
            let more_gas_coming = self.gas_budget > self.gas_spent
                && (self.has(Id::Extractor) || self.pending[Id::Extractor as usize] > 0);
            if more_gas_coming { need(Status::Wait) } else { need(Status::Never) }
        }

        status
    }

    /// Starts `id`, which must be `Ready`.
    pub fn begin(&mut self, id: Id) {
        let def = data::def(id);
        let build = ticks(def.time);
        self.log = Some(Rc::new(Step { tick: self.tick, supply: self.supply_used, id, prev: self.log.take() }));

        self.minerals -= self.mineral_cost(id);
        self.gas -= def.gas as f64;
        self.gas_spent += def.gas;
        match def.source {
            Source::Larva => {
                let hatch = self.hatches.iter_mut().max_by_key(|h| h.larva).unwrap();
                hatch.larva -= 1;
            }
            Source::Worker => {
                self.done[Id::Drone as usize] -= 1;
                self.supply_used -= data::def(Id::Drone).supply;
            }
            Source::Morph(src) => self.done[src as usize] -= 1,
            Source::TownHall => {
                let now = self.tick;
                let hatch = self.hatches.iter_mut().find(|h| h.busy_until <= now).unwrap();
                hatch.busy_until = now + build;
            }
            Source::Building(b) => self.building_busy[b as usize] = (self.tick + build) as u16,
        }
        self.supply_used += def.supply;
        self.jobs.push((self.tick + build, id));
        self.pending[id as usize] += 1;
        self.started[id as usize] += 1;
    }

    /// Waits until `id` can start, then starts it. `None` if it never can
    /// within the horizon.
    pub fn wait_and_begin(&self, id: Id, horizon: u32) -> Option<State> {
        let mut s = self.clone();
        loop {
            match s.status(id) {
                Status::Ready => {
                    s.begin(id);
                    return Some(s);
                }
                Status::Never => return None,
                Status::Wait if s.tick >= horizon => return None,
                Status::Wait => s.advance(),
            }
        }
    }

    /// One child per action in `ids`, each started at the earliest moment it
    /// can be. Simulates forward once rather than once per action.
    pub fn branch(&self, ids: &[Id], horizon: u32) -> Vec<State> {
        let mut children = Vec::with_capacity(ids.len());
        let mut todo = ids.to_vec();
        let mut s = self.clone();
        loop {
            todo.retain(|&id| match s.status(id) {
                Status::Ready => {
                    let mut child = s.clone();
                    child.begin(id);
                    children.push(child);
                    false
                }
                Status::Never => false,
                Status::Wait => true,
            });
            if todo.is_empty() || s.tick >= horizon {
                return children;
            }
            s.advance();
        }
    }

    pub fn goal_met(&self, plan: &Plan) -> bool {
        data::all().all(|id| self.count(id) >= plan.goal[id as usize])
    }

    /// Runs the clock until the goal is complete.
    pub fn finish(&self, plan: &Plan, horizon: u32) -> Option<u32> {
        let mut s = self.clone();
        while !s.goal_met(plan) {
            if s.tick >= horizon || s.jobs.is_empty() {
                return None;
            }
            s.advance();
        }
        Some(s.tick)
    }

    /// Earliest tick at which an in-progress `id` completes.
    pub fn first_finish(&self, id: Id) -> Option<u32> {
        self.jobs.iter().filter(|&&(_, j)| j == id).map(|&(t, _)| t).min()
    }

    /// In-progress work as (item, finish tick), in a canonical order.
    pub fn jobs_sorted(&self) -> Vec<(Id, u32)> {
        let mut jobs: Vec<_> = self.jobs.iter().map(|&(t, id)| (id, t)).collect();
        jobs.sort_unstable();
        jobs
    }

    pub fn last_finish(&self, id: Id) -> Option<u32> {
        self.jobs.iter().filter(|&&(_, j)| j == id).map(|&(t, _)| t).max()
    }

    pub fn steps(&self) -> Vec<&Step> {
        let mut out = Vec::new();
        let mut cur = self.log.as_deref();
        while let Some(step) = cur {
            out.push(step);
            cur = step.prev.as_deref();
        }
        out.reverse();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(goal: &[(Id, u16)]) -> (Plan, Config) {
        let cfg = Config {
            expand_travel: 12.0,
            max_expansions: 1,
            max_macro_hatcheries: 0,
            max_extractors: 2,
            max_queens: 2,
            horizon: ticks(900.0),
        };
        (Plan::new(goal), cfg)
    }

    #[test]
    fn twelve_drones_mine_about_eleven_minerals_a_second() {
        let (plan, cfg) = setup(&[(Id::Drone, 13)]);
        let mut s = State::start(&plan, &cfg);
        for _ in 0..ticks(10.0) {
            s.advance();
        }
        assert!((s.minerals - (50.0 + 12.0 * 0.94 * 10.0)).abs() < 0.01);
        assert_eq!(s.larva(), 3);
    }

    #[test]
    fn drone_completes_after_twelve_seconds() {
        let (plan, cfg) = setup(&[(Id::Drone, 13)]);
        let s = State::start(&plan, &cfg).wait_and_begin(Id::Drone, cfg.horizon).unwrap();
        assert_eq!(s.supply_used, 26);
        assert_eq!(s.finish(&plan, cfg.horizon), Some(ticks(12.0)));
    }

    #[test]
    fn roach_waits_for_its_warren_and_never_starts_without_it() {
        let (plan, cfg) = setup(&[(Id::Roach, 1)]);
        let s = State::start(&plan, &cfg);
        assert_eq!(s.status(Id::Roach), Status::Never);
        let s = s.wait_and_begin(Id::SpawningPool, cfg.horizon).unwrap();
        assert_eq!(s.status(Id::RoachWarren), Status::Wait);
    }

    #[test]
    fn supply_block_needs_an_overlord() {
        let (plan, cfg) = setup(&[(Id::Drone, 20)]);
        let mut s = State::start(&plan, &cfg);
        s.supply_used = s.supply_cap;
        assert_eq!(s.status(Id::Drone), Status::Never);
        let s = s.wait_and_begin(Id::Overlord, cfg.horizon).unwrap();
        assert_eq!(s.status(Id::Drone), Status::Wait);
    }
}

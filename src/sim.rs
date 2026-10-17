//! Tick-based simulation of one race's economy and production.
//!
//! Workers mine on their own and the simulation puts workers on gas only while
//! the build still needs gas. Queens inject, orbital commands call down MULEs
//! and nexuses chrono boost whenever they have the energy; a chrono goes to the
//! structure with the most production left. The only decisions left to the
//! search are what to start, and in which order.

use crate::data::{self, Id, N, Race, Source, slot};
use crate::plan::{self, Plan};
use std::rc::Rc;
use std::sync::LazyLock;

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

#[derive(Clone, Copy)]
pub struct Config {
    /// Seconds a worker walks to the expansion; it mines nothing meanwhile.
    pub expand_travel: f64,
    pub max_expansions: u16,
    pub max_macro_hatcheries: u16,
    /// Gas buildings allowed.
    pub max_extractors: u16,
    pub max_queens: u16,
    /// Structures allowed of each production type (barracks, gateways, ...).
    pub max_production: u16,
    /// Give up on any build that runs longer than this.
    pub horizon: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ready,
    Wait,
    Never,
}

/// A finished structure that trains, researches or morphs something.
#[derive(Clone)]
pub struct Structure {
    pub kind: Id,
    addon: Option<Id>,
    /// A command center that is or is becoming an orbital or planetary.
    upgraded: bool,
    /// A finished orbital command, which calls down MULEs.
    orbital: bool,
    /// Tick each production slot frees up. The second one needs a reactor.
    busy_until: [u16; 2],
    chrono_until: u16,
    energy: f32,
    larva: u8,
    larva_timer: u8,
    /// Tick the pending inject pops, or 0.
    inject_done: u16,
}

impl Structure {
    fn new(kind: Id) -> Structure {
        let hatch = matches!(kind, Id::Hatchery | Id::MacroHatchery);
        Structure {
            kind,
            addon: None,
            upgraded: false,
            orbital: false,
            busy_until: [0; 2],
            chrono_until: 0,
            energy: if kind == Id::Nexus { data::NEXUS_START_ENERGY as f32 } else { 0.0 },
            larva: if hatch { data::NEW_HATCH_LARVA } else { 0 },
            larva_timer: 0,
            inject_done: 0,
        }
    }

    fn has_reactor(&self) -> bool {
        self.addon.is_some() && data::addons(self.kind).map(|a| a.1) == self.addon
    }

    /// Whether this structure can ever start `def`, ignoring whether it is busy.
    fn hosts(&self, def: &data::Def) -> bool {
        match def.source {
            Source::TownHall => {
                data::is_town_hall(self.kind)
                    && (!data::is_town_hall_upgrade(def.id) || (self.kind == Id::CommandCenter && !self.upgraded))
            }
            Source::Building(b) => self.kind == b,
            Source::TechLab(b) => self.kind == b && self.addon.is_some() && data::addons(b).map(|a| a.0) == self.addon,
            Source::AddOn(b) => self.kind == b && self.addon.is_none(),
            Source::Larva | Source::Worker | Source::Morph(_) => false,
        }
    }

    /// Production slots `def` can use here.
    fn slots(&self, def: &data::Def) -> usize {
        if matches!(def.source, Source::Building(_)) && self.has_reactor() { 2 } else { 1 }
    }
}

/// Whether finished `id`s become a `Structure`.
fn is_producer(id: Id) -> bool {
    static PRODUCERS: LazyLock<Vec<bool>> = LazyLock::new(|| data::DEFS.iter().map(|d| data::is_producer(d.id)).collect());
    PRODUCERS[id as usize]
}

/// Optional items reserve their own gas when the search picks them, since the
/// plan's gas budget leaves them out.
fn reserves_gas(id: Id) -> bool {
    data::is_optional(id) && data::def(id).gas > 0
}

/// When `work` ticks of production starting at `now` finish, if the structure
/// is chrono boosted until `until`.
fn boosted_end(now: u32, work: u32, until: u32) -> u32 {
    if until <= now {
        return now + work;
    }
    let covered = ((until - now) as f64 * data::CHRONO_SPEED) as u32;
    if work <= covered { now + (work as f64 / data::CHRONO_SPEED).ceil() as u32 } else { until + work - covered }
}

#[derive(Clone, Copy)]
struct Job {
    end: u32,
    id: Id,
    /// The production slot the job holds, as structure index * 2 + slot, or `UNHELD`.
    at: u8,
    /// Index of the job's step in the build.
    step: u16,
}

const UNHELD: u8 = u8::MAX;

pub struct Step {
    pub tick: u32,
    /// Supply counter as shown in game when the step starts (half units).
    pub supply: u32,
    pub id: Id,
    prev: Option<Rc<Step>>,
}

#[derive(Clone)]
pub struct State {
    pub race: Race,
    pub tick: u32,
    pub minerals: f64,
    pub gas: f64,
    pub supply_used: u32,
    pub supply_cap: u32,
    // Counts are u8 to keep states small; the search holds millions of them.
    // `Plan::oversized` rejects goals that could overflow them.
    /// Completed and alive, per item slot (individual units).
    done: [u8; N],
    /// In production, per item slot (actions).
    pub pending: [u8; N],
    /// Actions ever started, per item slot.
    pub started: [u8; N],
    pub structures: Vec<Structure>,
    queens: Vec<f64>,
    /// Tick each active MULE leaves.
    mules: Vec<u32>,
    /// SCVs constructing a structure, and so not mining.
    builders: u8,
    jobs: Vec<Job>,
    gas_spent: u32,
    gas_budget: u32,
    expand_cost: f64,
    /// Actions started so far.
    pub depth: u16,
    pub log: Option<Rc<Step>>,
}

impl State {
    pub fn start(plan: &Plan, cfg: &Config) -> State {
        let race = plan.race;
        let mut done = [0u8; N];
        for id in race.items() {
            done[slot(id)] = plan::initial_count(id) as u8;
        }
        let mut hall = Structure::new(race.town_hall());
        if race == Race::Zerg {
            hall.larva = data::START_LARVA;
        }
        State {
            race,
            tick: 0,
            minerals: data::START_MINERALS,
            gas: 0.0,
            supply_used: data::START_WORKERS as u32 * data::def(race.worker()).supply,
            supply_cap: race.items().map(|id| plan::initial_count(id) as u32 * data::def(id).provides).sum(),
            done,
            pending: [0; N],
            started: [0; N],
            structures: vec![hall],
            queens: Vec::new(),
            mules: Vec::new(),
            builders: 0,
            jobs: Vec::new(),
            gas_spent: 0,
            gas_budget: plan.gas,
            // The worker stops mining while it walks to the expansion.
            expand_cost: cfg.expand_travel * data::MINERAL_RATE,
            depth: 0,
            log: None,
        }
    }

    pub fn count(&self, id: Id) -> u16 {
        debug_assert_eq!(data::race(id), self.race, "{id:?} in a {:?} game", self.race);
        self.done[slot(id)] as u16
    }

    pub fn pending(&self, id: Id) -> u16 {
        debug_assert_eq!(data::race(id), self.race, "{id:?} in a {:?} game", self.race);
        self.pending[slot(id)] as u16
    }

    pub fn started(&self, id: Id) -> u16 {
        debug_assert_eq!(data::race(id), self.race, "{id:?} in a {:?} game", self.race);
        self.started[slot(id)] as u16
    }

    pub fn count_with_pending(&self, id: Id) -> u16 {
        self.count(id) + self.pending(id) * data::def(id).yields
    }

    pub fn bases(&self) -> u16 {
        self.count(self.race.town_hall())
    }

    /// Finished town halls, including Zerg macro hatcheries.
    pub fn town_halls(&self) -> u16 {
        self.structures.iter().filter(|s| data::is_town_hall(s.kind)).count() as u16
    }

    /// Energy on orbital commands and nexuses, for MULEs and chrono boosts.
    pub fn caster_energy(&self) -> f64 {
        self.structures.iter().filter(|s| s.orbital || s.kind == Id::Nexus).map(|s| s.energy as f64).sum()
    }

    pub fn pending_supply(&self) -> u32 {
        self.race.items().map(|id| self.pending(id) as u32 * data::def(id).provides).sum()
    }

    /// Gas the build still has to mine, beyond what is banked.
    pub fn gas_still_needed(&self) -> f64 {
        self.gas_budget as f64 - self.gas_spent as f64 - self.gas
    }

    /// Workers neither constructing nor consumed.
    fn free_workers(&self) -> u32 {
        self.count(self.race.worker()) as u32 - self.builders as u32
    }

    pub fn gas_workers(&self) -> u32 {
        if self.gas_still_needed() <= 0.0 {
            return 0;
        }
        let slots = self.count(self.race.gas()) as u32 * data::WORKERS_PER_GEYSER;
        slots.min(self.free_workers())
    }

    pub fn mineral_income(&self) -> f64 {
        let workers = self.free_workers() - self.gas_workers();
        let patches = self.bases() as u32 * data::PATCHES_PER_BASE;
        let full = workers.min(2 * patches);
        let third = (workers - full).min(patches);
        let mules = self.mules.len() as f64 * data::MULE_MINERALS / data::MULE_DURATION;
        full as f64 * data::MINERAL_RATE + third as f64 * data::MINERAL_RATE_THIRD + mules
    }

    pub fn gas_income(&self) -> f64 {
        self.gas_workers() as f64 * data::GAS_RATE
    }

    pub fn larva(&self) -> u32 {
        self.structures.iter().map(|h| h.larva as u32).sum()
    }

    fn mineral_cost(&self, id: Id) -> f64 {
        let base = data::def(id).minerals as f64;
        if id == self.race.town_hall() { base + self.expand_cost } else { base }
    }

    /// Advances the game by one tick.
    pub fn advance(&mut self) {
        self.advance_with(&mut |_, _| {});
    }

    /// Advances the game by one tick, telling `on_done` the step index and
    /// tick of each job that completes.
    fn advance_with(&mut self, on_done: &mut impl FnMut(u16, u32)) {
        self.minerals += self.mineral_income() * DT;
        self.gas += self.gas_income() * DT;
        self.tick += 1;

        match self.race {
            Race::Zerg => self.grow_larva(),
            Race::Terran => self.call_down_mules(),
            Race::Protoss => self.chrono_boost(),
        }

        if self.jobs.iter().any(|j| j.end <= self.tick) {
            let now = self.tick;
            let (finished, running): (Vec<_>, Vec<_>) = self.jobs.drain(..).partition(|j| j.end <= now);
            self.jobs = running;
            for job in finished {
                on_done(job.step, now);
                self.complete(job);
            }
        }
    }

    fn grow_larva(&mut self) {
        let now = self.tick;
        let larva_interval = ticks(data::LARVA_INTERVAL) as u8;
        let hatch_count = if self.queens.is_empty() { 1 } else { self.town_halls() as usize };
        // Queen i injects the (i % hatch_count)-th hatchery.
        for (j, h) in self.structures.iter_mut().filter(|s| data::is_town_hall(s.kind)).enumerate() {
            if h.larva < data::NATURAL_LARVA_CAP {
                h.larva_timer += 1;
                if h.larva_timer >= larva_interval {
                    h.larva += 1;
                    h.larva_timer = 0;
                }
            }
            if h.inject_done != 0 && h.inject_done as u32 <= now {
                h.larva = (h.larva + data::INJECT_LARVA).min(data::MAX_LARVA);
                h.inject_done = 0;
            }
            for energy in self.queens.iter_mut().skip(j).step_by(hatch_count) {
                *energy = (*energy + data::ENERGY_REGEN * DT).min(data::MAX_ENERGY);
                if *energy >= data::INJECT_ENERGY && h.inject_done == 0 {
                    *energy -= data::INJECT_ENERGY;
                    h.inject_done = (now + ticks(data::INJECT_TIME)) as u16;
                }
            }
        }
    }

    fn call_down_mules(&mut self) {
        let now = self.tick;
        self.mules.retain(|&leaves| leaves > now);
        for s in self.structures.iter_mut().filter(|s| s.orbital) {
            s.energy = (s.energy + (data::ENERGY_REGEN * DT) as f32).min(data::MAX_ENERGY as f32);
            if s.energy >= data::MULE_ENERGY as f32 {
                s.energy -= data::MULE_ENERGY as f32;
                self.mules.push(now + ticks(data::MULE_DURATION));
            }
        }
    }

    fn chrono_boost(&mut self) {
        let now = self.tick;
        for i in 0..self.structures.len() {
            let nexus = &mut self.structures[i];
            if nexus.kind != Id::Nexus {
                continue;
            }
            nexus.energy = (nexus.energy + (data::ENERGY_REGEN * DT) as f32).min(data::MAX_ENERGY as f32);
            if nexus.energy < data::CHRONO_ENERGY as f32 {
                continue;
            }
            let target = self
                .structures
                .iter()
                .enumerate()
                .filter(|(_, s)| s.chrono_until as u32 <= now)
                .map(|(j, s)| (s.busy_until.iter().map(|&t| t as u32).max().unwrap().saturating_sub(now), j))
                .filter(|&(left, _)| left > 0)
                .max();
            if let Some((_, j)) = target {
                self.structures[i].energy -= data::CHRONO_ENERGY as f32;
                self.boost(j);
            }
        }
    }

    /// Chrono boosts structure `j`, finishing what it is producing sooner.
    fn boost(&mut self, j: usize) {
        let now = self.tick;
        let until = now + ticks(data::CHRONO_DURATION);
        let s = &mut self.structures[j];
        s.chrono_until = until as u16;
        for k in 0..2 {
            let old = s.busy_until[k] as u32;
            if old <= now {
                continue;
            }
            let new = boosted_end(now, old - now, until);
            s.busy_until[k] = new as u16;
            let at = (j * 2 + k) as u8;
            for job in self.jobs.iter_mut().filter(|job| job.at == at) {
                job.end = new;
            }
        }
    }

    fn complete(&mut self, job: Job) {
        let id = job.id;
        let def = data::def(id);
        self.pending[slot(id)] -= 1;
        self.done[slot(id)] += def.yields as u8;
        self.supply_cap += def.provides;
        if def.source == Source::Worker && self.race == Race::Terran {
            self.builders -= 1;
        }
        match id {
            Id::Queen => self.queens.push(data::QUEEN_START_ENERGY),
            Id::OrbitalCommand => {
                let cc = &mut self.structures[job.at as usize / 2];
                cc.orbital = true;
                cc.energy = data::ORBITAL_START_ENERGY as f32;
            }
            _ => {}
        }
        if is_producer(id) {
            self.structures.push(Structure::new(id));
        }
    }

    fn has(&self, id: Id) -> bool {
        self.count(id) > 0
    }

    /// The idle production slot `id` would start in, as (structure, slot).
    /// Units that need no tech lab prefer structures without one.
    fn free_slot(&self, id: Id) -> Option<(usize, usize)> {
        let def = data::def(id);
        let mut best: Option<(bool, usize, usize)> = None;
        for (i, s) in self.structures.iter().enumerate().filter(|(_, s)| s.hosts(def)) {
            let tech_lab = s.addon.is_some() && !s.has_reactor();
            for k in (0..s.slots(def)).filter(|&k| s.busy_until[k] as u32 <= self.tick) {
                if best.is_none_or(|(t, ..)| t && !tech_lab) {
                    best = Some((tech_lab, i, k));
                }
            }
        }
        best.map(|(_, i, k)| (i, k))
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
            if self.pending(item) > 0 { Status::Wait } else { Status::Never }
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
                if self.free_workers() == 0 {
                    need(if self.builders > 0 { Status::Wait } else { waiting_on(self.race.worker()) });
                }
            }
            Source::Morph(src) => {
                if !self.has(src) {
                    need(waiting_on(src));
                }
            }
            Source::TownHall | Source::Building(_) | Source::TechLab(_) | Source::AddOn(_) => {
                if self.free_slot(id).is_none() {
                    if self.structures.iter().any(|s| s.hosts(def)) {
                        need(Status::Wait);
                    } else {
                        let host = match def.source {
                            Source::Building(b) | Source::TechLab(b) | Source::AddOn(b) => b,
                            _ => self.race.town_hall(),
                        };
                        need(waiting_on(host));
                    }
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
            if self.count(self.race.worker()) > 0 { need(Status::Wait) } else { need(Status::Never) }
        }
        if self.gas < def.gas as f64 {
            let gas = self.race.gas();
            let budgeted = self.gas_budget > self.gas_spent || reserves_gas(id);
            let more_gas_coming = budgeted && (self.has(gas) || self.pending(gas) > 0);
            if more_gas_coming { need(Status::Wait) } else { need(Status::Never) }
        }

        status
    }

    /// Starts `id`, which must be `Ready`.
    pub fn begin(&mut self, id: Id) {
        let def = data::def(id);
        let now = self.tick;
        let build = ticks(def.time);
        self.log = Some(Rc::new(Step { tick: now, supply: self.supply_used, id, prev: self.log.take() }));

        self.minerals -= self.mineral_cost(id);
        self.gas -= def.gas as f64;
        self.gas_spent += def.gas;
        let (mut end, mut at) = (now + build, UNHELD);
        match def.source {
            Source::Larva => {
                let hatch = self.structures.iter_mut().max_by_key(|h| h.larva).unwrap();
                hatch.larva -= 1;
            }
            Source::Worker => match self.race {
                Race::Zerg => {
                    self.done[slot(Id::Drone)] -= 1;
                    self.supply_used -= data::def(Id::Drone).supply;
                }
                Race::Terran => self.builders += 1,
                Race::Protoss => {}
            },
            Source::Morph(src) => self.done[slot(src)] -= 1,
            Source::TownHall | Source::Building(_) | Source::TechLab(_) | Source::AddOn(_) => {
                let (i, k) = self.free_slot(id).unwrap();
                // Gateway units warp in once Warp Gate is done: the gate only
                // cools down, and the unit appears shortly after.
                let warping = def.warp > 0.0 && self.has(Id::WarpGate);
                let s = &mut self.structures[i];
                let work = if warping { ticks(def.warp) } else { build };
                let free_at = boosted_end(now, work, s.chrono_until as u32);
                s.busy_until[k] = free_at as u16;
                if let Source::AddOn(_) = def.source {
                    s.addon = Some(id);
                    s.busy_until = [free_at as u16; 2];
                }
                if data::is_town_hall_upgrade(id) {
                    s.upgraded = true;
                }
                if warping {
                    end = now + ticks(data::WARP_IN_TIME);
                } else {
                    (end, at) = (free_at, (i * 2 + k) as u8);
                }
            }
        }
        self.supply_used += def.supply;
        self.jobs.push(Job { end, id, at, step: self.depth });
        self.pending[slot(id)] += 1;
        self.started[slot(id)] += 1;
        self.depth += 1;
    }

    /// Waits until `id` can start, then starts it. `None` if it never can
    /// within the horizon.
    pub fn wait_and_begin(&self, id: Id, horizon: u32) -> Option<State> {
        let mut s = self.clone();
        if reserves_gas(id) {
            s.gas_budget += data::def(id).gas;
        }
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
    /// can be. Simulates forward once for all actions that share the plan's
    /// gas budget, and once more for each that reserves its own.
    pub fn branch(&self, ids: &[Id], horizon: u32) -> Vec<State> {
        let mut children = Vec::with_capacity(ids.len());
        let (mut todo, own_gas): (Vec<Id>, Vec<Id>) = ids.iter().partition(|&&id| !reserves_gas(id));
        let mut s = self.clone();
        while !todo.is_empty() {
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
                break;
            }
            s.advance();
        }
        children.extend(own_gas.into_iter().filter_map(|id| self.wait_and_begin(id, horizon)));
        children
    }

    pub fn goal_met(&self, plan: &Plan) -> bool {
        self.race.items().all(|id| self.count(id) >= plan.goal(id))
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
        self.jobs.iter().filter(|j| j.id == id).map(|j| j.end).min()
    }

    pub fn last_finish(&self, id: Id) -> Option<u32> {
        self.jobs.iter().filter(|j| j.id == id).map(|j| j.end).max()
    }

    /// In-progress work as (item, finish tick), in a canonical order.
    pub fn jobs_sorted(&self) -> Vec<(Id, u32)> {
        let mut jobs: Vec<_> = self.jobs.iter().map(|j| (j.id, j.end)).collect();
        jobs.sort_unstable();
        jobs
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

    /// The tick each step of this state's build completes, found by replaying
    /// the build from `root`. Chrono boosts make this differ from the build time.
    pub fn completions(&self, root: &State) -> Vec<u32> {
        let steps = self.steps();
        let mut ends = vec![0; steps.len()];
        let mut record = |step: u16, tick: u32| ends[step as usize] = tick;
        let mut s = root.clone();
        for step in &steps {
            // Reserved at the decision, which follows the previous step's start.
            if reserves_gas(step.id) {
                s.gas_budget += data::def(step.id).gas;
            }
            while s.tick < step.tick {
                s.advance_with(&mut record);
            }
            debug_assert_eq!(s.status(step.id), Status::Ready, "replay diverged at {:?}", step.id);
            s.begin(step.id);
        }
        while !s.jobs.is_empty() {
            s.advance_with(&mut record);
        }
        ends
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
            max_production: 3,
            horizon: ticks(900.0),
        };
        (Plan::new(goal), cfg)
    }

    fn run(s: &mut State, seconds: f64) {
        for _ in 0..ticks(seconds) {
            s.advance();
        }
    }

    #[test]
    fn twelve_drones_mine_about_eleven_minerals_a_second() {
        let (plan, cfg) = setup(&[(Id::Drone, 13)]);
        let mut s = State::start(&plan, &cfg);
        run(&mut s, 10.0);
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

    #[test]
    fn an_scv_stops_mining_while_it_builds() {
        let (plan, cfg) = setup(&[(Id::Marine, 1)]);
        let s = State::start(&plan, &cfg).wait_and_begin(Id::SupplyDepot, cfg.horizon).unwrap();
        assert!((s.mineral_income() - 11.0 * 0.94).abs() < 1e-9);
        assert_eq!(s.count(Id::Scv), 12);
        let mut s = s;
        run(&mut s, 21.0);
        assert_eq!(s.count(Id::SupplyDepot), 1);
        assert!((s.mineral_income() - 12.0 * 0.94).abs() < 1e-9);
    }

    #[test]
    fn orbital_commands_call_down_mules() {
        let (plan, cfg) = setup(&[(Id::OrbitalCommand, 1)]);
        let mut s = State::start(&plan, &cfg);
        s.done[slot(Id::Barracks)] = 1;
        s.minerals = 150.0;
        let mut s = s.wait_and_begin(Id::OrbitalCommand, cfg.horizon).unwrap();
        assert_eq!(s.status(Id::Scv), Status::Wait, "the command center is busy morphing");
        let before = s.mineral_income();
        run(&mut s, 25.5);
        assert_eq!(s.count(Id::OrbitalCommand), 1);
        assert!((s.mineral_income() - before - 225.0 / 64.0).abs() < 1e-9, "one MULE right away");
        assert_eq!(s.status(Id::OrbitalCommand), Status::Never, "a command center morphs once");
    }

    #[test]
    fn a_reactor_trains_two_marines_at_once() {
        let (plan, cfg) = setup(&[(Id::Marine, 2)]);
        let mut s = State::start(&plan, &cfg);
        s.done[slot(Id::SupplyDepot)] = 1;
        s.minerals = 1000.0;
        s.gas = 100.0;
        s.gas_budget = 100;
        s.done[slot(Id::Refinery)] = 1;
        let s = s.wait_and_begin(Id::Barracks, cfg.horizon).unwrap();
        let mut s = s.wait_and_begin(Id::BarracksReactor, cfg.horizon).unwrap();
        assert_eq!(s.tick, ticks(46.0));
        s.minerals = 1000.0;
        let s = s.wait_and_begin(Id::Marine, cfg.horizon).unwrap();
        assert_eq!(s.tick, ticks(46.0 + 36.0));
        let s = s.wait_and_begin(Id::Marine, cfg.horizon).unwrap();
        assert_eq!(s.tick, ticks(46.0 + 36.0), "the second marine starts alongside the first");
        assert_eq!(s.status(Id::Marine), Status::Wait);
    }

    #[test]
    fn chrono_boost_speeds_up_the_first_probe() {
        let (plan, cfg) = setup(&[(Id::Probe, 13)]);
        let s = State::start(&plan, &cfg).wait_and_begin(Id::Probe, cfg.horizon).unwrap();
        // The nexus starts with enough energy and chronos the probe on the next tick.
        assert_eq!(s.finish(&plan, cfg.horizon), Some(1 + (47.0f64 / 1.5).ceil() as u32));
        assert_eq!(s.completions(&State::start(&plan, &cfg)), [1 + 32]);
    }

    #[test]
    fn gateway_units_warp_in_after_warp_gate() {
        let (plan, cfg) = setup(&[(Id::Zealot, 2)]);
        let mut s = State::start(&plan, &cfg);
        s.structures.push(Structure::new(Id::Gateway));
        s.done[slot(Id::Gateway)] = 1;
        s.done[slot(Id::WarpGate)] = 1;
        s.minerals = 1000.0;
        s.supply_cap = 60;
        let s = s.wait_and_begin(Id::Zealot, cfg.horizon).unwrap();
        assert_eq!(s.first_finish(Id::Zealot), Some(ticks(data::WARP_IN_TIME)));
        let s = s.wait_and_begin(Id::Zealot, cfg.horizon).unwrap();
        assert!(s.tick < ticks(20.0), "chrono shortens the cooldown");
        assert!(s.tick > ticks(20.0 / 1.5));
    }
}

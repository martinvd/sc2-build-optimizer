//! Best-first branch and bound over build orders.
//!
//! A node is the game state right after starting an action. Children start one
//! more action at the earliest moment it becomes possible, so every build order
//! is explored with its actions in start-time order. Nodes are expanded in
//! order of a lower bound on the goal's completion time, which means the first
//! finished build popped from the queue is the fastest one the model allows.
//!
//! Two things keep the tree small:
//! - Optional actions are only offered when they can still pay off (no workers
//!   past saturation, no supply while supply is plentiful, no more barracks
//!   than marines left to make, ...).
//! - Among states that started the same multiset of actions, the search drops
//!   one that is later, poorer, no better off in larva and has nothing finishing
//!   sooner than another.

use crate::data::{self, Id, N, Race, Source};
use crate::plan::Plan;
use crate::progress::Progress;
use crate::sim::{Config, State, Status, TICKS_PER_SEC, ticks};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::LazyLock;

pub struct Solution {
    pub state: State,
    /// Tick each of the build's steps completes.
    pub ends: Vec<u32>,
    pub finish: u32,
    pub explored: usize,
    /// False when the search ran out of budget and this is the best found.
    pub proven: bool,
}

pub fn search(
    plan: &Plan,
    cfg: &Config,
    beam_width: usize,
    max_states: usize,
    progress: &mut Progress,
) -> Option<Solution> {
    let root = State::start(plan, cfg);
    let greedy = greedy_bound(&root, plan, cfg);
    let beamed = beam(&root, plan, cfg, beam_width, greedy.as_ref().map(|(t, _)| *t), progress);
    let quick = [greedy, beamed].into_iter().flatten().min_by_key(|(t, _)| *t);
    let (rough, _) = best_first(&root, plan, cfg, quick, Pruning::Loose, max_states, progress);
    let (best, outcome) = best_first(&root, plan, cfg, rough, Pruning::Strict, max_states, progress);
    progress.clear();
    best.map(|(finish, state)| Solution {
        ends: state.completions(&root),
        state,
        finish,
        explored: outcome.explored,
        proven: outcome.exhausted,
    })
}

/// How aggressively states that started the same actions are merged.
#[derive(Clone, Copy, PartialEq)]
enum Pruning {
    /// Ignores when in-progress work finishes. Much faster, but can drop the
    /// fastest build, so it only serves as a bound for the strict pass.
    Loose,
    Strict,
}

struct Outcome {
    explored: usize,
    /// Whether every state that could beat the result was explored.
    exhausted: bool,
}

/// A queue entry, popped lowest bound first, then deepest, then newest.
struct Queued {
    key: (Reverse<u32>, u16, usize),
    state: State,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

type Seen = HashMap<([u8; N], [u8; N]), Vec<Mark>>;

fn best_first(
    root: &State,
    plan: &Plan,
    cfg: &Config,
    mut best: Option<(u32, State)>,
    pruning: Pruning,
    max_states: usize,
    progress: &mut Progress,
) -> (Option<(u32, State)>, Outcome) {
    let pass = if pruning == Pruning::Strict { "Exact search" } else { "Rough search" };
    let mut queue = BinaryHeap::new();
    let mut pushed = 0;
    queue.push(Queued { key: (Reverse(0), 0, pushed), state: root.clone() });
    let mut seen = Seen::new();
    let mut explored = 0;

    while let Some(Queued { key: (Reverse(f), ..), state }) = queue.pop() {
        if best.as_ref().is_some_and(|(t, _)| *t <= f) {
            break;
        }
        if is_terminal(&state, plan) {
            best = Some((f, state));
            break;
        }
        explored += 1;
        progress.update(pass, explored, best.as_ref().map(|(t, _)| *t));
        if explored > max_states || progress.cancelled() {
            return (best, Outcome { explored, exhausted: false });
        }

        for child in state.branch(&candidates(&state, plan, cfg), cfg.horizon) {
            let f = if is_terminal(&child, plan) {
                let Some(t) = child.finish(plan, cfg.horizon) else { continue };
                t
            } else {
                lower_bound(&child, plan, cfg)
            };
            if best.as_ref().is_some_and(|(t, _)| *t <= f) {
                continue;
            }
            if !record(&mut seen, &child, pruning) {
                continue;
            }
            pushed += 1;
            queue.push(Queued { key: (Reverse(f), child.depth, pushed), state: child });
        }
    }

    (best, Outcome { explored, exhausted: true })
}

fn is_terminal(s: &State, plan: &Plan) -> bool {
    s.race.items().all(|id| {
        if data::is_optional(id) {
            s.count_with_pending(id) >= plan.goal(id)
        } else {
            s.started(id) >= plan.required(id)
        }
    })
}

fn remaining(s: &State, plan: &Plan, id: Id) -> u16 {
    plan.essential(id).saturating_sub(s.started(id))
}

/// Like `remaining`, but also counting production the plan needs, such as
/// the first barracks. Those cost minerals like anything else.
fn still_needed(s: &State, plan: &Plan, id: Id) -> u16 {
    plan.charged(id).saturating_sub(s.started(id))
}

/// Town halls finished or in production, including Zerg macro hatcheries.
fn town_halls_total(s: &State) -> u16 {
    s.town_halls() + s.race.items().filter(|&id| data::is_town_hall(id)).map(|id| s.pending(id)).sum::<u16>()
}

/// Town halls there could ever be: the current ones plus the expansions left.
fn town_halls_possible(s: &State, cfg: &Config) -> u16 {
    town_halls_total(s) + cfg.max_expansions.saturating_sub(s.started(s.race.town_hall()))
}

fn worker_cap(s: &State, plan: &Plan) -> u16 {
    let race = s.race;
    let bases = s.count_with_pending(race.town_hall());
    let geysers = s.started(race.gas());
    // Drones turn into the structures they build.
    let for_buildings: u16 = if race == Race::Zerg {
        race.items().filter(|&id| data::def(id).source == Source::Worker).map(|id| remaining(s, plan, id)).sum()
    } else {
        0
    };
    let saturation = bases * 2 * data::PATCHES_PER_BASE as u16 + geysers * data::WORKERS_PER_GEYSER as u16;
    (saturation + for_buildings).max(plan.goal(race.worker()))
}

fn queen_cap(s: &State, plan: &Plan, cfg: &Config) -> u16 {
    cfg.max_queens.min(town_halls_total(s)).max(plan.goal(Id::Queen))
}

/// Units the plan still needs from structures of type `b`, counting only
/// those whose source passes `filter`.
fn units_from(s: &State, plan: &Plan, b: Id, filter: impl Fn(Source) -> bool) -> u16 {
    s.race
        .items()
        .filter(|&id| data::producer(id) == Some(b) && filter(data::def(id).source))
        .map(|id| remaining(s, plan, id))
        .sum()
}

/// Whether another production item `id` could speed up the goal: structures
/// and add-ons while there are units left to make with them, and Warp Gate
/// for gateway units.
fn production_useful(s: &State, plan: &Plan, cfg: &Config, id: Id) -> bool {
    if s.started(id) < plan.required(id) {
        return true;
    }
    match data::def(id).source {
        Source::AddOn(b) => {
            let (lab, reactor) = data::addons(b).unwrap();
            let free = s.started(lab) + s.started(reactor) < s.count_with_pending(b);
            let wanted = if id == lab {
                units_from(s, plan, b, |src| matches!(src, Source::TechLab(_))) > 0
            } else {
                units_from(s, plan, b, |src| matches!(src, Source::Building(_))) > 1
            };
            free && wanted
        }
        _ if id == Id::WarpGate => s.started(id) == 0 && units_from(s, plan, Id::Gateway, |_| true) > 0,
        _ => s.started(id) < cfg.max_production.min(units_from(s, plan, id, |_| true)),
    }
}

/// Actions worth trying next from `s`.
fn candidates(s: &State, plan: &Plan, cfg: &Config) -> Vec<Id> {
    let race = s.race;
    let started = |id: Id| s.started(id);
    let goal = |id: Id| plan.goal(id);
    let bases = s.count_with_pending(race.town_hall());
    let useful = |id: Id| data::is_production(id) && production_useful(s, plan, cfg, id);
    // Optional items reserve their own gas, but only once a gas building is up.
    let gas_wanted = s.gas_still_needed() > 0.0 || race.items().any(|id| data::def(id).gas > 0 && useful(id));

    race.items()
        .filter(|&id| match id {
            _ if id == race.worker() => s.count_with_pending(id) < worker_cap(s, plan),
            _ if id == race.supply() => supply_useful(s, plan, cfg),
            _ if id == race.gas() => {
                // One geyser easily feeds the add-ons and tech the goal itself has no gas for.
                let allowed = if plan.gas > 0 { cfg.max_extractors } else { cfg.max_extractors.min(1) };
                let cap = allowed.min(bases * data::GEYSERS_PER_BASE);
                gas_wanted && started(id) < cap.max(goal(id))
            }
            _ if id == race.town_hall() => started(id) < cfg.max_expansions.max(goal(id).saturating_sub(1)),
            Id::Queen => started(id) < queen_cap(s, plan, cfg),
            Id::MacroHatchery => started(id) < cfg.max_macro_hatcheries.max(goal(id)),
            Id::OrbitalCommand => started(id) + started(Id::PlanetaryFortress) < s.count_with_pending(Id::CommandCenter),
            _ if data::is_production(id) => useful(id),
            _ => remaining(s, plan, id) > 0,
        })
        .filter(|&id| s.status(id) != Status::Never)
        .collect()
}

/// Supply only when it is getting tight and something still needs it, or
/// when the plan needs it as tech (pylons power Protoss structures).
fn supply_useful(s: &State, plan: &Plan, cfg: &Config) -> bool {
    let supply = s.race.supply();
    let total = s.supply_cap + s.pending_supply();
    if total >= data::MAX_SUPPLY {
        return false;
    }
    if s.count_with_pending(supply) < plan.goal(supply) || s.started(supply) < plan.required(supply) {
        return true;
    }
    let free = total - s.supply_used.min(total);
    let plan_supply: u32 = s.race.items().map(|id| remaining(s, plan, id) as u32 * data::def(id).supply).sum();
    let worker = s.race.worker();
    let workers = worker_cap(s, plan).saturating_sub(s.count_with_pending(worker)) as u32;
    let queens = if s.race == Race::Zerg { queen_cap(s, plan, cfg).saturating_sub(s.started(Id::Queen)) as u32 } else { 0 };
    let demand = plan_supply + workers * data::def(worker).supply + queens * data::def(Id::Queen).supply;
    free < data::def(supply).provides && free < demand
}

/// Fastest production time of `id`: chrono boosted the whole way, or warped in.
fn fastest(id: Id) -> u32 {
    static FASTEST: LazyLock<Vec<u32>> = LazyLock::new(|| data::DEFS.iter().map(|d| compute_fastest(d.id)).collect());
    FASTEST[id as usize]
}

fn compute_fastest(id: Id) -> u32 {
    let def = data::def(id);
    if data::race(id) != Race::Protoss || matches!(def.source, Source::Worker) {
        return ticks(def.time);
    }
    let boosted = ticks(def.time / data::CHRONO_SPEED);
    if def.warp > 0.0 { boosted.min(ticks(data::WARP_IN_TIME)) } else { boosted }
}

/// How long `id` keeps a structure busy at best: its production time or warp
/// gate cooldown, chrono boosted the whole way.
fn occupancy(id: Id) -> f64 {
    let def = data::def(id);
    let time = if def.warp > 0.0 { def.time.min(def.warp) } else { def.time };
    if data::race(id) == Race::Protoss { time / data::CHRONO_SPEED } else { time }
}

/// Most production time chrono boosts could save on any one chain of work
/// that ends at `until`: every nexus there could be casting as soon as it
/// has the energy, each boost saving its full amount.
fn chrono_saving(s: &State, cfg: &Config, until: u32) -> u32 {
    if s.race != Race::Protoss {
        return 0;
    }
    let span = until.saturating_sub(s.tick);
    let secs = span as f64 / TICKS_PER_SEC as f64;
    // Nexuses in production could be done right away; one started now takes
    // its build time.
    let building = (town_halls_total(s) - s.town_halls()) as f64;
    let unstarted = cfg.max_expansions.saturating_sub(s.started(Id::Nexus)) as f64;
    let new_secs = (secs - data::def(Id::Nexus).time).max(0.0);
    let new_energy = if new_secs > 0.0 { data::NEXUS_START_ENERGY + data::ENERGY_REGEN * new_secs } else { 0.0 };
    let energy = s.caster_energy()
        + (s.town_halls() as f64 + building) * data::ENERGY_REGEN * secs
        + building * data::NEXUS_START_ENERGY
        + unstarted * new_energy;
    let chronos = (energy / data::CHRONO_ENERGY).floor() as u32;
    let per_chrono = ticks(data::CHRONO_DURATION * (data::CHRONO_SPEED - 1.0));
    (chronos * per_chrono).min(span / 3)
}

/// Optimistic completion tick for the goal from `s`.
fn lower_bound(s: &State, plan: &Plan, cfg: &Config) -> u32 {
    let race = s.race;
    let mut memo = Memo::new(false);
    let mut bound = s.tick;

    // The tech path, without chrono boost less what boosts could save at most,
    // and with every boostable step boosted the whole way.
    let path = |memo: &mut Memo| {
        let mut path = s.tick;
        for id in race.items() {
            let want = plan.goal(id);
            if want == 0 || s.count(id) >= want {
                continue;
            }
            let t = if s.count_with_pending(id) >= want {
                s.last_finish(id).map_or(s.tick, |t| memo.shorten(s, id, t))
            } else {
                next_ready(s, id, memo)
            };
            path = path.max(t);
        }
        path
    };
    let plain = path(&mut memo);
    bound = bound.max(plain - chrono_saving(s, cfg, plain));
    if race == Race::Protoss {
        bound = bound.max(path(&mut Memo::new(true)));
    }

    // Gas: even with every allowed geyser saturated right away.
    let gas_needed = s.gas_still_needed();
    if gas_needed > 0.0 {
        let gas = race.gas();
        let first_geyser = if s.count(gas) > 0 {
            s.tick
        } else {
            s.first_finish(gas).unwrap_or(s.tick + ticks(data::def(gas).time))
        };
        let rate = data::GAS_RATE * (data::WORKERS_PER_GEYSER * cfg.max_extractors.max(1) as u32) as f64;
        let shortest = shortest_remaining(s, plan, |id| data::def(id).gas > 0);
        bound = bound.max(first_geyser + ticks(gas_needed / rate) + shortest);
    }

    // Larva: every hatchery we could ever have, spawning and injected nonstop.
    let larva_units: u32 = race
        .items()
        .filter(|&id| data::def(id).source == Source::Larva)
        .map(|id| remaining(s, plan, id) as u32)
        .sum();
    if larva_units > s.larva() {
        let hatches =
            town_halls_possible(s, cfg) + cfg.max_macro_hatcheries.saturating_sub(s.started(Id::MacroHatchery));
        let per_sec = 1.0 / data::LARVA_INTERVAL + data::INJECT_LARVA as f64 / data::INJECT_TIME;
        let wait = (larva_units - s.larva()) as f64 / (hatches as f64 * per_sec);
        let shortest = shortest_remaining(s, plan, |id| data::def(id).source == Source::Larva);
        bound = bound.max(s.tick + ticks(wait) + shortest);
    }

    // Production: each structure type works through its units with as many
    // structures as allowed from the moment the first one is up, two units
    // at a time where reactors fit.
    for b in race.items().filter(|&b| data::is_production(b) && data::def(b).source == Source::Worker) {
        let work: f64 = race
            .items()
            .filter(|&id| data::producer(id) == Some(b))
            .map(|id| remaining(s, plan, id) as f64 * occupancy(id))
            .sum();
        if work == 0.0 {
            continue;
        }
        let per_structure = if data::addons(b).is_some() { 2 } else { 1 };
        let slots = (cfg.max_production.max(s.count_with_pending(b)) * per_structure).max(1);
        bound = bound.max(available(s, b, &mut memo) + ticks(work / slots as f64));
    }

    // MULEs can only start once an orbital command is up.
    let first_orbital = match race {
        Race::Terran if s.count(Id::OrbitalCommand) > 0 => Some(s.tick),
        Race::Terran => Some(s.first_finish(Id::OrbitalCommand).unwrap_or_else(|| {
            available(s, Id::Barracks, &mut memo) + ticks(data::def(Id::OrbitalCommand).time)
        })),
        _ => None,
    };
    if let Some(t) = mineral_bound(s, plan, cfg, first_orbital) {
        bound = bound.max(t);
    }

    bound
}

/// Earliest tick the remaining plan could be paid for in minerals, then built.
///
/// The bound overestimates income. Current and in-production workers mine at
/// full rate, and every worker the town halls could ever produce (from every
/// larva, or back to back at every town hall) mines from the moment it is
/// done. Such a worker still only counts for what it nets by the deadline
/// (mining minus its cost), so short horizons get no credit. Terran also gets
/// a MULE from every town hall the moment it has the energy for one, from
/// `first_orbital` on, mining until the deadline however long that is.
fn mineral_bound(s: &State, plan: &Plan, cfg: &Config, first_orbital: Option<u32>) -> Option<u32> {
    let race = s.race;
    let cost: f64 = race.items().map(|id| still_needed(s, plan, id) as f64 * data::def(id).minerals as f64).sum();
    let needed = cost - s.minerals;
    if needed <= 0.0 {
        return None;
    }

    let worker = data::def(race.worker());
    let income = s.mineral_income() + s.pending(race.worker()) as f64 * data::MINERAL_RATE;
    let halls = town_halls_possible(s, cfg) as f64;
    let larva_per_sec = halls * (1.0 / data::LARVA_INTERVAL + data::INJECT_LARVA as f64 / data::INJECT_TIME);
    let larva_now = s.larva() as f64;
    let worker_time = fastest(race.worker()) as f64 / TICKS_PER_SEC as f64;
    // When the k-th extra worker could be done.
    let ready = |k: f64| match race {
        Race::Zerg => (if k < larva_now { 0.0 } else { (k - larva_now + 1.0) / larva_per_sec }) + worker.time,
        _ => (k / halls + 1.0) * worker_time,
    };
    let orbitals = if race == Race::Terran { s.count(Id::OrbitalCommand) as f64 } else { 0.0 };
    let mule_energy = s.caster_energy() + (halls - orbitals) * data::ORBITAL_START_ENERGY;
    let mule_from = first_orbital.map(|t| (t - s.tick) as f64 / TICKS_PER_SEC as f64);
    // MULE-seconds by `t`, with every MULE cast as soon as the energy is there.
    let mule_seconds = |t: f64| match mule_from {
        Some(from) if t > from => {
            let span = t - from;
            (mule_energy * span + halls * data::ENERGY_REGEN * span * span / 2.0) / data::MULE_ENERGY
        }
        _ => 0.0,
    };

    let mined_by = |t: f64| {
        let mut total = income * t + mule_seconds(t) * data::MULE_MINERALS / data::MULE_DURATION;
        for k in 0.. {
            let net = data::MINERAL_RATE * (t - ready(k as f64)) - worker.minerals as f64;
            if net <= 0.0 {
                break;
            }
            total += net;
        }
        total
    };

    let (mut lo, mut hi) = (0.0, 1.0);
    while mined_by(hi) < needed {
        hi *= 2.0;
        if hi > 3600.0 {
            return None;
        }
    }
    while hi - lo > 1.0 / TICKS_PER_SEC as f64 {
        let mid = (lo + hi) / 2.0;
        if mined_by(mid) >= needed { hi = mid } else { lo = mid }
    }
    let shortest = shortest_remaining(s, plan, |id| data::def(id).minerals > 0);
    Some(s.tick + (lo * TICKS_PER_SEC as f64) as u32 + shortest)
}

fn shortest_remaining(s: &State, plan: &Plan, filter: impl Fn(Id) -> bool) -> u32 {
    s.race.items().filter(|&id| remaining(s, plan, id) > 0 && filter(id)).map(fastest).min().unwrap_or(0)
}

/// Completion ticks `next_ready` found, and whether it assumes chrono boost
/// on every step it could apply to.
struct Memo {
    ready: [Option<u32>; N],
    boosted: bool,
}

impl Memo {
    fn new(boosted: bool) -> Memo {
        Memo { ready: [None; N], boosted }
    }

    fn boosts(&self, id: Id) -> bool {
        self.boosted && data::race(id) == Race::Protoss && data::def(id).source != Source::Worker
    }

    fn time(&self, id: Id) -> u32 {
        let time = data::def(id).time;
        ticks(if self.boosts(id) { time / data::CHRONO_SPEED } else { time })
    }

    /// The earliest an in-progress `id` due at `end` could finish.
    fn shorten(&self, s: &State, id: Id, end: u32) -> u32 {
        if self.boosts(id) { s.tick + ((end - s.tick) as f64 / data::CHRONO_SPEED) as u32 } else { end }
    }
}

/// Earliest tick one more `id` could be complete, given unlimited resources.
fn next_ready(s: &State, id: Id, memo: &mut Memo) -> u32 {
    if let Some(t) = memo.ready[data::slot(id)] {
        return t;
    }
    let def = data::def(id);
    let mut start = s.tick;
    for &r in def.requires {
        start = start.max(available(s, r, memo));
    }
    match def.source {
        Source::Morph(src) | Source::Building(src) | Source::TechLab(src) | Source::AddOn(src) => {
            start = start.max(available(s, src, memo))
        }
        _ => {}
    }
    let mut t = start + memo.time(id);
    if def.warp > 0.0 {
        t = t.min(start.max(available(s, Id::WarpGate, memo)) + ticks(data::WARP_IN_TIME));
    }
    memo.ready[data::slot(id)] = Some(t);
    t
}

fn available(s: &State, id: Id, memo: &mut Memo) -> u32 {
    if s.count(id) > 0 {
        s.tick
    } else if let Some(t) = s.first_finish(id) {
        memo.shorten(s, id, t)
    } else {
        next_ready(s, id, memo)
    }
}

struct Mark {
    tick: u32,
    minerals: f64,
    gas: f64,
    mineral_income: f64,
    larva: u32,
    jobs: Vec<(Id, u32)>,
}

/// Records `s` unless an equivalent state is already at least as good.
fn record(seen: &mut Seen, s: &State, pruning: Pruning) -> bool {
    let mark = Mark {
        tick: s.tick,
        minerals: s.minerals,
        gas: s.gas,
        mineral_income: s.mineral_income(),
        larva: s.larva(),
        jobs: if pruning == Pruning::Strict { s.jobs_sorted() } else { Vec::new() },
    };
    // Strictly, only states with the same work in progress are comparable.
    let pending = if pruning == Pruning::Strict { s.pending } else { [0; N] };
    let marks = seen.entry((s.started, pending)).or_default();
    if marks.iter().any(|m| dominates(m, &mark)) {
        return false;
    }
    marks.retain(|m| !dominates(&mark, m));
    marks.push(mark);
    true
}

fn dominates(a: &Mark, b: &Mark) -> bool {
    if a.tick > b.tick {
        return false;
    }
    let elapsed = (b.tick - a.tick) as f64 / TICKS_PER_SEC as f64;
    a.minerals + a.mineral_income * elapsed >= b.minerals - 1e-6
        && a.gas >= b.gas - 1e-6
        && a.mineral_income >= b.mineral_income - 1e-6
        && a.larva >= b.larva
        && a.jobs.len() == b.jobs.len()
        && a.jobs.iter().zip(&b.jobs).all(|(x, y)| x.0 == y.0 && x.1 <= y.1)
}

/// A good build found fast: the search tree explored one action at a time,
/// keeping only the `width` most promising states at each depth.
fn beam(
    root: &State,
    plan: &Plan,
    cfg: &Config,
    width: usize,
    known: Option<u32>,
    progress: &mut Progress,
) -> Option<(u32, State)> {
    let mut best: Option<(u32, State)> = None;
    let mut layer = vec![root.clone()];
    let mut explored = 0;
    while !layer.is_empty() {
        let mut seen = HashMap::new();
        let mut next = Vec::new();
        for state in &layer {
            explored += 1;
            let shown = best.as_ref().map(|(t, _)| *t).into_iter().chain(known).min();
            progress.update("Quick pass", explored, shown);
            if progress.cancelled() {
                return best;
            }
            for child in state.branch(&candidates(state, plan, cfg), cfg.horizon) {
                if is_terminal(&child, plan) {
                    if let Some(t) = child.finish(plan, cfg.horizon)
                        && best.as_ref().is_none_or(|(b, _)| t < *b)
                    {
                        best = Some((t, child));
                    }
                    continue;
                }
                let f = lower_bound(&child, plan, cfg);
                if best.as_ref().is_some_and(|(b, _)| *b <= f) || !record(&mut seen, &child, Pruning::Strict) {
                    continue;
                }
                next.push((f, child));
            }
        }
        next.sort_by_key(|(f, s)| (*f, s.tick));
        next.truncate(width);
        layer = next.into_iter().map(|(_, s)| s).collect();
    }
    best
}

/// A quick upper bound: build workers to a fixed count, then tech straight
/// up. Tried for every worker count, keeping the fastest.
fn greedy_bound(root: &State, plan: &Plan, cfg: &Config) -> Option<(u32, State)> {
    let start = root.count(root.race.worker());
    (start..=start + 2 * data::PATCHES_PER_BASE as u16)
        .filter_map(|target| greedy(root, plan, cfg, target))
        .min_by_key(|(t, _)| *t)
}

fn greedy(root: &State, plan: &Plan, cfg: &Config, worker_target: u16) -> Option<(u32, State)> {
    let race = root.race;
    let mut s = root.clone();
    loop {
        if is_terminal(&s, plan) {
            return s.finish(plan, cfg.horizon).map(|t| (t, s));
        }
        let free = (s.supply_cap + s.pending_supply()).saturating_sub(s.supply_used);
        let ids: Vec<Id> = candidates(&s, plan, cfg)
            .into_iter()
            .filter(|&id| match id {
                _ if id == race.supply() || id == race.town_hall() => false,
                Id::Queen | Id::MacroHatchery | Id::OrbitalCommand => false,
                _ if id == race.worker() => s.count_with_pending(id) < worker_target,
                _ if id == race.gas() => s.started(id) == 0 && s.gas_still_needed() > 0.0,
                _ if data::is_production(id) => s.started(id) < plan.required(id),
                _ => true,
            })
            .collect();
        let options = s.branch(&ids, cfg.horizon);
        if free <= 4 || options.is_empty() {
            s = s.wait_and_begin(race.supply(), cfg.horizon)?;
            continue;
        }
        s = options.into_iter().min_by_key(|c| (c.tick, c.log.as_ref().unwrap().id == race.worker())).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn cfg() -> Config {
        Config {
            expand_travel: 12.0,
            max_expansions: 1,
            max_macro_hatcheries: 0,
            max_extractors: 2,
            max_queens: 2,
            max_production: 3,
            horizon: ticks(900.0),
        }
    }

    fn solve(goal: &[(Id, u16)]) -> Solution {
        let plan = Plan::new(goal);
        search(&plan, &cfg(), 500, 2_000_000, &mut Progress::hidden()).expect("no build found")
    }

    fn order(sol: &Solution) -> Vec<Id> {
        sol.state.steps().iter().map(|s| s.id).collect()
    }

    #[test]
    fn single_drone_is_immediate() {
        let sol = solve(&[(Id::Drone, 13)]);
        assert_eq!(sol.finish, ticks(12.0));
        assert!(sol.proven);
    }

    #[test]
    fn roach_build_respects_the_tech_tree() {
        let order = order(&solve(&[(Id::Roach, 5)]));
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(Id::SpawningPool) < pos(Id::RoachWarren));
        assert!(pos(Id::RoachWarren) < pos(Id::Roach));
        assert!(pos(Id::Extractor) < pos(Id::Roach));
        assert_eq!(order.iter().filter(|&&x| x == Id::Roach).count(), 5);
    }

    #[test]
    fn marauders_come_from_a_barracks_with_a_tech_lab() {
        let sol = solve(&[(Id::Marauder, 2)]);
        let order = order(&sol);
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(Id::SupplyDepot) < pos(Id::Barracks));
        assert!(pos(Id::Refinery) < pos(Id::BarracksTechLab));
        assert!(pos(Id::BarracksTechLab) < pos(Id::Marauder));
        assert_eq!(order.iter().filter(|&&x| x == Id::Marauder).count(), 2);
        assert!(sol.proven);
    }

    #[test]
    fn stalkers_need_a_pylon_gateway_and_core() {
        let sol = solve(&[(Id::Stalker, 2)]);
        let order = order(&sol);
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(Id::Pylon) < pos(Id::Gateway));
        assert!(pos(Id::Gateway) < pos(Id::CyberneticsCore));
        assert!(pos(Id::CyberneticsCore) < pos(Id::Stalker));
        assert!(sol.proven);
        // Chrono boost makes some steps finish faster than their build time.
        let steps = sol.state.steps();
        assert!(steps.iter().zip(&sol.ends).any(|(s, &end)| end < s.tick + ticks(data::def(s.id).time)));
        assert!(steps.iter().zip(&sol.ends).all(|(s, &end)| end > s.tick));
    }

    #[test]
    fn cancelled_search_stops_early() {
        let plan = Plan::new(&[(Id::Mutalisk, 5)]);
        let mut progress = Progress::shared(Default::default(), Arc::new(AtomicBool::new(true)));
        let sol = search(&plan, &cfg(), 2000, 2_000_000, &mut progress).expect("the greedy bound still finds a build");
        assert!(!sol.proven);
        assert_eq!(sol.explored, 1);
    }
}

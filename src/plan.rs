//! Turning a goal like "5 roaches" into the set of things that must be built.

use crate::data::{self, Id, N, Race, Source, slot};

pub struct Plan {
    pub race: Race,
    /// Completed count wanted at the end, per item slot (individual units).
    goal: [u16; N],
    /// Actions that must be started to reach the goal, beyond the starting
    /// units, per item slot. Optional items (workers, supply, production, ...)
    /// only appear here when the goal names them or other items need them; the
    /// search adds more of them freely.
    required: [u16; N],
    /// `required`, but zero for optional items: what the search must start
    /// whatever else it decides.
    essential: [u16; N],
    /// `required`, but zero for economy items: what the search must pay for
    /// beyond growing the economy.
    charged: [u16; N],
    /// Total gas the non-optional items cost. Optional items reserve their own
    /// gas when the search picks them.
    pub gas: u32,
}

/// Parses a goal. Names several races share, like "gas" or "worker", resolve
/// to the race of the rest of the goal, or to `hint` when nothing else decides.
pub fn parse_goal(input: &str, hint: Race) -> Result<Vec<(Id, u16)>, String> {
    let mut parts = Vec::new();
    for part in input.split([',', '+', ';']).flat_map(|p| p.split(" and ")) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (count, name) = split_count(part);
        let ids = data::lookup(name);
        if ids.is_empty() {
            return Err(format!("unknown unit or structure: {name:?}"));
        }
        if count == 0 {
            return Err(format!("count for {name:?} must be at least 1"));
        }
        parts.push((ids, count));
    }
    if parts.is_empty() {
        return Err("empty goal".into());
    }

    let fits = |race: Race| parts.iter().all(|(ids, _)| ids.iter().any(|&id| data::race(id) == race));
    let race = [hint].into_iter().chain(Race::ALL).find(|&r| fits(r)).ok_or("goal mixes races")?;
    Ok(parts
        .into_iter()
        .map(|(ids, n)| (ids.into_iter().find(|&id| data::race(id) == race).unwrap(), n))
        .collect())
}

/// Accepts "5 roach", "5x roach", "5roach", "roach=5", "roach 5", "roach".
fn split_count(part: &str) -> (u16, &str) {
    if let Some((name, n)) = part.split_once('=') {
        return (n.trim().parse().unwrap_or(0), name.trim());
    }
    let digits = part.find(|c: char| !c.is_ascii_digit()).unwrap_or(part.len());
    if digits > 0 {
        let rest = part[digits..].trim_start();
        let rest = rest.strip_prefix(['x', '*']).filter(|r| !r.is_empty()).unwrap_or(rest);
        return (part[..digits].parse().unwrap_or(0), rest.trim());
    }
    if let Some((name, n)) = part.rsplit_once(' ')
        && let Ok(n) = n.parse()
    {
        return (n, name.trim());
    }
    (1, part)
}

impl Plan {
    /// `goal` must not be empty, and all its items must be of one race, as
    /// `parse_goal` guarantees.
    pub fn new(goal: &[(Id, u16)]) -> Plan {
        let race = data::race(goal[0].0);
        let mut want = [0u16; N];
        for &(id, n) in goal {
            want[slot(id)] += n;
        }
        let goal_counts = want;

        // A morph consumes its source unit, so the source has to be made too.
        let mut units = want;
        for id in race.items() {
            if let Source::Morph(src) = data::def(id).source {
                units[slot(src)] += units[slot(id)];
            }
        }

        let mut required = [0u16; N];
        for id in race.items() {
            let need = units[slot(id)].saturating_sub(initial_count(id));
            required[slot(id)] = need.div_ceil(data::def(id).yields);
        }

        // Pull in the tech each required item depends on.
        let mut stack: Vec<Id> = race.items().filter(|&id| required[slot(id)] > 0).collect();
        while let Some(id) = stack.pop() {
            let def = data::def(id);
            let building = match def.source {
                Source::Building(b) | Source::TechLab(b) | Source::AddOn(b) => Some(b),
                _ => None,
            };
            for &r in def.requires.iter().chain(building.iter()) {
                if required[slot(r)] == 0 && initial_count(r) == 0 {
                    required[slot(r)] = 1;
                    stack.push(r);
                }
            }
        }

        let (mut essential, mut charged) = (required, required);
        for id in race.items() {
            if data::is_optional(id) {
                essential[slot(id)] = 0;
            }
            if data::is_economy(id) {
                charged[slot(id)] = 0;
            }
        }

        let gas = race.items().map(|id| essential[slot(id)] as u32 * data::def(id).gas).sum();

        Plan { race, goal: goal_counts, required, essential, charged, gas }
    }

    pub fn goal(&self, id: Id) -> u16 {
        self.goal[slot(id)]
    }

    pub fn required(&self, id: Id) -> u16 {
        self.required[slot(id)]
    }

    pub fn essential(&self, id: Id) -> u16 {
        self.essential[slot(id)]
    }

    pub fn charged(&self, id: Id) -> u16 {
        self.charged[slot(id)]
    }

    /// An item the goal needs more than 200 of. A 200-supply army never does,
    /// and the simulation stores counts as u8.
    pub fn oversized(&self) -> Option<Id> {
        self.race.items().find(|&id| {
            let units = (initial_count(id) as u32 + self.required(id) as u32) * data::def(id).yields as u32;
            self.goal(id) > 200 || units > 200
        })
    }

    pub fn describe(goal: &[(Id, u16)]) -> String {
        goal.iter()
            .map(|&(id, n)| format!("{n} {}", data::def(id).name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What the player starts with on a standard ladder map.
pub fn initial_count(id: Id) -> u16 {
    let race = data::race(id);
    if id == race.worker() {
        data::START_WORKERS
    } else if id == race.town_hall() || id == Id::Overlord {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Id::*;

    fn parse(input: &str) -> Result<Vec<(Id, u16)>, String> {
        parse_goal(input, Race::Zerg)
    }

    #[test]
    fn parses_common_forms() {
        assert_eq!(parse("5 roaches").unwrap(), vec![(Roach, 5)]);
        assert_eq!(parse("roach=5, 2x ravager").unwrap(), vec![(Roach, 5), (Ravager, 2)]);
        assert_eq!(parse("ling speed and 20 lings").unwrap(), vec![(MetabolicBoost, 1), (Zergling, 20)]);
        assert!(parse("5 marines and 5 lings").is_err());
    }

    #[test]
    fn shared_names_follow_the_rest_of_the_goal() {
        assert_eq!(parse("5 marines, 2 gas").unwrap(), vec![(Marine, 5), (Refinery, 2)]);
        assert_eq!(parse("worker=20, stalker").unwrap(), vec![(Probe, 20), (Stalker, 1)]);
        assert_eq!(parse("gas").unwrap(), vec![(Extractor, 1)]);
        assert_eq!(parse_goal("gas", Race::Protoss).unwrap(), vec![(Assimilator, 1)]);
    }

    #[test]
    fn roaches_need_pool_warren_and_gas() {
        let plan = Plan::new(&[(Roach, 5)]);
        assert_eq!(plan.required(Roach), 5);
        assert_eq!(plan.required(RoachWarren), 1);
        assert_eq!(plan.required(SpawningPool), 1);
        assert_eq!(plan.required(Lair), 0);
        assert_eq!(plan.gas, 125);
    }

    #[test]
    fn morphs_require_their_source() {
        let plan = Plan::new(&[(Roach, 2), (Ravager, 1)]);
        assert_eq!(plan.required(Roach), 3);
        let plan = Plan::new(&[(Baneling, 4)]);
        assert_eq!(plan.required(Zergling), 2);
        assert_eq!(plan.required(BanelingNest), 1);
    }

    #[test]
    fn rejects_more_than_a_maxed_army() {
        assert_eq!(Plan::new(&[(Zergling, 200)]).oversized(), None);
        assert_eq!(Plan::new(&[(Zergling, 202)]).oversized(), Some(Zergling));
        assert_eq!(Plan::new(&[(Roach, 150), (Ravager, 100)]).oversized(), Some(Roach));
    }

    #[test]
    fn upgrades_require_their_building() {
        let plan = Plan::new(&[(GlialReconstitution, 1)]);
        assert_eq!(plan.required(RoachWarren), 1);
        assert_eq!(plan.required(Lair), 1);
    }

    #[test]
    fn tanks_need_a_factory_with_a_tech_lab() {
        let plan = Plan::new(&[(SiegeTank, 2)]);
        assert_eq!(plan.race, Race::Terran);
        assert_eq!(plan.required(FactoryTechLab), 1);
        assert_eq!(plan.required(Factory), 1);
        assert_eq!(plan.required(Barracks), 1);
        assert_eq!(plan.required(SupplyDepot), 1);
        // Production is optional, so its gas is reserved when it is started.
        assert_eq!(plan.gas, 250);
    }

    #[test]
    fn protoss_structures_need_a_pylon() {
        let plan = Plan::new(&[(Stalker, 4)]);
        assert_eq!(plan.race, Race::Protoss);
        assert_eq!(plan.required(Pylon), 1);
        assert_eq!(plan.required(Gateway), 1);
        assert_eq!(plan.required(CyberneticsCore), 1);
        assert_eq!(plan.gas, 200);
    }
}

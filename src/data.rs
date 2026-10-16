//! Zerg tech tree, costs and timings.
//!
//! All times are in real seconds at "Faster" game speed (what the in-game clock
//! shows in Legacy of the Void). Values follow the 5.0.x balance patches; check
//! them against Liquipedia when tuning, since the rest of the code depends on them.
//!
//! Supply is stored in half-supply units so a single zergling (0.5) is exact.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Id {
    // Units
    Drone,
    Overlord,
    Zergling,
    Queen,
    Roach,
    Ravager,
    Baneling,
    Hydralisk,
    Lurker,
    Mutalisk,
    Corruptor,
    BroodLord,
    Infestor,
    SwarmHost,
    Viper,
    Ultralisk,
    Overseer,
    // Structures
    Hatchery,
    MacroHatchery,
    Extractor,
    SpawningPool,
    EvolutionChamber,
    RoachWarren,
    BanelingNest,
    SpineCrawler,
    SporeCrawler,
    Lair,
    HydraliskDen,
    LurkerDen,
    InfestationPit,
    Spire,
    Hive,
    GreaterSpire,
    UltraliskCavern,
    // Research
    MetabolicBoost,
    GlialReconstitution,
    CentrifugalHooks,
    GroovedSpines,
    MuscularAugments,
    PneumatizedCarapace,
    Burrow,
    MissileWeapons1,
    MeleeWeapons1,
    GroundCarapace1,
}

/// Where an item comes from, which also decides what it occupies or consumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Morphs from a larva.
    Larva,
    /// A drone morphs into the structure and is consumed.
    Worker,
    /// Morphs from a completed unit of the given type, consuming it.
    Morph(Id),
    /// Trained or researched at an idle hatchery (queens, lair, hive, ...).
    TownHall,
    /// Researched at an idle structure of the given type.
    Building(Id),
}

pub struct Def {
    pub id: Id,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub minerals: u32,
    pub gas: u32,
    pub time: f64,
    /// Supply cost in half units. For morphs this is the difference to the source.
    pub supply: u32,
    /// Supply provided in half units.
    pub provides: u32,
    /// Units produced per action (zerglings come in pairs).
    pub yields: u16,
    pub source: Source,
    pub requires: &'static [Id],
}

const fn d(
    id: Id,
    name: &'static str,
    aliases: &'static [&'static str],
    (minerals, gas, time): (u32, u32, f64),
    supply: u32,
    source: Source,
    requires: &'static [Id],
) -> Def {
    Def { id, name, aliases, minerals, gas, time, supply, provides: 0, yields: 1, source, requires }
}

use Id::*;
use Source::*;

pub static DEFS: [Def; 44] = [
    d(Drone, "Drone", &["drone", "worker", "probe"], (50, 0, 12.0), 2, Larva, &[]),
    Def { provides: 16, ..d(Overlord, "Overlord", &["overlord", "ovi", "ovie"], (100, 0, 18.0), 0, Larva, &[]) },
    Def { yields: 2, ..d(Zergling, "Zergling", &["zergling", "ling"], (50, 0, 17.0), 2, Larva, &[SpawningPool]) },
    d(Queen, "Queen", &["queen"], (150, 0, 36.0), 4, TownHall, &[SpawningPool]),
    d(Roach, "Roach", &["roach"], (75, 25, 19.0), 4, Larva, &[RoachWarren]),
    d(Ravager, "Ravager", &["ravager"], (25, 75, 9.0), 2, Morph(Roach), &[RoachWarren]),
    d(Baneling, "Baneling", &["baneling", "bane", "bling"], (25, 25, 14.0), 0, Morph(Zergling), &[BanelingNest]),
    d(Hydralisk, "Hydralisk", &["hydralisk", "hydra"], (100, 50, 24.0), 4, Larva, &[HydraliskDen]),
    d(Lurker, "Lurker", &["lurker"], (50, 100, 18.0), 2, Morph(Hydralisk), &[LurkerDen]),
    d(Mutalisk, "Mutalisk", &["mutalisk", "muta"], (100, 100, 24.0), 4, Larva, &[Spire]),
    d(Corruptor, "Corruptor", &["corruptor"], (150, 100, 29.0), 4, Larva, &[Spire]),
    d(BroodLord, "Brood Lord", &["broodlord", "brood lord", "bl"], (150, 150, 24.0), 4, Morph(Corruptor), &[GreaterSpire]),
    d(Infestor, "Infestor", &["infestor"], (100, 150, 36.0), 4, Larva, &[InfestationPit]),
    d(SwarmHost, "Swarm Host", &["swarmhost", "swarm host", "host"], (100, 75, 29.0), 6, Larva, &[InfestationPit]),
    d(Viper, "Viper", &["viper"], (100, 200, 29.0), 6, Larva, &[Hive]),
    d(Ultralisk, "Ultralisk", &["ultralisk", "ultra"], (275, 200, 39.0), 12, Larva, &[UltraliskCavern]),
    d(Overseer, "Overseer", &["overseer"], (50, 50, 12.0), 0, Morph(Overlord), &[Lair]),
    Def { provides: 12, ..d(Hatchery, "Hatchery (expand)", &["hatchery", "hatch", "expansion", "expand", "natural", "base"], (300, 0, 71.0), 0, Worker, &[]) },
    Def { provides: 12, ..d(MacroHatchery, "Hatchery (macro)", &["macro hatchery", "macro hatch", "macrohatch"], (300, 0, 71.0), 0, Worker, &[]) },
    d(Extractor, "Extractor", &["extractor", "gas", "geyser"], (25, 0, 21.0), 0, Worker, &[]),
    d(SpawningPool, "Spawning Pool", &["spawning pool", "spawningpool", "pool"], (200, 0, 46.0), 0, Worker, &[]),
    d(EvolutionChamber, "Evolution Chamber", &["evolution chamber", "evolutionchamber", "evo"], (75, 0, 25.0), 0, Worker, &[]),
    d(RoachWarren, "Roach Warren", &["roach warren", "roachwarren", "warren"], (150, 0, 39.0), 0, Worker, &[SpawningPool]),
    d(BanelingNest, "Baneling Nest", &["baneling nest", "banelingnest", "nest", "bane nest"], (100, 50, 43.0), 0, Worker, &[SpawningPool]),
    d(SpineCrawler, "Spine Crawler", &["spine crawler", "spinecrawler", "spine"], (100, 0, 36.0), 0, Worker, &[SpawningPool]),
    d(SporeCrawler, "Spore Crawler", &["spore crawler", "sporecrawler", "spore"], (75, 0, 21.0), 0, Worker, &[SpawningPool]),
    d(Lair, "Lair", &["lair"], (150, 100, 57.0), 0, TownHall, &[SpawningPool]),
    d(HydraliskDen, "Hydralisk Den", &["hydralisk den", "hydraliskden", "hydra den", "den"], (100, 100, 29.0), 0, Worker, &[Lair]),
    d(LurkerDen, "Lurker Den", &["lurker den", "lurkerden"], (100, 150, 57.0), 0, Worker, &[HydraliskDen]),
    d(InfestationPit, "Infestation Pit", &["infestation pit", "infestationpit", "pit"], (100, 100, 36.0), 0, Worker, &[Lair]),
    d(Spire, "Spire", &["spire"], (200, 200, 71.0), 0, Worker, &[Lair]),
    d(Hive, "Hive", &["hive"], (200, 150, 71.0), 0, TownHall, &[Lair, InfestationPit]),
    d(GreaterSpire, "Greater Spire", &["greater spire", "greaterspire"], (100, 150, 71.0), 0, Building(Spire), &[Hive]),
    d(UltraliskCavern, "Ultralisk Cavern", &["ultralisk cavern", "ultraliskcavern", "cavern"], (150, 200, 46.0), 0, Worker, &[Hive]),
    d(MetabolicBoost, "Metabolic Boost", &["metabolic boost", "metabolicboost", "ling speed", "speed"], (100, 100, 79.0), 0, Building(SpawningPool), &[]),
    d(GlialReconstitution, "Glial Reconstitution", &["glial reconstitution", "glial", "roach speed"], (100, 100, 79.0), 0, Building(RoachWarren), &[Lair]),
    d(CentrifugalHooks, "Centrifugal Hooks", &["centrifugal hooks", "hooks", "bane speed"], (100, 100, 71.0), 0, Building(BanelingNest), &[Lair]),
    d(GroovedSpines, "Grooved Spines", &["grooved spines", "hydra range"], (100, 100, 71.0), 0, Building(HydraliskDen), &[]),
    d(MuscularAugments, "Muscular Augments", &["muscular augments", "hydra speed"], (100, 100, 71.0), 0, Building(HydraliskDen), &[]),
    d(PneumatizedCarapace, "Pneumatized Carapace", &["pneumatized carapace", "overlord speed"], (100, 100, 43.0), 0, TownHall, &[]),
    d(Burrow, "Burrow", &["burrow"], (100, 100, 71.0), 0, TownHall, &[]),
    d(MissileWeapons1, "Missile Attacks 1", &["missile attacks 1", "missile 1", "+1 range", "+1 missile"], (100, 100, 114.0), 0, Building(EvolutionChamber), &[]),
    d(MeleeWeapons1, "Melee Attacks 1", &["melee attacks 1", "melee 1", "+1 melee"], (100, 100, 114.0), 0, Building(EvolutionChamber), &[]),
    d(GroundCarapace1, "Ground Carapace 1", &["ground carapace 1", "carapace 1", "+1 carapace", "+1 armor"], (150, 150, 114.0), 0, Building(EvolutionChamber), &[]),
];

pub const N: usize = DEFS.len();

pub fn def(id: Id) -> &'static Def {
    &DEFS[id as usize]
}

pub fn all() -> impl Iterator<Item = Id> {
    DEFS.iter().map(|d| d.id)
}

/// Items the search may add on its own to grow the economy, beyond what the
/// goal strictly requires.
pub fn is_economy(id: Id) -> bool {
    matches!(id, Drone | Overlord | Queen | Extractor | Hatchery | MacroHatchery)
}

pub fn lookup(name: &str) -> Option<Id> {
    let name = name.trim().to_lowercase();
    let find = |n: &str| DEFS.iter().find(|d| d.aliases.contains(&n)).map(|d| d.id);
    find(&name)
        .or_else(|| name.strip_suffix("es").and_then(find))
        .or_else(|| name.strip_suffix('s').and_then(find))
}

// Game constants (real seconds, Faster speed).

/// Minerals per second for each of the first two workers on a patch.
pub const MINERAL_RATE: f64 = 0.94;
/// Minerals per second for a third worker on a patch (heavily diminished).
pub const MINERAL_RATE_THIRD: f64 = 0.40;
pub const PATCHES_PER_BASE: u32 = 8;
/// Gas per second per worker; three workers saturate a geyser.
pub const GAS_RATE: f64 = 0.89;
pub const WORKERS_PER_GEYSER: u32 = 3;
pub const GEYSERS_PER_BASE: u16 = 2;
pub const LARVA_INTERVAL: f64 = 11.0;
pub const NATURAL_LARVA_CAP: u8 = 3;
pub const MAX_LARVA: u8 = 19;
pub const INJECT_TIME: f64 = 29.0;
pub const INJECT_LARVA: u8 = 3;
pub const INJECT_ENERGY: f64 = 25.0;
pub const QUEEN_START_ENERGY: f64 = 25.0;
pub const QUEEN_MAX_ENERGY: f64 = 200.0;
pub const QUEEN_ENERGY_REGEN: f64 = 0.7875;
/// Larvae on a freshly completed hatchery.
pub const NEW_HATCH_LARVA: u8 = 1;
pub const MAX_SUPPLY: u32 = 400;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_indexed_by_id() {
        for (i, d) in DEFS.iter().enumerate() {
            assert_eq!(d.id as usize, i, "{} is out of order", d.name);
        }
    }

    #[test]
    fn lookup_handles_plurals_and_aliases() {
        assert_eq!(lookup("roaches"), Some(Roach));
        assert_eq!(lookup("Lings"), Some(Zergling));
        assert_eq!(lookup("pool"), Some(SpawningPool));
        assert_eq!(lookup("hydras"), Some(Hydralisk));
        assert_eq!(lookup("nope"), None);
    }
}

//! The figure list every reader shares (openspec AE.4, design decision 4):
//! the routes, the driver worker, Kaos, the crash watchdog and the product's
//! `sky_edit` all read [`FigureList::now`]. The product fills it once at boot
//! from the firmware pack and never replaces it; guacamole boots with it empty
//! and publishes the disc roster when it lands (a current extract at start, or
//! the roster stage of the extraction a Giants pick starts), the way AA.15
//! made the game list replaceable.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use guac_ir::roster::{Category as RCategory, ENGLISH, Element as RElement, FigureKind};
use guac_ir::{Roster, RosterFigure};
use serde::Serialize;
use skylander_core::{Category, Element, Figure, FigureId, GameOfOrigin};
use tokio::sync::watch;
use tracing::warn;

/// Where guacamole's roster stands, as `/api/status` says it; the product's
/// status carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RosterState {
    /// A Giants dump is set up and its roster hasn't landed yet.
    Reading,
    /// No Giants dump guacamole recognizes: none found, or an unknown one.
    Unrecognized,
    /// The dump or its roster couldn't be read.
    Unreadable,
    Ready,
}

/// One version of the list: the figures, their id index, guacamole's
/// portraits and its roster state.
#[derive(Debug, Default)]
pub struct FigureSet {
    figures: Vec<Figure>,
    index: HashMap<FigureId, usize>,
    /// guacamole: each figure's PNG in the extract.
    portraits: HashMap<FigureId, PathBuf>,
    /// guacamole only; `None` on the product.
    roster: Option<RosterState>,
}

impl FigureSet {
    /// The product's: the firmware pack's figures, no roster.
    pub fn pack(figures: Vec<Figure>) -> Self {
        Self::new(figures, HashMap::new(), None)
    }

    /// guacamole before its roster lands: no figures.
    pub fn waiting(state: RosterState) -> Self {
        Self::new(Vec::new(), HashMap::new(), Some(state))
    }

    /// guacamole's list off the disc roster, its portraits under `extract`
    /// (the identity dir the roster's paths are relative to). English names
    /// (design decision 3); a figure the disc has no English name for is left
    /// out and logged.
    pub fn from_roster(roster: &Roster, extract: &Path) -> Self {
        let mut figures = Vec::with_capacity(roster.figures.len());
        let mut portraits = HashMap::new();
        for f in &roster.figures {
            let Some(figure) = figure(roster, f) else {
                warn!(id = f.id(), "roster: no English name; left out");
                continue;
            };
            if let Some(rel) = f.portrait.as_deref().filter(|rel| plain(rel)) {
                portraits.insert(figure.id.clone(), extract.join(rel));
            }
            figures.push(figure);
        }
        Self::new(figures, portraits, Some(RosterState::Ready))
    }

    fn new(
        figures: Vec<Figure>,
        portraits: HashMap<FigureId, PathBuf>,
        roster: Option<RosterState>,
    ) -> Self {
        let index = figures
            .iter()
            .enumerate()
            .map(|(i, f)| (f.id.clone(), i))
            .collect();
        Self {
            figures,
            index,
            portraits,
            roster,
        }
    }

    pub fn figures(&self) -> &[Figure] {
        &self.figures
    }

    pub fn get(&self, id: &FigureId) -> Option<&Figure> {
        self.index.get(id).and_then(|i| self.figures.get(*i))
    }

    /// guacamole's portrait for `id`, when the disc has one.
    pub fn portrait(&self, id: &FigureId) -> Option<&Path> {
        self.portraits.get(id).map(PathBuf::as_path)
    }

    pub fn roster(&self) -> Option<RosterState> {
        self.roster
    }

    /// guacamole with no roster yet: the list is empty for a reason.
    pub fn is_waiting(&self) -> bool {
        matches!(self.roster, Some(s) if s != RosterState::Ready)
    }
}

#[cfg(feature = "test-hooks")]
impl FigureSet {
    /// This list with `figure` beside it: a toy the disc's roster doesn't
    /// hold, which a later game's catalogue would bring, for the TV's alert
    /// for a toy Giants can't use (openspec AG.73). `None` when its id is
    /// listed already.
    pub fn with_foreign(&self, figure: Figure) -> Option<Self> {
        if self.index.contains_key(&figure.id) {
            return None;
        }
        let mut figures = self.figures.clone();
        figures.push(figure);
        Some(Self::new(figures, self.portraits.clone(), self.roster))
    }
}

/// A roster row as the phone's collection groups it: variant group the
/// character's name, tag the disc's variant word (design decision 3).
fn figure(roster: &Roster, f: &RosterFigure) -> Option<Figure> {
    let name = roster.display_name(f, ENGLISH)?;
    let group = roster.text(&f.name, ENGLISH)?.to_owned();
    let tag = match &f.variant_word {
        Some(word) => roster.text(word, ENGLISH)?.to_owned(),
        None => "base".to_owned(),
    };
    Some(Figure {
        id: FigureId::new(f.id()),
        canonical_name: name,
        variant_group: group,
        variant_tag: tag,
        game: game(f),
        element: f.element.map(element),
        category: category(f.category),
        vehicle_terrain: None,
        sky_path: PathBuf::new(),
        element_icon_path: None,
        tag_identity: None,
    })
}

/// A sky1 toy's own figure is Spyro's Adventure's; everything else on the
/// disc is Giants', the alt decos, Series 2 and LightCore figures of sky1
/// toys included, since Giants brought those.
fn game(f: &RosterFigure) -> GameOfOrigin {
    if f.sky1 && f.kind == FigureKind::Base {
        GameOfOrigin::SpyrosAdventure
    } else {
        GameOfOrigin::Giants
    }
}

fn element(e: RElement) -> Element {
    match e {
        RElement::Magic => Element::Magic,
        RElement::Water => Element::Water,
        RElement::Air => Element::Air,
        RElement::Undead => Element::Undead,
        RElement::Tech => Element::Tech,
        RElement::Fire => Element::Fire,
        RElement::Earth => Element::Earth,
        RElement::Life => Element::Life,
    }
}

fn category(c: RCategory) -> Category {
    match c {
        RCategory::Figure => Category::Figure,
        RCategory::Giant => Category::Giant,
        RCategory::Item => Category::Item,
        RCategory::AdventurePack => Category::AdventurePack,
        RCategory::Sidekick => Category::Sidekick,
    }
}

/// Plain relative names only, so a portrait path can't leave the extract.
fn plain(rel: &str) -> bool {
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// The shared, replaceable list (the game list's pattern, AA.15): readers take
/// a snapshot with [`Self::now`]; a publish swaps it whole and wakes
/// [`Self::subscribe`]rs (the held resume prompts, AE.4).
#[derive(Clone)]
pub struct FigureList(Arc<watch::Sender<Arc<FigureSet>>>);

impl FigureList {
    pub fn new(set: FigureSet) -> Self {
        Self(Arc::new(watch::Sender::new(Arc::new(set))))
    }

    pub fn now(&self) -> Arc<FigureSet> {
        self.0.borrow().clone()
    }

    /// Replaces the list; every reader sees it from its next snapshot.
    pub fn publish(&self, set: FigureSet) {
        self.0.send_replace(Arc::new(set));
    }

    /// guacamole's roster state while no roster has landed; a landed roster
    /// stays.
    pub fn set_roster_state(&self, state: RosterState) {
        self.0.send_if_modified(|set| {
            let change = set.is_waiting() && set.roster != Some(state);
            if change {
                *set = Arc::new(FigureSet::waiting(state));
            }
            change
        });
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<FigureSet>> {
        self.0.subscribe()
    }
}

impl std::fmt::Debug for FigureList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let now = self.now();
        f.debug_struct("FigureList")
            .field("figures", &now.figures.len())
            .field("roster", &now.roster)
            .finish()
    }
}

/// guacamole's ids AA's interim catalogue gave 12 variants, and the disc's
/// (design decision 2): migration `20261018000003` rewrites each.
pub const AA_IDS_ON_THE_DISC: [(&str, &str); 12] = [
    ("000000-0802", "000000-0402"),
    ("000012-0802", "000012-0402"),
    ("00000a-0803", "00000a-0403"),
    ("00000f-0803", "00000f-0403"),
    ("00001a-0803", "00001a-0403"),
    ("000066-0202", "000066-0402"),
    ("00006d-0202", "00006d-0402"),
    ("000070-0202", "000070-0402"),
    ("000069-0002", "000069-0402"),
    ("00006c-0002", "00006c-0402"),
    ("00006a-0203", "00006a-0603"),
    ("00006e-0203", "00006e-0403"),
];

/// Rosters for the server's tests, built in code (no disc data).
#[cfg(test)]
pub(crate) mod fixture {
    use guac_ir::roster::{
        Category as RCategory, ENGLISH, Element as RElement, FigureKind, Language, Text,
    };
    use guac_ir::{DumpIdentity, Roster, RosterFigure, Sha256};

    fn text(key: &str, en: &str) -> Text {
        Text {
            key: key.into(),
            texts: vec![en.into()],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn row(
        toy_type: u32,
        variant: u16,
        kind: FigureKind,
        name: &str,
        word: Option<&str>,
        element: Option<RElement>,
        category: RCategory,
        sky1: bool,
    ) -> RosterFigure {
        RosterFigure {
            toy_type,
            variant,
            kind,
            deco: None,
            name: text("name", name),
            variant_word: word.map(|w| text("word", w)),
            element,
            category,
            load_file: format!("{toy_type:03}_x"),
            portrait: Some(format!("portraits/{toy_type:06x}-{variant:04x}.png")),
            sky1,
        }
    }

    pub(crate) fn roster(figures: Vec<RosterFigure>) -> Roster {
        Roster {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([1; 32]),
            },
            languages: vec![Language {
                id: ENGLISH,
                name: "ENGLISH".into(),
            }],
            figures,
        }
    }

    /// Eruptor and Spyro, the two most tests place.
    pub(crate) fn eruptor_and_spyro() -> Roster {
        roster(vec![
            row(
                9,
                0,
                FigureKind::Base,
                "Eruptor",
                None,
                Some(RElement::Fire),
                RCategory::Figure,
                true,
            ),
            row(
                16,
                0,
                FigureKind::Base,
                "Spyro",
                None,
                Some(RElement::Magic),
                RCategory::Figure,
                true,
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{roster, row};
    use super::*;

    /// AG.73's test hook: a figure the roster doesn't hold joins the list
    /// once, the roster's figures and state kept; its id listed already, it
    /// doesn't.
    #[cfg(feature = "test-hooks")]
    #[test]
    fn a_foreign_figure_joins_the_list_once() {
        let set = FigureSet::from_roster(&fixture::eruptor_and_spyro(), Path::new("/x"));
        let fryno = |id: &str| Figure {
            id: FigureId::new(id),
            canonical_name: "Fryno".into(),
            variant_group: "Fryno".into(),
            variant_tag: "base".into(),
            game: GameOfOrigin::SwapForce,
            element: None,
            category: Category::Figure,
            vehicle_terrain: None,
            sky_path: PathBuf::new(),
            element_icon_path: None,
            tag_identity: None,
        };
        let with = set.with_foreign(fryno("0001c2-0000")).expect("listed");
        assert_eq!(with.figures().len(), 3);
        assert_eq!(
            with.get(&FigureId::new("0001c2-0000"))
                .unwrap()
                .canonical_name,
            "Fryno"
        );
        assert!(with.get(&FigureId::new("000009-0000")).is_some());
        assert_eq!(with.roster(), Some(RosterState::Ready));
        assert!(with.with_foreign(fryno("0001c2-0000")).is_none());
        assert!(
            set.with_foreign(fryno("000009-0000")).is_none(),
            "Eruptor's id"
        );
    }

    #[test]
    fn a_roster_becomes_the_collections_figures() {
        use FigureKind::*;
        let r = roster(vec![
            row(
                16,
                0,
                Base,
                "Spyro",
                None,
                Some(RElement::Magic),
                RCategory::Figure,
                true,
            ),
            row(
                16,
                0x800,
                Series2,
                "Spyro",
                Some("Series 2"),
                Some(RElement::Magic),
                RCategory::Figure,
                true,
            ),
            row(
                28,
                0,
                Base,
                "Spyro",
                Some("Dark"),
                Some(RElement::Magic),
                RCategory::Figure,
                true,
            ),
            row(
                0,
                0x402,
                Alt,
                "Whirlwind",
                Some("Polar"),
                Some(RElement::Air),
                RCategory::Figure,
                true,
            ),
            row(
                112,
                0,
                Base,
                "Tree Rex",
                None,
                Some(RElement::Life),
                RCategory::Giant,
                false,
            ),
            row(
                205,
                0,
                Base,
                "Sky-Iron Shield",
                None,
                None,
                RCategory::Item,
                true,
            ),
            row(
                304,
                0,
                Base,
                "Volcanic Vault",
                None,
                Some(RElement::Fire),
                RCategory::AdventurePack,
                true,
            ),
            row(
                540,
                0,
                Base,
                "Barkley",
                None,
                Some(RElement::Life),
                RCategory::Sidekick,
                false,
            ),
        ]);
        let set = FigureSet::from_roster(&r, Path::new("/x/extract/BLUS30968-01.00-01010101"));
        assert_eq!(set.roster(), Some(RosterState::Ready));
        assert!(!set.is_waiting());
        let get = |id: &str| set.get(&FigureId::new(id)).unwrap();
        let rows: Vec<(&str, &str, &str, GameOfOrigin)> = set
            .figures()
            .iter()
            .map(|f| {
                (
                    f.canonical_name.as_str(),
                    f.variant_group.as_str(),
                    f.variant_tag.as_str(),
                    f.game,
                )
            })
            .collect();
        use GameOfOrigin::*;
        assert_eq!(
            rows,
            [
                ("Spyro", "Spyro", "base", SpyrosAdventure),
                ("Series 2 Spyro", "Spyro", "Series 2", Giants),
                ("Dark Spyro", "Spyro", "Dark", SpyrosAdventure),
                ("Polar Whirlwind", "Whirlwind", "Polar", Giants),
                ("Tree Rex", "Tree Rex", "base", Giants),
                (
                    "Sky-Iron Shield",
                    "Sky-Iron Shield",
                    "base",
                    SpyrosAdventure
                ),
                ("Volcanic Vault", "Volcanic Vault", "base", SpyrosAdventure),
                ("Barkley", "Barkley", "base", Giants),
            ]
        );
        assert_eq!(get("000070-0000").category, Category::Giant);
        assert_eq!(get("000070-0000").element, Some(Element::Life));
        assert_eq!(get("0000cd-0000").category, Category::Item);
        assert_eq!(get("0000cd-0000").element, None);
        assert_eq!(get("000130-0000").category, Category::AdventurePack);
        assert_eq!(get("00021c-0000").category, Category::Sidekick);
        assert!(
            set.figures()
                .iter()
                .all(|f| f.sky_path.as_os_str().is_empty())
        );
        assert_eq!(
            set.portrait(&FigureId::new("000000-0402")),
            Some(Path::new(
                "/x/extract/BLUS30968-01.00-01010101/portraits/000000-0402.png"
            ))
        );
    }

    #[test]
    fn a_portrait_path_that_leaves_the_extract_and_a_nameless_row_are_dropped() {
        let mut escaping = row(
            9,
            0,
            FigureKind::Base,
            "Eruptor",
            None,
            Some(RElement::Fire),
            RCategory::Figure,
            true,
        );
        escaping.portrait = Some("../../etc/passwd".into());
        let mut nameless = row(
            10,
            0,
            FigureKind::Base,
            "Ignitor",
            None,
            Some(RElement::Fire),
            RCategory::Figure,
            true,
        );
        nameless.name.texts.clear();
        let set = FigureSet::from_roster(&roster(vec![escaping, nameless]), Path::new("/x"));
        assert_eq!(set.figures().len(), 1);
        assert_eq!(set.portrait(&FigureId::new("000009-0000")), None);
        assert!(set.get(&FigureId::new("00000a-0000")).is_none());
    }

    #[test]
    fn a_state_change_never_unsets_a_landed_roster() {
        let list = FigureList::new(FigureSet::waiting(RosterState::Unrecognized));
        let mut rx = list.subscribe();
        list.set_roster_state(RosterState::Reading);
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().roster(), Some(RosterState::Reading));
        // The same state again wakes nobody.
        list.set_roster_state(RosterState::Reading);
        assert!(!rx.has_changed().unwrap());

        let r = roster(vec![row(
            9,
            0,
            FigureKind::Base,
            "Eruptor",
            None,
            Some(RElement::Fire),
            RCategory::Figure,
            true,
        )]);
        list.publish(FigureSet::from_roster(&r, Path::new("/x")));
        assert_eq!(rx.borrow_and_update().figures().len(), 1);
        list.set_roster_state(RosterState::Unreadable);
        assert!(!rx.has_changed().unwrap());
        assert_eq!(list.now().roster(), Some(RosterState::Ready));
        assert_eq!(list.now().figures().len(), 1);
        // The product's list has no state to set.
        let pack = FigureList::new(FigureSet::pack(Vec::new()));
        pack.set_roster_state(RosterState::Reading);
        assert_eq!(pack.now().roster(), None);
    }

    /// The id migration, run on a database at guacamole's earlier migrations
    /// that holds all 12 old ids in usage rows and saved layouts: exactly the
    /// disc's ids come out, everything else as it was (AE.4).
    #[tokio::test]
    async fn the_migration_rewrites_all_12_old_ids_and_nothing_else() {
        use skylander_core::{SLOT_COUNT, SlotState};
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

        const THIS: &str = "20261018000003_disc_roster_ids.sql";
        let tmp = tempfile::tempdir().unwrap();
        // The migrations before this one, as an older guacamole ran them.
        let earlier = tmp.path().join("migrations");
        std::fs::create_dir_all(&earlier).unwrap();
        let ours = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut copied = 0;
        for entry in std::fs::read_dir(&ours).unwrap() {
            let path = entry.unwrap().path();
            // Versions are the names' leading timestamps, so names sort in order.
            if path.file_name().unwrap().to_str().unwrap() < THIS {
                std::fs::copy(&path, earlier.join(path.file_name().unwrap())).unwrap();
                copied += 1;
            }
        }
        assert_eq!(copied, 2);
        let before = sqlx::migrate::Migrator::new(earlier).await.unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(tmp.path().join("db.sqlite"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new().connect_with(opts).await.unwrap();
        before.run(&pool).await.unwrap();

        let layout = |ids: &[&str]| {
            let slots: [SlotState; SLOT_COUNT] = std::array::from_fn(|i| match ids.get(i) {
                Some(id) => SlotState::Loaded {
                    figure_id: Some(FigureId::new(*id)),
                    display_name: (*id).into(),
                    placed_by: Some("a".into()),
                },
                None => SlotState::Empty,
            });
            serde_json::to_string(&slots).unwrap()
        };
        let old: Vec<&str> = AA_IDS_ON_THE_DISC.iter().map(|(o, _)| *o).collect();
        // Untouched: Eruptor, and Series 2 Whirlwind beside Polar Whirlwind's old id.
        let kept = ["000009-0000", "000000-0800"];
        let layouts = [
            ("a", layout(&old[..8])),
            ("b", layout(&[&old[8..], &kept[..]].concat())),
        ];
        for (profile, json) in &layouts {
            sqlx::query(
                "INSERT INTO profiles (id, display_name, pin_hash, color, created_at) \
                 VALUES (?1, ?1, 'x', '#000000', 'now')",
            )
            .bind(profile)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO sessions (profile_id, last_portal_layout_json, updated_at) \
                 VALUES (?1, ?2, 'now')",
            )
            .bind(profile)
            .bind(json)
            .execute(&pool)
            .await
            .unwrap();
        }
        for id in old.iter().chain(&kept) {
            sqlx::query(
                "INSERT INTO figure_usage (profile_id, figure_id, last_used_at) \
                 VALUES ('a', ?1, 'now')",
            )
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        }

        crate::setup::MIGRATOR.run(&pool).await.unwrap();

        let new: Vec<&str> = AA_IDS_ON_THE_DISC.iter().map(|(_, n)| *n).collect();
        let mut usage: Vec<String> =
            sqlx::query_scalar("SELECT figure_id FROM figure_usage ORDER BY figure_id")
                .fetch_all(&pool)
                .await
                .unwrap();
        usage.sort();
        let mut want: Vec<String> = new.iter().chain(&kept).map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(usage, want);
        let ids_in = |json: &str| -> Vec<String> {
            let slots: [SlotState; SLOT_COUNT] = serde_json::from_str(json).unwrap();
            slots
                .iter()
                .filter_map(|s| match s {
                    SlotState::Loaded { figure_id, .. } => {
                        figure_id.as_ref().map(|f| f.as_str().to_owned())
                    }
                    _ => None,
                })
                .collect()
        };
        for (profile, want) in [
            ("a", new[..8].to_vec()),
            ("b", [&new[8..], &kept[..]].concat()),
        ] {
            let json: String = sqlx::query_scalar(
                "SELECT last_portal_layout_json FROM sessions WHERE profile_id = ?1",
            )
            .bind(profile)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(ids_in(&json), want, "{profile}'s layout");
            for o in &old {
                assert!(!json.contains(o), "{o} left in {profile}'s layout");
            }
        }
        pool.close().await;
    }
}

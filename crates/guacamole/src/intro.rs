//! The intro queue (openspec AB, design decision 9): idle → playing(slot) →
//! idle with a FIFO queue behind it, driven by placements, removals, skips and
//! the player saying an intro ended. Pure, so the `guacamole/character-intros`
//! rules are tested without a window; `intro_player` plays what it says.

use std::collections::VecDeque;
use std::path::PathBuf;

use bevy::prelude::Resource;
use guac_extract::transcode::TARGET;
use serde::Serialize;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::Placement;

use crate::extraction::IntroLibrary;

/// One intro, queued or playing. `id` tells two plays of one movie apart, so a
/// late `Ended` never ends the wrong one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroEntry {
    pub id: u64,
    pub slot: SlotIndex,
    pub figure_id: String,
    pub movie: String,
}

/// What the queue hears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntroEvent {
    /// `movie` is `None` for a figure with no intro, or while intros aren't
    /// ready; either way the slot's old intro, if any, goes.
    Placed {
        slot: SlotIndex,
        figure_id: String,
        movie: Option<String>,
    },
    Cleared {
        slot: SlotIndex,
    },
    /// Any controller button or key.
    Skip,
    /// The player finished intro `id`.
    Ended {
        id: u64,
    },
    /// Off the portal screen: nothing plays.
    Reset,
}

/// Why a playing intro stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopWhy {
    Ended,
    Skipped,
    Removed,
    Reset,
}

/// What an event did, for the log and the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Queued(IntroEntry),
    Started(IntroEntry),
    /// Removed before its turn: it never plays.
    Dropped(IntroEntry),
    Stopped(IntroEntry, StopWhy),
}

#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct IntroQueue {
    playing: Option<IntroEntry>,
    queued: VecDeque<IntroEntry>,
    next_id: u64,
}

impl IntroQueue {
    pub fn playing(&self) -> Option<&IntroEntry> {
        self.playing.as_ref()
    }

    /// Waiting, in play order.
    pub fn queued(&self) -> impl Iterator<Item = &IntroEntry> {
        self.queued.iter()
    }

    pub fn apply(&mut self, event: IntroEvent) -> Vec<Change> {
        let mut changes = Vec::new();
        match event {
            IntroEvent::Placed {
                slot,
                figure_id,
                movie,
            } => {
                // A placement over an occupied slot replaces its figure.
                self.remove_slot(slot, &mut changes);
                if let Some(movie) = movie {
                    self.next_id += 1;
                    let entry = IntroEntry {
                        id: self.next_id,
                        slot,
                        figure_id,
                        movie,
                    };
                    changes.push(Change::Queued(entry.clone()));
                    self.queued.push_back(entry);
                }
            }
            IntroEvent::Cleared { slot } => self.remove_slot(slot, &mut changes),
            IntroEvent::Skip => self.stop(StopWhy::Skipped, &mut changes),
            IntroEvent::Ended { id } => {
                if self.playing.as_ref().is_some_and(|p| p.id == id) {
                    self.stop(StopWhy::Ended, &mut changes);
                }
            }
            IntroEvent::Reset => {
                changes.extend(self.queued.drain(..).map(Change::Dropped));
                self.stop(StopWhy::Reset, &mut changes);
            }
        }
        if self.playing.is_none()
            && let Some(next) = self.queued.pop_front()
        {
            changes.push(Change::Started(next.clone()));
            self.playing = Some(next);
        }
        changes
    }

    fn remove_slot(&mut self, slot: SlotIndex, changes: &mut Vec<Change>) {
        let (gone, kept) = std::mem::take(&mut self.queued)
            .into_iter()
            .partition(|e| e.slot == slot);
        self.queued = kept;
        changes.extend(gone.into_iter().map(Change::Dropped));
        if self.playing.as_ref().is_some_and(|p| p.slot == slot) {
            self.stop(StopWhy::Removed, changes);
        }
    }

    fn stop(&mut self, why: StopWhy, changes: &mut Vec<Change>) {
        if let Some(stopped) = self.playing.take() {
            changes.push(Change::Stopped(stopped, why));
        }
    }
}

/// The movie a placement plays: its toy type's intro in the extract's table,
/// so a variant sharing its base's toy type plays the base's. None without a
/// library (extraction running or failed) or an intro.
pub fn movie_for(library: Option<&IntroLibrary>, placement: &Placement) -> Option<String> {
    let character = library?.table.characters.get(&placement.toy_type.0)?;
    Some(character.intro.as_ref()?.movie.clone())
}

/// `(video, audio)` paths for `movie` in the extract.
pub fn files(library: &IntroLibrary, movie: &str) -> (PathBuf, PathBuf) {
    let (video, audio) = TARGET.outputs(movie);
    (library.dir.join(video), library.dir.join(audio))
}

/// A queued intro, from `IntroQueue`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueuedShot {
    /// 1-based.
    pub slot: u8,
    pub figure_id: String,
    pub movie: String,
}

impl From<&IntroEntry> for QueuedShot {
    fn from(e: &IntroEntry) -> Self {
        Self {
            slot: e.slot.as_u8() + 1,
            figure_id: e.figure_id.clone(),
            movie: e.movie.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use guac_ir::{Character, CharacterTable, DumpIdentity, Intro, IntroSource, VoiceTrack};
    use skylander_core::FigureId;

    use super::*;

    fn slot(n: u8) -> SlotIndex {
        SlotIndex::new(n).unwrap()
    }

    fn placed(n: u8, movie: &str) -> IntroEvent {
        IntroEvent::Placed {
            slot: slot(n),
            figure_id: format!("fig-{movie}"),
            movie: Some(movie.into()),
        }
    }

    fn cleared(n: u8) -> IntroEvent {
        IntroEvent::Cleared { slot: slot(n) }
    }

    fn playing(q: &IntroQueue) -> Option<&str> {
        q.playing().map(|p| p.movie.as_str())
    }

    fn queued(q: &IntroQueue) -> Vec<&str> {
        q.queued().map(|e| e.movie.as_str()).collect()
    }

    fn end(q: &mut IntroQueue) -> Vec<Change> {
        let id = q.playing().expect("something plays").id;
        q.apply(IntroEvent::Ended { id })
    }

    #[test]
    fn idle_to_playing_and_back() {
        let mut q = IntroQueue::default();
        let changes = q.apply(placed(0, "eruptor"));
        assert!(
            matches!(&changes[..], [Change::Queued(_), Change::Started(e)] if e.movie == "eruptor")
        );
        assert_eq!(playing(&q), Some("eruptor"));
        let changes = end(&mut q);
        assert!(matches!(&changes[..], [Change::Stopped(e, StopWhy::Ended)] if e.slot == slot(0)));
        assert_eq!(
            q,
            IntroQueue {
                next_id: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn placements_queue_in_placement_order() {
        let mut q = IntroQueue::default();
        for (n, movie) in [(3, "eruptor"), (0, "treerex"), (5, "spyro"), (1, "bash")] {
            q.apply(placed(n, movie));
        }
        assert_eq!(playing(&q), Some("eruptor"));
        assert_eq!(queued(&q), ["treerex", "spyro", "bash"]);
        let mut order = vec![playing(&q).unwrap().to_string()];
        while q.playing().is_some() {
            end(&mut q);
            order.extend(playing(&q).map(str::to_string));
        }
        assert_eq!(order, ["eruptor", "treerex", "spyro", "bash"]);
        assert!(q.queued().next().is_none());
    }

    #[test]
    fn removed_before_its_turn_never_plays() {
        let mut q = IntroQueue::default();
        q.apply(placed(0, "eruptor"));
        q.apply(placed(1, "treerex"));
        q.apply(placed(2, "spyro"));
        let changes = q.apply(cleared(1));
        assert!(matches!(&changes[..], [Change::Dropped(e)] if e.movie == "treerex"));
        assert_eq!(playing(&q), Some("eruptor"), "the playing one goes on");
        end(&mut q);
        assert_eq!(playing(&q), Some("spyro"));
        end(&mut q);
        assert_eq!(playing(&q), None);
    }

    #[test]
    fn removed_while_playing_stops_and_the_next_starts() {
        let mut q = IntroQueue::default();
        q.apply(placed(0, "eruptor"));
        q.apply(placed(1, "treerex"));
        let changes = q.apply(cleared(0));
        assert!(matches!(
            &changes[..],
            [Change::Stopped(a, StopWhy::Removed), Change::Started(b)]
                if a.movie == "eruptor" && b.movie == "treerex"
        ));
        // Clearing it alone: back to idle.
        let changes = q.apply(cleared(1));
        assert!(matches!(
            &changes[..],
            [Change::Stopped(_, StopWhy::Removed)]
        ));
        assert_eq!(playing(&q), None);
        assert!(q.apply(cleared(4)).is_empty());
    }

    #[test]
    fn a_skip_stops_the_playing_one_for_the_next_or_idle() {
        let mut q = IntroQueue::default();
        assert!(q.apply(IntroEvent::Skip).is_empty(), "nothing to skip");
        q.apply(placed(0, "eruptor"));
        q.apply(placed(1, "treerex"));
        let changes = q.apply(IntroEvent::Skip);
        assert!(matches!(
            &changes[..],
            [Change::Stopped(a, StopWhy::Skipped), Change::Started(b)]
                if a.movie == "eruptor" && b.movie == "treerex"
        ));
        q.apply(IntroEvent::Skip);
        assert_eq!(playing(&q), None);
    }

    #[test]
    fn a_late_end_for_a_stopped_intro_ends_nothing() {
        let mut q = IntroQueue::default();
        q.apply(placed(0, "eruptor"));
        let first = q.playing().unwrap().id;
        q.apply(IntroEvent::Skip);
        // The same movie again is a new play.
        q.apply(placed(0, "eruptor"));
        assert!(q.apply(IntroEvent::Ended { id: first }).is_empty());
        assert_eq!(playing(&q), Some("eruptor"));
        assert_ne!(q.playing().unwrap().id, first);
    }

    #[test]
    fn a_placement_over_an_occupied_slot_replaces_its_intro() {
        let mut q = IntroQueue::default();
        q.apply(placed(0, "eruptor"));
        q.apply(placed(1, "treerex"));
        // A swap in slot 2 of a queued one: the old drops, the new queues last.
        q.apply(placed(2, "spyro"));
        let changes = q.apply(placed(1, "bash"));
        assert!(matches!(
            &changes[..],
            [Change::Dropped(a), Change::Queued(b)] if a.movie == "treerex" && b.movie == "bash"
        ));
        assert_eq!(queued(&q), ["spyro", "bash"]);
        // Over the playing slot with a figure that has none: it just stops.
        let changes = q.apply(IntroEvent::Placed {
            slot: slot(0),
            figure_id: "sky-iron-shield".into(),
            movie: None,
        });
        assert!(matches!(
            &changes[..],
            [Change::Stopped(a, StopWhy::Removed), Change::Started(b)]
                if a.movie == "eruptor" && b.movie == "spyro"
        ));
    }

    #[test]
    fn a_figure_without_an_intro_queues_nothing() {
        let mut q = IntroQueue::default();
        let changes = q.apply(IntroEvent::Placed {
            slot: slot(0),
            figure_id: "sky-iron-shield".into(),
            movie: None,
        });
        assert!(changes.is_empty());
        assert_eq!(q, IntroQueue::default());
    }

    #[test]
    fn reset_drops_everything() {
        let mut q = IntroQueue::default();
        q.apply(placed(0, "eruptor"));
        q.apply(placed(1, "treerex"));
        let changes = q.apply(IntroEvent::Reset);
        assert!(matches!(
            &changes[..],
            [Change::Dropped(a), Change::Stopped(b, StopWhy::Reset)]
                if a.movie == "treerex" && b.movie == "eruptor"
        ));
        assert_eq!((playing(&q), queued(&q).len()), (None, 0));
    }

    fn library() -> IntroLibrary {
        let intro = |movie: &str, source| Character {
            archives: Vec::new(),
            vo_name: None,
            intro: Some(Intro {
                movie: movie.into(),
                source,
                voice: VoiceTrack::Only,
            }),
        };
        let mut characters = BTreeMap::new();
        characters.insert(9, intro("eruptor", IntroSource::Disc));
        characters.insert(10, intro("ignitor", IntroSource::Disc));
        characters.insert(16, intro("spyro", IntroSource::Disc));
        characters.insert(28, intro("spyro", IntroSource::Variant { base: 16 }));
        IntroLibrary {
            dir: PathBuf::from("/extract/BLUS30968-01.00-5e933248"),
            table: CharacterTable {
                identity: DumpIdentity {
                    title_id: "BLUS30968".into(),
                    app_ver: "01.00".into(),
                    manifest_hash: guac_ir::Sha256([0; 32]),
                },
                characters,
            },
        }
    }

    fn figure(id: &str) -> Placement {
        let figure_id = FigureId::new(id);
        let (toy_type, variant) = figure_id.subtype_parts().unwrap();
        Placement {
            figure_id,
            toy_type,
            variant,
            name: id.into(),
            element: None,
            portrait: None,
            owner: None,
        }
    }

    #[test]
    fn a_placement_finds_its_toy_types_intro() {
        let lib = library();
        let movie = |id| movie_for(Some(&lib), &figure(id));
        assert_eq!(movie("000009-0000").as_deref(), Some("eruptor"));
        // Legendary Ignitor: toy 10 with variant bits plays Ignitor's.
        assert_eq!(movie("00000a-0803").as_deref(), Some("ignitor"));
        // Spyro's Adventure Spyro, and Dark Spyro on a toy type of its own.
        assert_eq!(movie("000010-0000").as_deref(), Some("spyro"));
        assert_eq!(movie("00001c-0000").as_deref(), Some("spyro"));
        // Sky-Iron Shield, a magic item: none.
        assert_eq!(movie("0000cd-0000"), None);
        let (video, audio) = files(&lib, "eruptor");
        assert_eq!(
            video,
            Path::new("/extract/BLUS30968-01.00-5e933248/video/intro/eruptor.mp4")
        );
        assert_eq!(
            audio,
            Path::new("/extract/BLUS30968-01.00-5e933248/audio/intro/eruptor.mp3")
        );
    }

    #[test]
    fn placements_during_extraction_make_no_intro_and_later_ones_do() {
        let mut q = IntroQueue::default();
        // Extracting: no library yet.
        let during = movie_for(None, &figure("000009-0000"));
        assert_eq!(during, None);
        q.apply(IntroEvent::Placed {
            slot: slot(0),
            figure_id: "000009-0000".into(),
            movie: during,
        });
        assert_eq!(q, IntroQueue::default());
        // Finished: the figure already on the portal stays card only, the
        // next one placed plays.
        let lib = library();
        q.apply(IntroEvent::Placed {
            slot: slot(1),
            figure_id: "000010-0000".into(),
            movie: movie_for(Some(&lib), &figure("000010-0000")),
        });
        assert_eq!(playing(&q), Some("spyro"));
        assert_eq!(q.playing().unwrap().slot, slot(1));
    }
}

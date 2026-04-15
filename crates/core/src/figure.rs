//! Placeholder — populated in 2.2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FigureId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameSerial(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Air,
    Dark,
    Earth,
    Fire,
    Life,
    Light,
    Magic,
    Tech,
    Undead,
    Water,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Figure,
    Sidekick,
    Giant,
    Item,
    Trap,
    AdventurePack,
    CreationCrystal,
    Vehicle,
    Kaos,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figure {
    pub id: FigureId,
    // Fields filled out in 2.2.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicFigure {
    pub id: FigureId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: GameSerial,
    pub display_name: String,
}

//! The TV's shared state, below its screens (openspec AQ.7): the screen up
//! and since when ([`Current`]), its state and what redrew ([`TvState`],
//! [`Redrawn`]), the server it reads ([`ServerLink`], [`Setup`],
//! [`Framing`]), its font, QR and stars ([`TvAssets`]) and the [`Pen`] lines
//! are drawn with, its camera and screen root, and the markers and read-back
//! (`node_rect`, [`UiTree`]) the subsystems that report what the TV draws
//! find it by. The screens ([`crate::tv`]), the pads, the level, the
//! extraction and the rest read it here, so none of them names the screens.

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use guac_server::config::WindowMode;
use guac_server::state::{GameList, LauncherStatus};

use crate::screen::{SetupView, TvScreen};
use crate::starfield;
use crate::theme::{Role, Step, TvTheme};

/// What the TV reads from the server; built once in `main`.
#[derive(Resource, Clone)]
pub struct ServerLink {
    pub status: Arc<Mutex<LauncherStatus>>,
    pub clients: Arc<AtomicUsize>,
    pub phone_url: String,
    pub join_qr_png: Arc<Vec<u8>>,
    /// The phone's game list; a pick replaces it.
    pub games: GameList,
}

#[derive(Resource, Clone)]
pub struct Setup(pub SetupView);

/// The window mode: TV mode keeps everything inside the overscan inset.
#[derive(Resource, Clone, Copy)]
pub struct Framing(pub WindowMode);

/// The screen this frame, and since when.
#[derive(Resource)]
pub(crate) struct Current {
    pub(crate) screen: TvScreen,
    pub(crate) since: Instant,
    pub(crate) clients: usize,
}

#[derive(Resource)]
pub(crate) struct TvAssets {
    pub(crate) font: Handle<Font>,
    pub(crate) qr: Option<Handle<Image>>,
    /// The stars' two sprites (`starfield::Sprites`).
    pub(crate) stars: starfield::Sprites,
}

/// The extraction band's lines and bar fill (percent) as drawn.
#[derive(Component)]
pub(crate) struct ExtractTitle;

#[derive(Component)]
pub(crate) struct ExtractDetail;

#[derive(Component)]
pub(crate) struct ExtractBar(pub(crate) u8);

/// The TV's own camera. Every title renders through a `Camera2d` of its own
/// ([`title`]), so whatever reads the TV's camera (the UI's scale, the layout
/// report, the mirror, the shots) finds it by this, never by `Camera2d`.
#[derive(Component, Debug)]
pub(crate) struct TvCamera;

/// The TV camera's order: last on its target, so its UI lands over the level
/// drawn under it ([`crate::level::VIEW_ORDER`], AG.50).
pub(crate) const TV_ORDER: isize = 0;

/// Marks a screen's root node, spawned when its [`TvState`] is entered and
/// despawned with everything under it when it's left.
#[derive(Component)]
pub(crate) struct ScreenRoot;

/// What the TV just redrew: the layout report and the shots wait on these
/// instead of on the resources behind them, which change without a redraw.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Redrawn {
    /// A screen was entered and spawned.
    Screen(TvState),
    /// A portal cell's content respawned (0-based slot).
    Slot(u8),
    /// The band under the game's name, or the hint it replaces.
    Band,
    /// A drawn line took new text: the players line, a setup line, the message.
    Text,
    /// A choice's focus look turned on or off (`look`).
    Focus,
    /// The level came up under the TV's UI (the dev entry, AG.19; the walk,
    /// AG.21).
    Level,
    /// The level's overlay (Start's, the controller message) came or went
    /// (AG.50).
    Overlay,
    /// The portal's alert, or the small join code over the level, came or
    /// went (AG.29).
    Alert,
}

/// Which screen is up, without its data: [`Current`] keeps that. Set from
/// `screen_for(..)`'s kind, so Portal to Portal (another game) is no transition.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum TvState {
    #[default]
    Starting,
    Preparing,
    Setup,
    NoGiants,
    Found,
    Join,
    Portal,
    AlreadyRunning,
    ServerError,
    Farewell,
}

impl TvState {
    pub(crate) const ALL: [Self; 10] = [
        Self::Starting,
        Self::Preparing,
        Self::Setup,
        Self::NoGiants,
        Self::Found,
        Self::Join,
        Self::Portal,
        Self::AlreadyRunning,
        Self::ServerError,
        Self::Farewell,
    ];

    pub(crate) fn of(screen: &TvScreen) -> Self {
        match screen {
            TvScreen::Starting => Self::Starting,
            TvScreen::Preparing => Self::Preparing,
            TvScreen::Setup => Self::Setup,
            TvScreen::NoGiants => Self::NoGiants,
            TvScreen::Found => Self::Found,
            TvScreen::Join => Self::Join,
            TvScreen::Portal { .. } => Self::Portal,
            TvScreen::AlreadyRunning => Self::AlreadyRunning,
            TvScreen::ServerError { .. } => Self::ServerError,
            TvScreen::Farewell => Self::Farewell,
        }
    }
}

/// What every line is drawn with: the font and the theme, so a size comes
/// only from a [`Step`] and a colour from a [`Role`].
pub(crate) struct Pen {
    pub(crate) font: Handle<Font>,
    pub(crate) theme: TvTheme,
}

impl Pen {
    pub(crate) fn new(assets: &TvAssets, theme: &TvTheme) -> Self {
        Self {
            font: assets.font.clone(),
            theme: theme.clone(),
        }
    }

    pub(crate) fn font(&self, step: Step) -> TextFont {
        TextFont {
            font: self.font.clone().into(),
            font_size: FontSize::Px(self.theme.size(step)),
            ..default()
        }
    }

    /// Centred.
    pub(crate) fn text(&self, s: &str, step: Step, role: Role) -> impl Bundle + use<> {
        self.text_at(s, step, role, Justify::Center)
    }

    pub(crate) fn text_at(
        &self,
        s: &str,
        step: Step,
        role: Role,
        justify: Justify,
    ) -> impl Bundle + use<> {
        self.inked(s, step, self.theme.role(role), justify)
    }

    /// In a colour no role names: a pip's ink, picked against its fill.
    pub(crate) fn inked(
        &self,
        s: &str,
        step: Step,
        color: Color,
        justify: Justify,
    ) -> impl Bundle + use<> {
        (
            Text::new(s),
            self.font(step),
            TextColor(color),
            TextLayout::justify(justify),
        )
    }
}

/// A card on the portal screen, by slot (0-based).
#[derive(Component, Debug)]
pub(crate) struct CardSlot(pub(crate) u8);

/// A card's owner pip, by slot.
#[derive(Component, Debug)]
pub(crate) struct Pip(pub(crate) u8);

pub(crate) fn node_rect(node: &ComputedNode, at: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(at.translation, node.size())
}

/// Depth-first text and images under `root`, the pip's subtree apart.
pub(crate) fn walk(
    root: Entity,
    children: &Query<&Children>,
    texts: &Query<&Text>,
    images: &Query<(), With<ImageNode>>,
    pips: &Query<(&Pip, &BackgroundColor)>,
    out: &mut (Vec<String>, bool, Option<(String, Color)>),
) {
    let Ok(kids) = children.get(root) else {
        return;
    };
    for kid in kids.iter() {
        if let Ok((_, fill)) = pips.get(kid) {
            let mut inner = (Vec::new(), false, None);
            walk(kid, children, texts, images, pips, &mut inner);
            out.2 = Some((inner.0.join(""), fill.0));
            continue;
        }
        if let Ok(text) = texts.get(kid) {
            out.0.push(text.0.clone());
        }
        if images.contains(kid) {
            out.1 = true;
        }
        walk(kid, children, texts, images, pips, out);
    }
}

/// The laid-out UI tree as [`walk`] reads it: each node's children, its
/// text and image, and a pip's fill.
#[derive(SystemParam)]
pub(crate) struct UiTree<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    texts: Query<'w, 's, &'static Text>,
    images: Query<'w, 's, (), With<ImageNode>>,
    pips: Query<'w, 's, (&'static Pip, &'static BackgroundColor)>,
}

impl UiTree<'_, '_> {
    /// [`walk`] from `root`: its lines, whether it draws an image, and its
    /// pip's initial and fill.
    pub(crate) fn walk(&self, root: Entity) -> (Vec<String>, bool, Option<(String, Color)>) {
        let mut found = (Vec::new(), false, None);
        walk(
            root,
            &self.children,
            &self.texts,
            &self.images,
            &self.pips,
            &mut found,
        );
        found
    }
}

//! [`Controllers`] over Apple's GameController framework (macOS only),
//! through the objc2 bindings. Main thread only: the framework posts its
//! connections and runs its handlers on the main queue, and the Bevy system
//! that reads this is `NonSend`.
//!
//! Readings come from each controller's input-state queue (`input`,
//! `inputStateQueueDepth`, `nextInputState`: macOS 14 on), in order, so a
//! press shorter than a frame survives. Where the OS has no queue, or the
//! live input lacks the face buttons by Apple's names, the controller's
//! profile is read once a frame. Every element is looked up by Apple's own
//! input name (`GCInputButtonA` and the rest), the keys AG.5's probe saw.

use std::collections::HashMap;
use std::ptr::NonNull;

use bevy::math::Vec2;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{msg_send, sel};
use objc2_foundation::NSString;
use objc2_game_controller::{
    GCAxis2DInput, GCButtonElement, GCController, GCControllerInputState, GCControllerLiveInput,
    GCDevice, GCDevicePhysicalInput, GCDevicePhysicalInputState, GCDirectionPadElement,
    GCInputButtonA, GCInputButtonB, GCInputButtonHome, GCInputButtonMenu, GCInputButtonOptions,
    GCInputButtonShare, GCInputButtonX, GCInputButtonY, GCInputDirectionPad, GCInputLeftShoulder,
    GCInputLeftThumbstick, GCInputLeftThumbstickButton, GCInputLeftTrigger, GCInputRightShoulder,
    GCInputRightThumbstick, GCInputRightThumbstickButton, GCInputRightTrigger,
    GCPhysicalInputProfile, GCSystemGestureState,
};
use tracing::warn;

use super::{Controllers, Element, Glyph, Opened, PadState};

/// What the input-state handler is handed.
type Input = ProtocolObject<dyn GCDevicePhysicalInput>;

/// Apple's input name for a button element; none for the D-pad's
/// directions, which are parts of the D-pad element.
fn button_name(element: Element) -> Option<&'static NSString> {
    // SAFETY: the framework's exported constant strings, read once linked.
    unsafe {
        match element {
            Element::A => GCInputButtonA,
            Element::B => GCInputButtonB,
            Element::X => GCInputButtonX,
            Element::Y => GCInputButtonY,
            Element::LeftShoulder => GCInputLeftShoulder,
            Element::RightShoulder => GCInputRightShoulder,
            Element::LeftTrigger => GCInputLeftTrigger,
            Element::RightTrigger => GCInputRightTrigger,
            Element::Menu => GCInputButtonMenu,
            Element::Options => GCInputButtonOptions,
            Element::Home => GCInputButtonHome,
            Element::Share => GCInputButtonShare,
            Element::LeftThumbstickButton => GCInputLeftThumbstickButton,
            Element::RightThumbstickButton => GCInputRightThumbstickButton,
            Element::DpadUp | Element::DpadDown | Element::DpadLeft | Element::DpadRight => None,
        }
    }
}

/// The direction-pad elements a reading takes, D-pad, left stick, right
/// stick: one lookup each per reading.
fn dpad_names() -> [Option<&'static NSString>; 3] {
    // SAFETY: as in `button_name`.
    unsafe {
        [
            GCInputDirectionPad,
            GCInputLeftThumbstick,
            GCInputRightThumbstick,
        ]
    }
}

/// The live input's queue where the OS has it, else the profile polled
/// once a frame.
enum Source {
    /// Its live input's queue.
    Queue(Retained<GCControllerLiveInput>),
    /// Its profile, read once a frame; the last reading handed out.
    Polled(Option<PadState>),
}

struct Pad {
    controller: Retained<GCController>,
    profile: Retained<GCPhysicalInputProfile>,
    source: Source,
}

/// The controllers GameController reports and where each one's readings
/// come from, keyed by the controller object's address.
pub struct Framework {
    /// Every controller seen and not yet closed, by its object's address
    /// (held, so the address stays its own).
    seen: HashMap<u64, Retained<GCController>>,
    pads: HashMap<u64, Pad>,
}

impl Framework {
    /// Pads read whether or not guacamole's window is in front, as gilrs
    /// reads them on Windows; Home's claim holds while it's in front.
    pub fn new() -> Self {
        // SAFETY: a class property, set on the main thread before any read.
        unsafe { GCController::setShouldMonitorBackgroundEvents(true) };
        Self {
            seen: HashMap::new(),
            pads: HashMap::new(),
        }
    }
}

impl Default for Framework {
    fn default() -> Self {
        Self::new()
    }
}

fn key(controller: &GCController) -> u64 {
    std::ptr::from_ref(controller) as usize as u64
}

impl Controllers for Framework {
    fn connected(&mut self) -> Vec<u64> {
        // SAFETY: main thread (the system reading this is NonSend).
        let list = unsafe { GCController::controllers() };
        list.iter()
            .map(|controller| {
                let key = key(&controller);
                self.seen.entry(key).or_insert(controller);
                key
            })
            .collect()
    }

    fn open(&mut self, key: u64) -> Option<Opened> {
        let controller = self.seen.get(&key)?.clone();
        // SAFETY: plain property reads on a connected controller.
        unsafe {
            controller.extendedGamepad()?;
            let profile = controller.physicalInputProfile();
            let name = controller
                .vendorName()
                .map_or_else(String::new, |n| n.to_string());
            let category = controller.productCategory().to_string();
            let buttons = profile.buttons();
            let glyphs = Element::FACES.map(|face| {
                button_name(face)
                    .and_then(|n| buttons.objectForKey(n))
                    .map_or_else(Glyph::default, |b| Glyph {
                        symbol: b.sfSymbolsName().map(|s| s.to_string()),
                        localized: b.localizedName().map(|s| s.to_string()),
                    })
            });
            let state = read_profile(&profile);
            self.pads.insert(
                key,
                Pad {
                    controller,
                    profile,
                    source: Source::Polled(Some(state)),
                },
            );
            Some(Opened {
                name,
                category,
                glyphs,
                state,
            })
        }
    }

    fn queue(&mut self, key: u64, depth: usize) -> bool {
        let Some(pad) = self.pads.get_mut(&key) else {
            return false;
        };
        // SAFETY: `input` is checked for before it's called; the handler is
        // copied by the setter and runs on the main queue.
        unsafe {
            if !pad.controller.respondsToSelector(sel!(input)) {
                return false;
            }
            let input = pad.controller.input();
            let faces = input.buttons();
            if Element::FACES.iter().any(|f| {
                button_name(*f)
                    .and_then(|n| faces.objectForKeyedSubscript(n))
                    .is_none()
            }) {
                warn!(
                    key,
                    "the live input has no face buttons by Apple's names: reading the profile instead"
                );
                return false;
            }
            input.setInputStateQueueDepth(depth as isize);
            // Draining happens in the frame's system; the handler is only
            // there so the framework has someone to buffer for.
            let handler: RcBlock<dyn Fn(NonNull<Input>)> = RcBlock::new(|_: NonNull<Input>| {});
            input.setInputStateAvailableHandler(Some(&handler));
            // Readings from here on: whatever queued before is stale.
            while input.nextInputState().is_some() {}
            pad.source = Source::Queue(input);
        }
        true
    }

    fn claim_home(&mut self, key: u64) -> bool {
        let Some(pad) = self.pads.get(&key) else {
            return false;
        };
        // SAFETY: a property on one of the controller's elements.
        unsafe {
            let Some(home) =
                button_name(Element::Home).and_then(|n| pad.profile.buttons().objectForKey(n))
            else {
                return false;
            };
            home.setPreferredSystemGestureState(GCSystemGestureState::Disabled);
            home.preferredSystemGestureState() == GCSystemGestureState::Disabled
        }
    }

    fn next_state(&mut self, key: u64) -> Option<PadState> {
        let pad = self.pads.get_mut(&key)?;
        // SAFETY: main thread, as the handler's queue.
        unsafe {
            match &mut pad.source {
                Source::Queue(input) => input.nextInputState().map(|s| read_live(&s)),
                Source::Polled(last) => {
                    let now = read_profile(&pad.profile);
                    if *last == Some(now) {
                        None
                    } else {
                        *last = Some(now);
                        Some(now)
                    }
                }
            }
        }
    }

    fn close(&mut self, key: u64) {
        self.pads.remove(&key);
        self.seen.remove(&key);
    }
}

/// A queued reading.
///
/// # Safety
/// Main thread; `state` is one the framework queued.
unsafe fn read_live(state: &GCControllerInputState) -> PadState {
    let mut out = PadState::default();
    // SAFETY: element lookups and reads on a state the framework handed us;
    // `pressedInput` and a D-pad's directions conform to
    // `GCPressedStateInput`, whose `isPressed` returns BOOL.
    unsafe {
        let buttons = state.buttons();
        for element in Element::ALL {
            if let Some(button) =
                button_name(element).and_then(|n| buttons.objectForKeyedSubscript(n))
            {
                let pressed = button.pressedInput();
                let down: bool = msg_send![&*pressed, isPressed];
                out.set(element, down);
            }
        }
        let dpads = state.dpads();
        let [dpad, left, right] =
            dpad_names().map(|n| n.and_then(|n| dpads.objectForKeyedSubscript(n)));
        if let Some(dpad) = dpad {
            for (element, input) in [
                (Element::DpadUp, dpad.up()),
                (Element::DpadDown, dpad.down()),
                (Element::DpadLeft, dpad.left()),
                (Element::DpadRight, dpad.right()),
            ] {
                let down: bool = msg_send![&*input, isPressed];
                out.set(element, down);
            }
        }
        let stick = |pad: Option<Retained<ProtocolObject<dyn GCDirectionPadElement>>>| {
            pad.map_or(Vec2::ZERO, |p| {
                let v = p.xyAxes().value();
                Vec2::new(v.x, v.y)
            })
        };
        out.left = stick(left);
        out.right = stick(right);
    }
    out
}

/// The controller's profile as it stands.
///
/// # Safety
/// Main thread.
unsafe fn read_profile(profile: &GCPhysicalInputProfile) -> PadState {
    let mut out = PadState::default();
    // SAFETY: property reads on the controller's own profile elements.
    unsafe {
        let buttons = profile.buttons();
        for element in Element::ALL {
            if let Some(button) = button_name(element).and_then(|n| buttons.objectForKey(n)) {
                out.set(element, button.isPressed());
            }
        }
        let dpads = profile.dpads();
        let [dpad, left, right] = dpad_names().map(|n| n.and_then(|n| dpads.objectForKey(n)));
        if let Some(dpad) = dpad {
            out.set(Element::DpadUp, dpad.up().isPressed());
            out.set(Element::DpadDown, dpad.down().isPressed());
            out.set(Element::DpadLeft, dpad.left().isPressed());
            out.set(Element::DpadRight, dpad.right().isPressed());
        }
        let stick = |pad: Option<Retained<objc2_game_controller::GCControllerDirectionPad>>| {
            pad.map_or(Vec2::ZERO, |p| {
                Vec2::new(p.xAxis().value(), p.yAxis().value())
            })
        };
        out.left = stick(left);
        out.right = stick(right);
    }
    out
}

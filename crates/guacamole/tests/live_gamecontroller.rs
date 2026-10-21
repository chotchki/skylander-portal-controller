//! AG.53's live check, ignored unless asked for: a pad connected over
//! Bluetooth to the Mac, read through guacamole's GameController backend, on
//! the main thread (the framework delivers its connections on the main
//! queue, which a libtest worker thread never runs, hence `harness = false`).
//!
//! `cargo test -p guacamole --test live_gamecontroller -- --ignored`, the pad
//! on and paired, hands on it: it prints what the framework tells about the
//! pad, then asks for presses one at a time and checks each lands on the
//! Bevy button it should. AG.51 runs it with chotchki's 8BitDo in Switch
//! mode; the letters it expects are AG.5's (bottom B, right A, left Y,
//! top X).

#[cfg(target_os = "macos")]
mod live {
    use std::time::{Duration, Instant};

    use bevy::ecs::world::World;
    use bevy::input::gamepad::{GamepadAxis, GamepadButton};
    use guacamole::game_controller::apple::Framework;
    use guacamole::game_controller::{FacesPrinted, GcPads, PadEvent};
    use objc2_foundation::{NSDate, NSRunLoop};

    /// The main run loop for `secs`, so the framework's main-queue work runs.
    fn pump(secs: f64) {
        let until = NSDate::dateWithTimeIntervalSinceNow(secs);
        NSRunLoop::currentRunLoop().runUntilDate(&until);
    }

    struct Live {
        pads: GcPads,
        world: World,
    }

    impl Live {
        /// Frames of about 16 ms until `want` finds what it's after in one,
        /// or `secs` pass.
        fn until<T>(
            &mut self,
            secs: u64,
            mut want: impl FnMut(&[PadEvent]) -> Option<T>,
        ) -> Option<T> {
            let end = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < end {
                pump(0.016);
                let world = &mut self.world;
                let events = self.pads.poll(|| world.spawn_empty().id());
                for e in &events {
                    println!("    {e:?}");
                }
                if let Some(found) = want(&events) {
                    return Some(found);
                }
            }
            None
        }
    }

    pub fn run() {
        tracing_subscriber::fmt().with_env_filter("info").init();
        let mut live = Live {
            pads: GcPads::new(Box::new(Framework::new())),
            world: World::new(),
        };
        println!("waiting up to 20 s for a pad: switch it on (Bluetooth)");
        let (pad, faces) = live
            .until(20, |events| {
                events.iter().find_map(|e| match e {
                    PadEvent::Connected { entity, faces, .. } => Some((*entity, *faces)),
                    _ => None,
                })
            })
            .expect("no pad connected through GameController in 20 s");
        println!("connected: the letters per position (bottom, right, left, top) {faces:?}");
        println!("(the log line above has its category, layout, Home claim and queue)");
        let mut failures = Vec::new();
        if faces != FacesPrinted([Some('B'), Some('A'), Some('Y'), Some('X')]) {
            println!("NOTE: not AG.5's Switch Pro letters; fine for another pad");
        }
        let steps: [(&str, GamepadButton); 8] = [
            ("press the BOTTOM face button", GamepadButton::South),
            ("press the RIGHT face button", GamepadButton::East),
            ("press the LEFT face button", GamepadButton::West),
            ("press the TOP face button", GamepadButton::North),
            ("press PLUS (Start)", GamepadButton::Start),
            ("press MINUS (Select)", GamepadButton::Select),
            (
                "press HOME: the Games app must NOT open",
                GamepadButton::Mode,
            ),
            (
                "TAP the bottom face button as fast as you can",
                GamepadButton::South,
            ),
        ];
        for (ask, want) in steps {
            println!("\n>>> {ask} (15 s)");
            let pressed = live.until(15, |events| {
                events
                    .iter()
                    .any(|e| matches!(e, PadEvent::Button(p, b, v) if *p == pad && *b == want && *v == 1.0))
                    .then_some(())
            });
            match pressed {
                Some(()) => println!("ok: {want:?}"),
                None => failures.push(format!("{ask}: no {want:?} press in 15 s")),
            }
            // Let go of everything before the next ask.
            live.until(1, |_| None::<()>);
        }
        println!("\n>>> push the LEFT stick fully FORWARD and hold it (15 s)");
        let mut y = 0.0f32;
        let forward = live.until(15, |events| {
            for e in events {
                if let PadEvent::Axis(p, GamepadAxis::LeftStickY, v) = e
                    && *p == pad
                {
                    y = *v;
                }
            }
            (y >= 0.999).then_some(y)
        });
        match forward {
            Some(v) => println!("ok: forward reads {v}"),
            None => failures.push(format!("stick forward: reached {y} in 15 s, not 1")),
        }
        println!("\n>>> let go of the stick (5 s)");
        let mut at = (1.0f32, 1.0f32);
        let rest = live.until(5, |events| {
            for e in events {
                match e {
                    PadEvent::Axis(p, GamepadAxis::LeftStickX, v) if *p == pad => at.0 = *v,
                    PadEvent::Axis(p, GamepadAxis::LeftStickY, v) if *p == pad => at.1 = *v,
                    _ => {}
                }
            }
            (at.0.abs() < 0.05 && at.1.abs() < 0.05).then_some(at)
        });
        match rest {
            Some(v) => println!("ok: rest reads {v:?}"),
            None => failures.push(format!("stick rest: {at:?} after 5 s")),
        }
        assert!(failures.is_empty(), "live check: {failures:#?}");
        println!("\nlive check passed");
    }
}

fn main() {
    let asked = std::env::args().any(|a| a == "--ignored" || a == "--include-ignored");
    if !asked {
        println!(
            "\nrunning 1 test\ntest live_gamecontroller ... ignored, a Bluetooth pad in hand (-- --ignored)\n"
        );
        println!("test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n");
        return;
    }
    #[cfg(target_os = "macos")]
    live::run();
    #[cfg(not(target_os = "macos"))]
    println!("GameController is macOS's: nothing to check here");
}

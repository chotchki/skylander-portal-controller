use super::*;

/// `f` on a new thread, its answer back.
fn on_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().unwrap()
}

#[test]
fn a_thread_reads_back_the_class_it_asked_for() {
    for q in [Qos::Utility, Qos::Background, Qos::Default] {
        let (set, read) = on_thread(move || (set_current(q), current()));
        assert_eq!(set, Ok(()), "{q:?}");
        if cfg!(target_os = "macos") {
            assert_eq!(read, q);
        } else {
            assert_eq!(read, Qos::Unspecified);
        }
    }
}

#[test]
fn a_thread_follows_a_spawner_below_the_default_only() {
    for (spawner, want) in [
        (Qos::Utility, Qos::Utility),
        (Qos::Background, Qos::Background),
        // Never raised: a thread a terminal's main thread starts stays at
        // the default, as it was.
        (Qos::UserInteractive, Qos::Default),
        (Qos::Default, Qos::Default),
        (Qos::Unspecified, Qos::Default),
    ] {
        let got = on_thread(move || {
            follow(spawner);
            current()
        });
        if cfg!(target_os = "macos") {
            assert_eq!(got, want, "{spawner:?}");
        } else {
            assert_eq!(got, Qos::Unspecified);
        }
    }
}

#[test]
fn a_command_is_clamped_below_the_default_only() {
    let argv = |c: &Command| -> Vec<String> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    };
    let mac = cfg!(target_os = "macos");
    let c = clamped("/bin/ffmpeg", Qos::Utility, true);
    if mac {
        assert_eq!(argv(&c), [TASKPOLICY, "-c", "utility", "/bin/ffmpeg"]);
        let c = clamped("/bin/ffmpeg", Qos::Background, true);
        assert_eq!(argv(&c), [TASKPOLICY, "-c", "background", "/bin/ffmpeg"]);
    } else {
        assert_eq!(argv(&c), ["/bin/ffmpeg"]);
    }
    for q in [
        Qos::UserInteractive,
        Qos::UserInitiated,
        Qos::Default,
        Qos::Unspecified,
    ] {
        assert_eq!(
            argv(&clamped("/bin/ffmpeg", q, true)),
            ["/bin/ffmpeg"],
            "{q:?}"
        );
    }
    // No taskpolicy: the program as it is.
    assert_eq!(
        argv(&clamped("/bin/ffmpeg", Qos::Utility, false)),
        ["/bin/ffmpeg"]
    );
}

/// The base scheduling priority of the calling thread: what a class or a
/// process's clamp leaves it at (utility 20, the default 31, background 4).
#[cfg(target_os = "macos")]
pub(crate) fn thread_priority() -> i32 {
    // SAFETY: fills a struct of the size given for the calling thread.
    #[allow(deprecated)]
    unsafe {
        let mut info: libc::thread_extended_info = std::mem::zeroed();
        let mut count = libc::THREAD_EXTENDED_INFO_COUNT;
        let port = libc::mach_thread_self();
        let r = libc::thread_info(
            port,
            libc::THREAD_EXTENDED_INFO as u32,
            (&mut info as *mut libc::thread_extended_info).cast(),
            &mut count,
        );
        assert_eq!(r, 0, "thread_info");
        info.pth_priority
    }
}

/// The child the spawn tests start: this test binary running this test
/// alone with `GUAC_QOS_PROBE` set, printing the priority its thread runs at
/// (libtest runs it on a thread of its own, the kind ffmpeg's encoders are).
/// Without the variable it does nothing.
#[test]
fn qos_probe() {
    if std::env::var_os("GUAC_QOS_PROBE").is_none() {
        return;
    }
    #[cfg(target_os = "macos")]
    println!("GUAC_QOS_PROBE priority={}", thread_priority());
}

/// `c` with the probe's arguments, run; the priority it printed.
#[cfg(target_os = "macos")]
pub(crate) fn probe(mut c: Command) -> i32 {
    let out = c
        .args(["--exact", "qos::tests::qos_probe", "--nocapture"])
        .args(["--test-threads", "1"])
        .env("GUAC_QOS_PROBE", "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    // libtest prints it after its own `test ... ` on the same line.
    let after = text
        .split("GUAC_QOS_PROBE priority=")
        .nth(1)
        .unwrap_or_else(|| panic!("no probe line: {text}"));
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap()
}

/// macOS's base priority for a utility thread (XNU's `BASEPRI_UTILITY`): the
/// most a process clamped to utility runs any thread at.
#[cfg(target_os = "macos")]
pub(crate) const UTILITY_PRIORITY: i32 = 20;

/// A child started by a utility thread runs its threads at utility's
/// priority or under, its worker threads included; one started by a default
/// thread runs as it always did. (A thread's own reading of its priority
/// doesn't follow a class it set itself, only a clamp, so the spawner's
/// side is read off its class.)
#[cfg(target_os = "macos")]
#[test]
fn a_child_runs_every_thread_at_its_spawners_class() {
    let exe = std::env::current_exe().unwrap();
    let (plain, clamped) = on_thread(move || {
        let plain = probe(command(&exe));
        set_current(Qos::Utility).unwrap();
        assert_eq!(current(), Qos::Utility);
        (plain, probe(command(&exe)))
    });
    println!("a default thread's child at {plain}, a utility thread's at {clamped}");
    assert!(
        clamped <= UTILITY_PRIORITY,
        "the clamped child at {clamped}"
    );
    if plain > UTILITY_PRIORITY {
        assert!(clamped < plain);
    } else {
        // The whole test run is at or under utility already (a
        // `taskpolicy -b` run): nothing left to tell apart.
        println!("this run is at {plain} already; the plain child isn't told apart");
    }
}

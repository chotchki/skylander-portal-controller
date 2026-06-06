//! Software-OpenGL fallback for GPU-less machines (Windows). PLAN 19.
//!
//! The launcher + first-launch wizard are eframe/egui apps on the **glow**
//! (OpenGL 3.x) backend — `eframe = { features = ["glow", ...] }`, plus the
//! 3D badge (`badge.rs`) and vortex shader bind `frame.gl()` directly. On a
//! machine with no GPU (a winget validation VM, a headless/RDP session) the
//! only OpenGL available is Microsoft's GDI-generic software ICD, which tops
//! out at **OpenGL 1.1** — glutin can't build a 3.x context, so
//! `eframe::run_native` fails and the (`windows_subsystem = "windows"`)
//! process exits silently with no window. That is exactly the winget
//! reviewer's report: "installs normally, but doesn't run when launched."
//!
//! ## Why a re-exec instead of an env var
//!
//! We ship a bundled Mesa **llvmpipe** OpenGL (a pure-software 4.x
//! implementation) under `<exe_dir>/mesa/`. The obvious hook —
//! `GLUTIN_WGL_OPENGL_DLL` — only swaps the DLL glutin uses for the *GL
//! rendering* function pointers; the **WGL context-management** functions
//! (`wglCreateContext` / `wglMakeCurrent`) are **statically linked** to the
//! system `opengl32.dll` by `glutin_wgl_sys` (`StaticGenerator` +
//! `#[link(name = "opengl32")]`), resolved at process-load time. Pointing only
//! the rendering half at Mesa yields a system-created context with Mesa GL
//! calls — Mesa reports "glGetString called without a rendering context" and
//! glow panics. For Mesa to drive the whole stack it must be **the process's
//! `opengl32.dll`**, which the loader resolves from the **application
//! directory** (the exe's own folder) before System32.
//!
//! So: we ship a *copy of the launcher exe inside `mesa/`*, and on a GPU-less
//! machine the app re-execs `mesa/<exe>`. That child's application directory is
//! `mesa/`, so its static `opengl32.dll` import — and every GL call — resolves
//! to bundled Mesa. The child finds its shipped assets (`data/`, `phone-dist/`,
//! `rpcs3/`) via `SKYLANDER_APP_DIR`, which points back at the real install
//! dir (see [`crate::paths::app_asset_dir`]).
//!
//! Real HTPC users (who, by definition, run a GPU-hungry PS3 emulator) keep
//! hardware OpenGL: the up-front WGL probe finds ≥ 3.0 and we never re-exec.
//! The probe is fully guarded — any anomaly defaults to *hardware* so a working
//! GPU machine can never be pushed onto the slow software path by a probe bug.

use std::path::{Path, PathBuf};

/// Operator override: `software` forces the Mesa re-exec, `hardware` forces
/// system GL. Anything else falls through to auto-detection.
const FORCE_ENV: &str = "SKYLANDER_GL";
/// Set on the re-exec'd child so it doesn't probe or re-exec again (and to
/// flag the demo/no-GPU UX). Its presence means "we are running from `mesa/`
/// on the bundled software renderer".
const CHILD_ENV: &str = "SKYLANDER_GL_SOFTWARE";
/// Passed to the child so it resolves shipped assets against the real install
/// dir rather than its own `mesa/` folder. Read by [`crate::paths::app_asset_dir`].
pub const APP_DIR_ENV: &str = "SKYLANDER_APP_DIR";

/// What [`bootstrap`] decided, for the caller to log (after logging is
/// initialised — `bootstrap` runs before the subscriber) and to drive the
/// wizard's no-GPU messaging.
#[derive(Debug, Clone)]
pub struct GlBackendChoice {
    /// True when we're rendering through the bundled Mesa software renderer
    /// (no hardware OpenGL ≥ 3.0). Drives the wizard's "demo mode" notice.
    pub software: bool,
    /// Human-readable one-liner describing the decision + evidence.
    pub detail: String,
}

/// The decision, separated from process-level side effects (probe, re-exec) so
/// it can be unit-tested exhaustively.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    /// Continue in this process on the system OpenGL.
    Hardware(String),
    /// Re-exec `mesa/<exe>` so the bundled Mesa becomes the process opengl32.
    ReExecSoftware(String),
    /// We ARE the re-exec'd child — continue here; opengl32 is already Mesa.
    SoftwareChild(String),
}

/// Result of the hardware-OpenGL probe.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeResult {
    major: u32,
    version: String,
    renderer: String,
}

/// Pure decision logic. No I/O — `probe` is injected so tests can supply a
/// fixed version, and `mesa_runtime` says whether the bundled `mesa/<exe>` +
/// Mesa DLLs are actually present to re-exec into.
fn decide(
    is_child: bool,
    force: Option<&str>,
    mesa_runtime: bool,
    probe: impl FnOnce() -> Option<ProbeResult>,
) -> Plan {
    // The child short-circuits FIRST — it must never probe or re-exec again
    // (infinite-loop guard), regardless of force/probe.
    if is_child {
        return Plan::SoftwareChild("re-exec'd software-GL instance (bundled Mesa)".to_string());
    }

    match force {
        Some("hardware") => {
            return Plan::Hardware(format!("{FORCE_ENV}=hardware — using system opengl32.dll"));
        }
        Some("software") => {
            return if mesa_runtime {
                Plan::ReExecSoftware(format!("{FORCE_ENV}=software — re-exec into bundled Mesa"))
            } else {
                Plan::Hardware(format!(
                    "{FORCE_ENV}=software but no bundled mesa runtime found — using system opengl32.dll"
                ))
            };
        }
        Some(other) => {
            return decide_auto(mesa_runtime, probe).with_note(format!(
                "ignored unknown {FORCE_ENV}={other:?} (expected 'software'|'hardware')"
            ));
        }
        None => {}
    }

    decide_auto(mesa_runtime, probe)
}

fn decide_auto(mesa_runtime: bool, probe: impl FnOnce() -> Option<ProbeResult>) -> Plan {
    // No bundled Mesa runtime (a dev `cargo run`, or a build that didn't stage
    // it) — nothing to fall back to, so always stay on system GL.
    if !mesa_runtime {
        return Plan::Hardware("no bundled mesa runtime — using system opengl32.dll".to_string());
    }

    match probe() {
        Some(p) if p.major >= 3 => Plan::Hardware(format!(
            "hardware OpenGL {} ({}) — using system opengl32.dll",
            p.version, p.renderer
        )),
        Some(p) => Plan::ReExecSoftware(format!(
            "hardware OpenGL is only {} ({}) — re-exec into bundled Mesa software renderer",
            p.version, p.renderer
        )),
        // Couldn't even create a probe context. This effectively never happens
        // on a real Windows box (the GDI-generic ICD always yields a 1.1
        // context), so treat it as a probe anomaly and stay on system GL — the
        // conservative choice that can't regress a working GPU machine.
        None => Plan::Hardware("OpenGL probe inconclusive — using system opengl32.dll".to_string()),
    }
}

impl Plan {
    fn with_note(self, note: String) -> Self {
        match self {
            Plan::Hardware(d) => Plan::Hardware(format!("{note}; {d}")),
            Plan::ReExecSoftware(d) => Plan::ReExecSoftware(format!("{note}; {d}")),
            Plan::SoftwareChild(d) => Plan::SoftwareChild(format!("{note}; {d}")),
        }
    }
}

/// Resolve the bundled `mesa/<exe>` re-exec target, requiring Mesa's
/// `opengl32.dll` to be present beside it (else re-execing is pointless).
#[cfg(windows)]
fn mesa_runtime_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("mesa");
    let child = dir.join(exe.file_name()?);
    let dll = dir.join("opengl32.dll");
    (child.is_file() && dll.is_file()).then_some(child)
}

/// Decide the OpenGL backend and, on a GPU-less machine, re-exec into the
/// bundled Mesa software renderer (this call then **exits** the parent). Call
/// it **once, first thing in `main`** — before any thread is spawned and before
/// the wizard window or server bind.
///
/// On non-Windows this is a no-op: macOS/Linux ship their own working OpenGL.
#[cfg(windows)]
pub fn bootstrap() -> GlBackendChoice {
    let is_child = std::env::var_os(CHILD_ENV).is_some();
    let force = std::env::var(FORCE_ENV).ok();
    let mesa_exe = mesa_runtime_exe();

    let plan = decide(is_child, force.as_deref(), mesa_exe.is_some(), || {
        // The probe touches raw Win32/WGL; a panic here must never crash
        // startup, so isolate it and fold a panic into "inconclusive".
        std::panic::catch_unwind(probe_hardware_opengl)
            .ok()
            .flatten()
    });

    match plan {
        Plan::Hardware(detail) => GlBackendChoice {
            software: false,
            detail,
        },
        Plan::SoftwareChild(detail) => GlBackendChoice {
            software: true,
            detail,
        },
        Plan::ReExecSoftware(detail) => {
            let exe = mesa_exe.expect("ReExecSoftware implies a present mesa runtime");
            match reexec_into_mesa(&exe) {
                Ok(code) => std::process::exit(code),
                Err(e) => GlBackendChoice {
                    // Couldn't spawn the child — degrade to running here on
                    // system GL. It will likely fail to draw, but that's no
                    // worse than before this fix existed, and we at least get
                    // a logged reason.
                    software: false,
                    detail: format!("{detail}; re-exec failed: {e} — continuing on system GL"),
                },
            }
        }
    }
}

#[cfg(not(windows))]
pub fn bootstrap() -> GlBackendChoice {
    GlBackendChoice {
        software: false,
        detail: "non-Windows — using the platform OpenGL".to_string(),
    }
}

/// Launch `mesa/<exe>` (same args), wait for it, and return its exit code. The
/// child runs with `mesa/` as its application directory so the loader resolves
/// `opengl32.dll` to bundled Mesa; `GALLIUM_DRIVER=llvmpipe` pins the pure
/// software driver (no `dxil.dll` / d3d12 dependency); `SKYLANDER_APP_DIR`
/// points it back at the real install dir for shipped assets.
#[cfg(windows)]
fn reexec_into_mesa(mesa_exe: &Path) -> std::io::Result<i32> {
    let app_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));

    let status = std::process::Command::new(mesa_exe)
        .args(std::env::args_os().skip(1))
        .env(CHILD_ENV, "1")
        .env(APP_DIR_ENV, &app_dir)
        .env("GALLIUM_DRIVER", "llvmpipe")
        .status()?;
    Ok(status.code().unwrap_or(0))
}

/// Create a throwaway WGL context against the **system** `opengl32.dll` and
/// read `GL_VERSION` / `GL_RENDERER`. Returns `None` if a context couldn't be
/// established. Windows-only; the standard "dummy context" pattern (a hidden
/// window → pixel format → legacy WGL context).
#[cfg(windows)]
fn probe_hardware_opengl() -> Option<ProbeResult> {
    use std::ffi::CStr;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetDC, HDC, ReleaseDC};
    use windows::Win32::Graphics::OpenGL::{
        ChoosePixelFormat, GL_RENDERER, GL_VERSION, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW,
        PFD_MAIN_PLANE, PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat,
        glGetString, wglCreateContext, wglDeleteContext, wglMakeCurrent,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
        WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    unsafe {
        let hinstance = GetModuleHandleW(None).ok()?;
        let class_name = w!("SkylanderGlProbe");

        // Registering twice (e.g. a relaunch in-process) returns 0 with
        // ERROR_CLASS_ALREADY_EXISTS — harmless, CreateWindow still works.
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // A normal-but-never-shown window. ChoosePixelFormat needs a real
        // window DC (a memory/bitmap DC only ever yields the generic ICD).
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("gl probe"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1,
            1,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .ok()?;

        let result = (|| {
            let hdc = GetDC(Some(hwnd));
            if hdc.is_invalid() {
                return None;
            }

            let pfd = PIXELFORMATDESCRIPTOR {
                nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
                nVersion: 1,
                dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
                iPixelType: PFD_TYPE_RGBA,
                cColorBits: 32,
                cDepthBits: 24,
                iLayerType: PFD_MAIN_PLANE.0 as u8,
                ..Default::default()
            };
            let fmt = ChoosePixelFormat(hdc, &pfd);
            let probe = if fmt != 0 && SetPixelFormat(hdc, fmt, &pfd).is_ok() {
                match wglCreateContext(hdc) {
                    Ok(ctx) => {
                        let out = if wglMakeCurrent(hdc, ctx).is_ok() {
                            let version = gl_string(glGetString(GL_VERSION));
                            let renderer = gl_string(glGetString(GL_RENDERER));
                            let major = parse_gl_major(&version);
                            // Unbind (null HDC/HGLRC) before deleting the context.
                            let _ = wglMakeCurrent(HDC::default(), HGLRC::default());
                            major.map(|major| ProbeResult {
                                major,
                                version,
                                renderer,
                            })
                        } else {
                            None
                        };
                        let _ = wglDeleteContext(ctx);
                        out
                    }
                    Err(_) => None,
                }
            } else {
                None
            };
            ReleaseDC(Some(hwnd), hdc);
            probe
        })();

        let _ = DestroyWindow(hwnd);
        return result;

        // glGetString returns a NUL-terminated ASCII string owned by the GL
        // driver; copy it out before the context is torn down.
        unsafe fn gl_string(ptr: *const u8) -> String {
            if ptr.is_null() {
                return String::new();
            }
            unsafe { CStr::from_ptr(ptr as *const i8) }
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// Parse the leading major version out of a GL_VERSION string. Hardware
/// reports e.g. `"4.6.0 NVIDIA 552.44"`; the GDI-generic software ICD reports
/// `"1.1.0"`. Mesa llvmpipe reports `"4.6 (Core Profile) Mesa ..."`.
fn parse_gl_major(version: &str) -> Option<u32> {
    let first = version.trim().split(['.', ' ']).next()?;
    first.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(major: u32) -> impl FnOnce() -> Option<ProbeResult> {
        move || {
            Some(ProbeResult {
                major,
                version: format!("{major}.0.0"),
                renderer: "Test Renderer".to_string(),
            })
        }
    }
    fn no_probe() -> Option<ProbeResult> {
        None
    }

    #[test]
    fn child_continues_without_probe_or_reexec() {
        // The child must short-circuit even if SKYLANDER_GL/force says otherwise.
        let p = decide(true, Some("hardware"), true, || {
            panic!("child must not probe")
        });
        assert!(matches!(p, Plan::SoftwareChild(_)));
    }

    #[test]
    fn force_software_reexecs_when_runtime_present() {
        let p = decide(false, Some("software"), true, || panic!("probe skipped"));
        assert!(matches!(p, Plan::ReExecSoftware(_)));
    }

    #[test]
    fn force_software_without_runtime_stays_hardware() {
        let p = decide(false, Some("software"), false, || panic!("probe skipped"));
        assert!(matches!(p, Plan::Hardware(_)));
    }

    #[test]
    fn force_hardware_never_probes_or_reexecs() {
        let p = decide(false, Some("hardware"), true, || {
            panic!("probe must not run")
        });
        assert!(matches!(p, Plan::Hardware(_)));
    }

    #[test]
    fn auto_hardware_gl_3plus_stays_system() {
        let p = decide(false, None, true, probe(4));
        assert!(matches!(p, Plan::Hardware(d) if d.contains("hardware OpenGL")));
    }

    #[test]
    fn auto_gl_1_1_reexecs_into_mesa() {
        let p = decide(false, None, true, probe(1));
        assert!(
            matches!(p, Plan::ReExecSoftware(_)),
            "GL 1.1 must fall back"
        );
    }

    #[test]
    fn auto_exactly_gl_3_stays_system() {
        let p = decide(false, None, true, probe(3));
        assert!(matches!(p, Plan::Hardware(_)), "GL 3.0 is enough for glow");
    }

    #[test]
    fn auto_inconclusive_probe_stays_system() {
        // A probe anomaly must never push a working machine onto software GL.
        let p = decide(false, None, true, no_probe);
        assert!(matches!(p, Plan::Hardware(d) if d.contains("inconclusive")));
    }

    #[test]
    fn no_bundled_runtime_is_hardware() {
        let p = decide(false, None, false, || {
            panic!("probe skipped without runtime")
        });
        assert!(matches!(p, Plan::Hardware(_)));
    }

    #[test]
    fn unknown_force_value_falls_through_to_auto() {
        let p = decide(false, Some("sw"), true, probe(1));
        assert!(matches!(&p, Plan::ReExecSoftware(d) if d.contains("ignored unknown")));
    }

    #[test]
    fn gl_major_parsing() {
        assert_eq!(parse_gl_major("4.6.0 NVIDIA 552.44"), Some(4));
        assert_eq!(parse_gl_major("1.1.0"), Some(1));
        assert_eq!(parse_gl_major("4.6 (Core Profile) Mesa 26.1.1"), Some(4));
        assert_eq!(parse_gl_major("3.0 Mesa"), Some(3));
        assert_eq!(parse_gl_major(""), None);
        assert_eq!(parse_gl_major("OpenGL ES 3.2"), None);
    }
}

//! AG.28's synthetic gameplay camera (openspec AG.28; design decision 19), a
//! camera system written with this writer in the shape of the original's
//! (`CameraSystem.ai`'s update over `camera control data`, a level
//! definition defining its cameras and a transition plane, a camera volume,
//! the sliding rocks' activator and their shake), our numbers throughout.
//!
//! - `global.bld`'s entry is [`attack`]'s with the camera system beside the
//!   portal manager: `camera control data`, a global holding the cameras
//!   defined (each an id, a distance, a pitch, a heading and a far clip),
//!   the view's numbers as they stand (`from`, `to` and the share `pct`
//!   between them), the plane, the shake and the look-from and look-at
//!   references; the macros `Camera - Define Camera`, `Camera - Define
//!   Camera Transition Plane`, `Camera - Transition to Camera` (the view's
//!   numbers from where they stand to a camera's over a time, `pct` slid
//!   with the ease given, at once with none) and `Game - Shake Cam` (`shake
//!   x` set to 2.5 times the magnitude, slid back to 0 over
//!   [`CAMERA_SHAKE_SECONDS`]); and the portal manager's tick, whose first
//!   statement calls `Camera - System - Update`: the target the first of
//!   `actorinfo.players` (none: nothing), the plane's blend while he touches
//!   its box (3G² - 2G³ toward its back camera, G 0.5 plus or minus his
//!   feet's horizontal distance from its middle over its length, held to
//!   [0, 1]; leaving it, the camera on the side he left by), the numbers
//!   `pct` of the way from `from` to `to`, then the look-from point placed
//!   at his feet and moved out along the camera's heading and pitch by its
//!   distance and the look-at point [`CAMERA_LOOK_UP`] ft over his feet, the
//!   two references naming him both, and the viewport put at the look-from
//!   point looking at the look-at point, its far clip the camera's and its
//!   field of view [`CAMERA_FOV`], moved across the view by `shake x`;
//! - the fighter's is [`attack`]'s with his minion walking: each tick, the
//!   stick held, facing the viewport's facing plus the stick's heading and
//!   going [`CAMERA_WALK_SPEED`] ft/s that way; let go, still;
//! - the level's entry: `Level_Definition_213` defining the cameras
//!   ([`CAMERA_START`], [`CAMERA_BACK`], [`CAMERA_VOLUME`],
//!   [`CAMERA_ROCKS`]) and the plane ([`CAMERA_PLANE`], [`CAMERA_START`]
//!   in front of it and [`CAMERA_BACK`] behind) at its STARTUP and
//!   switching to [`CAMERA_START`] at once; the plane's box; a camera
//!   volume ([`CAMERA_VOLUME_NAME`], running `Pop_To_Camera_x.ai`), its id
//!   the camera it switches to and its tilt the switch's seconds, polling
//!   every 0.08 s while he's outside it and switching with ease 20 and 20 the
//!   tick he's inside; the sliding rocks' activator
//!   ([`CAMERA_ACTIVATOR`], `Activator_Sliding_Rocks.ai`), woken within
//!   [`CAMERA_ACTIVATOR_RANGE`] ft, which opens the rocks, keeps itself
//!   awake and two seconds on switches to [`CAMERA_ROCKS`] over two with
//!   ease 20 and 20; and a rock ([`CAMERA_ROCK`], `Falling_Rocks.ai`), which
//!   shakes the camera by [`CAMERA_SHAKE`] the tick it finds itself open.
//!
//! [`CameraFault::Frame`] makes the update stop on a member nothing models
//! [`CAMERA_FAULT_TICK`] ticks after it first finds a target.

use super::*;

const FLOAT: &str = "FloatMeasurement";
const INT: &str = "IntMeasurement";

/// A camera the synthetic level defines: its id, its eye's distance from
/// his feet (ft), the eye's pitch up and heading from them (degrees), and
/// its far clip (ft).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntheticCam {
    pub id: i32,
    pub distance: f32,
    pub pitch: f32,
    pub heading: f32,
    pub far: f32,
}

/// The level's first camera: the eye behind him (along -z), so the view
/// looks along +z.
pub const CAMERA_START: SyntheticCam = SyntheticCam {
    id: 0,
    distance: 30.0,
    pitch: 25.0,
    heading: 180.0,
    far: 600.0,
};
/// The camera behind the plane: farther and higher, looking the same way.
pub const CAMERA_BACK: SyntheticCam = SyntheticCam {
    id: 2,
    distance: 45.0,
    pitch: 40.0,
    heading: 180.0,
    far: 900.0,
};
/// The volume's camera: the eye off his -x side, so the view looks along
/// +x.
pub const CAMERA_VOLUME: SyntheticCam = SyntheticCam {
    id: 20,
    distance: 35.0,
    pitch: 20.0,
    heading: 270.0,
    far: 700.0,
};
/// The camera the activator switches to two seconds after it wakes.
pub const CAMERA_ROCKS: SyntheticCam = SyntheticCam {
    id: 7,
    distance: 57.0,
    pitch: 28.0,
    heading: 200.0,
    far: 1000.0,
};
/// How far over his feet the view aims (ft), and every camera's field of
/// view (degrees).
pub const CAMERA_LOOK_UP: f32 = 6.0;
pub const CAMERA_FOV: f32 = 68.0;

/// The transition plane's box: its name, and its width, height and length
/// (ft), the length along its facing (0).
pub const CAMERA_PLANE: &str = "Synthetic_Transition_0-2";
pub const CAMERA_PLANE_SIZE: [f32; 3] = [40.0, 12.0, 8.0];
/// The camera volume: its placement's name (its id [`CAMERA_VOLUME`]'s),
/// the seconds its tilt holds, its box (ft) and its activation range (ft).
pub const CAMERA_VOLUME_NAME: &str = "Pop_to_Camera_20_x";
pub const CAMERA_VOLUME_SECONDS: f32 = 1.5;
pub const CAMERA_VOLUME_SIZE: [f32; 3] = [6.0, 12.0, 6.0];
pub const CAMERA_VOLUME_RANGE: f32 = 50.0;
/// The sliding rocks' activator: its name, its activation range (ft), the
/// seconds it waits once woken and the switch's seconds.
pub const CAMERA_ACTIVATOR: &str = "Sliding_Rock_Activator_1";
pub const CAMERA_ACTIVATOR_RANGE: f32 = 20.0;
pub const CAMERA_ACTIVATOR_DELAY: f32 = 2.0;
pub const CAMERA_ACTIVATOR_SECONDS: f32 = 2.0;
/// Every switch's ease, out and in (each × 0.02 of its time).
pub const CAMERA_EASE: i32 = 20;
/// The rock, the shake's magnitude (`shake x` starts at 2.5 times it, ft)
/// and the seconds it dies down over.
pub const CAMERA_ROCK: &str = "Sliding_Rock_1";
pub const CAMERA_SHAKE: f32 = 1.0;
pub const CAMERA_SHAKE_SECONDS: f32 = 0.5;
/// How fast his minion walks with the stick held (ft/s).
pub const CAMERA_WALK_SPEED: f32 = 10.0;
/// The number the activator opens the rocks by, in the level's file.
pub const CAMERA_ROCKS_OPEN: &str = "Synthetic Rocks Open";
/// The update's ticks with a target before [`CameraFault::Frame`] stops it.
pub const CAMERA_FAULT_TICK: i32 = 120;

/// Whether the camera's update stops: never ([`CameraFault::None`]), or on
/// a member nothing models once it has had a target a while.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraFault {
    None,
    /// [`CAMERA_FAULT_TICK`] ticks after it first finds a target, on a
    /// member nothing models.
    Frame,
}

/// Where the level's placements stand (metres, their pivots): the plane's
/// box, the volume's, the activator and the rock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraAt {
    pub plane: [f32; 3],
    pub volume: [f32; 3],
    pub activator: [f32; 3],
    pub rock: [f32; 3],
}

/// What [`camera`] writes: [`Attack`]'s three entries with the camera
/// system and the walking minion, and the level's.
#[derive(Debug, Clone)]
pub struct Camera {
    pub global: Vec<u8>,
    pub init_setup: Vec<u8>,
    pub fighter: Vec<u8>,
    pub level: Vec<u8>,
}

/// AG.28's synthetic camera (the module docs), the level's placements at
/// `at`.
pub fn camera(at: CameraAt, fault: CameraFault) -> Camera {
    let a = attack();
    Camera {
        global: attack_global_with(Some(fault)),
        init_setup: a.init_setup,
        fighter: attack_fighter_with(true),
        level: camera_level(at),
    }
}

/// The camera's objects in `global.bld`'s file: the structures, the global
/// and the macros' variables.
pub(super) struct System {
    fault: CameraFault,
    cam_ty: Location,
    control_ty: Location,
    /// A camera's fields.
    id: Location,
    numbers: [Location; 4],
    /// `camera control data` and its fields.
    control: Location,
    cameras: Location,
    from: [Location; 4],
    to: [Location; 4],
    cur: [Location; 4],
    pct: Location,
    shake: Location,
    plane: Location,
    front: Location,
    back: Location,
    in_plane: Location,
    target: Location,
    look_from: Location,
    look_at: Location,
    g: [Location; 3],
    ticks: Location,
    /// The macros' parameters, as variables of their names.
    cam: [Location; 5],
    trigger: Location,
    front_id: Location,
    back_id: Location,
    to_id: Location,
    time: Location,
    ease_in: Location,
    ease_out: Location,
    magnitude: Location,
}

/// A camera's numbers by name: distance, pitch, heading, far clip.
const NUMBERS: [&str; 4] = ["distance", "pitch", "heading", "far clip"];

impl System {
    pub(super) fn new(w: &mut Writer<'_>, fault: CameraFault) -> Self {
        let cam_ty = w.op(Class::DefineStructure, "Synthetic Camera");
        let control_ty = w.op(Class::DefineStructure, "Synthetic Camera Control");
        let id = w.variable("id", "ValueInfo", Some(INT));
        let numbers = NUMBERS.map(|n| w.variable(n, "ValueInfo", Some(FLOAT)));
        let cameras = w.variable("cameras", "ScriptSet", None);
        w.refer(cameras, 0x24, cam_ty);
        let from = NUMBERS.map(|n| w.variable(&format!("from {n}"), "ValueInfo", Some(FLOAT)));
        let to = NUMBERS.map(|n| w.variable(&format!("to {n}"), "ValueInfo", Some(FLOAT)));
        let cur = NUMBERS.map(|n| w.variable(&format!("cur {n}"), "ValueInfo", Some(FLOAT)));
        let float = |w: &mut Writer<'_>, n: &str| w.variable(n, "ValueInfo", Some(FLOAT));
        let int = |w: &mut Writer<'_>, n: &str| w.variable(n, "ValueInfo", Some(INT));
        let reference = |w: &mut Writer<'_>, n: &str| w.variable(n, "ScriptReference", None);
        let pct = float(w, "pct");
        let shake = float(w, "shake x");
        let plane = reference(w, "plane");
        let front = int(w, "front");
        let back = int(w, "back");
        let in_plane = int(w, "in plane");
        let target = reference(w, "target");
        let look_from = reference(w, "look from");
        let look_at = reference(w, "look at");
        let g = [float(w, "g1"), float(w, "g2"), float(w, "g3")];
        let ticks = int(w, "ticks");
        let control = w.b.object(0, "ScriptStructure", 0x20);
        w.text(control, 0x08, "camera control data");
        w.refer(control, 0x1C, control_ty);
        let cam = [
            int(w, "cam id"),
            float(w, "cam distance"),
            float(w, "cam pitch"),
            float(w, "cam heading"),
            float(w, "cam far clip"),
        ];
        Self {
            fault,
            cam_ty,
            control_ty,
            id,
            numbers,
            control,
            cameras,
            from,
            to,
            cur,
            pct,
            shake,
            plane,
            front,
            back,
            in_plane,
            target,
            look_from,
            look_at,
            g,
            ticks,
            cam,
            trigger: reference(w, "trigger"),
            front_id: int(w, "front camera"),
            back_id: int(w, "back camera"),
            to_id: int(w, "to camera ID"),
            time: float(w, "transition time"),
            ease_in: int(w, "ease in"),
            ease_out: int(w, "ease out"),
            magnitude: float(w, "magnitude"),
        }
    }

    pub(super) fn structures(&self, w: &mut Writer<'_>) {
        let mut cam = vec![self.id];
        cam.extend(self.numbers);
        structure(w, self.cam_ty, &cam);
        let mut fields = vec![self.cameras];
        fields.extend(self.from);
        fields.extend(self.to);
        fields.extend(self.cur);
        fields.extend([
            self.pct,
            self.shake,
            self.plane,
            self.front,
            self.back,
            self.in_plane,
            self.target,
            self.look_from,
            self.look_at,
        ]);
        fields.extend(self.g);
        fields.push(self.ticks);
        structure(w, self.control_ty, &fields);
    }

    /// `camera control data.<field>`.
    fn at<'n>(&self, field: Location) -> [El<'n>; 2] {
        [El::Local(self.control), El::Local(field)]
    }

    fn read(&self, w: &mut Writer<'_>, field: Location) -> Location {
        w.rhs(FLOAT, &self.at(field))
    }

    /// `set camera control data.<field> = <rhs>`.
    fn put(&self, w: &mut Writer<'_>, field: Location, rhs: Location) {
        w.set(&self.at(field), rhs);
    }

    /// `for each` camera defined whose id is `id`: `body` with the camera
    /// as the loop's element.
    fn for_camera(
        &self,
        w: &mut Writer<'_>,
        id: impl FnOnce(&mut Writer<'_>) -> Location,
        body: impl FnOnce(&mut Writer<'_>, Location),
    ) {
        let set = w.stack(StackClass::Set, &self.at(self.cameras));
        let zero = w.int(0);
        let (at, each) = w.open(Class::ForEach, "for each");
        w.refer(each, 0x20, set);
        w.refer(each, 0x28, zero);
        let want = id(w);
        let found = w.check(&[El::Local(each), El::Local(self.id)], 1, want);
        body(w, each);
        w.end_block(found);
        w.end_loop(at);
    }

    /// `numbers[i] = <each>.<number i>` for each of the four.
    fn copy_from(&self, w: &mut Writer<'_>, each: Location, into: [Location; 4]) {
        for (i, field) in into.into_iter().enumerate() {
            let v = w.rhs(FLOAT, &[El::Local(each), El::Local(self.numbers[i])]);
            self.put(w, field, v);
        }
    }

    /// The macros; returns `Camera - System - Update`'s definition.
    pub(super) fn macros(&self, w: &mut Writer<'_>) -> Location {
        // A camera defined: one more in the set, its numbers the given.
        let (p, _) = define_with(
            w,
            "Camera - Define Camera",
            &[
                ("cam id", "ValueInfo"),
                ("cam distance", "ValueInfo"),
                ("cam pitch", "ValueInfo"),
                ("cam heading", "ValueInfo"),
                ("cam far clip", "ValueInfo"),
            ],
        );
        let set = self.at(self.cameras);
        w.inc(&[set[0], set[1], El::Member("setvariant", "count")]);
        let mut fields = vec![self.id];
        fields.extend(self.numbers);
        for (field, param) in fields.into_iter().zip(self.cam) {
            let v = w.rhs(FLOAT, &[El::Local(param)]);
            w.set(
                &[
                    set[0],
                    set[1],
                    El::Member("setvariant", "last"),
                    El::Local(field),
                ],
                v,
            );
        }
        w.flow_end();
        w.end_block(p);
        // The plane: its box and its two cameras' ids.
        let (p, _) = define_with(
            w,
            "Camera - Define Camera Transition Plane",
            &[
                ("trigger", "ScriptReference"),
                ("front camera", "ValueInfo"),
                ("back camera", "ValueInfo"),
            ],
        );
        setref(w, &self.at(self.plane), &[El::Local(self.trigger)]);
        let f = w.rhs(INT, &[El::Local(self.front_id)]);
        self.put(w, self.front, f);
        let b = w.rhs(INT, &[El::Local(self.back_id)]);
        self.put(w, self.back, b);
        w.flow_end();
        w.end_block(p);
        // A switch: from the numbers as they stand to the camera's, over
        // the time with the ease given; at once with none.
        let (p, _) = define_with(
            w,
            "Camera - Transition to Camera",
            &[
                ("to camera ID", "ValueInfo"),
                ("transition time", "ValueInfo"),
                ("ease in", "ValueInfo"),
                ("ease out", "ValueInfo"),
            ],
        );
        for i in 0..4 {
            let v = self.read(w, self.cur[i]);
            self.put(w, self.from[i], v);
        }
        self.for_camera(
            w,
            |w| w.rhs(INT, &[El::Local(self.to_id)]),
            |w, each| self.copy_from(w, each, self.to),
        );
        let zero = w.float(0.0);
        self.put(w, self.pct, zero);
        let none = w.float(0.0);
        let timed = w.check(&[El::Local(self.time)], 4, none);
        let pct = w.value(&self.at(self.pct));
        let one = w.float(1.0);
        let seconds = w.rhs(FLOAT, &[El::Local(self.time)]);
        let out = w.rhs(INT, &[El::Local(self.ease_out)]);
        let in_ = w.rhs(INT, &[El::Local(self.ease_in)]);
        slide(w, pct, one, seconds, out, in_);
        let at_once = w.otherwise(timed);
        let one = w.float(1.0);
        self.put(w, self.pct, one);
        w.close(at_once);
        w.flow_end();
        w.end_block(p);
        // The shake: across the view by 2.5 times the magnitude, dying
        // down.
        let (p, _) = define_with(w, "Game - Shake Cam", &[("magnitude", "ValueInfo")]);
        let m = w.rhs(FLOAT, &[El::Local(self.magnitude)]);
        let k = w.float(2.5);
        let x = w.expr(m, Some((2, k)));
        self.put(w, self.shake, x);
        let shake = w.value(&self.at(self.shake));
        let zero = w.float(0.0);
        let seconds = w.float(CAMERA_SHAKE_SECONDS);
        let (no_out, no_in) = (w.int(0), w.int(0));
        slide(w, shake, zero, seconds, no_out, no_in);
        w.flow_end();
        w.end_block(p);
        self.update(w)
    }

    /// `Camera - System - Update` (the module docs): its definition.
    fn update(&self, w: &mut Writer<'_>) -> Location {
        let (p, def) = define_with(w, "Camera - System - Update", &[]);
        setref(
            w,
            &self.at(self.target),
            &[
                El::Member("actorinfo", "players"),
                El::Member("setvariant", "first"),
            ],
        );
        let none = w.check_ref(
            &self.at(self.target),
            1,
            &[El::Member("abstractscriptgroup", "null")],
        );
        w.flow_end();
        w.end_block(none);
        if self.fault == CameraFault::Frame {
            w.inc(&self.at(self.ticks));
            let at = w.int(CAMERA_FAULT_TICK);
            let reached = w.check(&self.at(self.ticks), 1, at);
            let one = w.int(1);
            w.set(&[El::Member("synthetic", "unmodelled")], one);
            w.end_block(reached);
        }
        self.plane_blend(w);
        // The numbers `pct` of the way from `from` to `to`.
        for i in 0..4 {
            let to = self.read(w, self.to[i]);
            let from = self.read(w, self.from[i]);
            let d = w.expr(to, Some((1, from)));
            self.put(w, self.g[1], d);
            let d = self.read(w, self.g[1]);
            let pct = self.read(w, self.pct);
            let part = w.expr(d, Some((2, pct)));
            self.put(w, self.g[1], part);
            let from = self.read(w, self.from[i]);
            let part = self.read(w, self.g[1]);
            let now = w.expr(from, Some((0, part)));
            self.put(w, self.cur[i], now);
        }
        // The two points, both on him through their own references.
        let target = self.at(self.target);
        setref(w, &self.at(self.look_from), &target);
        setref(w, &self.at(self.look_at), &target);
        let remembered = El::Member("placementreference", "remembered");
        let from_point = [
            El::Local(self.control),
            El::Local(self.look_from),
            remembered,
        ];
        let at_point = [El::Local(self.control), El::Local(self.look_at), remembered];
        put_at(w, &from_point, &target);
        let len = self.read(w, self.cur[0]);
        let head = self.read(w, self.cur[2]);
        let pitch = self.read(w, self.cur[1]);
        displace_by(w, &from_point, 0, len, head, pitch);
        put_at(w, &at_point, &target);
        let (len, head, pitch) = (w.float(CAMERA_LOOK_UP), w.float(0.0), w.float(90.0));
        displace_by(w, &at_point, 0, len, head, pitch);
        // The viewport at the one looking at the other.
        let view = El::Member("viewportinfo", "default");
        put_at(w, &[view], &from_point);
        let turn = |name: &'static str| {
            [
                view,
                El::Member("matrixmeasurement", "orientation"),
                El::Member("orientationmeasurement", name),
            ]
        };
        for (name, member) in [("facing", "heading"), ("tilt", "pitch")] {
            let v = w.rhs(
                FLOAT,
                &[
                    at_point[0],
                    at_point[1],
                    at_point[2],
                    El::Member("positionmeasurement", member),
                ],
            );
            let from = w.reference(&from_point, None);
            w.refer(v, 0x18, from);
            w.set(&turn(name), v);
        }
        let far = self.read(w, self.cur[3]);
        w.set(&[view, El::Member("viewportinfo", "far clip")], far);
        let fov = w.float(CAMERA_FOV);
        w.set(&[view, El::Member("viewportinfo", "field of view")], fov);
        let level = w.int(0);
        w.set(&turn("roll"), level);
        // The shake: the eye across the view.
        let still = w.float(0.0);
        let shaking = w.check(&self.at(self.shake), 5, still);
        let len = self.read(w, self.shake);
        let facing = w.rhs(FLOAT, &turn("facing"));
        let quarter = w.int(90);
        let head = w.expr(facing, Some((1, quarter)));
        let pitch = w.int(0);
        displace_by(w, &[view], 0, len, head, pitch);
        w.end_block(shaking);
        w.flow_end();
        w.end_block(p);
        def
    }

    /// The plane's blend while he touches its box; leaving it, the camera
    /// on the side he left by.
    fn plane_blend(&self, w: &mut Writer<'_>) {
        let defined = w.check_ref(
            &self.at(self.plane),
            5,
            &[El::Member("abstractscriptgroup", "null")],
        );
        let plane = self.at(self.plane);
        let target = self.at(self.target);
        let touch = w.value(&[
            plane[0],
            plane[1],
            El::Member("positionmeasurement", "collision"),
        ]);
        let him = w.reference(&target, None);
        w.refer(touch, 0x18, him);
        let one = w.int(1);
        let (inside, o) = w.open(Class::CheckValue, "if");
        w.refer(o, 0x1C, touch);
        w.word(o, 0x20, 1);
        w.refer(o, 0x24, one);
        // G1: his feet's horizontal distance from its middle over its
        // length, at most a half.
        let hd = w.rhs(
            FLOAT,
            &[
                plane[0],
                plane[1],
                El::Member("positionmeasurement", "horizontal distance"),
            ],
        );
        let him = w.reference(&target, None);
        w.refer(hd, 0x18, him);
        let length = w.rhs(
            FLOAT,
            &[
                plane[0],
                plane[1],
                El::Member("actorinfo", "collision volume"),
                El::Member("collisioninfo", "length"),
            ],
        );
        let share = w.expr(hd, Some((3, length)));
        self.put(w, self.g[0], share);
        let half = w.float(0.5);
        let over = w.check(&self.at(self.g[0]), 4, half);
        let half = w.float(0.5);
        self.put(w, self.g[0], half);
        w.end_block(over);
        // Ahead of its middle along its facing or behind it.
        let heading = w.rhs(
            FLOAT,
            &[
                target[0],
                target[1],
                El::Member("positionmeasurement", "heading"),
            ],
        );
        let from = w.reference(&plane, None);
        w.refer(heading, 0x18, from);
        let facing = w.rhs(
            FLOAT,
            &[
                plane[0],
                plane[1],
                El::Member("matrixmeasurement", "orientation"),
                El::Member("orientationmeasurement", "facing"),
            ],
        );
        let angle = w.expr(heading, Some((1, facing)));
        let normalized = [El::Member("system", "normalized angle")];
        w.set(&normalized, angle);
        let behind_set = |w: &mut Writer<'_>| {
            let half = w.float(0.5);
            let g = self.read(w, self.g[0]);
            let v = w.expr(half, Some((1, g)));
            self.put(w, self.g[0], v);
        };
        let quarter = w.float(90.0);
        let behind = w.check(&normalized, 4, quarter);
        behind_set(w);
        let not_behind = w.otherwise(behind);
        let quarter = w.float(-90.0);
        let behind_too = w.check(&normalized, 3, quarter);
        behind_set(w);
        let ahead = w.otherwise(behind_too);
        let half = w.float(0.5);
        let g = self.read(w, self.g[0]);
        let v = w.expr(half, Some((0, g)));
        self.put(w, self.g[0], v);
        w.close(ahead);
        w.close(not_behind);
        // 3G² - 2G³.
        let (g1, g2, g3) = (self.g[0], self.g[1], self.g[2]);
        let a = self.read(w, g1);
        let b = self.read(w, g1);
        let sq = w.expr(a, Some((2, b)));
        self.put(w, g2, sq);
        let a = self.read(w, g2);
        let b = self.read(w, g1);
        let cube = w.expr(a, Some((2, b)));
        self.put(w, g3, cube);
        let three = w.float(3.0);
        let a = self.read(w, g2);
        let v = w.expr(three, Some((2, a)));
        self.put(w, g2, v);
        let two = w.float(2.0);
        let a = self.read(w, g3);
        let v = w.expr(two, Some((2, a)));
        self.put(w, g3, v);
        let a = self.read(w, g2);
        let b = self.read(w, g3);
        let v = w.expr(a, Some((1, b)));
        self.put(w, self.pct, v);
        self.for_camera(
            w,
            |w| w.rhs(INT, &self.at(self.front)),
            |w, each| self.copy_from(w, each, self.from),
        );
        self.for_camera(
            w,
            |w| w.rhs(INT, &self.at(self.back)),
            |w, each| self.copy_from(w, each, self.to),
        );
        let one = w.int(1);
        self.put(w, self.in_plane, one);
        // Leaving it.
        let outside = w.otherwise(inside);
        let one = w.int(1);
        let was = w.check(&self.at(self.in_plane), 1, one);
        let zero = w.int(0);
        self.put(w, self.in_plane, zero);
        let half = w.float(0.5);
        let past = w.check(&self.at(self.pct), 4, half);
        let one = w.float(1.0);
        self.put(w, self.pct, one);
        let back = w.otherwise(past);
        let zero = w.float(0.0);
        self.put(w, self.pct, zero);
        w.close(back);
        w.end_block(was);
        w.close(outside);
        w.end_block(defined);
    }

    /// The portal manager's tick's first statement: `update` called, the
    /// definition [`System::macros`] returned.
    pub(super) fn tick(w: &mut Writer<'_>, update: Location) {
        call_stub(w, update, &[]);
    }
}

/// `set <path>.positionmeasurement.distance.@<other> = 0`: the point at
/// `other`.
fn put_at(w: &mut Writer<'_>, path: &[El<'_>], other: &[El<'_>]) {
    let mut p = path.to_vec();
    p.push(El::Member("positionmeasurement", "distance"));
    let lhs = w.value(&p);
    let at = w.reference(other, None);
    w.refer(lhs, 0x18, at);
    let zero = w.int(0);
    let o = w.op(Class::SetValue, "set||=");
    w.refer(o, 0x1C, lhs);
    w.refer(o, 0x20, zero);
    w.push(o);
}

/// `displace <np> <combine> <len> at <head>, <pitch>`, each operand read.
fn displace_by(
    w: &mut Writer<'_>,
    np: &[El<'_>],
    combine: u32,
    len: Location,
    head: Location,
    pitch: Location,
) {
    let target = w.group(np, None);
    let o = w.op(Class::Displace, "displace");
    w.refer(o, 0x1C, target);
    w.word(o, 0x20, combine);
    w.refer(o, 0x24, len);
    w.refer(o, 0x28, head);
    w.refer(o, 0x2C, pitch);
    w.push(o);
}

/// `slide value <lhs> to <to> over <seconds> EASE: out <out> in <in_>`,
/// its block empty.
fn slide(
    w: &mut Writer<'_>,
    lhs: Location,
    to: Location,
    seconds: Location,
    out: Location,
    in_: Location,
) {
    let (p, o) = w.open(
        Class::SlideValue,
        "slide value||to|over|EASE:|out|in||slider",
    );
    w.refer(o, 0x1C, lhs);
    w.refer(o, 0x20, to);
    w.refer(o, 0x24, seconds);
    w.refer(o, 0x28, out);
    w.refer(o, 0x2C, in_);
    w.end_block(p);
}

/// `if <placement>.positionmeasurement.collision.@<first player> == 1`:
/// its position; the block follows.
fn touching_him(w: &mut Writer<'_>) -> usize {
    let lhs = w.value(&[
        El::Member("optoplevelbehavior", "my"),
        El::Member("positionmeasurement", "collision"),
    ]);
    let him = w.reference(
        &[
            El::Member("actorinfo", "players"),
            El::Member("setvariant", "first"),
        ],
        None,
    );
    w.refer(lhs, 0x18, him);
    let one = w.int(1);
    let (p, o) = w.open(Class::CheckValue, "if");
    w.refer(o, 0x1C, lhs);
    w.word(o, 0x20, 1);
    w.refer(o, 0x24, one);
    p
}

/// A stub call of `Camera - Transition to Camera`.
fn switch_to(w: &mut Writer<'_>, stub: Location, id: Location, seconds: Location) {
    let (in_, out) = (w.int(CAMERA_EASE), w.int(CAMERA_EASE));
    call_stub(
        w,
        stub,
        &[
            ("to camera ID", "ValueInfo", id),
            ("transition time", "ValueInfo", seconds),
            ("ease in", "ValueInfo", in_),
            ("ease out", "ValueInfo", out),
        ],
    );
}

/// `system.script timer`, read.
fn timer(w: &mut Writer<'_>) -> Location {
    w.rhs(FLOAT, &[El::Member("system", "script timer")])
}

/// The level's entry (the module docs).
fn camera_level(spots: CameraAt) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let root = b.object(0, "igObjectList", 0x18);
    let plane = b.object(0, "tfbActorInfo", 0xF8);
    let my = El::Member("optoplevelbehavior", "my");
    // Placed at `p` with `id`, `angles`, scale 100 and its range and buffer.
    let placed = |b: &mut IgzBuilder,
                  a: Location,
                  name: &str,
                  script: Option<Location>,
                  id: u32,
                  p: [f32; 3],
                  angles: [f32; 3],
                  range: f32| {
        let [x, y, z] = p;
        let [t, h, r] = angles;
        place(b, a, name, script, 4, &[x, y, z, t, h, r]);
        b.put_u32(at(a, 0x20), id);
        let scale: Vec<u8> = [100.0f32; 3].iter().flat_map(|v| v.to_be_bytes()).collect();
        b.put_bytes(at(a, 0xB8), &scale);
        let range: Vec<u8> = [range, 0.0].iter().flat_map(|v| v.to_be_bytes()).collect();
        b.put_bytes(at(a, 0xD0), &range);
    };
    let definition_script = {
        let mut w = Writer::new(&mut b);
        let define = w.op(Class::DefineMacro, "Camera - Define Camera");
        let define_plane = w.op(
            Class::DefineMacro,
            "Camera - Define Camera Transition Plane",
        );
        let switch = w.op(Class::DefineMacro, "Camera - Transition to Camera");
        w.script(
            "C:/tfb/Content/Levels/Level_213/Scripts/Level_Definition_213.ai",
            |w| {
                for c in [CAMERA_START, CAMERA_BACK, CAMERA_VOLUME, CAMERA_ROCKS] {
                    let id = w.int(c.id);
                    let (d, p, h, f) = (
                        w.float(c.distance),
                        w.float(c.pitch),
                        w.float(c.heading),
                        w.float(c.far),
                    );
                    call_stub(
                        w,
                        define,
                        &[
                            ("cam id", "ValueInfo", id),
                            ("cam distance", "ValueInfo", d),
                            ("cam pitch", "ValueInfo", p),
                            ("cam heading", "ValueInfo", h),
                            ("cam far clip", "ValueInfo", f),
                        ],
                    );
                }
                let trigger = w.reference(&[El::Local(plane)], None);
                let (front, back) = (w.int(CAMERA_START.id), w.int(CAMERA_BACK.id));
                call_stub(
                    w,
                    define_plane,
                    &[
                        ("trigger", "ScriptReference", trigger),
                        ("front camera", "ValueInfo", front),
                        ("back camera", "ValueInfo", back),
                    ],
                );
                let (id, now) = (w.int(CAMERA_START.id), w.float(0.0));
                switch_to(w, switch, id, now);
            },
            |_| {},
            |_| {},
        )
    };
    let definition = actor_with(&mut b, "Level_Definition_213", Some(definition_script), 4);
    placed(
        &mut b,
        plane,
        CAMERA_PLANE,
        None,
        0,
        spots.plane,
        [0.0; 3],
        0.0,
    );
    // A box (1), touching nothing.
    collision(&mut b, plane, 0x01, CAMERA_PLANE_SIZE);
    // The volume: its id the camera, its tilt the seconds.
    let volume_script = {
        let mut w = Writer::new(&mut b);
        let switch = w.op(Class::DefineMacro, "Camera - Transition to Camera");
        let wait = w.op(Class::UserBehavior, "Wait");
        let end = w.op(Class::UserBehavior, "End");
        let poll = w.variable("Timer", "ValueInfo", Some(FLOAT));
        let script = w.script(
            "C:/tfb/Content/Levels/Level_213/Scripts/Pop_To_Camera_x.ai",
            |_| {},
            |_| {},
            |w| {
                let p = w.push(wait);
                let now = timer(w);
                let due = w.check(&[El::Local(poll)], 0, now);
                let inside = touching_him(w);
                let now = timer(w);
                let later = w.float(0.08);
                let next = w.expr(now, Some((0, later)));
                w.set(&[El::Local(poll)], next);
                let id = w.rhs(INT, &[my, El::Member("abstractplacement", "id")]);
                let seconds = w.rhs(
                    FLOAT,
                    &[
                        my,
                        El::Member("matrixmeasurement", "orientation"),
                        El::Member("orientationmeasurement", "tilt"),
                    ],
                );
                switch_to(w, switch, id, seconds);
                w.set_behavior(end);
                w.end_block(inside);
                w.end_block(due);
                w.end_block(p);
                let p = w.push(end);
                w.end_block(p);
            },
        );
        w.declare(script, &[poll]);
        script
    };
    let volume = b.object(0, "tfbActorInfo", 0xF8);
    placed(
        &mut b,
        volume,
        CAMERA_VOLUME_NAME,
        Some(volume_script),
        CAMERA_VOLUME.id as u32,
        spots.volume,
        [CAMERA_VOLUME_SECONDS, 0.0, 0.0],
        CAMERA_VOLUME_RANGE,
    );
    collision(&mut b, volume, 0x01, CAMERA_VOLUME_SIZE);
    // The activator: woken in range, it opens the rocks and switches.
    let (activator_script, open) = {
        let mut w = Writer::new(&mut b);
        let switch = w.op(Class::DefineMacro, "Camera - Transition to Camera");
        let wait = w.op(Class::UserBehavior, "Wait");
        let delay = w.op(Class::UserBehavior, "Delay");
        let due = w.variable("Timer", "ValueInfo", Some(FLOAT));
        let open = file_int(&mut w, CAMERA_ROCKS_OPEN);
        let script = w.script(
            "C:/tfb/Content/Levels/Level_213/Scripts/Activator_Sliding_Rocks.ai",
            |_| {},
            |_| {},
            |w| {
                let p = w.push(wait);
                let one = w.int(1);
                w.set(&[El::Local(open)], one);
                let zero = w.int(0);
                w.set(&[my, El::Member("placement", "activation range")], zero);
                let now = timer(w);
                let wait_for = w.float(CAMERA_ACTIVATOR_DELAY);
                let then = w.expr(now, Some((0, wait_for)));
                w.set(&[El::Local(due)], then);
                w.set_behavior(delay);
                w.end_block(p);
                let p = w.push(delay);
                let now = timer(w);
                let ready = w.check(&[El::Local(due)], 0, now);
                let (id, seconds) = (w.int(CAMERA_ROCKS.id), w.float(CAMERA_ACTIVATOR_SECONDS));
                switch_to(w, switch, id, seconds);
                w.remove(&[my]);
                w.end_block(ready);
                w.end_block(p);
            },
        );
        w.declare(script, &[due]);
        (script, open)
    };
    let activator = b.object(0, "tfbActorInfo", 0xF8);
    placed(
        &mut b,
        activator,
        CAMERA_ACTIVATOR,
        Some(activator_script),
        0,
        spots.activator,
        [0.0; 3],
        CAMERA_ACTIVATOR_RANGE,
    );
    // The rock: open, it shakes the camera once.
    let rock_script = {
        let mut w = Writer::new(&mut b);
        let shake = w.op(Class::DefineMacro, "Game - Shake Cam");
        let waiting = w.op(Class::UserBehavior, "Waiting");
        let fallen = w.op(Class::UserBehavior, "Fallen");
        w.script(
            "C:/tfb/Content/Levels/Level_213/Scripts/Falling_Rocks.ai",
            |_| {},
            |_| {},
            |w| {
                let p = w.push(waiting);
                let one = w.int(1);
                let opened = w.check(&[El::Local(open)], 1, one);
                let m = w.float(CAMERA_SHAKE);
                call_stub(w, shake, &[("magnitude", "ValueInfo", m)]);
                w.set_behavior(fallen);
                w.end_block(opened);
                w.end_block(p);
                let p = w.push(fallen);
                w.end_block(p);
            },
        )
    };
    let rock = b.object(0, "tfbActorInfo", 0xF8);
    placed(
        &mut b,
        rock,
        CAMERA_ROCK,
        Some(rock_script),
        0,
        spots.rock,
        [0.0; 3],
        0.0,
    );
    let blocks = b.section(0x10);
    crate::igz::layouts::builder::list(
        &mut b,
        root,
        blocks,
        &[
            Some(definition),
            Some(plane),
            Some(volume),
            Some(activator),
            Some(rock),
        ],
    );
    b.build()
}

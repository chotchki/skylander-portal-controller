//! A dialogue's yes-or-no question on the TV's run (openspec AU.6, AU
//! design decision 2): what the scripts ask and how the answer goes back.
//!
//! - **The question.** A dialogue made with a yes-or-no title (Cali's
//!   challenge question in her hub intro, Flynn's ready question before the
//!   next chapter) says its lines, then, at state 11 of the dialogue UI
//!   (`UI Systems`' `current dialogue ui`), shows the original's confirmation
//!   prompt (`Dialogue - Show Yes/No Prompt`): its `control - yes_no prompt
//!   status` set to 0 and a menu page shown whose definition's `extra def
//!   data` holds the dialogue UI. The dialogue waits there until the status
//!   moves off 0: 1 for no, 2 for yes ([`up`]).
//! - **The answer.** The page's buttons run `Dialogue - No Action` or
//!   `Dialogue - Yes Action` on the page (the dialogue's `dialogue accepted`
//!   or `dialogue cancelled` set, the status moved, `Dialogue Last YesNo
//!   Accepted` written), then `Menu - Close Menu Immediately`, which takes
//!   the page off. [`answer`] runs those same three macros on the page, in
//!   the game systems' name, as the original's menu does when its button is
//!   pressed.
//!
//! What's ours (stand-in ledger: `yes-no`): the TV asks the question on a
//! modal of its own and runs the answer, in place of the menu system drawing
//! the confirmation prompt's page and taking its accept press (the host
//! feeds no menu buttons, and the interface draws nothing yet).

use guac_script::{Halt, Value, Vm};

use crate::scripts::{ScriptWorld, WorldHost, running};

/// The dialogue UI's state while its yes-or-no question waits.
pub const ASKING: i32 = 11;
/// `control - yes_no prompt status`: waiting, no, yes.
pub const WAITING: i32 = 0;
pub const NO: i32 = 1;
pub const YES: i32 = 2;

/// The macros the confirmation prompt's buttons run.
pub const YES_ACTION: &str = "Dialogue - Yes Action";
pub const NO_ACTION: &str = "Dialogue - No Action";
pub const CLOSE_MENU: &str = "Menu - Close Menu Immediately";

/// The game systems' script, in whose name the answer runs.
const GAME_SYSTEMS: &str = "GameSystems.ai";

/// A question the scripts wait on.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    /// The dialogue UI asking it.
    pub ui: Value,
    /// Its question: the string the dialogue was made with.
    pub question: Value,
    /// The question's English text, once the interface's model reads it.
    pub text: Option<String>,
    /// Whether the original's prompt starts on Yes (`force yes focus`).
    pub yes_first: bool,
    /// The menu page showing it, whose buttons the answer runs as.
    pub page: Value,
}

fn global(vm: &Vm, name: &str) -> Option<Value> {
    let g = vm.program().global_named(name)?;
    vm.global_peek(g).map(|v| vm.value(v))
}

fn field(vm: &Vm, v: Value, name: &str) -> Option<Value> {
    vm.field(v, name).map(|f| vm.value(f))
}

/// The question the current dialogue UI waits on, if one does and its page
/// is up.
pub fn up(vm: &Vm, world: &ScriptWorld) -> Option<Prompt> {
    let ui = global(vm, "UI Systems").and_then(|u| field(vm, u, "current dialogue ui"))?;
    if ui == Value::Null
        || field(vm, ui, "state").and_then(|s| s.as_i32()) != Some(ASKING)
        || field(vm, ui, "control - yes_no prompt status").and_then(|s| s.as_i32()) != Some(WAITING)
    {
        return None;
    }
    let text = field(vm, ui, "text data")?;
    let question = field(vm, text, "yes_no dialogue string").filter(|q| *q != Value::Null)?;
    let page = global(vm, "menu control data").and_then(|m| field(vm, m, "current menu"))?;
    let holds = field(vm, page, "menu def")
        .and_then(|d| field(vm, d, "extra def data"))
        .and_then(|s| vm.set(s).map(|e| e.contains(&ui)))
        .unwrap_or(false);
    if !holds {
        return None;
    }
    let yes_first = field(vm, text, "force yes focus")
        .and_then(|f| f.as_i32())
        .is_some_and(|f| f > 0);
    Some(Prompt {
        ui,
        question,
        text: world.text_of(question),
        yes_first,
        page,
    })
}

/// Whether `a` and `b` are one question: the same dialogue UI asking the
/// same string on the same page.
pub fn same(a: &Prompt, b: &Prompt) -> bool {
    (a.ui, a.question, a.page) == (b.ui, b.question, b.page)
}

/// `prompt` answered as its page's yes or no button answers it: the
/// button's action on the page, then the page closed. False when that
/// question is no longer up (another on the same dialogue UI included) or
/// the run has no game systems to answer in.
pub fn answer(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    prompt: &Prompt,
    yes: bool,
) -> Result<bool, Halt> {
    if !up(vm, host.world).is_some_and(|p| same(&p, prompt)) {
        return Ok(false);
    }
    // The original's menu runs its buttons in the UI's tick; the game
    // systems' placement stands in for it, as the level end's skip does.
    let Some(gs) = running(vm, host.world, GAME_SYSTEMS) else {
        return Ok(false);
    };
    let Some(control) = global(vm, "menu control data") else {
        return Ok(false);
    };
    let action = if yes { YES_ACTION } else { NO_ACTION };
    vm.call_for(gs, action, &[("current page info", prompt.page)], &[], host)?;
    vm.call_for(
        gs,
        CLOSE_MENU,
        &[
            ("current page info", prompt.page),
            ("cur menu control data", control),
        ],
        &[],
        host,
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph, StackClass};
    use guac_script::{Config, Generator, Mode, Obj, Program, Role, Unit, Whitelist};

    use super::*;
    use crate::script_engine::NoEngine;
    use crate::scripts::{Files, Placed, headless_objects, headless_spawner};

    const FILE: Obj = Obj { unit: 0, index: 2 };
    const HANDLE: u64 = 0x30;
    const PATH: &str = "C:/tfb/Content/Levels/Includes/Global/scripts/GameSystems.ai";

    /// A file in the original's shape, our ops, as the game systems' script:
    /// `UI Systems`' current dialogue UI at `state`, its prompt status
    /// `status`, its text's question a marker named as Flynn's and `force
    /// yes focus` 1, and `menu control data`'s current page holding it in
    /// its definition's extra data when `held`; the prompt's buttons' three
    /// macros, the yes and no actions setting the status 2 and 1 and the
    /// close raising the page's `closed`.
    fn asking(state: i32, status: i32, held: bool) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let text_ty = w.op(Class::DefineStructure, "Dialogue Text Data");
            let question = w.variable("yes_no dialogue string", "ScriptReference", None);
            let yes_first = w.variable("force yes focus", "ValueInfo", Some("IntMeasurement"));
            let ui_ty = w.op(Class::DefineStructure, "Dialogue UI Control");
            let state_var = w.variable("state", "ValueInfo", Some("IntMeasurement"));
            let status_var = w.variable(
                "control - yes_no prompt status",
                "ValueInfo",
                Some("IntMeasurement"),
            );
            let text = w.variable("text data", "ScriptStructure", None);
            w.refer(text, 0x24, text_ty);
            let systems_ty = w.op(Class::DefineStructure, "UI Systems Data");
            let current_ui = w.variable("current dialogue ui", "ScriptStructure", None);
            w.refer(current_ui, 0x24, ui_ty);
            let def_ty = w.op(Class::DefineStructure, "Menu Def");
            let extra = w.variable("extra def data", "ScriptSet", None);
            w.refer(extra, 0x24, ui_ty);
            let page_ty = w.op(Class::DefineStructure, "Menu Page Info");
            let menu_def = w.variable("menu def", "ScriptStructure", None);
            w.refer(menu_def, 0x24, def_ty);
            let closed = w.variable("closed", "ValueInfo", Some("IntMeasurement"));
            let control_ty = w.op(Class::DefineStructure, "Menu Control Data");
            let page = w.variable("current menu", "ScriptStructure", None);
            w.refer(page, 0x24, page_ty);
            let systems = w.b.object(0, "ScriptStructure", 0x20);
            w.text(systems, 0x08, "UI Systems");
            w.refer(systems, 0x1C, systems_ty);
            let control = w.b.object(0, "ScriptStructure", 0x20);
            w.text(control, 0x08, "menu control data");
            w.refer(control, 0x1C, control_ty);
            let marker = w.op(Class::DefineMacro, "Flynn_YES/NO_string");
            let yes = w.op(Class::DefineMacro, YES_ACTION);
            let no = w.op(Class::DefineMacro, NO_ACTION);
            let close = w.op(Class::DefineMacro, CLOSE_MENU);
            let ui = [El::Local(systems), El::Local(current_ui)];
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let n = w.int(state);
            w.set(&[ui[0], ui[1], El::Local(state_var)], n);
            let n = w.int(status);
            w.set(&[ui[0], ui[1], El::Local(status_var)], n);
            let one = w.int(1);
            w.set(&[ui[0], ui[1], El::Local(text), El::Local(yes_first)], one);
            let l = w.value(&[ui[0], ui[1], El::Local(text), El::Local(question)]);
            let r = w.reference(&[El::Local(marker)], None);
            let o = w.op(Class::SetReference, "setref||=");
            w.refer(o, 0x1C, l);
            w.refer(o, 0x20, r);
            w.push(o);
            // The page up either way, its extra data holding the UI or not.
            let zero = w.int(0);
            w.set(
                &[El::Local(control), El::Local(page), El::Local(closed)],
                zero,
            );
            if held {
                let set = w.stack(
                    StackClass::Set,
                    &[
                        El::Local(control),
                        El::Local(page),
                        El::Local(menu_def),
                        El::Local(extra),
                    ],
                );
                let element = w.group(&ui, None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, set);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, element);
                w.push(o);
            }
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            let null = w.reference(&[El::Member("abstractscriptgroup", "null")], None);
            for (def, to) in [
                (marker, None),
                (yes, Some(YES)),
                (no, Some(NO)),
                (close, None),
            ] {
                let mut params = vec![("current page info", "ScriptReference", null)];
                if def == close {
                    params.push(("cur menu control data", "ScriptReference", null));
                }
                let p = w.open_macro(def, &params);
                if let Some(to) = to {
                    let n = w.int(to);
                    w.set(&[ui[0], ui[1], El::Local(status_var)], n);
                }
                if def == close {
                    let one = w.int(1);
                    w.set(
                        &[El::Local(control), El::Local(page), El::Local(closed)],
                        one,
                    );
                }
                w.end_block(p);
            }
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            for (ty, vars) in [
                (text_ty, vec![question, yes_first]),
                (ui_ty, vec![state_var, status_var, text]),
                (systems_ty, vec![current_ui]),
                (def_ty, vec![extra]),
                (page_ty, vec![menu_def, closed]),
                (control_ty, vec![page]),
            ] {
                let p = w.push(ty);
                for v in vars {
                    w.push(v);
                }
                w.end_block(p);
            }
            w.end_block(types);
            w.end_block(pre);
            w.end(PATH);
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    /// The fixture's STARTUP run on the game systems' placement; the
    /// prompt [`up`] reads then, answered by `answer` when given (the
    /// question as `asked` leaves what was read), and the status and the
    /// page's `closed` after.
    fn run(
        state: i32,
        status: i32,
        held: bool,
        answer_yes: Option<bool>,
    ) -> (Option<Prompt>, Option<bool>, Option<Prompt>, i32, i32) {
        run_asking(state, status, held, answer_yes, |p| p.clone())
    }

    fn run_asking(
        state: i32,
        status: i32,
        held: bool,
        answer_yes: Option<bool>,
        asked: impl Fn(&Prompt) -> Prompt,
    ) -> (Option<Prompt>, Option<bool>, Option<Prompt>, i32, i32) {
        let program = asking(state, status, held);
        let files = Files {
            placed: vec![Placed::named(FILE, "Macros_GameSystems", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &["GameSystems.ai"]);
        let sid = program.script_named("GameSystems.ai").unwrap();
        let mut vm = Vm::new(program, config);
        vm.add_at(HANDLE, "Macros_GameSystems", sid, FILE);
        let mut generator = Generator::seeded(1);
        let mut engine = NoEngine;
        let mut spawn = headless_spawner(0x1000);
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator: &mut generator,
            spawn: &mut spawn,
        };
        vm.step(&mut host).unwrap();
        let before = up(&vm, host.world);
        let answered = match (answer_yes, &before) {
            (Some(yes), Some(p)) => Some(answer(&mut vm, &mut host, &asked(p), yes).unwrap()),
            _ => None,
        };
        let after = up(&vm, host.world);
        let g = |vm: &Vm, name| vm.program().global_named(name).unwrap();
        let systems = vm.global(g(&vm, "UI Systems")).unwrap();
        let ui = vm.field(systems, "current dialogue ui").unwrap();
        let status = vm
            .field(ui, "control - yes_no prompt status")
            .and_then(|v| v.as_i32())
            .unwrap();
        let control = vm.global(g(&vm, "menu control data")).unwrap();
        let page = vm.field(control, "current menu").unwrap();
        let closed = vm
            .field(page, "closed")
            .and_then(|v| v.as_i32())
            .unwrap_or(0);
        (before, answered, after, status, closed)
    }

    /// openspec AU.6: a dialogue UI at its asking state with its status
    /// waiting and its page up is a question, its marker the question and
    /// Yes first; answered yes, the yes action and the page's close run
    /// (status 2, closed), no the no action (status 1), and it's gone. Any
    /// other state, a status off waiting, or a page not holding it isn't one.
    #[test]
    fn a_dialogues_question_is_read_off_its_ui_and_answered_through_its_page() {
        let (before, answered, after, status, closed) = run(ASKING, WAITING, true, Some(true));
        let p = before.expect("a question");
        assert!(p.yes_first);
        assert_eq!(p.text, None, "no interface to read it by");
        assert!(matches!(p.question, Value::Object(_)), "{:?}", p.question);
        assert_eq!(answered, Some(true));
        assert_eq!((status, closed), (YES, 1));
        assert_eq!(after, None, "answered, it's gone");
        let (_, answered, after, status, closed) = run(ASKING, WAITING, true, Some(false));
        assert_eq!(answered, Some(true));
        assert_eq!((status, closed), (NO, 1));
        assert_eq!(after, None);
        for (state, status, held) in [
            (ASKING - 1, WAITING, true),
            (ASKING, NO, true),
            (ASKING, YES, true),
            (ASKING, WAITING, false),
        ] {
            let (before, ..) = run(state, status, held, None);
            assert_eq!(before, None, "{state} {status} {held}");
        }
        // An answer chosen for another question on the same dialogue UI (its
        // string or its page another) answers nothing.
        for other in [
            (|p: &Prompt| Prompt {
                question: Value::Int(99),
                ..p.clone()
            }) as fn(&Prompt) -> Prompt,
            |p: &Prompt| Prompt {
                page: Value::Int(98),
                ..p.clone()
            },
        ] {
            let (_, answered, after, status, closed) =
                run_asking(ASKING, WAITING, true, Some(true), other);
            assert_eq!(answered, Some(false));
            assert!(after.is_some(), "still asking");
            assert_eq!((status, closed), (WAITING, 0));
        }
    }
}

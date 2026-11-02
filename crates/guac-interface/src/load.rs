//! The model a run's install loads (openspec AT.9): every sprite template,
//! font and string of the archives the run links, from the extract's
//! `interface` part (the extract-wide archives') and each level's `sprites`
//! part, each keyed by its object in its archive's `level.bld` so a
//! script's reference to it finds it (the crate docs, The keeper).

use std::collections::HashMap;

use guac_formats::exports::{Export, Exports, NAMESPACE};
use guac_formats::fnv1a32;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{BitmapFont, FontDataInfo, Layout, SpriteInfo};
use guac_formats::language::string_infos;
use guac_ir::interface::{Content, Import, Interface, Kind, TextKey};
use guac_script::{Obj, Value};

use crate::keeper::{BASES, Keeper};
use crate::model::{Model, Str};

/// `global.bld` and `bootstrap.bld`, as an interface record names them: the
/// archives an import's export id lands in.
pub const GLOBAL: &str = "permanent/global.bld";
pub const BOOTSTRAP: &str = "permanent/bootstrap.bld";

/// `AbstractPlacement._ID`'s offset: a template is a placement.
const ID: u32 = 0x20;
/// `AbstractPlacement._activityState`'s.
const ACTIVITY_STATE: u32 = 0x54;
/// The activity states a placement is awake in from the load: active (1)
/// and unmanaged (4), as a level's running placements are; a template (5)
/// sleeps until a script clones it.
const AWAKE: [u32; 2] = [1, 4];

/// What the loader needs of one archive's `level.bld`, read once where the
/// run's link parses it: its `StringInfo`s' objects in address order, its
/// fonts' objects in object order, the font each font data info names and
/// each sprite template's `_ID`. Small enough to keep beside the program.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Keys {
    pub unit: u16,
    pub strings: Vec<u32>,
    pub fonts: Vec<u32>,
    pub font_data: HashMap<u32, u32>,
    pub ids: HashMap<u32, i32>,
    /// Each sprite template's `_activityState` (AbstractPlacement's 0x54).
    pub states: HashMap<u32, u32>,
}

impl Keys {
    /// `igz`'s, as unit `unit` of the program.
    pub fn read(unit: u16, igz: &Igz<'_>) -> Self {
        let mut keys = Keys {
            unit,
            strings: string_infos(igz).into_iter().map(|o| o as u32).collect(),
            ..Keys::default()
        };
        for (index, o) in igz.objects().iter().enumerate() {
            let ty = igz.type_name(o);
            if ty == BitmapFont::TYPE {
                keys.fonts.push(index as u32);
            } else if ty == FontDataInfo::TYPE {
                if let Some(f) = FontDataInfo::read(igz, index).ok().and_then(|d| d.font) {
                    keys.font_data.insert(index as u32, f as u32);
                }
            } else if ty == SpriteInfo::TYPE {
                if let Some(id) = o.field(ID).and_then(|at| igz.i32_at(at, "_ID").ok()) {
                    keys.ids.insert(index as u32, id);
                }
                if let Some(state) = o
                    .field(ACTIVITY_STATE)
                    .and_then(|at| igz.i32_at(at, "_activityState").ok())
                {
                    keys.states.insert(index as u32, state as u32);
                }
            }
        }
        keys
    }
}

/// `global.bld`'s exports and the units of the two files they land in.
#[derive(Debug, Clone)]
pub struct Imports {
    pub exports: Exports,
    pub bootstrap: u16,
    pub global: u16,
}

impl Imports {
    /// The object an import names, if it lands on one of the two files'.
    pub fn resolve(&self, i: Import) -> Option<Obj> {
        if i.namespace != fnv1a32(NAMESPACE.as_bytes()) {
            return None;
        }
        let e = self.exports.resolve(i.object).ok()?;
        let unit = match e {
            Export::Bootstrap(_) => self.bootstrap,
            Export::Global(_) => self.global,
        };
        Some(Obj {
            unit,
            index: e.object()? as u32,
        })
    }
}

/// One archive the run links: its unit in the program, its `level.bld` and
/// its interface record ([`load_igz`]'s).
#[derive(Clone, Copy)]
pub struct Archive<'a, 'b> {
    pub unit: u16,
    pub igz: &'b Igz<'a>,
    pub interface: &'b Interface,
}

/// The keeper a run starts from, and what didn't load, by archive with why.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub keeper: Keeper,
    pub refused: Vec<(String, String)>,
}

/// [`load`] over parsed archives: each one's [`Keys`] read, the imports
/// resolved through `bootstrap.bld`'s and `global.bld`'s when both are
/// among them.
pub fn load_igz(archives: &[Archive<'_, '_>]) -> Loaded {
    let keys: Vec<Keys> = archives.iter().map(|a| Keys::read(a.unit, a.igz)).collect();
    let find = |name: &str| archives.iter().find(|a| a.interface.archive == name);
    let mut refused = Vec::new();
    let imports = match (find(BOOTSTRAP), find(GLOBAL)) {
        (Some(b), Some(g)) => match Exports::new(b.igz, g.igz) {
            Ok(exports) => Some(Imports {
                exports,
                bootstrap: b.unit,
                global: g.unit,
            }),
            Err(e) => {
                refused.push((GLOBAL.to_owned(), format!("its exports: {e}")));
                None
            }
        },
        _ => None,
    };
    let pairs: Vec<(&Keys, &Interface)> = keys
        .iter()
        .zip(archives.iter().map(|a| a.interface))
        .collect();
    let mut loaded = load(&pairs, imports.as_ref());
    refused.append(&mut loaded.refused);
    loaded.refused = refused;
    loaded
}

/// The keeper over every template, font and string of `archives`, each
/// with its file's [`Keys`]; `imports` resolves what a template imports
/// from `global.bld` or `bootstrap.bld`. An archive whose strings don't
/// pair with its `StringInfo`s keeps no string; a template whose imported
/// font or string doesn't resolve keeps none; each such is listed in
/// [`Loaded::refused`].
pub fn load(archives: &[(&Keys, &Interface)], imports: Option<&Imports>) -> Loaded {
    let mut model = Model::new(BASES);
    let mut refused = Vec::new();
    let resolve = |i: Import| imports.and_then(|m| m.resolve(i));
    // Each font by its object, for an imported font's data info to name.
    let mut fonts: HashMap<Obj, usize> = HashMap::new();
    let mut local_fonts: HashMap<(u16, usize), usize> = HashMap::new();
    for (k, ir) in archives {
        for (n, (&object, font)) in k.fonts.iter().zip(&ir.fonts).enumerate() {
            let at = model.add_font(font.clone());
            fonts.insert(
                Obj {
                    unit: k.unit,
                    index: object,
                },
                at,
            );
            local_fonts.insert((k.unit, n), at);
        }
    }
    // The font a data info names: its `_font`'s object.
    let font_of = |obj: Obj| -> Option<usize> {
        let (k, _) = archives.iter().find(|(k, _)| k.unit == obj.unit)?;
        let font = *k.font_data.get(&obj.index)?;
        fonts
            .get(&Obj {
                unit: obj.unit,
                index: font,
            })
            .copied()
    };
    for (k, ir) in archives {
        let keys = &k.strings;
        let strings = &ir.strings;
        let paired = keys.len() == strings.len();
        if !paired && !strings.is_empty() {
            refused.push((
                ir.archive.clone(),
                format!(
                    "{} strings for {} StringInfos: none kept",
                    strings.len(),
                    keys.len()
                ),
            ));
        }
        if paired {
            for (&object, s) in keys.iter().zip(strings) {
                model.add_file_string(
                    Obj {
                        unit: k.unit,
                        index: object,
                    },
                    &s.text,
                    Str {
                        units: Vec::new(),
                        flags: s.flags as u32,
                        integer_digits: s.integer_digits,
                        decimal_digits: s.decimal_digits,
                    },
                );
            }
        }
        for t in &ir.templates {
            let obj = Obj {
                unit: k.unit,
                index: t.object,
            };
            let text = match t.string {
                None => Value::Null,
                Some(TextKey::Local(n)) => match keys.get(n) {
                    Some(&o) => Value::Object(Obj {
                        unit: k.unit,
                        index: o,
                    }),
                    None => Value::Null,
                },
                Some(TextKey::Imported(i)) => match resolve(i) {
                    Some(o) => Value::Object(o),
                    None => {
                        refused.push((
                            ir.archive.clone(),
                            format!("{}'s imported string {:#x}", t.name, i.object),
                        ));
                        Value::Null
                    }
                },
            };
            let id = model.add_template(Some(obj), t, text);
            let font = match &t.content {
                Content::Text(n) => local_fonts.get(&(k.unit, *n)).copied(),
                Content::Imported(Kind::Text, i) => {
                    let f = resolve(*i).and_then(font_of);
                    if f.is_none() {
                        refused.push((
                            ir.archive.clone(),
                            format!("{}'s imported font {:#x}", t.name, i.object),
                        ));
                    }
                    f
                }
                _ => None,
            };
            model.set_font(id, font);
            if let Some(&n) = k.ids.get(&t.object) {
                model.sprite_mut(id).id = n;
            }
            if k.states.get(&t.object).is_some_and(|s| AWAKE.contains(s)) {
                model.wake(id);
            }
        }
    }
    let keeper = Keeper::new(model);
    Loaded { keeper, refused }
}

#[cfg(test)]
mod tests;

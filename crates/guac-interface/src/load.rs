//! The model a run's install loads (openspec AT.9): every sprite template,
//! font and string of the archives the run links, from the extract's
//! `interface` part (the extract-wide archives') and each level's `sprites`
//! part, each keyed by its object in its archive's `level.bld` so a
//! script's reference to it finds it (the crate docs, The keeper).

use std::collections::HashMap;

use guac_formats::exports::{Export, Exports, NAMESPACE};
use guac_formats::fnv1a32;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{FontDataInfo, Layout};
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

/// One archive the run links: its unit in the program, its `level.bld` and
/// its interface record.
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

/// The keeper over every template, font and string of `archives`. An
/// archive whose strings don't pair with its `StringInfo`s keeps no string;
/// a template whose imported font or string doesn't resolve keeps none;
/// each such is listed in [`Loaded::refused`].
pub fn load(archives: &[Archive<'_, '_>]) -> Loaded {
    let mut model = Model::new(BASES);
    let mut refused = Vec::new();
    let find = |name: &str| archives.iter().find(|a| a.interface.archive == name);
    let (bootstrap, global) = (find(BOOTSTRAP), find(GLOBAL));
    let exports = match (bootstrap, global) {
        (Some(b), Some(g)) => match Exports::new(b.igz, g.igz) {
            Ok(e) => Some(e),
            Err(e) => {
                refused.push((GLOBAL.to_owned(), format!("its exports: {e}")));
                None
            }
        },
        _ => None,
    };
    // The object an import names, by the unit of the file it lands in.
    let resolve = |i: Import| -> Option<Obj> {
        if i.namespace != fnv1a32(NAMESPACE.as_bytes()) {
            return None;
        }
        let e = exports.as_ref()?.resolve(i.object).ok()?;
        let unit = match e {
            Export::Bootstrap(_) => bootstrap?.unit,
            Export::Global(_) => global?.unit,
        };
        Some(Obj {
            unit,
            index: e.object()? as u32,
        })
    };
    // Each font by its object, for an imported font's data info to name.
    let mut fonts: HashMap<Obj, usize> = HashMap::new();
    let mut local_fonts: HashMap<(u16, usize), usize> = HashMap::new();
    for a in archives {
        let objects = a
            .igz
            .objects()
            .iter()
            .enumerate()
            .filter(|(_, o)| a.igz.type_name(o) == guac_formats::igz::layouts::BitmapFont::TYPE)
            .map(|(i, _)| i);
        for (n, (object, font)) in objects.zip(&a.interface.fonts).enumerate() {
            let at = model.add_font(font.clone());
            fonts.insert(
                Obj {
                    unit: a.unit,
                    index: object as u32,
                },
                at,
            );
            local_fonts.insert((a.unit, n), at);
        }
    }
    // The font a data info names: its `_font`'s object.
    let font_of = |obj: Obj| -> Option<usize> {
        let a = archives.iter().find(|a| a.unit == obj.unit)?;
        let data = FontDataInfo::read(a.igz, obj.index as usize).ok()?;
        fonts
            .get(&Obj {
                unit: obj.unit,
                index: data.font? as u32,
            })
            .copied()
    };
    for a in archives {
        let keys = string_infos(a.igz);
        let strings = &a.interface.strings;
        let paired = keys.len() == strings.len();
        if !paired && !strings.is_empty() {
            refused.push((
                a.interface.archive.clone(),
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
                        unit: a.unit,
                        index: object as u32,
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
        for t in &a.interface.templates {
            let obj = Obj {
                unit: a.unit,
                index: t.object,
            };
            let text = match t.string {
                None => Value::Null,
                Some(TextKey::Local(n)) => match keys.get(n) {
                    Some(&o) => Value::Object(Obj {
                        unit: a.unit,
                        index: o as u32,
                    }),
                    None => Value::Null,
                },
                Some(TextKey::Imported(i)) => match resolve(i) {
                    Some(o) => Value::Object(o),
                    None => {
                        refused.push((
                            a.interface.archive.clone(),
                            format!("{}'s imported string {:#x}", t.name, i.object),
                        ));
                        Value::Null
                    }
                },
            };
            let id = model.add_template(Some(obj), t, text);
            let font = match &t.content {
                Content::Text(n) => local_fonts.get(&(a.unit, *n)).copied(),
                Content::Imported(Kind::Text, i) => {
                    let f = resolve(*i).and_then(font_of);
                    if f.is_none() {
                        refused.push((
                            a.interface.archive.clone(),
                            format!("{}'s imported font {:#x}", t.name, i.object),
                        ));
                    }
                    f
                }
                _ => None,
            };
            model.set_font(id, font);
            let placed = a.igz.objects().get(t.object as usize).and_then(|o| {
                let at = o.field(ID)?;
                a.igz.i32_at(at, "_ID").ok()
            });
            if let Some(n) = placed {
                model.sprite_mut(id).id = n;
            }
        }
    }
    Loaded {
        keeper: Keeper::new(model),
        refused,
    }
}

#[cfg(test)]
mod tests;

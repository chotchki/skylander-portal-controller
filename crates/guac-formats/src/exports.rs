//! `global.bld` export ids (openspec AE.1): what an import in namespace
//! `global.bld` points at.
//!
//! An IGZ names an object of `permanent/global.bld` by an EXID entry whose
//! namespace is FNV-1a 32 of `global.bld` and whose object word is an export
//! id, not a name hash. Ids are a running index over the root lists of the
//! files loaded into that namespace, 1-based: `permanent/bootstrap.bld`'s 19
//! roots take ids 1-19, then `global.bld`'s root list starts at 0x14 (aj-vm
//! measured it in the live handle table). So id = root-list position + 0x14
//! for `global.bld`, and the six ids the disc uses under 0x14 are bootstrap
//! objects. The root list is a file's first object, an `igObjectList`; a
//! root is a local object or, once in `global.bld` (position 0xA91), an
//! import from another namespace, which an id hands back unresolved.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use crate::igz::layouts::{Layout, ObjectRef, RootList};
use crate::igz::{ExternalId, Igz};
use crate::{ErrorKind, FormatError, fnv1a32};

const FORMAT: &str = "global.bld exports";

/// Hashed with FNV-1a 32, it's the EXID namespace word of every import
/// [`Exports`] resolves.
pub const NAMESPACE: &str = "global.bld";

/// `global.bld`'s first export id, on Giants 01.00: 1 + bootstrap's 19 roots.
pub const GLOBAL_BASE: u32 = 0x14;

/// A root-list slot an export id can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// A local object of that file.
    Object(usize),
    /// An import the file re-exports, left for the caller.
    Import(ExternalId),
}

/// Which file an export id lands in, and its root there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Bootstrap(Root),
    Global(Root),
}

impl Export {
    /// The local object it names, unless it lands on an import.
    pub fn object(&self) -> Option<usize> {
        match self {
            Export::Bootstrap(Root::Object(o)) | Export::Global(Root::Object(o)) => Some(*o),
            _ => None,
        }
    }
}

/// A slot as stored: null and named slots refuse when an id lands on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Root(Root),
    Null,
    Named,
}

/// The two root lists ids run over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exports {
    bootstrap: Vec<Slot>,
    global: Vec<Slot>,
}

impl Exports {
    pub fn new(bootstrap: &Igz<'_>, global: &Igz<'_>) -> Result<Self, FormatError> {
        Ok(Self {
            bootstrap: roots(bootstrap)?,
            global: roots(global)?,
        })
    }

    /// `global.bld`'s first id: 0x14 with Giants' bootstrap.
    pub fn global_base(&self) -> u32 {
        1 + self.bootstrap.len() as u32
    }

    pub fn global_roots(&self) -> usize {
        self.global.len()
    }

    /// The root export id `id` names. Id 0, an id past the last root and a
    /// null or named root slot are refused.
    pub fn resolve(&self, id: u32) -> Result<Export, FormatError> {
        let limit = self.global_base() as u64 + self.global.len() as u64;
        let out_of_range = || {
            FormatError::new(
                FORMAT,
                "export id",
                0,
                ErrorKind::OutOfRange {
                    value: id.into(),
                    limit,
                },
            )
        };
        let position = id.checked_sub(1).ok_or_else(out_of_range)? as usize;
        let (slot, wrap): (Slot, fn(Root) -> Export) =
            match position.checked_sub(self.bootstrap.len()) {
                None => (self.bootstrap[position], Export::Bootstrap),
                Some(k) => (
                    *self.global.get(k).ok_or_else(out_of_range)?,
                    Export::Global,
                ),
            };
        match slot {
            Slot::Root(root) => Ok(wrap(root)),
            Slot::Null => Err(FormatError::new(FORMAT, "export id", 0, ErrorKind::Missing)),
            Slot::Named => Err(FormatError::new(
                FORMAT,
                "export id",
                0,
                ErrorKind::Unsupported { value: id.into() },
            )),
        }
    }

    /// Each local object `global.bld`'s root list exports, with its export
    /// id: what a file of `global.bld`'s own names by object where any other
    /// file imports it by id (openspec AH.39).
    pub fn global_objects(&self) -> impl Iterator<Item = (usize, u32)> + '_ {
        let base = self.global_base();
        self.global
            .iter()
            .enumerate()
            .filter_map(move |(k, slot)| match slot {
                Slot::Root(Root::Object(o)) => Some((*o, base + k as u32)),
                _ => None,
            })
    }

    /// An import's export, or `None` when it imports from another namespace.
    pub fn resolve_import(&self, id: &ExternalId) -> Option<Result<Export, FormatError>> {
        (id.namespace == fnv1a32(NAMESPACE.as_bytes())).then(|| self.resolve(id.object))
    }
}

/// A file's root list: its first object, an `igObjectList`.
fn roots(igz: &Igz<'_>) -> Result<Vec<Slot>, FormatError> {
    Ok(RootList::read(igz, 0)?
        .elements
        .into_iter()
        .map(|e| match e {
            ObjectRef::Object(o) => Slot::Root(Root::Object(o)),
            ObjectRef::Import(i) => Slot::Root(Root::Import(i.id)),
            ObjectRef::Null => Slot::Null,
            ObjectRef::Named { .. } => Slot::Named,
        })
        .collect())
}

#[cfg(test)]
mod tests;

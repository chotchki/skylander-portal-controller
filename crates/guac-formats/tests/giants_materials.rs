//! AE.6's material layouts and readers off the Giants dump: every object of
//! each new attribute and material-animation type in every character and
//! item `level.bld` read by its layout (MTSZ, the four render states' file
//! pad, every fixup claimed), then every sprite-drawn model with a skeleton
//! walked and each of its techniques' materials read by the tfbmulti2
//! reader, refusals tallied by reason. Ignored by default; it reads
//! `GUAC_GIANTS_DUMP` like `giants_layouts`:
//!
//! `cargo test -p guac-formats --release --test giants_materials -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use common::usrdir;
use guac_formats::FormatError;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    AlphaFunctionAttr, AlphaStateAttr, BlendFunctionAttr, BlendStateAttr, CullFaceAttr,
    DepthWriteStateAttr, DestinationAlphaAttr, Hero, Layout, MaterialAnim, MaterialAnimTrackList,
    MaterialUvAnimTrack, ObjectRef, PhysicsModel, PixelShaderVariantMaskAttr, ScrollUvAnimData,
    ShaderConstantIntAttr, ShaderConstantVec4fAttr, SpriteInfo, VertexShaderVariantMaskAttr, is_a,
    read_all,
};
use guac_formats::material::Material;

fn level_bld(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .unwrap_or_else(|| panic!("no level.bld in {}", path.display()));
    archive.read(entry).unwrap().into_owned()
}

fn blds(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bld"))
        .collect();
    paths.sort();
    paths
}

#[derive(Default)]
struct Tally {
    read: BTreeMap<&'static str, usize>,
    failures: Vec<String>,
    materials: usize,
    refusals: BTreeMap<String, usize>,
    heroes: usize,
}

impl Tally {
    fn all<'a, T: Layout<'a>>(&mut self, name: &str, igz: &Igz<'a>, ty: &'static str) {
        for (i, r) in read_all::<T>(igz).filter(|(i, _)| igz.type_name(&igz.objects()[*i]) == ty) {
            match r {
                Ok(_) => *self.read.entry(ty).or_default() += 1,
                Err(e) => self.failures.push(format!("{name}: {ty} #{i}: {e}")),
            }
        }
    }

    fn take(&mut self, name: &str, bytes: &[u8]) {
        let igz = match Igz::parse(bytes) {
            Ok(i) => i,
            Err(e) => {
                self.failures.push(format!("{name}: {e}"));
                return;
            }
        };
        self.all::<PixelShaderVariantMaskAttr>(name, &igz, "igPixelShaderVariantMaskAttr");
        self.all::<VertexShaderVariantMaskAttr>(name, &igz, "igVertexShaderVariantMaskAttr");
        for ty in [
            "tfbMulti2FixedAlphaValuesAttr",
            "tfbMulti2Layer1ExtraValuesAttr",
            "tfbMulti2Layer2ExtraValuesAttr",
            "tfbMulti2Layer2BlendFactorsAttr",
            "tfbMulti2Layer3BlendFactorsAttr",
            "tfbMulti2Layer4BlendFactorsAttr",
            "tfbMulti2Layer4ValuesAttr",
        ] {
            self.all::<ShaderConstantVec4fAttr>(name, &igz, ty);
        }
        self.all::<ShaderConstantIntAttr>(name, &igz, "tfbMaterialTintAttr");
        self.all::<BlendStateAttr>(name, &igz, "igBlendStateAttr");
        self.all::<BlendFunctionAttr>(name, &igz, "igBlendFunctionAttr");
        self.all::<DepthWriteStateAttr>(name, &igz, "igDepthWriteStateAttr");
        self.all::<CullFaceAttr>(name, &igz, "igCullFaceAttr");
        self.all::<AlphaStateAttr>(name, &igz, "igAlphaStateAttr");
        self.all::<AlphaFunctionAttr>(name, &igz, "igAlphaFunctionAttr");
        self.all::<DestinationAlphaAttr>(name, &igz, "tfbDestinationAlphaAttr");
        self.all::<MaterialAnim>(name, &igz, "tfbMaterialAnim");
        self.all::<MaterialAnimTrackList>(name, &igz, "tfbMaterialAnimTrackList");
        for ty in [
            "tfbScrollUVAnimTrack",
            "tfbTransformUVAnimTrack",
            "tfbFlipBookUVAnimTrack",
        ] {
            self.all::<MaterialUvAnimTrack>(name, &igz, ty);
        }
        self.all::<ScrollUvAnimData>(name, &igz, "tfbScrollUVAnimData");

        // Every sprite-drawn model with a skeleton: its materials.
        for (_, s) in read_all::<SpriteInfo>(&igz) {
            let Ok(s) = s else { continue };
            let ObjectRef::Object(m) = s.context_data_info else {
                continue;
            };
            if !is_a(igz.type_name(&igz.objects()[m]), PhysicsModel::TYPE) {
                continue;
            }
            let Ok(hero) = Hero::read(&igz, m) else {
                continue;
            };
            self.heroes += 1;
            for mesh in &hero.meshes {
                self.materials += 1;
                if let Err(e) = Material::read(&igz, &hero, mesh) {
                    *self.refusals.entry(reason(&e)).or_default() += 1;
                }
            }
        }
    }
}

fn reason(e: &FormatError) -> String {
    format!("{}: {}", e.field, e.kind)
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_material_attribute_reads_by_its_layout() {
    let usrdir = usrdir();
    let started = Instant::now();
    let mut tally = Tally::default();
    let mut files = 0;
    for dir in ["character", "item"] {
        for path in blds(&usrdir.join(dir)) {
            let name = format!("{dir}/{}", path.file_name().unwrap().to_string_lossy());
            tally.take(&name, &level_bld(&path));
            files += 1;
        }
    }
    println!(
        "{files} character and item level.bld files in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    for (ty, n) in &tally.read {
        println!("  {ty}: {n} read");
    }
    println!(
        "  {} sprite-drawn models with a skeleton, {} techniques; the tfbmulti2 reader refused {}:",
        tally.heroes,
        tally.materials,
        tally.refusals.values().sum::<usize>()
    );
    for (r, n) in &tally.refusals {
        println!("    {n} x {r}");
    }
    println!("  layout failures: {}", tally.failures.len());
    for f in tally.failures.iter().take(40) {
        println!("    {f}");
    }
    assert!(files > 0 && tally.materials > 0);
    assert!(tally.failures.is_empty());
}

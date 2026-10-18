//! guacamole's dump scan from a terminal, until the binary's `--setup` lands
//! (AA.6). Reads only each dump's `PS3_GAME/PARAM.SFO`.
//!
//! - `scan_dumps <folder>...` scans the folders and stores nothing.
//! - `scan_dumps --store <folder>...` stores them as guacamole's dumps folders.
//! - `scan_dumps` scans whatever guacamole would load (stored or `GUAC_DUMP_DIRS`).

use std::path::PathBuf;

use anyhow::Result;
use guac_server::{dumps, paths, setup};

fn main() -> Result<()> {
    let mut args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let store = args.first().is_some_and(|a| a.as_os_str() == "--store");
    if store {
        args.remove(0);
    }
    let root = paths::guac_data_root()?;

    let scan = if store {
        let scan = setup::Setup::open(&root)?.set_dump_dirs(&args)?;
        println!("stored under {}", root.display());
        scan
    } else if args.is_empty() {
        let loaded = setup::load(&root)?;
        println!("data root: {}", root.display());
        println!("needs setup: {}", loaded.needs_setup());
        for dir in &loaded.dump_dirs {
            println!("dumps folder: {}", dir.display());
        }
        loaded.scan()
    } else {
        dumps::scan(&args)
    };

    for dump in &scan.found {
        let name = dump.display_name().unwrap_or("(not a Skylanders title)");
        println!("{}  {name}  {}", dump.title_id, dump.dir.display());
    }
    for sfo in &scan.unreadable {
        println!("unreadable  {}", sfo.display());
    }
    for dir in &scan.unlisted {
        println!("can't list  {}", dir.display());
    }
    match scan.offered() {
        Some(giants) => println!("offered: {}", giants.title_id),
        None => println!("offered: nothing (no {} dump)", dumps::GIANTS_TITLE_ID),
    }
    Ok(())
}

//! AG.221's `GUAC_TEST_UNMODEL`: one registered member taken out of the TV's
//! run, so a walker whose minion reaches it stops in play and falls back.
//! Since AG.160 nothing on the dump stops in play otherwise. `test-hooks`
//! builds and tests only: the module and the guac-script switch it stands on
//! (`Members::without`, `test-fixtures`) are not in any other build.

use bevy::prelude::Resource;

/// The process key (`test-hooks` builds only, like
/// `GUAC_TEST_POSTED_PADS_ONLY`) naming one member to take out of the TV's
/// run: `namespace.name`, e.g. `collisioninfo.object collisions`. A script
/// reaching it stops as on a member never registered, so a walker whose
/// minion asks it falls back at its stop with no change to the product's
/// listing (openspec AG.221): since AG.160 nothing on the dump stops in play
/// otherwise.
#[cfg(feature = "unmodel")]
pub const UNMODEL_KEY: &str = "GUAC_TEST_UNMODEL";

/// The member [`UNMODEL_KEY`] named, as given: [`install_from`] takes it out
/// of every run it installs, or refuses it with an error logged and the run
/// unchanged ([`take_out`]). Inserted by `main` only in a `test-hooks`
/// build with the key set.
#[cfg(feature = "unmodel")]
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct Unmodel(pub String);

/// [`UNMODEL_KEY`] as `read` finds it: always `None` without `test-hooks`,
/// whatever `read` answers, so a test build (`unmodel` on, `test-hooks`
/// off) can't be told to either; a build without `unmodel` has no module.
#[cfg(feature = "unmodel")]
pub fn unmodel_request(read: impl FnOnce(&str) -> Option<String>) -> Option<Unmodel> {
    requested(cfg!(feature = "test-hooks"), read)
}

/// [`unmodel_request`] with the build's `test-hooks` given, so its tests
/// read the key in every build, not only under `test-hooks`.
#[cfg(feature = "unmodel")]
fn requested(test_hooks: bool, read: impl FnOnce(&str) -> Option<String>) -> Option<Unmodel> {
    if !test_hooks {
        return None;
    }
    read(UNMODEL_KEY).map(Unmodel)
}

/// `request` (`namespace.name`, split at the first dot) taken out of
/// `config`'s members through [`guac_script::Members::without`]; the member's
/// name as the registry spells it. Refused, `config` unchanged, when the name
/// is empty, lacks either half or isn't a registered member.
#[cfg(feature = "unmodel")]
pub fn take_out(config: &mut guac_script::Config, request: &str) -> Result<String, String> {
    let request = request.trim();
    let Some((namespace, name)) = request.split_once('.') else {
        return Err(format!("{request:?} is not `namespace.name`"));
    };
    if namespace.is_empty() || name.is_empty() {
        return Err(format!("{request:?} is not `namespace.name`"));
    }
    // `without` forgets the name even of a member that isn't registered (one
    // the VM keeps itself is only a name), so a refusal looks first.
    if config
        .members
        .get(guac_script::MemberId::named(namespace, name))
        .is_none()
    {
        return Err(format!("{request:?} is not a registered member"));
    }
    config.members.without(namespace, name);
    Ok(format!(
        "{}.{}",
        namespace.to_ascii_lowercase(),
        name.to_ascii_lowercase()
    ))
}

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;
    use crate::scripts::{self, smash_config};

    /// AG.221: the key is read by name, and only in a `test-hooks` build:
    /// whatever the process env holds, any other build asks for nothing.
    /// The read is held in every build through [`requested`], so a key
    /// renamed or never read fails `cargo test --workspace` too, which runs
    /// without `test-hooks`.
    #[test]
    fn the_unmodel_key_is_read_in_test_hooks_builds_only() {
        let named = |key: &str| {
            assert_eq!(key, "GUAC_TEST_UNMODEL");
            Some("collisioninfo.object collisions".to_owned())
        };
        let wanted = Some(Unmodel("collisioninfo.object collisions".into()));
        assert_eq!(requested(true, named), wanted);
        assert_eq!(requested(true, |_| None), None, "unset asks for nothing");
        assert_eq!(
            requested(false, |_| panic!("read without test-hooks")),
            None
        );
        // `cargo test --workspace` lands on `false`; the `test-hooks` run on `true`.
        match cfg!(feature = "test-hooks") {
            true => assert_eq!(unmodel_request(named), wanted),
            false => assert_eq!(unmodel_request(named), None),
        }
    }

    /// AG.221: a registered member is taken out of the run's registry and
    /// named in lower case; an unknown name, an empty one, a name missing a
    /// half and one the VM only keeps by name are refused with the registry
    /// as it was (eight refusals); a second member named is the one taken
    /// out.
    #[test]
    fn the_key_takes_a_registered_member_out_and_refuses_anything_else() {
        let id = guac_script::MemberId::named("collisioninfo", "object collisions");
        let mut config = scripts::with_walkers(smash_config(Mode::Play), &[]);
        let registered = config.members.len();
        assert!(config.members.get(id).is_some(), "modelled since AG.160");
        for refused in [
            "",
            "   ",
            "collisioninfo",
            ".object collisions",
            "collisioninfo.",
            "collisioninfo.no such member",
            "nosuchnamespace.object collisions",
            // Known by name only: the VM keeps it, the registry holds no entry.
            "slider.ignores time scales",
        ] {
            assert!(take_out(&mut config, refused).is_err(), "{refused:?}");
            assert_eq!(config.members.len(), registered, "{refused:?}");
            assert!(config.members.get(id).is_some(), "{refused:?}");
        }
        let vm_kept = guac_script::MemberId::named("slider", "ignores time scales");
        assert_eq!(
            config.members.name(vm_kept),
            "slider.ignores time scales",
            "a refusal leaves even the name"
        );
        assert_eq!(
            take_out(&mut config, " CollisionInfo.Object Collisions ").as_deref(),
            Ok("collisioninfo.object collisions")
        );
        assert_eq!(config.members.len(), registered - 1);
        assert!(config.members.get(id).is_none());
        // Once out it isn't registered, so it isn't taken out twice.
        assert!(take_out(&mut config, "collisioninfo.object collisions").is_err());
        // Another member is the one taken out when named, and only it.
        let other =
            guac_script::MemberId::named("redoctaneperipheralscriptobject", "size of region 1");
        assert!(config.members.get(other).is_some());
        assert_eq!(
            take_out(
                &mut config,
                "redoctaneperipheralscriptobject.size of region 1"
            )
            .as_deref(),
            Ok("redoctaneperipheralscriptobject.size of region 1")
        );
        assert!(config.members.get(other).is_none());
        assert_eq!(config.members.len(), registered - 2);
    }
}

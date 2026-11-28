//! Complete the source's resource-preparation phase on an authored world.
use super::*;
pub fn prepare(
    world: &mut crate::vehicle_runtime::SinglePlayer,
    loaded: &mut Loaded,
    data: &[u8],
    level_archive: &[u8],
    shared: &[u8],
    cup: usize,
) -> Result<(), Box<dyn Error>> {
    let seed = world.shared.random;
    world.registry.enable_projectiles();
    let ufo_class = world.registry.register_ufo();
    world.objects.world.classes = world.registry.classes.clone();
    let catalog_archive = Archive::parse(data)?;
    let catalog = Catalog::decode_checked(
        catalog_archive
            .get("OBPARAMS.DAT")
            .ok_or("Missing object settings")?,
    )
    .map_err(|e| format!("Object settings: {e:?}"))?;
    let terrain = world
        .objects
        .components
        .terrain
        .clone()
        .ok_or("Missing terrain")?;
    let source = Mounted {
        shapes: Bundles {
            vehicle: vec![],
            shared: vec![shapes(shared)?],
            level: vec![shapes(level_archive)?],
        },
        level: Archive::parse(level_archive)?,
        shared: Archive::parse(shared)?,
    };
    let mut materials = MaterialFiles {
        level: Archive::parse(level_archive)?,
        shared: Archive::parse(shared)?,
        bindings: loaded.materials.clone(),
    };
    let mut loader = Loader::new(
        &mut world.objects,
        scene_loading::Resources {
            source: &source,
            models: &mut loaded.models,
            terrain: &terrain,
            random: &mut world.shared.random,
            catalog: &catalog,
            materials: &mut materials,
            matrix_tail: Some([0; 3]),
        },
        &world.registry,
    );
    loader
        .prepare_projectiles(cup)
        .map_err(|e| format!("Projectile preparation: {e:?}"))?;
    loader
        .prepare_ufos(ufo_class)
        .map_err(|e| format!("UFO preparation: {e:?}"))?;
    loaded.materials = materials.bindings;
    assert_eq!(
        world.shared.random, seed,
        "resource preparation must not consume game RNG"
    );
    Ok(())
}

//! External model reads and successful device feedback for original class calls.
use std::sync::Arc;
use tankracer_assets::model::Model;
use tankracer_game::{obstacle_collision, scene_objects, vehicle_runtime};

pub struct Devices<'a> {
    pub models: &'a [(String, Arc<Model>)],
}
impl scene_objects::collision::Services for Devices<'_> {
    fn auxiliary_model(&mut self, name: &str) -> Result<Option<Arc<Model>>, i32> {
        for (key, model) in self.models {
            if key.eq_ignore_ascii_case(name) {
                return Ok(Some(Arc::clone(model)));
            }
        }
        panic!("missing authored auxiliary model: {name}")
    }
    fn sound(
        &mut self,
        _: scene_objects::collision::Sound,
        _: &mut scene_objects::collision::Collision<'_>,
        _: obstacle_collision::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut scene_objects::collision::Collision<'_>,
        _: obstacle_collision::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
}
impl vehicle_runtime::Devices for Devices<'_> {
    fn surface_feedback(&mut self) -> Result<(), i32> {
        Ok(())
    }
    fn impact_feedback(&mut self, _: i32) -> Result<(), i32> {
        Ok(())
    }
}

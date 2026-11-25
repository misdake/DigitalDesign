//! Pure source-world matrix preparation, usable at compile time or runtime.
use crate::dsl_rt::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_transforms;
use gpu_scene_transforms::*;
mod gpu_value_constructors;
use gpu_value_constructors::*;

const fn transform_rotation(angles: Vec3) -> Mat3x3 {
    let x = fsincos(angles.x);
    let y = fsincos(angles.y);
    let z = fsincos(angles.z);
    let sx = x.x();
    let cx = x.y();
    let sy = y.x();
    let cy = y.y();
    let sz = z.x();
    let cz = z.y();
    Mat3x3::from_rows(
        Vec3::new(
            cz.wrapping_mul(cy),
            cz.wrapping_mul(sy)
                .wrapping_mul(sx)
                .wrapping_sub(sz.wrapping_mul(cx)),
            cz.wrapping_mul(sy)
                .wrapping_mul(cx)
                .wrapping_add(sz.wrapping_mul(sx)),
        ),
        Vec3::new(
            sz.wrapping_mul(cy),
            sz.wrapping_mul(sy)
                .wrapping_mul(sx)
                .wrapping_add(cz.wrapping_mul(cx)),
            sz.wrapping_mul(sy)
                .wrapping_mul(cx)
                .wrapping_sub(cz.wrapping_mul(sx)),
        ),
        Vec3::new(sy.wrapping_neg(), cy.wrapping_mul(sx), cy.wrapping_mul(cx)),
    )
}
impl Pose {
    /// Rz * Ry * Rx with scale applied to columns, then source-world translation.
    pub const fn world_matrix(self) -> Mat3x4 {
        let r = transform_rotation(self.angles);
        Mat3x4::from_rows(
            Vec4::new(
                r.row0.x.wrapping_mul(self.scale.x),
                r.row0.y.wrapping_mul(self.scale.y),
                r.row0.z.wrapping_mul(self.scale.z),
                self.position.x,
            ),
            Vec4::new(
                r.row1.x.wrapping_mul(self.scale.x),
                r.row1.y.wrapping_mul(self.scale.y),
                r.row1.z.wrapping_mul(self.scale.z),
                self.position.y,
            ),
            Vec4::new(
                r.row2.x.wrapping_mul(self.scale.x),
                r.row2.y.wrapping_mul(self.scale.y),
                r.row2.z.wrapping_mul(self.scale.z),
                self.position.z,
            ),
        )
    }
    /// Inverse-transpose of TRS's linear part. Singular scale has no normal matrix.
    pub const fn normal_matrix(self) -> Mat3x3 {
        assert!(
            !self.scale.x.equals(fix32::zero())
                && !self.scale.y.equals(fix32::zero())
                && !self.scale.z.equals(fix32::zero())
        );
        let r = transform_rotation(self.angles);
        let ix = frcp(self.scale.x);
        let iy = frcp(self.scale.y);
        let iz = frcp(self.scale.z);
        Mat3x3::from_rows(
            Vec3::new(
                r.row0.x.wrapping_mul(ix),
                r.row0.y.wrapping_mul(iy),
                r.row0.z.wrapping_mul(iz),
            ),
            Vec3::new(
                r.row1.x.wrapping_mul(ix),
                r.row1.y.wrapping_mul(iy),
                r.row1.z.wrapping_mul(iz),
            ),
            Vec3::new(
                r.row2.x.wrapping_mul(ix),
                r.row2.y.wrapping_mul(iy),
                r.row2.z.wrapping_mul(iz),
            ),
        )
    }
}

use glam::{Mat4, Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub fov: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self::iso(Vec3::ZERO, 120.0)
    }
}

impl Camera {
    pub fn iso(target: Vec3, dist: f32) -> Self {
        Self { target, yaw: -PI / 4.0, pitch: 0.6155, dist, fov: 35f32.to_radians() }
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + self.dist * Vec3::new(cp * cy, cp * sy, sp)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Z);
        let proj = glam::camera::rh::proj::directx::perspective(self.fov, aspect, self.dist * 0.02, self.dist * 100.0);
        proj * view
    }

    /// World-space ray through a point in normalized device coords.
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect).inverse();
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    /// World point to NDC; None if behind the camera.
    pub fn project(&self, p: Vec3, aspect: f32) -> Option<Vec2> {
        let c = self.view_proj(aspect) * p.extend(1.0);
        (c.w > 1e-6).then(|| Vec2::new(c.x, c.y) / c.w)
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-FRAC_PI_2 + 1e-3, FRAC_PI_2 - 1e-3);
    }

    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let fwd = (self.target - self.eye()).normalize();
        let right = fwd.cross(Vec3::Z).normalize_or(Vec3::X);
        (right, right.cross(fwd), fwd)
    }

    pub fn key_light(&self) -> Vec3 {
        let (right, up, fwd) = self.basis();
        (-fwd * 0.5 + up * 0.7 - right * 0.6).normalize()
    }

    pub fn pan(&mut self, dx: f32, dy: f32, viewport_h: f32) {
        let (right, up, _) = self.basis();
        let k = 2.0 * self.dist * (self.fov * 0.5).tan() / viewport_h.max(1.0);
        self.target += (-right * dx + up * dy) * k;
    }

    pub fn zoom(&mut self, amount: f32) {
        self.dist = (self.dist * (-amount * 0.002).exp()).clamp(0.1, 1e5);
    }

    pub fn fit(&self, lo: Vec3, hi: Vec3) -> Self {
        let r = ((hi - lo).length() * 0.5).max(1.0);
        Self { target: (lo + hi) * 0.5, dist: r / (self.fov * 0.5).sin() * 1.15, ..*self }
    }

    pub fn looking(&self, yaw: f32, pitch: f32) -> Self {
        Self { yaw, pitch, ..*self }
    }

    pub fn approach(&mut self, goal: &Camera, t: f32) -> bool {
        let dyaw = (goal.yaw - self.yaw + PI).rem_euclid(2.0 * PI) - PI;
        self.yaw += dyaw * t;
        self.pitch += (goal.pitch - self.pitch) * t;
        self.dist += (goal.dist - self.dist) * t;
        self.target = self.target.lerp(goal.target, t);
        let done = dyaw.abs() < 1e-3 && (goal.pitch - self.pitch).abs() < 1e-3 && (goal.dist / self.dist - 1.0).abs() < 1e-3;
        if done {
            *self = *goal;
        }
        !done
    }
}

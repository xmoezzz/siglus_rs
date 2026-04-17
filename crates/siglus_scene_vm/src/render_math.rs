use crate::layer::Sprite;

#[derive(Debug, Clone, Copy)]
pub struct ProjectedPoint {
    pub x: f32,
    pub y: f32,
    pub depth: f32,
}

#[derive(Debug, Clone, Copy)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }

    fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }

    fn normalize(self) -> Self {
        let len2 = self.dot(self);
        if len2 <= f32::EPSILON {
            return Self::new(0.0, 0.0, 0.0);
        }
        let inv = len2.sqrt().recip();
        Self::new(self.x * inv, self.y * inv, self.z * inv)
    }
}

fn rotate_x(v: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    Vec3::new(v.x, v.y * c - v.z * s, v.y * s + v.z * c)
}

fn rotate_y(v: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

fn rotate_z(v: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    Vec3::new(v.x * c - v.y * s, v.x * s + v.y * c, v.z)
}

fn uses_3d(sprite: &Sprite) -> bool {
    sprite.billboard
        || sprite.camera_enabled
        || sprite.z.abs() > f32::EPSILON
        || sprite.pivot_z.abs() > f32::EPSILON
        || (sprite.scale_z - 1.0).abs() > 1e-6
        || sprite.rotate_x.abs() > f32::EPSILON
        || sprite.rotate_y.abs() > f32::EPSILON
}

fn transform_local_point(sprite: &Sprite, px: f32, py: f32, dst_x: f32, dst_y: f32) -> Vec3 {
    let mut p = Vec3::new(px - sprite.pivot_x, py - sprite.pivot_y, -sprite.pivot_z);
    p.x *= sprite.scale_x;
    p.y *= sprite.scale_y;
    p.z *= sprite.scale_z;
    p = rotate_x(p, sprite.rotate_x);
    p = rotate_y(p, sprite.rotate_y);
    p = rotate_z(p, sprite.rotate);
    p.add(Vec3::new(
        dst_x + sprite.pivot_x,
        dst_y + sprite.pivot_y,
        sprite.z + sprite.pivot_z,
    ))
}

fn camera_basis(sprite: &Sprite) -> (Vec3, Vec3, Vec3, Vec3) {
    let eye = Vec3::new(
        sprite.camera_eye[0],
        sprite.camera_eye[1],
        sprite.camera_eye[2],
    );
    let target = Vec3::new(
        sprite.camera_target[0],
        sprite.camera_target[1],
        sprite.camera_target[2],
    );
    let up = Vec3::new(
        sprite.camera_up[0],
        sprite.camera_up[1],
        sprite.camera_up[2],
    );
    let forward = target.sub(eye).normalize();
    let right = up.cross(forward).normalize();
    let up2 = forward.cross(right).normalize();
    (eye, forward, right, up2)
}

fn transform_billboard_point(sprite: &Sprite, px: f32, py: f32, dst_x: f32, dst_y: f32) -> Vec3 {
    let (_, _, right, up) = camera_basis(sprite);
    let lx = (px - sprite.pivot_x) * sprite.scale_x;
    let ly = (py - sprite.pivot_y) * sprite.scale_y;
    let (s, c) = sprite.rotate.sin_cos();
    let rx = lx * c - ly * s;
    let ry = lx * s + ly * c;
    let anchor = Vec3::new(
        dst_x + sprite.pivot_x,
        dst_y + sprite.pivot_y,
        sprite.z + sprite.pivot_z,
    );
    anchor.add(Vec3::new(
        right.x * rx + up.x * ry,
        right.y * rx + up.y * ry,
        right.z * rx + up.z * ry,
    ))
}

fn project_point(sprite: &Sprite, p: Vec3, win_w: f32, win_h: f32) -> Option<ProjectedPoint> {
    if !sprite.camera_enabled {
        let depth = (0.5 - p.z / 100000.0).clamp(0.0, 1.0);
        return Some(ProjectedPoint {
            x: p.x,
            y: p.y,
            depth,
        });
    }

    let (eye, zaxis, xaxis, yaxis) = camera_basis(sprite);

    let rel = p.sub(eye);
    let cx = rel.dot(xaxis);
    let cy = rel.dot(yaxis);
    let cz = rel.dot(zaxis);
    if cz <= 1e-3 {
        return None;
    }

    let aspect = if win_h.abs() > f32::EPSILON {
        win_w / win_h
    } else {
        1.0
    };
    let hfov = sprite
        .camera_view_angle_deg
        .to_radians()
        .clamp(1e-3, std::f32::consts::PI - 1e-3);
    let tan_half_h = (hfov * 0.5).tan().max(1e-3);
    let tan_half_v = (tan_half_h / aspect.max(1e-3)).max(1e-3);

    let x_ndc = cx / (cz * tan_half_h);
    let y_ndc = cy / (cz * tan_half_v);

    let sx = (x_ndc + 1.0) * 0.5 * win_w;
    let sy = (1.0 - y_ndc) * 0.5 * win_h;
    let depth = ((cz - 1.0) / 20000.0).clamp(0.0, 1.0);
    Some(ProjectedPoint {
        x: sx,
        y: sy,
        depth,
    })
}

fn signed_area(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

pub fn sprite_quad_points(
    sprite: &Sprite,
    dst_x: f32,
    dst_y: f32,
    dst_w: f32,
    dst_h: f32,
    win_w: f32,
    win_h: f32,
) -> Option<[ProjectedPoint; 4]> {
    if !uses_3d(sprite) {
        let p0 = transform_local_point(sprite, 0.0, 0.0, dst_x, dst_y);
        let p1 = transform_local_point(sprite, dst_w, 0.0, dst_x, dst_y);
        let p2 = transform_local_point(sprite, dst_w, dst_h, dst_x, dst_y);
        let p3 = transform_local_point(sprite, 0.0, dst_h, dst_x, dst_y);
        return Some([
            ProjectedPoint {
                x: p0.x,
                y: p0.y,
                depth: 0.0,
            },
            ProjectedPoint {
                x: p1.x,
                y: p1.y,
                depth: 0.0,
            },
            ProjectedPoint {
                x: p2.x,
                y: p2.y,
                depth: 0.0,
            },
            ProjectedPoint {
                x: p3.x,
                y: p3.y,
                depth: 0.0,
            },
        ]);
    }

    let xf = if sprite.billboard {
        transform_billboard_point as fn(&Sprite, f32, f32, f32, f32) -> Vec3
    } else {
        transform_local_point as fn(&Sprite, f32, f32, f32, f32) -> Vec3
    };

    let p0 = project_point(sprite, xf(sprite, 0.0, 0.0, dst_x, dst_y), win_w, win_h)?;
    let p1 = project_point(sprite, xf(sprite, dst_w, 0.0, dst_x, dst_y), win_w, win_h)?;
    let p2 = project_point(sprite, xf(sprite, dst_w, dst_h, dst_x, dst_y), win_w, win_h)?;
    let p3 = project_point(sprite, xf(sprite, 0.0, dst_h, dst_x, dst_y), win_w, win_h)?;

    if sprite.culling && signed_area((p0.x, p0.y), (p1.x, p1.y), (p2.x, p2.y)) <= 0.0 {
        return None;
    }

    Some([p0, p1, p2, p3])
}

pub fn project_model_point(
    sprite: &Sprite,
    local: [f32; 3],
    anchor_x: f32,
    anchor_y: f32,
    win_w: f32,
    win_h: f32,
) -> Option<ProjectedPoint> {
    let mut p = Vec3::new(
        local[0] - sprite.pivot_x,
        local[1] - sprite.pivot_y,
        local[2] - sprite.pivot_z,
    );
    p.x *= sprite.scale_x;
    p.y *= sprite.scale_y;
    p.z *= sprite.scale_z;
    p = rotate_x(p, sprite.rotate_x);
    p = rotate_y(p, sprite.rotate_y);
    p = rotate_z(p, sprite.rotate);
    p = p.add(Vec3::new(
        anchor_x + sprite.pivot_x,
        anchor_y + sprite.pivot_y,
        sprite.z + sprite.pivot_z,
    ));
    project_point(sprite, p, win_w, win_h)
}

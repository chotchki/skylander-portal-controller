//! Havok's qs transforms (translation, quaternion, per-axis scale) in f64.

/// x, y, z, w.
pub type Quat = [f64; 4];

pub const IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

pub fn mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// `v` rotated by a unit quaternion.
pub fn rotate(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let c = [
        y * v[2] - z * v[1],
        z * v[0] - x * v[2],
        x * v[1] - y * v[0],
    ];
    let cc = [
        y * c[2] - z * c[1],
        z * c[0] - x * c[2],
        x * c[1] - y * c[0],
    ];
    [
        v[0] + 2.0 * (w * c[0] + cc[0]),
        v[1] + 2.0 * (w * c[1] + cc[1]),
        v[2] + 2.0 * (w * c[2] + cc[2]),
    ]
}

pub fn length(q: Quat) -> f64 {
    q.iter().map(|c| c * c).sum::<f64>().sqrt()
}

/// Unit length; a zero quaternion becomes the identity.
pub fn normalize(q: Quat) -> Quat {
    let n = length(q);
    if n > 0.0 && n.is_finite() {
        q.map(|c| c / n)
    } else {
        IDENTITY
    }
}

/// The angle between two unit quaternions' rotations, in radians.
pub fn angle(a: Quat, b: Quat) -> f64 {
    let d: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    2.0 * d.abs().min(1.0).acos()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QsTransform {
    pub translation: [f64; 3],
    pub rotation: Quat,
    pub scale: [f64; 3],
}

impl Default for QsTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl QsTransform {
    pub const IDENTITY: Self = Self {
        translation: [0.0; 3],
        rotation: IDENTITY,
        scale: [1.0; 3],
    };

    /// Parent (model space) times child (local), Havok's way:
    /// `t = tp + rp (sp t)`, `r = rp r`, `s = sp s`.
    pub fn mul(&self, child: &Self) -> Self {
        let st = [0, 1, 2].map(|i| self.scale[i] * child.translation[i]);
        let rt = rotate(self.rotation, st);
        Self {
            translation: [0, 1, 2].map(|i| self.translation[i] + rt[i]),
            rotation: mul(self.rotation, child.rotation),
            scale: [0, 1, 2].map(|i| self.scale[i] * child.scale[i]),
        }
    }

    /// Column-vector `T * R * S` as rows; the rotation is normalized first.
    pub fn matrix(&self) -> [[f64; 4]; 4] {
        let [x, y, z, w] = normalize(self.rotation);
        let r = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        let row = |i: usize| {
            [
                r[i][0] * self.scale[0],
                r[i][1] * self.scale[1],
                r[i][2] * self.scale[2],
                self.translation[i],
            ]
        };
        [row(0), row(1), row(2), [0.0, 0.0, 0.0, 1.0]]
    }
}

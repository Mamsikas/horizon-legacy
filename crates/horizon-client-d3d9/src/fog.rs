//! Two independent vanilla fog curves, using only ps_2_0. The fixed-function
//! vertex pipeline supplies camera-relative world position as TEXCOORD2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub color: [f32; 4],
    pub environment: [f32; 2],
    pub distance: [f32; 2],
}
/// Equivalent programs selected only when section bounds prove a curve inactive
/// or the combined fog fully opaque. Unknown/nonfinite bounds keep both curves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum FogProgram {
    #[default]
    Dual,
    Environment,
    Distance,
    Unfogged,
    FullyFogged,
}
impl FogProgram {
    pub fn position(self) -> bool {
        matches!(self, Self::Dual | Self::Environment | Self::Distance)
    }
}
impl Fog {
    pub fn program(self, bounds: [[f32; 3]; 2]) -> FogProgram {
        use FogProgram::*;
        if bounds.iter().flatten().any(|v| !v.is_finite())
            || (0..3).any(|i| bounds[0][i] > bounds[1][i])
            || self
                .color
                .iter()
                .chain(&self.environment)
                .chain(&self.distance)
                .any(|v| !v.is_finite())
        {
            return Dual;
        }
        if self.color[3] == 0.0 {
            return Unfogged;
        }
        // Account for interpolation/matrix rounding at a curve boundary. This
        // is conservative: it can retain work, never omit a possibly active ramp.
        let lo = bounds[0].map(|v| v - 1.0 / 1024.0);
        let hi = bounds[1].map(|v| v + 1.0 / 1024.0);
        let far = std::array::from_fn::<_, 3, _>(|i| lo[i].abs().max(hi[i].abs()));
        let near = std::array::from_fn::<_, 3, _>(|i| lo[i].max(-hi[i]).max(0.0));
        let lengths = |d: [f32; 3]| {
            let horizontal = (d[0] * d[0] + d[2] * d[2]).max(1.0e-20).sqrt();
            [
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
                    .max(1.0e-20)
                    .sqrt(),
                horizontal.max(d[1]),
            ]
        };
        let min = lengths(near);
        let max = lengths(far);
        let constants = self.constants();
        if self.color[3] == 1.0
            && (0..2).any(|i| (min[i] - constants[0][i]) * constants[1][i] >= 1.0)
        {
            FullyFogged
        } else {
            match (max[0] <= self.environment[0], max[1] <= self.distance[0]) {
                (true, true) => Unfogged,
                (false, true) => Environment,
                (true, false) => Distance,
                (false, false) => Dual,
            }
        }
    }
    pub fn constants(self) -> [[f32; 4]; 3] {
        [
            [self.environment[0], self.distance[0], 0.0, 0.0],
            [
                1.0 / (self.environment[1] - self.environment[0]).max(0.001),
                1.0 / (self.distance[1] - self.distance[0]).max(0.001),
                1.0e-20,
                0.0,
            ],
            self.color,
        ]
    }
}
// Compiled offline by the Windows SDK, including its SM2 validation. The
// workflow recompiles tools/d3d9/fog.hlsl and checks these executable words.
#[cfg(any(windows, test))]
#[path = "fog_distance_program.rs"]
mod distance;
#[cfg(any(windows, test))]
#[path = "fog_environment_program.rs"]
mod environment;
#[cfg(any(windows, test))]
#[path = "fog_full_program.rs"]
mod fully_fogged;
#[cfg(any(windows, test))]
#[path = "fog_program.rs"]
mod program;
#[cfg(any(windows, test))]
#[path = "fog_none_program.rs"]
mod unfogged;
#[cfg(any(windows, test))]
pub fn variant(mode: FogProgram) -> &'static [u32] {
    match mode {
        FogProgram::Dual => program::WORDS,
        FogProgram::Environment => environment::WORDS,
        FogProgram::Distance => distance::WORDS,
        FogProgram::Unfogged => unfogged::WORDS,
        FogProgram::FullyFogged => fully_fogged::WORDS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Execute the embedded arithmetic tokens, with two known texture samples.
    // This catches register masks, swizzles, saturation and alpha preservation
    // against numerical scene expectations rather than another shader builder.
    fn shade(position: [f32; 3], fog: Fog, mode: FogProgram) -> [f32; 4] {
        let code = variant(mode);
        assert_eq!(code[0], 0xffff_0200);
        let mut regs = [[[0.0; 4]; 16]; 11];
        regs[1][0] = [0.5, 0.75, 1.0, 0.8];
        regs[3][2] = [position[0], position[1], position[2], 1.0];
        regs[2][..3].copy_from_slice(&fog.constants());
        let kind = |token: u32| (((token >> 28) & 7) | ((token >> 8) & 24)) as usize;
        let mut cursor = 1;
        let mut arithmetic = 0;
        while code[cursor] != 0xffff {
            let opcode = code[cursor] & 0xffff;
            let length = ((code[cursor] >> 24) & 15) as usize;
            let args = &code[cursor + 1..cursor + 1 + length];
            cursor += 1 + length;
            if opcode == 31 {
                continue;
            }
            let read = |token: u32| {
                let v = regs[kind(token)][(token & 0x7ff) as usize];
                std::array::from_fn::<_, 4, _>(|i| {
                    let value = v[((token >> (16 + i * 2)) & 3) as usize];
                    if (token >> 24) & 15 == 1 {
                        -value
                    } else {
                        value
                    }
                })
            };
            let a = args.get(1).map_or([0.0; 4], |&t| read(t));
            let b = args.get(2).map_or([0.0; 4], |&t| read(t));
            let c = args.get(3).map_or([0.0; 4], |&t| read(t));
            let v = match opcode {
                1 => a,
                2 => std::array::from_fn(|i| a[i] + b[i]),
                4 => std::array::from_fn(|i| a[i] * b[i] + c[i]),
                5 => std::array::from_fn(|i| a[i] * b[i]),
                6 => [1.0 / a[0]; 4],
                7 => [1.0 / a[0].abs().sqrt(); 4],
                8 => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; 4],
                11 => std::array::from_fn(|i| a[i].max(b[i])),
                18 => std::array::from_fn(|i| a[i] * b[i] + (1.0 - a[i]) * c[i]),
                35 => a.map(f32::abs),
                66 => {
                    if args[2] & 0x7ff == 0 {
                        [0.8, 0.8, 0.8, 0.5]
                    } else {
                        [0.5; 4]
                    }
                }
                _ => panic!("instruction exceeds the tested SM2 subset: {opcode}"),
            };
            arithmetic += usize::from(opcode != 66);
            for i in 0..4 {
                if args[0] & (1 << (16 + i)) != 0 {
                    regs[kind(args[0])][(args[0] & 0x7ff) as usize][i] = if args[0] & (1 << 20) != 0
                    {
                        v[i].clamp(0.0, 1.0)
                    } else {
                        v[i]
                    };
                }
            }
        }
        assert!(arithmetic < 64);
        assert_eq!(cursor + 1, code.len());
        regs[8][0]
    }
    #[test]
    fn sm2_tokens_preserve_alpha_and_combine_sphere_cylinder_and_density() {
        for (position, environment, distance, alpha, expected_fog) in [
            ([0.0; 3], [0.0, 1000.0], [230.0, 256.0], 1.0, 0.0),
            (
                [48.0, 64.0, 0.0],
                [-8.0, 96.0],
                [230.0, 256.0],
                0.5,
                88.0 / 208.0,
            ),
            (
                [240.0, 0.0, 0.0],
                [0.0, 1.0e9],
                [230.0, 256.0],
                0.25,
                10.0 / 104.0,
            ),
            (
                [0.0, 240.0, 0.0],
                [0.0, 1.0e9],
                [230.0, 256.0],
                1.0,
                10.0 / 26.0,
            ),
            ([300.0, 0.0, 0.0], [0.0, 1000.0], [230.0, 256.0], 1.0, 1.0),
        ] {
            let output = shade(
                position,
                Fog {
                    color: [0.7, 0.8, 0.9, alpha],
                    environment,
                    distance,
                },
                FogProgram::Dual,
            );
            for i in 0..3 {
                let base = [0.2, 0.3, 0.4][i];
                let expected = base + expected_fog * ([0.7, 0.8, 0.9][i] - base);
                assert!(
                    (output[i] - expected).abs() < 1.0e-5,
                    "{position:?}: {output:?}"
                );
            }
            assert!((output[3] - 0.4).abs() < 1.0e-6);
        }
    }
    #[test]
    fn bounds_select_shorter_sm2_programs_without_changing_fog_or_alpha() {
        use FogProgram::*;
        let configurations = [
            (
                Fog {
                    color: [0.7, 0.8, 0.9, 1.0],
                    environment: [1.0e9, 2.0e9],
                    distance: [1.0e9, 2.0e9],
                },
                [[-1024.0; 3], [1024.0; 3]],
                Unfogged,
            ),
            (
                Fog {
                    color: [0.7, 0.8, 0.9, 1.0],
                    environment: [0.0, 1000.0],
                    distance: [24.0, 32.0],
                },
                [[1.0, -2.0, 1.0], [8.0, 4.0, 8.0]],
                Environment,
            ),
            (
                Fog {
                    color: [0.7, 0.8, 0.9, 1.0],
                    environment: [100.0, 1000.0],
                    distance: [24.0, 32.0],
                },
                [[25.0, -2.0, -3.0], [28.0, 4.0, 3.0]],
                Distance,
            ),
            (
                Fog {
                    color: [0.7, 0.8, 0.9, 1.0],
                    environment: [100.0, 1000.0],
                    distance: [24.0, 32.0],
                },
                [[1.0, -2.0, 1.0], [8.0, 4.0, 8.0]],
                Unfogged,
            ),
            (
                Fog {
                    color: [0.7, 0.8, 0.9, 1.0],
                    environment: [0.0, 1000.0],
                    distance: [24.0, 32.0],
                },
                [[34.0, -2.0, -3.0], [50.0, 4.0, 3.0]],
                FullyFogged,
            ),
            (
                Fog {
                    color: [0.7, 0.8, 0.9, 0.5],
                    environment: [0.0, 1000.0],
                    distance: [24.0, 32.0],
                },
                [[24.0, -2.0, -3.0], [50.0, 4.0, 3.0]],
                Dual,
            ),
        ];
        for (fog, bounds, expected) in configurations {
            assert_eq!(fog.program(bounds), expected);
            for x in 0..=4 {
                for y in 0..=4 {
                    for z in 0..=4 {
                        let at = [x, y, z].map(|v| v as f32 / 4.0);
                        let pos = std::array::from_fn(|i| {
                            bounds[0][i] + (bounds[1][i] - bounds[0][i]) * at[i]
                        });
                        let original = shade(pos, fog, Dual);
                        let selected = shade(pos, fog, expected);
                        for i in 0..4 {
                            assert!(
                                (original[i] - selected[i]).abs() < 1.0e-6,
                                "{expected:?}: {pos:?}: {original:?} != {selected:?}"
                            );
                        }
                    }
                }
            }
            if expected != Dual {
                assert!(variant(expected).len() < variant(Dual).len());
            }
        }
        // Boundary boxes retain both curves, including a vertical-distance-only
        // crossing and a box straddling the camera; nonfinite data stays general.
        let fog = configurations[1].0;
        assert_eq!(fog.program([[23.9999, 0.0, 0.0], [24.0, 0.0, 0.0]]), Dual);
        assert_eq!(fog.program([[0.0, 24.0, 0.0], [0.0, 30.0, 0.0]]), Dual);
        assert_eq!(fog.program([[-40.0; 3], [40.0; 3]]), Dual);
        assert_eq!(fog.program([[f32::NAN; 3], [1.0; 3]]), Dual);
    }
}

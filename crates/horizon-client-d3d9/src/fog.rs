//! Two independent vanilla fog curves, using only ps_2_0. The fixed-function
//! vertex pipeline supplies camera-relative world position as TEXCOORD2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub color: [f32; 4],
    pub environment: [f32; 2],
    pub distance: [f32; 2],
}
impl Fog {
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
#[path = "fog_program.rs"]
mod program;
#[cfg(any(windows, test))]
pub fn bytecode() -> &'static [u32] {
    program::WORDS
}

#[cfg(test)]
mod tests {
    use super::*;
    // Execute the embedded arithmetic tokens, with two known texture samples.
    // This catches register masks, swizzles, saturation and alpha preservation
    // against numerical scene expectations rather than another shader builder.
    fn shade(position: [f32; 3], fog: Fog) -> [f32; 4] {
        let code = bytecode();
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
}

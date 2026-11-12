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
#[cfg(any(windows, test))]
fn register(kind: u32, index: u32) -> u32 {
    0x8000_0000 | ((kind & 7) << 28) | ((kind & 24) << 8) | index
}
#[cfg(any(windows, test))]
fn dst(kind: u32, index: u32, mask: u32) -> u32 {
    register(kind, index) | (mask << 16)
}
#[cfg(any(windows, test))]
fn src(kind: u32, index: u32, swizzle: u32) -> u32 {
    register(kind, index) | (swizzle << 16)
}
/// Direct3D's documented shader token format, not HLSL compilation or a DLL
/// dependency. Kept as explicit SM2 instructions for profile/cap inspection.
#[cfg(any(windows, test))]
pub fn bytecode() -> Vec<u32> {
    const XYZW: u32 = 0xe4;
    const X: u32 = 0;
    const Y: u32 = 0x55;
    const Z: u32 = 0xaa;
    const W: u32 = 0xff;
    let r = |n, mask| dst(0, n, mask);
    let s = |n, swizzle| src(0, n, swizzle);
    let t = |n| src(3, n, XYZW);
    let c = |n, swizzle| src(2, n, swizzle);
    let mut out = vec![0xffff_0200]; // ps_2_0
    let mut op = |code, args: &[u32]| {
        out.push(code | ((args.len() as u32) << 24));
        out.extend_from_slice(args);
    };
    op(31, &[0x8000_0000, dst(1, 0, 15)]); // dcl v0
    for (n, mask) in [(0, 3), (1, 3), (2, 7)] {
        op(31, &[0x8000_0000, dst(3, n, mask)]); // dcl t#.xy[z]
    }
    for n in 0..2 {
        op(31, &[0x9000_0000, dst(10, n, 15)]); // dcl_2d s#
    }
    op(66, &[r(0, 15), t(0), src(10, 0, XYZW)]); // texld r0,t0,s0
    op(66, &[r(1, 15), t(1), src(10, 1, XYZW)]); // texld r1,t1,s1
    op(5, &[r(0, 15), s(0, XYZW), src(1, 0, XYZW)]); // mul r0,r0,v0
    op(5, &[r(0, 7), s(0, XYZW), s(1, XYZW)]); // mul r0.rgb,r0,r1
    op(8, &[r(2, 1), t(2), t(2)]); // dp3 r2.x,t2,t2
    op(11, &[r(2, 1), s(2, X), c(1, Z)]);
    op(7, &[r(2, 1), s(2, X)]);
    op(6, &[r(2, 1), s(2, X)]); // r2.x = length(position)
    op(5, &[r(3, 5), t(2), t(2)]);
    op(2, &[r(3, 1), s(3, X), s(3, Z)]);
    op(11, &[r(3, 1), s(3, X), c(1, Z)]);
    op(7, &[r(3, 1), s(3, X)]);
    op(6, &[r(3, 1), s(3, X)]);
    op(35, &[r(3, 2), src(3, 2, Y)]); // abs r3.y,t2.y
    op(11, &[r(2, 2), s(3, X), s(3, Y)]); // cylindrical distance
    op(2, &[r(2, 3), s(2, XYZW), c(0, XYZW) | (1 << 24)]); // -starts
    op(5, &[r(2, 3) | (1 << 20), s(2, XYZW), c(1, XYZW)]); // mul_sat
    op(11, &[r(2, 1), s(2, X), s(2, Y)]); // max of both curves
    op(5, &[r(2, 1), s(2, X), c(2, W)]); // fog colour alpha
    op(18, &[r(0, 7), s(2, X), c(2, XYZW), s(0, XYZW)]); // lrp RGB only
    op(1, &[dst(8, 0, 15), s(0, XYZW)]); // mov oC0,r0
    out.push(0x0000_ffff);
    out
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

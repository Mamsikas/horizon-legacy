//! Make same-material packets adjacent within an existing immutable arena.
//! Packet order within each material is the original section/vertex order.
use super::{Packet, dx};
use std::ops::Range;

// Bound the job metadata and sorting cost separately from the existing vertex cap.
const MAX_RANGES: usize = 512;

pub(super) struct Run {
    section: usize,
    base: u32,
    source: Range<u32>,
    material: Option<(usize, bool)>,
}

pub(super) fn plan<'a>(sections: impl Iterator<Item = (usize, &'a [Packet])>) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    let mut base = 0u32;
    for (section, (len, packets)) in sections.enumerate() {
        let len = u32::try_from(len).ok()?;
        if len % 4 != 0 || runs.len() + packets.len() + 1 > MAX_RANGES {
            return None;
        }
        runs.try_reserve(packets.len() + 1).ok()?;
        let mut start = 0;
        for packet in packets {
            let end = packet.first.checked_add(packet.count)?;
            if packet.first != start || end > len || packet.count == 0 || packet.count % 4 != 0 {
                return None;
            }
            runs.push(Run {
                section,
                base,
                source: start..end,
                material: Some((packet.page, packet.opaque)),
            });
            start = end;
        }
        if start < len {
            runs.push(Run {
                section,
                base,
                source: start..len,
                material: None,
            });
        }
        base = base.checked_add(len)?;
    }
    Some(runs)
}

pub(super) fn pack(
    encoded: &dx::EncodedVertices,
    mut runs: Vec<Run>,
) -> Option<(dx::EncodedVertices, Vec<dx::UploadPart<usize>>)> {
    let joins = |runs: &[Run]| {
        runs.windows(2)
            .filter(|pair| pair[0].material.is_some() && pair[0].material == pair[1].material)
            .count()
    };
    let original_joins = joins(&runs);
    runs.sort_unstable_by_key(|run| {
        // Place the translucent tails together, in their original order.
        (
            run.material.is_none(),
            run.material,
            run.base,
            run.source.start,
        )
    });
    if joins(&runs) <= original_joins {
        return None;
    }

    let mut output = dx::EncodedVertices::empty(encoded.encoding());
    output.try_reserve_vertices(encoded.len())?;
    let mut parts = Vec::new();
    parts.try_reserve_exact(runs.len()).ok()?;
    for run in runs {
        let first = output.len() as u32;
        let start = (run.base + run.source.start) as usize;
        let end = (run.base + run.source.end) as usize;
        output.append_range(encoded, start..end).ok()?;
        parts.push(dx::UploadPart {
            key: run.section,
            first,
            source: run.source,
        });
    }
    if output.len() != encoded.len() {
        return None;
    }
    parts.sort_unstable_by_key(|part| (part.key, part.source.start));
    parts.dedup_by(|next, prev| {
        if next.key == prev.key
            && next.source.start == prev.source.end
            && next.first == prev.first + prev.source.len() as u32
        {
            prev.source.end = next.source.end;
            true
        } else {
            false
        }
    });
    Some((output, parts))
}

/// A page assembled from current, complete sections can use the same material
/// layout as resident compaction. A split section or a stale part leaves the
/// original page in place, including its existing source-to-buffer offsets.
pub(super) fn pack_arrival<'a, K: Copy>(
    encoded: &dx::EncodedVertices,
    parts: &[dx::UploadPart<K>],
    section: impl Fn(K) -> (usize, &'a [Packet]),
) -> Option<(dx::EncodedVertices, Vec<dx::UploadPart<K>>)> {
    if parts.len() < 2 || parts.len() > MAX_RANGES {
        return None;
    }
    let mut sources = smallvec::SmallVec::<[(usize, &[Packet]); 8]>::new();
    sources.try_reserve(parts.len()).ok()?;
    let mut offset = 0;
    for part in parts {
        let (len, packets) = section(part.key);
        if part.first as usize != offset
            || part.source.start != 0
            || part.source.end as usize != len
        {
            return None;
        }
        offset = offset.checked_add(len)?;
        sources.push((len, packets));
    }
    if offset != encoded.len() {
        return None;
    }
    let (vertices, ranges) = pack(encoded, plan(sources.into_iter())?)?;
    let mut mapped = Vec::new();
    mapped.try_reserve_exact(ranges.len()).ok()?;
    mapped.extend(ranges.into_iter().map(|part| dx::UploadPart {
        key: parts[part.key].key,
        first: part.first,
        source: part.source,
    }));
    Some((vertices, mapped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(first: u32, page: usize, opaque: bool) -> Packet {
        Packet {
            page,
            opaque,
            first,
            count: 4,
            source: first / 4..first / 4 + 1,
        }
    }

    fn input() -> dx::EncodedVertices {
        let vertices: Vec<_> = (0..28)
            .map(|n| dx::Vertex {
                pos: [n as f32, 0.0, 0.0],
                uv: [0.25, 0.5],
                color: 0xff12_3456,
                light: [0.125, 0.875],
            })
            .collect();
        dx::EncodedVertices::new(&vertices, false)
    }

    #[test]
    fn packing_joins_materials_in_original_order_and_preserves_every_source_slice() {
        let first = [packet(0, 0, true), packet(4, 0, false), packet(8, 0, true)];
        let second = [packet(0, 1, true), packet(4, 0, true)];
        let original = input();
        let runs = plan([(16, first.as_slice()), (12, second.as_slice())].into_iter()).unwrap();
        let (packed, parts) = pack(&original, runs).unwrap();
        let stride = original.encoding().stride();
        assert_eq!(packed.len(), original.len());
        // Same-page opaque packets from both sections now occupy one draw range.
        // Cutout, a separate page and the two translucent tails retain their identities.
        for (dest, source) in [4, 0, 8, 20, 16, 12, 24].into_iter().enumerate() {
            assert_eq!(
                packed.bytes()[dest * 4 * stride..(dest + 1) * 4 * stride],
                original.bytes()[source * stride..(source + 4) * stride]
            );
        }
        for (section, (base, len)) in [(0, 16), (16, 12)].into_iter().enumerate() {
            let mut next = 0;
            for part in parts.iter().filter(|p| p.key == section) {
                assert_eq!(part.source.start, next);
                let count = part.source.len();
                let gpu = part.first as usize;
                let src = base + part.source.start as usize;
                assert_eq!(
                    packed.bytes()[gpu * stride..(gpu + count) * stride],
                    original.bytes()[src * stride..(src + count) * stride]
                );
                next = part.source.end;
            }
            assert_eq!(next, len);
        }
    }

    #[test]
    fn packing_leaves_contiguous_or_invalid_input_on_the_normal_path() {
        let first = [packet(0, 0, true), packet(4, 0, true)];
        let second = [packet(0, 1, true), packet(4, 1, true)];
        let contiguous =
            plan([(8, first.as_slice()), (20, second.as_slice())].into_iter()).unwrap();
        assert!(pack(&input(), contiguous).is_none());
        let broken = [packet(4, 0, true)];
        assert!(plan([(8, broken.as_slice())].into_iter()).is_none());
        let too_many = (0..MAX_RANGES)
            .map(|i| packet(i as u32 * 4, 0, true))
            .collect::<Vec<_>>();
        assert!(plan([(MAX_RANGES * 4, too_many.as_slice())].into_iter()).is_none());
    }

    #[test]
    fn first_upload_packs_only_whole_pages_with_current_sections() {
        let first = [packet(0, 0, true), packet(4, 0, false), packet(8, 0, true)];
        let second = [packet(0, 1, true), packet(4, 0, true)];
        let original = input();
        let lookup = |key| match key {
            (9, 11) => (16, first.as_slice()),
            (4, 15) => (12, second.as_slice()),
            _ => unreachable!(),
        };
        let mut parts = [
            dx::UploadPart {
                key: (9, 11),
                first: 0,
                source: 0..16,
            },
            dx::UploadPart {
                key: (4, 15),
                first: 16,
                source: 0..12,
            },
        ];
        let (encoded, packed) = pack_arrival(&original, &parts, lookup).unwrap();
        assert_eq!(encoded.len(), original.len());
        let stride = original.encoding().stride();
        let mut source_offset = [0, 16];
        for part in packed {
            let i = usize::from(part.key == (4, 15));
            assert_eq!(part.key, parts[i].key);
            assert_eq!(source_offset[i], parts[i].first + part.source.start);
            let dest = part.first as usize * stride;
            let src = source_offset[i] as usize * stride;
            let bytes = part.source.len() * stride;
            assert_eq!(
                &encoded.bytes()[dest..dest + bytes],
                &original.bytes()[src..src + bytes]
            );
            source_offset[i] += part.source.len() as u32;
        }
        assert_eq!(source_offset, [16, 28]);

        // A dropped stale part leaves a hole; a section split between native
        // pages has incomplete coverage. Both use their original offsets.
        assert!(pack_arrival(&original, &parts[1..], lookup).is_none());
        parts[0].source.end = 12;
        assert!(pack_arrival(&original, &parts, lookup).is_none());
        parts[0].source.end = 16;
        parts[1].first += 4;
        assert!(pack_arrival(&original, &parts, lookup).is_none());
    }
}

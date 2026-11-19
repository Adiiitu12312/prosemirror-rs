use serde::{Deserialize, Serialize};
use std::fmt;

pub const DEL_BEFORE: u8 = 1;
pub const DEL_AFTER: u8 = 2;
pub const DEL_ACROSS: u8 = 4;
pub const DEL_SIDE: u8 = 8;

pub trait Mappable {
    fn map(&self, pos: usize, assoc: i32) -> usize;
    fn map_result(&self, pos: usize, assoc: i32) -> MapResult;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapResult {
    pub pos: usize,
    pub del_info: u8,
    pub recover: Option<u64>,
}

impl MapResult {
    pub fn new(pos: usize, del_info: u8, recover: Option<u64>) -> Self {
        Self {
            pos,
            del_info,
            recover,
        }
    }

    pub fn deleted(&self) -> bool {
        (self.del_info & DEL_SIDE) > 0
    }

    pub fn deleted_before(&self) -> bool {
        (self.del_info & (DEL_BEFORE | DEL_ACROSS)) > 0
    }

    pub fn deleted_after(&self) -> bool {
        (self.del_info & (DEL_AFTER | DEL_ACROSS)) > 0
    }

    pub fn deleted_across(&self) -> bool {
        (self.del_info & DEL_ACROSS) > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StepMap {
    pub ranges: Vec<usize>,
    #[serde(default)]
    pub inverted: bool,
}

impl StepMap {
    pub fn empty() -> Self {
        Self {
            ranges: Vec::new(),
            inverted: false,
        }
    }

    pub fn new(ranges: Vec<usize>) -> Self {
        Self::new_inverted(ranges, false)
    }

    pub fn new_inverted(ranges: Vec<usize>, inverted: bool) -> Self {
        assert_eq!(ranges.chunks_exact(3).remainder().len(), 0, "Ranges must be multiple of 3");
        let inverted = inverted && !ranges.is_empty();
        Self { ranges, inverted }
    }

    pub fn single(pos: usize, old_size: usize, new_size: usize) -> Self {
        Self {
            ranges: vec![pos, old_size, new_size],
            inverted: false,
        }
    }

    pub fn map(&self, pos: usize, assoc: i32) -> usize {
        self.map_internal(pos, assoc, true).pos
    }

    pub fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        self.map_internal(pos, assoc, false)
    }

    pub fn offset(offset: isize) -> Self {
        match offset.cmp(&0) {
            std::cmp::Ordering::Less => Self::single(0, offset.unsigned_abs(), 0),
            std::cmp::Ordering::Equal => Self::empty(),
            std::cmp::Ordering::Greater => Self::single(0, 0, offset as usize),
        }
    }

    pub fn for_each(&self, mut visit: impl FnMut(usize, usize, usize, usize)) {
        let mut diff = 0;
        for range in self.ranges.chunks_exact(3) {
            let start = range[0] as isize;
            let (old_start, new_start, old_size, new_size) = if self.inverted {
                (start - diff, start, range[2], range[1])
            } else {
                (start, start + diff, range[1], range[2])
            };
            visit(
                old_start as usize,
                old_start as usize + old_size,
                new_start as usize,
                new_start as usize + new_size,
            );
            diff += new_size as isize - old_size as isize;
        }
    }

    pub fn touches(&self, pos: usize, recover: u64) -> bool {
        let range_index = (recover & 0xffff) as usize;
        let mut touches = false;
        let mut index = 0;
        self.for_each(|start, end, _, _| {
            if index == range_index && start <= pos && pos <= end {
                touches = true;
            }
            index += 1;
        });
        touches
    }

    fn map_internal(&self, pos: usize, assoc: i32, simple: bool) -> MapResult {
        let mut diff: isize = 0;
        let old_index = if self.inverted { 2 } else { 1 };
        let new_index = if self.inverted { 1 } else { 2 };

        let mut i = 0;
        while i < self.ranges.len() {
            let base_start = self.ranges[i] as isize;
            let start = if self.inverted {
                base_start - diff
            } else {
                base_start
            };

            if start > pos as isize {
                break;
            }

            let old_size = self.ranges[i + old_index] as isize;
            let new_size = self.ranges[i + new_index] as isize;
            let end = start + old_size;

            if (pos as isize) <= end {
                let side = if old_size == 0 {
                    assoc
                } else if (pos as isize) == start {
                    -1
                } else if (pos as isize) == end {
                    1
                } else {
                    assoc
                };

                let res_offset = if side < 0 { 0 } else { new_size };
                let result = (start + diff + res_offset).max(0) as usize;

                if simple {
                    return MapResult::new(result, 0, None);
                }

                let mut del = if (pos as isize) == start {
                    DEL_AFTER
                } else if (pos as isize) == end {
                    DEL_BEFORE
                } else {
                    DEL_ACROSS
                };

                let side_deleted = if assoc < 0 {
                    (pos as isize) != start
                } else {
                    (pos as isize) != end
                };

                if side_deleted {
                    del |= DEL_SIDE;
                }

                let recover = if (pos as isize) == (if assoc < 0 { start } else { end }) {
                    None
                } else {
                    Some(((i / 3) as u64) | (((pos as isize - start) as u64) << 16))
                };

                return MapResult::new(result, del, recover);
            }

            diff += new_size - old_size;
            i += 3;
        }

        let final_pos = (pos as isize + diff).max(0) as usize;
        MapResult::new(final_pos, 0, None)
    }

    pub fn recover(&self, value: u64) -> usize {
        let mut diff: isize = 0;
        let index = (value & 0xffff) as usize;
        let offset = (value >> 16) as usize;
        if !self.inverted {
            for i in 0..index {
                diff += self.ranges[i * 3 + 2] as isize - self.ranges[i * 3 + 1] as isize;
            }
        }
        (self.ranges[index * 3] as isize + diff + offset as isize).max(0) as usize
    }

    pub fn invert(&self) -> Self {
        Self::new_inverted(self.ranges.clone(), !self.inverted)
    }
}

impl Mappable for StepMap {
    fn map(&self, pos: usize, assoc: i32) -> usize {
        Self::map(self, pos, assoc)
    }

    fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        Self::map_result(self, pos, assoc)
    }
}

impl fmt::Display for StepMap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inverted {
            formatter.write_str("-")?;
        }
        formatter.write_str("[")?;
        for (index, value) in self.ranges.iter().enumerate() {
            if index != 0 {
                formatter.write_str(",")?;
            }
            write!(formatter, "{value}")?;
        }
        formatter.write_str("]")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mapping {
    pub maps: Vec<StepMap>,
    #[serde(default)]
    pub mirror: Option<Vec<usize>>,
    pub from: usize,
    pub to: usize,
}

impl Mapping {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_maps(maps: Vec<StepMap>) -> Self {
        let to = maps.len();
        Self {
            maps,
            mirror: None,
            from: 0,
            to,
        }
    }

    pub fn append_map(&mut self, map: StepMap, mirrors: Option<usize>) {
        self.maps.push(map);
        self.to = self.maps.len();
        if let Some(m) = mirrors {
            self.set_mirror(self.maps.len() - 1, m);
        }
    }

    pub fn set_mirror(&mut self, n: usize, m: usize) {
        let mirror = self.mirror.get_or_insert_with(Vec::new);
        mirror.push(n);
        mirror.push(m);
    }

    pub fn get_mirror(&self, n: usize) -> Option<usize> {
        if let Some(ref mirror) = self.mirror {
            for i in 0..mirror.len() {
                if mirror[i] == n {
                    let other_idx = if i % 2 == 1 { i - 1 } else { i + 1 };
                    return Some(mirror[other_idx]);
                }
            }
        }
        None
    }

    pub fn append_mapping(&mut self, mapping: &Mapping) {
        let start_size = self.maps.len();
        for i in 0..mapping.maps.len() {
            let mirr = mapping.get_mirror(i);
            let target_mirror = mirr.and_then(|m| if m < i { Some(start_size + m) } else { None });
            self.append_map(mapping.maps[i].clone(), target_mirror);
        }
    }

    pub fn append_mapping_inverted(&mut self, mapping: &Mapping) {
        let total_size = self.maps.len() + mapping.maps.len();
        for i in (0..mapping.maps.len()).rev() {
            let mirr = mapping.get_mirror(i);
            let target_mirror = mirr.and_then(|m| if m > i { Some(total_size - m - 1) } else { None });
            self.append_map(mapping.maps[i].invert(), target_mirror);
        }
    }

    pub fn invert(&self) -> Self {
        let mut inverse = Mapping::new();
        inverse.append_mapping_inverted(self);
        inverse
    }

    pub fn slice(&self, from: usize, to: usize) -> Self {
        let end = to.min(self.maps.len());
        Self {
            maps: self.maps.clone(),
            mirror: self.mirror.clone(),
            from,
            to: end,
        }
    }

    pub fn map(&self, pos: usize, assoc: i32) -> usize {
        if self.mirror.is_some() {
            return self.map_result(pos, assoc).pos;
        }
        (self.from..self.to.min(self.maps.len())).fold(pos, |mapped, index| {
            self.maps[index].map(mapped, assoc)
        })
    }

    pub fn map_result(&self, mut pos: usize, assoc: i32) -> MapResult {
        let mut del_info = 0;
        let mut i = self.from;
        let end = self.to.min(self.maps.len());

        while i < end {
            let res = self.maps[i].map_result(pos, assoc);
            if let Some(recover) = res.recover {
                if let Some(corr) = self.get_mirror(i) {
                    if corr > i && corr < end {
                        i = corr + 1;
                        pos = self.maps[corr].recover(recover);
                        continue;
                    }
                }
            }
            del_info |= res.del_info;
            pos = res.pos;
            i += 1;
        }

        MapResult::new(pos, del_info, None)
    }
}

impl Mappable for Mapping {
    fn map(&self, pos: usize, assoc: i32) -> usize {
        Self::map(self, pos, assoc)
    }

    fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        Self::map_result(self, pos, assoc)
    }
}

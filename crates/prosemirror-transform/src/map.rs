use serde::{Deserialize, Serialize};

pub const DEL_BEFORE: u8 = 1;
pub const DEL_AFTER: u8 = 2;
pub const DEL_ACROSS: u8 = 4;
pub const DEL_SIDE: u8 = 8;

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
        assert!(ranges.len() % 3 == 0, "Ranges must be multiple of 3");
        Self {
            ranges,
            inverted: false,
        }
    }

    pub fn new_inverted(ranges: Vec<usize>, inverted: bool) -> Self {
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
        Self {
            ranges: self.ranges.clone(),
            inverted: !self.inverted,
        }
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
        Self {
            maps: Vec::new(),
            mirror: None,
            from: 0,
            to: 0,
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
        self.map_internal(pos, assoc, true).pos
    }

    pub fn map_result(&self, pos: usize, assoc: i32) -> MapResult {
        self.map_internal(pos, assoc, false)
    }

    fn map_internal(&self, mut pos: usize, assoc: i32, simple: bool) -> MapResult {
        let mut del_info = 0;
        let mut i = self.from;
        let end = if self.to == 0 { self.maps.len() } else { self.to.min(self.maps.len()) };

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

        if simple {
            MapResult::new(pos, 0, None)
        } else {
            MapResult::new(pos, del_info, None)
        }
    }
}

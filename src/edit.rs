use crate::model::*;
use anyhow::{Result, ensure};

#[derive(Default)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
}
impl History {
    /// Copy / validate / commit makes every command atomic, including errors.
    pub fn apply(
        &mut self,
        p: &mut Project,
        f: impl FnOnce(&mut Project) -> Result<()>,
    ) -> Result<bool> {
        let mut candidate = p.clone();
        f(&mut candidate)?;
        candidate.validate()?;
        if candidate == *p {
            return Ok(false);
        }
        self.undo.push(std::mem::replace(p, candidate));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(true)
    }
    pub fn undo(&mut self, p: &mut Project) -> bool {
        if let Some(old) = self.undo.pop() {
            self.redo.push(std::mem::replace(p, old));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self, p: &mut Project) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(p, next));
            true
        } else {
            false
        }
    }
}
fn index(p: &Project, id: &str) -> Result<usize> {
    let i = p
        .clips
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| anyhow::anyhow!("No clip selected"))?;
    ensure!(!p.tracks[p.clips[i].lane].locked, "Track is locked");
    Ok(i)
}
pub fn crop(p: &mut Project, clip: &str, crop: Option<CropRect>) -> Result<()> {
    let i = index(p, clip)?;
    let c = &p.clips[i];
    ensure!(c.lane == 0, "Select a video clip to crop");
    if let Some(rect) = crop {
        let m = p.media(&c.media)?;
        rect.validate(m.width, m.height)?;
    }
    p.clips[i].crop = crop;
    p.clips[i].framing.clear();
    Ok(())
}
/// Static framing and animation use the same rectangle; disabling motion freezes
/// the currently displayed view, rather than jumping back to an old crop.
pub fn animate_frame(p: &mut Project, clip: &str, at: Tick, enabled: bool) -> Result<()> {
    let i = index(p, clip)?;
    ensure!(p.clips[i].lane == 0, "Select a video clip to frame");
    let c = &p.clips[i];
    let m = p.media(&c.media)?;
    ensure!(
        (0..=m.duration).contains(&at),
        "Framing point exceeds source"
    );
    let rect = c
        .crop_at(at)
        .unwrap_or(CropRect::full(m.width & !1, m.height & !1));
    let c = &mut p.clips[i];
    if enabled && c.framing.is_empty() {
        c.framing.push(FramePoint { at, rect });
    }
    if !enabled {
        c.crop = Some(rect);
        c.framing.clear();
    }
    Ok(())
}
pub fn frame_at(p: &mut Project, clip: &str, at: Tick, rect: CropRect) -> Result<()> {
    let i = index(p, clip)?;
    let c = &p.clips[i];
    let m = p.media(&c.media)?;
    rect.validate(m.width, m.height)?;
    ensure!(
        c.lane == 0 && (0..=m.duration).contains(&at),
        "Invalid framing point"
    );
    let full = CropRect::full(m.width & !1, m.height & !1);
    let c = &mut p.clips[i];
    if c.framing.is_empty() {
        c.crop = (rect != full).then_some(rect);
    } else {
        match c.framing.binary_search_by_key(&at, |p| p.at) {
            Ok(i) => c.framing[i].rect = rect,
            Err(i) => {
                ensure!(
                    c.framing.len() < 256,
                    "A clip supports up to 256 framing points"
                );
                c.framing.insert(i, FramePoint { at, rect });
            }
        }
    }
    Ok(())
}
pub fn remove_frame(p: &mut Project, clip: &str, at: Tick) -> Result<()> {
    let i = index(p, clip)?;
    ensure!(p.clips[i].lane == 0, "Select a video clip to frame");
    let c = &mut p.clips[i];
    let rect = c.crop_at(at);
    c.framing.retain(|point| point.at != at);
    if c.framing.is_empty() {
        c.crop = rect;
    }
    Ok(())
}
fn members(p: &Project, clip: &str) -> Result<Vec<usize>> {
    let i = index(p, clip)?;
    let group = p.clips[i].link.as_ref();
    let ids: Vec<_> = p
        .clips
        .iter()
        .enumerate()
        .filter(|(j, c)| *j == i || group.is_some() && c.link.as_ref() == group)
        .map(|(j, _)| j)
        .collect();
    ensure!(
        ids.iter().all(|j| !p.tracks[p.clips[*j].lane].locked),
        "A linked track is locked; unlock it or unlink the clips"
    );
    Ok(ids)
}
pub fn unlink(p: &mut Project, clip: &str) -> Result<()> {
    for i in members(p, clip)? {
        p.clips[i].link = None;
    }
    Ok(())
}
pub fn link(p: &mut Project, clip: &str) -> Result<()> {
    let i = index(p, clip)?;
    let c = &p.clips[i];
    ensure!(c.link.is_none(), "Clip is already linked");
    let j = p.clips.iter().position(|x| x.id != c.id && x.link.is_none() && (x.lane == 0) != (c.lane == 0)
        && x.media == c.media && x.start == c.start && x.source_in == c.source_in && x.duration == c.duration)
        .ok_or_else(|| anyhow::anyhow!("Align an audio and video clip from the same source, with matching source in and duration, before linking"))?;
    ensure!(!p.tracks[p.clips[j].lane].locked, "Partner track is locked");
    let group = id();
    p.clips[i].link = Some(group.clone());
    p.clips[j].link = Some(group);
    Ok(())
}
pub fn append(p: &mut Project, media: &str, lane: usize, at: Option<Tick>) -> Result<String> {
    ensure!(
        lane < LANES && !p.tracks[lane].locked,
        "Track is locked or invalid"
    );
    let start = at.unwrap_or_else(|| p.lane(lane).last().map(|c| c.end()).unwrap_or(0));
    let mut c = Clip::new(p.media(media)?, lane, p.fps.align(start).max(0));
    // All primary edits are project-frame aligned; source PTS remain independent.
    if lane == 0 {
        c.duration = p.fps.frame(
            (c.duration as i128 * p.fps.num as i128 / (HZ as i128 * p.fps.den as i128)) as i64,
        );
    }
    ensure!(c.duration > 0, "Source is shorter than one project frame");
    if lane == 0 && at.is_some() {
        ensure!(
            !p.lane(0).iter().any(|x| x.start < start && x.end() > start),
            "Insert at a clip boundary; split first"
        );
        shift_after(p, start, c.duration);
    }
    let id = c.id.clone();
    if lane == 0 && p.media(media)?.audio.is_some() {
        ensure!(!p.tracks[1].locked, "Source audio track is locked");
        let group = crate::model::id();
        c.link = Some(group);
        let mut audio = c.clone();
        audio.id = crate::model::id();
        audio.lane = 1;
        p.clips.push(audio);
    }
    p.clips.push(c);
    Ok(id)
}
fn shift_after(p: &mut Project, at: Tick, delta: Tick) {
    for c in &mut p.clips {
        if !p.tracks[c.lane].locked && c.start >= at {
            c.start += delta;
        }
    }
}
pub fn split(p: &mut Project, clip: &str, at: Tick) -> Result<String> {
    let ids = members(p, clip)?;
    let at = p.fps.align(at);
    let group = ids
        .first()
        .and_then(|i| p.clips[*i].link.as_ref())
        .map(|_| id());
    let mut selected_right = String::new();
    for i in ids {
        let c = &mut p.clips[i];
        ensure!(
            at > c.start && at < c.end(),
            "Place the playhead inside the selected clip"
        );
        let mut right = c.clone();
        right.id = id();
        right.link = group.clone();
        right.source_in += at - c.start;
        right.duration = c.end() - at;
        right.start = at;
        c.duration = at - c.start;
        c.fade_out = 0;
        right.fade_in = 0;
        c.clamp_fades();
        right.clamp_fades();
        if c.id == clip {
            selected_right = right.id.clone();
        }
        p.clips.push(right);
    }
    Ok(selected_right)
}
pub fn trim(
    p: &mut Project,
    clip: &str,
    source_in: Tick,
    duration: Tick,
    ripple: bool,
) -> Result<()> {
    let ids = members(p, clip)?;
    let i = index(p, clip)?;
    let old_end = p.clips[i].end();
    let old_duration = p.clips[i].duration;
    ensure!(
        duration > 0 && source_in >= 0,
        "Trim must retain positive duration"
    );
    if ripple && p.clips[i].lane == 0 {
        shift_after(p, old_end, duration - old_duration);
    }
    for i in ids {
        p.clips[i].source_in = source_in;
        p.clips[i].duration = duration;
        p.clips[i].clamp_fades();
    }
    Ok(())
}
/// Clamp an ordinary edge trim to source and neighbouring clips on every linked lane.
/// The opposite edge is fixed. There is deliberately no implicit ripple.
pub fn trim_edge(p: &mut Project, clip: &str, left: bool, requested: Tick) -> Result<Tick> {
    let ids = members(p, clip)?;
    let c = p.clips[index(p, clip)?].clone();
    let frame = p.fps.frame(1);
    let (mut low, mut high) = if left {
        ((c.start - c.source_in).max(0), c.end() - frame)
    } else {
        (
            c.start + frame,
            c.start + p.media(&c.media)?.duration - c.source_in,
        )
    };
    for i in &ids {
        for other in p
            .lane(p.clips[*i].lane)
            .into_iter()
            .filter(|x| !ids.iter().any(|j| p.clips[*j].id == x.id))
        {
            if left && other.end() <= c.start {
                low = low.max(other.end());
            }
            if !left && other.start >= c.end() {
                high = high.min(other.start);
            }
        }
    }
    let edge = p.fps.align(requested).clamp(low, high);
    for i in ids {
        let x = &mut p.clips[i];
        if left {
            x.start = edge;
            x.source_in = c.source_in + edge - c.start;
            x.duration = c.end() - edge;
        } else {
            x.duration = edge - c.start;
        }
        x.clamp_fades();
    }
    Ok(edge)
}
pub fn move_clip(p: &mut Project, clip: &str, at: Tick, lane: usize) -> Result<()> {
    let (ids, i) = move_members(p, clip, lane)?;
    let delta = p.fps.align(at).max(0) - p.clips[i].start;
    for j in ids {
        p.clips[j].start += delta;
    }
    p.clips[i].lane = lane;
    Ok(())
}
fn move_members(p: &Project, clip: &str, lane: usize) -> Result<(Vec<usize>, usize)> {
    let ids = members(p, clip)?;
    let i = index(p, clip)?;
    ensure!(
        lane < LANES && !p.tracks[lane].locked,
        "Destination track is locked"
    );
    ensure!(
        (p.clips[i].lane == 0) == (lane == 0),
        "Video and audio lanes cannot be interchanged"
    );
    Ok((ids, i))
}
/// Nearest legal position shared by drag feedback and drop. Obstacles on every
/// linked destination lane constrain the same start time. Touching edges are legal.
pub fn move_target(p: &Project, clip: &str, requested: Tick, lane: usize) -> Result<Tick> {
    let (ids, i) = move_members(p, clip, lane)?;
    let c = &p.clips[i];
    let high = MAX_TIME - c.duration;
    let requested = requested.clamp(0, high);
    let lanes: Vec<_> = ids
        .iter()
        .map(|j| if *j == i { lane } else { p.clips[*j].lane })
        .collect();
    // An obstacle [a,b) excludes starts strictly inside (a-duration,b).
    // Merge overlapping open intervals, but retain a shared endpoint: it can
    // be the one exact-fitting slot between two clips.
    let mut blocked: Vec<_> = p
        .clips
        .iter()
        .enumerate()
        .filter(|(j, other)| !ids.contains(j) && lanes.contains(&other.lane))
        .map(|(_, other)| (other.start - c.duration, other.end()))
        .collect();
    blocked.sort_unstable();
    let mut merged: Vec<(Tick, Tick)> = vec![];
    for (lo, hi) in blocked {
        if let Some(last) = merged.last_mut()
            && lo < last.1
        {
            last.1 = last.1.max(hi);
        } else {
            merged.push((lo, hi));
        }
    }
    if let Some((lo, hi)) = merged
        .into_iter()
        .find(|(lo, hi)| *lo < requested && requested < *hi)
    {
        [lo, hi]
            .into_iter()
            .filter(|t| *t >= 0 && *t <= high)
            .min_by_key(|t| ((t - requested).abs(), (t - c.start).abs(), *t))
            .ok_or_else(|| anyhow::anyhow!("No space for this clip on the destination track"))
    } else {
        Ok(requested)
    }
}
pub fn move_clip_clamped(
    p: &mut Project,
    clip: &str,
    requested: Tick,
    lane: usize,
) -> Result<Tick> {
    let at = move_target(p, clip, requested, lane)?;
    let (ids, i) = move_members(p, clip, lane)?;
    let delta = at - p.clips[i].start;
    for j in ids {
        p.clips[j].start += delta;
    }
    p.clips[i].lane = lane;
    Ok(at)
}
/// Magnetic alignment considers both ends of the moving event and excludes
/// its linked partner, so its own sound cannot pull it back to the old position.
pub fn snap_move(
    p: &Project,
    clip: &str,
    at: Tick,
    tolerance: Tick,
    playhead: Tick,
) -> Result<Tick> {
    let ids = members(p, clip)?;
    let c = &p.clips[index(p, clip)?];
    Ok(p.clips
        .iter()
        .enumerate()
        .filter(|(j, _)| !ids.contains(j))
        .flat_map(|(_, other)| [other.start, other.end()])
        .chain([0, playhead])
        .flat_map(|edge| [edge, edge - c.duration])
        .filter(|t| *t >= 0 && (*t - at).abs() <= tolerance)
        .min_by_key(|t| ((*t - at).abs(), *t))
        .unwrap_or(at))
}
pub fn reorder(p: &mut Project, clip: &str, direction: i32) -> Result<()> {
    let i = index(p, clip)?;
    let lane = p.clips[i].lane;
    let order: Vec<String> = p.lane(lane).iter().map(|c| c.id.clone()).collect();
    let pos = order.iter().position(|id| id == clip).unwrap();
    let dest = pos as i32 + direction;
    ensure!(
        dest >= 0 && dest < order.len() as i32,
        "Already at lane boundary"
    );
    let j = p
        .clips
        .iter()
        .position(|c| c.id == order[dest as usize])
        .unwrap();
    let partner = p.clips[j].id.clone();
    let ids = members(p, clip)?;
    let others = members(p, &partner)?;
    let (old_i, old_j) = (p.clips[i].start, p.clips[j].start);
    if direction < 0 {
        let start = p.clips[j].start;
        let gap = p.clips[i].start - p.clips[j].end();
        p.clips[j].start = start + p.clips[i].duration + gap;
        p.clips[i].start = start;
    } else {
        let start = p.clips[i].start;
        let gap = p.clips[j].start - p.clips[i].end();
        p.clips[i].start = start + p.clips[j].duration + gap;
        p.clips[j].start = start;
    }
    let (di, dj) = (p.clips[i].start - old_i, p.clips[j].start - old_j);
    for k in ids {
        if k != i {
            p.clips[k].start += di;
        }
    }
    for k in others {
        if k != j {
            p.clips[k].start += dj;
        }
    }
    Ok(())
}
pub fn delete(p: &mut Project, clip: &str, ripple: bool) -> Result<()> {
    let ids = members(p, clip)?;
    let i = index(p, clip)?;
    let c = p.clips[i].clone();
    p.clips = p
        .clips
        .iter()
        .enumerate()
        .filter(|(j, _)| !ids.contains(j))
        .map(|(_, c)| c.clone())
        .collect();
    if ripple {
        ripple_region(p, c.start, c.end())?;
    }
    Ok(())
}
/// Remove [start,end) from all unlocked lanes. Intersections are trimmed/split;
/// a spanning clip retains both surviving portions. Locked lanes are untouched.
pub fn ripple_region(p: &mut Project, start: Tick, end: Tick) -> Result<()> {
    ensure!(start >= 0 && end > start, "Invalid ripple region");
    let mut result = vec![];
    let mut right_groups = std::collections::HashMap::new();
    for c in &p.clips {
        if p.tracks[c.lane].locked || c.end() <= start {
            result.push(c.clone());
            continue;
        }
        if c.start >= end {
            let mut n = c.clone();
            n.start -= end - start;
            result.push(n);
            continue;
        }
        if c.start < start {
            let mut left = c.clone();
            left.duration = start - c.start;
            left.fade_out = 0;
            left.clamp_fades();
            result.push(left);
        }
        if c.end() > end {
            let mut right = c.clone();
            if c.start < start {
                right.id = id();
                if let Some(group) = &c.link {
                    right.link = Some(right_groups.entry(group.clone()).or_insert_with(id).clone());
                }
            }
            right.source_in += end - c.start;
            right.duration = c.end() - end;
            right.start = start;
            right.fade_in = 0;
            right.clamp_fades();
            result.push(right);
        }
    }
    p.clips = result;
    Ok(())
}
pub fn close_gaps(p: &mut Project) -> Result<()> {
    ensure!(!p.tracks[0].locked, "Video track is locked");
    let mut cursor = 0;
    let ranges: Vec<_> = p.lane(0).iter().map(|c| (c.start, c.end())).collect();
    let mut removed = 0;
    for (s, e) in ranges {
        let s = s - removed;
        let e = e - removed;
        if s > cursor {
            ripple_region(p, cursor, s)?;
            removed += s - cursor;
        }
        cursor = e - (s - cursor).max(0);
    }
    Ok(())
}
pub fn snap(p: &Project, t: Tick, tolerance: Tick, excluding: Option<&str>) -> Tick {
    p.clips
        .iter()
        .filter(|c| Some(c.id.as_str()) != excluding)
        .flat_map(|c| [c.start, c.end()])
        .chain([0])
        .filter(|x| (x - t).abs() <= tolerance)
        .min_by_key(|x| (x - t).abs())
        .unwrap_or(t)
}

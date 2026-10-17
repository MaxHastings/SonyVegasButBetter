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
pub fn append(p: &mut Project, media: &str, lane: usize, at: Option<Tick>) -> Result<String> {
    ensure!(
        lane < 3 && !p.tracks[lane].locked,
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
    let i = index(p, clip)?;
    let at = p.fps.align(at);
    let c = &mut p.clips[i];
    ensure!(
        at > c.start && at < c.end(),
        "Place the playhead inside the selected clip"
    );
    let mut right = c.clone();
    right.id = id();
    right.source_in += at - c.start;
    right.duration = c.end() - at;
    right.start = at;
    c.duration = at - c.start;
    c.fade_out = 0;
    right.fade_in = 0;
    c.clamp_fades();
    right.clamp_fades();
    let id = right.id.clone();
    p.clips.push(right);
    Ok(id)
}
pub fn trim(
    p: &mut Project,
    clip: &str,
    source_in: Tick,
    duration: Tick,
    ripple: bool,
) -> Result<()> {
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
    p.clips[i].source_in = source_in;
    p.clips[i].duration = duration;
    p.clips[i].clamp_fades();
    Ok(())
}
pub fn move_clip(p: &mut Project, clip: &str, at: Tick, lane: usize) -> Result<()> {
    let i = index(p, clip)?;
    ensure!(
        lane < 3 && !p.tracks[lane].locked,
        "Destination track is locked"
    );
    ensure!(
        (p.clips[i].lane == 0) == (lane == 0),
        "Video and audio lanes cannot be interchanged"
    );
    p.clips[i].start = p.fps.align(at).max(0);
    p.clips[i].lane = lane;
    Ok(())
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
    Ok(())
}
pub fn delete(p: &mut Project, clip: &str, ripple: bool) -> Result<()> {
    let i = index(p, clip)?;
    let c = p.clips.remove(i);
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

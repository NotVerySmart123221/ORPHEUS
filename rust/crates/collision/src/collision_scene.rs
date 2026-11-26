//! Placed static probe and ray geometry for retail build 104319.
//!
//! Ports `probe_triangles` from `ow174/game/collision.py` and the shape
//! triangulation and placement math from `ow174/game/props.py`. Movement
//! contacts have their own f32 representation and solver.

use std::collections::{BTreeSet, HashMap, HashSet, hash_map::Entry};
use std::error::Error;
use std::fmt;
use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;

use crate::collision_cache::{
    CacheError, CollisionCache, Hull, Mesh, Placement, RayCacheIdentity, Shape,
};
use crate::collision_grid::cell;
use crate::collision_index::{IndexError, RayHit, RayPrimitive, SceneTriangle, TriangleIndex};
use crate::static_ray::{Vec3, norm, round_hit};

const PRISM_SIDES: usize = 12;
const MAX_SCENE_SHAPES: usize = 2_000_000;

#[derive(Debug)]
pub enum SceneError {
    Cache(CacheError),
    Index(IndexError),
    InvalidGeometry(&'static str),
    ExpansionLimit,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cache(error) => fmt::Display::fmt(error, f),
            Self::Index(error) => fmt::Display::fmt(error, f),
            Self::InvalidGeometry(reason) => write!(f, "invalid scene geometry: {reason}"),
            Self::ExpansionLimit => {
                f.write_str("collision scene expansion exceeds the shape limit")
            }
        }
    }
}

impl Error for SceneError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cache(error) => Some(error),
            Self::Index(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CacheError> for SceneError {
    fn from(value: CacheError) -> Self {
        Self::Cache(value)
    }
}

impl From<IndexError> for SceneError {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

#[derive(Clone, Copy, Debug)]
struct RoundProp {
    a: Vec3,
    b: Vec3,
    radius: f64,
    mask: u16,
}

impl RoundProp {
    fn from_shape(shape: &Shape) -> Option<Self> {
        let (&a, &b, &radius, &mask) = match shape {
            Shape::Sphere {
                center,
                radius,
                mask,
            } => (center, center, radius, mask),
            Shape::Capsule { a, b, radius, mask } => (a, b, radius, mask),
            _ => return None,
        };
        Some(Self {
            a: a.map(f64::from),
            b: b.map(f64::from),
            radius: f64::from(radius),
            mask,
        })
    }

    fn placed(self, transform: Transform) -> Self {
        Self {
            a: transform.place(self.a),
            b: transform.place(self.b),
            radius: self.radius * transform.scale[0],
            mask: self.mask,
        }
    }
}

/// Per-room mutable state tracking which breakable map placements have been destroyed.
#[derive(Debug, Default)]
pub struct BreakableState {
    any_broken: AtomicBool,
    broken: RwLock<HashSet<usize>>,
}

impl BreakableState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_broken(&self, placement: usize) -> bool {
        if !self.any_broken.load(Ordering::Acquire) {
            return false;
        }
        self.broken
            .read()
            .expect("breakable state lock")
            .contains(&placement)
    }

    pub fn mark_broken(&self, placement: usize) -> bool {
        let mut guard = self.broken.write().expect("breakable state lock");
        let inserted = guard.insert(placement);
        if inserted {
            self.any_broken.store(true, Ordering::Release);
        }
        inserted
    }

    pub fn clear(&self) {
        let mut guard = self.broken.write().expect("breakable state lock");
        guard.clear();
        self.any_broken.store(false, Ordering::Release);
    }

    pub fn broken_count(&self) -> usize {
        if !self.any_broken.load(Ordering::Acquire) {
            return 0;
        }
        self.broken.read().expect("breakable state lock").len()
    }
}

/// Result of a weapon terrain ray that may pierce intact breakable panels.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponRayHit {
    pub wall: Option<RayHit>,
    /// Intact breakable placements intersected before `wall` (or `length`), sorted by distance.
    pub breakables: Vec<(f64, usize)>,
}

#[derive(Clone, Copy, Debug)]
struct BreakableTriSpan {
    placement: usize,
    start_tri: usize,
    end_tri: usize,
    min: Vec3,
    max: Vec3,
}

#[derive(Debug)]
pub struct CollisionScene {
    triangles: TriangleIndex,
    weapon_terrain_category: Option<u16>,
    triangle_sources: Vec<(usize, StaticRaySource, bool)>,
    tri_breakable_placement: Vec<u32>,
    breakable_tri_spans: Vec<BreakableTriSpan>,
    round_props: Vec<(RoundProp, StaticRaySource, bool)>,
    round_prop_bounds: Vec<(Vec3, Vec3)>,
    has_breakables: bool,
}

/// Location of a static ray primitive in the loaded collision cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StaticRaySource {
    Mesh {
        index: usize,
    },
    Hull {
        index: usize,
    },
    Placement {
        index: usize,
        model: u32,
        group: u16,
    },
}

impl CollisionScene {
    /// Build the legacy triangle view for map probes from a movement cache.
    pub fn movement_probe(
        cache: &CollisionCache,
        identifiers: &BTreeSet<u64>,
    ) -> Result<Self, SceneError> {
        cache.header.check_movement()?;
        Self::build(cache, identifiers, false)
    }

    /// Build the expanded static ray view, checking map and prop provenance first.
    pub fn expanded_rays(
        cache: &CollisionCache,
        identifiers: &BTreeSet<u64>,
        identity: RayCacheIdentity<'_>,
    ) -> Result<Self, SceneError> {
        cache.header.check_ray(identity)?;
        Self::build(cache, identifiers, true)
    }

    fn build(
        cache: &CollisionCache,
        identifiers: &BTreeSet<u64>,
        expanded: bool,
    ) -> Result<Self, SceneError> {
        let groups = cache.header.placement_groups(identifiers);
        let mut triangles = Vec::new();
        let mut triangle_sources = Vec::new();
        let mut round_props = Vec::new();
        for (index, mesh) in cache.meshes.iter().enumerate() {
            append_mesh(&mut triangles, mesh, true)?;
            triangle_sources.push((triangles.len(), StaticRaySource::Mesh { index }, false));
        }
        for (index, hull) in cache.hulls.iter().enumerate() {
            append_hull(&mut triangles, hull)?;
            triangle_sources.push((triangles.len(), StaticRaySource::Hull { index }, false));
        }
        let mut local_models = HashMap::new();
        let mut has_breakables = false;
        for (index, placement) in cache.placements.iter().enumerate() {
            if !groups.contains(&placement.group) {
                continue;
            }
            let model =
                cache
                    .models
                    .get(placement.model as usize)
                    .ok_or(SceneError::InvalidGeometry(
                        "placement model index out of range",
                    ))?;
            let transform = Transform::from(*placement);
            let [sx, sy, sz] = transform.scale;
            let analytic = expanded && sx > 0.0 && sx == sy && sx == sz && sx.is_finite();
            let local = match local_models.entry((placement.model, analytic)) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(model_triangles(model, expanded, analytic)?),
            };
            let local_count: usize = local.iter().map(|(_, tris)| tris.len()).sum();
            if triangles.len() + local_count > MAX_SCENE_SHAPES {
                return Err(SceneError::ExpansionLimit);
            }
            let source = StaticRaySource::Placement {
                index,
                model: placement.model,
                group: placement.group,
            };
            for (slot, slot_tris) in local.iter() {
                if slot_tris.is_empty() {
                    continue;
                }
                let breakable = cache.header.is_breakable_shape(placement.model, *slot);
                has_breakables |= breakable;
                triangles.extend(slot_tris.iter().map(|triangle| SceneTriangle {
                    vertices: triangle.vertices.map(|point| transform.place(point)),
                    ..*triangle
                }));
                triangle_sources.push((triangles.len(), source, breakable));
            }
            if analytic {
                for (slot, shape) in model.iter().enumerate() {
                    let Some(round) = RoundProp::from_shape(shape) else {
                        continue;
                    };
                    if round_props.len() >= MAX_SCENE_SHAPES {
                        return Err(SceneError::ExpansionLimit);
                    }
                    let breakable = cache.header.is_breakable_shape(placement.model, slot);
                    has_breakables |= breakable;
                    round_props.push((round.placed(transform), source, breakable));
                }
            }
        }
        let mut tri_breakable_placement = Vec::with_capacity(triangles.len());
        let mut breakable_tri_spans = Vec::new();
        let mut prev_end = 0usize;
        for &(end_tri, source, breakable) in &triangle_sources {
            let start_tri = prev_end;
            prev_end = end_tri;
            let placement_u32 = if breakable {
                match source {
                    StaticRaySource::Placement { index, .. } => {
                        u32::try_from(index).unwrap_or(u32::MAX)
                    }
                    _ => u32::MAX,
                }
            } else {
                u32::MAX
            };
            tri_breakable_placement.resize(end_tri, placement_u32);
            if placement_u32 != u32::MAX && start_tri < end_tri {
                let mut min = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for tri in &triangles[start_tri..end_tri] {
                    for v in &tri.vertices {
                        for axis in 0..3 {
                            min[axis] = min[axis].min(v[axis]);
                            max[axis] = max[axis].max(v[axis]);
                        }
                    }
                }
                breakable_tri_spans.push(BreakableTriSpan {
                    placement: placement_u32 as usize,
                    start_tri,
                    end_tri,
                    min,
                    max,
                });
            }
        }
        let round_prop_bounds: Vec<(Vec3, Vec3)> = round_props
            .iter()
            .map(|(prop, _, _)| {
                let min = std::array::from_fn(|a| prop.a[a].min(prop.b[a]) - prop.radius);
                let max = std::array::from_fn(|a| prop.a[a].max(prop.b[a]) + prop.radius);
                (min, max)
            })
            .collect();
        Ok(Self {
            triangles: TriangleIndex::new(triangles)?,
            weapon_terrain_category: cache
                .header
                .weapon_terrain_category
                .filter(|_| expanded)
                .map(|category| category.get()),
            triangle_sources,
            tri_breakable_placement,
            breakable_tri_spans,
            round_props,
            round_prop_bounds,
            has_breakables,
        })
    }

    pub fn weapon_terrain_category(&self) -> Option<u16> {
        self.weapon_terrain_category
    }

    pub fn triangles(&self) -> &TriangleIndex {
        &self.triangles
    }

    /// Whether geometry leaves the segment clear before its target endpoint.
    /// None means the endpoints or the ray traversal are outside the query domain.
    pub fn segment_clear(&self, origin: Vec3, target: Vec3, category: u16) -> Option<bool> {
        self.segment_clear_with_breakables(origin, target, category, None)
    }

    /// Whether geometry leaves the segment clear, skipping placements marked broken in `breakables`.
    pub fn segment_clear_with_breakables(
        &self,
        origin: Vec3,
        target: Vec3,
        category: u16,
        breakables: Option<&BreakableState>,
    ) -> Option<bool> {
        for point in [origin, target] {
            if !point.into_iter().all(f64::is_finite) {
                return None;
            }
            cell(point[0])?;
            cell(point[2])?;
        }
        let delta: Vec3 = std::array::from_fn(|axis| target[axis] - origin[axis]);
        let scale = delta.into_iter().map(f64::abs).fold(0.0, f64::max);
        if scale == 0.0 {
            return Some(true);
        }
        if !scale.is_finite() {
            return None;
        }
        let scaled = delta.map(|component| component / scale);
        let magnitude = norm(scaled);
        let length = scale * magnitude;
        if !length.is_finite() {
            return None;
        }
        let direction = scaled.map(|component| component / magnitude);
        self.cast_ray_with_breakables(origin, direction, length, category, breakables)
            .ok()
            .map(|hit| hit.is_none_or(|hit| hit.distance >= length))
    }

    /// A bounded query along a unit direction, distinguishing no hit from an
    /// unavailable ray. Both endpoints must fit the collision grid.
    pub fn checked_cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
    ) -> Option<Option<RayHit>> {
        self.checked_cast_ray_with_breakables(origin, direction, length, category, None)
    }

    /// A bounded query along a unit direction, skipping broken placements in `breakables`.
    pub fn checked_cast_ray_with_breakables(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
        breakables: Option<&BreakableState>,
    ) -> Option<Option<RayHit>> {
        if length <= 0.0
            || !length.is_finite()
            || direction == [0.0; 3]
            || !direction.into_iter().all(f64::is_finite)
        {
            return None;
        }
        let endpoint = std::array::from_fn(|axis| origin[axis] + direction[axis] * length);
        for point in [origin, endpoint] {
            if !point.into_iter().all(f64::is_finite) {
                return None;
            }
            cell(point[0])?;
            cell(point[2])?;
        }
        self.cast_ray_with_breakables(origin, direction, length, category, breakables)
            .ok()
    }

    /// First static hit along a unit direction. Triangles win exact ties.
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
    ) -> Result<Option<RayHit>, IndexError> {
        self.cast_ray_with_breakables(origin, direction, length, category, None)
    }

    /// First static hit along a unit direction, skipping placements marked broken in `breakables`.
    pub fn cast_ray_with_breakables(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
        breakables: Option<&BreakableState>,
    ) -> Result<Option<RayHit>, IndexError> {
        let Some(breakables) = breakables.filter(|_| self.has_breakables) else {
            return self.cast_ray_filtered(origin, direction, length, category, |_| true, |_| true);
        };
        self.cast_ray_filtered(
            origin,
            direction,
            length,
            category,
            |tri| {
                self.triangle_breakable_placement(tri)
                    .is_none_or(|placement| !breakables.is_broken(placement))
            },
            |round| {
                self.round_breakable_placement(round)
                    .is_none_or(|placement| !breakables.is_broken(placement))
            },
        )
    }

    /// Cast a weapon ray that pierces intact breakable panels and reports them before the first solid wall.
    pub fn cast_weapon_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
        breakables: Option<&BreakableState>,
    ) -> Result<WeaponRayHit, IndexError> {
        if !self.has_breakables {
            return Ok(WeaponRayHit {
                wall: self.cast_ray(origin, direction, length, category)?,
                breakables: Vec::new(),
            });
        }
        let wall = self.cast_ray_filtered(
            origin,
            direction,
            length,
            category,
            |tri| self.triangle_breakable_placement(tri).is_none(),
            |round| self.round_breakable_placement(round).is_none(),
        )?;
        let max_dist = wall.map_or(length, |hit| hit.distance);
        let mut hit_breakables = Vec::new();
        let mut skip_placements = HashSet::new();
        for _ in 0..16 {
            let Some(hit) = self.cast_ray_filtered(
                origin,
                direction,
                max_dist,
                category,
                |tri| match self.triangle_breakable_placement(tri) {
                    Some(placement) => {
                        !skip_placements.contains(&placement)
                            && !breakables.is_some_and(|state| state.is_broken(placement))
                    }
                    None => false,
                },
                |round| match self.round_breakable_placement(round) {
                    Some(placement) => {
                        !skip_placements.contains(&placement)
                            && !breakables.is_some_and(|state| state.is_broken(placement))
                    }
                    None => false,
                },
            )?
            else {
                break;
            };
            let Some(placement) = self.ray_hit_breakable_placement(hit) else {
                break;
            };
            if hit.distance < max_dist && skip_placements.insert(placement) {
                hit_breakables.push((hit.distance, placement));
            } else {
                break;
            }
        }
        hit_breakables.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        Ok(WeaponRayHit {
            wall,
            breakables: hit_breakables,
        })
    }

    /// Collect intact breakable placement indices whose breakable geometry overlaps `sphere`.
    pub fn breakables_in_sphere(
        &self,
        center: Vec3,
        radius: f64,
        breakables: Option<&BreakableState>,
    ) -> Vec<usize> {
        if !self.has_breakables
            || !radius.is_finite()
            || radius <= 0.0
            || !center.into_iter().all(f64::is_finite)
        {
            return Vec::new();
        }
        let r2 = radius * radius;
        let mut found = BTreeSet::new();
        for &(prop, source, breakable) in &self.round_props {
            if !breakable {
                continue;
            }
            let StaticRaySource::Placement { index, .. } = source else {
                continue;
            };
            if breakables.is_some_and(|state| state.is_broken(index)) || found.contains(&index) {
                continue;
            }
            if round_prop_near_sphere(prop, center, radius) {
                found.insert(index);
            }
        }
        for span in &self.breakable_tri_spans {
            if breakables.is_some_and(|state| state.is_broken(span.placement))
                || found.contains(&span.placement)
            {
                continue;
            }
            if !aabb_intersects_sphere(span.min, span.max, center, r2) {
                continue;
            }
            for tri_idx in span.start_tri..span.end_tri {
                if self
                    .triangles
                    .triangle_aabb_within_radius(tri_idx, center, r2)
                {
                    found.insert(span.placement);
                    break;
                }
            }
        }
        found.into_iter().collect()
    }

    fn cast_ray_filtered<FT, FR>(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
        allow_triangle: FT,
        allow_round: FR,
    ) -> Result<Option<RayHit>, IndexError>
    where
        FT: Fn(u32) -> bool,
        FR: Fn(usize) -> bool,
    {
        let mut hit = self.triangles.cast_ray_filtered(
            origin,
            direction,
            length,
            category,
            allow_triangle,
        )?;
        let mut best = hit.map_or(length, |hit| hit.distance);
        if !self.round_props.is_empty() {
            let inv_dir = std::array::from_fn(|a| {
                if direction[a].abs() > 1e-12 {
                    1.0 / direction[a]
                } else {
                    f64::INFINITY
                }
            });
            for (index, ((prop, _, _), &(bmin, bmax))) in self
                .round_props
                .iter()
                .zip(&self.round_prop_bounds)
                .enumerate()
            {
                if prop.mask & category == 0
                    || !ray_intersects_aabb(origin, direction, inv_dir, best, bmin, bmax)
                    || !allow_round(index)
                {
                    continue;
                }
                let Some(surface) = round_hit(origin, direction, best, prop.a, prop.b, prop.radius)
                else {
                    continue;
                };
                if hit.is_none() || surface.distance < best {
                    best = surface.distance;
                    hit = Some(RayHit {
                        distance: best,
                        point: std::array::from_fn(|axis| origin[axis] + best * direction[axis]),
                        normal: surface.normal,
                        mask: prop.mask,
                        surface: 0,
                        primitive: RayPrimitive::Round(index),
                    });
                }
            }
        }
        Ok(hit)
    }

    fn triangle_source_entry(&self, index: u32) -> Option<(StaticRaySource, bool)> {
        let span = self
            .triangle_sources
            .partition_point(|&(end, _, _)| end <= index as usize);
        self.triangle_sources
            .get(span)
            .map(|&(_, source, breakable)| (source, breakable))
    }

    fn triangle_breakable_placement(&self, index: u32) -> Option<usize> {
        let placement = *self.tri_breakable_placement.get(index as usize)?;
        (placement != u32::MAX).then_some(placement as usize)
    }

    fn round_breakable_placement(&self, index: usize) -> Option<usize> {
        let &(_, source, breakable) = self.round_props.get(index)?;
        if !breakable {
            return None;
        }
        match source {
            StaticRaySource::Placement { index, .. } => Some(index),
            _ => None,
        }
    }

    /// Resolve a hit from this scene to its source mesh, hull or map placement.
    pub fn ray_source(&self, hit: RayHit) -> Option<StaticRaySource> {
        match hit.primitive {
            RayPrimitive::Round(index) => self.round_props.get(index).map(|&(_, source, _)| source),
            RayPrimitive::Triangle(index) => {
                self.triangle_source_entry(index).map(|(source, _)| source)
            }
        }
    }

    /// Return the placement index if `hit` struck a breakable prop shape.
    pub fn ray_hit_breakable_placement(&self, hit: RayHit) -> Option<usize> {
        match hit.primitive {
            RayPrimitive::Round(index) => self.round_breakable_placement(index),
            RayPrimitive::Triangle(index) => self.triangle_breakable_placement(index),
        }
    }
}

fn model_triangles(
    model: &[Shape],
    surfaces: bool,
    analytic: bool,
) -> Result<Vec<(usize, Vec<SceneTriangle>)>, SceneError> {
    let mut slots = Vec::with_capacity(model.len());
    for (slot, shape) in model.iter().enumerate() {
        let mut triangles = Vec::new();
        match shape {
            Shape::Mesh { mesh, .. } => append_mesh(&mut triangles, mesh, surfaces)?,
            Shape::Hull(hull) => append_hull(&mut triangles, hull)?,
            _ if analytic => {}
            _ => {
                let round = RoundProp::from_shape(shape).expect("only round shapes remain");
                append_prism(&mut triangles, round)?;
            }
        }
        slots.push((slot, triangles));
    }
    Ok(slots)
}

fn append_mesh(
    out: &mut Vec<SceneTriangle>,
    mesh: &Mesh,
    surfaces: bool,
) -> Result<(), SceneError> {
    for triangle in &mesh.triangles {
        let mut vertices = [[0.0; 3]; 3];
        for (point, corner) in vertices.iter_mut().zip(triangle.corners) {
            *point = mesh
                .vertices
                .get(corner as usize)
                .ok_or(SceneError::InvalidGeometry(
                    "mesh corner index out of range",
                ))?
                .map(f64::from);
        }
        push(
            out,
            vertices,
            triangle.mask,
            if surfaces { triangle.surface } else { 0 },
        )?;
    }
    Ok(())
}

fn append_hull(out: &mut Vec<SceneTriangle>, hull: &Hull) -> Result<(), SceneError> {
    let edge_at = |number: u8| {
        hull.half_edges
            .get(usize::from(number))
            .ok_or(SceneError::InvalidGeometry("hull edge index out of range"))
    };
    let point_at = |number: u8| {
        hull.vertices
            .get(usize::from(number))
            .map(|point| point.map(f64::from))
            .ok_or(SceneError::InvalidGeometry(
                "hull vertex index out of range",
            ))
    };
    for &first in &hull.faces {
        let start = edge_at(first)?;
        if start.next == first {
            continue;
        }
        let a = point_at(start.origin)?;
        let second = edge_at(start.next)?;
        let mut b = point_at(second.origin)?;
        let mut next = second.next;
        for _ in 2..hull.half_edges.len().min(256) {
            if next == first {
                break;
            }
            let edge = edge_at(next)?;
            let c = point_at(edge.origin)?;
            push(out, [a, b, c], hull.mask, 0)?;
            b = c;
            next = edge.next;
        }
        if next != first {
            return Err(SceneError::InvalidGeometry("hull face loop does not close"));
        }
    }
    Ok(())
}

fn push(
    out: &mut Vec<SceneTriangle>,
    vertices: [Vec3; 3],
    mask: u16,
    surface: u16,
) -> Result<(), SceneError> {
    if out.len() >= MAX_SCENE_SHAPES {
        return Err(SceneError::ExpansionLimit);
    }
    out.push(SceneTriangle {
        vertices,
        mask,
        surface,
    });
    Ok(())
}

fn round_prop_near_sphere(prop: RoundProp, center: Vec3, radius: f64) -> bool {
    let ab: Vec3 = std::array::from_fn(|i| prop.b[i] - prop.a[i]);
    let ab_len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    let t = if ab_len2 <= 1e-12 {
        0.0
    } else {
        let ac: Vec3 = std::array::from_fn(|i| center[i] - prop.a[i]);
        ((ac[0] * ab[0] + ac[1] * ab[1] + ac[2] * ab[2]) / ab_len2).clamp(0.0, 1.0)
    };
    let closest: Vec3 = std::array::from_fn(|i| prop.a[i] + t * ab[i]);
    let dist2: f64 = (0..3)
        .map(|i| {
            let d = center[i] - closest[i];
            d * d
        })
        .sum();
    let total = radius + prop.radius;
    dist2 <= total * total
}

fn aabb_intersects_sphere(min: Vec3, max: Vec3, center: Vec3, r2: f64) -> bool {
    let mut dist2 = 0.0;
    for axis in 0..3 {
        let clamped = center[axis].clamp(min[axis], max[axis]);
        let d = center[axis] - clamped;
        dist2 += d * d;
    }
    dist2 <= r2
}

fn ray_intersects_aabb(
    origin: Vec3,
    direction: Vec3,
    inv_dir: Vec3,
    max_t: f64,
    min: Vec3,
    max: Vec3,
) -> bool {
    let mut tmin = 0.0_f64;
    let mut tmax = max_t;
    for axis in 0..3 {
        if direction[axis].abs() <= 1e-12 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return false;
            }
        } else {
            let t1 = (min[axis] - origin[axis]) * inv_dir[axis];
            let t2 = (max[axis] - origin[axis]) * inv_dir[axis];
            tmin = tmin.max(t1.min(t2));
            tmax = tmax.min(t1.max(t2));
            if tmax < tmin {
                return false;
            }
        }
    }
    true
}

#[derive(Clone, Copy)]
struct Transform {
    translation: Vec3,
    scale: Vec3,
    rotation: [f64; 4],
}

impl From<Placement> for Transform {
    fn from(placement: Placement) -> Self {
        Self {
            translation: placement.translation.map(f64::from),
            scale: placement.scale.map(f64::from),
            rotation: placement.rotation.map(f64::from),
        }
    }
}

impl Transform {
    fn place(self, point: Vec3) -> Vec3 {
        let [x, y, z, w] = self.rotation;
        let [vx, vy, vz] = std::array::from_fn(|axis| point[axis] * self.scale[axis]);
        let tx = 2.0 * (y * vz - z * vy);
        let ty = 2.0 * (z * vx - x * vz);
        let tz = 2.0 * (x * vy - y * vx);
        let rotated = [
            vx + w * tx + (y * tz - z * ty),
            vy + w * ty + (z * tx - x * tz),
            vz + w * tz + (x * ty - y * tx),
        ];
        std::array::from_fn(|axis| rotated[axis] + self.translation[axis])
    }
}

// Python 3.12+ compensates these three-term sums in props.prism.
fn sum(terms: Vec3) -> f64 {
    let mut total: f64 = 0.0;
    let mut error = 0.0;
    for term in terms {
        let next = total + term;
        error += if total.abs() >= term.abs() {
            (total - next) + term
        } else {
            (term - next) + total
        };
        total = next;
    }
    total + error
}

fn append_prism(out: &mut Vec<SceneTriangle>, round: RoundProp) -> Result<(), SceneError> {
    let RoundProp { a, b, radius, mask } = round;
    let axis: Vec3 = std::array::from_fn(|axis| b[axis] - a[axis]);
    let length = sum(axis.map(|axis| axis * axis)).sqrt();
    let axis = if length > 1e-6 {
        axis.map(|value| value / length)
    } else {
        [0.0, 1.0, 0.0]
    };
    let a: Vec3 = std::array::from_fn(|i| a[i] - axis[i] * radius);
    let b: Vec3 = std::array::from_fn(|i| b[i] + axis[i] * radius);
    let u = if axis[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let dot = sum(std::array::from_fn(|i| u[i] * axis[i]));
    let u = std::array::from_fn(|i| u[i] - axis[i] * dot);
    let norm = sum(u.map(|i| i * i)).sqrt();
    let u = u.map(|i| i / norm);
    let v = [
        axis[1] * u[2] - axis[2] * u[1],
        axis[2] * u[0] - axis[0] * u[2],
        axis[0] * u[1] - axis[1] * u[0],
    ];
    let reach = radius / (std::f64::consts::PI / PRISM_SIDES as f64).cos();
    let mut ring_a = [[0.0; 3]; PRISM_SIDES];
    let mut ring_b = [[0.0; 3]; PRISM_SIDES];
    for k in 0..PRISM_SIDES {
        let angle = std::f64::consts::TAU * k as f64 / PRISM_SIDES as f64;
        let (cos, sin) = (angle.cos(), angle.sin());
        let offset: Vec3 = std::array::from_fn(|i| reach * (cos * u[i] + sin * v[i]));
        ring_a[k] = std::array::from_fn(|i| a[i] + offset[i]);
        ring_b[k] = std::array::from_fn(|i| b[i] + offset[i]);
    }
    for k in 0..PRISM_SIDES {
        let j = (k + 1) % PRISM_SIDES;
        push(out, [ring_a[k], ring_a[j], ring_b[j]], mask, 0)?;
        push(out, [ring_a[k], ring_b[j], ring_b[k]], mask, 0)?;
    }
    for k in 1..PRISM_SIDES - 1 {
        push(out, [ring_a[0], ring_a[k], ring_a[k + 1]], mask, 0)?;
        push(out, [ring_b[0], ring_b[k + 1], ring_b[k]], mask, 0)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision_cache::{HalfEdge, RAY_GEOMETRY, RAY_PROP_SCOPE, Triangle, chunk_guid};
    use serde_json::json;

    const MAP: u64 = 0x0800_0000_0000_0688;
    const MODE_PROP: u64 = 0x2345;
    const TEAM0: u16 = 0x08;
    const TEAM1: u16 = 0x10;
    const EXTRA: u16 = 0x80;
    const ORIGIN: Vec3 = [0.0; 3];
    const FORWARD: Vec3 = [0.0, 0.0, 1.0];

    fn identity() -> RayCacheIdentity<'static> {
        RayCacheIdentity {
            map_guid: MAP,
            collision_ckey: "synthetic",
            placement_ckey: None,
            prop_records_sha256: None,
        }
    }

    fn wall(x: f32, z: f32, mask: u16, surface: u16) -> Mesh {
        Mesh {
            vertices: vec![[x - 1.0, -1.0, z], [x + 1.0, -1.0, z], [x, 1.0, z]],
            triangles: vec![Triangle {
                corners: [0, 1, 2],
                wings: [-1; 3],
                mask,
                surface,
            }],
        }
    }

    fn placement(group: u16, scale: [f32; 3]) -> Placement {
        Placement {
            model: 0,
            group,
            translation: [0.0, 0.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale,
        }
    }

    fn fixture(expanded: bool) -> CollisionCache {
        let mut header = json!({
            "meshes": [[3, 1], [3, 1]], "hulls": 0, "models": 1, "placements": 1,
            "identifiers": [format!("{MODE_PROP:X}")],
        });
        if expanded {
            header["geometry"] = json!(RAY_GEOMETRY);
            header["category_mask"] = json!(0xffff);
            header["prop_scope"] = json!(RAY_PROP_SCOPE);
            header["map"] = json!(format!("0x{MAP:016X}"));
            header["chunk"] = json!(format!("0x{:016X}", chunk_guid(MAP)));
            header["ckey"] = json!("synthetic");
        }
        CollisionCache {
            header: serde_json::from_value(header).unwrap(),
            meshes: vec![wall(0.0, 1.0, TEAM0, 7), wall(0.0, 5.0, TEAM1, 11)],
            hulls: vec![],
            models: vec![vec![
                Shape::Sphere {
                    center: [0.0; 3],
                    radius: 1.0,
                    mask: TEAM1,
                },
                Shape::Mesh {
                    mask: TEAM1,
                    mesh: wall(2.0, 0.0, EXTRA, 23),
                },
            ]],
            placements: vec![placement(1, [2.0; 3])],
        }
    }

    #[test]
    fn segment_visibility_reports_unaddressable_or_over_budget_rays() {
        let scene = CollisionScene::movement_probe(&fixture(false), &BTreeSet::new()).unwrap();
        let outside = [f64::MAX, 0.0, 0.0];
        assert_eq!(scene.segment_clear(ORIGIN, outside, TEAM0), None);
        assert_eq!(scene.segment_clear(outside, ORIGIN, TEAM0), None);
        assert_eq!(scene.segment_clear(outside, outside, TEAM0), None);
        assert_eq!(
            scene.segment_clear([10.0, 0.0, 0.0], [10.0, 0.0, 1_000_000.0], TEAM0),
            None
        );
    }

    #[test]
    fn checked_ray_distinguishes_bounded_hits_misses_and_unavailable_queries() {
        let scene =
            CollisionScene::expanded_rays(&fixture(true), &BTreeSet::from([MODE_PROP]), identity())
                .unwrap();
        let hit = scene
            .checked_cast_ray(ORIGIN, FORWARD, 1.0, TEAM1)
            .unwrap()
            .unwrap();
        assert_eq!(hit.distance, 1.0);
        assert_eq!(hit.point, [0.0, 0.0, 1.0]);
        assert_eq!(hit.normal, [0.0, 0.0, -1.0]);
        assert_eq!(
            scene.checked_cast_ray(ORIGIN, FORWARD, 0.5, TEAM1),
            Some(None)
        );
        for (origin, direction, length) in [
            (ORIGIN, FORWARD, 0.0),
            (ORIGIN, FORWARD, f64::NAN),
            (ORIGIN, [0.0; 3], 1.0),
            (ORIGIN, [f64::INFINITY, 0.0, 0.0], 1.0),
            ([f64::MAX, 0.0, 0.0], FORWARD, 1.0),
            (ORIGIN, [1.0, 0.0, 0.0], f64::MAX),
            ([10.0, 0.0, 0.0], FORWARD, 1_000_000.0),
        ] {
            assert_eq!(
                scene.checked_cast_ray(origin, direction, length, TEAM1),
                None
            );
        }
    }

    #[test]
    fn expanded_scene_checks_provenance_filters_groups_and_prefers_triangle_ties() {
        let cache = fixture(true);
        assert!(matches!(
            CollisionScene::movement_probe(&cache, &BTreeSet::new()),
            Err(SceneError::Cache(_))
        ));
        let mut wrong = identity();
        wrong.collision_ckey = "wrong";
        assert!(matches!(
            CollisionScene::expanded_rays(&cache, &BTreeSet::new(), wrong),
            Err(SceneError::Cache(_))
        ));

        let baseline = CollisionScene::expanded_rays(&cache, &BTreeSet::new(), identity()).unwrap();
        let core = baseline
            .cast_ray(ORIGIN, FORWARD, 8.0, TEAM1)
            .unwrap()
            .unwrap();
        assert_eq!((core.distance, core.surface), (5.0, 11));
        assert_eq!(
            baseline.ray_source(core),
            Some(StaticRaySource::Mesh { index: 1 })
        );

        let mode = CollisionScene::expanded_rays(&cache, &BTreeSet::from([MODE_PROP]), identity())
            .unwrap();
        let round = mode.cast_ray(ORIGIN, FORWARD, 8.0, TEAM1).unwrap().unwrap();
        assert_eq!(
            (round.distance, round.point, round.normal, round.surface),
            (1.0, FORWARD, [0.0, 0.0, -1.0], 0)
        );
        let placed = Some(StaticRaySource::Placement {
            index: 0,
            model: 0,
            group: 1,
        });
        assert_eq!(mode.ray_source(round), placed);
        let tie = mode
            .cast_ray(ORIGIN, FORWARD, 8.0, TEAM0 | TEAM1)
            .unwrap()
            .unwrap();
        assert_eq!((tie.distance, tie.surface), (1.0, 7));
        assert_eq!(
            mode.ray_source(tie),
            Some(StaticRaySource::Mesh { index: 0 })
        );
        let mesh = mode
            .cast_ray([4.0, 0.0, 0.0], FORWARD, 8.0, EXTRA)
            .unwrap()
            .unwrap();
        assert_eq!((mesh.distance, mesh.mask, mesh.surface), (3.0, EXTRA, 23));
        assert_eq!(mode.ray_source(mesh), placed);

        use crate::{contact_gjk::sweep::MeshProbe, contact_world::ContactWorld};
        let fixture_data: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../fixtures/behavior/map-combat-ray-geometry-v1.json"
        )))
        .unwrap();
        let sweep_contract = &fixture_data["placed_mesh_sweep"];
        let category = sweep_contract["query_category"].as_u64().unwrap() as u16;
        let excluded = sweep_contract["excluded_category"].as_u64().unwrap() as u16;
        let radius = sweep_contract["radius"].as_f64().unwrap() as f32;
        let identifiers = BTreeSet::from([MODE_PROP]);
        let sweep = |world: &ContactWorld, category| {
            world
                .projectile_sweep(MeshProbe {
                    points: &[[0.0; 3]],
                    radius,
                    start: [4.0, 0.0, 0.0],
                    end: [4.0, 0.0, 8.0],
                    category,
                })
                .unwrap()
        };
        let expanded =
            ContactWorld::from_expanded_ray_cache(cache, &identifiers, identity()).unwrap();
        assert_eq!(category, mesh.mask);
        let hit = sweep(&expanded, category).expect("same placed face as the point ray");
        assert_eq!(hit.triangle, Some(0));
        assert!(sweep(&expanded, excluded).is_none());
        let movement = ContactWorld::from_movement_cache(fixture(false), &identifiers).unwrap();
        assert!(sweep(&movement, category).is_none());
    }

    #[test]
    fn expanded_sphere_keeps_its_world_radius_at_a_scaled_placement() {
        use crate::{contact_gjk::sweep::MeshProbe, contact_world::ContactWorld};

        let fixture_data: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../fixtures/behavior/map-combat-ray-geometry-v1.json"
        )))
        .unwrap();
        let contract = &fixture_data["scaled_placed_mesh_sweep"];
        let mut cache = fixture(true);
        cache.placements[0].scale = serde_json::from_value(contract["scale"].clone()).unwrap();
        let radius = serde_json::from_value(contract["radius"].clone()).unwrap();
        let mode = BTreeSet::from([MODE_PROP]);
        let rays = CollisionScene::expanded_rays(&cache, &mode, identity()).unwrap();
        let sweeps = ContactWorld::from_expanded_ray_cache(cache, &mode, identity()).unwrap();

        for path in contract["paths"].as_array().unwrap() {
            let origin: [f32; 3] = serde_json::from_value(path["origin"].clone()).unwrap();
            let ray = rays
                .cast_ray(origin.map(f64::from), FORWARD, 8.0, EXTRA)
                .unwrap();
            let sweep = sweeps
                .projectile_sweep(MeshProbe {
                    points: &[[0.0; 3]],
                    radius,
                    start: origin,
                    end: [origin[0], origin[1], 8.0],
                    category: EXTRA,
                })
                .unwrap();
            assert_eq!(
                ray.is_some(),
                path["point_blocked"].as_bool().unwrap(),
                "{origin:?}"
            );
            assert_eq!(
                sweep.is_some(),
                path["sphere_blocked"].as_bool().unwrap(),
                "{origin:?}"
            );
        }
    }

    #[test]
    fn movement_and_nonuniform_placement_keep_prisms_and_movement_strips_prop_surfaces() {
        let mode = BTreeSet::from([MODE_PROP]);
        let probe = CollisionScene::movement_probe(&fixture(false), &mode).unwrap();
        let round = probe
            .cast_ray(ORIGIN, FORWARD, 8.0, TEAM1)
            .unwrap()
            .unwrap();
        assert!((round.distance - 0.929_447_639_179_834_1).abs() < 2e-15);
        let mesh = probe
            .cast_ray([4.0, 0.0, 0.0], FORWARD, 8.0, EXTRA)
            .unwrap()
            .unwrap();
        assert_eq!((mesh.distance, mesh.surface), (3.0, 0));

        let mut cache = fixture(true);
        cache.placements[0].scale[0] = 1.0;
        let expanded = CollisionScene::expanded_rays(&cache, &mode, identity()).unwrap();
        let nonuniform = expanded
            .cast_ray(ORIGIN, FORWARD, 8.0, TEAM1)
            .unwrap()
            .unwrap();
        assert_eq!(nonuniform.distance, round.distance);
    }

    #[test]
    fn hull_face_fan_preserves_order_and_rejects_a_nonclosing_cycle() {
        let mut hull = Hull {
            mask: TEAM1,
            centroid: [1.0, 0.0, 1.0],
            vertices: vec![
                [0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [2.0, 0.0, 2.0],
                [0.0, 0.0, 2.0],
            ],
            planes: vec![[0.0, 1.0, 0.0, 0.0]],
            faces: vec![0],
            half_edges: (0..4)
                .map(|origin| HalfEdge {
                    twin: -1,
                    origin,
                    face: 0,
                    next: (origin + 1) % 4,
                })
                .collect(),
        };
        let mut out = Vec::new();
        append_hull(&mut out, &hull).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].vertices,
            [hull.vertices[0], hull.vertices[1], hull.vertices[2]]
                .map(|point| point.map(f64::from))
        );
        assert_eq!(
            out[1].vertices,
            [hull.vertices[0], hull.vertices[2], hull.vertices[3]]
                .map(|point| point.map(f64::from))
        );
        assert_eq!((out[0].mask, out[0].surface), (TEAM1, 0));
        hull.half_edges[3].next = 2;
        assert!(matches!(
            append_hull(&mut Vec::new(), &hull),
            Err(SceneError::InvalidGeometry(_))
        ));
    }

    #[test]
    fn placement_applies_scale_xyzw_rotation_and_translation_in_reference_order() {
        let transform = Transform::from(Placement {
            translation: [4.0, 5.0, 6.0],
            scale: [2.0, 3.0, 4.0],
            rotation: [
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            ..placement(0, [1.0; 3])
        });
        assert_eq!(
            transform.place([1.0, 2.0, 3.0]),
            [15.999_999_657_714_582, 11.0, 4.000_000_479_199_585]
        );
    }

    #[test]
    fn weapon_ray_pierces_breakable_panels_until_solid_wall_and_tracks_broken_state() {
        let mut cache = fixture(true);
        cache.header.breakable_model_shapes.insert(0, vec![0]);
        cache.meshes = vec![wall(0.0, 6.0, TEAM0, 7)];
        cache.models = vec![vec![
            Shape::Mesh {
                mesh: wall(0.0, 0.0, TEAM0, 1),
                mask: TEAM0,
            },
            Shape::Mesh {
                mesh: wall(3.0, 0.5, TEAM0, 2),
                mask: TEAM0,
            },
        ]];
        cache.placements = vec![placement(0, [1.0; 3])];
        let scene = CollisionScene::expanded_rays(&cache, &BTreeSet::new(), identity()).unwrap();
        let state = BreakableState::new();

        let hit = scene
            .cast_weapon_ray(ORIGIN, FORWARD, 10.0, TEAM0, Some(&state))
            .unwrap();
        assert_eq!(hit.breakables, vec![(3.0, 0)]);
        let solid = hit.wall.expect("solid wall behind breakable");
        assert!((solid.distance - 6.0).abs() < 1e-6);

        assert!(state.mark_broken(0));
        assert!(!state.mark_broken(0));
        assert_eq!(state.broken_count(), 1);

        let after_break = scene
            .cast_weapon_ray(ORIGIN, FORWARD, 10.0, TEAM0, Some(&state))
            .unwrap();
        assert!(after_break.breakables.is_empty());
        assert!((after_break.wall.unwrap().distance - 6.0).abs() < 1e-6);

        let ray = scene
            .cast_ray_with_breakables(ORIGIN, FORWARD, 10.0, TEAM0, Some(&state))
            .unwrap()
            .unwrap();
        assert!((ray.distance - 6.0).abs() < 1e-6);
    }
}

//! Placed static probe and ray geometry for retail build 104319.
//!
//! Ports `probe_triangles` from `ow174/game/collision.py` and the shape
//! triangulation and placement math from `ow174/game/props.py`. Movement
//! contacts have their own f32 representation and solver.

use std::collections::{BTreeSet, HashMap, hash_map::Entry};
use std::error::Error;
use std::fmt;

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

#[derive(Debug)]
pub struct CollisionScene {
    triangles: TriangleIndex,
    triangle_sources: Vec<(usize, StaticRaySource)>,
    round_props: Vec<(RoundProp, StaticRaySource)>,
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
            triangle_sources.push((triangles.len(), StaticRaySource::Mesh { index }));
        }
        for (index, hull) in cache.hulls.iter().enumerate() {
            append_hull(&mut triangles, hull)?;
            triangle_sources.push((triangles.len(), StaticRaySource::Hull { index }));
        }
        let mut local_models = HashMap::new();
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
            if triangles.len() + local.len() > MAX_SCENE_SHAPES {
                return Err(SceneError::ExpansionLimit);
            }
            let source = StaticRaySource::Placement {
                index,
                model: placement.model,
                group: placement.group,
            };
            triangles.extend(local.iter().map(|triangle| SceneTriangle {
                vertices: triangle.vertices.map(|point| transform.place(point)),
                ..*triangle
            }));
            if !local.is_empty() {
                triangle_sources.push((triangles.len(), source));
            }
            if analytic {
                for shape in model.iter().filter_map(RoundProp::from_shape) {
                    if round_props.len() >= MAX_SCENE_SHAPES {
                        return Err(SceneError::ExpansionLimit);
                    }
                    round_props.push((shape.placed(transform), source));
                }
            }
        }
        Ok(Self {
            triangles: TriangleIndex::new(triangles)?,
            triangle_sources,
            round_props,
        })
    }

    pub fn triangles(&self) -> &TriangleIndex {
        &self.triangles
    }

    /// Whether geometry leaves the segment clear before its target endpoint.
    /// None means the endpoints or the ray traversal are outside the query domain.
    pub fn segment_clear(&self, origin: Vec3, target: Vec3, category: u16) -> Option<bool> {
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
        self.cast_ray(origin, direction, length, category)
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
        self.cast_ray(origin, direction, length, category).ok()
    }

    /// First static hit along a unit direction. Triangles win exact ties.
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        length: f64,
        category: u16,
    ) -> Result<Option<RayHit>, IndexError> {
        let mut hit = self
            .triangles
            .cast_ray(origin, direction, length, category)?;
        let mut best = hit.map_or(length, |hit| hit.distance);
        for (index, (prop, _)) in self.round_props.iter().enumerate() {
            if prop.mask & category == 0 {
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
        Ok(hit)
    }

    /// Resolve a hit from this scene to its source mesh, hull or map placement.
    pub fn ray_source(&self, hit: RayHit) -> Option<StaticRaySource> {
        match hit.primitive {
            RayPrimitive::Round(index) => self.round_props.get(index).map(|&(_, source)| source),
            RayPrimitive::Triangle(index) => {
                let span = self
                    .triangle_sources
                    .partition_point(|&(end, _)| end <= index as usize);
                self.triangle_sources.get(span).map(|&(_, source)| source)
            }
        }
    }
}

fn model_triangles(
    model: &[Shape],
    surfaces: bool,
    analytic: bool,
) -> Result<Vec<SceneTriangle>, SceneError> {
    let mut triangles = Vec::new();
    for shape in model {
        match shape {
            Shape::Mesh { mesh, .. } => append_mesh(&mut triangles, mesh, surfaces)?,
            Shape::Hull(hull) => append_hull(&mut triangles, hull)?,
            _ if analytic => {}
            _ => {
                let round = RoundProp::from_shape(shape).expect("only round shapes remain");
                append_prism(&mut triangles, round)?;
            }
        }
    }
    Ok(triangles)
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
}

//! Authored view selection fields on the native TrackTargets state.

use serde_json::Value as Json;

use crate::script_expr::{Variable, lvalue, native_fields, native_shape};

use super::{Node, asset_guid, neutral_flag, valid_output_plug};

pub(crate) struct ViewTrack<'a> {
    pub(crate) origin: &'a Json,
    pub(crate) offset: &'a Json,
    pub(crate) range: &'a Json,
    pub(crate) aperture: &'a Json,
    pub(crate) limit: &'a Json,
    pub(crate) output: Variable,
    pub(crate) profile: u64,
    pub(crate) masks: [u64; 3],
    pub(crate) ranking: &'a Json,
    pub(crate) period: u32,
}

impl Node {
    pub(crate) fn view_track(&self) -> Option<ViewTrack<'_>> {
        if self.class != "STUStatescriptStateTrackTargets"
            || !self.runs_on_server()
            || !self.is_networked()
            || self.state.is_none()
            || self.remote != -1
            || self.state_behavior.is_some()
            || self.action_behavior.is_some()
        {
            return None;
        }
        let f = self.fields.as_object()?;
        if !f.iter().all(|(key, value)| match key.as_str() {
            "m_0297060F" | "m_08285CD6" | "m_1C2A9945" | "m_2BBEEAB8" | "m_32AFE9AA"
            | "m_95FFC3C5" | "m_E4A295CE" => value.as_u64() == Some(1),
            "m_43478CAD" => value.as_u64() == Some(3),
            "m_05BE62DD" | "m_3D667CE3" | "m_84E5DE4E" | "m_8A2905A0" | "m_8DCA77B6"
            | "m_A30B383E" | "m_C39691D9" | "m_DD792FF0" | "m_filter" | "m_radius" => true,
            "m_transitionPlug" => value
                .as_array()
                .is_some_and(|plugs| plugs.iter().all(valid_output_plug)),
            "m_F198FD3A" | "m_beginPlug" | "m_onBeginPlug" | "m_onEndPlug" | "m_subgraphPlug" => {
                value.is_null()
                    || valid_output_plug(value)
                        && value["links"].as_array().is_some_and(Vec::is_empty)
            }
            // Unknown non-neutral settings must not silently change query semantics.
            _ => neutral_flag(value),
        }) || [
            "m_0297060F",
            "m_08285CD6",
            "m_1C2A9945",
            "m_2BBEEAB8",
            "m_32AFE9AA",
            "m_95FFC3C5",
            "m_E4A295CE",
            "m_43478CAD",
        ]
        .iter()
        .any(|key| !f.contains_key(*key))
        {
            return None;
        }
        let filter = f.get("m_filter")?;
        if filter["$"] != "STUConfigVarFilter"
            || filter["m_89831602"] != 4
            || !filter.as_object()?.iter().all(|(key, value)| {
                matches!(
                    key.as_str(),
                    "$" | "m_89831602" | "m_84065FD1" | "m_1E14CCA8" | "m_4EA686DB"
                ) || neutral_flag(value)
            })
        {
            return None;
        }
        let sight = f.get("m_3D667CE3")?;
        if sight["$"] != "STUConfigVarFilter"
            || !sight.as_object()?.iter().all(|(key, value)| {
                matches!(key.as_str(), "$" | "m_C1903771") || neutral_flag(value)
            })
        {
            return None;
        }
        let config = &sight["m_C1903771"];
        if config["$"] != "STU_20DB8AB5"
            || !native_fields(config, &["m_C1903771", "m_resourceKey"])
            || !config["m_resourceKey"].is_null()
        {
            return None;
        }
        let profile = asset_guid(&config["m_C1903771"]).filter(|guid| guid >> 48 == 0x0690)?;
        let ranking = f.get("m_84E5DE4E")?;
        if ranking["$"] != "STU_B61DA2A5" || !ranking["m_503F0304"].is_array() {
            return None;
        }
        let origin = f.get("m_8DCA77B6")?;
        if !native_shape(origin, "STU_B5A0CAF0", None) {
            return None;
        }
        let period =
            *[1, 3, 6, 12, 30].get(usize::try_from(f.get("m_DD792FF0")?.as_u64()?).ok()?)?;
        Some(ViewTrack {
            origin,
            offset: f.get("m_05BE62DD")?,
            range: f.get("m_radius")?,
            aperture: f.get("m_A30B383E")?,
            limit: f.get("m_C39691D9")?,
            output: lvalue(f.get("m_8A2905A0")?).ok().flatten()?,
            profile,
            masks: [
                filter["m_84065FD1"].as_u64()?,
                filter["m_1E14CCA8"].as_u64()?,
                filter["m_4EA686DB"].as_u64()?,
            ],
            ranking,
            period,
        })
    }
}

use std::{
    env,
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    game_debug::summarize_script_snapshot,
    script_init::{Issue, NodeObservations, Snapshot},
};

pub(super) struct Capture {
    path: PathBuf,
    bundle_sha256: String,
    source_revision: String,
}

impl Capture {
    pub(super) fn from_env(runtime: &Path) -> Option<Self> {
        let path = PathBuf::from(env::var_os("ORPHEUS_SCRIPT_COVERAGE")?);
        assert!(!path.try_exists().unwrap(), "coverage output must be new");
        let source_revision = env::var("ORPHEUS_SCRIPT_COVERAGE_REVISION")
            .expect("coverage capture requires its source revision");
        assert!(
            source_revision.len() == 40 && source_revision.bytes().all(|b| b.is_ascii_hexdigit()),
            "coverage source revision must be a full Git SHA-1"
        );
        let mut file = File::open(runtime.join("data/game_graphs_174.json.gz")).unwrap();
        let mut digest = Sha256::new();
        io::copy(&mut file, &mut digest).unwrap();
        Some(Self {
            path,
            bundle_sha256: format!("{:x}", digest.finalize()),
            source_revision: source_revision.to_ascii_lowercase(),
        })
    }

    pub(super) fn save(self, hero: u64, frames: u32, snapshot: &Snapshot) {
        let observed = snapshot
            .node_observations()
            .expect("coverage build collects node observations");
        let summary = summarize_script_snapshot(snapshot.entity, "coverage", snapshot)
            .node_observations
            .unwrap();
        let site_count = observed.sites().len();
        let response_limit = NodeObservations::DEFAULT_SITE_LIMIT;
        assert_eq!(summary.sites.len(), site_count.min(response_limit));
        assert_eq!(
            summary.truncated,
            observed.truncated() || site_count > response_limit
        );
        let observations: Vec<_> = observed
            .sites()
            .map(|(graph, node, client_scope_skipped)| {
                json!({"graph": graph, "node": node, "client_scope_skipped": client_scope_skipped})
            })
            .collect();
        let mut sites = Vec::new();
        for diagnostic in &snapshot.diagnostics {
            let Some(node) = diagnostic.site.node else {
                continue;
            };
            let kind = if diagnostic.registration_rejected {
                "registration_rejection"
            } else if matches!(
                diagnostic.issue,
                Issue::UnsupportedAction(_)
                    | Issue::UnsupportedState(_)
                    | Issue::UnsupportedMessageRegistration
            ) {
                "unsupported"
            } else {
                continue;
            };
            sites.push((diagnostic.site.graph, node, kind));
        }
        sites.sort_unstable();
        sites.dedup();
        let diagnostics: Vec<_> = sites
            .into_iter()
            .map(|(graph, node, kind)| json!({"graph": graph, "node": node, "kind": kind}))
            .collect();
        let scenario = if cfg!(feature = "script-coverage") {
            "owner-probe-900-frames-extended-v1"
        } else {
            "owner-probe-900-frames-v1"
        };
        let value = json!({
            "schema_version": 1,
            "scenario": scenario,
            "hero": format!("0x{hero:016X}"),
            "available": true,
            "frames": frames,
            "source_revision": self.source_revision,
            "graph_bundle_sha256": self.bundle_sha256,
            "truncated": observed.truncated(),
            "observations": observations,
            "diagnostics": diagnostics,
        });
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut output = tempfile::NamedTempFile::new_in(parent).unwrap();
        serde_json::to_writer(output.as_file_mut(), &value).unwrap();
        writeln!(output).unwrap();
        output
            .persist_noclobber(self.path)
            .expect("coverage output must be new");
    }
}

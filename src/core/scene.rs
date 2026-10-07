//! Persistent spatial scene graph for Waynote and compositor integrations.
//!
//! Notes are still stored as Markdown files. This graph is the durable linkage
//! layer around them: it can place a note beside a DriftWM window, connect it to
//! a Chromium tab, collect objects into groups, and retain freehand scribbles.
//! Runtime compositor IDs are hints only; `ExternalIdentity` is the reattachable
//! key used by future DriftWM/browser adapters.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

use crate::platform::store::atomic_write;

pub const SCENE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    #[serde(default = "scene_version")]
    pub version: u32,
    #[serde(default)]
    pub objects: Vec<SceneObject>,
    #[serde(default)]
    pub groups: Vec<SceneGroup>,
    #[serde(default)]
    pub links: Vec<SceneLink>,
}

fn scene_version() -> u32 {
    SCENE_VERSION
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            version: SCENE_VERSION,
            objects: Vec::new(),
            groups: Vec::new(),
            links: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneObject {
    /// A Waynote ULID. This is the identity of the scene object, not a window
    /// handle; it remains stable when a compositor recreates a window.
    pub id: String,
    pub kind: ObjectKind,
    #[serde(default)]
    pub identity: Option<ExternalIdentity>,
    pub frame: SceneFrame,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub group_ids: Vec<String>,
}

fn default_opacity() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjectKind {
    /// A live DriftWM window. `compositor_id` is refreshed at runtime and is
    /// deliberately not the durable identity of this object.
    DriftWindow {
        app_id: String,
        title: String,
        #[serde(default)]
        compositor_id: Option<u64>,
    },
    /// A Chromium tab supplied by a browser adapter/extension. Chromium does
    /// not expose this identity through DriftWM IPC alone.
    ChromiumTab {
        profile: String,
        tab_id: String,
        url: String,
        title: String,
    },
    /// A Waynote Markdown note, keyed by its existing ULID.
    Note {
        note_id: String,
    },
    /// An image, including a `grim`/`slurp` screenshot saved as an attachment.
    Image {
        path: String,
    },
    /// A freehand stroke in canvas coordinates.
    Scribble {
        points: Vec<ScenePoint>,
    },
    File {
        path: String,
        title: String,
    },
    Uri {
        uri: String,
        title: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ExternalIdentity {
    DriftWindow { key: String },
    ChromiumTab { profile: String, key: String },
    WaynoteNote { note_id: String },
    File { path: String },
    Uri { uri: String },
}

impl ExternalIdentity {
    pub fn drift_window(key: impl Into<String>) -> Self {
        Self::DriftWindow { key: key.into() }
    }

    pub fn chromium_tab(profile: impl Into<String>, key: impl Into<String>) -> Self {
        Self::ChromiumTab {
            profile: profile.into(),
            key: key.into(),
        }
    }

    pub fn waynote_note(note_id: impl Into<String>) -> Self {
        Self::WaynoteNote {
            note_id: note_id.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SceneFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Placement {
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub layer: SceneLayer,
    #[serde(default)]
    pub z_index: i32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneLayer {
    #[default]
    Desktop,
    Top,
    Overlay,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScenePoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneGroup {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub member_ids: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneLink {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relation: LinkRelation,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkRelation {
    #[default]
    Related,
    References,
    AttachedTo,
    DependsOn,
}

impl Scene {
    pub fn object(&self, id: &str) -> Option<&SceneObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn object_mut(&mut self, id: &str) -> Option<&mut SceneObject> {
        self.objects.iter_mut().find(|object| object.id == id)
    }

    pub fn add_object(&mut self, object: SceneObject) -> bool {
        if self.object(&object.id).is_some() {
            return false;
        }
        self.objects.push(object);
        true
    }

    /// Add or refresh a live external object without changing its scene ID.
    /// Returns the existing/new scene object ID.
    pub fn upsert_external(&mut self, object: SceneObject) -> String {
        if let Some(identity) = object.identity.as_ref() {
            if let Some(existing) = self
                .objects
                .iter_mut()
                .find(|candidate| candidate.identity.as_ref() == Some(identity))
            {
                let id = existing.id.clone();
                existing.kind = object.kind;
                existing.frame = object.frame;
                existing.placement = object.placement;
                existing.opacity = object.opacity;
                existing.locked = object.locked;
                return id;
            }
        }
        let id = object.id.clone();
        let _ = self.add_object(object);
        id
    }

    pub fn add_group(&mut self, group: SceneGroup) -> bool {
        if self.groups.iter().any(|candidate| candidate.id == group.id) {
            return false;
        }
        if group
            .member_ids
            .iter()
            .any(|member_id| self.object(member_id).is_none())
        {
            return false;
        }
        for member_id in &group.member_ids {
            if let Some(object) = self.object_mut(member_id) {
                if !object.group_ids.contains(&group.id) {
                    object.group_ids.push(group.id.clone());
                }
            }
        }
        self.groups.push(group);
        true
    }

    pub fn add_link(&mut self, link: SceneLink) -> bool {
        if self.links.iter().any(|candidate| candidate.id == link.id)
            || self.object(&link.from).is_none()
            || self.object(&link.to).is_none()
        {
            return false;
        }
        self.links.push(link);
        true
    }
}

/// Load the scene sidecar. Missing files are an empty scene; malformed files
/// are returned as errors so callers can preserve the user's data.
pub fn load(path: &Path) -> io::Result<Scene> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid scene JSON: {error}"),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Scene::default()),
        Err(error) => Err(error),
    }
}

/// Serialize the scene and replace the sidecar atomically.
pub fn save(path: &Path, scene: &Scene) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_vec_pretty(scene)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    atomic_write(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn note(id: &str) -> SceneObject {
        SceneObject {
            id: id.to_owned(),
            kind: ObjectKind::Note {
                note_id: id.to_owned(),
            },
            identity: Some(ExternalIdentity::waynote_note(id)),
            frame: SceneFrame {
                x: 10.0,
                y: 20.0,
                width: 320.0,
                height: 240.0,
            },
            placement: Placement::default(),
            opacity: 1.0,
            locked: false,
            group_ids: Vec::new(),
        }
    }

    fn window(id: &str, compositor_id: u64) -> SceneObject {
        SceneObject {
            id: id.to_owned(),
            kind: ObjectKind::DriftWindow {
                app_id: "chromium".to_owned(),
                title: "Research".to_owned(),
                compositor_id: Some(compositor_id),
            },
            identity: Some(ExternalIdentity::drift_window("chromium:research")),
            frame: SceneFrame::default(),
            placement: Placement::default(),
            opacity: 1.0,
            locked: false,
            group_ids: Vec::new(),
        }
    }

    #[test]
    fn group_and_link_require_real_objects_and_persist() {
        let mut scene = Scene::default();
        assert!(scene.add_object(note("note-1")));
        assert!(scene.add_object(window("window-1", 42)));
        assert!(scene.add_group(SceneGroup {
            id: "group-1".to_owned(),
            title: "Research cluster".to_owned(),
            member_ids: vec!["note-1".to_owned(), "window-1".to_owned()],
            collapsed: false,
        }));
        assert!(scene.add_link(SceneLink {
            id: "link-1".to_owned(),
            from: "note-1".to_owned(),
            to: "window-1".to_owned(),
            relation: LinkRelation::References,
            label: Some("source".to_owned()),
        }));
        assert_eq!(scene.object("note-1").unwrap().group_ids, vec!["group-1"]);

        let path = tempdir().unwrap().path().join("state/scene.json");
        save(&path, &scene).unwrap();
        assert_eq!(load(&path).unwrap(), scene);
    }

    #[test]
    fn external_refresh_keeps_scene_id_and_links() {
        let mut scene = Scene::default();
        scene.add_object(window("scene-window", 10));
        scene.add_object(note("note-1"));
        scene.add_link(SceneLink {
            id: "link-1".to_owned(),
            from: "note-1".to_owned(),
            to: "scene-window".to_owned(),
            relation: LinkRelation::AttachedTo,
            label: None,
        });

        let mut refreshed = window("ignored-runtime-id", 99);
        refreshed.kind = ObjectKind::DriftWindow {
            app_id: "chromium".to_owned(),
            title: "Research (updated)".to_owned(),
            compositor_id: Some(99),
        };
        assert_eq!(scene.upsert_external(refreshed), "scene-window");
        assert_eq!(scene.objects.len(), 2);
        assert_eq!(scene.links[0].to, "scene-window");
        assert_eq!(
            scene.object("scene-window").unwrap().kind,
            ObjectKind::DriftWindow {
                app_id: "chromium".to_owned(),
                title: "Research (updated)".to_owned(),
                compositor_id: Some(99),
            }
        );
    }

    #[test]
    fn malformed_scene_is_not_silently_discarded() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scene.json");
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}

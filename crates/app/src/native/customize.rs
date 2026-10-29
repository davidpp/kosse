//! Central overrides are edited separately from the repository's read-only values.
mod fields;
mod form;
pub use form::view;
use kosse_core::{
    contract::SaveProfileRequest,
    model::Observation,
    profiles::{
        ProjectProfile,
        document::{DeclaredProtocol, LinkKind, LinkProfile, ServiceMatch, ServiceProfile},
        view::{ConfiguredService, ProfileSource},
    },
};
use rune::TextEdit;
use std::collections::BTreeMap;

pub struct Editor {
    pub root: String,
    name: TextEdit,
    inherited_name: String,
    services: Vec<ServiceEditor>,
    selected: Option<usize>,
    pub busy: bool,
    pub error: Option<String>,
    remove: bool,
    remove_service: Option<usize>,
}
struct ServiceEditor {
    id: TextEdit,
    name: TextEdit,
    cwd: TextEdit,
    port: TextEdit,
    probe: TextEdit,
    protocol: Option<DeclaredProtocol>,
    disabled: Option<bool>,
    match_override: bool,
    links_override: bool,
    links: Vec<LinkEditor>,
    inherited: Option<ConfiguredService>,
}
struct LinkEditor {
    label: TextEdit,
    url: TextEdit,
    kind: Option<LinkKind>,
}
impl Editor {
    pub fn new(root: String, observation: &Observation) -> Self {
        let metadata = observation.profiles.iter().find(|p| p.root == root);
        let name = metadata
            .filter(|p| p.name_source == Some(ProfileSource::Central))
            .and_then(|p| p.name.clone())
            .unwrap_or_default();
        let inherited_name = metadata
            .filter(|p| p.name_source == Some(ProfileSource::Repository))
            .and_then(|p| p.name.clone())
            .or_else(|| {
                observation
                    .projects
                    .iter()
                    .find(|p| p.root == root)
                    .map(|p| p.name.clone())
            })
            .unwrap_or_default();
        Self {
            root,
            name: TextEdit::new(name),
            inherited_name,
            services: metadata
                .into_iter()
                .flat_map(|m| &m.services)
                .map(ServiceEditor::from)
                .collect(),
            selected: None,
            busy: false,
            error: None,
            remove: false,
            remove_service: None,
        }
    }
    fn request(&self) -> Result<SaveProfileRequest, String> {
        let mut services = BTreeMap::new();
        for entry in &self.services {
            let profile = entry.profile()?;
            if profile == ServiceProfile::default() {
                continue;
            }
            let id = entry.id.text.trim();
            if id.is_empty() {
                return Err("Give each service an ID before saving.".into());
            }
            if services.insert(id.to_owned(), profile).is_some() {
                return Err(format!("Service ID “{id}” is used twice."));
            }
        }
        Ok(SaveProfileRequest {
            root: self.root.clone(),
            profile: Some(ProjectProfile {
                name: optional(&self.name),
                services,
                ..ProjectProfile::new()
            }),
        })
    }
    fn add(&mut self) {
        self.services.push(ServiceEditor::empty());
        self.selected = Some(self.services.len() - 1);
    }
}
fn optional(edit: &TextEdit) -> Option<String> {
    let text = edit.text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
impl ServiceEditor {
    fn empty() -> Self {
        Self {
            id: TextEdit::new(""),
            name: TextEdit::new(""),
            cwd: TextEdit::new(""),
            port: TextEdit::new(""),
            probe: TextEdit::new(""),
            protocol: None,
            disabled: None,
            match_override: false,
            links_override: false,
            links: Vec::new(),
            inherited: None,
        }
    }
    fn profile(&self) -> Result<ServiceProfile, String> {
        let port = self
            .match_override
            .then(|| optional(&self.port))
            .flatten()
            .map(|value| {
                value
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p > 0)
                    .ok_or_else(|| "Port must be between 1 and 65535.".to_owned())
            })
            .transpose()?;
        let matcher = self.match_override.then(|| ServiceMatch {
            cwd: optional(&self.cwd),
            port,
        });
        if matcher
            .as_ref()
            .is_some_and(|m| m.cwd.is_none() && m.port.is_none())
        {
            return Err("Set a directory or port for the matching rule.".into());
        }
        let links = self.links_override.then(|| {
            self.links
                .iter()
                .map(|l| LinkProfile {
                    label: l.label.text.trim().into(),
                    url: l.url.text.trim().into(),
                    kind: l.kind,
                    unknown: BTreeMap::new(),
                })
                .collect()
        });
        Ok(ServiceProfile {
            name: optional(&self.name),
            matcher,
            protocol: self.protocol,
            probe_path: optional(&self.probe),
            disabled: self.disabled,
            links,
            ..ServiceProfile::default()
        })
    }
}
impl From<&ConfiguredService> for ServiceEditor {
    fn from(entry: &ConfiguredService) -> Self {
        let local = |source| source == Some(ProfileSource::Central);
        let source = entry.sources;
        let matcher = entry.matcher.as_ref();
        Self {
            id: TextEdit::new(&entry.id),
            name: TextEdit::new(if local(source.name) {
                entry.name.as_deref().unwrap_or("")
            } else {
                ""
            }),
            cwd: TextEdit::new(matcher.and_then(|m| m.cwd.clone()).unwrap_or_default()),
            port: TextEdit::new(
                matcher
                    .and_then(|m| m.port)
                    .map_or_else(String::new, |p| p.to_string()),
            ),
            probe: TextEdit::new(if local(source.probe_path) {
                entry.probe_path.as_deref().unwrap_or("")
            } else {
                ""
            }),
            protocol: if local(source.protocol) {
                entry.protocol
            } else {
                None
            },
            disabled: local(source.disabled).then_some(entry.disabled),
            match_override: local(source.matcher),
            links_override: local(source.links),
            links: entry
                .links
                .iter()
                .map(|l| LinkEditor {
                    label: TextEdit::new(&l.label),
                    url: TextEdit::new(&l.url),
                    kind: Some(l.kind),
                })
                .collect(),
            inherited: Some(entry.clone()),
        }
    }
}

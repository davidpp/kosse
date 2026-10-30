//! The portable project profile: what a repository's `kosse.json` contains, and what the central
//! configuration stores per worktree root. [`crate::profiles::validate`] enforces the rules the
//! JSON Schema can only describe; the schema is editor help, the validator is the authority.

use std::{collections::BTreeMap, fmt, marker::PhantomData};

use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess},
};
use serde_json::Value;
use ts_rs::TS;

/// The profile format this build reads and writes. A newer one is refused, never guessed at.
pub const PROFILE_VERSION: u32 = 1;
/// The optional repository file, read only at a detected project root.
pub const REPO_FILE_NAME: &str = "kosse.json";
/// A service ID: what merges a central entry with a repository entry.
pub const SERVICE_ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$";
pub const MAX_SERVICES: usize = 64;
pub const MAX_LINKS: usize = 16;
pub const MAX_LABEL_CHARS: usize = 100;
pub const MAX_URL_BYTES: usize = 2048;
pub const MAX_PATH_BYTES: usize = 512;
pub const MAX_ENDPOINTS: usize = 64;
/// `match.process`: the kernel truncates process names to 31 bytes, so longer never matches.
pub const MAX_PROCESS_NAME_CHARS: usize = 64;

/// Optional metadata about one project (a worktree root). Nothing in it is executed, fetched or
/// applied to the system: it names services, narrows which processes they are, and declares
/// links and a health path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(optional_fields)]
#[schemars(title = "Kossé project profile")]
pub struct ProjectProfile {
    /// Editor metadata only: Kossé never resolves or fetches it.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = 2048))]
    pub schema: Option<String>,
    /// Profile format version; this build understands `1`.
    #[schemars(extend("const" = 1))]
    pub version: u32,
    /// Display name for the project, instead of its directory name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 100))]
    pub name: Option<String>,
    /// Services by ID. IDs are unique; a central entry overrides the repository entry with the
    /// same ID field by field.
    #[serde(
        default,
        deserialize_with = "unique_keys",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    #[schemars(extend(
        "propertyNames" = { "pattern": "^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$" },
        "maxProperties" = 64
    ))]
    pub services: BTreeMap<String, ServiceProfile>,
    /// Fields this build does not know, kept as they are when Kossé rewrites the profile.
    #[serde(flatten)]
    #[schemars(skip)]
    #[ts(skip)]
    pub unknown: BTreeMap<String, Value>,
}

/// One service of a project: usually one dev server, possibly several processes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(optional_fields)]
pub struct ServiceProfile {
    /// Display name, instead of the process name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 100))]
    pub name: Option<String>,
    /// Which of the project's processes this service is. Without it the service is only declared
    /// (links, name) and never matches a process.
    #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
    #[ts(rename = "match")]
    pub matcher: Option<ServiceMatch>,
    /// What the service speaks: a hint, never proof of health. Only in the central profile does
    /// `http`/`https` make an endpoint an HTTP probe candidate; in a repository file it is shown,
    /// never acted on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<DeclaredProtocol>,
    /// Friendly links. Replaces (never extends) the other source's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = 16))]
    pub links: Option<Vec<LinkProfile>>,
    /// Origin-relative health path requested by the HTTP probe instead of `/` on the service's
    /// summary endpoints (only the matched port, when `match` names one); an endpoint's own
    /// `probePath` wins. Requested only on a loopback endpoint of the user's own process where
    /// `GET /` already got an HTTP response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        length(max = 512),
        regex(
            pattern = r"^/(?:(?:[A-Za-z0-9._~!$&'()*+,;=:@-]|%[0-9A-Fa-f]{2})(?:[A-Za-z0-9._~!$&'()*+,;=:@/-]|%[0-9A-Fa-f]{2})*)?(?:\?(?:[A-Za-z0-9._~!$&'()*+,;=:@/?-]|%[0-9A-Fa-f]{2})*)?$"
        )
    )]
    pub probe_path: Option<String>,
    /// Endpoints of the matched processes by port (decimal, `1`–`65535`): a name, whether a
    /// person opens it (`summary`) or it is shown for diagnostics only (`details`), and its own
    /// health path. Replaces (never extends) the other source's object.
    #[serde(
        default,
        deserialize_with = "some_unique_keys",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(extend(
        "propertyNames" = { "pattern": "^[1-9][0-9]{0,4}$" },
        "maxProperties" = 64
    ))]
    pub endpoints: Option<BTreeMap<String, EndpointProfile>>,
    /// How the matched processes' endpoints that `endpoints` does not list are shown: for a
    /// process whose ports change on every start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_endpoints: Option<OtherEndpoints>,
    /// `true` suppresses an entry with this ID from the other source (and this one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(flatten)]
    #[schemars(skip)]
    #[ts(skip)]
    pub unknown: BTreeMap<String, Value>,
}

/// Narrows the project's processes to this service. Every criterion given must hold. A port alone
/// matches only processes of this project: it never attributes another project's process.
/// Unknown criteria are refused rather than ignored, since ignoring one would widen the match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(optional_fields)]
pub struct ServiceMatch {
    /// `/`-separated directory relative to the project root (`.` for the root). The process's
    /// working directory must be it or inside it. No `..`; symbolic links must stay inside the
    /// project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 512))]
    pub cwd: Option<String>,
    /// The process must listen on this port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub port: Option<u16>,
    /// The OS process name, exactly as the PROCESS column shows it (`workerd`, `node`): for a
    /// child whose ports change on every start. Never the command line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 64))]
    pub process: Option<String>,
}

/// One listed endpoint of a service. At least one field is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(optional_fields)]
pub struct EndpointProfile {
    /// Shown with the port: `Site`, `Inspector`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = 100))]
    pub name: Option<String>,
    /// `summary` (the default for a listed endpoint) or `details`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<EndpointShow>,
    /// The health path of this endpoint, on either tier; wins over the service's `probePath`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        length(max = 512),
        regex(
            pattern = r"^/(?:(?:[A-Za-z0-9._~!$&'()*+,;=:@-]|%[0-9A-Fa-f]{2})(?:[A-Za-z0-9._~!$&'()*+,;=:@/-]|%[0-9A-Fa-f]{2})*)?(?:\?(?:[A-Za-z0-9._~!$&'()*+,;=:@/?-]|%[0-9A-Fa-f]{2})*)?$"
        )
    )]
    pub probe_path: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    #[ts(skip)]
    pub unknown: BTreeMap<String, Value>,
}

/// The default for endpoints `endpoints` does not list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct OtherEndpoints {
    pub show: EndpointShow,
    #[serde(flatten)]
    #[schemars(skip)]
    #[ts(skip)]
    pub unknown: BTreeMap<String, Value>,
}

/// Where an endpoint is shown. Display only: it never changes identity, eligibility or binds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum EndpointShow {
    /// On the service row, with Open: what a person opens. The only launch targets.
    #[default]
    Summary,
    /// Visible for diagnostics (the row's muted line, Inspect), never a launch target, and its
    /// HTTP errors never mark the service failed.
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum DeclaredProtocol {
    Http,
    Https,
    Tcp,
}

/// A link shown with the service, opened only on user action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(optional_fields)]
pub struct LinkProfile {
    #[schemars(length(min = 1, max = 100))]
    pub label: String,
    /// `http` or `https` only, without credentials. Loopback and `.localhost` hosts count as
    /// local; any other host is shown as declared and never probed.
    #[schemars(length(max = 2048), regex(pattern = "^[Hh][Tt][Tt][Pp][Ss]?://"))]
    pub url: String,
    /// Defaults to `local` for loopback and `.localhost` hosts, `external` otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<LinkKind>,
    #[serde(flatten)]
    #[schemars(skip)]
    #[ts(skip)]
    pub unknown: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    /// Served on this machine (directly or through a local proxy such as Portless or `LocalCan`).
    Local,
    /// Elsewhere: a staging site, documentation.
    External,
    /// A public tunnel to a local service.
    Tunnel,
}

impl ProjectProfile {
    /// An empty profile of the current version.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: None,
            version: PROFILE_VERSION,
            name: None,
            services: BTreeMap::new(),
            unknown: BTreeMap::new(),
        }
    }

    /// Carries over fields this build does not know from `previous`, the profile this one
    /// replaces, wherever this one does not set them: at the profile level, per service ID, and per
    /// link by URL (so reordering or removing links never moves fields to another link; a link
    /// whose URL changed starts without them). Saving from the dashboard thus never drops what a
    /// newer Kossé or a hand edit added. `$schema` (editor metadata the snapshot never shows, so
    /// the dashboard cannot send it back) is kept the same way.
    pub fn keep_unknown_from(&mut self, previous: &Self) {
        keep_missing(&mut self.unknown, &previous.unknown);
        if self.schema.is_none() {
            self.schema.clone_from(&previous.schema);
        }
        for (id, service) in &mut self.services {
            let Some(old) = previous.services.get(id) else {
                continue;
            };
            keep_missing(&mut service.unknown, &old.unknown);
            if let (Some(endpoints), Some(old_endpoints)) = (&mut service.endpoints, &old.endpoints)
            {
                for (port, endpoint) in endpoints.iter_mut() {
                    if let Some(old_endpoint) = old_endpoints.get(port) {
                        keep_missing(&mut endpoint.unknown, &old_endpoint.unknown);
                    }
                }
            }
            if let (Some(other), Some(old_other)) =
                (&mut service.other_endpoints, &old.other_endpoints)
            {
                keep_missing(&mut other.unknown, &old_other.unknown);
            }
            if let (Some(links), Some(old_links)) = (&mut service.links, &old.links) {
                for link in links.iter_mut() {
                    if let Some(old_link) = old_links.iter().find(|old| old.url == link.url) {
                        keep_missing(&mut link.unknown, &old_link.unknown);
                    }
                }
            }
        }
    }
}

impl Default for ProjectProfile {
    fn default() -> Self {
        Self::new()
    }
}

fn keep_missing(into: &mut BTreeMap<String, Value>, from: &BTreeMap<String, Value>) {
    for (key, value) in from {
        into.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

/// Only the version, read before the rest so that a newer format is refused as newer rather than
/// reported as malformed.
#[derive(Deserialize)]
pub(crate) struct VersionProbe {
    pub version: Option<Value>,
}

/// A JSON object into a map, refusing duplicate keys (which `serde_json` would otherwise resolve
/// silently, last one wins).
pub(crate) fn unique_keys<'de, D, V>(deserializer: D) -> Result<BTreeMap<String, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct UniqueKeys<V>(PhantomData<V>);

    impl<'de, V: Deserialize<'de>> de::Visitor<'de> for UniqueKeys<V> {
        type Value = BTreeMap<String, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object with unique keys")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                if entries.contains_key(&key) {
                    return Err(de::Error::custom(format_args!("duplicate key {key:?}")));
                }
                let value = map.next_value()?;
                entries.insert(key, value);
            }
            Ok(entries)
        }
    }

    deserializer.deserialize_map(UniqueKeys(PhantomData))
}

/// [`unique_keys`] for an optional object.
fn some_unique_keys<'de, D, V>(deserializer: D) -> Result<Option<BTreeMap<String, V>>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    unique_keys(deserializer).map(Some)
}

/// The JSON Schema of [`ProjectProfile`], as `kosse config schema` prints it.
#[must_use]
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(ProjectProfile);
    let mut text = serde_json::to_string_pretty(&schema).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_service_ids_are_refused() {
        let text = r#"{"version":1,"services":{"web":{},"web":{"name":"x"}}}"#;
        let error = serde_json::from_str::<ProjectProfile>(text).expect_err("duplicate");
        assert!(error.to_string().contains("duplicate key"), "{error}");
    }

    #[test]
    fn unknown_fields_survive_a_round_trip_and_a_replacement() {
        let text = r#"{"version":1,"future":{"a":1},"services":{"web":{"name":"Web","hint":2,"links":[{"label":"x","url":"http://localhost:1/","icon":"z"}],"endpoints":{"3000":{"name":"Site","tint":"blue"}},"otherEndpoints":{"show":"details","why":1}}}}"#;
        let previous: ProjectProfile = serde_json::from_str(text).expect("parse");
        let round_trip: Value = serde_json::to_value(&previous).expect("serialize");
        assert_eq!(
            round_trip,
            serde_json::from_str::<Value>(text).expect("value")
        );

        let mut edited = ProjectProfile::new();
        let mut service = previous.services["web"].clone();
        service.unknown.clear();
        if let Some(links) = &mut service.links {
            links[0].unknown.clear();
        }
        if let Some(endpoints) = &mut service.endpoints {
            for endpoint in endpoints.values_mut() {
                endpoint.unknown.clear();
            }
        }
        if let Some(other) = &mut service.other_endpoints {
            other.unknown.clear();
        }
        service.name = Some("Renamed".into());
        edited.services.insert("web".into(), service);
        edited.keep_unknown_from(&previous);
        assert_eq!(edited.unknown["future"], serde_json::json!({"a": 1}));
        assert_eq!(edited.services["web"].unknown["hint"], 2);
        assert_eq!(
            edited.services["web"].links.as_ref().expect("links")[0].unknown["icon"],
            "z"
        );
        let web = &edited.services["web"];
        assert_eq!(
            web.endpoints.as_ref().expect("endpoints")["3000"].unknown["tint"],
            "blue"
        );
        assert_eq!(
            web.other_endpoints.as_ref().expect("other").unknown["why"],
            1
        );
    }

    #[test]
    fn unknown_match_criteria_are_refused() {
        let text = r#"{"version":1,"services":{"web":{"match":{"cwd":".","command":"x"}}}}"#;
        assert!(serde_json::from_str::<ProjectProfile>(text).is_err());
    }
}

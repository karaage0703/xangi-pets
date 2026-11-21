use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceSpec {
    pub url: String,
    pub name: String,
    pub agent_id: String,
    pub directories: BTreeMap<String, String>,
    pub primary: bool,
}

pub fn normalize_url(input: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(input.trim()).map_err(|_| "xangi URL is invalid")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("xangi URL must use http:// or https://".into());
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("invalid xangi URL host or credentials".into());
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// Only explicitly enabled saved instances are restored. A legacy primary
/// without an enabled flag remains active; unused legacy profiles stay off.
pub fn enabled_instances(
    profiles: &[Value],
    primary: Option<(&str, Option<&str>)>,
) -> BTreeMap<String, InstanceSpec> {
    let mut result = BTreeMap::new();
    for profile in profiles {
        let Some(raw) = profile["url"].as_str() else {
            continue;
        };
        let Ok(url) = normalize_url(raw) else {
            continue;
        };
        let is_primary = primary.map(|(base, _)| base == url).unwrap_or(false);
        let legacy_primary = is_primary
            && !profiles.iter().any(|p| {
                p["url"]
                    .as_str()
                    .and_then(|u| normalize_url(u).ok())
                    .as_deref()
                    == Some(&url)
                    && p["enabled"].is_boolean()
            });
        if profile["enabled"] != true && !legacy_primary {
            continue;
        }
        result.entry(url.clone()).or_insert_with(|| {
            let agent_id = profile["agentId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .unwrap_or("xangi:default")
                .to_string();
            let directories = BTreeMap::from([(
                agent_id.clone(),
                profile["spriteDirectory"]
                    .as_str()
                    .unwrap_or("assets/xangi-pets")
                    .to_string(),
            )]);
            InstanceSpec {
                url: url.clone(),
                name: profile["name"].as_str().unwrap_or("xangi").into(),
                agent_id,
                directories,
                primary: is_primary,
            }
        });
    }
    if let Some((url, agent)) = primary {
        // Environment-only bootstrap has no saved profile yet.
        if !profiles.iter().any(|p| {
            p["url"]
                .as_str()
                .and_then(|u| normalize_url(u).ok())
                .as_deref()
                == Some(url)
        }) {
            result.insert(
                url.into(),
                InstanceSpec {
                    url: url.into(),
                    name: "xangi".into(),
                    agent_id: agent.unwrap_or("xangi:default").into(),
                    directories: BTreeMap::new(),
                    primary: true,
                },
            );
        }
    }
    for (url, spec) in &mut result {
        for profile in profiles {
            if profile["url"]
                .as_str()
                .and_then(|u| normalize_url(u).ok())
                .as_deref()
                != Some(url)
            {
                continue;
            }
            if let Some(agent) = profile["agentId"].as_str() {
                let agent = if agent.is_empty() {
                    "xangi:default"
                } else {
                    agent
                };
                spec.directories.entry(agent.into()).or_insert_with(|| {
                    profile["spriteDirectory"]
                        .as_str()
                        .unwrap_or("assets/xangi-pets")
                        .into()
                });
            }
        }
    }
    result
}

pub fn pet_key(url: &str, session: &str, persistent: bool) -> String {
    serde_json::to_string(&(url, persistent, session)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn enabled_profile_owns_its_default_agent_sprite_directory() {
        let profiles = vec![
            json!({"url":"http://a","enabled":true,"agentId":"","spriteDirectory":"assets/chosen"}),
            json!({"url":"http://a","enabled":false,"agentId":"","spriteDirectory":"assets/old"}),
        ];
        let instances = enabled_instances(&profiles, None);
        assert_eq!(
            instances["http://a"].directories["xangi:default"],
            "assets/chosen"
        );
    }
    #[test]
    fn enabled_sources_are_independent_and_urls_are_deduplicated() {
        let mut profiles = vec![
            json!({"url":"http://a/","name":"A","enabled":true}),
            json!({"url":"http://a","enabled":true}),
            json!({"url":"http://b","name":"B","enabled":true}),
        ];
        assert_eq!(
            enabled_instances(&profiles, Some(("http://a", None))).len(),
            2
        );
        profiles[0]["enabled"] = json!(false);
        profiles[1]["enabled"] = json!(false);
        let enabled = enabled_instances(&profiles, Some(("http://a", None)));
        assert_eq!(enabled.len(), 1);
        assert!(enabled.contains_key("http://b"));
        assert_ne!(
            pet_key("http://a", "same", false),
            pet_key("http://b", "same", false)
        );
        assert_ne!(
            pet_key("http://a", "", true),
            pet_key("http://a", "", false)
        );
    }
    #[test]
    fn legacy_primary_is_preserved_without_enabling_unused_instances() {
        let profiles = vec![json!({"url":"http://a"}), json!({"url":"http://b"})];
        assert!(enabled_instances(&profiles, None).is_empty());
        let active = enabled_instances(&profiles, Some(("http://a", None)));
        assert_eq!(active.len(), 1);
        assert!(active["http://a"].primary);
        assert_eq!(
            enabled_instances(
                &[json!({"url":"http://a","enabled":false})],
                Some(("http://a", None))
            )
            .len(),
            0
        );
    }
}

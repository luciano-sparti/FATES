use crate::error::Error;
use crate::health::{HealthCheckConfig, RestartPolicyConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Conditional(HashMap<String, String>),
}

impl DependencySpec {
    pub fn name(&self) -> String {
        match self {
            DependencySpec::Simple(s) => s.clone(),
            DependencySpec::Conditional(map) => map.keys().next().cloned().unwrap_or_default(),
        }
    }

    pub fn condition(&self) -> Option<String> {
        match self {
            DependencySpec::Simple(_) => None,
            DependencySpec::Conditional(map) => map.values().next().cloned(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Config {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default, alias = "services")]
    pub groups: HashMap<String, GroupConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GroupConfig {
    #[serde(alias = "command")]
    pub cmd: String,
    pub cwd: Option<String>,
    #[serde(default, alias = "depends_on")]
    pub depends: Vec<DependencySpec>,
    /// Environment variables set for this group's process. Also consulted when
    /// expanding `$VAR` in `cmd` and `cwd`.
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub healthcheck: Option<HealthCheckConfig>,
    #[serde(default)]
    pub restart_policy: Option<RestartPolicyConfig>,
    #[serde(default)]
    pub stop_grace_period_ms: Option<u64>,
}

impl GroupConfig {
    pub fn depends_names(&self) -> Vec<String> {
        self.depends
            .iter()
            .map(|d| d.name())
            .filter(|n| !n.is_empty())
            .collect()
    }

    pub fn requires_healthy(&self, dep_name: &str) -> bool {
        self.depends.iter().any(|d| {
            d.name() == dep_name
                && d.condition()
                    .map(|cond| cond.eq_ignore_ascii_case("healthy"))
                    .unwrap_or(false)
        })
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Config {
                version: None,
                groups: HashMap::new(),
            });
        }
        let content = fs::read_to_string(path).map_err(|e| {
            Error::command(format!("Failed to load config '{}': {}", path.display(), e))
        })?;
        let config: Config = serde_yaml::from_str(&content).map_err(|e| {
            Error::command(format!(
                "Failed to parse config '{}': {}",
                path.display(),
                e
            ))
        })?;
        Ok(config)
    }

    /// Validate the config and return a list of human-readable errors.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        for (name, group) in &self.groups {
            // Check cmd is not empty
            if group.cmd.trim().is_empty() {
                errors.push(format!("Group '{}': 'cmd' must not be empty.", name));
            }

            let dep_names = group.depends_names();

            // Check all dependencies exist
            for dep in &dep_names {
                if !self.groups.contains_key(dep) {
                    errors.push(format!(
                        "Group '{}': dependency '{}' is not defined in config.",
                        name, dep
                    ));
                }
            }

            // Check for self-dependency
            if dep_names.contains(name) {
                errors.push(format!("Group '{}': depends on itself.", name));
            }
        }

        errors
    }

    /// Order the given group names so that dependencies come before their
    /// dependents (dependency-first). Names absent from the config are treated
    /// as independent. If the dependency graph contains a cycle, the leftover
    /// names are appended at the end in their original order.
    pub fn topological_order(&self, names: &[String]) -> Vec<String> {
        use std::collections::{HashMap, HashSet, VecDeque};

        let present: HashSet<String> = names.iter().cloned().collect();
        let mut indegree: HashMap<String, usize> = names.iter().cloned().map(|n| (n, 0)).collect();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

        for name in names {
            if let Some(group) = self.groups.get(name) {
                for dep in group.depends_names() {
                    if present.contains(&dep) && dep != *name {
                        *indegree.get_mut(name).unwrap() += 1;
                        dependents.entry(dep).or_default().push(name.clone());
                    }
                }
            }
        }

        let mut ready: VecDeque<String> = indegree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(n, _)| n.clone())
            .collect();

        let mut order = Vec::new();
        while let Some(name) = ready.pop_front() {
            order.push(name.clone());
            if let Some(ds) = dependents.get(&name) {
                for d in ds {
                    let deg = indegree.get_mut(d).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        ready.push_back(d.clone());
                    }
                }
            }
        }

        for name in names {
            if !order.contains(name) {
                order.push(name.clone());
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(groups: &[(&str, &str, &[&str])]) -> Config {
        let mut map = HashMap::new();
        for (name, cmd, depends) in groups {
            map.insert(
                name.to_string(),
                GroupConfig {
                    cmd: cmd.to_string(),
                    cwd: None,
                    depends: depends
                        .iter()
                        .map(|s| DependencySpec::Simple(s.to_string()))
                        .collect(),
                    env: HashMap::new(),
                    healthcheck: None,
                    restart_policy: None,
                    stop_grace_period_ms: None,
                },
            );
        }
        Config {
            version: None,
            groups: map,
        }
    }

    #[test]
    fn valid_config_has_no_errors() {
        let cfg = make_config(&[
            ("db", "postgres -D ~/data", &[]),
            ("web", "npm run dev", &["db"]),
            ("api", "uvicorn main:app", &["db"]),
        ]);
        assert!(
            cfg.validate().is_empty(),
            "Expected no errors for a valid config"
        );
    }

    #[test]
    fn empty_cmd_is_an_error() {
        let cfg = make_config(&[("broken", "   ", &[])]);
        let errors = cfg.validate();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("'cmd' must not be empty"));
    }

    #[test]
    fn missing_dependency_is_an_error() {
        let cfg = make_config(&[("web", "npm run dev", &["db"])]);
        let errors = cfg.validate();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("dependency 'db' is not defined"));
    }

    #[test]
    fn self_dependency_is_an_error() {
        let cfg = make_config(&[("web", "npm run dev", &["web"])]);
        let errors = cfg.validate();
        let self_dep_err = errors.iter().any(|e| e.contains("depends on itself"));
        assert!(
            self_dep_err,
            "Expected self-dependency error, got: {:?}",
            errors
        );
    }

    #[test]
    fn multiple_errors_are_all_reported() {
        let cfg = make_config(&[
            ("broken_cmd", "", &[]),
            ("missing_dep", "some-cmd", &["nonexistent"]),
        ]);
        let errors = cfg.validate();
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got: {:?}",
            errors
        );
    }

    #[test]
    fn topological_order_puts_dependencies_first() {
        let cfg = make_config(&[
            ("db", "postgres", &[]),
            ("api", "uvicorn", &["db"]),
            ("web", "npm", &["api"]),
        ]);
        let names = vec!["web".into(), "api".into(), "db".into()];
        let order = cfg.topological_order(&names);
        let pos = |n: &str| order.iter().position(|x| x == n).unwrap();
        assert!(pos("db") < pos("api"), "order: {:?}", order);
        assert!(pos("api") < pos("web"), "order: {:?}", order);
    }

    #[test]
    fn topological_order_keeps_every_name() {
        let cfg = make_config(&[("db", "postgres", &[]), ("web", "npm", &["db"])]);
        let names = vec!["web".into(), "db".into(), "extra".into()];
        let mut order = cfg.topological_order(&names);
        order.sort();
        let mut expected = names.clone();
        expected.sort();
        assert_eq!(order, expected);
    }

    #[test]
    fn topological_order_survives_cycles() {
        let cfg = make_config(&[("a", "x", &["b"]), ("b", "y", &["a"])]);
        let names = vec!["a".into(), "b".into()];
        let mut order = cfg.topological_order(&names);
        order.sort();
        assert_eq!(order, names);
    }

    #[test]
    fn parses_v2_fates_yaml_with_services_and_healthcheck() {
        let yaml = r#"
version: "2"
services:
  database:
    command: "postgres -D /data"
    healthcheck:
      tcp: "127.0.0.1:5432"
      interval_ms: 100
      retries: 3
    restart_policy:
      condition: on_failure
      max_retries: 3
  api:
    command: "uvicorn app:main"
    depends_on:
      - database: healthy
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.version.as_deref(), Some("2"));
        assert!(cfg.groups.contains_key("database"));
        assert!(cfg.groups.contains_key("api"));

        let db = &cfg.groups["database"];
        assert_eq!(db.cmd, "postgres -D /data");
        assert!(db.healthcheck.is_some());
        assert_eq!(
            db.healthcheck.as_ref().unwrap().tcp.as_deref(),
            Some("127.0.0.1:5432")
        );

        let api = &cfg.groups["api"];
        assert_eq!(api.depends_names(), vec!["database"]);
        assert!(api.requires_healthy("database"));
    }
}

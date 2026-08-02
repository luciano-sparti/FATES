use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub groups: HashMap<String, GroupConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GroupConfig {
    pub cmd: String,
    pub cwd: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    /// Environment variables set for this group's process. Also consulted when
    /// expanding `$VAR` in `cmd` and `cwd`.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Config {
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

            // Check all dependencies exist
            for dep in &group.depends {
                if !self.groups.contains_key(dep) {
                    errors.push(format!(
                        "Group '{}': dependency '{}' is not defined in config.",
                        name, dep
                    ));
                }
            }

            // Check for self-dependency
            if group.depends.contains(name) {
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
                for dep in &group.depends {
                    if present.contains(dep) && dep != name {
                        *indegree.get_mut(name).unwrap() += 1;
                        dependents
                            .entry(dep.clone())
                            .or_default()
                            .push(name.clone());
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
                    depends: depends.iter().map(|s| s.to_string()).collect(),
                    env: HashMap::new(),
                },
            );
        }
        Config { groups: map }
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
        // self-dep triggers both the missing-dep check AND the self-dep check
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
}

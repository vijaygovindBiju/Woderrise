use crate::{Manifest, Result, WoderriseError};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub order: Vec<String>,
}

/// Deterministic dependency resolver. Constraints are checked for exact versions
/// and simple >=/<=/>/< ranges; cycles are rejected with a useful trace.
pub fn resolve(manifests: &HashMap<String, Manifest>, roots: &[String]) -> Result<Resolution> {
    let mut state = HashMap::<String, u8>::new();
    let mut stack = Vec::new();
    let mut order = Vec::new();
    fn visit(
        id: &str,
        all: &HashMap<String, Manifest>,
        state: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        match state.get(id).copied() {
            Some(2) => return Ok(()),
            Some(1) => {
                let at = stack.iter().position(|x| x == id).unwrap_or(0);
                return Err(WoderriseError::Validation(format!(
                    "dependency cycle: {}",
                    stack[at..].join(" -> ")
                )));
            }
            _ => {}
        }
        let m = all
            .get(id)
            .ok_or_else(|| WoderriseError::Validation(format!("missing dependency {id}")))?;
        state.insert(id.to_string(), 1);
        stack.push(id.to_string());
        if let Some(deps) = &m.dependencies {
            for dep in &deps.packages {
                let child = all.get(&dep.id).ok_or_else(|| {
                    WoderriseError::Validation(format!("missing dependency {}", dep.id))
                })?;
                if !version_satisfies(&child.package.version, &dep.version) {
                    return Err(WoderriseError::Validation(format!(
                        "{} requires {} {}, found {}",
                        id, dep.id, dep.version, child.package.version
                    )));
                }
                visit(&dep.id, all, state, stack, order)?;
            }
        }
        stack.pop();
        state.insert(id.to_string(), 2);
        order.push(id.to_string());
        Ok(())
    }
    let roots: Vec<_> = if roots.is_empty() {
        manifests.keys().cloned().collect()
    } else {
        roots.to_vec()
    };
    for id in roots {
        visit(&id, manifests, &mut state, &mut stack, &mut order)?;
    }
    Ok(Resolution { order })
}

pub fn version_satisfies(version: &str, constraint: &str) -> bool {
    if constraint.trim().is_empty() || constraint == "*" {
        return true;
    }
    let parse = |s: &str| {
        s.trim_start_matches('v')
            .split('.')
            .map(|x| x.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let cmp = |a: &str, b: &str| parse(a).cmp(&parse(b));
    constraint.split(',').all(|part| {
        let p = part.trim();
        for op in [">=", "<=", "=", ">", "<"] {
            if let Some(v) = p.strip_prefix(op) {
                let c = cmp(version, v);
                return match op {
                    ">=" => c.is_ge(),
                    "<=" => c.is_le(),
                    "=" => c.is_eq(),
                    ">" => c.is_gt(),
                    "<" => c.is_lt(),
                    _ => false,
                };
            }
        }
        cmp(version, p).is_eq()
    })
}

pub fn dependency_ids(manifest: &Manifest) -> HashSet<String> {
    manifest
        .dependencies
        .as_ref()
        .map(|d| d.packages.iter().map(|p| p.id.clone()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_cycle() {
        let yaml = |id: &str, dep: &str| {
            format!("schema_version: '1.0.0'\npackage:\n  id: {id}\n  name: x\n  version: '1.0.0'\n  type: config\n  category: x\n  license: MIT\n  description: x\ndependencies:\n  packages:\n    - id: {dep}\n      version: '>=1.0.0'\ninstall: {{ files: [] }}")
        };
        let mut all = HashMap::new();
        all.insert(
            "org.test.a".into(),
            Manifest::parse(&yaml("org.test.a", "org.test.b")).unwrap(),
        );
        all.insert(
            "org.test.b".into(),
            Manifest::parse(&yaml("org.test.b", "org.test.a")).unwrap(),
        );
        assert!(resolve(&all, &["org.test.a".into()]).is_err());
    }
}

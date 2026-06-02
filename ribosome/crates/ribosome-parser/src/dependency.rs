use crate::version::{Version, VersionConstraint};

/// Parsed dependency entry from mRNA `depends` lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencySpec {
    pub name: String,
    pub constraint: Option<VersionConstraint>,
}

/// Parse a dependency string such as `glibc >= 2.39` or `gmp`.
pub fn parse_dependency_spec(input: &str) -> Result<DependencySpec, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("empty dependency string".to_string());
    }

    type ConstraintFactory = fn(Version) -> VersionConstraint;

    const OPS: &[(&str, ConstraintFactory)] = &[
        (">=", |v| VersionConstraint::GreaterOrEqual(v)),
        ("<", |v| VersionConstraint::LessThan(v)),
        (">", |v| VersionConstraint::GreaterThan(v)),
        ("=", |v| VersionConstraint::Equal(v)),
    ];

    for (op, mk) in OPS {
        if let Some(idx) = input.find(op) {
            let name_part = input[..idx].trim();
            let ver_part = input[idx + op.len()..].trim();
            if name_part.is_empty() || ver_part.is_empty() {
                return Err(format!("invalid dependency format: {input}"));
            }
            if !is_valid_package_name(name_part) {
                return Err(format!("invalid package name: {name_part}"));
            }
            let version = Version::parse(ver_part).map_err(|e| e.to_string())?;
            return Ok(DependencySpec {
                name: name_part.to_string(),
                constraint: Some(mk(version)),
            });
        }
    }

    if !is_valid_package_name(input) {
        return Err(format!("invalid package name: {input}"));
    }

    Ok(DependencySpec {
        name: input.to_string(),
        constraint: None,
    })
}

pub fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut parts = name.split('-');
    let first = match parts.next() {
        Some(p) => p,
        None => return false,
    };
    if first.is_empty()
        || !first
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return false;
    }
    for part in parts {
        if part.is_empty()
            || !part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_constraint() {
        let d = parse_dependency_spec("gmp").unwrap();
        assert_eq!(d.name, "gmp");
        assert!(d.constraint.is_none());
    }

    #[test]
    fn parse_greater_equal() {
        let d = parse_dependency_spec("glibc >= 2.39").unwrap();
        assert_eq!(d.name, "glibc");
        assert!(d.constraint.is_some());
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_dependency_spec("").is_err());
        assert!(parse_dependency_spec("bad name >= 1").is_err());
    }
}

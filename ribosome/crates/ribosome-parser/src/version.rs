use std::cmp::Ordering;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub rest: String,
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    GreaterOrEqual(Version),
    LessThan(Version),
    Equal(Version),
    GreaterThan(Version),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VersionParseError {
    #[error("empty version string")]
    Empty,
    #[error("invalid version component: {0}")]
    InvalidComponent(String),
}

impl Version {
    pub fn parse(input: &str) -> Result<Self, VersionParseError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(VersionParseError::Empty);
        }

        let (core, rest) = match input.split_once('-') {
            Some((c, r)) => (c, format!("-{r}")),
            None => (input, String::new()),
        };

        let mut parts = core.split('.');
        let major = parse_component(parts.next().unwrap_or(""))?;
        let minor = parse_component(parts.next().unwrap_or("0"))?;
        let patch = parse_component(parts.next().unwrap_or("0"))?;

        let extra = parts.collect::<Vec<_>>().join(".");
        let rest = if extra.is_empty() {
            rest
        } else if rest.is_empty() {
            format!(".{extra}")
        } else {
            format!("{rest}.{extra}")
        };

        Ok(Self {
            major,
            minor,
            patch,
            rest,
        })
    }

    pub fn satisfies(&self, constraint: &VersionConstraint) -> bool {
        match constraint {
            VersionConstraint::GreaterOrEqual(v) => self >= v,
            VersionConstraint::LessThan(v) => self < v,
            VersionConstraint::Equal(v) => self == v,
            VersionConstraint::GreaterThan(v) => self > v,
        }
    }
}

fn parse_component(s: &str) -> Result<u64, VersionParseError> {
    if s.is_empty() {
        return Ok(0);
    }
    s.parse::<u64>()
        .map_err(|_| VersionParseError::InvalidComponent(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_version() {
        let v = Version::parse("14.2.0").unwrap();
        assert_eq!(v.major, 14);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn parse_partial_version() {
        let v = Version::parse("2.39").unwrap();
        assert_eq!(v.minor, 39);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn compare_versions() {
        let a = Version::parse("2.39").unwrap();
        let b = Version::parse("2.40").unwrap();
        assert!(a < b);
    }

    #[test]
    fn satisfies_greater_equal() {
        let v = Version::parse("2.40").unwrap();
        let c = VersionConstraint::GreaterOrEqual(Version::parse("2.39").unwrap());
        assert!(v.satisfies(&c));
    }
}

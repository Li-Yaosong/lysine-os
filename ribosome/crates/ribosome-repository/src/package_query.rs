use crate::index::{IndexEntry, RepositoryIndex};

/// High-level package query interface over a repository index.
pub struct PackageQuery<'a> {
    index: &'a RepositoryIndex,
}

impl<'a> PackageQuery<'a> {
    pub fn new(index: &'a RepositoryIndex) -> Self {
        Self { index }
    }

    /// Get detailed info for a package by name.
    pub fn info(&self, name: &str) -> Option<PackageInfo<'a>> {
        self.index.find(name).map(PackageInfo::from_entry)
    }

    /// List all packages in the index.
    pub fn list_all(&self) -> Vec<PackageInfo<'a>> {
        self.index
            .list_all()
            .into_iter()
            .map(PackageInfo::from_entry)
            .collect()
    }

    /// Search packages by keyword (matches name and description).
    pub fn search(&self, keyword: &str) -> Vec<PackageInfo<'a>> {
        self.index
            .search(keyword)
            .into_iter()
            .map(PackageInfo::from_entry)
            .collect()
    }

    /// Check if all runtime dependencies of the given packages are satisfied
    /// by the available packages in the index.
    pub fn check_dependencies(&self, names: &[&str]) -> Vec<DependencyIssue> {
        let mut issues = Vec::new();
        for name in names {
            if let Some(entry) = self.index.find(name) {
                for dep in &entry.depends.runtime {
                    let dep_name = dep.split_once(' ').map_or(dep.as_str(), |(n, _)| n);
                    if self.index.find(dep_name).is_none() {
                        issues.push(DependencyIssue {
                            package: name.to_string(),
                            missing_dependency: dep_name.to_string(),
                            constraint: Some(dep.clone()),
                        });
                    }
                }
            }
        }
        issues
    }
}

/// Structured package info for display.
#[derive(Debug)]
pub struct PackageInfo<'a> {
    pub entry: &'a IndexEntry,
}

impl<'a> PackageInfo<'a> {
    fn from_entry(entry: &'a IndexEntry) -> Self {
        Self { entry }
    }

    /// Format as a human-readable summary.
    pub fn to_summary(&self) -> String {
        let e = &self.entry;
        format!(
            "{name} {version}-{release}\n  Description: {desc}\n  License: {license}\n  Architecture: {arch}\n  Category: {cat}\n  Filename: {filename}\n  SHA-256: {sha256}\n  Size: {size} bytes\n  Built: {date}",
            name = e.name,
            version = e.version,
            release = e.release,
            desc = if e.description.is_empty() { "(none)" } else { &e.description },
            license = if e.license.is_empty() { "(unknown)" } else { &e.license },
            arch = e.arch,
            cat = e.category,
            filename = e.filename,
            sha256 = &e.sha256[..e.sha256.len().min(22)],
            size = e.installed_size,
            date = if e.build_date.is_empty() { "(unknown)" } else { &e.build_date },
        )
    }
}

/// A missing dependency issue.
#[derive(Debug)]
pub struct DependencyIssue {
    pub package: String,
    pub missing_dependency: String,
    pub constraint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexDepends;

    fn make_index() -> RepositoryIndex {
        let mut index = RepositoryIndex::default();
        index.add_entry(IndexEntry {
            name: "bash".to_string(),
            version: "5.2.37".to_string(),
            release: 1,
            description: "GNU Bourne Again Shell".to_string(),
            license: "GPL-3.0-or-later".to_string(),
            arch: "x86_64".to_string(),
            category: "core".to_string(),
            filename: "core/bash-5.2.37-1-x86_64.prot".to_string(),
            sha256: "sha256:abc123".to_string(),
            depends: IndexDepends {
                runtime: vec!["glibc >= 2.39".to_string(), "ncurses".to_string()],
                build: vec![],
            },
            provides: vec![],
            conflicts: vec![],
            installed_size: 1024,
            build_date: String::new(),
        });
        index.add_entry(IndexEntry {
            name: "glibc".to_string(),
            version: "2.39.4".to_string(),
            release: 1,
            description: "GNU C Library".to_string(),
            license: "LGPL-2.1".to_string(),
            arch: "x86_64".to_string(),
            category: "core".to_string(),
            filename: "core/glibc-2.39.4-1-x86_64.prot".to_string(),
            sha256: "sha256:def456".to_string(),
            depends: IndexDepends::default(),
            provides: vec![],
            conflicts: vec![],
            installed_size: 4096,
            build_date: String::new(),
        });
        index
    }

    #[test]
    fn query_info_found() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        let info = query.info("bash").unwrap();
        assert_eq!(info.entry.version, "5.2.37");
        assert_eq!(info.entry.depends.runtime.len(), 2);
    }

    #[test]
    fn query_info_not_found() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        assert!(query.info("nonexistent").is_none());
    }

    #[test]
    fn query_list_all() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        let all = query.list_all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn query_search() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        let results = query.search("gnu");
        assert_eq!(results.len(), 2); // both bash and glibc have "GNU" in description
    }

    #[test]
    fn query_check_deps_missing() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        // bash depends on ncurses (not in index) and glibc (in index)
        let issues = query.check_dependencies(&["bash"]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].missing_dependency, "ncurses");
    }

    #[test]
    fn query_check_deps_satisfied() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        let issues = query.check_dependencies(&["glibc"]);
        assert!(issues.is_empty());
    }

    #[test]
    fn info_to_summary() {
        let index = make_index();
        let query = PackageQuery::new(&index);
        let info = query.info("bash").unwrap();
        let summary = info.to_summary();
        assert!(summary.contains("bash 5.2.37-1"));
        assert!(summary.contains("GPL-3.0-or-later"));
    }
}
